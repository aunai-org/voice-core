//! Streaming analysis for live feedback: level and speech/silence per 10 ms frame.
//!
//! Audio is pushed in chunks of any size; a [`FrameInfo`] is emitted for every frame
//! whose 32 ms window is complete, so a frame arrives about 16 ms after its centre.
//! Levels are the same Gaussian-windowed levels as the batch [`crate::pauses`] module
//! and match it exactly. A frame is speech when its level is within 25 dB of the loudest
//! frame heard so far and above [`SPEECH_FLOOR_DBFS`].
//!
//! Limits: the decision is made frame by frame with no minimum run lengths, so it
//! flickers at word edges, short gaps under 0.1 s show as silence and short sounds
//! count as speech, and a loud sound early in the stream raises the bar for everything
//! after it; [`Analyzer::with_reference_level`] fixes the last problem when a typical
//! level is known. The batch `pauses` module sees the whole recording and does not have
//! these limits; use it for the final numbers.

use crate::pauses::LevelMeter;
use crate::Error;

/// Frames at or below this level are never speech, so digital silence and a quiet
/// room do not count as speech before the first real sound.
pub const SPEECH_FLOOR_DBFS: f32 = -70.0;

const WINDOW_S: f64 = 0.032;
const HOP_S: f64 = 0.01;
const THRESHOLD_DB: f32 = 25.0;

/// What is known about one 10 ms frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameInfo {
    /// Centre of the frame in seconds from the start of the stream.
    pub time_s: f64,
    /// Level of the frame in dBFS (Gaussian-windowed mean square).
    pub level_dbfs: f32,
    /// Whether the frame counts as speech.
    pub speech: bool,
}

/// Push-style analyzer for one mono stream.
pub struct Analyzer {
    sample_rate: u32,
    hop: usize,
    meter: LevelMeter,
    buf: Vec<f32>,
    /// Stream index of `buf[0]`.
    buf_start: usize,
    /// Samples received so far.
    total: usize,
    next_frame: usize,
    max_level: f32,
}

impl Analyzer {
    /// Creates an analyzer for audio at `sample_rate` Hz.
    pub fn new(sample_rate: u32) -> Result<Self, Error> {
        if sample_rate == 0 {
            return Err(Error::InvalidConfig("sample_rate must be positive"));
        }
        let sr = f64::from(sample_rate);
        Ok(Self {
            sample_rate,
            hop: ((HOP_S * sr).round() as usize).max(1),
            meter: LevelMeter::new(((WINDOW_S * sr).round() as usize).max(2)),
            buf: Vec::new(),
            buf_start: 0,
            total: 0,
            next_frame: 0,
            max_level: f32::NEG_INFINITY,
        })
    }

    /// Starts with `level_dbfs` as the loudest level heard so far, for example the
    /// loudest frame of the previous recording, so the first words are judged against
    /// a realistic speech level instead of against themselves. A louder frame raises it.
    pub fn with_reference_level(mut self, level_dbfs: f32) -> Self {
        self.max_level = level_dbfs;
        self
    }

    fn emit(&mut self, out: &mut Vec<FrameInfo>, flush: bool) {
        loop {
            let centre = self.next_frame * self.hop;
            let window_end = centre + self.meter.len() - self.meter.half();
            if flush {
                if centre >= self.total {
                    break;
                }
            } else if window_end > self.total {
                break;
            }
            let level = self.meter.level(&self.buf, self.buf_start, centre);
            self.max_level = self.max_level.max(level);
            out.push(FrameInfo {
                time_s: centre as f64 / f64::from(self.sample_rate),
                level_dbfs: level,
                speech: level > SPEECH_FLOOR_DBFS && level >= self.max_level - THRESHOLD_DB,
            });
            self.next_frame += 1;
        }
        // Drop samples the next window no longer reaches.
        let keep_from = (self.next_frame * self.hop).saturating_sub(self.meter.half());
        let drop = keep_from.saturating_sub(self.buf_start).min(self.buf.len());
        self.buf.drain(..drop);
        self.buf_start += drop;
    }

