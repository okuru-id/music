use std::time::Duration;

use domain::{FilePath, Volume};

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum AudioError {
    #[error("cannot load `{0}`: {1}")]
    Load(String, String),
    #[error("device error: {0}")]
    Device(String),
}

/// Low-level playback. Implementations must keep I/O off the UI thread.
/// `enqueue_next` is the gapless hook: preload the next track before the
/// current one ends.
pub trait AudioEngine: Send + Sync {
    fn load(&self, path: &FilePath) -> Result<(), AudioError>;
    fn play(&self);
    fn pause(&self);
    fn seek(&self, pos: Duration) -> Result<(), AudioError>;
    fn set_volume(&self, v: Volume);
    fn enqueue_next(&self, path: &FilePath);
    fn position(&self) -> Duration;
    fn is_playing(&self) -> bool;
    /// False when nothing is loaded — track ended with no gapless next.
    fn is_active(&self) -> bool {
        true
    }
}
