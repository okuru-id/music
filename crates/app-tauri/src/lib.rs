mod commands;
mod events;
mod state;

use std::sync::Arc;
use std::sync::atomic::AtomicU32;

use application::AudioEngine;
use infrastructure::persistence::{SqlitePlaylistRepository, SqliteTrackRepository};
use infrastructure::{CpalSymphoniaEngine, LoftyMetadataReader, SouvlakiControls, WalkdirScanner};
use state::AppState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let dir = handle.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;

            let pool = tauri::async_runtime::block_on(infrastructure::persistence::connect(
                &dir.join("library.db"),
            ))
            .map_err(std::io::Error::other)?;

            let tracks = Arc::new(SqliteTrackRepository::new(pool.clone()));
            let playlists = Arc::new(SqlitePlaylistRepository::new(pool));
            let audio = Arc::new(CpalSymphoniaEngine::new().map_err(std::io::Error::other)?);
            let media = Arc::new(
                SouvlakiControls::new("spotiflac", "Spotiflac").map_err(std::io::Error::other)?,
            );
            let events = Arc::new(events::TauriEvents::new(handle));

            let play = application::PlayTrack::new(
                Arc::clone(&tracks),
                Arc::clone(&audio),
                Arc::clone(&media),
                Arc::clone(&events),
            );
            let scan = application::ScanLibrary::new(
                Arc::clone(&tracks),
                LoftyMetadataReader,
                WalkdirScanner,
                Arc::clone(&events),
            );

            // Media keys (MPRIS etc.) → engine, until queue use case lands.
            {
                let audio = Arc::clone(&audio);
                let media = Arc::clone(&media);
                std::thread::spawn(move || {
                    loop {
                        match media.try_recv() {
                            Some(infrastructure::MediaCommand::Play)
                            | Some(infrastructure::MediaCommand::Toggle) => {
                                if !audio.is_playing() {
                                    audio.play();
                                    media.set_playback_state(false, audio.position());
                                }
                            }
                            Some(infrastructure::MediaCommand::Pause) => {
                                audio.pause();
                                media.set_playback_state(true, audio.position());
                            }
                            Some(infrastructure::MediaCommand::Next)
                            | Some(infrastructure::MediaCommand::Previous) => {
                                // queue use case: fase 1 slice 2
                            }
                            None => std::thread::sleep(std::time::Duration::from_millis(200)),
                        }
                    }
                });
            }

            app.manage(AppState {
                tracks,
                playlists,
                audio,
                media,
                events,
                play,
                scan,
                scan_lock: tokio::sync::Mutex::new(()),
                volume: Arc::new(AtomicU32::new(1.0f32.to_bits())),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::scan_library,
            commands::search_tracks,
            commands::list_albums,
            commands::play_track,
            commands::enqueue_next,
            commands::toggle_play,
            commands::set_volume,
            commands::seek,
            commands::get_playback_state,
            commands::create_playlist,
            commands::list_playlists,
            commands::get_playlist,
            commands::delete_playlist,
            commands::add_to_playlist
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
