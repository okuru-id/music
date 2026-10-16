use std::time::Duration;

use domain::FilePath;
use lofty::file::{AudioFile, TaggedFileExt};

use application::{MetaError, MetadataReader, TrackMetadata};

/// Metadata via lofty. Unreadable files → MetaError::Read, caller decides.
pub struct LoftyMetadataReader;

impl MetadataReader for LoftyMetadataReader {
    fn read(&self, path: &FilePath) -> Result<TrackMetadata, MetaError> {
        let tagged = lofty::read_from_path(&path.0)
            .map_err(|e| MetaError::Read(path.0.clone(), e.to_string()))?;
        let properties = tagged.properties();
        let tag = tagged.primary_tag().or_else(|| tagged.first_tag());

        let text =
            |key: lofty::tag::ItemKey| tag.and_then(|t| t.get_string(key)).map(str::to_string);
        let track_no = text(lofty::tag::ItemKey::TrackNumber).and_then(|s| {
            // "3/12" → 3
            s.split('/').next().and_then(|n| n.trim().parse().ok())
        });
        let year =
            text(lofty::tag::ItemKey::Year).and_then(|s| s.get(..4).unwrap_or(&s).parse().ok());

        let cover = tag
            .and_then(|t| t.pictures().first())
            .map(|p| p.data().to_vec());

        Ok(TrackMetadata {
            title: text(lofty::tag::ItemKey::TrackTitle),
            artist: text(lofty::tag::ItemKey::TrackArtist),
            album: text(lofty::tag::ItemKey::AlbumTitle),
            album_artist: text(lofty::tag::ItemKey::AlbumArtist),
            track_no,
            year,
            duration: Some(properties.duration()),
            cover,
        })
    }
}

// keep Duration import used even if properties().duration() is adjusted
#[allow(unused)]
fn _duration_type_check(d: Duration) -> Duration {
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unreadable_file_is_read_error() {
        let err = LoftyMetadataReader.read(&FilePath("/definitely/not/here.mp3".into()));
        assert!(matches!(err, Err(MetaError::Read(..))));
    }
}
