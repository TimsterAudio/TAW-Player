//! One Player at a time: a second launch (the Library's Play key, or
//! "Open with" in a file manager) hands its files to the Player that's
//! already running and exits. It's done with files in the Player's data
//! folder, not a socket, so nothing listens on the network.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// `%APPDATA%\Timster's Audio Works\Player` on Windows,
/// `~/.local/share/timsters-audio-works/player` elsewhere.
pub fn data_dir() -> Option<PathBuf> {
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
}

impl Beacon {
    pub fn new(dir: PathBuf) -> Beacon {
        let _ = std::fs::create_dir_all(dir.join("inbox"));
        Beacon { dir }
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

/// True if another Player checked in within the last few seconds.
fn other_running(dir: &Path) -> bool {
    let Ok(s) = std::fs::read_to_string(dir.join("running")) else {
        return false;
    };
    let mut it = s.split_whitespace();
    let pid: u32 = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    let at: u128 = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    pid != std::process::id() && now_ms().saturating_sub(at) < 3_000
}

/// Gives `paths` (to play now if `play`) to a Player that's already running. True if it took them
/// (this launch can then exit).
pub fn hand_over(dir: &Path, paths: &[PathBuf], play: bool) -> bool {
    if paths.is_empty() || !other_running(dir) {
        return false;
    }
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
        assert!(!other_running(&d));
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
        assert!(other_running(&d));
        std::fs::write(d.join("running"), format!("1 {}", now_ms() - 10_000)).unwrap();
        assert!(!other_running(&d));
        let _ = std::fs::remove_dir_all(&d);
    }
}
