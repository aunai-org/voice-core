//! Streaming analysis for live feedback: level, speech/silence, pitch and syllable nuclei
//! per 10 ms frame.
//!
//! Audio is pushed in chunks of any size; a [`FrameInfo`] is emitted for every frame
//! whose analysis windows are complete, so a frame arrives about 20 ms after its centre
//! (the pitch window is 40 ms), plus the pitch lag below. Levels are the same Gaussian-windowed levels as the batch [`crate::pauses`] module
//! and match it exactly. A frame is speech when its level is within 25 dB of the loudest
//! frame heard so far and above [`SPEECH_FLOOR_DBFS`].
//!
//! Pitch uses the same per-frame candidates and transition costs as the batch
//! [`crate::pitch`] tracker, but the path search looks ahead only [`PITCH_LAG_FRAMES`]
//! frames (100 ms): a frame is reported once that many later frames have been heard, by
//! tracing back from the best path at that point. Frame loudness is judged against the
//! loudest sample heard up to the end of the frame's window. A frame therefore arrives
//! about 120 ms after its centre, and the track can still differ from the batch one
//! where a decision needs more than 100 ms of context.
//!
//! The speech flag uses the same lookahead: it changes only once the new value has held
//! for 0.1 s, so gaps and sounds shorter than that are ignored, as in the batch module.
//!
//! Syllable nuclei follow the batch rules of [`crate::pace`] (a voiced peak of the 64 ms
//! level within 25 dB of the 99th percentile, 2 dB above the dip to the previous nucleus),
//! with the percentile taken over the frames heard so far. Because a reported nucleus
//! cannot be taken back, a peak that a higher one would replace more than 100 ms later is
//! still reported, so the live count runs higher than the batch count.
//!
//! Limits: a loud sound early in the stream raises the bar for everything
//! after it; [`Analyzer::with_reference_level`] fixes this when a typical
//! level is known. The batch `pauses` module sees the whole recording and does not have
//! these limits; use it for the final numbers.

use crate::pauses::LevelMeter;
use crate::pitch::{AutocorrEstimator, Candidate, FrameSearch};
use crate::Error;
use std::collections::VecDeque;

/// Frames at or below this level are never speech, so digital silence and a quiet
/// room do not count as speech before the first real sound.
pub const SPEECH_FLOOR_DBFS: f32 = -70.0;

const WINDOW_S: f64 = 0.032;
const HOP_S: f64 = 0.01;
const THRESHOLD_DB: f32 = 25.0;
/// Intensity window of the syllable detector, as in [`crate::pace`].
const PACE_WINDOW_S: f64 = 0.064;
/// Minimum dip between neighbouring nuclei, in dB (the batch default).
const MIN_DIP_DB: f32 = 2.0;

/// Frames of lookahead for the pitch path search (100 ms).
pub const PITCH_LAG_FRAMES: usize = 10;

/// The speech flag only changes after this many frames (0.1 s) of the new value, as in
/// the batch [`crate::pauses`] defaults. Needs the lookahead above.
const SPEECH_MIN_RUN_FRAMES: usize = 10;

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
    /// Whether this frame is a syllable nucleus (a voiced peak of the 64 ms level that is
    /// separated from the previous nucleus by a 2 dB dip; see [`crate::pace`]).
    pub syllable_nucleus: bool,
}

/// Width of the level histogram bins in dB, and the level of its first bin.
const HIST_STEP_DB: f32 = 0.1;
const HIST_FLOOR_DB: f32 = -150.0;

/// A frame waiting for its pitch decision.
struct Pending {
    info: FrameInfo,
    /// 64 ms Gaussian level for the syllable detector, `None` when the window is not fully
    /// inside the stream.
    pace_level: Option<f32>,
    cands: Vec<Candidate>,
    /// For each candidate, the best previous candidate on its path.
    back: Vec<usize>,
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
    /// Frames whose pitch is not decided yet, oldest first.
    pending: VecDeque<Pending>,
    /// Speech flag of the last released frame, after the minimum-run rule.
    speech: bool,
    /// Best path score ending in each candidate of the newest pending frame.
    path_score: Vec<f64>,
    pace_meter: LevelMeter,
    /// Histogram of the 64 ms levels seen so far, for the 99th percentile.
    hist: Vec<u32>,
    hist_n: u32,
    /// Lowest and highest 64 ms level seen so far.
    pace_range: (f32, f32),
    /// Level of the last released frame with a 64 ms level.
    prev_pace: Option<f32>,
    /// Level of the last nucleus and the lowest level since (and including) it.
    nucleus: Option<(f32, f32)>,
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
            pending: VecDeque::new(),
            speech: false,
            path_score: Vec::new(),
            pace_meter: LevelMeter::new(((PACE_WINDOW_S * sr).round() as usize).max(2)),
            hist: vec![0; ((0.0 - HIST_FLOOR_DB) / HIST_STEP_DB) as usize + 1],
            hist_n: 0,
            pace_range: (f32::INFINITY, f32::NEG_INFINITY),
            prev_pace: None,
            nucleus: None,
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

