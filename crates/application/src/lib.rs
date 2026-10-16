pub mod dto;
pub mod ports;
pub mod usecases;

pub use dto::{PlaybackStateDto, TrackDto};
pub use ports::audio::{AudioEngine, AudioError};
pub use ports::events::{DomainEvent, EventPublisher};
pub use ports::media::{MediaControls, NowPlaying};
pub use ports::metadata::{MetaError, MetadataReader, TrackMetadata};
pub use ports::repository::{Page, PlaylistRepository, RepoError, SearchQuery, TrackRepository};
pub use ports::scanner::{FileScanner, ScannedFile};
pub use usecases::play_track::{PlayTrack, PlayTrackError};
pub use usecases::scan_library::{ScanError, ScanLibrary, ScanSummary};
