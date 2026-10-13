//! Turning what the user opens (folders, files, M3U and M3U8 playlists) into
//! a flat list of tracks, in the order they'll play. Nothing here reads
//! audio or tags; it only finds files.

use std::path::{Path, PathBuf};

/// Extensions the Player can decode.
pub const AUDIO: &[&str] = &[
    "flac", "mp3", "wav", "wave", "aif", "aiff", "aifc", "m4a", "mp4", "aac", "alac", "ogg", "oga",
];

pub fn is_audio(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| AUDIO.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

pub fn is_playlist(p: &Path) -> bool {
    matches!(
        p.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("m3u" | "m3u8")
    )
}

/// One entry of a playlist file: the path and the `#EXTINF` title, if any.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub path: PathBuf,
    pub title: Option<String>,
}

/// Parses M3U or M3U8 text. Relative paths are taken from `base` (the
/// playlist's folder). `file://` URLs are understood; other URLs (streams)
/// are skipped, since the Player only plays files.
pub fn parse_m3u(text: &str, base: &Path) -> Vec<Entry> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out = Vec::new();
    let mut title: Option<String> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(info) = line.strip_prefix("#EXTINF:") {
            // #EXTINF:<seconds>,<Artist - Title>
            title = info
                .split_once(',')
                .map(|(_, t)| t.trim().to_string())
                .filter(|t| !t.is_empty());
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        let path = if let Some(rest) = line.strip_prefix("file://") {
            PathBuf::from(file_url_path(rest))
        } else if line.contains("://") {
            title = None;
            continue;
        } else {
            let p = PathBuf::from(native_separators(line));
            if p.is_absolute() || looks_like_windows_absolute(line) {
                p
            } else {
                base.join(p)
            }
        };
        out.push(Entry {
            path,
            title: title.take(),
        });
    }
    out
}

/// `file:///home/me/a%20b.flac` → `/home/me/a b.flac`;
/// `file:///C:/Music/x.flac` → `C:/Music/x.flac`.
fn file_url_path(rest: &str) -> String {
    // Drop the host part ("localhost" or empty).
    let path = match rest.find('/') {
        Some(i) => &rest[i..],
        None => rest,
    };
    let decoded = percent_decode(path);
    // "/C:/Music" on Windows is "C:/Music".
    let b = decoded.as_bytes();
    if b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':' {
        native_separators(&decoded[1..])
    } else {
        native_separators(&decoded)
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = s
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
            {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn looks_like_windows_absolute(s: &str) -> bool {
    let b = s.as_bytes();
    (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/'))
        || s.starts_with("\\\\")
}

/// Playlists written on Windows use backslashes; elsewhere they're slashes.
fn native_separators(s: &str) -> String {
    if cfg!(windows) {
        s.replace('/', "\\")
    } else if looks_like_windows_absolute(s) {
        s.to_string()
    } else {
        s.replace('\\', "/")
    }
}

/// Writes an M3U8 playlist of `paths` (absolute paths, UTF-8), as the
/// Library does.
#[cfg(test)]
pub fn write_m3u8(paths: &[PathBuf]) -> String {
    let mut s = String::from("#EXTM3U\n");
    for p in paths {
        s.push_str(&p.to_string_lossy());
        s.push('\n');
    }
    s
}

/// Compares names the way people expect: "Track 2" before "Track 10".
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (mut ai, mut bi) = (a.chars().peekable(), b.chars().peekable());
    // Case only decides between names that are otherwise the same.
    let mut case = Ordering::Equal;
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return case,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut na = String::new();
                while let Some(c) = ai.peek().copied().filter(char::is_ascii_digit) {
                    na.push(c);
                    ai.next();
                }
                let mut nb = String::new();
                while let Some(c) = bi.peek().copied().filter(char::is_ascii_digit) {
                    nb.push(c);
                    bi.next();
                }
                let (ta, tb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
                let o = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if o != Ordering::Equal {
                    return o;
                }
            }
            (Some(x), Some(y)) => {
                let o = x.to_lowercase().cmp(y.to_lowercase());
                if case == Ordering::Equal {
                    case = x.cmp(&y);
                }
                if o != Ordering::Equal {
                    return o;
                }
                ai.next();
                bi.next();
            }
        }
    }
}

/// Every audio file under `dir`, folders in name order and files in name
/// order within each, so an album split into CD1 and CD2 plays in order.
/// Hidden folders are skipped.
pub fn scan_folder(dir: &Path, subfolders: bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(dir, subfolders, &mut out, 0);
    out
}

fn walk(dir: &Path, subfolders: bool, out: &mut Vec<PathBuf>, depth: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files = Vec::new();
    let mut dirs = Vec::new();
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        match e.file_type() {
            Ok(t) if t.is_dir() => dirs.push((name, p)),
            Ok(_) if is_audio(&p) => files.push((name, p)),
            _ => {}
        }
    }
    files.sort_by(|a, b| natural_cmp(&a.0, &b.0));
    out.extend(files.into_iter().map(|(_, p)| p));
    if subfolders && depth < 32 {
        dirs.sort_by(|a, b| natural_cmp(&a.0, &b.0));
        for (_, d) in dirs {
            walk(&d, subfolders, out, depth + 1);
        }
    }
}

/// What came of opening something: the tracks, and titles a playlist gave.
#[derive(Default, Debug)]
pub struct Opened {
    pub tracks: Vec<Entry>,
    /// Paths that were asked for but aren't there, or aren't audio.
    pub missing: usize,
}

