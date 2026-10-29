//! One Player at a time: a second launch (the Library's Play key, "Open
//! with" in a file manager, or the family switcher in another app) hands
//! its files, if any, to the Player that's already running, which comes to
//! the front, and exits. It's done with files in the Player's data
//! folder, not a socket, so nothing listens on the network.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// `%APPDATA%\Timster's Audio Works\Player` on Windows,
/// `~/Library/Application Support/Timster's Audio Works/Player` on macOS,
/// `~/.local/share/timsters-audio-works/player` elsewhere.
pub fn data_dir() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        return std::env::var_os("HOME").map(|h| {
            PathBuf::from(h)
                .join("Library/Application Support")
                .join("Timster's Audio Works")
                .join("Player")
        });
    }
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(|a| {
            PathBuf::from(a)
                .join("Timster's Audio Works")
                .join("Player")
        })
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .map(|d| d.join("timsters-audio-works").join("player"))
    }
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// A list handed from one launch to the running Player.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Handover {
    pub paths: Vec<PathBuf>,
    /// `--play`: play it now, whatever the setting says.
    pub play: bool,
}

impl Handover {
    fn parse(text: &str) -> Option<Handover> {
        serde_json::from_str(text).ok().or_else(|| {
            // 0.1.0 wrote just the list, and always played it.
            let paths = serde_json::from_str::<Vec<PathBuf>>(text).ok()?;
            Some(Handover { paths, play: true })
        })
    }
}

/// The running Player's mark: its process id and when it last checked in.
pub struct Beacon {
    dir: PathBuf,
    requests: std::sync::mpsc::Receiver<Handover>,
}

impl Beacon {
    pub fn new(dir: PathBuf) -> Beacon {
        let _ = std::fs::create_dir_all(dir.join("inbox"));
        let (_, requests) = std::sync::mpsc::channel();
        Beacon { dir, requests }
    }

    /// Checks in every second and watches the inbox, on a thread of its
    /// own: a minimised Player doesn't redraw, and would otherwise miss a
    /// hand-over and let a second Player open. Each request brings the
    /// window to the front; its files wait in `requests` for the panel.
    pub fn start(dir: PathBuf, ctx: egui::Context) -> Beacon {
        let mut b = Beacon::new(dir.clone());
        let (tx, rx) = std::sync::mpsc::channel();
        b.requests = rx;
        let _ = std::thread::Builder::new()
            .name("instance".into())
            .spawn(move || {
                let me = Beacon::new(dir);
                let mut last_beat = 0u128;
                loop {
                    let t = now_ms();
                    if t - last_beat >= 1_000 {
                        last_beat = t;
                        me.beat();
                    }
                    let lists = me.take_inbox();
                    if !lists.is_empty() {
                        for h in lists {
                            if tx.send(h).is_err() {
                                return;
                            }
                        }
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                        ctx.request_repaint();
                    }
                    std::thread::sleep(Duration::from_millis(200));
                }
            });
        b
    }

    /// Lists handed over since the last call, oldest first.
    pub fn requests(&self) -> Vec<Handover> {
        self.requests.try_iter().collect()
    }

    /// Says "still running"; call about once a second.
    pub fn beat(&self) {
        let _ = std::fs::write(
            self.dir.join("running"),
            format!("{} {}", std::process::id(), now_ms()),
        );
    }

    /// Lists handed over by later launches, oldest first, each removed as
    /// it's read.
    pub fn take_inbox(&self) -> Vec<Handover> {
        let Ok(rd) = std::fs::read_dir(self.dir.join("inbox")) else {
            return Vec::new();
        };
        let mut files: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "json"))
            .collect();
        files.sort();
        files
            .into_iter()
            .filter_map(|p| {
                let text = std::fs::read_to_string(&p).ok();
                let _ = std::fs::remove_file(&p);
                Handover::parse(&text?)
            })
            .collect()
    }

    pub fn leave(&self) {
        let _ = std::fs::remove_file(self.dir.join("running"));
    }
}

/// The process id of another Player that checked in within the last few
/// seconds.
fn other_running(dir: &Path) -> Option<u32> {
    let s = std::fs::read_to_string(dir.join("running")).ok()?;
    let mut it = s.split_whitespace();
    let pid: u32 = it.next()?.parse().ok()?;
    let at: u128 = it.next()?.parse().ok()?;
    (pid != std::process::id() && now_ms().saturating_sub(at) < 3_000).then_some(pid)
}

/// Windows only lets the app the user is using bring another to the front;
/// this launch is that app for now, so it passes the right on.
#[cfg(windows)]
fn allow_to_front(pid: u32) {
    // SAFETY: a plain Win32 call with no pointers.
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(pid);
    }
}

#[cfg(not(windows))]
fn allow_to_front(_pid: u32) {}

/// Gives `paths` (to play now if `play`) to a Player that's already running. True if it took them
/// (this launch can then exit).
pub fn hand_over(dir: &Path, paths: &[PathBuf], play: bool) -> bool {
    // With no files too: a second launch (the family switcher in another
    // app, a double-click) brings the open Player forward instead of
    // opening another.
    let Some(pid) = other_running(dir) else {
        return false;
    };
    allow_to_front(pid);
    let inbox = dir.join("inbox");
    let _ = std::fs::create_dir_all(&inbox);
    let file = inbox.join(format!("{}-{}.json", now_ms(), std::process::id()));
    let tmp = file.with_extension("tmp");
    let Ok(json) = serde_json::to_string(&Handover {
        paths: paths.to_vec(),
        play,
    }) else {
        return false;
    };
    if std::fs::write(&tmp, json).is_err() || std::fs::rename(&tmp, &file).is_err() {
        return false;
    }
    // The running Player looks twice a second.
    for _ in 0..30 {
        std::thread::sleep(Duration::from_millis(100));
        if !file.exists() {
            return true;
        }
    }
    // It didn't take them: open them here instead.
    std::fs::remove_file(&file).is_err()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inbox_round_trip() {
        let d = crate::decode::tests::temp_dir("instance");
        let b = Beacon::new(d.clone());
        // Our own beacon doesn't count as another Player.
        b.beat();
        assert!(other_running(&d).is_none());
        assert!(!hand_over(&d, &[PathBuf::from("/x.flac")], true));
        // A list dropped in the inbox comes out once, with or without
        // --play; one from 0.1.0 (just the list) plays.
        std::fs::write(
            d.join("inbox/1-1.json"),
            r#"{"paths":["/a.flac"],"play":false}"#,
        )
        .unwrap();
        std::fs::write(d.join("inbox/2-1.json"), r#"["/a.flac","/b"]"#).unwrap();
        let got = b.take_inbox();
        assert_eq!(
            got,
            vec![
                Handover {
                    paths: vec![PathBuf::from("/a.flac")],
                    play: false
                },
                Handover {
                    paths: vec![PathBuf::from("/a.flac"), PathBuf::from("/b")],
                    play: true
                }
            ]
        );
        assert!(b.take_inbox().is_empty());
        // Another Player's fresh beacon does count; a stale one doesn't.
        std::fs::write(d.join("running"), format!("1 {}", now_ms())).unwrap();
        assert_eq!(other_running(&d), Some(1));
        std::fs::write(d.join("running"), format!("1 {}", now_ms() - 10_000)).unwrap();
        assert!(other_running(&d).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }
}
