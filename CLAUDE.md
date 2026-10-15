# TAW Player: working conventions

The Player (PL-001) is a native, gapless music player by Timster's Audio Works, written in Rust with egui (via eframe), the native successor to Spool (TimsterAudio/Spool). Read `docs/ARCHITECTURE.md` before you change code. The panel is the TAW hi-fi look; its finishes, nameplate, window buttons, controls, meters and spectrum analysis come from the shared `taw-panel` crate's `hifi` module (TimsterAudio/TAW-Panel, feature `analyser`), pinned by commit in Cargo.toml. The window has no system title bar unless the user turns it on in Settings. Change the shared look there, then move the pin. TAW-Panel's `FAMILY.md` covers how it fits with the Library, Equaliser and Analyser.

## Hard rules

- **Native, not HTML.** No web views or browser shells.
- **Stand-alone.** The Player must work with no other family app installed. The family switcher only starts the others if they're found, and says plainly when they aren't.
- **Never change a music file.** The Player only reads.
- **No network requests at runtime.** Fonts and the icon are built into the binary. The hand-off between launches uses files in the data folder, not a socket.
- **Keep the output callback real-time safe.** It must not lock, allocate, do file I/O or log. It shares only atomics (`engine::Shared`) and the sample ring with the rest of the Player. Decoding, resampling and file reading happen on the player thread.
- **Be honest about the signal path.** Never claim "bit-perfect". Say what the Player does (native rate, resampled, volume) and no more; the system mixer after it is outside its control. Only resample when the output can't take the track's rate.
- **Never break saved settings.** `Saved` in `app.rs` uses `#[serde(default)]`. Add new fields with defaults, and never rename or repurpose existing ones.
- **Keep the command line stable:** `taw-player [--play] [FILE | FOLDER | PLAYLIST ...]`. The Library depends on it.

## Files

| File | What it does |
|---|---|
| `src/main.rs` | Command line, hand-off to a running Player, the window. |
| `src/app.rs` | The panel: nameplate and family switcher, now playing, the display (time, meters, spectrum), the list, the transport bar and the settings drawer. |
| `src/engine.rs` | The player thread, the output (cpal) and its callback, gapless joins, rate changes. |
| `src/decode.rs` | Reading one file into samples (symphonia). |
| `src/convert.rs` | The rate decision, channel mapping, resampling (rubato) only when needed. |
| `src/playlist.rs` | Folders, files and M3U/M3U8 into a list. |
| `src/tags.rs` | Tags and artwork (lofty), on worker threads. |
| `src/media.rs` | Media keys and the desktop's media controls (souvlaki: MPRIS over zbus on Linux, SMTC on Windows). |
| `src/family.rs` | Finding and starting the Library, Equaliser and Analyser. |
| `src/instance.rs` | One Player at a time; the data folder. |
| `src/icon.rs` | The window icon. |

## Checks before pushing

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo clippy --target x86_64-pc-windows-gnu -- -D warnings   # Windows still compiles
```

`tools/make-test-tracks.sh` (ffmpeg) makes test tracks; `cargo test -- --include-ignored` then decodes them all. For playback changes, play `tools/fixtures/gapless-flac` and listen (or record the output) for gaps at the joins. For panel changes, screenshot it under Xvfb at 1280 × 800 and at a laptop size such as 1280 × 720, in Spool and Porcelain, light and dark, Simple and Detailed. In a cloud session there's no sound card: run PipeWire with a null sink (see `docs/ARCHITECTURE.md`, "Testing without a sound card").

## Workflow

- Work on a feature branch. `main` always holds a working version.
- Open a pull request when the user asks, or merge when they ask.
- For every user-visible change, add a line under **Unreleased** in `CHANGELOG.md`, and tick off `ROADMAP.md`.
- For a release: move Unreleased into a section headed `## X.Y.Z – YYYY-MM-DD`, and bump `version` in `Cargo.toml` and the version in `README.md`. Merging that to `main` publishes the release with the Linux and Windows packages (`.github/workflows/release.yml`).
