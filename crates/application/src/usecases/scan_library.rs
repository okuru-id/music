use std::time::Duration;

use futures::StreamExt;

use domain::{FilePath, Track, TrackId};

use crate::ports::events::{DomainEvent, EventPublisher};
use crate::ports::metadata::{MetaError, MetadataReader};
use crate::ports::repository::{RepoError, TrackRepository};
use crate::ports::scanner::{FileScanner, ScannedFile};

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ScanError {
    #[error(transparent)]
    Repo(#[from] RepoError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScanSummary {
    pub scanned: u64,
    pub added: u64,
    pub skipped: u64,
    pub failed: u64,
}

const BATCH: usize = 200;

/// Incremental library scan: walk roots, skip unchanged files (mtime),
/// read metadata, resolve albums, upsert in batches, publish progress.
pub struct ScanLibrary<R, M, S, E> {
    repo: R,
    metadata: M,
    scanner: S,
    events: E,
}

impl<R, M, S, E> ScanLibrary<R, M, S, E>
where
    R: TrackRepository,
    M: MetadataReader,
    S: FileScanner,
    E: EventPublisher,
{
    pub fn new(repo: R, metadata: M, scanner: S, events: E) -> Self {
        Self {
            repo,
            metadata,
            scanner,
            events,
        }
    }

    pub async fn execute(&self, roots: Vec<FilePath>) -> Result<ScanSummary, ScanError> {
        let mut summary = ScanSummary::default();
        let mut batch: Vec<Track> = Vec::new();

        let files = self.scanner.scan(&roots);
        futures::pin_mut!(files);
        while let Some(ScannedFile { path, mtime_secs }) = files.next().await {
            summary.scanned += 1;

            // Incremental: skip if present and unmodified.
            if self.repo.find_mtime_by_path(&path.0).await? == Some(mtime_secs) {
                summary.skipped += 1;
                continue;
            }

            match self.metadata.read(&path) {
                Ok(meta) => {
                    let duration = meta.duration.unwrap_or(Duration::ZERO);
                    let album_id = match (meta.album.as_deref(), meta.album_artist.as_deref()) {
                        (Some(album), artist) => Some(self.repo.upsert_album(album, artist).await?),
                        (None, _) => None,
                    };
                    match Track::new(
                        TrackId(0), // assigned by the database
                        meta.title.unwrap_or_else(|| fallback_title(&path)),
                        meta.artist.unwrap_or_else(|| "Unknown Artist".into()),
                        album_id,
                        duration,
                        path.clone(),
                        meta.track_no,
                        mtime_secs,
                    ) {
                        Ok(track) => {
                            batch.push(track);
                            summary.added += 1;
                        }
                        // Zero-duration or otherwise invalid: count and move on.
                        Err(_) => summary.failed += 1,
                    }
                }
                // Unreadable file: don't abort the scan.
                Err(MetaError::Read(..)) => summary.failed += 1,
            }

            if batch.len() >= BATCH {
                self.repo.upsert_many(&batch).await?;
                batch.clear();
                self.events.publish(DomainEvent::ScanProgress {
                    scanned: summary.scanned,
                    added: summary.added,
                });
            }
        }

        if !batch.is_empty() {
            self.repo.upsert_many(&batch).await?;
        }
        self.events.publish(DomainEvent::ScanFinished {
            scanned: summary.scanned,
            added: summary.added,
            skipped: summary.skipped,
            failed: summary.failed,
        });
        Ok(summary)
    }
}

fn fallback_title(path: &FilePath) -> String {
    path.0
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(&path.0)
        .trim_end_matches(|c: char| !c.is_alphanumeric() && c != ' ')
        .to_string()
        .trim_end_matches('.')
        .to_string()
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use domain::{AlbumId, Track};
    use futures::executor::block_on;
    use futures::stream::{self, BoxStream, StreamExt};

    use super::*;
    use crate::ports::metadata::TrackMetadata;
    use crate::ports::repository::{Page, SearchQuery};
    use crate::ports::scanner::ScannedFile;

    struct MockRepo {
        mtimes: Mutex<std::collections::HashMap<String, u64>>,
        next_album: std::sync::atomic::AtomicU64,
    }

    impl MockRepo {
        fn new() -> Self {
            Self {
                mtimes: Mutex::new(std::collections::HashMap::new()),
                next_album: std::sync::atomic::AtomicU64::new(0),
            }
        }
    }

    #[async_trait]
    impl TrackRepository for MockRepo {
        async fn upsert_many(&self, tracks: &[Track]) -> Result<(), RepoError> {
            let mut m = self.mtimes.lock().unwrap();
            for t in tracks {
                m.insert(t.path.0.clone(), t.mtime_secs);
            }
            Ok(())
        }
        async fn find_by_id(&self, _: TrackId) -> Result<Option<Track>, RepoError> {
            Ok(None)
        }
        async fn search(&self, _: &SearchQuery) -> Result<Page<Track>, RepoError> {
            unimplemented!()
        }
        async fn find_mtime_by_path(&self, path: &str) -> Result<Option<u64>, RepoError> {
            Ok(self.mtimes.lock().unwrap().get(path).copied())
        }
        async fn upsert_album(&self, _: &str, _: Option<&str>) -> Result<AlbumId, RepoError> {
            let n = self
                .next_album
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(AlbumId(n + 1))
        }
    }

    struct MockMeta;

    impl MetadataReader for MockMeta {
        fn read(&self, path: &FilePath) -> Result<TrackMetadata, MetaError> {
            if path.0.ends_with("bad.flac") {
                return Err(MetaError::Read(path.0.clone(), "corrupt".into()));
            }
            Ok(TrackMetadata {
                title: Some("Song".into()),
                artist: Some("Artist".into()),
                album: Some("Album".into()),
                duration: Some(Duration::from_secs(180)),
                ..Default::default()
            })
        }
    }

    struct MockScanner(Vec<ScannedFile>);

    impl FileScanner for MockScanner {
        fn scan(&self, _: &[FilePath]) -> BoxStream<'static, ScannedFile> {
            stream::iter(self.0.clone()).boxed()
        }
    }

    #[derive(Default)]
    struct MockEvents(Mutex<Vec<DomainEvent>>);

    impl EventPublisher for MockEvents {
        fn publish(&self, e: DomainEvent) {
            self.0.lock().unwrap().push(e);
        }
    }

    fn file(p: &str, mtime: u64) -> ScannedFile {
        ScannedFile {
            path: FilePath(p.into()),
            mtime_secs: mtime,
        }
    }

    #[test]
    fn scans_adds_and_finishes() {
        let uc = ScanLibrary::new(
            MockRepo::new(),
            MockMeta,
            MockScanner(vec![file("/m/a.mp3", 1), file("/m/bad.flac", 1)]),
            MockEvents::default(),
        );
        let summary = block_on(uc.execute(vec![FilePath("/m".into())])).unwrap();
        assert_eq!(
            summary,
            ScanSummary {
                scanned: 2,
                added: 1,
                skipped: 0,
                failed: 1
            }
        );
        let events = uc.events.0.lock().unwrap();
        assert!(matches!(
            events.last(),
            Some(DomainEvent::ScanFinished {
                added: 1,
                failed: 1,
                ..
            })
        ));
    }

    #[test]
    fn skips_unchanged_by_mtime() {
        let repo = MockRepo::new();
        repo.mtimes.lock().unwrap().insert("/m/a.mp3".into(), 1);
        let uc = ScanLibrary::new(
            repo,
            MockMeta,
            MockScanner(vec![file("/m/a.mp3", 1), file("/m/a.mp3", 2)]),
            MockEvents::default(),
        );
        let summary = block_on(uc.execute(vec![FilePath("/m".into())])).unwrap();
        // same mtime skipped; changed mtime re-added
        assert_eq!(
            summary,
            ScanSummary {
                scanned: 2,
                added: 1,
                skipped: 1,
                failed: 0
            }
        );
    }
}
