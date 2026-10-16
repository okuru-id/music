use domain::PlaylistId;

use crate::dto::{PlaybackStateDto, TrackDto};

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum DomainEvent {
    TrackChanged(TrackDto),
    PlaybackStateChanged(PlaybackStateDto),
    ScanProgress {
        scanned: u64,
        added: u64,
    },
    ScanFinished {
        scanned: u64,
        added: u64,
        skipped: u64,
        failed: u64,
    },
    QueueChanged,
    PlaylistChanged {
        playlist_id: PlaylistId,
    },
}

/// Port pushing state changes to the presentation layer (Tauri `emit`).
pub trait EventPublisher: Send + Sync {
    fn publish(&self, e: DomainEvent);
}
