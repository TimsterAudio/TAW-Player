//! The playback engine.
//!
//! A player thread decodes tracks one after another into a lock-free ring
//! buffer, and the output callback only reads from it. Because the next
//! track is decoded straight on from the last one into the same stream,
//! tracks at the same rate join with no gap. A track at a different rate
//! needs the output reopened at its rate: the ring plays out first, then
//! the output is reopened (a short, honest gap between albums).
//!
//! The callback never locks, allocates, logs or touches files. It shares
//! only atomics with the rest of the Player (`Shared`), plus the ring.

use crate::convert::{choose_rate, Converter};
use crate::decode::{Source, StreamInfo};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use std::path::PathBuf;
use std::sync::atomic::Ordering::{Acquire, Relaxed, Release};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use taw_panel::analyzer::{analyzer_channel, AnalyzerTap};

/// What the panel asks of the player thread.
pub enum Cmd {
    /// The list to play from (doesn't start playing).
    Queue(Vec<PathBuf>),
    /// Play track `i` of the list from its start.
    Play(usize),
    /// Seek within the track that's playing.
    Seek(f64),
    Pause(bool),
    Stop,
    /// Output device by name; None for the system default.
    Device(Option<String>),
    /// Volume as a gain (1.0 = untouched).
    Gain(f32),
}

/// Where a track begins in the output stream.
#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    /// Output frame (in `Shared::played` units) where it starts.
    pub start: i64,
    /// Seconds into the track at `start` (after a seek).
    pub offset: f64,
    /// Index in the list; None marks the end of the list.
    pub index: Option<usize>,
    pub info: StreamInfo,
    pub out_rate: u32,
    pub resampled: bool,
}

/// The track at output position `played`, and seconds into it.
pub fn locate(marks: &[Mark], played: u64) -> Option<(&Mark, f64)> {
    let m = marks.iter().rev().find(|m| m.start <= played as i64)?;
    let secs = m.offset + (played as i64 - m.start) as f64 / m.out_rate.max(1) as f64;
    Some((m, secs))
}

#[derive(Clone, Debug, Default)]
pub struct OutputInfo {
    pub host: String,
    pub device: String,
    pub rate: u32,
    pub channels: u16,
    pub format: String,
}

/// What the player thread reports; only it writes this.
#[derive(Default)]
pub struct Status {
    pub marks: Vec<Mark>,
    pub output: Option<OutputInfo>,
    /// A message for the panel (couldn't open a file or device).
    pub message: Option<String>,
    /// A new feed for the spectrum, after the output was (re)opened.
    pub analyzer: Option<rtrb::Consumer<f32>>,
    pub devices: Vec<String>,
}

/// The atomics the callback shares with everyone else.
pub struct Shared {
    pub paused: AtomicBool,
    flush: AtomicBool,
    /// Frames the callback has taken from the ring (or flushed), ever.
    pub played: AtomicU64,
    /// Peak level per side since the panel last looked, as f32 bits.
    pub peaks: [AtomicU32; 2],
    /// Times the callback ran short of audio while it was playing.
    pub underruns: AtomicU32,
    /// True while there's more to play (so running short is an underrun).
    feeding: AtomicBool,
    gain: AtomicU32,
    pub status: Mutex<Status>,
}

impl Shared {
    pub fn take_peaks(&self) -> [f32; 2] {
        [
            f32::from_bits(self.peaks[0].swap(0, Relaxed)),
            f32::from_bits(self.peaks[1].swap(0, Relaxed)),
        ]
    }
    pub fn gain(&self) -> f32 {
        f32::from_bits(self.gain.load(Relaxed))
    }
}

/// The panel's handle on the engine.
pub struct Engine {
    tx: Sender<Cmd>,
    pub shared: Arc<Shared>,
}

impl Engine {
    pub fn start(device: Option<String>, gain: f32) -> Engine {
        let shared = Arc::new(Shared {
            paused: AtomicBool::new(false),
            flush: AtomicBool::new(false),
            played: AtomicU64::new(0),
            peaks: [AtomicU32::new(0), AtomicU32::new(0)],
            underruns: AtomicU32::new(0),
            feeding: AtomicBool::new(false),
            gain: AtomicU32::new(gain.to_bits()),
            status: Mutex::new(Status::default()),
        });
        let (tx, rx) = channel();
        let s = shared.clone();
        let _ = std::thread::Builder::new()
            .name("player".into())
            .spawn(move || Player::new(s, rx, device).run());
        Engine { tx, shared }
    }

