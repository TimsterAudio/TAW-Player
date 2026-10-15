//! The Player's panel, in the TAW look: the nameplate and family switcher
//! along the top, the playing track (artwork, tags, what the signal path is
//! doing) on the left, the display (time, meters, spectrum) and the playlist
//! on the right, and the transport along the bottom like a front panel.

use crate::engine::{locate, Cmd, Engine, Mark, OutputInfo};
use crate::family::{self, App};
use crate::instance::{self, Beacon};
use crate::media::{self, Media};
use crate::playlist::{self, Entry};
use crate::tags::{self, ArtLoader, TagReader, Tags};
use egui::{
    Align2, Color32, CornerRadius, CursorIcon, Id, Key, Pos2, Rect, Response, Sense, Shape, Stroke,
    StrokeKind, Ui, Vec2, ViewportCommand,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use taw_panel::analyzer::{Analyzer, Ballistics};
use taw_panel::hifi::{self, Tokens};
use taw_panel::lists;
use taw_panel::theme::PanelKind;
use taw_panel::widgets::Spec;

const STORAGE_KEY: &str = "taw-player";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const DEFAULT_SIZE: Vec2 = Vec2::new(1280.0, 800.0);
/// Below this size the panel scales down; above it the list gets the room.
const MIN_DESIGN: Vec2 = Vec2::new(1180.0, 720.0);
const PLATE_H: f32 = 66.0;
/// The middle of the nameplate's row of keys (family, view, settings, and
/// the list's keys over the right column), just under the window buttons.
const ROW_Y: f32 = 46.0;
/// Under the artwork: the track, what's happening to it, and the list's
/// size or the latest message.
const UNDER_ART: f32 = 230.0;
/// The artwork stops growing here.
const ART_MAX: f32 = 1000.0;
const BAR_H: f32 = 96.0;
const DISPLAY_H: f32 = 112.0;
const ROW_H: f32 = 30.0;

/// Everything remembered between runs.
#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    /// The Jupiter panel before the TAW look. Kept so older settings load.
    #[allow(dead_code)]
    panel: PanelKind,
    /// Volume in dB; 0 leaves the samples untouched.
    volume_db: f32,
    /// Output device by name; None follows the system default.
    device: Option<String>,
    subfolders: bool,
    spectrum: bool,
    /// The display shows time remaining rather than elapsed.
    remain: bool,
    /// Where the open dialogs start.
    last_dir: Option<PathBuf>,
    /// Where the user pointed the Player to family apps it couldn't find.
    family: HashMap<String, PathBuf>,
    /// Finish, light or dark, and accent.
    finish: hifi::Look,
    /// Simple view: no display window and no numbers; the list gets the room.
    simple: bool,
    /// Use the system's title bar instead of the Player's own.
    system_title_bar: bool,
    /// Opening music (a folder, files, a drop) starts it playing. Off: the
    /// list fills and waits. `--play` (the Library) always plays.
    play_on_open: bool,
    /// The meters in the classic green, yellow and red, or in the
    /// display's own colour.
    meter_colours: hifi::MeterColours,
}

impl Default for Saved {
    fn default() -> Self {
        Saved {
            panel: PanelKind::Black,
            volume_db: 0.0,
            device: None,
            subfolders: true,
            spectrum: true,
            remain: false,
            last_dir: None,
            family: HashMap::new(),
            finish: hifi::Look::default(),
            simple: false,
            system_title_bar: false,
            play_on_open: false,
            meter_colours: hifi::MeterColours::Classic,
        }
    }
}

const VOLUME: Spec = Spec {
    min: -60.0,
    max: 0.0,
    default: 0.0,
    log: false,
    step: 0.5,
};

struct Row {
    path: PathBuf,
    /// The title a playlist gave it.
    list_title: Option<String>,
    tags: Option<Tags>,
    ext: String,
}

impl Row {
    fn new(e: Entry) -> Row {
        let ext = e
            .path
            .extension()
            .map(|x| x.to_string_lossy().to_uppercase())
            .unwrap_or_default();
        Row {
            path: e.path,
            list_title: e.title,
            tags: None,
            ext,
        }
    }

    fn title(&self) -> String {
        if let Some(t) = self.tags.as_ref().and_then(|t| t.title.clone()) {
            return t;
        }
        if let Some(t) = &self.list_title {
            return t.clone();
        }
        tags::from_file_name(&self.path).1
    }

    fn artist(&self) -> String {
        self.tags
            .as_ref()
            .and_then(|t| t.artist.clone())
            .or_else(|| {
                if self
                    .tags
                    .as_ref()
                    .map(|t| t.title.is_none())
                    .unwrap_or(true)
                {
                    tags::from_file_name(&self.path).0
                } else {
                    None
                }
            })
            .unwrap_or_default()
    }

    fn album(&self) -> String {
        self.tags
            .as_ref()
            .and_then(|t| t.album.clone())
            .unwrap_or_default()
    }

