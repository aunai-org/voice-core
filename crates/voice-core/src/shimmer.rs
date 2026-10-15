//! Local shimmer: how much consecutive voice periods differ in loudness.
//!
//! Uses the same period tracking as [`crate::jitter`]. The amplitude of a period is
//! the peak-to-peak height of the waveform over it. Local shimmer is the mean absolute
//! difference of consecutive amplitudes divided by the mean amplitude (Titze, 1994,
//! "Workshop on acoustic voice analysis"; the same definition as the "local shimmer"
//! of the usual voice-analysis tools).
//!
//! Pairs whose periods differ by more than a factor of 1.3, or whose amplitudes differ
//! by more than a factor of 1.6, are left out as tracking errors rather than voice
//! behaviour.

use crate::jitter::{period_chains, voiced_runs};
use crate::pitch::{AutocorrEstimator, PitchEstimator, PitchFrame};
use crate::Error;

const MAX_PERIOD_FACTOR: f64 = 1.3;
const MAX_AMPLITUDE_FACTOR: f64 = 1.6;

/// Shimmer of a recording.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shimmer {
    /// Local shimmer as a fraction (0.05 means 5%).
    pub local: f32,
    /// Number of consecutive period pairs it is based on.
    pub pairs: usize,
}

/// Local shimmer of `samples` (mono, `sample_rate` Hz), or `None` when fewer than
/// 20 period pairs were found.
pub fn local_shimmer(samples: &[f32], sample_rate: u32) -> Result<Option<Shimmer>, Error> {
    let track = AutocorrEstimator::default().estimate(samples, sample_rate)?;
    Ok(local_shimmer_with_track(samples, sample_rate, &track))
}

/// Like [`local_shimmer`], reusing a pitch track already computed for `samples`.
pub fn local_shimmer_with_track(samples: &[f32], sample_rate: u32, track: &[PitchFrame]) -> Option<Shimmer> {
    let sr = f64::from(sample_rate);
    let mut sum_diff = 0.0;
    let mut sum_amp = 0.0;
    let mut pairs = 0usize;
    for run in voiced_runs(track) {
        for chain in period_chains(samples, sr, &run) {
            for w in chain.windows(2) {
                let (a, b) = (w[0], w[1]);
                if a.period / b.period > MAX_PERIOD_FACTOR
                    || b.period / a.period > MAX_PERIOD_FACTOR
                    || a.amplitude <= 0.0
                    || b.amplitude <= 0.0
                    || a.amplitude / b.amplitude > MAX_AMPLITUDE_FACTOR
                    || b.amplitude / a.amplitude > MAX_AMPLITUDE_FACTOR
                {
                    continue;
                }
                sum_diff += (a.amplitude - b.amplitude).abs();
                sum_amp += 0.5 * (a.amplitude + b.amplitude);
                pairs += 1;
            }
        }
    }
    (pairs >= 20).then(|| Shimmer {
        local: (sum_diff / sum_amp) as f32,
        pairs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 120 Hz voice-like signal whose pulses alternate in height by `amp` (0.05 means
    /// 5% above and below the mean).
    fn pulse_train(amp: f64, sr: u32) -> Vec<f32> {
        let period = 1.0 / 120.0;
        let n = (1.5 * f64::from(sr)) as usize;
        let mut x = vec![0.0f32; n];
        for k in 0..170 {
            let t0 = 0.02 + f64::from(k) * period;
            let h = 1.0 + if k % 2 == 0 { amp } else { -amp };
            let first = (t0 * f64::from(sr)) as usize;
            for (i, s) in x.iter_mut().enumerate().take(first + 200).skip(first) {
                let t = i as f64 / f64::from(sr) - t0;
                let v = h * (-t * 600.0).exp() * (2.0 * std::f64::consts::PI * 700.0 * t).sin();
                *s += v as f32;
            }
        }
        x
    }

    #[test]
    fn steady_voice_has_almost_no_shimmer() {
        let s = local_shimmer(&pulse_train(0.0, 16_000), 16_000).unwrap().unwrap();
        assert!(s.local < 0.02, "{}", s.local);
    }

    #[test]
    fn known_shimmer_is_recovered() {
        // alternating heights 1 +- 0.05: consecutive difference 0.1 over mean 1.0
        let s = local_shimmer(&pulse_train(0.05, 16_000), 16_000)
            .unwrap()
            .unwrap();
        assert!((f64::from(s.local) - 0.10).abs() < 0.04, "{}", s.local);
    }

    #[test]
    fn silence_has_no_shimmer() {
        assert_eq!(local_shimmer(&vec![0.0; 16_000], 16_000).unwrap(), None);
    }
}
