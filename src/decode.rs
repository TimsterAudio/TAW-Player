//! Reading one audio file into blocks of interleaved f32 samples, with
//! symphonia. Encoder delay and padding are trimmed where the file says
//! what they are (MP3 with a LAME/Xing header), so gapless albums join up.

use std::fs::File;
use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{Decoder, DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;

/// Plain facts about a file's audio, as decoded.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StreamInfo {
    pub rate: u32,
    pub channels: usize,
    /// Bits per sample for lossless formats; None for lossy ones.
    pub bits: Option<u32>,
    /// "FLAC", "MP3", "AAC", "ALAC", "VORBIS", "WAV", "AIFF".
    pub codec: String,
    /// Length in frames, when the file says.
    pub frames: Option<u64>,
}

impl StreamInfo {
    pub fn seconds(&self) -> Option<f64> {
        self.frames
            .filter(|_| self.rate > 0)
            .map(|f| f as f64 / self.rate as f64)
    }
}

pub struct Source {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    pub info: StreamInfo,
    buf: Option<SampleBuffer<f32>>,
    /// Channels in the last block (a file can change, rarely).
    pub block_channels: usize,
}

fn codec_label(codec: symphonia::core::codecs::CodecType, path: &Path) -> String {
    use symphonia::core::codecs::*;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    match codec {
        CODEC_TYPE_FLAC => "FLAC".into(),
        CODEC_TYPE_MP3 | CODEC_TYPE_MP2 | CODEC_TYPE_MP1 => "MP3".into(),
        CODEC_TYPE_AAC => "AAC".into(),
        CODEC_TYPE_ALAC => "ALAC".into(),
        CODEC_TYPE_VORBIS => "VORBIS".into(),
        CODEC_TYPE_OPUS => "OPUS".into(),
        _ if ext.starts_with("aif") => "AIFF".into(),
        _ if ext.starts_with("wav") => "WAV".into(),
        _ => get_codecs_name(codec),
    }
}

fn get_codecs_name(codec: symphonia::core::codecs::CodecType) -> String {
    symphonia::default::get_codecs()
        .get_codec(codec)
        .map(|d| d.short_name.to_uppercase())
        .unwrap_or_else(|| "AUDIO".into())
}

