//! Media keys and the desktop's media controls: MPRIS on Linux (the sound
//! menu, `playerctl`, the keyboard's media keys), the System Media
//! Transport Controls on Windows. The keys arrive as `Key`s for the panel
//! to act on, and the panel tells the desktop what's playing. None of it
//! goes near the output callback.

use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
    SeekDirection,
};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

/// What the desktop asked for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Key {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
    Stop,
    /// Seek by this many seconds (negative is back).
    SeekBy(f64),
    /// Seek to this many seconds into the track.
    SeekTo(f64),
    Raise,
}

/// The track the desktop shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    pub path: PathBuf,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub seconds: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Playback {
    Stopped,
    Paused(f64),
    Playing(f64),
}

pub struct Media {
    controls: MediaControls,
    keys: Receiver<Key>,
    /// Finds the artwork off the panel's thread.
    art_jobs: Sender<(PathBuf, Option<PathBuf>)>,
    art_done: Receiver<(PathBuf, Option<String>)>,
    track: Option<Track>,
    art: Option<String>,
    playback: Option<Playback>,
}

impl Media {
    /// Connects to the desktop. None if there's nothing to connect to (no
    /// session bus, say); the Player works the same without it.
    pub fn start(ctx: egui::Context, hwnd: Option<*mut std::ffi::c_void>) -> Option<Media> {
        let mut controls = MediaControls::new(PlatformConfig {
            display_name: "TAW Player",
            dbus_name: "taw_player",
            hwnd,
        })
        .ok()?;
        let (tx, keys) = channel();
        let wake = ctx.clone();
        controls
            .attach(move |e| {
                if let Some(k) = key(e) {
                    let _ = tx.send(k);
                    wake.request_repaint();
                }
            })
            .ok()?;
        let (art_jobs, jobs) = channel::<(PathBuf, Option<PathBuf>)>();
        let (done, art_done) = channel();
        let _ = std::thread::Builder::new()
            .name("media-art".into())
            .spawn(move || {
                while let Ok((track, root)) = jobs.recv() {
                    let url = art_url(&track, root.as_deref());
                    if done.send((track, url)).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            });
        Some(Media {
            controls,
            keys,
            art_jobs,
            art_done,
            track: None,
            art: None,
            playback: None,
        })
    }

    pub fn keys(&self) -> Vec<Key> {
        self.keys.try_iter().collect()
    }

    /// Tells the desktop what's playing; only sends what changed.
    pub fn update(&mut self, track: Option<Track>, playback: Playback, root: Option<&Path>) {
        let mut changed = false;
        while let Ok((p, url)) = self.art_done.try_recv() {
            if self.track.as_ref().is_some_and(|t| t.path == p) && self.art != url {
                self.art = url;
                changed = true;
            }
        }
        if track != self.track {
            if let Some(t) = &track {
                if self.track.as_ref().map(|o| &o.path) != Some(&t.path) {
                    self.art = None;
                    let _ = self
                        .art_jobs
                        .send((t.path.clone(), root.map(Path::to_path_buf)));
                }
            } else {
                self.art = None;
            }
            self.track = track;
            changed = true;
        }
        if changed {
            let t = self.track.clone().unwrap_or_default();
            let _ = self.controls.set_metadata(MediaMetadata {
                title: self.track.as_ref().map(|_| t.title.as_str()),
                artist: t.artist.as_deref(),
                album: t.album.as_deref(),
                cover_url: self.art.as_deref(),
                duration: t.seconds.map(Duration::from_secs_f64),
            });
        }
        if self.playback.is_none_or(|old| moved(old, playback)) {
            self.playback = Some(playback);
            let at = |s: f64| Some(MediaPosition(Duration::from_secs_f64(s.max(0.0))));
            let _ = self.controls.set_playback(match playback {
                Playback::Stopped => MediaPlayback::Stopped,
                Playback::Paused(s) => MediaPlayback::Paused { progress: at(s) },
                Playback::Playing(s) => MediaPlayback::Playing { progress: at(s) },
            });
        }
    }
}

/// Worth telling the desktop: a change of state, or the position half a
/// second or more on (MPRIS doesn't count the time itself; the sound menu
/// asks for it).
fn moved(old: Playback, new: Playback) -> bool {
    match (old, new) {
        (Playback::Stopped, Playback::Stopped) => false,
        (Playback::Paused(a), Playback::Paused(b))
        | (Playback::Playing(a), Playback::Playing(b)) => (a - b).abs() >= 0.5,
        _ => true,
    }
}

fn key(e: MediaControlEvent) -> Option<Key> {
    let back = |d: SeekDirection| {
        if d == SeekDirection::Backward {
            -1.0
        } else {
            1.0
        }
    };
    Some(match e {
        MediaControlEvent::Play => Key::Play,
        MediaControlEvent::Pause => Key::Pause,
        MediaControlEvent::Toggle => Key::Toggle,
        MediaControlEvent::Next => Key::Next,
        MediaControlEvent::Previous => Key::Previous,
        MediaControlEvent::Stop => Key::Stop,
        MediaControlEvent::Seek(d) => Key::SeekBy(10.0 * back(d)),
        MediaControlEvent::SeekBy(d, by) => Key::SeekBy(by.as_secs_f64() * back(d)),
        MediaControlEvent::SetPosition(MediaPosition(p)) => Key::SeekTo(p.as_secs_f64()),
        MediaControlEvent::Raise => Key::Raise,
        _ => return None,
    })
}

/// The artwork as a `file://` URL: the folder's cover, as the panel shows
/// it, or else the picture in the file, copied out to the data folder (the
/// music file itself is only read).
fn art_url(track: &Path, root: Option<&Path>) -> Option<String> {
    if let Some(p) = crate::tags::folder_art(track, root) {
        return Some(file_url(&p));
    }
    let bytes = crate::tags::embedded_art(track)?;
    let dir = crate::instance::data_dir()?.join("now-playing");
    let _ = std::fs::create_dir_all(&dir);
    // A name per track, so the desktop doesn't keep showing the last one.
    let ext = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "png"
    } else {
        "jpg"
    };
    let name = format!("art-{:016x}.{ext}", hash(track));
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            if e.file_name() != name.as_str() {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let p = dir.join(name);
    std::fs::write(&p, bytes).ok()?;
    Some(file_url(&p))
}

fn hash(p: &Path) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    p.hash(&mut h);
    h.finish()
}