    fn seconds(&self) -> Option<f64> {
        self.tags.as_ref().and_then(|t| t.seconds)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    Stopped,
    Playing,
    Paused,
}

pub struct PlayerApp {
    s: Saved,
    engine: Engine,
    rows: Vec<Row>,
    generation: u64,
    /// The folder that was opened, so cover art isn't looked for above it.
    root: Option<PathBuf>,
    selected: Option<usize>,
    tag_reader: TagReader,
    art: ArtLoader,
    analyzer: Analyzer,
    meter_db: [f32; 2],
    marks: Vec<Mark>,
    output: Option<OutputInfo>,
    devices: Vec<String>,
    message: Option<(String, f64)>,
    beacon: Option<Beacon>,
    last_beat: f64,
    last_inbox: f64,
    last_index: Option<usize>,
    scroll_to: Option<usize>,
    user_scrolled: f64,
    /// While the position bar is dragged: where to, 0..1.
    seeking: Option<f32>,
    found: HashMap<&'static str, Option<PathBuf>>,
    /// A family key pressed when its app wasn't found; pressed again soon,
    /// it asks where the app is.
    locate: Option<(App, f64)>,
    settings_open: bool,
    /// Media keys and the desktop's media controls, if there are any.
    media: Option<Media>,
}

fn now(ui: &Ui) -> f64 {
    ui.input(|i| i.time)
}

pub fn rate_label(hz: u32) -> String {
    if hz.is_multiple_of(1000) {
        format!("{} kHz", hz / 1000)
    } else {
        let k = hz as f64 / 1000.0;
        let s = format!("{k:.1}");
        format!("{} kHz", s.trim_end_matches(".0"))
    }
}

pub fn time_label(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

fn db_to_gain(db: f32) -> f32 {
    if db >= 0.0 {
        1.0
    } else {
        10f32.powf(db / 20.0)
    }
}

impl PlayerApp {
    pub fn new(cc: &eframe::CreationContext<'_>, open: Vec<PathBuf>, play: bool) -> Self {
        hifi::install_fonts(&cc.egui_ctx);
        let s: Saved = cc
            .storage
            .and_then(|st| eframe::get_value(st, STORAGE_KEY))
            .unwrap_or_default();
        let engine = Engine::start(s.device.clone(), db_to_gain(s.volume_db));
        let beacon = instance::data_dir().map(Beacon::new);
        if let Some(b) = &beacon {
            b.beat();
        }
        let mut analyzer = Analyzer::default();
        analyzer.hop = 1024;
        analyzer.ballistics = Ballistics {
            rise_ms: 0.0,
            fall_ms: 300.0,
            hold_s: 0.0,
            peak_fall_db_s: 20.0,
        };
        let mut app = PlayerApp {
            s,
            engine,
            rows: Vec::new(),
            generation: 0,
            root: None,
            selected: None,
            tag_reader: TagReader::start(cc.egui_ctx.clone()),
            art: ArtLoader::start(cc.egui_ctx.clone()),
            analyzer,
            meter_db: [-90.0; 2],
            marks: Vec::new(),
            output: None,
            devices: Vec::new(),
            message: None,
            beacon,
            last_beat: 0.0,
            last_inbox: 0.0,
            last_index: None,
            scroll_to: None,
            user_scrolled: -100.0,
            seeking: None,
            found: HashMap::new(),
            locate: None,
            settings_open: false,
            media: Media::start(cc.egui_ctx.clone(), window_handle(cc)),
        };
        if app.s.system_title_bar {
            cc.egui_ctx
                .send_viewport_cmd(ViewportCommand::Decorations(true));
        }
        app.refresh_family();
        if !open.is_empty() {
            let play = play || app.s.play_on_open;
            app.load(&open, play, 0.0);
        }
        app
    }

    fn refresh_family(&mut self) {
        for a in App::ALL {
            let f = family::find(
                a.program(),
                self.s.family.get(a.name()).map(PathBuf::as_path),
            );
            self.found.insert(a.name(), f);
        }
    }

    fn tell(&mut self, msg: impl Into<String>, t: f64) {
        self.message = Some((msg.into(), t));
    }

    // ------------------------------------------------------------ actions

    /// Opens folders, files or playlists in place of the current list.
    fn load(&mut self, paths: &[PathBuf], play: bool, t: f64) {
        let opened = playlist::expand(paths, self.s.subfolders);
        if opened.tracks.is_empty() {
            self.tell(
                if paths.len() == 1 && paths[0].is_dir() {
                    "There's no music the Player can play in that folder.".to_string()
                } else {
                    "Nothing there the Player can play.".to_string()
                },
                t,
            );
            return;
        }
        self.engine.send(Cmd::Stop);
        self.root = match paths {
            [p] if p.is_dir() => Some(p.clone()),
            _ => opened.tracks[0].path.parent().map(Path::to_path_buf),
        };
        if let Some(d) = paths.first().and_then(|p| {
            if p.is_dir() {
                Some(p.clone())
            } else {
                p.parent().map(Path::to_path_buf)
            }
        }) {
            self.s.last_dir = Some(d);
        }
        self.rows = opened.tracks.into_iter().map(Row::new).collect();
        self.generation += 1;
        let list: Vec<PathBuf> = self.rows.iter().map(|r| r.path.clone()).collect();
        self.tag_reader.request(self.generation, &list);
        self.engine.send(Cmd::Queue(list));
        self.selected = Some(0);
        self.scroll_to = Some(0);
        let n = self.rows.len();
        let mut msg = format!("{n} track{}", if n == 1 { "" } else { "s" });
        if opened.missing > 0 {
            msg += &format!(" ({} in the playlist couldn't be found)", opened.missing);
        }
        self.tell(msg, t);
        if play {
            self.engine.send(Cmd::Play(0));
        }
    }

    fn open_folder(&mut self, t: f64) {
        let mut d = rfd::FileDialog::new().set_title("Open a folder of music");
        if let Some(dir) = &self.s.last_dir {
            d = d.set_directory(dir);
        }
        if let Some(dir) = d.pick_folder() {
            self.load(&[dir], self.s.play_on_open, t);
        }
    }

    fn open_files(&mut self, t: f64) {
        let mut exts: Vec<&str> = playlist::AUDIO.to_vec();
        exts.extend(["m3u", "m3u8"]);
        let mut d = rfd::FileDialog::new()
            .set_title("Open music files or a playlist")
            .add_filter("Music and playlists", &exts);
        if let Some(dir) = &self.s.last_dir {
            d = d.set_directory(dir);
        }
        if let Some(files) = d.pick_files() {
            self.load(&files, self.s.play_on_open, t);
        }
    }

    /// Adds folders, files or playlists to the end of the list. If the
    /// list is still playing, it plays on into them without a gap.
    fn add(&mut self, paths: &[PathBuf], t: f64) {
        if self.rows.is_empty() {
            self.load(paths, self.s.play_on_open, t);
            return;
        }
        let opened = playlist::expand(paths, self.s.subfolders);
        if opened.tracks.is_empty() {
            self.tell("Nothing there the Player can play.", t);
            return;
        }
        if let Some(d) = paths.first().and_then(|p| {
            if p.is_dir() {
                Some(p.clone())
            } else {
                p.parent().map(Path::to_path_buf)
            }
        }) {
            self.s.last_dir = Some(d);
        }
        let first = self.rows.len();
        let n = opened.tracks.len();
        let list: Vec<PathBuf> = opened.tracks.iter().map(|e| e.path.clone()).collect();
        for (k, p) in list.iter().enumerate() {
            self.tag_reader
                .request_one(self.generation, first + k, p.clone());
        }
        self.rows.extend(opened.tracks.into_iter().map(Row::new));
        self.engine.send(Cmd::Append(list));
        let mut msg = format!("Added {n} track{}", if n == 1 { "" } else { "s" });
        if opened.missing > 0 {
            msg += &format!(" ({} in the playlist couldn't be found)", opened.missing);
        }
        self.tell(msg, t);
    }

    fn add_folder(&mut self, t: f64) {
        let mut d = rfd::FileDialog::new().set_title("Add a folder of music to the list");
        if let Some(dir) = &self.s.last_dir {
            d = d.set_directory(dir);
        }
        if let Some(dir) = d.pick_folder() {
            self.add(&[dir], t);
        }
    }

    fn add_files(&mut self, t: f64) {
        let mut exts: Vec<&str> = playlist::AUDIO.to_vec();
        exts.extend(["m3u", "m3u8"]);
        let mut d = rfd::FileDialog::new()
            .set_title("Add music files or a playlist to the list")
            .add_filter("Music and playlists", &exts);
        if let Some(dir) = &self.s.last_dir {
            d = d.set_directory(dir);
        }
        if let Some(files) = d.pick_files() {
            self.add(&files, t);
        }
    }

    fn clear(&mut self) {
        self.engine.send(Cmd::Stop);
        self.engine.send(Cmd::Queue(Vec::new()));
        self.rows.clear();
        self.generation += 1;
        self.selected = None;
        self.root = None;
    }

    /// Spool's random play: shuffle the list and play it from the top.
    fn random(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1);
        playlist::shuffle(&mut self.rows, seed);
        self.generation += 1;
        let untagged: Vec<(usize, PathBuf)> = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.tags.is_none())
            .map(|(i, r)| (i, r.path.clone()))
            .collect();
        // Rows still waiting for tags: ask again under the new order.
        for (i, p) in untagged {
            self.tag_reader.request_one(self.generation, i, p);
        }
        let list: Vec<PathBuf> = self.rows.iter().map(|r| r.path.clone()).collect();
        self.engine.send(Cmd::Queue(list));
        self.engine.send(Cmd::Play(0));
        self.selected = Some(0);
        self.scroll_to = Some(0);
    }

    /// Spool's back-to-start key: the first track of the list. Playing, it
    /// plays it; stopped, it chooses it.
    fn back_to_start(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        if self.state() == State::Stopped {
            self.selected = Some(0);
        } else {
            self.play(0);
        }
        self.scroll_to = Some(0);
    }

    fn play(&mut self, i: usize) {
        if i < self.rows.len() {
            self.engine.send(Cmd::Play(i));
        }
    }

    /// The track that's playing (or paused), and how far in.
    fn now_playing(&self) -> Option<(usize, f64, &Mark)> {
        let (m, secs) = locate(&self.marks, self.engine.played())?;
        Some((m.index?, secs, m))
    }

    fn state(&self) -> State {
        match self.now_playing() {
            None => State::Stopped,
            Some(_)
                if self
                    .engine
                    .shared
                    .paused
                    .load(std::sync::atomic::Ordering::Relaxed) =>
            {
                State::Paused
            }
            Some(_) => State::Playing,
        }
    }

    fn play_pause(&mut self) {
        match self.state() {
            State::Stopped => {
                let i = self.selected.unwrap_or(0);
                self.play(i);
            }
            State::Playing => self.engine.send(Cmd::Pause(true)),
            State::Paused => self.engine.send(Cmd::Pause(false)),
        }
    }

    fn next(&mut self) {
        if let Some((i, _, _)) = self.now_playing() {
            if i + 1 < self.rows.len() {
                self.play(i + 1);
            }
        } else if let Some(s) = self.selected {
            self.selected = Some((s + 1).min(self.rows.len().saturating_sub(1)));
        }
    }

    fn previous(&mut self) {
        if let Some((i, secs, _)) = self.now_playing() {
            if secs > 3.0 || i == 0 {
                self.engine.send(Cmd::Seek(0.0));
            } else {
                self.play(i - 1);
            }
        } else if let Some(s) = self.selected {
            self.selected = Some(s.saturating_sub(1));
        }
    }

    fn seek_by(&mut self, d: f64) {
        if let Some((i, secs, m)) = self.now_playing() {
            let len = m
                .info
                .seconds()
                .or_else(|| self.rows.get(i).and_then(Row::seconds))
                .unwrap_or(f64::MAX);
            self.engine
                .send(Cmd::Seek((secs + d).clamp(0.0, (len - 1.0).max(0.0))));
        }
    }

    fn start_family(&mut self, a: App, t: f64) {
        let remembered = self.s.family.get(a.name()).cloned();
        let exe = family::find(a.program(), remembered.as_deref());
        self.found.insert(a.name(), exe.clone());
        match exe {
            Some(exe) => match family::launch(&exe, &[]) {
                Ok(()) => self.tell(format!("Starting the {}…", a.name()), t),
                Err(e) => self.tell(format!("The {} wouldn't start: {e}", a.name()), t),
            },
            None => {
                if matches!(self.locate, Some((b, at)) if b == a && t - at < 15.0) {
                    self.locate = None;
                    let mut d =
                        rfd::FileDialog::new().set_title(format!("Where is the {}?", a.name()));
                    if cfg!(windows) {
                        d = d.add_filter(a.name(), &["exe"]);
                    }
                    if let Some(p) = d.pick_file() {
                        self.s.family.insert(a.name().to_string(), p.clone());
                        self.found.insert(a.name(), Some(p.clone()));
                        let _ = family::launch(&p, &[]);
                        self.tell(format!("Starting the {}…", a.name()), t);
                    }
                } else {
                    self.locate = Some((a, t));
                    self.tell(
                        format!(
                            "The {} isn't installed here (no {} next to the Player or on the PATH). Click it again to show the Player where it is.",
                            a.name(),
                            family::exe_name(a.program()),
                        ),
                        t,
                    );
                }
            }
        }
    }