    pub fn send(&self, c: Cmd) {
        if let Cmd::Pause(p) = c {
            // Pausing is instant: the callback sees it on its next run.
            self.shared.paused.store(p, Relaxed);
        }
        if let Cmd::Gain(g) = c {
            self.shared.gain.store(g.to_bits(), Relaxed);
        }
        let _ = self.tx.send(c);
    }

    pub fn played(&self) -> u64 {
        self.shared.played.load(Relaxed)
    }
}

// ---------------------------------------------------------------- output

struct Output {
    _stream: cpal::Stream,
    ring: rtrb::Producer<f32>,
    info: OutputInfo,
    channels: usize,
}

/// PipeWire on Linux when it's running (ALSA otherwise), WASAPI on
/// Windows. `TAW_PLAYER_HOST=alsa` (or any cpal host name) forces one.
fn host() -> cpal::Host {
    if let Ok(name) = std::env::var("TAW_PLAYER_HOST") {
        if let Some(id) = cpal::available_hosts()
            .into_iter()
            .find(|id| id.name().eq_ignore_ascii_case(&name))
        {
            if let Ok(h) = cpal::host_from_id(id) {
                return h;
            }
        }
    }
    #[cfg(target_os = "linux")]
    if let Ok(h) = cpal::host_from_id(cpal::HostId::PipeWire) {
        if h.default_output_device().is_some() {
            return h;
        }
    }
    cpal::default_host()
}

fn device_name(d: &cpal::Device) -> String {
    d.description()
        .map(|d| d.name().to_string())
        .unwrap_or_else(|_| "Unnamed device".into())
}

fn device_names(h: &cpal::Host) -> Vec<String> {
    let mut v: Vec<String> = h
        .output_devices()
        .map(|it| it.map(|d| device_name(&d)).collect())
        .unwrap_or_default();
    v.retain(|n| !n.starts_with("taw_equaliser") && !n.starts_with("default_"));
    v.dedup();
    v
}

fn find_device(h: &cpal::Host, name: Option<&str>) -> Option<cpal::Device> {
    if let Some(n) = name {
        if let Ok(mut it) = h.output_devices() {
            if let Some(d) = it.find(|d| device_name(d) == n) {
                return Some(d);
            }
        }
    }
    h.default_output_device()
}

/// The rates a device can be opened at. On Windows the shared-mode mixer
/// runs at one rate, so that's the only one offered: anything else is
/// resampled here, in the open, rather than quietly by Windows.
fn device_rates(d: &cpal::Device) -> Vec<(u32, u32)> {
    if cfg!(windows) {
        return d
            .default_output_config()
            .map(|c| vec![(c.sample_rate(), c.sample_rate())])
            .unwrap_or_default();
    }
    d.supported_output_configs()
        .map(|it| {
            it.map(|r| (r.min_sample_rate(), r.max_sample_rate()))
                .collect()
        })
        .unwrap_or_default()
}

fn format_rank(f: SampleFormat) -> u8 {
    match f {
        SampleFormat::F32 => 0,
        SampleFormat::I32 => 1,
        SampleFormat::F64 => 2,
        SampleFormat::I16 => 3,
        _ => 9,
    }
}

