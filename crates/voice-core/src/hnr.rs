//! Harmonics-to-noise ratio: how much of the voice is periodic and how much is noise.
//!
//! Method (Boersma, 1993, "Accurate short-term analysis of the fundamental frequency
//! and the harmonics-to-noise ratio of a sampled sound"): in each voiced frame the
//! height `r` of the normalised autocorrelation peak at the pitch period is the share
//! of the energy that repeats, so the ratio of periodic to noise energy is
//! `r / (1 - r)`, and the frame's HNR is `10 * log10(r / (1 - r))` dB. The recording's
//! HNR is the mean over the voiced frames. The peak heights are the ones the pitch
//! tracker already measures ([`crate::pitch::PitchFrame::strength`]).

use crate::pitch::{AutocorrEstimator, PitchEstimator, PitchFrame};
use crate::Error;

/// Peak heights are kept below this, so a perfect sine reads about 40 dB, not infinity.
const MAX_STRENGTH: f64 = 0.9999;
/// Fewest voiced frames (10 ms apart) a result needs.
const MIN_FRAMES: usize = 10;

/// Harmonics-to-noise ratio of a recording.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hnr {
    /// Mean HNR over the voiced frames, in dB.
    pub mean_db: f32,
    /// Number of voiced frames it is based on.
    pub frames: usize,
}

/// Mean HNR of `samples` (mono, `sample_rate` Hz), or `None` with fewer than 10
/// voiced frames.
pub fn mean_hnr(samples: &[f32], sample_rate: u32) -> Result<Option<Hnr>, Error> {
    let track = AutocorrEstimator::default().estimate(samples, sample_rate)?;
    Ok(mean_hnr_with_track(&track))
}

/// Like [`mean_hnr`], reusing a pitch track already computed for the samples.
pub fn mean_hnr_with_track(track: &[PitchFrame]) -> Option<Hnr> {
    let mut sum = 0.0;
    let mut frames = 0usize;
    for f in track.iter().filter(|f| f.f0_hz.is_some()) {
        let r = f64::from(f.strength).clamp(1e-6, MAX_STRENGTH);
        sum += 10.0 * (r / (1.0 - r)).log10();
        frames += 1;
    }
    (frames >= MIN_FRAMES).then(|| Hnr {
        mean_db: (sum / frames as f64) as f32,
        frames,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(freq: f32, secs: f32) -> Vec<f32> {
        (0..(16_000.0 * secs) as usize)
            .map(|i| 0.5 * (2.0 * std::f32::consts::PI * freq * i as f32 / 16_000.0).sin())
            .collect()
    }

    /// Deterministic white-ish noise (xorshift), peak about 1.
    fn noise(n: usize) -> Vec<f32> {
        let mut s = 0x2545_f491_u32;
        (0..n)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 17;
                s ^= s << 5;
                (s as f32 / u32::MAX as f32) * 2.0 - 1.0
            })
            .collect()
    }

    #[test]
    fn clean_tone_has_high_hnr() {
        let h = mean_hnr(&tone(150.0, 1.0), 16_000).unwrap().unwrap();
        assert!(h.mean_db > 25.0, "{}", h.mean_db);
    }

    #[test]
    fn noise_lowers_hnr() {
        let n = noise(16_000);
        let mixed = |amp: f32| -> Vec<f32> {
            tone(150.0, 1.0)
                .iter()
                .zip(&n)
                .map(|(t, w)| t + amp * w)
                .collect()
        };
        let clean = mean_hnr(&mixed(0.0), 16_000).unwrap().unwrap().mean_db;
        let light = mean_hnr(&mixed(0.2), 16_000).unwrap().unwrap().mean_db;
        let heavy = mean_hnr(&mixed(0.5), 16_000).unwrap().unwrap().mean_db;
        assert!(clean > light && light > heavy, "{clean} {light} {heavy}");
        // a 0.5-amplitude tone (power 0.125) over 0.2-amplitude uniform noise (power 0.0133) is
        // about 9.7 dB
        assert!((light - 9.7).abs() < 3.0, "{light}");
    }

    #[test]
    fn silence_has_no_hnr() {
        assert_eq!(mean_hnr(&vec![0.0; 16_000], 16_000).unwrap(), None);
    }
}
