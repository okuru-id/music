use std::sync::Mutex;
use std::sync::mpsc;
use std::time::Duration;

use application::{MediaControls, NowPlaying};
use souvlaki::{
    MediaControlEvent, MediaControls as Souvlaki, MediaMetadata, MediaPlayback, MediaPosition,
    PlatformConfig,
};

/// Commands coming back from OS media keys / MPRIS clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaCommand {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
}

/// souvlaki-backed media controls (MPRIS on Linux, SMTC on Windows,
/// Now Playing on macOS). Desktop only in fase 1.
pub struct SouvlakiControls {
    inner: Mutex<Souvlaki>,
    // Mutex: Receiver is !Sync but MediaControls requires Sync.
    events: Mutex<mpsc::Receiver<MediaCommand>>,
}

impl SouvlakiControls {
    pub fn new(dbus_name: &str, display_name: &str) -> Result<Self, souvlaki::Error> {
        let config = PlatformConfig {
            dbus_name,
            display_name,
            hwnd: None,
        };
        let mut controls = Souvlaki::new(config)?;
        let (tx, rx) = mpsc::sync_channel::<MediaCommand>(32);
        controls.attach(move |e| {
            let cmd = match e {
                MediaControlEvent::Play => MediaCommand::Play,
                MediaControlEvent::Pause => MediaCommand::Pause,
                MediaControlEvent::Toggle => MediaCommand::Toggle,
                MediaControlEvent::Next => MediaCommand::Next,
                MediaControlEvent::Previous => MediaCommand::Previous,
                _ => return,
            };
            let _ = tx.send(cmd);
        })?;
        Ok(Self {
            inner: Mutex::new(controls),
            events: Mutex::new(rx),
        })
    }

    /// Consumes pending media-key commands; non-blocking.
    pub fn try_recv(&self) -> Option<MediaCommand> {
        self.events.lock().unwrap().try_recv().ok()
    }

    pub fn set_playback_state(&self, paused: bool, position: Duration) {
        let playback = if paused {
            MediaPlayback::Paused {
                progress: Some(MediaPosition(position)),
            }
        } else {
            MediaPlayback::Playing {
                progress: Some(MediaPosition(position)),
            }
        };
        let _ = self.inner.lock().unwrap().set_playback(playback);
    }
}

impl MediaControls for SouvlakiControls {
    fn update(&self, now: &NowPlaying) {
        let mut controls = self.inner.lock().unwrap();
        let _ = controls.set_metadata(MediaMetadata {
            title: now.title.as_deref(),
            artist: now.artist.as_deref(),
            album: now.album.as_deref(),
            duration: now.duration,
            cover_url: None,
        });
        let position = now.position.unwrap_or(Duration::ZERO);
        let playback = if now.paused {
            MediaPlayback::Paused {
                progress: Some(MediaPosition(position)),
            }
        } else {
            MediaPlayback::Playing {
                progress: Some(MediaPosition(position)),
            }
        };
        let _ = controls.set_playback(playback);
    }
}
