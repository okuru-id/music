pub mod audio;
pub mod events;
pub mod media;
pub mod metadata;
pub mod repository;
pub mod scanner;

// Shared port handles (Arc<T>) satisfy the ports via blanket forwarding —
// generic bounds like `R: TrackRepository` then accept Arc<Sqlite...>.
impl<T: audio::AudioEngine + ?Sized> audio::AudioEngine for std::sync::Arc<T> {
    fn load(&self, path: &domain::FilePath) -> Result<(), audio::AudioError> {
        (**self).load(path)
    }
    fn play(&self) {
        (**self).play()
    }
    fn pause(&self) {
        (**self).pause()
    }
    fn seek(&self, pos: std::time::Duration) -> Result<(), audio::AudioError> {
        (**self).seek(pos)
    }
    fn set_volume(&self, v: domain::Volume) {
        (**self).set_volume(v)
    }
    fn enqueue_next(&self, path: &domain::FilePath) {
        (**self).enqueue_next(path)
    }
    fn position(&self) -> std::time::Duration {
        (**self).position()
    }
    fn is_playing(&self) -> bool {
        (**self).is_playing()
    }
    fn is_active(&self) -> bool {
        (**self).is_active()
    }
}

impl<T: media::MediaControls + ?Sized> media::MediaControls for std::sync::Arc<T> {
    fn update(&self, now: &media::NowPlaying) {
        (**self).update(now)
    }
}

impl<T: events::EventPublisher + ?Sized> events::EventPublisher for std::sync::Arc<T> {
    fn publish(&self, e: events::DomainEvent) {
        (**self).publish(e)
    }
}

#[async_trait::async_trait]
impl<T: repository::TrackRepository + ?Sized> repository::TrackRepository for std::sync::Arc<T> {
    async fn upsert_many(&self, tracks: &[domain::Track]) -> Result<(), repository::RepoError> {
        (**self).upsert_many(tracks).await
    }
    async fn find_by_id(
        &self,
        id: domain::TrackId,
    ) -> Result<Option<domain::Track>, repository::RepoError> {
        (**self).find_by_id(id).await
    }
    async fn search(
        &self,
        q: &repository::SearchQuery,
    ) -> Result<repository::Page<domain::Track>, repository::RepoError> {
        (**self).search(q).await
    }
    async fn find_mtime_by_path(&self, path: &str) -> Result<Option<u64>, repository::RepoError> {
        (**self).find_mtime_by_path(path).await
    }
    async fn upsert_album(
        &self,
        title: &str,
        album_artist: Option<&str>,
    ) -> Result<domain::AlbumId, repository::RepoError> {
        (**self).upsert_album(title, album_artist).await
    }
}

#[async_trait::async_trait]
impl<T: repository::PlaylistRepository + ?Sized> repository::PlaylistRepository
    for std::sync::Arc<T>
{
    async fn save(&self, p: &domain::Playlist) -> Result<(), repository::RepoError> {
        (**self).save(p).await
    }
    async fn find_by_id(
        &self,
        id: domain::PlaylistId,
    ) -> Result<Option<domain::Playlist>, repository::RepoError> {
        (**self).find_by_id(id).await
    }
    async fn list(&self) -> Result<Vec<domain::Playlist>, repository::RepoError> {
        (**self).list().await
    }
    async fn delete(&self, id: domain::PlaylistId) -> Result<(), repository::RepoError> {
        (**self).delete(id).await
    }
}