    /// Adds `samples` and returns the frames that are now complete (possibly none).
    pub fn push(&mut self, samples: &[f32]) -> Vec<FrameInfo> {
        self.buf.extend_from_slice(samples);
        self.total += samples.len();
        let mut out = Vec::new();
        self.emit(&mut out, false);
        out
    }

    /// Ends the stream and returns the remaining frames, with silence assumed after
    /// the last sample, so the frame count equals the batch count.
    pub fn finish(mut self) -> Vec<FrameInfo> {
        let mut out = Vec::new();
        self.emit(&mut out, true);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pauses::smoothed_levels;

    const SR: u32 = 16_000;

    fn signal() -> Vec<f32> {
        // 0.3 s silence, 1 s tone, 0.4 s silence, 0.8 s quieter tone, 0.3 s silence.
        let tone = |secs: f32, amp: f32| {
            (0..(SR as f32 * secs) as usize)
                .map(move |i| amp * (2.0 * std::f32::consts::PI * 180.0 * i as f32 / SR as f32).sin())
        };
        let gap = |secs: f32| std::iter::repeat_n(0.0, (SR as f32 * secs) as usize);
        gap(0.3)
            .chain(tone(1.0, 0.5))
            .chain(gap(0.4))
            .chain(tone(0.8, 0.1))
            .chain(gap(0.3))
            .collect()
    }

    fn run(chunk: usize, x: &[f32]) -> Vec<FrameInfo> {
        let mut a = Analyzer::new(SR).unwrap();
        let mut out = Vec::new();
        for c in x.chunks(chunk) {
            out.extend(a.push(c));
        }
        out.extend(a.finish());
        out
    }

    #[test]
    fn chunk_size_does_not_change_the_result() {
        let x = signal();
        let whole = run(x.len(), &x);
        for chunk in [1, 7, 160, 1000, 4096] {
            assert_eq!(run(chunk, &x), whole, "chunk {chunk}");
        }
    }

    #[test]
    fn levels_match_the_batch_levels() {
        let x = signal();
        let frames = run(512, &x);
        let batch = smoothed_levels(&x, 512, 160);
        assert_eq!(frames.len(), batch.len());
        for (f, b) in frames.iter().zip(&batch) {
            assert_eq!(f.level_dbfs, *b);
        }
    }

    #[test]
    fn frames_arrive_with_a_short_delay() {
        let mut a = Analyzer::new(SR).unwrap();
        // The first frame is centred on sample 0 and needs 16 ms (256 samples) of audio.
        assert!(a.push(&vec![0.1; 255]).is_empty());
        assert_eq!(a.push(&[0.1]).len(), 1);
    }

    #[test]
    fn speech_flags_follow_the_tones() {
        let frames = run(160, &signal());
        let at = |t: f64| frames.iter().find(|f| f.time_s >= t).unwrap().speech;
        assert!(!at(0.1));
        assert!(at(0.8));
        assert!(!at(1.5));
        // The quieter tone is 14 dB down, inside the 25 dB range.
        assert!(at(2.0));
        assert!(!at(2.7));
    }

    #[test]
    fn reference_level_makes_a_quiet_start_silent() {
        // Quiet room noise (about -50 dBFS) first, then speech-level tone.
        let noise: Vec<f32> = (0..SR as usize / 2)
            .map(|i| if i % 2 == 0 { 0.003 } else { -0.003 })
            .collect();
        let mut x = noise.clone();
        x.extend(signal());
        let plain = run(160, &x);
        let mut a = Analyzer::new(SR).unwrap().with_reference_level(-9.0);
        let mut seeded = a.push(&x);
        seeded.extend(a.finish());
        // Without a reference the first frames are the loudest so far, hence speech.
        assert!(plain[20].speech);
        assert!(!seeded[20].speech);
    }

    #[test]
    fn silence_is_never_speech() {
        let frames = run(1000, &vec![0.0; SR as usize]);
        assert_eq!(frames.len(), 100);
        assert!(frames.iter().all(|f| !f.speech));
    }

    #[test]
    fn empty_stream_and_bad_rate() {
        assert!(Analyzer::new(0).is_err());
        assert!(Analyzer::new(SR).unwrap().finish().is_empty());
    }
}
