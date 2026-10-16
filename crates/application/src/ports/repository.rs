use async_trait::async_trait;

use domain::{Playlist, Track, TrackId};

#[derive(Debug, Clone, Default)]
pub struct SearchQuery {
    pub text: String,
    pub limit: u32,
    pub offset: u32,
}

impl SearchQuery {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            limit: 50,
            offset: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: u64,
    pub offset: u32,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum RepoError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("storage error: {0}")]
    Storage(String),
}

#[async_trait]
pub trait TrackRepository: Send + Sync {
    async fn upsert_many(&self, tracks: &[Track]) -> Result<(), RepoError>;
    async fn find_by_id(&self, id: TrackId) -> Result<Option<Track>, RepoError>;
    async fn search(&self, q: &SearchQuery) -> Result<Page<Track>, RepoError>;
    /// mtime recorded at last scan, if the path is already in the library.
    async fn find_mtime_by_path(&self, path: &str) -> Result<Option<u64>, RepoError>;
    /// Find or create the album identified by (title, album_artist). Returns its id.
    async fn upsert_album(
        &self,
        title: &str,
        album_artist: Option<&str>,
    ) -> Result<domain::AlbumId, RepoError>;
}

#[async_trait]
pub trait PlaylistRepository: Send + Sync {
    async fn save(&self, playlist: &Playlist) -> Result<(), RepoError>;
    async fn find_by_id(&self, id: domain::PlaylistId) -> Result<Option<Playlist>, RepoError>;
    async fn list(&self) -> Result<Vec<Playlist>, RepoError>;
    async fn delete(&self, id: domain::PlaylistId) -> Result<(), RepoError>;
}
