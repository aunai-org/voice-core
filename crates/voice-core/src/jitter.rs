//! Local jitter: how much consecutive voice periods differ.
//!
//! Method: inside each voiced stretch of the pitch track, one period of the waveform
//! is matched against the next by normalised cross-correlation (the lag of the peak,
//! refined with a parabola, is the period), then the window moves on by one period.
//! Local jitter is the mean absolute difference of consecutive periods divided by the
//! mean period (Titze, 1994, "Workshop on acoustic voice analysis"; the same
//! definition as the "local jitter" of the usual voice-analysis tools).
//!
//! Pairs whose periods differ by more than a factor of 1.3 are left out, as period
//! tracking errors rather than voice behaviour.

use crate::pitch::{AutocorrEstimator, PitchEstimator, PitchFrame};
use crate::Error;

/// Largest ratio between two consecutive periods that still counts as a pair.
const MAX_PERIOD_FACTOR: f64 = 1.3;
/// Matches weaker than this end a voiced stretch.
const MIN_MATCH: f64 = 0.5;

/// Jitter of a recording.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Jitter {
    /// Local jitter as a fraction (0.01 means 1%).
    pub local: f32,
    /// Number of consecutive period pairs it is based on.
    pub pairs: usize,
}

/// Local jitter of `samples` (mono, `sample_rate` Hz), or `None` when fewer than
/// 20 period pairs were found.
pub fn local_jitter(samples: &[f32], sample_rate: u32) -> Result<Option<Jitter>, Error> {
    let track = AutocorrEstimator::default().estimate(samples, sample_rate)?;
    Ok(local_jitter_with_track(samples, sample_rate, &track))
}

/// Like [`local_jitter`], reusing a pitch track already computed for `samples`.
pub fn local_jitter_with_track(samples: &[f32], sample_rate: u32, track: &[PitchFrame]) -> Option<Jitter> {
    let sr = f64::from(sample_rate);
    let mut sum_diff = 0.0;
    let mut sum_period = 0.0;
    let mut pairs = 0usize;
    for run in voiced_runs(track) {
        for chain in period_chains(samples, sr, &run) {
            for w in chain.windows(2) {
                let (a, b) = (w[0].period, w[1].period);
                if a / b > MAX_PERIOD_FACTOR || b / a > MAX_PERIOD_FACTOR {
                    continue;
                }
                sum_diff += (a - b).abs();
                sum_period += 0.5 * (a + b);
                pairs += 1;
            }
        }
    }
    (pairs >= 20).then(|| Jitter {
        local: (sum_diff / sum_period) as f32,
        pairs,
    })
}

/// One tracked voice period.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Period {
    /// Length in seconds.
    pub period: f64,
    /// Peak-to-peak amplitude of the waveform over the period.
    pub amplitude: f64,
}

/// Voiced stretches of the track as (time in s, f0 in Hz) lists.
pub(crate) fn voiced_runs(track: &[PitchFrame]) -> Vec<Vec<(f64, f64)>> {
    let mut runs = vec![];
    let mut cur: Vec<(f64, f64)> = vec![];
    for f in track {
        match f.f0_hz {
            Some(hz) => cur.push((f.time_s, f64::from(hz))),
            None => {
                if cur.len() >= 3 {
                    runs.push(std::mem::take(&mut cur));
                }
                cur.clear();
            }
        }
    }
    if cur.len() >= 3 {
        runs.push(cur);
    }
    runs
}

/// Chains of consecutive periods (in seconds) tracked along one voiced stretch; a
/// weak match ends a chain.
pub(crate) fn period_chains(samples: &[f32], sr: f64, run: &[(f64, f64)]) -> Vec<Vec<Period>> {
    let start = run[0].0 * sr;
    let end = run[run.len() - 1].0 * sr;
    let mut pos = start.round().max(0.0) as usize;
    let mut chains = vec![];
    let mut periods: Vec<Period> = vec![];
    while (pos as f64) < end {
        // f0 of the nearest frame sets the window length and the search range
        let t = pos as f64 / sr;
        let hz = run
            .iter()
            .min_by(|a, b| (a.0 - t).abs().total_cmp(&(b.0 - t).abs()))
            .map_or(100.0, |p| p.1);
        let nominal = sr / hz;
        let len = nominal.round() as usize;
        let lo = (0.75 * nominal).floor() as usize;
        let hi = (1.35 * nominal).ceil() as usize;
        if len < 4 || pos + hi + len + 1 >= samples.len() {
            break;
        }
        let Some((lag, delta, score)) = best_lag(samples, pos, len, lo, hi) else {
            break;
        };
        if score < MIN_MATCH {
            chains.push(std::mem::take(&mut periods));
            pos += lag.max(1);
            continue;
        }
        let (lo_v, hi_v) = samples[pos..pos + lag]
            .iter()
            .fold((f32::MAX, f32::MIN), |(l, h), &v| (l.min(v), h.max(v)));
        periods.push(Period {
            period: (lag as f64 + delta) / sr,
            amplitude: f64::from(hi_v - lo_v),
        });
        pos += lag;
    }
    chains.push(periods);
    chains
}

