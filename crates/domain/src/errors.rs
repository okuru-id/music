#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum DomainError {
    #[error("index out of range: {0}")]
    IndexOutOfRange(usize),
    #[error("playlist name cannot be empty")]
    EmptyPlaylistName,
    #[error("volume out of range 0.0-1.0: {0}")]
    VolumeOutOfRange(f32),
    #[error("invalid track duration: {0}s")]
    InvalidDuration(u64),
}
