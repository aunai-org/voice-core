//! Speech/silence segmentation and pause statistics.
//!
//! Levels are the squared signal averaged under a bell-shaped (Gaussian) window of
//! `window_s` seconds, so a single click or plosive burst does not read as speech.
//! A frame is *sounding* when its level is within `threshold_db` of the loudest
//! frame (default 25 dB), the same relative-threshold
//! idea as Praat's silence detection, which serves as the offline reference in
//! `tests/speech_pauses.rs`. Sounding runs shorter than `min_sounding_s` are dropped
//! first (clicks and breaths), then silent runs shorter than `min_silent_s` are
//! filled in (a stop closure is not a pause).
//!
//! Limits: the threshold is relative to the recording's own loudest passages, so a
//! recording that is pure noise still shows "speech"; a noise-floor check is not
//! part of this module. Checked on eight clean read-speech clips only.

use crate::Error;

/// Settings for speech/silence segmentation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PauseConfig {
    /// Length of the Gaussian level window in seconds.
    pub window_s: f64,
    /// Time between level frames in seconds.
    pub hop_s: f64,
    /// Frames more than this many dB below the loudest frame are silent.
    pub threshold_db: f32,
    /// Silent runs shorter than this are not pauses, in seconds.
    pub min_silent_s: f64,
    /// Sounding runs shorter than this are not speech, in seconds.
    pub min_sounding_s: f64,
}

impl Default for PauseConfig {
    /// 32 ms window, 10 ms hop, 25 dB threshold, 0.1 s minimum silent and sounding runs.
    fn default() -> Self {
        Self {
            window_s: 0.032,
            hop_s: 0.01,
            threshold_db: 25.0,
            min_silent_s: 0.1,
            min_sounding_s: 0.1,
        }
    }
}

/// A stretch of speech, in seconds from the start of the recording.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    pub start_s: f64,
    pub end_s: f64,
}

/// Summary of the pauses between speech segments.
///
/// Leading and trailing silence is not a pause: only gaps between two speech
/// segments count.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PauseStats {
    /// Number of pauses.
    pub count: usize,
    /// Total pause time in seconds.
    pub total_s: f64,
    /// Longest pause in seconds, 0 when there is none.
    pub longest_s: f64,
    /// Speech time (all segments) divided by the time from the first segment's start
    /// to the last segment's end, 0 to 1; 0 when there is no speech.
    pub speech_ratio: f64,
}

/// Level meter: the mean-removed squared signal averaged under a Gaussian window, in dB
/// relative to full scale. Samples beyond either end of the signal count as silence.
pub(crate) struct LevelMeter {
    weights: Vec<f64>,
    total: f64,
    half: usize,
}

impl LevelMeter {
    pub(crate) fn new(len: usize) -> Self {
        // w(x) = exp(-12 (x - 1/2)^2) for x in 0..1, so the edges sit near 5% of the peak.
        let weights: Vec<f64> = (0..len)
            .map(|i| (-12.0 * ((i as f64 + 0.5) / len as f64 - 0.5).powi(2)).exp())
            .collect();
        Self {
            total: weights.iter().sum(),
            half: len / 2,
            weights,
        }
    }

    /// Samples before the centre that the window reaches.
    pub(crate) fn half(&self) -> usize {
        self.half
    }

    /// Window length in samples.
    pub(crate) fn len(&self) -> usize {
        self.weights.len()
    }

    /// Level of the window centred on sample `centre` of a signal whose first
    /// sample held in `buf` is sample number `buf_start`.
    pub(crate) fn level(&self, buf: &[f32], buf_start: usize, centre: usize) -> f32 {
        // Weighted sums over the window: samples, and squares. The mean is removed so a
        // DC offset or slow drift in the recording does not read as sound.
        let (mut s1, mut s2) = (0.0_f64, 0.0_f64);
        for (k, w) in self.weights.iter().enumerate() {
            let Some(idx) = (centre + k)
                .checked_sub(self.half)
                .and_then(|i| i.checked_sub(buf_start))
            else {
                continue;
            };
            if let Some(&x) = buf.get(idx) {
                let x = f64::from(x);
                s1 += w * x;
                s2 += w * x * x;
            }
        }
        let mean = s1 / self.total;
        let power = (s2 / self.total - mean * mean).max(0.0);
        (10.0 * power.max(1e-12).log10()) as f32
    }
}

/// One level per `hop` samples; the first frame is centred on sample 0.
pub(crate) fn smoothed_levels(samples: &[f32], len: usize, hop: usize) -> Vec<f32> {
    let meter = LevelMeter::new(len);
    (0..samples.len().div_ceil(hop))
        .map(|f| meter.level(samples, 0, f * hop))
        .collect()
}

/// Flips every run of `value` shorter than `min_len` frames to the opposite value.
fn absorb_short_runs(flags: &mut [bool], value: bool, min_len: usize) {
    let mut i = 0;
    while i < flags.len() {
        if flags[i] != value {
            i += 1;
            continue;
        }
        let start = i;
        while i < flags.len() && flags[i] == value {
            i += 1;
        }
        if i - start < min_len {
            flags[start..i].iter_mut().for_each(|f| *f = !value);
        }
    }
}