    /// One step of the batch Viterbi search: extends every path by the new frame.
    fn push_pending(&mut self, info: FrameInfo, pace_level: Option<f32>, cands: Vec<Candidate>) {
        if let Some(l) = pace_level {
            let bin = ((l - HIST_FLOOR_DB) / HIST_STEP_DB).clamp(0.0, (self.hist.len() - 1) as f32);
            self.hist[bin as usize] += 1;
            self.hist_n += 1;
            self.pace_range = (self.pace_range.0.min(l), self.pace_range.1.max(l));
        }
        let (jump, vu) = (
            f64::from(self.pitch_cfg.octave_jump_cost),
            f64::from(self.pitch_cfg.voiced_unvoiced_cost),
        );
        let transition = |a: &Candidate, b: &Candidate| match (a.f0_hz, b.f0_hz) {
            (Some(x), Some(y)) => jump * (x / y).log2().abs(),
            (None, None) => 0.0,
            _ => vu,
        };
        let mut back = vec![0; cands.len()];
        let mut score = Vec::with_capacity(cands.len());
        for (k, cur) in cands.iter().enumerate() {
            let mut best = 0.0;
            if let Some(prev) = self.pending.back() {
                let (i, v) = prev
                    .cands
                    .iter()
                    .enumerate()
                    .map(|(i, p)| (i, self.path_score[i] - transition(p, cur)))
                    .max_by(|a, b| a.1.total_cmp(&b.1))
                    .expect("every frame has an unvoiced candidate");
                back[k] = i;
                best = v;
            }
            score.push(best + cur.strength);
        }
        self.path_score = score;
        self.pending.push_back(Pending {
            info,
            pace_level,
            cands,
            back,
        });
    }

