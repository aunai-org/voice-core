//! Speaking pace: syllable nuclei and syllables per second.
//!
//! Follows the idea of de Jong, N. H. & Wempe, T. (2009), "Praat script to detect
//! syllable nuclei and measure speech rate automatically", Behavior Research
//! Methods 41(2), 385-390, implemented here from the paper's description. A syllable nucleus is a peak of the
//! intensity contour that
//!
//! 1. lies within 25 dB of the 99th percentile of the contour,
//! 2. is separated from the previous nucleus by a dip of at least 2 dB on both
//!    sides (two peaks without such a dip merge into the higher one), and
//! 3. falls on a voiced frame of the pitch track.
//!
//! Limits: validated on synthetic syllable-like bursts only; real speech (fast
//! speech, vowel-less syllables, noisy recordings) has not been tested.

use crate::loudness::frame_rms_db;
use crate::pitch::{AutocorrEstimator, PitchEstimator};
use crate::Error;

/// Settings for syllable-nucleus detection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaceConfig {
    /// Length of the intensity window in seconds.
    pub frame_s: f64,
    /// Time between intensity frames in seconds.
    pub hop_s: f64,
    /// A nucleus must be within this many dB of the 99th percentile level.
    pub range_db: f32,
    /// Minimum dip between neighbouring nuclei, in dB.
    pub min_dip_db: f32,
}

impl Default for PaceConfig {
    /// 32 ms window, 10 ms hop, 25 dB range, 2 dB dip.
    fn default() -> Self {
        Self {
            frame_s: 0.032,
            hop_s: 0.01,
            range_db: 25.0,
            min_dip_db: 2.0,
        }
    }
}

fn percentile(levels: &[f32], p: f64) -> f32 {
    let mut sorted = levels.to_vec();
    sorted.sort_by(f32::total_cmp);
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

/// Times (seconds from the start) of the detected syllable nuclei.
pub fn syllable_nuclei(samples: &[f32], sample_rate: u32, config: &PaceConfig) -> Result<Vec<f64>, Error> {
    if samples.is_empty() {
        return Err(Error::EmptyInput);
    }
    if sample_rate == 0 {
        return Err(Error::InvalidConfig("sample_rate must be positive"));
    }
    if config.frame_s.is_nan() || config.frame_s <= 0.0 || config.hop_s.is_nan() || config.hop_s <= 0.0 {
        return Err(Error::InvalidConfig("frame_s and hop_s must be positive"));
    }
    let sr = f64::from(sample_rate);
    let frame_len = ((config.frame_s * sr).round() as usize).max(1);
    let hop = ((config.hop_s * sr).round() as usize).max(1);
    let levels = frame_rms_db(samples, frame_len, hop)?;
    if levels.len() < 3 {
        return Ok(Vec::new());
    }
    let pitch = AutocorrEstimator::default().estimate(samples, sample_rate)?;
    let time_of = |i: usize| (i * hop + frame_len / 2) as f64 / sr;
    let voiced = |t: f64| -> bool {
        let Some(first) = pitch.first() else {
            return false;
        };
        let step = AutocorrEstimator::default().hop_s;
        let idx = (((t - first.time_s) / step).round().max(0.0) as usize).min(pitch.len() - 1);
        pitch[idx].f0_hz.is_some()
    };

    // A contour that never moves by the minimum dip (steady tone) has no syllables.
    let (lo, hi) = levels
        .iter()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &v| {
            (lo.min(v), hi.max(v))
        });
    if hi - lo < config.min_dip_db {
        return Ok(Vec::new());
    }
    let threshold = percentile(&levels, 0.99) - config.range_db;
    let mut nuclei: Vec<usize> = Vec::new();
    for i in 1..levels.len() - 1 {
        let is_peak = levels[i] > levels[i - 1] && levels[i] >= levels[i + 1];
        if !is_peak || levels[i] < threshold || !voiced(time_of(i)) {
            continue;
        }
        match nuclei.last().copied() {
            None => nuclei.push(i),
            Some(prev) => {
                let dip = levels[prev..=i].iter().copied().fold(f32::INFINITY, f32::min);
                if levels[prev] - dip >= config.min_dip_db && levels[i] - dip >= config.min_dip_db {
                    nuclei.push(i);
                } else if levels[i] > levels[prev] {
                    *nuclei.last_mut().expect("non-empty") = i;
                }
            }
        }
    }
    Ok(nuclei.into_iter().map(time_of).collect())
}

