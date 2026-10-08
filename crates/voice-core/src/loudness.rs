//! Level measurements. Levels are relative dBFS (full scale = 0 dB); phone
//! microphones are uncalibrated, so these are not sound pressure levels.

use crate::{dsp, Error};

/// Floor returned for digital silence instead of negative infinity.
pub const SILENCE_DB: f32 = -120.0;

/// Root-mean-square level of a block in dBFS, clamped at [`SILENCE_DB`].
pub fn rms_db(block: &[f32]) -> f32 {
    if block.is_empty() {
        return SILENCE_DB;
    }
    let mean_sq = block.iter().map(|&s| f64::from(s) * f64::from(s)).sum::<f64>() / block.len() as f64;
    if mean_sq <= 0.0 {
        return SILENCE_DB;
    }
    (10.0 * mean_sq.log10() as f32).max(SILENCE_DB)
}

/// Per-frame RMS level in dBFS.
pub fn frame_rms_db(samples: &[f32], frame_len: usize, hop: usize) -> Result<Vec<f32>, Error> {
    if samples.is_empty() {
        return Err(Error::EmptyInput);
    }
    Ok(dsp::frames(samples, frame_len, hop)?
        .into_iter()
        .map(rms_db)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 16_000.0).sin())
            .collect()
    }

    #[test]
    fn full_scale_sine_is_minus_3db() {
        // RMS of a full-scale sine is 1/sqrt(2): about -3.01 dBFS.
        let db = rms_db(&sine(1.0, 16_000));
        assert!((db + 3.01).abs() < 0.05, "got {db}");
    }

    #[test]
    fn halving_amplitude_drops_6db() {
        let a = rms_db(&sine(0.8, 16_000));
        let b = rms_db(&sine(0.4, 16_000));
        assert!((a - b - 6.02).abs() < 0.05);
    }

    #[test]
    fn silence_hits_floor() {
        assert_eq!(rms_db(&[0.0; 160]), SILENCE_DB);
        assert_eq!(rms_db(&[]), SILENCE_DB);
    }

    #[test]
    fn per_frame_levels() {
        let mut x = vec![0.0f32; 1600];
        x.extend(sine(0.5, 1600));
        let levels = frame_rms_db(&x, 400, 400).unwrap();
        assert_eq!(levels.len(), 8);
        assert_eq!(levels[0], SILENCE_DB);
        assert!(levels[7] > -10.0);
    }

    #[test]
    fn empty_input_is_error() {
        assert_eq!(frame_rms_db(&[], 400, 160), Err(Error::EmptyInput));
    }
}