/// Speech segments of a recording, in time order.
pub fn speech_segments(
    samples: &[f32],
    sample_rate: u32,
    config: &PauseConfig,
) -> Result<Vec<Segment>, Error> {
    if samples.is_empty() {
        return Err(Error::EmptyInput);
    }
    if sample_rate == 0 {
        return Err(Error::InvalidConfig("sample_rate must be positive"));
    }
    if config.window_s.is_nan() || config.window_s <= 0.0 || config.hop_s.is_nan() || config.hop_s <= 0.0 {
        return Err(Error::InvalidConfig("window_s and hop_s must be positive"));
    }
    let sr = f64::from(sample_rate);
    let frame_len = ((config.window_s * sr).round() as usize).max(2);
    let hop = ((config.hop_s * sr).round() as usize).max(1);
    let levels = smoothed_levels(samples, frame_len, hop);
    let max_level = levels.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let threshold = max_level - config.threshold_db;
    let mut sounding: Vec<bool> = levels.iter().map(|&l| l >= threshold).collect();

    let frames_for = |s: f64| (s / config.hop_s).ceil().max(1.0) as usize;
    absorb_short_runs(&mut sounding, true, frames_for(config.min_sounding_s));
    absorb_short_runs(&mut sounding, false, frames_for(config.min_silent_s));

    // Each frame owns the hop-wide cell around its centre.
    let duration = samples.len() as f64 / sr;
    let centre = |i: usize| (i * hop) as f64 / sr;
    let half_hop = hop as f64 / sr / 2.0;
    let mut segments = Vec::new();
    let mut i = 0;
    while i < sounding.len() {
        if !sounding[i] {
            i += 1;
            continue;
        }
        let first = i;
        while i < sounding.len() && sounding[i] {
            i += 1;
        }
        segments.push(Segment {
            start_s: if first == 0 {
                0.0
            } else {
                (centre(first) - half_hop).max(0.0)
            },
            end_s: if i == sounding.len() {
                duration
            } else {
                (centre(i - 1) + half_hop).min(duration)
            },
        });
    }
    Ok(segments)
}

/// Pause statistics for a list of speech segments.
pub fn pause_stats(segments: &[Segment]) -> PauseStats {
    let (Some(first), Some(last)) = (segments.first(), segments.last()) else {
        return PauseStats {
            count: 0,
            total_s: 0.0,
            longest_s: 0.0,
            speech_ratio: 0.0,
        };
    };
    let gaps: Vec<f64> = segments.windows(2).map(|w| w[1].start_s - w[0].end_s).collect();
    let speech: f64 = segments.iter().map(|s| s.end_s - s.start_s).sum();
    let span = last.end_s - first.start_s;
    PauseStats {
        count: gaps.len(),
        total_s: gaps.iter().sum(),
        longest_s: gaps.iter().copied().fold(0.0, f64::max),
        speech_ratio: if span > 0.0 { speech / span } else { 0.0 },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 16_000;

    fn tone(secs: f32, amp: f32) -> Vec<f32> {
        (0..(SR as f32 * secs) as usize)
            .map(|i| amp * (2.0 * std::f32::consts::PI * 200.0 * i as f32 / SR as f32).sin())
            .collect()
    }

    fn silence(secs: f32) -> Vec<f32> {
        vec![0.0; (SR as f32 * secs) as usize]
    }

    #[test]
    fn finds_two_bursts_with_known_gap() {
        let mut x = silence(0.5);
        x.extend(tone(1.0, 0.5));
        x.extend(silence(0.4));
        x.extend(tone(1.0, 0.5));
        x.extend(silence(0.5));
        let segs = speech_segments(&x, SR, &PauseConfig::default()).unwrap();
        assert_eq!(segs.len(), 2, "{segs:?}");
        assert!((segs[0].start_s - 0.5).abs() < 0.03 && (segs[0].end_s - 1.5).abs() < 0.03);
        assert!((segs[1].start_s - 1.9).abs() < 0.03 && (segs[1].end_s - 2.9).abs() < 0.03);
        let st = pause_stats(&segs);
        assert_eq!(st.count, 1);
        assert!((st.total_s - 0.4).abs() < 0.05, "{st:?}");
        assert!((st.longest_s - 0.4).abs() < 0.05);
        assert!((st.speech_ratio - 2.0 / 2.4).abs() < 0.03);
    }

    #[test]
    fn short_gap_is_not_a_pause() {
        let mut x = tone(0.5, 0.5);
        x.extend(silence(0.06));
        x.extend(tone(0.5, 0.5));
        assert_eq!(speech_segments(&x, SR, &PauseConfig::default()).unwrap().len(), 1);
    }

    #[test]
    fn short_click_is_not_speech() {
        let mut x = silence(0.5);
        x.extend(tone(0.03, 0.5));
        x.extend(silence(0.5));
        x.extend(tone(1.0, 0.5));
        let segs = speech_segments(&x, SR, &PauseConfig::default()).unwrap();
        assert_eq!(segs.len(), 1, "{segs:?}");
    }

    #[test]
    fn all_silence_has_no_pauses_counted() {
        // Digital silence: every frame sits at the same level, so the relative
        // threshold marks it all sounding; there is still no gap to report.
        let segs = speech_segments(&silence(1.0), SR, &PauseConfig::default()).unwrap();
        assert_eq!(pause_stats(&segs).count, 0);
    }

    #[test]
    fn no_segments_gives_zero_stats() {
        let st = pause_stats(&[]);
        assert_eq!((st.count, st.total_s, st.speech_ratio), (0, 0.0, 0.0));
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(
            speech_segments(&[], SR, &PauseConfig::default()),
            Err(Error::EmptyInput)
        );
        assert!(speech_segments(&[0.1], 0, &PauseConfig::default()).is_err());
        let bad = PauseConfig {
            hop_s: 0.0,
            ..PauseConfig::default()
        };
        assert!(speech_segments(&[0.1; 800], SR, &bad).is_err());
    }
}
