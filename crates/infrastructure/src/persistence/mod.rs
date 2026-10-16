use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use sqlx::{
    Row,
    sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions},
};

use application::PlaylistRepository;
use application::{Page, RepoError, SearchQuery, TrackRepository};
use domain::{AlbumId, Playlist, PlaylistId, Track, TrackId};

const SCHEMA_SQL: &str = include_str!("../../migrations/0001_init.sql");

async fn connect_opts(
    opts: SqliteConnectOptions,
    max_connections: u32,
) -> Result<SqlitePool, RepoError> {
    let pool = SqlitePoolOptions::new()
        .max_connections(max_connections)
        .connect_with(opts)
        .await
        .map_err(|e| RepoError::Storage(e.to_string()))?;
    // ponytail: sqlx 0.9 dropped the `migrate!` macro re-export; one schema
    // file via raw_sql covers the MVP. Reintroduce migrations when v2 lands.
    sqlx::raw_sql(SCHEMA_SQL)
        .execute(&pool)
        .await
        .map_err(|e| RepoError::Storage(e.to_string()))?;
    Ok(pool)
}

/// Open (creating if needed) the library database and run migrations.
pub async fn connect(db_path: &Path) -> Result<SqlitePool, RepoError> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| RepoError::Storage(e.to_string()))?;
    }
    let opts = SqliteConnectOptions::new()
        .filename(db_path)
        .create_if_missing(true)
        .foreign_keys(true);
    connect_opts(opts, 5).await
}

#[cfg(test)]
pub async fn connect_in_memory() -> Result<SqlitePool, RepoError> {
    // Single connection: :memory: lives per-connection.
    connect_opts(
        SqliteConnectOptions::new()
            .filename(":memory:")
            .foreign_keys(true),
        1,
    )
    .await
}

fn row_to_track(row: &sqlx::sqlite::SqliteRow) -> Track {
    Track::new(
        TrackId(row.get::<i64, _>("id") as u64),
        row.get::<String, _>("title"),
        row.get::<String, _>("artist"),
        row.get::<Option<i64>, _>("album_id")
            .map(|a| AlbumId(a as u64)),
        std::time::Duration::from_secs(row.get::<i64, _>("duration_secs") as u64),
        domain::FilePath(row.get::<String, _>("path")),
        row.get::<Option<i64>, _>("track_no").map(|t| t as u32),
        row.get::<i64, _>("mtime_secs") as u64,
    )
    .expect("db invariants (duration>0, non-empty) hold")
}

pub struct SqliteTrackRepository {
    pool: Arc<SqlitePool>,
}

impl SqliteTrackRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool: Arc::new(pool),
        }
    }

    /// (id, title, album_artist, track_count) for the library grid. Concrete
    /// method — the UI grid needs it but no use case does (ponytail).
    pub async fn list_albums(&self) -> Result<Vec<(u64, String, Option<String>, u64)>, RepoError> {
        let rows: Vec<(i64, String, Option<String>, i64)> = sqlx::query_as(
            "SELECT a.id, a.title, a.album_artist, COUNT(t.id) AS n
             FROM albums a JOIN tracks t ON t.album_id = a.id
             GROUP BY a.id ORDER BY a.title",
        )
        .fetch_all(&*self.pool)
        .await
        .map_err(|e| RepoError::Storage(e.to_string()))?;
        Ok(rows
            .into_iter()
            .map(|(i, t, a, n)| (i as u64, t, a, n as u64))
            .collect())
    }
}

