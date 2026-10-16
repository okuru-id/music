# Spotiflac

Pemutar musik lokal (FLAC, MP3, AAC, Vorbis, WAV) berbasis Tauri 2 dan Rust.

## Fitur

- Scan folder musik secara rekursif, metadata dibaca dengan `lofty`
- Library tersimpan di SQLite (`sqlx`)
- Playback via `symphonia` + `cpal`, dengan queue, seek, dan volume
- Playlist
- Kontrol media OS (MPRIS di Linux, lewat `souvlaki`)

## Arsitektur

Rust workspace berlapis (clean architecture):

| Crate | Isi |
|---|---|
| `domain` | Entitas: track, album, playlist, queue, playback |
| `application` | Ports (trait) dan use case (`play_track`, `scan_library`) |
| `infrastructure` | Audio, scanner, metadata, persistence SQLite, media controls |
| `app-tauri` | Shell Tauri: commands, events, state |

Aturan layer: `application` tidak boleh bergantung pada `tauri`, `sqlx`, `tokio`, `walkdir`, `symphonia`, `cpal`, atau `lofty`. CI memeriksanya.

UI berupa satu file statis di `ui/dist/index.html`, tanpa build step.

## Menjalankan

Prasyarat: Rust (edition 2024) dan [dependensi sistem Tauri](https://v2.tauri.app/start/prerequisites/), plus ALSA dev (`libasound2-dev`) di Linux.

```bash
cargo install tauri-cli --version "^2" --locked
cd crates/app-tauri
cargo tauri dev
```

## Pengembangan

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

`music-player.pen` dan `music-player-assets/` adalah referensi desain UI.