    // ------------------------------------------------------------ polling

    fn poll(&mut self, ui: &Ui) {
        let t = now(ui);
        if let Ok(mut st) = self.engine.shared.status.try_lock() {
            self.marks.clone_from(&st.marks);
            self.output.clone_from(&st.output);
            if !st.devices.is_empty() {
                self.devices = std::mem::take(&mut st.devices);
            }
            if let Some(m) = st.message.take() {
                self.message = Some((m, t));
            }
            if let Some(rx) = st.analyzer.take() {
                self.analyzer.connect(rx);
            }
        }
        if let Some(o) = &self.output {
            self.analyzer.sample_rate = o.rate as f32;
        }
        self.analyzer.update();

        for (generation, i, tags) in self.tag_reader.results() {
            if generation == self.generation {
                if let Some(r) = self.rows.get_mut(i) {
                    r.tags = Some(tags);
                }
            }
        }

        if let Some(b) = &self.beacon {
            if t - self.last_beat > 1.0 {
                self.last_beat = t;
                b.beat();
            }
            if t - self.last_inbox > 0.5 {
                self.last_inbox = t;
                let lists = b.take_inbox();
                if let Some(h) = lists.into_iter().last() {
                    let play = h.play || self.s.play_on_open;
                    self.load(&h.paths, play, t);
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Focus);
                }
            }
        }

