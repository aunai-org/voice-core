//! Streaming analysis for live feedback: level, speech/silence and pitch per 10 ms frame.
//!
//! Audio is pushed in chunks of any size; a [`FrameInfo`] is emitted for every frame
//! whose analysis windows are complete, so a frame arrives about 20 ms after its centre
//! (the pitch window is 40 ms). Levels are the same Gaussian-windowed levels as the batch [`crate::pauses`] module
//! and match it exactly. A frame is speech when its level is within 25 dB of the loudest
//! frame heard so far and above [`SPEECH_FLOOR_DBFS`].
//!
//! Pitch uses the same per-frame candidates as the batch [`crate::pitch`] tracker but
//! picks one per frame using only the past (the batch octave-jump and voiced/unvoiced
//! costs relative to the previous frame's choice, with no lookahead and no path search),
//! and judges frame loudness against the loudest
//! sample heard up to the end of the frame's window. Expect more octave errors and
//! flicker than the batch track.
//!
//! Limits: the decision is made frame by frame with no minimum run lengths, so it
//! flickers at word edges, short gaps under 0.1 s show as silence and short sounds
//! count as speech, and a loud sound early in the stream raises the bar for everything
//! after it; [`Analyzer::with_reference_level`] fixes the last problem when a typical
//! level is known. The batch `pauses` module sees the whole recording and does not have
//! these limits; use it for the final numbers.

use crate::pauses::LevelMeter;
use crate::pitch::{AutocorrEstimator, Candidate, FrameSearch};
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
    /// Estimated fundamental frequency in Hz, or `None` when the frame is unvoiced.
    pub f0_hz: Option<f32>,
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
    search: FrameSearch,
    pitch_cfg: AutocorrEstimator,
    /// f0 chosen for the previous frame, for the transition costs.
    last_f0: Option<f64>,
    /// Running maximum of the absolute sample value, one entry per `buf` entry.
    peaks: Vec<f32>,
    /// Running maximum before `buf[0]`.
    peak_before: f32,
}

impl Analyzer {
    /// Creates an analyzer for audio at `sample_rate` Hz.
    pub fn new(sample_rate: u32) -> Result<Self, Error> {
        if sample_rate == 0 {
            return Err(Error::InvalidConfig("sample_rate must be positive"));
        }
        let sr = f64::from(sample_rate);
        let pitch_cfg = AutocorrEstimator::default();
        let search = FrameSearch::new(&pitch_cfg, sample_rate)?;
        Ok(Self {
            sample_rate,
            hop: ((HOP_S * sr).round() as usize).max(1),
            meter: LevelMeter::new(((WINDOW_S * sr).round() as usize).max(2)),
            buf: Vec::new(),
            buf_start: 0,
            total: 0,
            next_frame: 0,
            max_level: f32::NEG_INFINITY,
            search,
            pitch_cfg,
            last_f0: None,
            peaks: Vec::new(),
            peak_before: 0.0,
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
            let window_end = (centre + self.meter.len() - self.meter.half())
                .max(centre + self.search.window_len() - self.search.half());
            if flush {
                if centre >= self.total {
                    break;
                }
            } else if window_end > self.total {
                break;
            }
            let level = self.meter.level(&self.buf, self.buf_start, centre);
            self.max_level = self.max_level.max(level);
            // Loudest sample up to the end of this frame's window (or of the stream).
            let last = window_end.min(self.total).saturating_sub(1);
            let peak = last
                .checked_sub(self.buf_start)
                .and_then(|i| self.peaks.get(i).copied())
                .unwrap_or(self.peak_before);
            let (buf, start) = (&self.buf, self.buf_start);
            let cands = self.search.candidates(centre, peak, |j| {
                if j < window_end {
                    j.checked_sub(start).and_then(|i| buf.get(i).copied())
                } else {
                    None
                }
            });
            // One step of the batch path search with no lookahead: each candidate pays the
            // transition cost from the previous frame's choice.
            let (jump, vu) = (
                f64::from(self.pitch_cfg.octave_jump_cost),
                f64::from(self.pitch_cfg.voiced_unvoiced_cost),
            );
            let score = |c: &Candidate| match (c.f0_hz, self.last_f0) {
                (Some(f), Some(prev)) => c.strength - jump * (f / prev).log2().abs(),
                (None, None) => c.strength,
                _ => c.strength - vu,
            };
            let best = cands
                .iter()
                .fold(&cands[0], |b, c| if score(c) > score(b) { c } else { b });
            self.last_f0 = best.f0_hz;
            out.push(FrameInfo {
                time_s: centre as f64 / f64::from(self.sample_rate),
                level_dbfs: level,
                speech: level > SPEECH_FLOOR_DBFS && level >= self.max_level - THRESHOLD_DB,
                f0_hz: best.f0_hz.map(|f| f as f32),
            });
            self.next_frame += 1;
        }
        // Drop samples the next window no longer reaches.
        let keep_from =
            (self.next_frame * self.hop).saturating_sub(self.meter.half().max(self.search.half()));
        let drop = keep_from.saturating_sub(self.buf_start).min(self.buf.len());
        if drop > 0 {
            self.peak_before = self.peaks[drop - 1];
        }
        self.buf.drain(..drop);
        self.peaks.drain(..drop);
        self.buf_start += drop;
    }

    /// Adds `samples` and returns the frames that are now complete (possibly none).
    pub fn push(&mut self, samples: &[f32]) -> Vec<FrameInfo> {
        self.buf.extend_from_slice(samples);
        let mut running = self.peaks.last().copied().unwrap_or(self.peak_before);
        for x in samples {
            running = running.max(x.abs());
            self.peaks.push(running);
        }
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
        // The first frame is centred on sample 0 and needs 20 ms (321 samples) of audio
        // for the 40 ms pitch window.
        assert!(a.push(&vec![0.1; 320]).is_empty());
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
    fn pitch_follows_the_tone() {
        let frames = run(160, &signal());
        let at = |t: f64| frames.iter().find(|f| f.time_s >= t).unwrap().f0_hz;
        assert!(at(0.1).is_none());
        let f = at(0.8).expect("voiced in the loud tone");
        assert!((f - 180.0).abs() < 2.0, "{f}");
        assert!(at(1.5).is_none());
        let f = at(2.0).expect("voiced in the quiet tone");
        assert!((f - 180.0).abs() < 2.0, "{f}");
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
