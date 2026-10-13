//! What's in a file's tags, and its artwork: read-only, on worker threads
//! so a big playlist never holds up the panel.

use egui::{ColorImage, Context};
use lofty::config::ParseOptions;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::picture::PictureType;
use lofty::probe::Probe;
use lofty::tag::{Accessor, Tag};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tags {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub track_no: Option<u32>,
    pub seconds: Option<f64>,
}

fn clean(s: Option<std::borrow::Cow<'_, str>>) -> Option<String> {
    let s = s?.trim().trim_matches('\0').trim().to_string();
    (!s.is_empty()).then_some(s)
}

pub fn read(path: &Path) -> Tags {
    let Ok(file) =
        Probe::open(path).and_then(|p| p.options(ParseOptions::new().read_cover_art(false)).read())
    else {
        return Tags::default();
    };
    let secs = file.properties().duration().as_secs_f64();
    let mut t = Tags {
        seconds: (secs > 0.0).then_some(secs),
        ..Default::default()
    };
    let mut tags: Vec<&Tag> = file.primary_tag().into_iter().collect();
    for x in file.tags() {
        if !tags.iter().any(|y| std::ptr::eq(*y, x)) {
            tags.push(x);
        }
    }
    for x in tags {
        t.title = t.title.or_else(|| clean(x.title()));
        t.artist = t.artist.or_else(|| clean(x.artist()));
        t.album = t.album.or_else(|| clean(x.album()));
        t.track_no = t.track_no.or(x.track()).filter(|&n| n > 0);
    }
    t
}

/// "Artist - Title" from a file name with no tags; Spool did the same.
pub fn from_file_name(path: &Path) -> (Option<String>, String) {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    // Leading track numbers: "01 ", "01. ", "01 - ".
    let trimmed = stem
        .trim_start_matches(|c: char| c.is_ascii_digit())
        .trim_start_matches(['.', ' ', '-', '_'])
        .to_string();
    let base = if trimmed.is_empty() {
        stem.clone()
    } else {
        trimmed
    };
    match base.split_once(" - ") {
        Some((a, t)) if !a.trim().is_empty() && !t.trim().is_empty() => {
            (Some(a.trim().to_string()), t.trim().to_string())
        }
        _ => (None, base),
    }
}

/// Reads tags for playlist rows in the background.
pub struct TagReader {
    tx: Sender<(u64, usize, PathBuf)>,
    rx: Receiver<(u64, usize, Tags)>,
}

impl TagReader {
    pub fn start(ctx: Context) -> TagReader {
        let (tx, jobs) = channel::<(u64, usize, PathBuf)>();
        let (done, rx) = channel();
        let _ = std::thread::Builder::new()
            .name("tags".into())
            .spawn(move || {
                let mut n = 0u32;
                while let Ok((generation, i, p)) = jobs.recv() {
                    if done.send((generation, i, read(&p))).is_err() {
                        break;
                    }
                    n += 1;
                    if n.is_multiple_of(25) {
                        ctx.request_repaint();
                    }
                }
            });
        TagReader { tx, rx }
    }

    /// Asks for the tags of `paths`; `generation` tells answers for an old
    /// list from the current one.
    pub fn request(&self, generation: u64, paths: &[PathBuf]) {
        for (i, p) in paths.iter().enumerate() {
            let _ = self.tx.send((generation, i, p.clone()));
        }
    }

    pub fn request_one(&self, generation: u64, i: usize, path: PathBuf) {
        let _ = self.tx.send((generation, i, path));
    }

    pub fn results(&self) -> impl Iterator<Item = (u64, usize, Tags)> + '_ {
        self.rx.try_iter()
    }
}

// ---------------------------------------------------------------- artwork

const COVER_NAMES: [&str; 4] = ["cover", "front", "folder", "album"];

/// A cover image in the track's folder, then its parents up to `root`.
pub fn folder_art(track: &Path, root: Option<&Path>) -> Option<PathBuf> {
    let mut dir = track.parent();
    let mut steps = 0;
    while let Some(d) = dir {
        if let Ok(rd) = std::fs::read_dir(d) {
            let mut found: Vec<(usize, PathBuf)> = rd
                .flatten()
                .filter_map(|e| {
                    let p = e.path();
                    let stem = p.file_stem()?.to_str()?.to_ascii_lowercase();
                    let ext = p.extension()?.to_str()?.to_ascii_lowercase();
                    let rank = COVER_NAMES.iter().position(|n| *n == stem)?;
                    matches!(ext.as_str(), "jpg" | "jpeg" | "png").then_some((rank, p))
                })
                .collect();
            found.sort();
            if let Some((_, p)) = found.into_iter().next() {
                return Some(p);
            }
        }
        if Some(d) == root || steps >= 3 {
            break;
        }
        dir = d.parent();
        steps += 1;
    }
    None
}