        // Files dropped on the window.
        let dropped: Vec<PathBuf> = ui.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .collect()
        });
        if !dropped.is_empty() {
            // Shift while dropping adds to the list.
            if ui.input(|i| i.modifiers.shift) {
                self.add(&dropped, t);
            } else {
                self.load(&dropped, self.s.play_on_open, t);
            }
        }

        // Meters: instant rise, a steady fall; at rest when paused.
        let dt = ui.input(|i| i.stable_dt).min(0.1);
        let peaks = self.engine.shared.take_peaks();
        let state = self.state();
        for (c, p) in peaks.iter().enumerate() {
            let db = if *p > 0.0 { 20.0 * p.log10() } else { -90.0 };
            let fall = self.meter_db[c] - 30.0 * dt;
            self.meter_db[c] = if state == State::Playing {
                db.max(fall)
            } else {
                -90.0
            };
        }

        self.media_keys(ui);

        // Follow the playing track, unless the list was just scrolled.
        let idx = self.now_playing().map(|(i, _, _)| i);
        if idx != self.last_index {
            self.last_index = idx;
            if let Some(i) = idx {
                if t - self.user_scrolled > 4.0 {
                    self.scroll_to = Some(i);
                    self.selected = Some(i);
                }
            }
        }
    }

    /// The keyboard's media keys and the desktop's sound menu; and telling
    /// the desktop what's playing.
    fn media_keys(&mut self, ui: &Ui) {
        let Some(m) = &self.media else { return };
        for k in m.keys() {
            let state = self.state();
            match k {
                media::Key::Play if state != State::Playing => self.play_pause(),
                media::Key::Pause if state == State::Playing => self.play_pause(),
                media::Key::Toggle => self.play_pause(),
                media::Key::Next => self.next(),
                media::Key::Previous => self.previous(),
                media::Key::Stop => self.engine.send(Cmd::Stop),
                media::Key::SeekBy(d) => self.seek_by(d),
                media::Key::SeekTo(secs) => {
                    if state != State::Stopped {
                        self.engine.send(Cmd::Seek(secs.max(0.0)));
                    }
                }
                media::Key::Raise => ui.ctx().send_viewport_cmd(ViewportCommand::Focus),
                _ => {}
            }
        }
        let np = self.now_playing().map(|(i, secs, m)| (i, secs, m.clone()));
        let track = np.as_ref().and_then(|(i, _, m)| {
            let r = self.rows.get(*i)?;
            Some(media::Track {
                path: r.path.clone(),
                title: r.title(),
                artist: Some(r.artist()).filter(|a| !a.is_empty()),
                album: Some(r.album()).filter(|a| !a.is_empty()),
                seconds: m.info.seconds().or_else(|| r.seconds()),
            })
        });
        let playback = match (self.state(), &np) {
            (State::Playing, Some((_, secs, _))) => media::Playback::Playing(*secs),
            (State::Paused, Some((_, secs, _))) => media::Playback::Paused(*secs),
            _ => media::Playback::Stopped,
        };
        let root = self.root.clone();
        if let Some(m) = self.media.as_mut() {
            m.update(track, playback, root.as_deref());
        }
    }

    fn keys(&mut self, ui: &Ui) {
        if ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        let (space, next, prev, fwd, back, up, down, enter, home) = ui.input_mut(|i| {
            let shift = egui::Modifiers::SHIFT;
            (
                i.consume_key(egui::Modifiers::NONE, Key::Space),
                i.consume_key(shift, Key::ArrowRight),
                i.consume_key(shift, Key::ArrowLeft),
                i.consume_key(egui::Modifiers::NONE, Key::ArrowRight),
                i.consume_key(egui::Modifiers::NONE, Key::ArrowLeft),
                i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
                i.consume_key(egui::Modifiers::NONE, Key::Enter),
                i.consume_key(egui::Modifiers::NONE, Key::Home),
            )
        });
        if home {
            self.back_to_start();
        }
        if space {
            self.play_pause();
        }
        if next {
            self.next();
        }
        if prev {
            self.previous();
        }
        if fwd {
            self.seek_by(10.0);
        }
        if back {
            self.seek_by(-10.0);
        }
        if (up || down) && !self.rows.is_empty() {
            let s = self.selected.unwrap_or(0) as i64 + if down { 1 } else { -1 };
            let s = s.clamp(0, self.rows.len() as i64 - 1) as usize;
            self.selected = Some(s);
            self.scroll_to = Some(s);
            self.user_scrolled = now(ui);
        }
        if enter {
            if let Some(s) = self.selected {
                self.play(s);
            }
        }
    }

    // ------------------------------------------------------------ what's playing, in words

    /// The track's format: the codec, then bit depth, rate and channels.
    fn format_chips(&self, i: usize, m: &Mark) -> (String, String) {
        let mut fmt = String::new();
        // ALAC in M4A only gives its bit depth in the file's header.
        let bits = m.info.bits.or_else(|| {
            (m.info.codec == "ALAC")
                .then(|| self.rows.get(i).and_then(|r| r.tags.as_ref()?.bits))
                .flatten()
                .map(u32::from)
        });
        if let Some(b) = bits {
            fmt += &format!("{b}-bit · ");
        }
        fmt += &rate_label(m.info.rate);
        fmt += match m.info.channels {
            1 => " · mono",
            2 => "",
            _ => " · multichannel",
        };
        (m.info.codec.clone(), fmt)
    }

    /// What the Player does to the signal: a light and a line. Simple says
    /// it in plain words; Detailed gives the numbers. Neither claims more
    /// than the Player knows.
    fn signal_path(&self, i: usize, m: &Mark, simple: bool) -> (bool, String) {
        let lowered = self.engine.shared.gain() != 1.0;
        let paused = self.state() == State::Paused;
        if simple {
            let bits = m.info.bits.or_else(|| {
                self.rows
                    .get(i)
                    .and_then(|r| r.tags.as_ref()?.bits)
                    .map(u32::from)
            });
            let lossy = matches!(
                m.info.codec.as_str(),
                "MP3" | "AAC" | "Vorbis" | "Opus" | "MP2" | "MP1"
            );
            let quality = if lossy {
                "compressed"
            } else if m.info.rate > 48_000 || bits.is_some_and(|b| b > 16) {
                "high resolution"
            } else if m.info.rate == 44_100 && bits == Some(16) {
                "CD quality"
            } else {
                "lossless"
            };
            let mut s = String::from(if paused { "Paused" } else { "Playing" });
            if m.resampled {
                s += " · converted for this output";
            } else {
                s += " as recorded";
            }
            s += " · ";
            s += quality;
            if lowered {
                s += " · volume lowered";
            }
            (!m.resampled, s)
        } else {
            let mut s = if m.resampled {
                format!("Resampled to {} for this output", rate_label(m.out_rate))
            } else if lowered {
                format!("Native rate · volume {:.1} dB", self.s.volume_db)
            } else {
                "Native rate, untouched".to_string()
            };
            if m.resampled && lowered {
                s += &format!(" · volume {:.1} dB", self.s.volume_db);
            }
            (!m.resampled, s)
        }
    }

    fn output_line(&self) -> String {
        match &self.output {
            Some(o) => format!(
                "Out: {} · {} · {} · {} ch · {}",
                o.host,
                if o.device.starts_with("default") {
                    "System default"
                } else {
                    &o.device
                },
                rate_label(o.rate),
                o.channels,
                o.format
            ),
            None => "Output closed".to_string(),
        }
    }

    /// Gaps in the sound because the output ran dry, if there were any.
    fn dropouts(&self) -> Option<String> {
        let n = self
            .engine
            .shared
            .underruns
            .load(std::sync::atomic::Ordering::Relaxed);
        (n > 0).then(|| format!("{n} dropout{} so far", if n == 1 { "" } else { "s" }))
    }

    // ------------------------------------------------------------ top

    /// The nameplate and its row of keys. Returns where the family
    /// switcher begins, so the list's keys can stop short of it.
    fn plate(&mut self, ui: &mut Ui, screen: Rect, t: &Tokens) -> f32 {
        let now = now(ui);
        let plate = Rect::from_min_size(screen.min, Vec2::new(screen.width(), PLATE_H));
        if !self.s.system_title_bar {
            hifi::drag_to_move(ui, plate);
        }
        hifi::nameplate(
            ui,
            screen.min + Vec2::new(24.0, 10.0),
            t,
            "Player",
            &format!("PL-001 · Music player · V{VERSION}"),
        );
        if !self.s.system_title_bar {
            hifi::window_buttons(ui, Pos2::new(screen.right() - 10.0, screen.top() + 6.0), t);
        }
        let right = screen.right() - 24.0;
        let row = screen.top() + ROW_Y;
        let gear = Rect::from_min_size(Pos2::new(right - 30.0, row - 15.0), Vec2::splat(30.0));
        if hifi::gear_button(ui, gear, Id::new("settings"), t, self.settings_open).clicked() {
            self.settings_open = !self.settings_open;
        }
        let (seg, picked) = hifi::segmented(
            ui,
            Pos2::new(gear.left() - 12.0, row),
            true,
            Id::new("simple"),
            t,
            &["Simple", "Detailed"],
            if self.s.simple { 0 } else { 1 },
            hifi::Seg::Plain,
            &[],
            &[],
        );
        if let Some(i) = picked {
            self.s.simple = i == 0;
        }
        let found = |a: App| self.found.get(a.name()).is_some_and(Option::is_some);
        let installed = [
            found(App::Library),
            true,
            found(App::Equaliser),
            found(App::Analyser),
        ];
        let (fam, picked) =
            hifi::family_switch(ui, Pos2::new(seg.left() - 14.0, row), t, 1, installed);
        let app = match picked {
            Some(0) => Some(App::Library),
            Some(2) => Some(App::Equaliser),
            Some(3) => Some(App::Analyser),
            _ => None,
        };
        if let Some(a) = app {
            self.start_family(a, now);
        }
        fam.left()
    }

    // ------------------------------------------------------------ left column

    fn left(&mut self, ui: &mut Ui, rect: Rect, t: &Tokens) {
        let np = self.now_playing().map(|(i, secs, m)| (i, secs, m.clone()));
        let current = np
            .as_ref()
            .map(|(i, _, _)| *i)
            .or(self.selected)
            .filter(|&i| i < self.rows.len());

        // Artwork.
        let art = Rect::from_min_size(rect.min, Vec2::splat(rect.width()));
        let tex = current.and_then(|i| {
            let path = self.rows[i].path.clone();
            self.art.get(ui.ctx(), &path, self.root.as_deref())
        });
        match tex {
            Some(tex) => {
                let s = tex.size_vec2();
                let scale = (art.width() / s.x).min(art.height() / s.y);
                let r = Rect::from_center_size(art.center(), s * scale);
                ui.painter().rect_filled(
                    r.translate(Vec2::new(0.0, 3.0)),
                    CornerRadius::same(hifi::RADIUS + 2),
                    Color32::from_black_alpha(if t.dark { 70 } else { 28 }),
                );
                egui::Image::new((tex.id(), r.size()))
                    .corner_radius(hifi::RADIUS)
                    .paint_at(ui, r);
            }
            None => {
                hifi::well(ui.painter(), art, t);
                hifi::label(
                    ui.painter(),
                    art.center(),
                    Align2::CENTER_CENTER,
                    if current.is_some() {
                        "No artwork"
                    } else {
                        "Artwork"
                    },
                    hifi::ui_font(13.0),
                    t.dim,
                );
            }
        }

        // The track: title, artist, album.
        let p = ui.painter();
        let w = rect.width();
        let x = rect.left();
        let mut y = art.bottom() + 16.0;
        match current {
            Some(i) => {
                let r = &self.rows[i];
                hifi::label_fit(
                    p,
                    Pos2::new(x, y),
                    Align2::LEFT_TOP,
                    &r.title(),
                    hifi::bold(20.0),
                    t.text,
                    w,
                );
                hifi::label_fit(
                    p,
                    Pos2::new(x, y + 28.0),
                    Align2::LEFT_TOP,
                    &r.artist(),
                    hifi::ui_font(15.0),
                    t.text,
                    w,
                );
                let mut line = r.album();
                if !self.s.simple {
                    let n = format!("Track {} of {}", i + 1, self.rows.len());
                    line = if line.is_empty() {
                        n
                    } else {
                        format!("{line} · {n}")
                    };
                }
                hifi::label_fit(
                    p,
                    Pos2::new(x, y + 50.0),
                    Align2::LEFT_TOP,
                    &line,
                    hifi::ui_font(13.0),
                    t.dim,
                    w,
                );
            }
            None => {
                hifi::label(
                    p,
                    Pos2::new(x, y),
                    Align2::LEFT_TOP,
                    if self.rows.is_empty() {
                        "No music loaded"
                    } else {
                        "Stopped"
                    },
                    hifi::bold(20.0),
                    t.text,
                );
                hifi::label_fit(
                    p,
                    Pos2::new(x, y + 28.0),
                    Align2::LEFT_TOP,
                    if self.rows.is_empty() {
                        "Open a folder or files, or drop them on the window."
                    } else {
                        "Double-click a track, or press Play."
                    },
                    hifi::ui_font(13.0),
                    t.dim,
                    w,
                );
            }
        }
        y += 82.0;

        // What the file is, and what's being done to it: Detailed only.
        // Simple shows the artwork and the track's names, nothing more.
        if let (Some((i, _, m)), false) = (&np, self.s.simple) {
            let (ok, path) = self.signal_path(*i, m, false);
            let p = ui.painter();
            // Resampling is worth knowing, not a fault: amber, not red.
            let light = if ok {
                t.ok
            } else {
                Color32::from_rgb(0xe0, 0xa0, 0x30)
            };
            {
                let (codec, fmt) = self.format_chips(*i, m);
                hifi::status_light(p, Pos2::new(x + 5.0, y + 9.0), light);
                let mut cx = x + 18.0;
                cx += chip(p, Pos2::new(cx, y + 9.0), &codec, t) + 6.0;
                cx += chip(p, Pos2::new(cx, y + 9.0), &fmt, t) + 8.0;
                let need = p
                    .layout_no_wrap(path.clone(), hifi::ui_font(13.0), t.text)
                    .size()
                    .x;
                if cx + need <= rect.right() {
                    hifi::label_fit(
                        p,
                        Pos2::new(cx, y + 9.0),
                        Align2::LEFT_CENTER,
                        &path,
                        hifi::ui_font(13.0),
                        t.text,
                        rect.right() - cx,
                    );
                    y += 26.0;
                } else {
                    hifi::label_fit(
                        p,
                        Pos2::new(x + 18.0, y + 35.0),
                        Align2::LEFT_CENTER,
                        &path,
                        hifi::ui_font(13.0),
                        t.text,
                        w - 18.0,
                    );
                    y += 52.0;
                }
                hifi::label_fit(
                    p,
                    Pos2::new(x + 18.0, y + 9.0),
                    Align2::LEFT_CENTER,
                    &self.output_line(),
                    hifi::ui_font(11.5),
                    t.dim,
                    w - 18.0,
                );
                y += 18.0;
                if let Some(d) = self.dropouts() {
                    hifi::label_fit(
                        p,
                        Pos2::new(x + 18.0, y + 9.0),
                        Align2::LEFT_CENTER,
                        &d,
                        hifi::ui_font(11.5),
                        t.dim,
                        w - 18.0,
                    );
                    y += 18.0;
                }
                y += 14.0;
            }
        }

        // The list's size (or the latest message), under the rest.
        self.list_summary(ui, x, y, w, t);
    }

    // ------------------------------------------------------------ display

    fn display(&mut self, ui: &mut Ui, rect: Rect, t: &Tokens) {
        hifi::display(ui.painter(), rect, t);
        let np = self.now_playing().map(|(i, secs, m)| (i, secs, m.clone()));

        // Time, big, with what it is under it. Click to switch between
        // elapsed and remaining.
        let time_r = Rect::from_min_size(rect.min, Vec2::new(230.0, rect.height()));
        let resp = ui
            .interact(time_r, Id::new("time"), Sense::click())
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text("Click to show time elapsed or remaining");
        if resp.clicked() {
            self.s.remain = !self.s.remain;
        }
        let (time, under) = match &np {
            Some((i, secs, m)) => {
                let len = m
                    .info
                    .seconds()
                    .or_else(|| self.rows.get(*i).and_then(Row::seconds));
                let secs = match (self.seeking, len) {
                    (Some(f), Some(l)) => f as f64 * l,
                    _ => *secs,
                };
                let remain = self.s.remain && len.is_some();
                (
                    if remain {
                        format!("-{}", time_label(len.unwrap_or(0.0) - secs))
                    } else {
                        time_label(secs)
                    },
                    format!(
                        "{} · TRACK {:02}",
                        if remain { "REMAINING" } else { "ELAPSED" },
                        i + 1
                    ),
                )
            }
            None => (
                "-:--".to_string(),
                if self.rows.is_empty() {
                    "NO MUSIC".to_string()
                } else {
                    "STOPPED".to_string()
                },
            ),
        };
        let p = ui.painter();
        hifi::ink_text(
            p,
            Pos2::new(rect.left() + 20.0, rect.top() + 18.0),
            Align2::LEFT_TOP,
            &time,
            hifi::num(40.0),
            t,
        );
        hifi::label(
            p,
            Pos2::new(rect.left() + 22.0, rect.top() + 72.0),
            Align2::LEFT_TOP,
            &under,
            hifi::num(11.0),
            t.ink_faint(0.7),
        );

        // Meters with their dB scale, and the spectrum under them. With the
        // spectrum off, the meters sit in the middle of the display.
        let mx = rect.left() + 250.0;
        let mw = rect.right() - 20.0 - mx;
        // Two meters 16 apart, then the scale's 12.
        const METERS_H: f32 = 10.0 + 16.0 + 12.0;
        let top = if self.s.spectrum {
            rect.top() + 20.0
        } else {
            (rect.center().y - METERS_H / 2.0).round()
        };
        for (c, s) in ["L", "R"].iter().enumerate() {
            let y = top + c as f32 * 16.0;
            hifi::label(
                p,
                Pos2::new(mx - 14.0, y + 5.0),
                Align2::CENTER_CENTER,
                s,
                hifi::num(10.0),
                t.ink_faint(0.7),
            );
            let r = Rect::from_min_size(Pos2::new(mx, y), Vec2::new(mw, 10.0));
            hifi::meter_in(p, r, t, self.meter_db[c], self.s.meter_colours);
            if c == 1 {
                hifi::meter_scale(p, r, t);
            }
        }
        if self.s.spectrum && self.state() == State::Playing {
            let sp = Rect::from_min_max(
                Pos2::new(mx, top + METERS_H + 6.0),
                Pos2::new(rect.right() - 20.0, rect.bottom() - 14.0),
            );
            self.spectrum_bars(p, sp, t);
        }
    }

    fn spectrum_bars(&self, p: &egui::Painter, r: Rect, t: &Tokens) {
        const BARS: usize = 40;
        let (f_min, f_max) = (30.0f32, 16_000.0f32);
        let gap = 3.0;
        let w = (r.width() - gap * (BARS - 1) as f32) / BARS as f32;
        let range = 72.0;
        let sr = self.analyzer.sample_rate;
        for b in 0..BARS {
            let f0 = f_min * (f_max / f_min).powf(b as f32 / BARS as f32);
            let f1 = f_min * (f_max / f_min).powf((b + 1) as f32 / BARS as f32);
            // A 3 dB/octave tilt so music reads level.
            let tilt = 3.0 * ((f0 * f1).sqrt() / 1000.0).log2();
            let db = self.analyzer.level(f0, f1, sr) + tilt;
            let l = ((db + range) / range).clamp(0.0, 1.0);
            let h = (l * r.height()).max(2.0);
            let x = r.left() + b as f32 * (w + gap);
            p.rect_filled(
                Rect::from_min_max(Pos2::new(x, r.bottom() - h), Pos2::new(x + w, r.bottom())),
                CornerRadius::same(1),
                t.ink.gamma_multiply(0.85),
            );
        }
    }

    // ------------------------------------------------------------ list

    /// The list's keys, from `left` along the row centred on `y`.
    fn toolbar(&mut self, ui: &mut Ui, left: f32, y: f32, t: &Tokens) {
        let now = now(ui);
        let mut x = left;
        for (id, s, tip) in [
            (
                "folder",
                "Open folder",
                "Open a folder in place of the list",
            ),
            (
                "files",
                "Open files",
                "Open files or a playlist in place of the list",
            ),
            (
                "add",
                "Add",
                "Add files or a folder to the end of the list (or hold Shift as you drop)",
            ),
            ("clear", "Clear", "Empty the list"),
        ] {
            let w = hifi::button_width(ui, s);
            let r = Rect::from_min_size(Pos2::new(x, y - 14.0), Vec2::new(w, 28.0));
            let resp = hifi::button(ui, r, Id::new(id), t, s, false).on_hover_text(tip);
            match id {
                "folder" if resp.clicked() => self.open_folder(now),
                "files" if resp.clicked() => self.open_files(now),
                "clear" if resp.clicked() => self.clear(),
                "add" => {
                    let mut pick = None;
                    egui::Popup::menu(&resp).gap(4.0).show(|ui| {
                        ui.style_mut().override_font_id = Some(hifi::ui_font(13.0));
                        ui.style_mut().spacing.button_padding = Vec2::new(10.0, 6.0);
                        if ui.button("Add files or a playlist…").clicked() {
                            pick = Some(false);
                        }
                        if ui.button("Add a folder…").clicked() {
                            pick = Some(true);
                        }
                    });
                    match pick {
                        Some(true) => self.add_folder(now),
                        Some(false) => self.add_files(now),
                        None => {}
                    }
                }
                _ => {}
            }
            x += w + 8.0;
        }
    }

    /// How wide the list's keys are, all told.
    fn toolbar_width(ui: &Ui) -> f32 {
        ["Open folder", "Open files", "Add", "Clear"]
            .iter()
            .map(|s| hifi::button_width(ui, s) + 8.0)
            .sum::<f32>()
            - 8.0
    }

    /// Under the track: the list's size and length (Detailed), or for a
    /// while after something happens, what happened (up to two lines).
    fn list_summary(&self, ui: &Ui, x: f32, top: f32, room: f32, t: &Tokens) {
        let now = now(ui);
        let p = ui.painter();
        match &self.message {
            Some((m, at)) if now - at < 10.0 => {
                // Up to two lines, so a longer message still fits.
                let mut job = egui::text::LayoutJob::single_section(
                    m.clone(),
                    egui::TextFormat {
                        font_id: hifi::ui_font(12.5),
                        color: t.text,
                        ..Default::default()
                    },
                );
                job.wrap = egui::text::TextWrapping {
                    max_width: room,
                    max_rows: 2,
                    break_anywhere: false,
                    overflow_character: Some('…'),
                };
                let g = p.layout_job(job);
                p.galley(Pos2::new(x, top), g, t.text);
            }
            _ if !self.rows.is_empty() && !self.s.simple => {
                let n = self.rows.len();
                let total: f64 = self.rows.iter().filter_map(Row::seconds).sum();
                let mut s = format!("{n} track{}", if n == 1 { "" } else { "s" });
                if total > 0.0 {
                    s += &format!(" · {}", duration_words(total));
                }
                hifi::label_fit(
                    p,
                    Pos2::new(x, top),
                    Align2::LEFT_TOP,
                    &s,
                    hifi::ui_font(13.0),
                    t.dim,
                    room,
                );
            }
            _ => {}
        }
    }

    fn playlist(&mut self, ui: &mut Ui, rect: Rect, t: &Tokens) {
        let playing = self.now_playing().map(|(i, _, _)| i);
        hifi::well(ui.painter(), rect, t);
        let head = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 32.0));
        let detailed = !self.s.simple;
        let x0 = rect.left() + 14.0;
        let c_num = x0 + 26.0;
        let c_title = x0 + 40.0;
        let c_type = rect.right() - 14.0;
        let c_time = if detailed {
            c_type - 52.0
        } else {
            rect.right() - 14.0
        };
        let flex = c_time - c_title - 60.0;
        let (c_artist, c_album) = if detailed {
            (c_title + flex * 0.42, Some(c_title + flex * 0.71))
        } else {
            (c_title + flex * 0.6, None)
        };
        {
            let p = ui.painter();
            let hl = |x: f32, s: &str, a: Align2| {
                hifi::label(
                    p,
                    Pos2::new(x, head.center().y),
                    a,
                    s,
                    hifi::semi(10.5),
                    t.dim,
                );
            };
            hl(c_num, "#", Align2::RIGHT_CENTER);
            hl(c_title, "TITLE", Align2::LEFT_CENTER);
            hl(c_artist, "ARTIST", Align2::LEFT_CENTER);
            if let Some(c) = c_album {
                hl(c, "ALBUM", Align2::LEFT_CENTER);
            }
            hl(c_time, "TIME", Align2::RIGHT_CENTER);
            if detailed {
                hl(c_type, "TYPE", Align2::RIGHT_CENTER);
            }
            p.line_segment(
                [
                    Pos2::new(rect.left() + 1.0, head.bottom()),
                    Pos2::new(rect.right() - 1.0, head.bottom()),
                ],
                Stroke::new(1.0, t.line),
            );
        }
        let body = Rect::from_min_max(
            Pos2::new(rect.left() + 1.0, head.bottom() + 1.0),
            rect.max - Vec2::new(1.0, 2.0),
        );
        if self.rows.is_empty() {
            hifi::label(
                ui.painter(),
                body.center(),
                Align2::CENTER_CENTER,
                "Open a folder, files or a playlist, or drop them here",
                hifi::ui_font(13.0),
                t.dim,
            );
            return;
        }
        if ui.rect_contains_pointer(body) && ui.input(|i| i.smooth_scroll_delta.y != 0.0) {
            self.user_scrolled = now(ui);
        }
        let scroll_to = self.scroll_to.take();
        let mut clicked = None;
        let mut double = None;
        let rows = &self.rows;
        let selected = self.selected;
        lists::rows(
            ui,
            body,
            "playlist",
            ROW_H,
            rows.len(),
            scroll_to,
            |ui, i, r, resp| {
                let row = &rows[i];
                let p = ui.painter();
                if selected == Some(i) {
                    p.rect_filled(r, 0, t.select);
                } else if resp.hovered() {
                    p.rect_filled(r, 0, t.line_soft);
                }
                p.line_segment(
                    [r.left_bottom(), r.right_bottom()],
                    Stroke::new(1.0, t.line_soft),
                );
                let is_playing = playing == Some(i);
                let ink = if is_playing { t.accent } else { t.text };
                let y = r.center().y;
                if is_playing {
                    let c = Pos2::new(c_num - 6.0, y);
                    p.add(Shape::convex_polygon(
                        vec![
                            c + Vec2::new(-4.0, -5.0),
                            c + Vec2::new(5.0, 0.0),
                            c + Vec2::new(-4.0, 5.0),
                        ],
                        t.accent,
                        Stroke::NONE,
                    ));
                } else {
                    hifi::label(
                        p,
                        Pos2::new(c_num, y),
                        Align2::RIGHT_CENTER,
                        &(i + 1).to_string(),
                        hifi::num(12.0),
                        t.dim,
                    );
                }
                let f = if is_playing {
                    hifi::semi(14.0)
                } else {
                    hifi::ui_font(14.0)
                };
                hifi::label_fit(
                    p,
                    Pos2::new(c_title, y),
                    Align2::LEFT_CENTER,
                    &row.title(),
                    f,
                    ink,
                    c_artist - c_title - 14.0,
                );
                let artist_end = c_album.unwrap_or(c_time - 60.0);
                hifi::label_fit(
                    p,
                    Pos2::new(c_artist, y),
                    Align2::LEFT_CENTER,
                    &row.artist(),
                    hifi::ui_font(13.5),
                    t.dim,
                    artist_end - c_artist - 14.0,
                );
                if let Some(c) = c_album {
                    hifi::label_fit(
                        p,
                        Pos2::new(c, y),
                        Align2::LEFT_CENTER,
                        &row.album(),
                        hifi::ui_font(13.5),
                        t.dim,
                        c_time - c - 60.0,
                    );
                }
                if let Some(s) = row.seconds() {
                    hifi::label(
                        p,
                        Pos2::new(c_time, y),
                        Align2::RIGHT_CENTER,
                        &time_label(s),
                        hifi::num(12.0),
                        t.dim,
                    );
                }
                if detailed {
                    hifi::label(
                        p,
                        Pos2::new(c_type, y),
                        Align2::RIGHT_CENTER,
                        &row.ext,
                        hifi::semi(10.5),
                        t.dim,
                    );
                }
                if resp.double_clicked() {
                    double = Some(i);
                } else if resp.clicked() {
                    clicked = Some(i);
                }
            },
        );
        if let Some(i) = clicked {
            self.selected = Some(i);
        }
        if let Some(i) = double {
            self.selected = Some(i);
            self.play(i);
        }
    }

    // ------------------------------------------------------------ transport bar

    fn transport(&mut self, ui: &mut Ui, bar: Rect, t: &Tokens) {
        hifi::bar(ui.painter(), bar, t);
        let cy = bar.center().y;
        let x0 = bar.left() + 24.0;
        let state = self.state();

        let buttons = [
            (
                18.0,
                18.0,
                Btn::Flat,
                Icon::Shuffle,
                "random",
                "Random: shuffle the list and play it",
            ),
            (
                60.0,
                18.0,
                Btn::Flat,
                Icon::ToStart,
                "start",
                "Back to the start of the list (Home)",
            ),
            (
                106.0,
                21.0,
                Btn::Raised,
                Icon::Prev,
                "prev",
                "Previous (Shift+Left)",
            ),
            (
                164.0,
                29.0,
                Btn::Primary,
                if state == State::Playing {
                    Icon::Pause
                } else {
                    Icon::Play
                },
                "play",
                "Play or pause (Space)",
            ),
            (
                222.0,
                21.0,
                Btn::Raised,
                Icon::Next,
                "next",
                "Next (Shift+Right)",
            ),
            (266.0, 18.0, Btn::Flat, Icon::Stop, "stop", "Stop"),
        ];
        for (dx, r, kind, icon, id, tip) in buttons {
            let c = Pos2::new(x0 + dx, cy);
            if round_button(ui, c, r, Id::new(id), t, kind, icon)
                .on_hover_text(tip)
                .clicked()
            {
                match id {
                    "random" => self.random(),
                    "start" => self.back_to_start(),
                    "prev" => self.previous(),
                    "play" => self.play_pause(),
                    "next" => self.next(),
                    _ => self.engine.send(Cmd::Stop),
                }
            }
        }

        // Volume on the right; the position fills the middle.
        let vol_w = 130.0;
        let vol = Rect::from_min_size(
            Pos2::new(bar.right() - 24.0 - vol_w, cy - 9.0),
            Vec2::new(vol_w, 18.0),
        );
        speaker(
            ui.painter(),
            Pos2::new(vol.left() - 22.0, cy),
            t.dim,
            self.s.volume_db,
        );
        let before = self.s.volume_db;
        hifi::hslider(
            ui,
            vol,
            Id::new("volume"),
            t,
            &mut self.s.volume_db,
            &VOLUME,
        )
        .on_hover_text(if self.s.volume_db >= 0.0 {
            "Volume: full. At full volume the samples are left untouched.".to_string()
        } else {
            format!("Volume: {:.1} dB", self.s.volume_db)
        });
        if self.s.volume_db != before {
            self.engine.send(Cmd::Gain(db_to_gain(self.s.volume_db)));
        }

        let pos = Rect::from_min_max(
            Pos2::new(x0 + 356.0, cy - 9.0),
            Pos2::new(vol.left() - 108.0, cy + 9.0),
        );
        self.position(ui, pos, t);
    }

    fn position(&mut self, ui: &mut Ui, r: Rect, t: &Tokens) {
        let np = self.now_playing().map(|(i, secs, m)| (i, secs, m.clone()));
        let len = np.as_ref().and_then(|(i, _, m)| {
            m.info
                .seconds()
                .or_else(|| self.rows.get(*i).and_then(Row::seconds))
        });
        let secs = np.as_ref().map(|(_, s, _)| *s).unwrap_or(0.0);
        let resp = ui.interact(
            r.expand2(Vec2::new(6.0, 8.0)),
            Id::new("seek"),
            Sense::click_and_drag(),
        );
        let frac_at = |x: f32| ((x - r.left()) / r.width()).clamp(0.0, 1.0);
        if let (Some(l), Some(pos)) = (len, resp.interact_pointer_pos()) {
            if resp.dragged() || resp.is_pointer_button_down_on() {
                self.seeking = Some(frac_at(pos.x));
            }
            if resp.drag_stopped() || resp.clicked() {
                let f = frac_at(pos.x) as f64;
                self.engine.send(Cmd::Seek((f * l).min((l - 0.5).max(0.0))));
                self.seeking = None;
            }
        }
        if !resp.is_pointer_button_down_on() && !resp.dragged() {
            self.seeking = None;
        }
        if resp.hovered() && len.is_some() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        let shown = match (self.seeking, len) {
            (Some(f), Some(_)) => f,
            (_, Some(l)) if l > 0.0 => (secs / l).clamp(0.0, 1.0) as f32,
            _ => 0.0,
        };
        let p = ui.painter();
        let track = Rect::from_center_size(r.center(), Vec2::new(r.width(), 6.0));
        p.rect_filled(track, CornerRadius::same(3), t.groove);
        let x = r.left() + r.width() * shown;
        p.rect_filled(
            Rect::from_min_max(track.min, Pos2::new(x, track.bottom())),
            CornerRadius::same(3),
            t.accent,
        );
        if len.is_some() {
            let c = Pos2::new(x, r.center().y);
            p.circle_filled(c + Vec2::new(0.0, 1.0), 8.0, Color32::from_black_alpha(64));
            p.circle_filled(c, 8.0, t.thumb);
        }
        let shown_secs = match (self.seeking, len) {
            (Some(f), Some(l)) => f as f64 * l,
            _ => secs,
        };
        hifi::label(
            p,
            Pos2::new(r.left() - 14.0, r.center().y),
            Align2::RIGHT_CENTER,
            &if np.is_some() {
                time_label(shown_secs)
            } else {
                "-:--".into()
            },
            hifi::num(12.0),
            t.dim,
        );
        hifi::label(
            p,
            Pos2::new(r.right() + 14.0, r.center().y),
            Align2::LEFT_CENTER,
            &len.map(time_label).unwrap_or_else(|| "-:--".into()),
            hifi::num(12.0),
            t.dim,
        );
    }

    // ------------------------------------------------------------ settings

    fn settings(&mut self, ctx: &egui::Context, t: &Tokens) {
        let mut open = self.settings_open;
        let mut look = self.s.finish;
        let mut system_bar = self.s.system_title_bar;
        let mut subfolders = self.s.subfolders;
        let mut spectrum = self.s.spectrum;
        let mut play_on_open = self.s.play_on_open;
        let mut meter_colours = self.s.meter_colours;
        let mut items = vec!["System default".to_string()];
        items.extend(self.devices.iter().cloned());
        let sel = self
            .s
            .device
            .as_ref()
            .and_then(|d| items.iter().position(|x| x == d))
            .unwrap_or(0);
        let mut device = None;
        let mut out = self.output_line();
        if let Some(d) = self.dropouts() {
            out += &format!("\n{d}");
        }
        hifi::drawer(ctx, t, "Settings", &mut open, |ui| {
            hifi::look_settings(ui, t, &mut look);

            hifi::drawer_heading(ui, t, "Output");
            let r = hifi::drawer_row(ui, t, "Play through", 220.0);
            device = hifi::select(ui, r, "device", t, &items, sel);
            ui.label(
                egui::RichText::new(out)
                    .font(hifi::ui_font(12.0))
                    .color(t.dim),
            );

            hifi::drawer_heading(ui, t, "Playing");
            for (label, id, v) in [
                (
                    "Start playing when I open music",
                    "play-on-open",
                    &mut play_on_open,
                ),
                ("Include sub-folders", "subfolders", &mut subfolders),
            ] {
                let r = hifi::drawer_row(ui, t, label, 32.0);
                let r = Rect::from_min_size(r.min + Vec2::new(0.0, 4.0), Vec2::new(32.0, 18.0));
                if hifi::switch(ui, r, Id::new(id), t, *v).clicked() {
                    *v = !*v;
                }
            }

            hifi::drawer_heading(ui, t, "Display");
            let r = hifi::drawer_row(ui, t, "Meter colours", 150.0);
            let names = hifi::MeterColours::ALL.map(|m| m.name());
            let sel = hifi::MeterColours::ALL
                .iter()
                .position(|m| *m == meter_colours)
                .unwrap_or(0);
            if let (_, Some(i)) = hifi::segmented(
                ui,
                Pos2::new(r.right(), r.center().y),
                true,
                Id::new("meter-colours"),
                t,
                &names,
                sel,
                hifi::Seg::Plain,
                &[],
                &[],
            ) {
                meter_colours = hifi::MeterColours::ALL[i];
            }
            let r = hifi::drawer_row(ui, t, "Spectrum in the display", 32.0);
            let r = Rect::from_min_size(r.min + Vec2::new(0.0, 4.0), Vec2::new(32.0, 18.0));
            if hifi::switch(ui, r, Id::new("spectrum"), t, spectrum).clicked() {
                spectrum = !spectrum;
            }

            hifi::drawer_heading(ui, t, "Window");
            let r = hifi::drawer_row(ui, t, "Use the system title bar", 32.0);
            let r = Rect::from_min_size(r.min + Vec2::new(0.0, 4.0), Vec2::new(32.0, 18.0));
            if hifi::switch(ui, r, Id::new("system-bar"), t, system_bar).clicked() {
                system_bar = !system_bar;
            }

            hifi::drawer_heading(ui, t, "Keys");
            for (k, what) in [
                ("Space", "Play or pause"),
                ("Left, Right", "Back or forward 10 seconds"),
                ("Shift+Left, Right", "Previous or next track"),
                ("Up, Down", "Choose a track"),
                ("Enter", "Play the chosen track"),
                ("Home", "Back to the start of the list"),
                ("Shift+drop", "Add to the list"),
            ] {
                // Two columns, both from the left edge.
                let (row, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 18.0), Sense::hover());
                let y = row.center().y;
                let p = ui.painter();
                hifi::label(
                    p,
                    Pos2::new(row.left(), y),
                    Align2::LEFT_CENTER,
                    k,
                    hifi::num(12.0),
                    t.text,
                );
                hifi::label(
                    p,
                    Pos2::new(row.left() + 136.0, y),
                    Align2::LEFT_CENTER,
                    what,
                    hifi::ui_font(12.5),
                    t.dim,
                );
            }
        });
        self.settings_open = open;
        self.s.finish = look;
        self.s.subfolders = subfolders;
        self.s.spectrum = spectrum;
        self.s.play_on_open = play_on_open;
        self.s.meter_colours = meter_colours;
        if let Some(n) = device {
            self.s.device = (n > 0).then(|| items[n].clone());
            self.engine.send(Cmd::Device(self.s.device.clone()));
            let t = ctx.input(|i| i.time);
            self.tell(format!("Output: {}", items[n]), t);
        }
        if system_bar != self.s.system_title_bar {
            self.s.system_title_bar = system_bar;
            ctx.send_viewport_cmd(ViewportCommand::Decorations(system_bar));
        }
    }

    fn fit_to_window(&self, ctx: &egui::Context) {
        let zoom = ctx.zoom_factor();
        let screen = ctx.content_rect().size() * zoom;
        let want = (screen.x / MIN_DESIGN.x)
            .min(screen.y / MIN_DESIGN.y)
            .clamp(0.5, 1.0);
        if (want - zoom).abs() > 0.002 {
            ctx.set_zoom_factor(want);
            ctx.request_repaint();
        }
    }
}

