//! From a file's samples to what the output takes: the same channel count
//! as the device, and its sample rate. When the rates match, the samples
//! pass through untouched; only when they don't is anything resampled, and
//! the panel says so.
//!
//! Also the rate decision itself: which rate to open the output at for a
//! track, given what the device can do.

use rubato::{FftFixedIn, Resampler};

/// The rate to open the output at for a track at `src` Hz.
///
/// `supported` is the device's rate ranges (inclusive). The track's own
/// rate wins whenever the device can take it. Otherwise the closest rate
/// in the same family (44.1k or 48k multiples) that's at least the
/// track's, then any rate in that family, then the closest at all.
pub fn choose_rate(src: u32, supported: &[(u32, u32)]) -> u32 {
    if supported.is_empty() || supported.iter().any(|&(lo, hi)| (lo..=hi).contains(&src)) {
        return src;
    }
    const COMMON: [u32; 10] = [
        44_100, 48_000, 88_200, 96_000, 176_400, 192_000, 352_800, 384_000, 32_000, 22_050,
    ];
    let ok = |r: u32| supported.iter().any(|&(lo, hi)| (lo..=hi).contains(&r));
    let family = |r: u32| {
        if src.is_multiple_of(11_025) {
            r.is_multiple_of(11_025)
        } else {
            r.is_multiple_of(8_000)
        }
    };
    let mut candidates: Vec<u32> = COMMON.iter().copied().filter(|&r| ok(r)).collect();
    // Ranges whose ends aren't common rates still count.
    for &(lo, hi) in supported {
        candidates.push(lo);
        candidates.push(hi);
    }
    let dist = |r: u32| (r as i64 - src as i64).unsigned_abs();
    candidates
        .iter()
        .copied()
        .filter(|&r| family(r) && r >= src)
        .min()
        .or_else(|| {
            candidates
                .iter()
                .copied()
                .filter(|&r| family(r))
                .min_by_key(|&r| dist(r))
        })
        .or_else(|| candidates.iter().copied().min_by_key(|&r| dist(r)))
        .unwrap_or(src)
}

const CHUNK: usize = 1024;

pub struct Converter {
    pub src_rate: u32,
    pub out_rate: u32,
    out_ch: usize,
    rs: Option<FftFixedIn<f32>>,
    /// Planar input waiting for a full chunk.
    pending: Vec<Vec<f32>>,
    out: Vec<Vec<f32>>,
    /// Output frames still to drop: the resampler's own delay.
    delay: usize,
    in_total: u64,
    out_total: u64,
}

impl Converter {
    pub fn new(src_rate: u32, out_rate: u32, out_ch: usize) -> Converter {
        let out_ch = out_ch.max(1);
        let rs = (src_rate != out_rate)
            .then(|| FftFixedIn::new(src_rate as usize, out_rate as usize, CHUNK, 2, out_ch).ok())
            .flatten();
        let delay = rs.as_ref().map(|r| r.output_delay()).unwrap_or(0);
        let out = rs
            .as_ref()
            .map(|r| r.output_buffer_allocate(true))
            .unwrap_or_default();
        Converter {
            src_rate,
            out_rate,
            out_ch,
            rs,
            pending: vec![Vec::with_capacity(CHUNK * 2); out_ch],
            out,
            delay,
            in_total: 0,
            out_total: 0,
        }
    }

    pub fn resampling(&self) -> bool {
        self.rs.is_some()
    }

    /// Converts a block of interleaved samples with `ch` channels, adding
    /// interleaved output to `out`.
    pub fn push(&mut self, block: &[f32], ch: usize, out: &mut Vec<f32>) {
        let ch = ch.max(1);
        let frames = block.len() / ch;
        if self.rs.is_none() {
            out.reserve(frames * self.out_ch);
            if ch == self.out_ch {
                out.extend_from_slice(&block[..frames * ch]);
            } else {
                for f in block.chunks_exact(ch) {
                    for c in 0..self.out_ch {
                        out.push(map_channel(f, c));
                    }
                }
            }
            return;
        }
        for f in block.chunks_exact(ch) {
            for c in 0..self.out_ch {
                self.pending[c].push(map_channel(f, c));
            }
        }
        self.in_total += frames as u64;
        while self.pending[0].len() >= CHUNK {
            self.run(out, false);
        }
    }

    /// At the end of the stream: pushes out what the resampler still holds.
    pub fn finish(&mut self, out: &mut Vec<f32>) {
        if self.rs.is_none() {
            return;
        }
        let expected =
            (self.in_total as f64 * self.out_rate as f64 / self.src_rate as f64).round() as u64;
        let mut guard = 0;
        while self.out_total < expected && guard < 64 {
            let before = out.len();
            self.run(out, true);
            guard += 1;
            // Trim anything past the exact length.
            let over = self.out_total.saturating_sub(expected) as usize;
            if over > 0 {
                out.truncate(out.len() - over * self.out_ch);
                self.out_total = expected;
            }
            if out.len() == before && self.pending[0].is_empty() && guard > 8 {
                break;
            }
        }
        self.reset();
    }

