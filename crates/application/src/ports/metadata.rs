use std::time::Duration;

use domain::FilePath;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrackMetadata {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub track_no: Option<u32>,
    pub year: Option<u16>,
    pub duration: Option<Duration>,
    /// Embedded cover bytes (raw image, format detected by decoder).
    pub cover: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum MetaError {
    #[error("unreadable metadata in `{0}`: {1}")]
    Read(String, String),
}

pub trait MetadataReader: Send + Sync {
    fn read(&self, path: &FilePath) -> Result<TrackMetadata, MetaError>;
}
