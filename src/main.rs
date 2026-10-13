//! Timster's Audio Works Player (PL-001): a native, gapless music player.
//! It works on its own; the Library can hand it albums to play, and it can
//! start the Equaliser and Analyser when they're installed.

// Hide the console window on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod convert;
mod decode;
mod engine;
mod family;
mod icon;
mod instance;
mod playlist;
mod tags;

use std::path::PathBuf;

const USAGE: &str = "TAW Player (PL-001)

  taw-player [--play] [FILE | FOLDER | PLAYLIST.m3u8 ...]

Plays what it's given: audio files, folders (with their sub-folders) or
M3U/M3U8 playlists. If a Player is already open, it takes them instead.";

fn main() -> eframe::Result {
    let mut paths: Vec<PathBuf> = Vec::new();
    for a in std::env::args_os().skip(1) {
        match a.to_str() {
            Some("--play") => {}
            Some("-h" | "--help") => {
                println!("{USAGE}");
                return Ok(());
            }
            Some("-V" | "--version") => {
                println!("taw-player {}", app::VERSION);
                return Ok(());
            }
            _ => {
                let p = PathBuf::from(a);
                paths.push(std::path::absolute(&p).unwrap_or(p));
            }
        }
    }
    if let Some(dir) = instance::data_dir() {
        if instance::hand_over(&dir, &paths) {
            return Ok(());
        }
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("TAW Player")
            .with_app_id("taw-player")
            .with_inner_size(app::DEFAULT_SIZE)
            .with_min_inner_size(egui::Vec2::new(640.0, 400.0))
            .with_icon(icon::icon()),
        ..Default::default()
    };
    eframe::run_native(
        "TAW Player",
        options,
        Box::new(move |cc| Ok(Box::new(app::PlayerApp::new(cc, paths)))),
    )
}
