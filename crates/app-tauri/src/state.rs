use std::sync::Arc;

use application::{PlayTrack, ScanLibrary};
use infrastructure::persistence::{SqlitePlaylistRepository, SqliteTrackRepository};
use infrastructure::{CpalSymphoniaEngine, LoftyMetadataReader, SouvlakiControls, WalkdirScanner};

use crate::events::TauriEvents;

pub struct AppState {
    pub tracks: Arc<SqliteTrackRepository>,
    /// ponytail: wired up in slice 2 (playlist CRUD commands).
    #[allow(dead_code)]
    pub playlists: Arc<SqlitePlaylistRepository>,
    pub audio: Arc<CpalSymphoniaEngine>,
    pub media: Arc<SouvlakiControls>,
    pub events: Arc<TauriEvents>,
    pub play: PlayTrack<
        Arc<SqliteTrackRepository>,
        Arc<CpalSymphoniaEngine>,
        Arc<SouvlakiControls>,
        Arc<TauriEvents>,
    >,
    pub scan: ScanLibrary<
        Arc<SqliteTrackRepository>,
        LoftyMetadataReader,
        WalkdirScanner,
        Arc<TauriEvents>,
    >,
    /// One library scan at a time.
    pub scan_lock: tokio::sync::Mutex<()>,
    /// Last requested volume (bits), mirrored from commands for readback.
    pub volume: Arc<std::sync::atomic::AtomicU32>,
}