/// Expands a mix of folders, audio files and playlists, in the order given.
pub fn expand(paths: &[PathBuf], subfolders: bool) -> Opened {
    let mut o = Opened::default();
    for p in paths {
        if p.is_dir() {
            o.tracks.extend(
                scan_folder(p, subfolders)
                    .into_iter()
                    .map(|path| Entry { path, title: None }),
            );
        } else if is_playlist(p) {
            match std::fs::read(p) {
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    let base = p.parent().unwrap_or(Path::new("."));
                    for e in parse_m3u(&text, base) {
                        if e.path.is_file() && is_audio(&e.path) {
                            o.tracks.push(e);
                        } else {
                            o.missing += 1;
                        }
                    }
                }
                Err(_) => o.missing += 1,
            }
        } else if p.is_file() && is_audio(p) {
            o.tracks.push(Entry {
                path: p.clone(),
                title: None,
            });
        } else {
            o.missing += 1;
        }
    }
    o
}

/// A small, dependency-free shuffle (xorshift, seeded from the clock).
pub fn shuffle<T>(v: &mut [T], seed: u64) {
    let mut x = seed | 1;
    for i in (1..v.len()).rev() {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let j = (x % (i as u64 + 1)) as usize;
        v.swap(i, j);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn m3u_relative_absolute_and_titles() {
        let text = "\u{feff}#EXTM3U\n#EXTINF:215,Artist - Song\nCD1/01 Song.flac\n\n/abs/02.mp3\n# a comment\nhttp://radio.example/stream\n03.ogg\n";
        let e = parse_m3u(text, Path::new("/music/Album"));
        assert_eq!(e.len(), 3);
        assert_eq!(e[0].path, PathBuf::from("/music/Album/CD1/01 Song.flac"));
        assert_eq!(e[0].title.as_deref(), Some("Artist - Song"));
        assert_eq!(e[1].path, PathBuf::from("/abs/02.mp3"));
        assert_eq!(e[1].title, None);
        // The stream URL is skipped, and its title doesn't leak onto 03.
        assert_eq!(e[2].path, PathBuf::from("/music/Album/03.ogg"));
    }

    #[test]
    fn m3u_file_urls() {
        let e = parse_m3u("file:///home/me/My%20Music/a.flac\n", Path::new("/"));
        assert_eq!(e[0].path, PathBuf::from("/home/me/My Music/a.flac"));
        let e = parse_m3u("file://localhost/srv/x.mp3\n", Path::new("/"));
        assert_eq!(e[0].path, PathBuf::from("/srv/x.mp3"));
    }

    #[cfg(not(windows))]
    #[test]
    fn m3u_windows_paths_on_linux() {
        let e = parse_m3u("Sub\\01.flac\nC:\\Music\\02.flac\n", Path::new("/m"));
        assert_eq!(e[0].path, PathBuf::from("/m/Sub/01.flac"));
        // Can't be found here, but it isn't joined onto the base either.
        assert_eq!(e[1].path, PathBuf::from("C:\\Music\\02.flac"));
    }

    #[test]
    fn write_then_parse() {
        let paths = vec![PathBuf::from("/a/b c.flac"), PathBuf::from("/d/é.mp3")];
        let back: Vec<PathBuf> = parse_m3u(&write_m3u8(&paths), Path::new("/x"))
            .into_iter()
            .map(|e| e.path)
            .collect();
        assert_eq!(back, paths);
    }

    #[test]
    fn natural_order() {
        let mut v = vec!["Track 10", "track 2", "Track 1", "CD2", "CD10", "CD1"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(
            v,
            vec!["CD1", "CD2", "CD10", "Track 1", "track 2", "Track 10"]
        );
    }

    #[test]
    fn audio_extensions() {
        assert!(is_audio(Path::new("x.FLAC")));
        assert!(is_audio(Path::new("x.m4a")));
        assert!(!is_audio(Path::new("cover.jpg")));
        assert!(is_playlist(Path::new("a.M3U8")));
    }

    #[test]
    fn folders_expand_in_order() {
        let dir = std::env::temp_dir().join(format!("taw-player-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for f in [
            "CD2/01.flac",
            "CD1/10.flac",
            "CD1/2.flac",
            "CD1/cover.jpg",
            ".hidden/x.flac",
        ] {
            let p = dir.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, b"").unwrap();
        }
        std::fs::write(dir.join("list.m3u8"), "CD2/01.flac\nmissing.flac\n").unwrap();
        let names = |o: &Opened| -> Vec<String> {
            o.tracks
                .iter()
                .map(|e| {
                    e.path
                        .strip_prefix(&dir)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/")
                })
                .collect()
        };
        let o = expand(std::slice::from_ref(&dir), true);
        assert_eq!(names(&o), vec!["CD1/2.flac", "CD1/10.flac", "CD2/01.flac"]);
        let o = expand(std::slice::from_ref(&dir), false);
        assert!(o.tracks.is_empty());
        let o = expand(&[dir.join("list.m3u8")], true);
        assert_eq!(names(&o), vec!["CD2/01.flac"]);
        assert_eq!(o.missing, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shuffle_keeps_everything() {
        let mut v: Vec<u32> = (0..50).collect();
        shuffle(&mut v, 12345);
        let mut s = v.clone();
        s.sort();
        assert_eq!(s, (0..50).collect::<Vec<_>>());
        assert_ne!(v, s);
    }
}
