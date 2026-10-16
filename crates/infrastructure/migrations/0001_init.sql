CREATE TABLE IF NOT EXISTS albums (
    id           INTEGER PRIMARY KEY,
    title        TEXT NOT NULL,
    album_artist TEXT,
    UNIQUE(title, album_artist)
);

CREATE TABLE IF NOT EXISTS tracks (
    id            INTEGER PRIMARY KEY,
    path          TEXT NOT NULL UNIQUE,
    title         TEXT NOT NULL,
    artist        TEXT NOT NULL,
    album_id      INTEGER REFERENCES albums(id) ON DELETE SET NULL,
    track_no      INTEGER,
    duration_secs INTEGER NOT NULL,
    mtime_secs    INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_tracks_album ON tracks(album_id);

CREATE VIRTUAL TABLE IF NOT EXISTS tracks_fts USING fts5(title, artist, album, content='');

CREATE TABLE IF NOT EXISTS playlists (
    id   INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE
);

CREATE TABLE IF NOT EXISTS playlist_tracks (
    playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
    track_id    INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    position    INTEGER NOT NULL,
    UNIQUE(playlist_id, position)
);

CREATE INDEX IF NOT EXISTS idx_playlist_tracks ON playlist_tracks(playlist_id, position);
