use std::time::Duration;

/// What the OS media controls (MPRIS/SMTC/Now Playing/MediaSession) display.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NowPlaying {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration: Option<Duration>,
    pub position: Option<Duration>,
    pub paused: bool,
}

pub trait MediaControls: Send + Sync {
    fn update(&self, now: &NowPlaying);
}