    /// Candidate index chosen for each pending frame by the best path ending at the newest one.
    fn best_path(&self) -> Vec<usize> {
        let mut idx = self
            .path_score
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, _)| i)
            .unwrap_or(0);
        let mut path = vec![0; self.pending.len()];
        for t in (0..self.pending.len()).rev() {
            path[t] = idx;
            idx = self.pending[t].back[idx];
        }
        path
    }

    /// Speech flag of the oldest pending frame: it differs from the previous frame's flag
    /// only when the raw flag keeps the new value for [`SPEECH_MIN_RUN_FRAMES`] frames
    /// from here (or to the end of the stream, which is then too short and is ignored).
    fn smoothed_speech(&mut self) -> bool {
        let raw = self.pending[0].info.speech;
        if raw != self.speech {
            let run = self
                .pending
                .iter()
                .take(SPEECH_MIN_RUN_FRAMES)
                .take_while(|p| p.info.speech == raw)
                .count();
            if run >= SPEECH_MIN_RUN_FRAMES {
                self.speech = raw;
            }
        }
        self.speech
    }

    /// 99th percentile of the 64 ms levels seen so far, to the histogram's resolution.
    fn level_p99(&self) -> f32 {
        let target = ((self.hist_n - 1) as f64 * 0.99).round() as u32;
        let mut seen = 0;
        for (i, &c) in self.hist.iter().enumerate() {
            seen += c;
            if seen > target {
                return HIST_FLOOR_DB + (i as f32 + 0.5) * HIST_STEP_DB;
            }
        }
        0.0
    }

    /// Whether a later pending frame (within the lookahead) would take this peak's place
    /// in the batch rules: a higher voiced peak with no 2 dB dip between them.
    fn replaced_soon(&self, level: f32, path: &[usize]) -> bool {
        let mut low = level;
        for (j, &choice) in path.iter().enumerate().take(self.pending.len() - 1).skip(1) {
            let (Some(prev), Some(l), Some(next)) = (
                self.pending[j - 1].pace_level,
                self.pending[j].pace_level,
                self.pending[j + 1].pace_level,
            ) else {
                return false;
            };
            low = low.min(l);
            let voiced = self.pending[j].cands[choice].f0_hz.is_some();
            if l > prev && l >= next && voiced {
                if level - low >= MIN_DIP_DB && l - low >= MIN_DIP_DB {
                    return false;
                }
                if l > level {
                    return true;
                }
            }
        }
        false
    }

    /// Whether the oldest pending frame is a syllable nucleus, given whether it is voiced
    /// and the chosen candidates of the pending frames.
    /// The rules are the batch ones ([`crate::pace`]) applied causally: a peak that is not
    /// separated from the previous nucleus by a dip cannot take that nucleus back, and a
    /// peak is held back when a higher one follows within the lookahead without a dip.
    fn is_nucleus(&mut self, voiced: bool, path: &[usize]) -> bool {
        let Some(level) = self.pending[0].pace_level else {
            return false;
        };
        let prev = self.prev_pace.replace(level);
        let next = self.pending.get(1).and_then(|p| p.pace_level);
        if let Some((_, low)) = self.nucleus.as_mut() {
            *low = low.min(level);
        }
        let is_peak = matches!((prev, next), (Some(a), Some(b)) if level > a && level >= b);
        let moves = self.pace_range.1 - self.pace_range.0 >= MIN_DIP_DB;
        if !is_peak || !voiced || !moves || level < self.level_p99() - THRESHOLD_DB {
            return false;
        }
        if let Some((last, low)) = self.nucleus {
            if last - low < MIN_DIP_DB || level - low < MIN_DIP_DB {
                if level > last {
                    self.nucleus = Some((level, level));
                }
                return false;
            }
        }
        if self.replaced_soon(level, path) {
            return false;
        }
        self.nucleus = Some((level, level));
        true
    }

    /// Decides and removes the oldest pending frame.
    fn release(&mut self) -> FrameInfo {
        let path = self.best_path();
        let speech = self.smoothed_speech();
        let f0_hz = self.pending[0].cands[path[0]].f0_hz;
        let syllable_nucleus = self.is_nucleus(f0_hz.is_some(), &path);
        let p = self.pending.pop_front().expect("pending frame");
        FrameInfo {
            speech,
            f0_hz: f0_hz.map(|f| f as f32),
            syllable_nucleus,
            ..p.info
        }
    }

    fn emit(&mut self, out: &mut Vec<FrameInfo>, flush: bool) {
        loop {
            let centre = self.next_frame * self.hop;
            let pace_half = self.pace_meter.half();
            let window_end = (centre + self.meter.len() - self.meter.half())
                .max(centre + self.search.window_len() - self.search.half())
                .max(centre + self.pace_meter.len() - pace_half);
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
            // Like the batch detector, only frames whose whole window lies inside the stream.
            let pace_level = (centre >= pace_half && centre + pace_half <= self.total)
                .then(|| self.pace_meter.level(&self.buf, self.buf_start, centre));
            let info = FrameInfo {
                time_s: centre as f64 / f64::from(self.sample_rate),
                level_dbfs: level,
                speech: level > SPEECH_FLOOR_DBFS && level >= self.max_level - THRESHOLD_DB,
                f0_hz: None,
                syllable_nucleus: false,
            };
            self.push_pending(info, pace_level, cands);
            while self.pending.len() > PITCH_LAG_FRAMES {
                out.push(self.release());
            }
            self.next_frame += 1;
        }
        // Drop samples the next window no longer reaches.
        let keep_from = (self.next_frame * self.hop).saturating_sub(
            self.meter
                .half()
                .max(self.search.half())
                .max(self.pace_meter.half()),
        );
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
        let path = self.best_path();
        for (i, &choice) in path.iter().enumerate() {
            let speech = self.smoothed_speech();
            let f0_hz = self.pending[0].cands[choice].f0_hz;
            let syllable_nucleus = self.is_nucleus(f0_hz.is_some(), &path[i..]);
            let p = self.pending.pop_front().expect("pending frame");
            out.push(FrameInfo {
                speech,
                f0_hz: f0_hz.map(|f| f as f32),
                syllable_nucleus,
                ..p.info
            });
        }
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
        // Frame 0 is centred on sample 0 and needs 32 ms (512 samples) for the 64 ms syllable
        // window; frame 0 is reported once the 10 later frames are complete (100 ms more).
        let hop = 160;
        assert!(a.push(&vec![0.1; 511 + PITCH_LAG_FRAMES * hop]).is_empty());
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
    fn short_clicks_and_gaps_do_not_flip_the_flag() {
        let tone = |secs: f32| -> Vec<f32> {
            (0..(SR as f32 * secs) as usize)
                .map(|i| 0.5 * (2.0 * std::f32::consts::PI * 180.0 * i as f32 / SR as f32).sin())
                .collect()
        };
        let gap = |secs: f32| vec![0.0; (SR as f32 * secs) as usize];
        let mut x = gap(0.5);
        x.extend(tone(0.04)); // a click, well under 0.1 s
        x.extend(gap(0.5));
        x.extend(tone(0.8));
        x.extend(gap(0.05)); // a gap well under 0.1 s
        x.extend(tone(0.8));
        x.extend(gap(0.5));
        let frames = run(160, &x);
        let at = |t: f64| frames.iter().find(|f| f.time_s >= t).unwrap().speech;
        assert!(!at(0.52), "click counted as speech");
        assert!(at(1.5));
        assert!(at(1.865), "short gap counted as a pause");
        assert!(at(2.4));
        assert!(!at(3.0));
    }

    #[test]
    fn syllable_bursts_are_counted() {
        // Four voiced bursts per second for 3 s, as raised-cosine envelopes on a 150 Hz tone.
        let x: Vec<f32> = (0..SR as usize * 3)
            .map(|i| {
                let t = i as f32 / SR as f32;
                let env = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * 4.0 * t).cos());
                0.5 * env * (2.0 * std::f32::consts::PI * 150.0 * t).sin()
            })
            .collect();
        let n = run(160, &x).iter().filter(|f| f.syllable_nucleus).count();
        assert!((11..=13).contains(&n), "{n} nuclei for 12 bursts");
        let none = run(160, &vec![0.0; SR as usize]);
        assert!(none.iter().all(|f| !f.syllable_nucleus));
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