/// Lag in `lo..=hi` where the window of `len` samples at `pos` best matches the
/// window at `pos + lag`; returns (lag, parabola offset, score).
fn best_lag(x: &[f32], pos: usize, len: usize, lo: usize, hi: usize) -> Option<(usize, f64, f64)> {
    let a = &x[pos..pos + len];
    let ea: f64 = a.iter().map(|&v| f64::from(v) * f64::from(v)).sum();
    if ea < 1e-12 {
        return None;
    }
    let score_at = |lag: usize| -> f64 {
        let b = &x[pos + lag..pos + lag + len];
        let (mut ab, mut eb) = (0.0, 0.0);
        for (&p, &q) in a.iter().zip(b) {
            ab += f64::from(p) * f64::from(q);
            eb += f64::from(q) * f64::from(q);
        }
        ab / (ea * eb).sqrt().max(1e-12)
    };
    let scores: Vec<f64> = (lo..=hi).map(score_at).collect();
    let (i, &best) = scores.iter().enumerate().max_by(|p, q| p.1.total_cmp(q.1))?;
    let delta = if i > 0 && i + 1 < scores.len() {
        let (l, c, r) = (scores[i - 1], scores[i], scores[i + 1]);
        let den = l - 2.0 * c + r;
        if den.abs() > 1e-12 {
            (0.5 * (l - r) / den).clamp(-0.5, 0.5)
        } else {
            0.0
        }
    } else {
        0.0
    };
    Some((lo + i, delta, best))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Voice-like signal: a decaying 700 Hz burst at every glottal pulse, with pulse
    /// times spaced by `periods` (seconds). Returns the samples at `sr`.
    fn pulse_train(periods: &[f64], sr: u32) -> Vec<f32> {
        let total: f64 = periods.iter().sum::<f64>() + 0.05;
        let n = (total * f64::from(sr)) as usize;
        let mut x = vec![0.0f32; n];
        let mut t0 = 0.02;
        for &p in periods {
            let first = (t0 * f64::from(sr)) as usize;
            for (i, s) in x.iter_mut().enumerate().take(first + 200).skip(first) {
                let t = i as f64 / f64::from(sr) - t0;
                let v = (-t * 600.0).exp() * (2.0 * std::f64::consts::PI * 700.0 * t).sin();
                *s += v as f32;
            }
            t0 += p;
        }
        x
    }

    fn expected(periods: &[f64]) -> f64 {
        let d: f64 = periods.windows(2).map(|w| (w[0] - w[1]).abs()).sum();
        let m: f64 = periods.windows(2).map(|w| 0.5 * (w[0] + w[1])).sum();
        d / m
    }

    #[test]
    fn steady_voice_has_almost_no_jitter() {
        let periods = vec![1.0 / 120.0; 120];
        let j = local_jitter(&pulse_train(&periods, 16_000), 16_000)
            .unwrap()
            .unwrap();
        assert!(j.local < 0.002, "{}", j.local);
    }

    #[test]
    fn known_jitter_is_recovered() {
        // alternating periods: 1% and 2% jitter around 8 ms
        for (amp, name) in [(0.005, "1%"), (0.01, "2%")] {
            let periods: Vec<f64> = (0..150)
                .map(|i| 0.008 * (1.0 + if i % 2 == 0 { amp } else { -amp }))
                .collect();
            let want = expected(&periods);
            let j = local_jitter(&pulse_train(&periods, 16_000), 16_000)
                .unwrap()
                .unwrap();
            assert!(
                (f64::from(j.local) - want).abs() < 0.004,
                "{name}: got {} want {want}",
                j.local
            );
        }
    }

    #[test]
    fn silence_has_no_jitter() {
        assert_eq!(local_jitter(&vec![0.0; 16_000], 16_000).unwrap(), None);
    }
}