fn open_output(
    h: &cpal::Host,
    device: Option<&str>,
    rate: u32,
    shared: &Arc<Shared>,
) -> Result<(Output, rtrb::Consumer<f32>), String> {
    let dev = find_device(h, device).ok_or("There's no audio output device.")?;
    let default = dev.default_output_config().map_err(|e| e.to_string())?;
    let want_ch = default.channels();
    let ranges: Vec<_> = dev
        .supported_output_configs()
        .map(|it| it.collect())
        .unwrap_or_default();
    let pick = ranges
        .iter()
        .filter(|r| format_rank(r.sample_format()) < 9)
        .filter(|r| (r.min_sample_rate()..=r.max_sample_rate()).contains(&rate))
        .min_by_key(|r| (r.channels() != want_ch, format_rank(r.sample_format())))
        .and_then(|r| r.try_with_sample_rate(rate));
    let supported = match pick {
        Some(s) => s,
        None if default.sample_rate() == rate => default,
        None => return Err(format!("The output can't run at {rate} Hz.")),
    };
    let fmt = supported.sample_format();
    let cfg: cpal::StreamConfig = supported.config();
    let ch = cfg.channels as usize;
    // A second and a half of audio, in whole frames.
    let cap = (rate as usize * 3 / 2).max(4096) * ch;
    let (prod, cons) = rtrb::RingBuffer::<f32>::new(cap);
    let (tap, tap_rx) = analyzer_channel();
    let on_error = |_e: cpal::Error| {};
    macro_rules! build {
        ($T:ty) => {
            build_stream::<$T>(&dev, &cfg, cons, tap, shared.clone(), on_error)
        };
    }
    let stream = match fmt {
        SampleFormat::F32 => build!(f32),
        SampleFormat::F64 => build!(f64),
        SampleFormat::I32 => build!(i32),
        SampleFormat::I16 => build!(i16),
        other => {
            return Err(format!(
                "The output wants {other} samples, which isn't supported."
            ))
        }
    }?;
    stream.play().map_err(|e| e.to_string())?;
    let info = OutputInfo {
        host: h.id().name().to_string(),
        device: device_name(&dev),
        rate,
        channels: cfg.channels,
        format: format!("{fmt}").to_uppercase(),
    };
    Ok((
        Output {
            _stream: stream,
            ring: prod,
            info,
            channels: ch,
        },
        tap_rx,
    ))
}

