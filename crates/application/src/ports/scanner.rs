use futures::stream::BoxStream;

use domain::FilePath;

/// A file yielded by the scanner, with metadata the use case needs
/// (mtime drives incremental rescans) so the use case itself stays I/O-free.
#[derive(Debug, Clone, PartialEq)]
pub struct ScannedFile {
    pub path: FilePath,
    pub mtime_secs: u64,
}

/// Recursively yields audio files under the given roots.
/// Streaming on purpose: a 50k-file library must not be collected in memory first.
pub trait FileScanner: Send + Sync {
    fn scan(&self, roots: &[FilePath]) -> BoxStream<'static, ScannedFile>;
}
