use domain::Track;

/// Wire format sent to the UI. Flat and serde-friendly by design;
/// UI never sees domain types directly.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct TrackDto {
    pub id: u64,
    pub title: String,
    pub artist: String,
    pub album_id: Option<u64>,
    pub duration_secs: u64,
    pub path: String,
    pub track_no: Option<u32>,
}

impl From<&Track> for TrackDto {
    fn from(t: &Track) -> Self {
        Self {
            id: t.id.raw(),
            title: t.title.clone(),
            artist: t.artist.clone(),
            album_id: t.album_id.map(|a| a.raw()),
            duration_secs: t.duration.as_secs(),
            path: t.path.0.clone(),
            track_no: t.track_no,
        }
    }
}

impl From<Track> for TrackDto {
    fn from(t: Track) -> Self {
        (&t).into()
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PlaybackStateDto {
    /// "stopped" | "playing" | "paused"
    pub status: String,
    pub position_secs: u64,
    pub volume: f32,
}