fn build_stream<T>(
    dev: &cpal::Device,
    cfg: &cpal::StreamConfig,
    mut ring: rtrb::Consumer<f32>,
    mut tap: AnalyzerTap,
    shared: Arc<Shared>,
    on_error: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32> + Send + 'static,
{
    let ch = cfg.channels as usize;
    dev.build_output_stream(
        *cfg,
        move |data: &mut [T], _| {
            if shared.flush.load(Acquire) {
                let n = ring.slots();
                if let Ok(c) = ring.read_chunk(n) {
                    c.commit_all();
                }
                shared.played.fetch_add((n / ch) as u64, Relaxed);
                shared.flush.store(false, Release);
            }
            let silence = T::from_sample_(0.0f32);
            if shared.paused.load(Relaxed) {
                data.fill(silence);
                return;
            }
            let gain = f32::from_bits(shared.gain.load(Relaxed));
            let n = ring.slots().min(data.len()) / ch * ch;
            let mut peak = [0.0f32; 2];
            if let Ok(chunk) = ring.read_chunk(n) {
                let (a, b) = chunk.as_slices();
                let mut i = 0;
                for part in [a, b] {
                    for &s in part {
                        let v = if gain == 1.0 { s } else { s * gain };
                        let side = (i % ch).min(1);
                        peak[side] = peak[side].max(v.abs());
                        data[i] = T::from_sample_(v);
                        i += 1;
                    }
                    tap.push(part, ch);
                }
                chunk.commit_all();
            }
            if n < data.len() {
                data[n..].fill(silence);
                if shared.feeding.load(Relaxed) {
                    shared.underruns.fetch_add(1, Relaxed);
                }
            }
            if ch == 1 {
                peak[1] = peak[0];
            }
            // Positive floats order the same as their bits.
            shared.peaks[0].fetch_max(peak[0].to_bits(), Relaxed);
            shared.peaks[1].fetch_max(peak[1].to_bits(), Relaxed);
            shared.played.fetch_add((n / ch) as u64, Relaxed);
        },
        on_error,
        None,
    )
    .map_err(|e| format!("Couldn't open the output: {e}"))
}

// ---------------------------------------------------------------- player

struct Player {
    host: cpal::Host,
    /// The chosen device's rates, looked up once.
    rates: Option<Vec<(u32, u32)>>,
    shared: Arc<Shared>,
    rx: Receiver<Cmd>,
    queue: Vec<PathBuf>,
    device: Option<String>,
    out: Option<Output>,
    /// The track being decoded, and its index.
    src: Option<(usize, Source)>,
    conv: Option<Converter>,
    /// Converted samples not yet in the ring, and how far in we are.
    pending: Vec<f32>,
    pending_pos: usize,
    /// Frames written to the ring, ever (same units as `played`).
    written: u64,
    marks: Vec<Mark>,
    /// The next track wants another rate: play out the ring, then reopen.
    switch_to: Option<(usize, Source, u32)>,
    /// The end of the list has been decoded; mark it once it's written.
    ending: bool,
    message: Option<String>,
    status_dirty: bool,
    new_analyzer: Option<rtrb::Consumer<f32>>,
}

impl Player {
    fn new(shared: Arc<Shared>, rx: Receiver<Cmd>, device: Option<String>) -> Player {
        Player {
            host: host(),
            rates: None,
            shared,
            rx,
            queue: Vec::new(),
            device,
            out: None,
            src: None,
            conv: None,
            pending: Vec::new(),
            pending_pos: 0,
            written: 0,
            marks: Vec::new(),
            switch_to: None,
            ending: false,
            message: None,
            status_dirty: true,
            new_analyzer: None,
        }
    }

    fn run(mut self) {
        if let Ok(mut st) = self.shared.status.lock() {
            st.devices = device_names(&self.host);
        }
        loop {
            let busy = self.src.is_some() || self.switch_to.is_some() || self.has_pending();
            let wait = if busy {
                Duration::from_millis(10)
            } else {
                Duration::from_millis(200)
            };
            match self.rx.recv_timeout(wait) {
                Ok(c) => {
                    self.handle(c);
                    while let Ok(c) = self.rx.try_recv() {
                        self.handle(c);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            self.work();
            self.publish();
        }
    }

    fn has_pending(&self) -> bool {
        self.pending_pos < self.pending.len()
    }

    fn handle(&mut self, c: Cmd) {
        match c {
            Cmd::Queue(q) => self.queue = q,
            Cmd::Play(i) => {
                self.shared.paused.store(false, Relaxed);
                self.jump(i, 0.0);
            }
            Cmd::Seek(secs) => {
                let played = self.shared.played.load(Relaxed);
                let current = locate(&self.marks, played).and_then(|(m, _)| m.index);
                if let Some(i) = current {
                    self.jump(i, secs);
                }
            }
            Cmd::Pause(_) => {} // already applied by Engine::send
            Cmd::Stop => {
                self.flush();
                self.src = None;
                self.conv = None;
                self.switch_to = None;
                self.ending = false;
                self.marks.clear();
                self.shared.feeding.store(false, Relaxed);
                self.shared.paused.store(false, Relaxed);
                self.status_dirty = true;
            }
            Cmd::Device(d) => {
                let played = self.shared.played.load(Relaxed);
                let at =
                    locate(&self.marks, played).and_then(|(m, secs)| m.index.map(|i| (i, secs)));
                self.flush();
                self.device = d;
                self.out = None;
                self.rates = None;
                self.marks.clear();
                if let Some((i, secs)) = at {
                    self.jump(i, secs);
                }
                self.status_dirty = true;
            }
            Cmd::Gain(_) => {}
        }
    }

    /// Empties the ring and everything waiting for it.
    fn flush(&mut self) {
        self.pending.clear();
        self.pending_pos = 0;
        if self.out.is_some() {
            self.shared.flush.store(true, Release);
            let t = Instant::now();
            while self.shared.flush.load(Acquire) && t.elapsed() < Duration::from_millis(300) {
                std::thread::sleep(Duration::from_millis(1));
            }
            if self.shared.flush.load(Acquire) {
                // The callback isn't running (device gone?): start afresh.
                self.shared.flush.store(false, Release);
                self.out = None;
            }
        }
        self.written = self.shared.played.load(Relaxed);
    }

    /// Plays track `i` from `secs`, skipping tracks that can't be read.
    fn jump(&mut self, i: usize, secs: f64) {
        self.flush();
        self.src = None;
        self.switch_to = None;
        self.ending = false;
        self.marks.clear();
        self.status_dirty = true;
        let mut i = i;
        while i < self.queue.len() {
            match Source::open(&self.queue[i]) {
                Ok(mut s) => {
                    let offset = if secs > 0.0 {
                        s.seek(secs).unwrap_or(0.0)
                    } else {
                        0.0
                    };
                    let rate = self.rate_for(&s.info);
                    if self.out.as_ref().map(|o| o.info.rate) != Some(rate) {
                        self.out = None;
                        if let Err(e) = self.open(rate) {
                            self.message = Some(e);
                            self.shared.feeding.store(false, Relaxed);
                            return;
                        }
                    }
                    self.begin(i, s, offset, false);
                    return;
                }
                Err(e) => {
                    self.message = Some(format!("Skipped {}: {e}", file_name(&self.queue[i])));
                    i += 1;
                }
            }
        }
        self.shared.feeding.store(false, Relaxed);
    }

    fn rate_for(&mut self, info: &StreamInfo) -> u32 {
        if self.rates.is_none() {
            self.rates = Some(
                find_device(&self.host, self.device.as_deref())
                    .map(|d| device_rates(&d))
                    .unwrap_or_default(),
            );
        }
        choose_rate(info.rate, self.rates.as_deref().unwrap_or(&[]))
    }

    fn open(&mut self, rate: u32) -> Result<(), String> {
        let (out, tap_rx) = open_output(&self.host, self.device.as_deref(), rate, &self.shared)
            .or_else(|e| {
                // A device that's gone: fall back to the default.
                if self.device.is_some() {
                    open_output(&self.host, None, rate, &self.shared)
                } else {
                    Err(e)
                }
            })?;
        self.written = self.shared.played.load(Relaxed);
        self.out = Some(out);
        self.new_analyzer = Some(tap_rx);
        self.status_dirty = true;
        Ok(())
    }

    /// Starts decoding track `i` into the stream, after what's queued.
    /// `continuing` means straight on from the track before, in the same
    /// stream: a resampler then carries on too, so the join stays seamless.
    fn begin(&mut self, i: usize, s: Source, offset: f64, continuing: bool) {
        let Some(out) = self.out.as_ref() else { return };
        let out_rate = out.info.rate;
        let same = self
            .conv
            .as_ref()
            .map(|c| c.src_rate == s.info.rate && c.out_rate == out_rate)
            .unwrap_or(false);
        if !(continuing && same) {
            if continuing {
                if let Some(c) = self.conv.as_mut() {
                    c.finish(&mut self.pending);
                }
            }
            self.conv = Some(Converter::new(s.info.rate, out_rate, out.channels));
        }
        let start =
            self.written as i64 + ((self.pending.len() - self.pending_pos) / out.channels) as i64;
        self.marks.push(Mark {
            start,
            offset,
            index: Some(i),
            info: s.info.clone(),
            out_rate,
            resampled: self.conv.as_ref().map(|c| c.resampling()).unwrap_or(false),
        });
        self.src = Some((i, s));
        self.status_dirty = true;
    }

    /// Keeps the ring full.
    fn work(&mut self) {
        // Waiting to reopen the output at another rate?
        if let Some((_, _, rate)) = self.switch_to.as_ref() {
            let rate = *rate;
            let drained = !self.has_pending() && self.shared.played.load(Relaxed) >= self.written;
            if !drained {
                self.push_pending();
                return;
            }
            let (i, s, _) = self.switch_to.take().unwrap();
            self.out = None;
            self.conv = None;
            if let Err(e) = self.open(rate) {
                self.message = Some(e);
                self.shared.feeding.store(false, Relaxed);
                return;
            }
            self.begin(i, s, 0.0, false);
        }
        let Some(ch) = self.out.as_ref().map(|o| o.channels) else {
            return;
        };
        loop {
            if self.has_pending() && !self.push_pending() {
                return; // the ring is full
            }
            if self.src.is_none() {
                if self.ending && !self.has_pending() {
                    self.ending = false;
                    let start = self.written as i64;
                    let out_rate = self.out.as_ref().map(|o| o.info.rate).unwrap_or(48_000);
                    self.marks.push(Mark {
                        start,
                        offset: 0.0,
                        index: None,
                        info: StreamInfo::default(),
                        out_rate,
                        resampled: false,
                    });
                    self.status_dirty = true;
                }
                if !self.has_pending() {
                    // Everything's written: the callback will run dry
                    // at the end, which isn't an underrun.
                    if self.marks.last().map(|m| m.index.is_none()).unwrap_or(true) {
                        self.shared.feeding.store(false, Relaxed);
                    }
                }
                return;
            }
            self.pending.clear();
            self.pending_pos = 0;
            let (_, src) = self.src.as_mut().unwrap();
            match src.next_block() {
                Some((block, bch)) => {
                    if let Some(c) = self.conv.as_mut() {
                        c.push(block, bch, &mut self.pending);
                    }
                }
                None => self.next_track(ch),
            }
        }
    }

    /// The decoder reached the end of a track: on to the next one.
    fn next_track(&mut self, _ch: usize) {
        let (i, _) = self.src.take().unwrap();
        let mut n = i + 1;
        while n < self.queue.len() {
            match Source::open(&self.queue[n]) {
                Ok(s) => {
                    let rate = self.rate_for(&s.info);
                    let out_rate = self.out.as_ref().map(|o| o.info.rate).unwrap_or(0);
                    if rate == out_rate {
                        // Same stream: straight on, no gap.
                        self.begin(n, s, 0.0, true);
                    } else {
                        if let Some(c) = self.conv.as_mut() {
                            c.finish(&mut self.pending);
                        }
                        self.switch_to = Some((n, s, rate));
                        // Running dry before the reopen is on purpose.
                        self.shared.feeding.store(false, Relaxed);
                    }
                    return;
                }
                Err(e) => {
                    self.message = Some(format!("Skipped {}: {e}", file_name(&self.queue[n])));
                    n += 1;
                }
            }
        }
        if let Some(c) = self.conv.as_mut() {
            c.finish(&mut self.pending);
        }
        self.ending = true;
    }

    /// Moves what fits into the ring. False if some is still waiting.
    fn push_pending(&mut self) -> bool {
        let Some(out) = self.out.as_mut() else {
            return true;
        };
        let ch = out.channels;
        let left = self.pending.len() - self.pending_pos;
        let n = left.min(out.ring.slots()) / ch * ch;
        if n > 0 {
            if let Ok(mut chunk) = out.ring.write_chunk(n) {
                let (a, b) = chunk.as_mut_slices();
                let src = &self.pending[self.pending_pos..self.pending_pos + n];
                a.copy_from_slice(&src[..a.len()]);
                b.copy_from_slice(&src[a.len()..]);
                chunk.commit_all();
                self.pending_pos += n;
                self.written += (n / ch) as u64;
                // Only now can running short count as a dropout.
                if self.switch_to.is_none() {
                    self.shared.feeding.store(true, Relaxed);
                }
            }
        }
        self.pending_pos >= self.pending.len()
    }

    fn publish(&mut self) {
        let played = self.shared.played.load(Relaxed);
        // Forget tracks that have finished playing.
        if self.marks.len() > 1 {
            let keep_from = self
                .marks
                .iter()
                .rposition(|m| m.start <= played as i64)
                .unwrap_or(0);
            if keep_from > 0 {
                self.marks.drain(..keep_from);
                self.status_dirty = true;
            }
        }
        if !self.status_dirty && self.message.is_none() && self.new_analyzer.is_none() {
            return;
        }
        if let Ok(mut st) = self.shared.status.lock() {
            st.marks = self.marks.clone();
            st.output = self.out.as_ref().map(|o| o.info.clone());
            if let Some(m) = self.message.take() {
                st.message = Some(m);
            }
            if let Some(a) = self.new_analyzer.take() {
                st.analyzer = Some(a);
            }
            self.status_dirty = false;
        }
    }
}

fn file_name(p: &std::path::Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mark(start: i64, index: Option<usize>, offset: f64) -> Mark {
        Mark {
            start,
            offset,
            index,
            info: StreamInfo::default(),
            out_rate: 1000,
            resampled: false,
        }
    }

    #[test]
    fn locate_finds_the_playing_track() {
        let marks = vec![
            mark(0, Some(0), 0.0),
            mark(5000, Some(1), 0.0),
            mark(9000, None, 0.0),
        ];
        assert_eq!(locate(&marks, 0).unwrap().0.index, Some(0));
        let (m, secs) = locate(&marks, 4999).unwrap();
        assert_eq!(m.index, Some(0));
        assert!((secs - 4.999).abs() < 1e-9);
        let (m, secs) = locate(&marks, 6500).unwrap();
        assert_eq!(m.index, Some(1));
        assert!((secs - 1.5).abs() < 1e-9);
        assert_eq!(locate(&marks, 9000).unwrap().0.index, None);
    }

    #[test]
    fn locate_after_a_seek() {
        let marks = vec![mark(100, Some(3), 30.0)];
        let (_, secs) = locate(&marks, 1100).unwrap();
        assert!((secs - 31.0).abs() < 1e-9);
        assert!(locate(&marks, 50).is_none());
    }
}
