use std::sync::atomic::Ordering;
use std::time::Duration;

use application::{
    AudioEngine, DomainEvent, EventPublisher, PlaybackStateDto, PlaylistRepository, SearchQuery,
    TrackDto, TrackRepository,
};
use domain::{FilePath, TrackId, Volume};
use tauri::State;

use crate::state::AppState;

/// Start a library scan in the background; progress arrives via events.
#[tauri::command]
pub async fn scan_library(state: State<'_, AppState>, root: String) -> Result<(), String> {
    let _guard = state.scan_lock.lock().await;
    let roots = vec![FilePath(root)];
    state
        .scan
        .execute(roots)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn search_tracks(
    state: State<'_, AppState>,
    query: String,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<serde_json::Value, String> {
    let mut q = SearchQuery::new(query);
    q.limit = limit.unwrap_or(50).min(500);
    q.offset = offset.unwrap_or(0);
    let page = state.tracks.search(&q).await.map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "items": page.items.iter().map(TrackDto::from).collect::<Vec<_>>(),
        "total": page.total,
        "offset": page.offset,
    }))
}

/// Library albums for the grid (id, title, album_artist, track_count).
#[tauri::command]
pub async fn list_albums(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let albums = state
        .tracks
        .list_albums()
        .await
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!(
        albums
            .into_iter()
            .map(|(id, title, artist, count)| serde_json::json!({
                "id": id, "title": title, "artist": artist, "count": count
            }))
            .collect::<Vec<_>>()
    ))
}

#[tauri::command]
pub async fn play_track(state: State<'_, AppState>, id: u64) -> Result<TrackDto, String> {
    state
        .play
        .execute(TrackId(id))
        .await
        .map_err(|e| e.to_string())
}

/// Gapless hook: preload the next track while the current one plays.
#[tauri::command]
pub async fn enqueue_next(state: State<'_, AppState>, id: u64) -> Result<(), String> {
    if let Some(t) = state
        .tracks
        .find_by_id(TrackId(id))
        .await
        .map_err(|e| e.to_string())?
    {
        state.audio.enqueue_next(&t.path);
    }
    Ok(())
}

/// ponytail: pause/resume/seek/volume are stateless engine tweaks — commands
/// call the adapter directly; promote to a use case when queueing lands.
#[tauri::command]
pub async fn toggle_play(state: State<'_, AppState>) -> Result<PlaybackStateDto, String> {
    let audio = &state.audio;
    if audio.is_playing() {
        audio.pause();
    } else {
        audio.play();
    }
    let dto = current_state(&state);
    state
        .events
        .publish(DomainEvent::PlaybackStateChanged(dto.clone()));
    state
        .media
        .set_playback_state(!audio.is_playing(), audio.position());
    Ok(dto)
}

#[tauri::command]
pub async fn set_volume(state: State<'_, AppState>, volume: f32) -> Result<(), String> {
    let v = Volume::try_new(volume).map_err(|e| e.to_string())?;
    state.volume.store(v.inner().to_bits(), Ordering::Relaxed);
    state.audio.set_volume(v);
    Ok(())
}

#[tauri::command]
pub async fn seek(
    state: State<'_, AppState>,
    position_ms: u64,
) -> Result<PlaybackStateDto, String> {
    state
        .audio
        .seek(Duration::from_millis(position_ms))
        .map_err(|e| e.to_string())?;
    Ok(current_state(&state))
}

/// UI polls this ~2x/sec for position display.
#[tauri::command]
pub fn get_playback_state(state: State<'_, AppState>) -> PlaybackStateDto {
    current_state(&state)
}

fn current_state(state: &AppState) -> PlaybackStateDto {
    let status = if state.audio.is_playing() {
        "playing"
    } else if !state.audio.is_active() {
        "stopped"
    } else {
        "paused"
    };
    PlaybackStateDto {
        status: status.into(),
        position_secs: state.audio.position().as_secs(),
        volume: f32::from_bits(state.volume.load(Ordering::Relaxed)),
    }
}

// --- playlists (direct repo calls; no use case behind them yet, ponytail) ---

#[tauri::command]
pub async fn create_playlist(
    state: State<'_, AppState>,
    name: String,
) -> Result<serde_json::Value, String> {
    let id = state
        .playlists
        .create(&name)
        .await
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "id": id, "name": name }))
}

#[tauri::command]
pub async fn list_playlists(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let rows = state
        .playlists
        .list_with_counts()
        .await
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!(
        rows.into_iter()
            .map(|(id, name, count)| serde_json::json!({ "id": id, "name": name, "count": count }))
            .collect::<Vec<_>>()
    ))
}

#[tauri::command]
pub async fn get_playlist(
    state: State<'_, AppState>,
    id: u64,
) -> Result<serde_json::Value, String> {
    let playlist = state
        .playlists
        .find_by_id(domain::PlaylistId(id))
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "playlist not found".to_string())?;
    let tracks = state
        .playlists
        .tracks_of(id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "id": id,
        "name": playlist.name(),
        "tracks": tracks.iter().map(TrackDto::from).collect::<Vec<_>>(),
    }))
}

#[tauri::command]
pub async fn delete_playlist(state: State<'_, AppState>, id: u64) -> Result<(), String> {
    state
        .playlists
        .delete(domain::PlaylistId(id))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn add_to_playlist(
    state: State<'_, AppState>,
    playlist_id: u64,
    track_id: u64,
) -> Result<(), String> {
    state
        .playlists
        .append_track(playlist_id, track_id)
        .await
        .map_err(|e| e.to_string())
}
