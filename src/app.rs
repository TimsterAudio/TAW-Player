//! The Player's panel: transport and the display along the top, the
//! playing track (artwork, tags, what the signal path is doing, position
//! and meters) on the left, and the playlist on the right.

use crate::engine::{locate, Cmd, Engine, Mark, OutputInfo};
use crate::family::{self, App};
use crate::instance::{self, Beacon};
use crate::playlist::{self, Entry};
use crate::tags::{self, ArtLoader, TagReader, Tags};
use egui::text::{LayoutJob, TextFormat};
use egui::{
    Align2, Color32, CornerRadius, Id, Key, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use taw_panel::analyzer::{Analyzer, Ballistics};
use taw_panel::frame::{draw_panel_bg, fit_to_window};
use taw_panel::lists::{self, chip, fit, light, well, Light};
use taw_panel::spectrum::{self, Look, Scheme};
use taw_panel::theme::*;
use taw_panel::widgets::*;

const STORAGE_KEY: &str = "taw-player";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The panel's design size; smaller windows scale it down.
const DESIGN: Vec2 = Vec2::new(1560.0, 880.0);
pub const DEFAULT_SIZE: Vec2 = Vec2::new(1440.0, 812.0);
const TOP_ROW: f32 = 136.0;
const LEFT_W: f32 = 440.0;
const ROW_H: f32 = 26.0;

/// Everything remembered between runs.
#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Saved {
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
    pub fn new(cc: &eframe::CreationContext<'_>, open: Vec<PathBuf>) -> Self {
        install_fonts(&cc.egui_ctx);
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
        };
        app.refresh_family();
        if !open.is_empty() {
            app.load(&open, true, 0.0);
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
            self.load(&[dir], true, t);
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
            self.load(&files, true, t);
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
                            "The {} isn't installed here: there's no {} next to the Player or on the PATH. Press {} again to show the Player where it is.",
                            a.name(),
                            family::exe_name(a.program()),
                            key_label(a)
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
                if let Some(paths) = lists.into_iter().last() {
                    self.load(&paths, true, t);
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
            self.load(&dropped, true, t);
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

    fn keys(&mut self, ui: &Ui) {
        if ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        let (space, next, prev, fwd, back, up, down, enter) = ui.input_mut(|i| {
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
            )
        });
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

    // ------------------------------------------------------------ top strip

    fn top_strip(&mut self, ui: &mut Ui, panel: Rect, th: &Theme) {
        let o = panel.min.to_vec2();
        let at = |x: f32, y: f32| Pos2::new(x, y) + o;
        let t = now(ui);

        // Maker wordmark, logo and model line.
        {
            let p = ui.painter();
            let mut job = LayoutJob::default();
            job.append(
                "Timster's Audio Works - ",
                0.0,
                TextFormat {
                    font_id: font(F_MAKER, 13.0),
                    color: th.legend_dim,
                    ..Default::default()
                },
            );
            job.append(
                "Player",
                0.0,
                TextFormat {
                    font_id: font(F_MAKER_BOLD, 13.0),
                    color: th.legend,
                    ..Default::default()
                },
            );
            let g = p.layout_job(job);
            let r = Rect::from_min_size(at(22.0, 14.0), g.size() + Vec2::new(20.0, 10.0));
            p.rect_stroke(
                r,
                CornerRadius::same(3),
                Stroke::new(1.0, th.frame),
                StrokeKind::Inside,
            );
            p.galley(r.min + Vec2::new(10.0, 5.0), g, th.legend);
        }
        slanted_text(
            ui,
            at(24.0, 50.0),
            "PLAYER",
            font(F_LOGO, 30.0),
            th.legend,
            3.0,
            12.0,
        );
        text(
            ui.painter(),
            at(22.0, 92.0),
            Align2::LEFT_TOP,
            &format!("PL-001 · MUSIC PLAYER · V{VERSION}"),
            font(F_LEGEND, 10.0),
            th.legend_dim,
            2.4,
        );

        // The display.
        let lcd_rect = Rect::from_min_size(at(262.0, 26.0), Vec2::new(400.0, 52.0));
        let np = self.now_playing().map(|(i, secs, m)| (i, secs, m.clone()));
        let lines = match &np {
            Some((i, secs, m)) => {
                let row = &self.rows.get(*i);
                let title = row.map(|r| r.title()).unwrap_or_default();
                let len = m.info.seconds().or_else(|| row.and_then(|r| r.seconds()));
                let secs = match self.seeking {
                    Some(f) => len.map(|l| f as f64 * l).unwrap_or(*secs),
                    None => *secs,
                };
                let time = match (self.s.remain, len) {
                    (true, Some(l)) => format!("-{}", time_label(l - secs)),
                    _ => time_label(secs),
                };
                let mut file = m.info.codec.clone();
                if let Some(b) = m.info.bits {
                    file += &format!(" {b}/");
                } else {
                    file += " ";
                }
                file += &rate_label(m.info.rate).replace(" kHz", "");
                let who = row
                    .map(|r| {
                        let (a, al) = (r.artist(), r.album());
                        match (a.is_empty(), al.is_empty()) {
                            (false, false) => format!("{a} - {al}"),
                            (false, true) => a,
                            (true, false) => al,
                            _ => String::new(),
                        }
                    })
                    .unwrap_or_default();
                [(format!("{:02} {title}", i + 1), time), (who, file)]
            }
            None => {
                let n = self.rows.len();
                if n == 0 {
                    [
                        ("No music loaded".to_string(), String::new()),
                        ("Press Folder or Files".to_string(), String::new()),
                    ]
                } else {
                    let total: f64 = self.rows.iter().filter_map(Row::seconds).sum();
                    [
                        (
                            format!("{n} tracks"),
                            if total > 0.0 {
                                time_label(total)
                            } else {
                                String::new()
                            },
                        ),
                        ("Stopped".to_string(), String::new()),
                    ]
                }
            }
        };
        lcd(ui.painter(), lcd_rect, &lines);
        let lcd_resp = ui
            .interact(lcd_rect, Id::new("lcd"), Sense::click())
            .on_hover_text("Click to show time elapsed or remaining");
        if lcd_resp.clicked() {
            self.s.remain = !self.s.remain;
        }

        // Transport.
        let state = self.state();
        let k = |x: f32| Rect::from_min_size(at(x, 46.0), Vec2::new(54.0, 24.0));
        legend(
            ui.painter(),
            at(886.0, 14.0),
            Align2::CENTER_TOP,
            "Transport",
            th,
        );
        let x0 = 706.0;
        let step = 64.0;
        if key(ui, k(x0), Id::new("prev"), K_GREY, None, "Prev", th).clicked() {
            self.previous();
        }
        if key(
            ui,
            k(x0 + step),
            Id::new("play"),
            K_CREAM,
            Some(state == State::Playing),
            "Play",
            th,
        )
        .clicked()
        {
            match state {
                State::Paused => self.engine.send(Cmd::Pause(false)),
                State::Stopped => self.play_pause(),
                State::Playing => {} // latched, as on a deck
            }
        }
        if key(
            ui,
            k(x0 + step * 2.0),
            Id::new("pause"),
            K_YELLOW,
            Some(state == State::Paused),
            "Pause",
            th,
        )
        .clicked()
        {
            match state {
                State::Playing => self.engine.send(Cmd::Pause(true)),
                State::Paused => self.engine.send(Cmd::Pause(false)),
                State::Stopped => {}
            }
        }
        if key(
            ui,
            k(x0 + step * 3.0),
            Id::new("stop"),
            K_GREY,
            None,
            "Stop",
            th,
        )
        .clicked()
        {
            self.engine.send(Cmd::Stop);
        }
        if key(
            ui,
            k(x0 + step * 4.0),
            Id::new("next"),
            K_GREY,
            None,
            "Next",
            th,
        )
        .clicked()
        {
            self.next();
        }
        if key(
            ui,
            k(x0 + step * 5.0),
            Id::new("random"),
            K_ORANGE,
            None,
            "Random",
            th,
        )
        .clicked()
        {
            self.random();
        }

        // Open.
        let ox = 1112.0;
        legend(
            ui.painter(),
            at(ox + 27.0 + step, 14.0),
            Align2::CENTER_TOP,
            "Open",
            th,
        );
        if key(ui, k(ox), Id::new("folder"), K_CREAM, None, "Folder", th).clicked() {
            self.open_folder(t);
        }
        if key(
            ui,
            k(ox + step),
            Id::new("files"),
            K_CREAM,
            None,
            "Files",
            th,
        )
        .clicked()
        {
            self.open_files(t);
        }
        if key(
            ui,
            k(ox + step * 2.0),
            Id::new("clear"),
            K_GREY,
            None,
            "Clear",
            th,
        )
        .clicked()
        {
            self.clear();
        }

        // Family: the LED is lit when the app was found.
        let fx = 1326.0;
        legend(
            ui.painter(),
            at(fx + 27.0 + step, 14.0),
            Align2::CENTER_TOP,
            "Family",
            th,
        );
        for (n, a) in App::ALL.into_iter().enumerate() {
            let found = self
                .found
                .get(a.name())
                .map(Option::is_some)
                .unwrap_or(false);
            let r = key(
                ui,
                k(fx + step * n as f32),
                Id::new(("family", a.name())),
                K_RED,
                Some(found),
                key_label(a),
                th,
            );
            let tip = if found {
                format!("Start the {}", a.name())
            } else {
                format!("The {} isn't installed here", a.name())
            };
            if r.on_hover_text(tip).clicked() {
                self.start_family(a, t);
            }
        }
    }

    // ------------------------------------------------------------ left column

    fn left(&mut self, ui: &mut Ui, rect: Rect, th: &Theme) {
        let t = now(ui);
        let np = self.now_playing().map(|(i, secs, m)| (i, secs, m.clone()));
        let current = np
            .as_ref()
            .map(|(i, _, _)| *i)
            .or(self.selected)
            .filter(|&i| i < self.rows.len());

        // Artwork.
        let size = (rect.height() - 372.0).clamp(160.0, LEFT_W);
        let art = Rect::from_min_size(rect.min, Vec2::splat(size));
        well(ui.painter(), art, th);
        let tex = current.and_then(|i| {
            let path = self.rows[i].path.clone();
            self.art.get(ui.ctx(), &path, self.root.as_deref())
        });
        match tex {
            Some(tex) => {
                let s = tex.size_vec2();
                let scale = (art.width() / s.x).min(art.height() / s.y);
                let r = Rect::from_center_size(art.center(), s * scale);
                ui.painter().image(
                    tex.id(),
                    r,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            None => {
                legend(
                    ui.painter(),
                    art.center(),
                    Align2::CENTER_CENTER,
                    if current.is_some() {
                        "No artwork"
                    } else {
                        "Artwork"
                    },
                    th,
                );
            }
        }

        // The track: title, artist, album.
        let mut y = art.bottom() + 14.0;
        let w = rect.width();
        let p = ui.painter();
        if let Some(i) = current {
            let r = &self.rows[i];
            fit(
                p,
                Pos2::new(rect.left(), y),
                Align2::LEFT_TOP,
                &r.title(),
                font(F_LEGEND_BOLD, 20.0),
                th.legend,
                w,
            );
            fit(
                p,
                Pos2::new(rect.left(), y + 27.0),
                Align2::LEFT_TOP,
                &r.artist(),
                font(F_LEGEND, 15.0),
                th.legend,
                w,
            );
            fit(
                p,
                Pos2::new(rect.left(), y + 48.0),
                Align2::LEFT_TOP,
                &r.album(),
                font(F_LEGEND, 13.0),
                th.legend_dim,
                w,
            );
        }
        y += 72.0;

        // What the file is, and what's being done to it.
        if let Some((i, _, m)) = &np {
            let mut x = rect.left();
            x += chip(p, Pos2::new(x, y + 8.0), &m.info.codec, STRIPE[3], true) + 6.0;
            let mut fmt = String::new();
            // ALAC in M4A only gives its bit depth in the file's header.
            let bits = m.info.bits.or_else(|| {
                (m.info.codec == "ALAC")
                    .then(|| self.rows.get(*i).and_then(|r| r.tags.as_ref()?.bits))
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
            chip(p, Pos2::new(x, y + 8.0), &fmt, th.legend_dim, false);
            let gain = self.engine.shared.gain();
            let (l, path) = if m.resampled {
                (
                    Light::Warn,
                    format!("Resampled to {} for this output", rate_label(m.out_rate)),
                )
            } else {
                (
                    Light::Ok,
                    format!("Native rate: {} out", rate_label(m.out_rate)),
                )
            };
            let path = if gain != 1.0 {
                format!("{path} · volume {:.1} dB", self.s.volume_db)
            } else {
                path
            };
            light(p, Pos2::new(rect.left() + 5.0, y + 30.0), l, th);
            fit(
                p,
                Pos2::new(rect.left() + 16.0, y + 30.0),
                Align2::LEFT_CENTER,
                &path,
                font(F_LEGEND, 13.0),
                th.legend,
                w - 16.0,
            );
        }
        y += 48.0;
        // The output row sits on the bottom edge; position and meters
        // above it.
        let row_top = rect.bottom() - 64.0;
        y = y.max(row_top - 146.0);

        // Position.
        self.position_bar(
            ui,
            Rect::from_min_size(Pos2::new(rect.left(), y), Vec2::new(w, 40.0)),
            np.as_ref(),
            th,
        );
        y += 52.0;

        // Meters.
        let p = ui.painter();
        let mr = Rect::from_min_size(Pos2::new(rect.left() + 18.0, y), Vec2::new(w - 18.0, 26.0));
        meter(p, mr, self.meter_db);
        for (n, s) in ["L", "R"].iter().enumerate() {
            text(
                p,
                Pos2::new(rect.left() + 4.0, mr.top() + 7.0 + n as f32 * 12.0),
                Align2::LEFT_CENTER,
                s,
                font(F_LEGEND_BOLD, 10.0),
                th.legend_dim,
                0.0,
            );
        }
        for db in [-48, -36, -24, -12, -6, -3, 0] {
            let x = mr.left() + 3.0 + (mr.width() - 6.0) * (db as f32 + 48.0) / 48.0;
            text(
                p,
                Pos2::new(x, mr.bottom() + 4.0),
                if db == 0 {
                    Align2::RIGHT_TOP
                } else {
                    Align2::CENTER_TOP
                },
                &db.to_string(),
                font(F_LEGEND, 9.5),
                th.legend_dim,
                0.0,
            );
        }

        // Output: volume, device, spectrum and panel.
        let kc = Pos2::new(rect.left() + 24.0, row_top + 24.0);
        let before = self.s.volume_db;
        knob(
            ui,
            kc,
            18.0,
            Id::new("volume"),
            &mut self.s.volume_db,
            &VOLUME,
            Some(FAM_TONE),
            th,
        )
        .on_hover_text("Volume. At 0 dB the samples are left untouched.");
        if self.s.volume_db != before {
            self.engine.send(Cmd::Gain(db_to_gain(self.s.volume_db)));
        }
        let vol = if self.s.volume_db >= 0.0 {
            "0 dB".to_string()
        } else {
            format!("{:.1} dB", self.s.volume_db)
        };
        readout(ui.painter(), Pos2::new(kc.x, kc.y + 26.0), &vol, 52.0);
        legend(
            ui.painter(),
            Pos2::new(kc.x, row_top - 14.0),
            Align2::CENTER_TOP,
            "Volume",
            th,
        );

        let combo = Rect::from_min_size(
            Pos2::new(rect.left() + 66.0, row_top + 12.0),
            Vec2::new(230.0, 24.0),
        );
        legend(
            ui.painter(),
            Pos2::new(combo.left(), row_top - 14.0),
            Align2::LEFT_TOP,
            "Output",
            th,
        );
        let mut items = vec!["System default".to_string()];
        items.extend(self.devices.iter().cloned());
        let sel = self
            .s
            .device
            .as_ref()
            .and_then(|d| items.iter().position(|x| x == d))
            .unwrap_or(0);
        if let Some(n) = lcd_combo(ui, combo, "device", &items, sel) {
            self.s.device = (n > 0).then(|| items[n].clone());
            self.engine.send(Cmd::Device(self.s.device.clone()));
            self.tell(format!("Output: {}", items[n]), t);
        }
        let kr = |x: f32| Rect::from_min_size(Pos2::new(x, row_top + 12.0), Vec2::new(54.0, 24.0));
        if lit_key(
            ui,
            kr(rect.left() + 314.0),
            Id::new("spectrum"),
            LCD_INK,
            self.s.spectrum,
            "Spectrum",
            th,
        )
        .clicked()
        {
            self.s.spectrum = !self.s.spectrum;
        }
        let panel_label = match self.s.panel {
            PanelKind::Black => "Black",
            PanelKind::Silver => "Silver",
        };
        if key(
            ui,
            kr(rect.left() + 382.0),
            Id::new("panel"),
            K_GREY,
            None,
            panel_label,
            th,
        )
        .on_hover_text("Black or Silver panel")
        .clicked()
        {
            self.s.panel = match self.s.panel {
                PanelKind::Black => PanelKind::Silver,
                PanelKind::Silver => PanelKind::Black,
            };
        }
    }

    fn position_bar(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        np: Option<&(usize, f64, Mark)>,
        th: &Theme,
    ) {
        let len = np.and_then(|(i, _, m)| {
            m.info
                .seconds()
                .or_else(|| self.rows.get(*i).and_then(Row::seconds))
        });
        let secs = np.map(|(_, s, _)| *s).unwrap_or(0.0);
        let groove = Rect::from_min_size(
            Pos2::new(rect.left(), rect.top() + 6.0),
            Vec2::new(rect.width(), 8.0),
        );
        let resp = ui.interact(
            groove.expand2(Vec2::new(0.0, 8.0)),
            Id::new("seek"),
            Sense::click_and_drag(),
        );
        let frac_at = |x: f32| ((x - groove.left()) / groove.width()).clamp(0.0, 1.0);
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
        let shown = match (self.seeking, len) {
            (Some(f), Some(_)) => f,
            (_, Some(l)) if l > 0.0 => (secs / l).clamp(0.0, 1.0) as f32,
            _ => 0.0,
        };
        let p = ui.painter();
        p.rect_filled(groove.expand(2.0), CornerRadius::same(3), Color32::BLACK);
        p.rect_filled(groove, CornerRadius::same(2), LCD_BG);
        let fill = Rect::from_min_size(
            groove.min,
            Vec2::new(groove.width() * shown, groove.height()),
        );
        p.rect_filled(fill, CornerRadius::same(2), LCD_INK.gamma_multiply(0.85));
        if len.is_some() {
            let x = groove.left() + groove.width() * shown;
            p.rect_filled(
                Rect::from_center_size(Pos2::new(x, groove.center().y), Vec2::new(4.0, 16.0)),
                CornerRadius::same(1),
                th.legend,
            );
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let shown_secs = match (self.seeking, len) {
            (Some(f), Some(l)) => f as f64 * l,
            _ => secs,
        };
        let p = ui.painter();
        text(
            p,
            Pos2::new(rect.left(), groove.bottom() + 8.0),
            Align2::LEFT_TOP,
            &if np.is_some() {
                time_label(shown_secs)
            } else {
                "-:--".into()
            },
            font(F_MONO, 12.0),
            th.legend_dim,
            0.5,
        );
        text(
            p,
            Pos2::new(rect.right(), groove.bottom() + 8.0),
            Align2::RIGHT_TOP,
            &len.map(time_label).unwrap_or_else(|| "-:--".into()),
            font(F_MONO, 12.0),
            th.legend_dim,
            0.5,
        );
    }

    // ------------------------------------------------------------ right column

    fn spectrum_strip(&self, ui: &Ui, rect: Rect) {
        let p = ui.painter().with_clip_rect(rect.expand(2.0));
        p.rect_filled(rect.expand(2.0), CornerRadius::same(4), Color32::BLACK);
        p.rect_filled(rect, CornerRadius::same(3), LCD_BG);
        let plot = rect.shrink2(Vec2::new(8.0, 6.0));
        let (f_min, f_max) = (20.0f32, 20_000.0f32);
        let f_of = |x: f32| f_min * ((x - plot.left()) / plot.width() * (f_max / f_min).ln()).exp();
        let range = 84.0;
        let lvl = |db: f32| ((db + range) / range).clamp(0.0, 1.0);
        let sr = self.analyzer.sample_rate;
        let mut pts = Vec::new();
        let mut levels = Vec::new();
        let mut x = plot.left();
        while x <= plot.right() {
            let (f0, f1) = (f_of(x - 1.5), f_of(x + 1.5));
            // A 3 dB/octave tilt so music reads level.
            let tilt = 3.0 * (f_of(x) / 1000.0).log2();
            let l = lvl(self.analyzer.level(f0, f1, sr) + tilt);
            pts.push(Pos2::new(x, plot.bottom() - l * plot.height()));
            levels.push(l);
            x += 3.0;
        }
        spectrum::draw(
            &p,
            &pts,
            &levels,
            plot.left(),
            plot.right(),
            plot.bottom(),
            spectrum::Style {
                scheme: Scheme::Display,
                fill: Look::FillAndLine,
                strength: 0.9,
            },
        );
    }

    fn playlist(&mut self, ui: &mut Ui, rect: Rect, th: &Theme) {
        let playing = self.now_playing().map(|(i, _, _)| i);
        well(ui.painter(), rect, th);
        let head = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 26.0));
        let x0 = rect.left() + 12.0;
        let w = rect.width() - 24.0;
        let c_title = x0 + 40.0;
        let c_time = rect.right() - 12.0 - 52.0;
        let flex = c_time - c_title - 16.0;
        let c_artist = c_title + flex * 0.42;
        let c_album = c_title + flex * 0.70;
        let c_type = rect.right() - 12.0;
        lists::header(
            ui.painter(),
            head,
            &[
                (x0 + 24.0, "#", Align2::RIGHT_CENTER),
                (c_title, "Title", Align2::LEFT_CENTER),
                (c_artist, "Artist", Align2::LEFT_CENTER),
                (c_album, "Album", Align2::LEFT_CENTER),
                (c_time, "Time", Align2::RIGHT_CENTER),
                (c_type, "Type", Align2::RIGHT_CENTER),
            ],
            th,
        );
        let _ = w;
        let body = Rect::from_min_max(
            Pos2::new(rect.left(), head.bottom() + 2.0),
            rect.max - Vec2::new(0.0, 2.0),
        );
        if self.rows.is_empty() {
            legend(
                ui.painter(),
                body.center(),
                Align2::CENTER_CENTER,
                "Open a folder, files or a playlist, or drop them here",
                th,
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
                lists::row_bg(p, r, resp, selected == Some(i), th);
                let is_playing = playing == Some(i);
                let ink = if is_playing { LCD_INK } else { th.legend };
                let y = r.center().y;
                if is_playing {
                    // A small play triangle in place of the number.
                    let c = Pos2::new(x0 + 16.0, y);
                    p.add(egui::Shape::convex_polygon(
                        vec![
                            c + Vec2::new(-4.0, -5.0),
                            c + Vec2::new(5.0, 0.0),
                            c + Vec2::new(-4.0, 5.0),
                        ],
                        LCD_INK,
                        Stroke::NONE,
                    ));
                } else {
                    text(
                        p,
                        Pos2::new(x0 + 24.0, y),
                        Align2::RIGHT_CENTER,
                        &(i + 1).to_string(),
                        font(F_MONO, 12.0),
                        th.legend_dim,
                        0.0,
                    );
                }
                let f = font(F_LEGEND, 14.0);
                fit(
                    p,
                    Pos2::new(c_title, y),
                    Align2::LEFT_CENTER,
                    &row.title(),
                    f.clone(),
                    ink,
                    c_artist - c_title - 12.0,
                );
                fit(
                    p,
                    Pos2::new(c_artist, y),
                    Align2::LEFT_CENTER,
                    &row.artist(),
                    f.clone(),
                    th.legend_dim,
                    c_album - c_artist - 12.0,
                );
                fit(
                    p,
                    Pos2::new(c_album, y),
                    Align2::LEFT_CENTER,
                    &row.album(),
                    f.clone(),
                    th.legend_dim,
                    c_time - c_album - 60.0,
                );
                if let Some(s) = row.seconds() {
                    text(
                        p,
                        Pos2::new(c_time, y),
                        Align2::RIGHT_CENTER,
                        &time_label(s),
                        font(F_MONO, 12.0),
                        th.legend_dim,
                        0.0,
                    );
                }
                text(
                    p,
                    Pos2::new(c_type, y),
                    Align2::RIGHT_CENTER,
                    &row.ext,
                    font(F_LEGEND_BOLD, 10.5),
                    th.legend_dim,
                    1.0,
                );
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

    fn footer(&mut self, ui: &mut Ui, panel: Rect, th: &Theme) {
        let t = now(ui);
        let y = panel.bottom() - 20.0;
        let p = ui.painter();
        if let Some((m, at)) = &self.message {
            if t - at < 12.0 {
                fit(
                    p,
                    Pos2::new(panel.left() + 22.0, y),
                    Align2::LEFT_CENTER,
                    m,
                    font(F_LEGEND, 13.0),
                    th.legend,
                    panel.width() * 0.62,
                );
            }
        }
        let mut right = match &self.output {
            Some(o) => format!(
                "{} · {} · {} · {} ch · {}",
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
        };
        let drops = self
            .engine
            .shared
            .underruns
            .load(std::sync::atomic::Ordering::Relaxed);
        if drops > 0 {
            right += &format!(" · {drops} dropout{}", if drops == 1 { "" } else { "s" });
        }
        fit(
            p,
            Pos2::new(panel.right() - 22.0, y),
            Align2::RIGHT_CENTER,
            &right,
            font(F_LEGEND, 12.0),
            th.legend_dim,
            panel.width() * 0.34,
        );
    }
}

fn key_label(a: App) -> &'static str {
    match a {
        App::Library => "Library",
        App::Equaliser => "EQ",
        App::Analyser => "Analyser",
    }
}

fn set_visuals(ctx: &egui::Context, th: &Theme) {
    let mut v = if th.silver {
        egui::Visuals::light()
    } else {
        egui::Visuals::dark()
    };
    v.panel_fill = th.panel;
    v.extreme_bg_color = LCD_BG;
    v.selection.bg_fill = LCD_DIM;
    v.selection.stroke = Stroke::new(1.0, LCD_INK);
    v.text_cursor.stroke = Stroke::new(2.0, LCD_INK);
    if ctx.global_style().visuals != v {
        ctx.set_visuals(v);
    }
}

impl eframe::App for PlayerApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        // Never larger than designed: on a big window the playlist gets the room.
        fit_to_window(ui.ctx(), DESIGN, 1.0);
        let th = Theme::panel(self.s.panel);
        set_visuals(ui.ctx(), &th);
        self.poll(ui);
        self.keys(ui);

        let panel = ui.max_rect();
        draw_panel_bg(ui.painter(), panel, &th);
        self.top_strip(ui, panel, &th);
        stripe(
            ui.painter(),
            Rect::from_min_size(
                panel.min + Vec2::new(22.0, 112.0),
                Vec2::new(panel.width() - 44.0, 6.0),
            ),
        );
        let body = Rect::from_min_max(
            panel.min + Vec2::new(22.0, TOP_ROW),
            panel.max - Vec2::new(22.0, 44.0),
        );
        let left = Rect::from_min_size(body.min, Vec2::new(LEFT_W, body.height()));
        self.left(ui, left, &th);
        let mut right = Rect::from_min_max(Pos2::new(left.right() + 26.0, body.top()), body.max);
        if self.s.spectrum {
            let sp = Rect::from_min_size(right.min, Vec2::new(right.width(), 132.0));
            self.spectrum_strip(ui, sp);
            right.min.y = sp.bottom() + 16.0;
        }
        self.playlist(ui, right, &th);
        self.footer(ui, panel, &th);

        if self.state() == State::Playing || self.seeking.is_some() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(30));
        } else {
            // Keep checking for hand-overs from the Library.
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(500));
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
        assert_eq!(db_to_gain(0.0), 1.0);
        assert!((db_to_gain(-6.0) - 0.501).abs() < 0.001);
    }

    #[test]
    fn saved_settings_tolerate_missing_fields() {
        let s: Saved = serde_json::from_str(r#"{"volume_db": -3.0}"#).unwrap();
        assert_eq!(s.volume_db, -3.0);
        assert!(s.subfolders);
    }
}