pub fn embedded_art(path: &Path) -> Option<Vec<u8>> {
    let file = Probe::open(path)
        .and_then(|p| p.options(ParseOptions::new().read_cover_art(true)).read())
        .ok()?;
    let mut best: Option<Vec<u8>> = None;
    for t in file.tags() {
        let pics = t.pictures();
        if let Some(p) = pics
            .iter()
            .find(|p| p.pic_type() == PictureType::CoverFront)
            .or(pics.first())
        {
            if p.pic_type() == PictureType::CoverFront || best.is_none() {
                best = Some(p.data().to_vec());
            }
        }
    }
    best
}

/// Loads the current track's artwork: the folder's cover first (as Spool
/// does), then the picture in the file.
pub struct ArtLoader {
    tx: Sender<(PathBuf, Option<PathBuf>)>,
    rx: Receiver<(PathBuf, Option<ColorImage>)>,
    cache: HashMap<PathBuf, Option<egui::TextureHandle>>,
    asked: Option<PathBuf>,
}

impl ArtLoader {
    pub fn start(ctx: Context) -> ArtLoader {
        let (tx, jobs) = channel::<(PathBuf, Option<PathBuf>)>();
        let (done, rx) = channel();
        let _ = std::thread::Builder::new()
            .name("artwork".into())
            .spawn(move || {
                while let Ok((track, root)) = jobs.recv() {
                    let bytes = folder_art(&track, root.as_deref())
                        .and_then(|p| std::fs::read(p).ok())
                        .or_else(|| embedded_art(&track));
                    let img = bytes.and_then(|b| decode(&b));
                    if done.send((track, img)).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            });
        ArtLoader {
            tx,
            rx,
            cache: HashMap::new(),
            asked: None,
        }
    }

    /// The artwork for `track`, once loaded. Only a few are kept.
    pub fn get(
        &mut self,
        ctx: &Context,
        track: &Path,
        root: Option<&Path>,
    ) -> Option<egui::TextureHandle> {
        while let Ok((p, img)) = self.rx.try_recv() {
            let tex =
                img.map(|i| ctx.load_texture(p.to_string_lossy(), i, egui::TextureOptions::LINEAR));
            if self.cache.len() > 16 {
                self.cache.clear();
            }
            self.cache.insert(p, tex);
        }
        if let Some(t) = self.cache.get(track) {
            return t.clone();
        }
        if self.asked.as_deref() != Some(track) {
            self.asked = Some(track.to_path_buf());
            let _ = self
                .tx
                .send((track.to_path_buf(), root.map(Path::to_path_buf)));
        }
        None
    }
}

fn decode(bytes: &[u8]) -> Option<ColorImage> {
    let img = image::load_from_memory(bytes).ok()?;
    let img = img.thumbnail(640, 640).to_rgba8();
    Some(ColorImage::from_rgba_unmultiplied(
        [img.width() as usize, img.height() as usize],
        img.as_raw(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_without_tags() {
        assert_eq!(
            from_file_name(Path::new("/m/03 - Artist - Song.flac")),
            (Some("Artist".into()), "Song".into())
        );
        assert_eq!(
            from_file_name(Path::new("/m/07. Song.mp3")),
            (None, "Song".into())
        );
        assert_eq!(
            from_file_name(Path::new("/m/1999.mp3")),
            (None, "1999".into())
        );
    }

    #[test]
    fn cover_in_a_parent_folder() {
        let d = crate::decode::tests::temp_dir("art");
        std::fs::create_dir_all(d.join("Album/CD1")).unwrap();
        std::fs::write(d.join("Album/Folder.JPG"), b"x").unwrap();
        std::fs::write(d.join("Album/cover.png"), b"x").unwrap();
        let track = d.join("Album/CD1/01.flac");
        // "cover" ranks above "folder".
        assert_eq!(
            folder_art(&track, Some(&d)),
            Some(d.join("Album/cover.png"))
        );
        // Never above the opened folder.
        assert_eq!(folder_art(&track, Some(&d.join("Album/CD1"))), None);
        let _ = std::fs::remove_dir_all(&d);
    }
}