/// Syllables per second over the whole recording (pauses included), using the
/// default [`PaceConfig`].
pub fn syllables_per_second(samples: &[f32], sample_rate: u32) -> Result<f64, Error> {
    let nuclei = syllable_nuclei(samples, sample_rate, &PaceConfig::default())?;
    Ok(nuclei.len() as f64 / (samples.len() as f64 / f64::from(sample_rate)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    const SR: u32 = 16_000;

    /// 150 Hz tone with raised-cosine bursts at `rate` Hz inside each `(start, end)` segment.
    fn bursts(rate: f64, segments: &[(f64, f64)], secs: f64) -> Vec<f32> {
        (0..(f64::from(SR) * secs) as usize)
            .map(|i| {
                let t = i as f64 / f64::from(SR);
                let env = segments
                    .iter()
                    .find(|(a, b)| t >= *a && t < *b)
                    .map_or(0.0, |(a, _)| 0.5 * (1.0 - (TAU * rate * (t - a)).cos()));
                (0.5 * env * (TAU * 150.0 * t).sin()) as f32
            })
            .collect()
    }

    #[test]
    fn counts_bursts_at_several_rates() {
        for rate in [3.0, 4.0, 5.0, 6.0] {
            let x = bursts(rate, &[(0.0, 3.0)], 3.0);
            let n = syllable_nuclei(&x, SR, &PaceConfig::default()).unwrap();
            assert_eq!(n.len() as f64, rate * 3.0, "rate {rate}: {n:?}");
        }
    }

    #[test]
    fn pause_adds_no_syllables_and_lowers_the_rate() {
        let x = bursts(4.0, &[(0.0, 1.0), (2.5, 3.5)], 3.5);
        assert_eq!(syllable_nuclei(&x, SR, &PaceConfig::default()).unwrap().len(), 8);
        let sps = syllables_per_second(&x, SR).unwrap();
        assert!((sps - 8.0 / 3.5).abs() < 1e-9, "{sps}");
    }

    #[test]
    fn silence_steady_tone_and_noise_have_no_syllables() {
        let silence = vec![0.0f32; 32_000];
        assert!(syllable_nuclei(&silence, SR, &PaceConfig::default())
            .unwrap()
            .is_empty());
        let steady: Vec<f32> = (0..32_000)
            .map(|i| 0.3 * (TAU * 150.0 * f64::from(i) / f64::from(SR)).sin() as f32)
            .collect();
        assert!(syllable_nuclei(&steady, SR, &PaceConfig::default())
            .unwrap()
            .is_empty());
        let mut s = 12345u32;
        let noise: Vec<f32> = (0..32_000)
            .map(|_| {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (s >> 8) as f32 / (1u32 << 24) as f32 - 0.5
            })
            .collect();
        assert!(syllable_nuclei(&noise, SR, &PaceConfig::default())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(
            syllable_nuclei(&[], SR, &PaceConfig::default()),
            Err(Error::EmptyInput)
        );
        assert!(syllable_nuclei(&[0.1; 100], 0, &PaceConfig::default()).is_err());
        let bad = PaceConfig {
            hop_s: 0.0,
            ..PaceConfig::default()
        };
        assert!(syllable_nuclei(&[0.1; 100], SR, &bad).is_err());
        assert!(syllable_nuclei(&[0.1; 100], SR, &PaceConfig::default())
            .unwrap()
            .is_empty());
    }
}