#[async_trait]
impl TrackRepository for SqliteTrackRepository {
    async fn upsert_many(&self, tracks: &[Track]) -> Result<(), RepoError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| RepoError::Storage(e.to_string()))?;
        for t in tracks {
            sqlx::query(
                "INSERT INTO tracks (path, title, artist, album_id, track_no, duration_secs, mtime_secs)
                 VALUES (?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT(path) DO UPDATE SET
                   title = excluded.title, artist = excluded.artist,
                   album_id = excluded.album_id, track_no = excluded.track_no,
                   duration_secs = excluded.duration_secs, mtime_secs = excluded.mtime_secs",
            )
            .bind(&t.path.0)
            .bind(&t.title)
            .bind(&t.artist)
            .bind(t.album_id.map(|a| a.raw() as i64))
            .bind(t.track_no.map(|t| t as i64))
            .bind(t.duration.as_secs() as i64)
            .bind(t.mtime_secs as i64)
            .execute(&mut *tx)
            .await
            .map_err(|e| RepoError::Storage(e.to_string()))?;

            // Keep the FTS index in sync for this row.
            sqlx::query(
                "INSERT INTO tracks_fts (rowid, title, artist, album)
                 SELECT t.id, t.title, t.artist, COALESCE(a.title, '')
                 FROM tracks t LEFT JOIN albums a ON a.id = t.album_id
                 WHERE t.path = ?",
            )
            .bind(&t.path.0)
            .execute(&mut *tx)
            .await
            .map_err(|e| RepoError::Storage(e.to_string()))?;
        }
        tx.commit()
            .await
            .map_err(|e| RepoError::Storage(e.to_string()))
    }

    async fn find_by_id(&self, id: TrackId) -> Result<Option<Track>, RepoError> {
        let row = sqlx::query(
            "SELECT id, path, title, artist, album_id, track_no, duration_secs, mtime_secs FROM tracks WHERE id = ?",
        )
        .bind(id.raw() as i64)
        .fetch_optional(&*self.pool)
        .await
        .map_err(|e| RepoError::Storage(e.to_string()))?;
        Ok(row.as_ref().map(row_to_track))
    }

    async fn search(&self, q: &SearchQuery) -> Result<Page<Track>, RepoError> {
        let limit = i64::from(q.limit.max(1));
        let offset = i64::from(q.offset);

        if q.text.trim().is_empty() {
            let rows = sqlx::query(
                "SELECT id, path, title, artist, album_id, track_no, duration_secs, mtime_secs FROM tracks ORDER BY artist, album_id, track_no, title LIMIT ? OFFSET ?",
            )
            .bind(limit)
            .bind(offset)
            .fetch_all(&*self.pool)
            .await
            .map_err(|e| RepoError::Storage(e.to_string()))?;
            let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tracks")
                .fetch_one(&*self.pool)
                .await
                .map_err(|e| RepoError::Storage(e.to_string()))?;
            return Ok(Page {
                items: rows.iter().map(row_to_track).collect(),
                total: total as u64,
                offset: q.offset,
            });
        }

        let pattern = format!("{}*", q.text.trim());
        let rows = sqlx::query(
            "SELECT t.id, t.path, t.title, t.artist, t.album_id, t.track_no, t.duration_secs, t.mtime_secs FROM tracks_fts f
             JOIN tracks t ON t.id = f.rowid
             WHERE tracks_fts MATCH ?
             ORDER BY rank LIMIT ? OFFSET ?",
        )
        .bind(&pattern)
        .bind(limit)
        .bind(offset)
        .fetch_all(&*self.pool)
        .await
        .map_err(|e| RepoError::Storage(e.to_string()))?;
        let total: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM tracks_fts WHERE tracks_fts MATCH ?")
                .bind(&pattern)
                .fetch_one(&*self.pool)
                .await
                .map_err(|e| RepoError::Storage(e.to_string()))?;
        Ok(Page {
            items: rows.iter().map(row_to_track).collect(),
            total: total as u64,
            offset: q.offset,
        })
    }

    async fn find_mtime_by_path(&self, path: &str) -> Result<Option<u64>, RepoError> {
        let mtime: Option<i64> = sqlx::query_scalar("SELECT mtime_secs FROM tracks WHERE path = ?")
            .bind(path)
            .fetch_optional(&*self.pool)
            .await
            .map_err(|e| RepoError::Storage(e.to_string()))?;
        Ok(mtime.map(|m| m as u64))
    }

    async fn upsert_album(
        &self,
        title: &str,
        album_artist: Option<&str>,
    ) -> Result<AlbumId, RepoError> {
        sqlx::query(
            "INSERT INTO albums (title, album_artist) VALUES (?, ?)
                     ON CONFLICT(title, album_artist) DO NOTHING",
        )
        .bind(title)
        .bind(album_artist)
        .execute(&*self.pool)
        .await
        .map_err(|e| RepoError::Storage(e.to_string()))?;
        let id: i64 =
            sqlx::query_scalar("SELECT id FROM albums WHERE title = ? AND album_artist IS ?")
                .bind(title)
                .bind(album_artist)
                .fetch_one(&*self.pool)
                .await
                .map_err(|e| RepoError::Storage(e.to_string()))?;
        Ok(AlbumId(id as u64))
    }
}

