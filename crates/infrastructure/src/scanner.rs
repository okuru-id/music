use std::sync::mpsc;
use std::thread;

use futures::stream::BoxStream;
use futures::{StreamExt, stream};
use walkdir::WalkDir;

use domain::{AudioFormat, FilePath};

use application::{FileScanner, ScannedFile};

/// Recursive filesystem walk yielding audio files by extension.
/// Runs on its own OS thread; the stream is consumed by the use case.
pub struct WalkdirScanner;

impl FileScanner for WalkdirScanner {
    fn scan(&self, roots: &[FilePath]) -> BoxStream<'static, ScannedFile> {
        let roots: Vec<String> = roots.iter().map(|r| r.0.clone()).collect();
        let (tx, rx) = mpsc::channel::<ScannedFile>();
        thread::spawn(move || {
            for root in roots {
                for entry in WalkDir::new(&root).follow_links(false) {
                    let Ok(entry) = entry else { continue };
                    let path = entry.path();
                    if !path.is_file() {
                        continue;
                    }
                    if let Some(ext) = path.extension().and_then(|e| e.to_str())
                        && AudioFormat::from_extension(&ext.to_ascii_lowercase()).is_some()
                    {
                        let mtime_secs = entry
                            .metadata()
                            .ok()
                            .and_then(|m| m.modified().ok())
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs())
                            .unwrap_or(0);
                        if tx
                            .send(ScannedFile {
                                path: FilePath(path.display().to_string()),
                                mtime_secs,
                            })
                            .is_err()
                        {
                            return; // consumer dropped: stop scanning
                        }
                    }
                }
            }
        });
        stream::unfold(rx, |rx| async move {
            match rx.recv() {
                Ok(f) => Some((f, rx)),
                Err(_) => None,
            }
        })
        .boxed()
    }
}