/// The window, for the media controls on Windows (they belong to a window
/// there). Elsewhere they don't need one.
fn window_handle(cc: &eframe::CreationContext<'_>) -> Option<*mut std::ffi::c_void> {
    #[cfg(windows)]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(h) = cc.window_handle() {
            if let RawWindowHandle::Win32(w) = h.as_raw() {
                return Some(w.hwnd.get() as *mut std::ffi::c_void);
            }
        }
        None
    }
    #[cfg(not(windows))]
    {
        let _ = cc;
        None
    }
}

/// "25 s", "39 min", "1 h 12 min".
fn duration_words(secs: f64) -> String {
    if secs < 59.5 {
        return format!("{} s", secs.round().max(1.0) as u64);
    }
    let m = (secs / 60.0).round() as u64;
    if m >= 60 {
        format!("{} h {} min", m / 60, m % 60)
    } else {
        format!("{m} min")
    }
}

/// A small outlined label for a format. Returns its width.
fn chip(p: &egui::Painter, left_centre: Pos2, s: &str, t: &Tokens) -> f32 {
    let g = p.layout_no_wrap(s.to_string(), hifi::ui_font(11.5), t.dim);
    let r = Rect::from_min_size(
        Pos2::new(left_centre.x, left_centre.y - 10.0),
        Vec2::new(g.size().x + 14.0, 20.0),
    );
    p.rect_stroke(
        r,
        CornerRadius::same(hifi::RADIUS),
        Stroke::new(1.0, t.line),
        StrokeKind::Inside,
    );
    p.galley(
        Pos2::new(r.left() + 7.0, r.center().y - g.size().y / 2.0),
        g,
        t.dim,
    );
    r.width()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Btn {
    /// Just the icon, with a soft circle on hover.
    Flat,
    /// A raised round key.
    Raised,
    /// Filled with the accent: Play.
    Primary,
}

#[derive(Clone, Copy)]
enum Icon {
    Shuffle,
    ToStart,
    Prev,
    Play,
    Pause,
    Next,
    Stop,
}

/// A round transport key with an icon.
fn round_button(
    ui: &mut Ui,
    c: Pos2,
    r: f32,
    id: Id,
    t: &Tokens,
    kind: Btn,
    icon: Icon,
) -> Response {
    let hit = Rect::from_center_size(c, Vec2::splat(r * 2.0));
    let resp = ui
        .interact(hit, id, Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand);
    let p = ui.painter();
    let down = resp.is_pointer_button_down_on();
    let hover = resp.hovered() && !down;
    let shade = |col: Color32, k: f32| {
        let to = if t.dark {
            Color32::WHITE
        } else {
            Color32::BLACK
        };
        taw_panel::theme::lerp_colour(col, to, k)
    };
    let fg = match kind {
        Btn::Flat => {
            if hover {
                p.circle_filled(c, r, t.line_soft);
            }
            t.dim
        }
        Btn::Raised => {
            p.circle_filled(c + Vec2::new(0.0, 1.5), r, Color32::from_black_alpha(36));
            p.circle_filled(
                c,
                r,
                if hover {
                    shade(t.raised, 0.05)
                } else {
                    t.raised
                },
            );
            p.circle_stroke(c, r, Stroke::new(1.0, t.line));
            t.text
        }
        Btn::Primary => {
            p.circle_filled(c + Vec2::new(0.0, 2.0), r, Color32::from_black_alpha(50));
            p.circle_filled(
                c,
                r,
                if hover {
                    shade(t.accent, 0.08)
                } else {
                    t.accent
                },
            );
            t.on_accent
        }
    };
    if resp.has_focus() {
        p.circle_stroke(c, r + 3.0, Stroke::new(2.0, t.accent));
    }
    draw_icon(
        p,
        c,
        icon,
        if kind == Btn::Primary { 10.0 } else { 7.0 },
        fg,
    );
    resp
}

fn draw_icon(p: &egui::Painter, c: Pos2, icon: Icon, s: f32, col: Color32) {
    let tri = |pts: Vec<Pos2>| Shape::convex_polygon(pts, col, Stroke::NONE);
    match icon {
        Icon::Play => {
            p.add(tri(vec![
                c + Vec2::new(-s * 0.55, -s),
                c + Vec2::new(s * 0.95, 0.0),
                c + Vec2::new(-s * 0.55, s),
            ]));
        }
        Icon::Pause => {
            for dx in [-0.5, 0.5] {
                p.rect_filled(
                    Rect::from_center_size(
                        c + Vec2::new(dx * s, 0.0),
                        Vec2::new(s * 0.42, s * 1.8),
                    ),
                    CornerRadius::same(1),
                    col,
                );
            }
        }
        Icon::Stop => {
            p.rect_filled(
                Rect::from_center_size(c, Vec2::splat(s * 1.4)),
                CornerRadius::same(2),
                col,
            );
        }
        Icon::Prev | Icon::Next => {
            let d = if matches!(icon, Icon::Next) {
                1.0
            } else {
                -1.0
            };
            p.add(tri(vec![
                c + Vec2::new(-s * 0.7 * d, -s * 0.85),
                c + Vec2::new(s * 0.55 * d, 0.0),
                c + Vec2::new(-s * 0.7 * d, s * 0.85),
            ]));
            p.rect_filled(
                Rect::from_center_size(c + Vec2::new(s * 0.75 * d, 0.0), Vec2::new(2.2, s * 1.7)),
                CornerRadius::same(1),
                col,
            );
        }
        Icon::ToStart => {
            // A bar, then two triangles pointing back to it: |◀◀
            p.rect_filled(
                Rect::from_center_size(c + Vec2::new(-s * 0.95, 0.0), Vec2::new(2.0, s * 1.5)),
                CornerRadius::same(1),
                col,
            );
            for dx in [-0.75, 0.05] {
                p.add(tri(vec![
                    c + Vec2::new(dx * s, 0.0),
                    c + Vec2::new((dx + 0.8) * s, -s * 0.72),
                    c + Vec2::new((dx + 0.8) * s, s * 0.72),
                ]));
            }
        }
        Icon::Shuffle => {
            let st = Stroke::new(1.8, col);
            let (l, r) = (c.x - s, c.x + s * 0.7);
            let (top, bot) = (c.y - s * 0.6, c.y + s * 0.6);
            p.add(Shape::line(
                vec![
                    Pos2::new(l, top),
                    Pos2::new(c.x - s * 0.3, top),
                    Pos2::new(c.x + s * 0.3, bot),
                    Pos2::new(r, bot),
                ],
                st,
            ));
            p.add(Shape::line(
                vec![
                    Pos2::new(l, bot),
                    Pos2::new(c.x - s * 0.3, bot),
                    Pos2::new(c.x + s * 0.3, top),
                    Pos2::new(r, top),
                ],
                st,
            ));
            for y in [top, bot] {
                p.add(tri(vec![
                    Pos2::new(r + s * 0.55, y),
                    Pos2::new(r - s * 0.05, y - s * 0.38),
                    Pos2::new(r - s * 0.05, y + s * 0.38),
                ]));
            }
        }
    }
}

/// A small loudspeaker, with waves for the volume.
fn speaker(p: &egui::Painter, c: Pos2, col: Color32, db: f32) {
    p.add(Shape::convex_polygon(
        vec![
            c + Vec2::new(-7.0, -3.0),
            c + Vec2::new(-3.5, -3.0),
            c + Vec2::new(1.0, -7.0),
            c + Vec2::new(1.0, 7.0),
            c + Vec2::new(-3.5, 3.0),
            c + Vec2::new(-7.0, 3.0),
        ],
        col,
        Stroke::NONE,
    ));
    let waves = if db > -1.0 {
        2
    } else if db > -30.0 {
        1
    } else {
        0
    };
    for w in 0..waves {
        let rad = 5.0 + w as f32 * 4.0;
        let pts: Vec<Pos2> = (0..=8)
            .map(|k| {
                let a = -0.8 + 1.6 * k as f32 / 8.0;
                c + Vec2::new(1.5 + a.cos() * rad, a.sin() * rad)
            })
            .collect();
        p.add(Shape::line(pts, Stroke::new(1.5, col)));
    }
}

impl eframe::App for PlayerApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.fit_to_window(&ctx);
        let t = Tokens::from_ctx(&ctx, &self.s.finish);
        hifi::apply_visuals(&ctx, &t);
        self.poll(ui);
        if !self.settings_open {
            self.keys(ui);
        }

        let screen = ui.max_rect();
        hifi::faceplate(ui.painter(), screen, &t);
        let family_left = self.plate(ui, screen, &t);
        let bar = Rect::from_min_max(
            Pos2::new(screen.left(), screen.bottom() - BAR_H),
            screen.max,
        );
        self.transport(ui, bar, &t);
        // The bar covers the faceplate's edge; draw the window's outline
        // over both.
        ui.painter()
            .rect_stroke(screen, 0, Stroke::new(1.0, t.edge), StrokeKind::Inside);

        let body = Rect::from_min_max(
            Pos2::new(screen.left() + 24.0, screen.top() + PLATE_H + 8.0),
            Pos2::new(screen.right() - 24.0, bar.top() - 22.0),
        );
        // The artwork is as big as the height allows, up to 800, leaving
        // the list's keys room on the nameplate's row beside the family
        // switcher.
        let tools_w = Self::toolbar_width(ui);
        let room = family_left - 16.0 - tools_w - 28.0 - body.left();
        let left_w = (body.height() - UNDER_ART).min(room).clamp(240.0, ART_MAX);
        let left = Rect::from_min_size(body.min, Vec2::new(left_w, body.height()));
        self.left(ui, left, &t);
        let mut right = Rect::from_min_max(Pos2::new(left.right() + 28.0, body.top()), body.max);
        if !self.s.simple {
            let d = Rect::from_min_size(right.min, Vec2::new(right.width(), DISPLAY_H));
            self.display(ui, d, &t);
            right.min.y = d.bottom() + 14.0;
        }
        self.toolbar(ui, right.left(), screen.top() + ROW_Y, &t);
        self.playlist(ui, right, &t);

        self.settings(&ctx, &t);
        if !self.s.system_title_bar {
            hifi::resize_edges(ui, screen);
        }

        if self.state() == State::Playing || self.seeking.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        } else {
            // Keep checking for hand-overs from the Library.
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, STORAGE_KEY, &self.s);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(b) = &self.beacon {
            b.leave();
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        Tokens::new(&self.s.finish, false)
            .face_bottom
            .to_normalized_gamma_f32()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        assert_eq!(rate_label(44_100), "44.1 kHz");
        assert_eq!(rate_label(48_000), "48 kHz");
        assert_eq!(rate_label(88_200), "88.2 kHz");
        assert_eq!(rate_label(352_800), "352.8 kHz");
        assert_eq!(time_label(65.4), "1:05");
        assert_eq!(time_label(3725.0), "1:02:05");
        assert_eq!(duration_words(39.0 * 60.0), "39 min");
        assert_eq!(duration_words(72.0 * 60.0), "1 h 12 min");
        assert_eq!(db_to_gain(0.0), 1.0);
        assert!((db_to_gain(-6.0) - 0.501).abs() < 0.001);
    }

    #[test]
    fn saved_settings_tolerate_missing_fields() {
        let s: Saved = serde_json::from_str(r#"{"volume_db": -3.0}"#).unwrap();
        assert_eq!(s.volume_db, -3.0);
        assert!(s.subfolders);
        assert!(!s.simple);
        assert_eq!(s.finish, hifi::Look::default());
    }

    #[test]
    fn settings_from_the_jupiter_panel_still_load() {
        let s: Saved =
            serde_json::from_str(r#"{"panel": "Silver", "volume_db": -6.0, "spectrum": false}"#)
                .unwrap();
        assert_eq!(s.volume_db, -6.0);
        assert!(!s.spectrum);
        assert!(!s.system_title_bar);
    }
}