    /// Forgets everything in flight (after a seek, or between tracks at
    /// different rates).
    pub fn reset(&mut self) {
        if let Some(r) = self.rs.as_mut() {
            r.reset();
            self.delay = r.output_delay();
        }
        for p in &mut self.pending {
            p.clear();
        }
        self.in_total = 0;
        self.out_total = 0;
    }

    fn run(&mut self, out: &mut Vec<f32>, flush: bool) {
        let Some(rs) = self.rs.as_mut() else { return };
        let need = rs.input_frames_next();
        if flush {
            for p in &mut self.pending {
                p.resize(p.len().max(need), 0.0);
            }
        }
        if self.pending[0].len() < need {
            return;
        }
        let input: Vec<&[f32]> = self.pending.iter().map(|p| &p[..need]).collect();
        let Ok((used, made)) = rs.process_into_buffer(&input, &mut self.out, None) else {
            for p in &mut self.pending {
                p.clear();
            }
            return;
        };
        for p in &mut self.pending {
            p.drain(..used);
        }
        let skip = self.delay.min(made);
        self.delay -= skip;
        out.reserve((made - skip) * self.out_ch);
        for i in skip..made {
            for c in 0..self.out_ch {
                out.push(self.out[c][i]);
            }
        }
        self.out_total += (made - skip) as u64;
    }
}

/// Output channel `c` from one input frame: mono goes to both sides; extra
/// output channels beyond the file's are silent.
fn map_channel(frame: &[f32], c: usize) -> f32 {
    match frame.len() {
        1 if c < 2 => frame[0],
        n if c < n => frame[c],
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_rate_when_the_device_takes_it() {
        assert_eq!(choose_rate(44_100, &[(8_000, 384_000)]), 44_100);
        assert_eq!(
            choose_rate(96_000, &[(44_100, 44_100), (96_000, 96_000)]),
            96_000
        );
        assert_eq!(choose_rate(44_100, &[]), 44_100);
    }

    #[test]
    fn fixed_rate_device_resamples_to_it() {
        // Windows shared mode: only the mix rate.
        assert_eq!(choose_rate(44_100, &[(48_000, 48_000)]), 48_000);
        assert_eq!(choose_rate(192_000, &[(48_000, 48_000)]), 48_000);
    }

    #[test]
    fn same_family_preferred() {
        let dev = [
            (44_100, 44_100),
            (48_000, 48_000),
            (88_200, 88_200),
            (96_000, 96_000),
        ];
        assert_eq!(choose_rate(176_400, &dev), 88_200);
        assert_eq!(choose_rate(192_000, &dev), 96_000);
        assert_eq!(choose_rate(22_050, &dev), 44_100);
        assert_eq!(choose_rate(32_000, &dev), 48_000);
    }

    #[test]
    fn matching_rates_pass_through_untouched() {
        let mut c = Converter::new(44_100, 44_100, 2);
        assert!(!c.resampling());
        let input: Vec<f32> = (0..2000).map(|i| (i as f32 * 0.37).sin()).collect();
        let mut out = Vec::new();
        c.push(&input, 2, &mut out);
        c.finish(&mut out);
        // Bit for bit.
        assert_eq!(out, input);
    }

    #[test]
    fn mono_goes_to_both_sides() {
        let mut c = Converter::new(48_000, 48_000, 2);
        let mut out = Vec::new();
        c.push(&[0.1, 0.2], 1, &mut out);
        assert_eq!(out, vec![0.1, 0.1, 0.2, 0.2]);
    }

    #[test]
    fn resampling_keeps_length_and_level() {
        let (src, dst) = (44_100u32, 48_000u32);
        let mut c = Converter::new(src, dst, 2);
        assert!(c.resampling());
        let n = 44_100usize; // one second
        let mut out = Vec::new();
        // Fed in odd-sized blocks, as a decoder would.
        let tone: Vec<f32> = (0..n)
            .flat_map(|i| {
                let v = (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / src as f32).sin() * 0.5;
                [v, v]
            })
            .collect();
        for b in tone.chunks(1152 * 2) {
            c.push(b, 2, &mut out);
        }
        c.finish(&mut out);
        assert_eq!(out.len() / 2, 48_000);
        // Level in the middle is still about 0.5 peak.
        let mid = &out[20_000..60_000];
        let peak = mid.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!((peak - 0.5).abs() < 0.02, "peak {peak}");
        // And it starts on time: the first samples are near zero, rising.
        assert!(out[0].abs() < 0.05, "{}", out[0]);
    }
}
