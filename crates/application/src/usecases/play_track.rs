use std::time::Duration;

use domain::TrackId;

use crate::dto::{PlaybackStateDto, TrackDto};
use crate::ports::audio::{AudioEngine, AudioError};
use crate::ports::events::{DomainEvent, EventPublisher};
use crate::ports::media::{MediaControls, NowPlaying};
use crate::ports::repository::{RepoError, TrackRepository};

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PlayTrackError {
    #[error("track {0} not found")]
    NotFound(TrackId),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Audio(#[from] AudioError),
}

/// Load, play, notify media controls and the UI.
/// Ports are injected; this use case is testable with mocks, no Tauri, no I/O.
pub struct PlayTrack<R, A, M, E> {
    repo: R,
    audio: A,
    media: M,
    events: E,
}

impl<R, A, M, E> PlayTrack<R, A, M, E>
where
    R: TrackRepository,
    A: AudioEngine,
    M: MediaControls,
    E: EventPublisher,
{
    pub fn new(repo: R, audio: A, media: M, events: E) -> Self {
        Self {
            repo,
            audio,
            media,
            events,
        }
    }

    pub async fn execute(&self, id: TrackId) -> Result<TrackDto, PlayTrackError> {
        let track = self
            .repo
            .find_by_id(id)
            .await?
            .ok_or(PlayTrackError::NotFound(id))?;

        self.audio.load(&track.path)?;
        self.audio.play();

        self.media.update(&NowPlaying {
            title: Some(track.title.clone()),
            artist: Some(track.artist.clone()),
            album: None,
            duration: Some(track.duration),
            position: Some(Duration::ZERO),
            paused: false,
        });

        self.events
            .publish(DomainEvent::TrackChanged((&track).into()));
        self.events
            .publish(DomainEvent::PlaybackStateChanged(PlaybackStateDto {
                status: "playing".into(),
                position_secs: 0,
                volume: 1.0,
            }));

        Ok((&track).into())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};

    use async_trait::async_trait;
    use futures::executor::block_on;

    use super::*;
    use crate::ports::repository::{Page, SearchQuery};
    use domain::{FilePath, Volume};

    // ---- mocks (hand-rolled; swap for mockall when they multiply) ----

    struct MockRepo(Option<domain::Track>);

    #[async_trait]
    impl TrackRepository for MockRepo {
        async fn upsert_many(&self, _: &[domain::Track]) -> Result<(), RepoError> {
            Ok(())
        }
        async fn find_by_id(&self, _: TrackId) -> Result<Option<domain::Track>, RepoError> {
            Ok(self.0.clone())
        }
        async fn search(&self, _: &SearchQuery) -> Result<Page<domain::Track>, RepoError> {
            unimplemented!()
        }
        async fn find_mtime_by_path(&self, _: &str) -> Result<Option<u64>, RepoError> {
            Ok(None)
        }
        async fn upsert_album(
            &self,
            _: &str,
            _: Option<&str>,
        ) -> Result<domain::AlbumId, RepoError> {
            Ok(domain::AlbumId(1))
        }
    }

    #[derive(Default)]
    struct MockAudio {
        loaded: Mutex<Vec<String>>,
        played: AtomicBool,
        volumes: Mutex<Vec<f32>>,
    }

    impl AudioEngine for MockAudio {
        fn load(&self, path: &FilePath) -> Result<(), AudioError> {
            self.loaded.lock().unwrap().push(path.0.clone());
            Ok(())
        }
        fn play(&self) {
            self.played.store(true, Ordering::SeqCst);
        }
        fn pause(&self) {}
        fn seek(&self, _: Duration) -> Result<(), AudioError> {
            Ok(())
        }
        fn set_volume(&self, v: Volume) {
            self.volumes.lock().unwrap().push(v.inner());
        }
        fn enqueue_next(&self, _: &FilePath) {}
        fn position(&self) -> Duration {
            Duration::ZERO
        }
        fn is_playing(&self) -> bool {
            self.played.load(Ordering::SeqCst)
        }
    }

    #[derive(Default)]
    struct MockMedia(Mutex<Vec<NowPlaying>>);

    impl MediaControls for MockMedia {
        fn update(&self, now: &NowPlaying) {
            self.0.lock().unwrap().push(now.clone());
        }
    }

    #[derive(Default)]
    struct MockEvents(Mutex<Vec<DomainEvent>>);

    impl EventPublisher for MockEvents {
        fn publish(&self, e: DomainEvent) {
            self.0.lock().unwrap().push(e);
        }
    }

    fn sample_track() -> domain::Track {
        domain::Track::new(
            TrackId(7),
            "Souvlaki Space Station",
            "Slowdive",
            None,
            Duration::from_secs(354),
            FilePath("/music/slowdive/03.mp3".into()),
            Some(3),
            0,
        )
        .unwrap()
    }

    #[test]
    fn plays_found_track_and_emits_events() {
        let uc = PlayTrack::new(
            MockRepo(Some(sample_track())),
            MockAudio::default(),
            MockMedia::default(),
            MockEvents::default(),
        );
        let dto = block_on(uc.execute(TrackId(7))).unwrap();

        assert_eq!(dto.title, "Souvlaki Space Station");
        assert_eq!(dto.duration_secs, 354);

        let audio: &MockAudio = uc.audio_ref();
        assert_eq!(
            audio.loaded.lock().unwrap().as_slice(),
            ["/music/slowdive/03.mp3"]
        );
        assert!(audio.played.load(Ordering::SeqCst));

        let events = uc.events_ref().0.lock().unwrap();
        assert!(matches!(events[0], DomainEvent::TrackChanged(_)));
        assert!(
            matches!(&events[1], DomainEvent::PlaybackStateChanged(s) if s.status == "playing")
        );
    }

    #[test]
    fn errors_when_track_missing() {
        let uc = PlayTrack::new(
            MockRepo(None),
            MockAudio::default(),
            MockMedia::default(),
            MockEvents::default(),
        );
        let err = block_on(uc.execute(TrackId(99))).unwrap_err();
        assert_eq!(err, PlayTrackError::NotFound(TrackId(99)));
    }

    // Accessors for tests live behind cfg(test) to keep the public API clean.
    impl<R, A, M, E> PlayTrack<R, A, M, E> {
        fn audio_ref(&self) -> &A {
            &self.audio
        }
        fn events_ref(&self) -> &E {
            &self.events
        }
    }
}