impl Source {
    pub fn open(path: &Path) -> Result<Source, String> {
        let file = File::open(path).map_err(|e| e.to_string())?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }
        let fmt_opts = FormatOptions {
            enable_gapless: true,
            ..Default::default()
        };
        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &fmt_opts, &MetadataOptions::default())
            .map_err(|e| format!("not a format the Player can read ({e})"))?;
        let format = probed.format;
        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or("no audio in the file")?;
        let params = &track.codec_params;
        let decoder = symphonia::default::get_codecs()
            .make(params, &DecoderOptions::default())
            .map_err(|e| format!("can't decode it ({e})"))?;
        let lossless = params.bits_per_sample.is_some()
            && !matches!(
                params.codec,
                symphonia::core::codecs::CODEC_TYPE_MP3
                    | symphonia::core::codecs::CODEC_TYPE_AAC
                    | symphonia::core::codecs::CODEC_TYPE_VORBIS
            );
        let info = StreamInfo {
            rate: params.sample_rate.unwrap_or(0),
            channels: params.channels.map(|c| c.count()).unwrap_or(0),
            bits: if lossless {
                params.bits_per_sample
            } else {
                None
            },
            codec: codec_label(params.codec, path),
            frames: params.n_frames,
        };
        let track_id = track.id;
        let mut s = Source {
            format,
            decoder,
            track_id,
            info,
            buf: None,
            block_channels: 0,
        };
        // Some files only say their rate and channels once decoding starts.
        if s.info.rate == 0 || s.info.channels == 0 {
            return Err("the file doesn't say its sample rate".into());
        }
        s.block_channels = s.info.channels;
        Ok(s)
    }

    /// The next block of interleaved samples, or None at the end of the
    /// file. Damaged packets are skipped rather than stopping playback.
    pub fn next_block(&mut self) -> Option<(&[f32], usize)> {
        let mut errors = 0;
        loop {
            let packet = match self.format.next_packet() {
                Ok(p) => p,
                Err(Error::ResetRequired) => return None,
                Err(_) => return None, // end of stream, or unreadable
            };
            if packet.track_id() != self.track_id {
                continue;
            }
            match self.decoder.decode(&packet) {
                Ok(decoded) => {
                    let spec = *decoded.spec();
                    let frames = decoded.frames();
                    if frames == 0 {
                        continue;
                    }
                    let need = decoded.capacity() as u64;
                    let fits = self
                        .buf
                        .as_ref()
                        .map(|b| b.capacity() as u64 >= need * spec.channels.count() as u64)
                        .unwrap_or(false);
                    if !fits || self.block_channels != spec.channels.count() {
                        self.buf = Some(SampleBuffer::new(need, spec));
                    }
                    self.block_channels = spec.channels.count();
                    let buf = self.buf.as_mut()?;
                    buf.copy_interleaved_ref(decoded);
                    return Some((buf.samples(), self.block_channels));
                }
                Err(Error::DecodeError(_)) | Err(Error::IoError(_)) => {
                    errors += 1;
                    if errors > 100 {
                        return None;
                    }
                }
                Err(_) => return None,
            }
        }
    }

    /// Jumps to `secs` into the file; returns where it actually landed.
    pub fn seek(&mut self, secs: f64) -> Result<f64, String> {
        let secs = secs.max(0.0);
        let to = SeekTo::Time {
            time: Time::new(secs.trunc() as u64, secs.fract()),
            track_id: Some(self.track_id),
        };
        let r = self
            .format
            .seek(SeekMode::Accurate, to)
            .map_err(|e| e.to_string())?;
        self.decoder.reset();
        Ok(r.actual_ts as f64 / self.info.rate.max(1) as f64)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Writes a 16-bit PCM WAV of `frames` frames of a sine at `freq`,
    /// starting at phase `start_frame` so consecutive files continue it.
    pub fn sine_wav(
        path: &Path,
        rate: u32,
        channels: u16,
        freq: f64,
        start_frame: u64,
        frames: u64,
    ) {
        let mut data = Vec::with_capacity(frames as usize * channels as usize * 2);
        for n in start_frame..start_frame + frames {
            let v = (2.0 * std::f64::consts::PI * freq * n as f64 / rate as f64).sin() * 0.5;
            let s = (v * 32767.0).round() as i16;
            for _ in 0..channels {
                data.extend_from_slice(&s.to_le_bytes());
            }
        }
        let mut w = Vec::new();
        let block = channels as u32 * 2;
        w.extend_from_slice(b"RIFF");
        w.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        w.extend_from_slice(b"WAVEfmt ");
        w.extend_from_slice(&16u32.to_le_bytes());
        w.extend_from_slice(&1u16.to_le_bytes());
        w.extend_from_slice(&channels.to_le_bytes());
        w.extend_from_slice(&rate.to_le_bytes());
        w.extend_from_slice(&(rate * block).to_le_bytes());
        w.extend_from_slice(&(block as u16).to_le_bytes());
        w.extend_from_slice(&16u16.to_le_bytes());
        w.extend_from_slice(b"data");
        w.extend_from_slice(&(data.len() as u32).to_le_bytes());
        w.extend_from_slice(&data);
        std::fs::write(path, w).unwrap();
    }

    pub fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("taw-player-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn reads_wav_facts_and_every_frame() {
        let d = temp_dir("decode");
        let p = d.join("a.wav");
        sine_wav(&p, 44_100, 2, 1000.0, 0, 30_000);
        let mut s = Source::open(&p).unwrap();
        assert_eq!(s.info.rate, 44_100);
        assert_eq!(s.info.channels, 2);
        assert_eq!(s.info.bits, Some(16));
        assert_eq!(s.info.codec, "WAV");
        assert_eq!(s.info.frames, Some(30_000));
        let mut n = 0;
        while let Some((b, _)) = s.next_block() {
            n += b.len();
        }
        assert_eq!(n, 60_000);
        let at = s.seek(0.5).unwrap();
        assert!((at - 0.5).abs() < 0.01, "{at}");
        let _ = std::fs::remove_dir_all(&d);
    }
}