pub struct SqlitePlaylistRepository {
    pool: Arc<SqlitePool>,
}

impl SqlitePlaylistRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool: Arc::new(pool),
        }
    }

    /// Create a playlist, returning its id (concrete method — UI-only, no use
    /// case yet, ponytail).
    pub async fn create(&self, name: &str) -> Result<u64, RepoError> {
        let id: i64 = sqlx::query_scalar("INSERT INTO playlists (name) VALUES (?) RETURNING id")
            .bind(name)
            .fetch_one(&*self.pool)
            .await
            .map_err(|e| RepoError::Storage(e.to_string()))?;
        Ok(id as u64)
    }

    /// Append at the end; same track may appear twice (allowed, ponytail).
    pub async fn append_track(&self, playlist_id: u64, track_id: u64) -> Result<(), RepoError> {
        sqlx::query(
            "INSERT INTO playlist_tracks (playlist_id, track_id, position)
             VALUES (?, ?, (SELECT COALESCE(MAX(position) + 1, 0)
                            FROM playlist_tracks WHERE playlist_id = ?))",
        )
        .bind(playlist_id as i64)
        .bind(track_id as i64)
        .bind(playlist_id as i64)
        .execute(&*self.pool)
        .await
        .map_err(|e| RepoError::Storage(e.to_string()))?;
        Ok(())
    }

    /// (id, name, track_count) for the playlist list.
    pub async fn list_with_counts(&self) -> Result<Vec<(u64, String, u64)>, RepoError> {
        let rows: Vec<(i64, String, i64)> = sqlx::query_as(
            "SELECT p.id, p.name, COUNT(pt.track_id) AS n
             FROM playlists p LEFT JOIN playlist_tracks pt ON pt.playlist_id = p.id
             GROUP BY p.id ORDER BY p.name",
        )
        .fetch_all(&*self.pool)
        .await
        .map_err(|e| RepoError::Storage(e.to_string()))?;
        Ok(rows
            .into_iter()
            .map(|(i, n, c)| (i as u64, n, c as u64))
            .collect())
    }

    /// Tracks of a playlist in order.
    pub async fn tracks_of(&self, playlist_id: u64) -> Result<Vec<Track>, RepoError> {
        let rows = sqlx::query(
            "SELECT t.id, t.path, t.title, t.artist, t.album_id, t.track_no,
                    t.duration_secs, t.mtime_secs
             FROM playlist_tracks pt
             JOIN tracks t ON t.id = pt.track_id
             WHERE pt.playlist_id = ?
             ORDER BY pt.position",
        )
        .bind(playlist_id as i64)
        .fetch_all(&*self.pool)
        .await
        .map_err(|e| RepoError::Storage(e.to_string()))?;
        Ok(rows.iter().map(row_to_track).collect())
    }
}

