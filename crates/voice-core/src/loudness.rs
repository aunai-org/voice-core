//! Level measurements. Levels are relative dBFS (full scale = 0 dB); phone
//! microphones are uncalibrated, so these are not sound pressure levels.

use crate::{dsp, Error};
use ebur128::{EbuR128, Mode};

/// Floor returned for digital silence instead of negative infinity.
pub const SILENCE_DB: f32 = -120.0;

/// Floor returned for integrated loudness of digital silence (LUFS) instead of negative infinity.
pub const LUFS_FLOOR: f64 = -120.0;

/// Integrated loudness of a mono signal in LUFS (ITU-R BS.1770 / EBU R128, with the
/// standard absolute and relative gating, so pauses do not pull the value down).
///
/// Levels are relative to digital full scale: a 997 Hz full-scale sine reads about
/// -3.01 LUFS. Returns [`LUFS_FLOOR`] when everything is gated out (silence).
pub fn integrated_lufs(samples: &[f32], sample_rate: u32) -> Result<f64, Error> {
    if samples.is_empty() {
        return Err(Error::EmptyInput);
    }
    if sample_rate == 0 {
        return Err(Error::InvalidConfig("sample_rate must be non-zero"));
    }
    let unsupported = |_| Error::InvalidConfig("sample rate not supported by the loudness meter");
    let mut meter = EbuR128::new(1, sample_rate, Mode::I).map_err(unsupported)?;
    meter.add_frames_f32(samples).map_err(unsupported)?;
    let lufs = meter.loudness_global().map_err(unsupported)?;
    Ok(if lufs.is_finite() {
        lufs.max(LUFS_FLOOR)
    } else {
        LUFS_FLOOR
    })
}

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

    fn tone(freq: f32, amp: f32, secs: f32) -> Vec<f32> {
        (0..(16_000.0 * secs) as usize)
            .map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / 16_000.0).sin())
            .collect()
    }

    #[test]
    fn full_scale_997hz_sine_is_about_minus_3_lufs() {
        // BS.1770: a 997 Hz full-scale mono sine reads -3.01 LUFS.
        let lufs = integrated_lufs(&tone(997.0, 1.0, 5.0), 16_000).unwrap();
        println!("997 Hz full scale: {lufs:.4} LUFS");
        assert!((lufs + 3.01).abs() < 0.1, "got {lufs}");
    }

    #[test]
    fn gating_ignores_pauses() {
        let mut x = tone(997.0, 0.4, 2.0);
        let alone = integrated_lufs(&x, 16_000).unwrap();
        x.extend(vec![0.0; 16_000 * 3]);
        x.extend(tone(997.0, 0.4, 2.0));
        let with_pause = integrated_lufs(&x, 16_000).unwrap();
        println!("alone {alone:.4}, with pause {with_pause:.4}");
        // Blocks straddling the pause edges still count, so allow a little drift.
        assert!((alone - with_pause).abs() < 0.5);
        // Plain RMS over the same signal is pulled down by the pause.
        assert!(f64::from(rms_db(&x)) < alone - 1.0);
    }

    #[test]
    fn silence_is_floor_and_bad_input_errors() {
        assert_eq!(integrated_lufs(&[0.0; 16_000], 16_000), Ok(LUFS_FLOOR));
        assert_eq!(integrated_lufs(&[], 16_000), Err(Error::EmptyInput));
        assert!(integrated_lufs(&[0.1], 0).is_err());
    }

    #[test]
    fn empty_input_is_error() {
        assert_eq!(frame_rms_db(&[], 400, 160), Err(Error::EmptyInput));
    }
}