/// `file:///home/me/My%20Music/cover.jpg` on Linux. On Windows the
/// controls read the path straight after `file://`, so it stays as it is.
pub fn file_url(p: &Path) -> String {
    if cfg!(windows) {
        return format!("file://{}", p.display());
    }
    let mut s = String::from("file://");
    for b in p.to_string_lossy().bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
            s.push(b as char);
        } else {
            s += &format!("%{b:02X}");
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(not(windows))]
    fn file_urls_are_escaped() {
        assert_eq!(
            file_url(Path::new("/home/me/My Music/AC#DC/cover.jpg")),
            "file:///home/me/My%20Music/AC%23DC/cover.jpg"
        );
        assert_eq!(
            file_url(Path::new("/m/Björk.png")),
            "file:///m/Bj%C3%B6rk.png"
        );
    }

    #[test]
    fn keys_from_the_desktop() {
        assert_eq!(key(MediaControlEvent::Toggle), Some(Key::Toggle));
        assert_eq!(
            key(MediaControlEvent::SeekBy(
                SeekDirection::Backward,
                Duration::from_secs(5)
            )),
            Some(Key::SeekBy(-5.0))
        );
        assert_eq!(key(MediaControlEvent::SetVolume(0.5)), None);
    }

    #[test]
    fn position_sent_about_twice_a_second() {
        assert!(!moved(Playback::Playing(10.0), Playback::Playing(10.4)));
        assert!(moved(Playback::Playing(10.0), Playback::Playing(10.5)));
        assert!(moved(Playback::Playing(10.0), Playback::Paused(10.0)));
        assert!(!moved(Playback::Stopped, Playback::Stopped));
    }
}