#[async_trait]
impl PlaylistRepository for SqlitePlaylistRepository {
    async fn save(&self, p: &Playlist) -> Result<(), RepoError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| RepoError::Storage(e.to_string()))?;
        sqlx::query(
            "INSERT INTO playlists (id, name) VALUES (?, ?)
                     ON CONFLICT(id) DO UPDATE SET name = excluded.name",
        )
        .bind(p.id.raw() as i64)
        .bind(p.name())
        .execute(&mut *tx)
        .await
        .map_err(|e| RepoError::Storage(e.to_string()))?;
        sqlx::query("DELETE FROM playlist_tracks WHERE playlist_id = ?")
            .bind(p.id.raw() as i64)
            .execute(&mut *tx)
            .await
            .map_err(|e| RepoError::Storage(e.to_string()))?;
        for (i, tid) in p.tracks().iter().enumerate() {
            sqlx::query(
                "INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (?, ?, ?)",
            )
            .bind(p.id.raw() as i64)
            .bind(tid.raw() as i64)
            .bind(i as i64)
            .execute(&mut *tx)
            .await
            .map_err(|e| RepoError::Storage(e.to_string()))?;
        }
        tx.commit()
            .await
            .map_err(|e| RepoError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn find_by_id(&self, id: PlaylistId) -> Result<Option<Playlist>, RepoError> {
        let (name,): (String,) = match sqlx::query_as("SELECT name FROM playlists WHERE id = ?")
            .bind(id.raw() as i64)
            .fetch_optional(&*self.pool)
            .await
            .map_err(|e| RepoError::Storage(e.to_string()))?
        {
            Some(r) => r,
            None => return Ok(None),
        };
        let ids: Vec<i64> = sqlx::query_as(
            "SELECT track_id FROM playlist_tracks WHERE playlist_id = ? ORDER BY position",
        )
        .bind(id.raw() as i64)
        .fetch_all(&*self.pool)
        .await
        .map_err(|e| RepoError::Storage(e.to_string()))?
        .into_iter()
        .map(|(i,): (i64,)| i)
        .collect();
        // Deleted tracks drop out of the playlist silently.
        let mut track_ids = Vec::with_capacity(ids.len());
        for i in ids {
            let exists: Option<i64> = sqlx::query_scalar("SELECT 1 FROM tracks WHERE id = ?")
                .bind(i)
                .fetch_optional(&*self.pool)
                .await
                .map_err(|e| RepoError::Storage(e.to_string()))?;
            if exists.is_some() {
                track_ids.push(TrackId(i as u64));
            }
        }
        Playlist::build(id, name, track_ids)
            .map(Some)
            .map_err(|e| RepoError::Storage(e.to_string()))
    }

    async fn list(&self) -> Result<Vec<Playlist>, RepoError> {
        let rows: Vec<(i64, String)> =
            sqlx::query_as("SELECT id, name FROM playlists ORDER BY name")
                .fetch_all(&*self.pool)
                .await
                .map_err(|e| RepoError::Storage(e.to_string()))?;
        let mut out = Vec::with_capacity(rows.len());
        for (i, n) in rows {
            out.push(
                Playlist::build(PlaylistId(i as u64), n, Vec::new())
                    .map_err(|e| RepoError::Storage(e.to_string()))?,
            );
        }
        Ok(out)
    }

    async fn delete(&self, id: PlaylistId) -> Result<(), RepoError> {
        sqlx::query("DELETE FROM playlists WHERE id = ?")
            .bind(id.raw() as i64)
            .execute(&*self.pool)
            .await
            .map_err(|e| RepoError::Storage(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(path: &str, title: &str, mtime: u64) -> Track {
        Track::new(
            TrackId(0),
            title,
            "Artist",
            None,
            std::time::Duration::from_secs(100),
            domain::FilePath(path.into()),
            None,
            mtime,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn upsert_search_roundtrip() {
        let pool = connect_in_memory().await.unwrap();
        let repo = SqliteTrackRepository::new(pool);

        repo.upsert_many(&[
            track("/m/a.mp3", "Alpha Song", 1),
            track("/m/b.flac", "Beta Song", 1),
        ])
        .await
        .unwrap();

        // update same path (new mtime) — no duplicate
        repo.upsert_many(&[track("/m/a.mp3", "Alpha Song v2", 2)])
            .await
            .unwrap();

        let page = repo.search(&SearchQuery::new("beta")).await.unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].title, "Beta Song");

        let page = repo.search(&SearchQuery::new("alpha")).await.unwrap();
        assert_eq!(page.items[0].title, "Alpha Song v2");

        let mtime = repo.find_mtime_by_path("/m/a.mp3").await.unwrap();
        assert_eq!(mtime, Some(2));

        let t = repo.find_by_id(TrackId(1)).await.unwrap().unwrap();
        assert_eq!(t.path.0, "/m/a.mp3");

        // prefix search: "alph" matches
        let page = repo.search(&SearchQuery::new("alph")).await.unwrap();
        assert_eq!(page.total, 1);
    }

    #[tokio::test]
    async fn album_upsert_is_idempotent() {
        let pool = connect_in_memory().await.unwrap();
        let repo = SqliteTrackRepository::new(pool);
        let a1 = repo
            .upsert_album("Nightlife", Some("Artist"))
            .await
            .unwrap();
        let a2 = repo
            .upsert_album("Nightlife", Some("Artist"))
            .await
            .unwrap();
        assert_eq!(a1, a2);
        let a3 = repo.upsert_album("Nightlife", None).await.unwrap();
        assert_ne!(a1, a3);
    }
}
