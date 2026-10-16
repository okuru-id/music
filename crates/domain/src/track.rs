use std::fmt;
use std::time::Duration;

use crate::errors::DomainError;
use crate::ids::{AlbumId, TrackId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AudioFormat {
    Mp3,
    Flac,
    Aac,
    Vorbis,
    Wav,
}

impl AudioFormat {
    /// Map a file extension (case-insensitive) to its format.
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "mp3" => Some(Self::Mp3),
            "flac" => Some(Self::Flac),
            "m4a" | "mp4" | "aac" => Some(Self::Aac),
            "ogg" | "oga" | "opus" => Some(Self::Vorbis),
            "wav" => Some(Self::Wav),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FilePath(pub String);

impl FilePath {
    pub fn extension(&self) -> Option<String> {
        let name = self.0.rsplit(['/', '\\']).next().unwrap_or(&self.0);
        name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase())
    }
}

impl fmt::Display for FilePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A music track. Invariants: duration > 0; path unique per library.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Track {
    pub id: TrackId,
    pub title: String,
    pub artist: String,
    pub album_id: Option<AlbumId>,
    pub duration: Duration,
    pub path: FilePath,
    pub track_no: Option<u32>,
    /// File modification time at last scan — drives incremental rescan.
    pub mtime_secs: u64,
}

impl Track {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: TrackId,
        title: impl Into<String>,
        artist: impl Into<String>,
        album_id: Option<AlbumId>,
        duration: Duration,
        path: FilePath,
        track_no: Option<u32>,
        mtime_secs: u64,
    ) -> Result<Self, DomainError> {
        if duration.is_zero() {
            return Err(DomainError::InvalidDuration(duration.as_secs()));
        }
        Ok(Self {
            id,
            title: title.into(),
            artist: artist.into(),
            album_id,
            duration,
            path,
            track_no,
            mtime_secs,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_zero_duration() {
        let err = Track::new(
            TrackId(1),
            "Song",
            "Artist",
            None,
            Duration::ZERO,
            FilePath("/m/s.mp3".into()),
            None,
            0,
        );
        assert_eq!(err.unwrap_err(), DomainError::InvalidDuration(0));
    }

    #[test]
    fn extension_mapping() {
        assert_eq!(AudioFormat::from_extension("FLAC"), Some(AudioFormat::Flac));
        assert_eq!(AudioFormat::from_extension("m4a"), Some(AudioFormat::Aac));
        assert_eq!(AudioFormat::from_extension("txt"), None);
    }
}
