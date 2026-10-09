//! Recording-quality warnings: cheap checks that say when the other numbers
//! should be read with care.
//!
//! The thresholds are practical defaults, not values from a standard, and
//! there is no Praat measurement to compare them against.

use serde::{Deserialize, Serialize};

use crate::loudness::rms_db;
use crate::Error;

/// Shorter than this many seconds, pitch and pace summaries are unreliable.
pub const MIN_DURATION_S: f64 = 1.0;
/// Absolute sample value from which a sample counts as clipped.
pub const CLIP_LEVEL: f32 = 0.999;
/// Share of clipped samples from which the recording is flagged.
pub const CLIP_FRACTION: f64 = 0.001;
/// Overall RMS below this level (dBFS) is flagged as too quiet.
pub const QUIET_RMS_DBFS: f32 = -50.0;

/// Something that makes a recording's measurements less trustworthy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Warning {
    /// The recording is shorter than [`MIN_DURATION_S`].
    TooShort,
    /// At least [`CLIP_FRACTION`] of the samples sit at full scale.
    Clipped,
    /// Overall RMS level is below [`QUIET_RMS_DBFS`] (includes silence).
    TooQuiet,
}

/// Checks mono `samples` recorded at `sample_rate` Hz and returns every warning
/// that applies, in the order of the [`Warning`] variants.
///
/// Returns [`Error::EmptyInput`] for an empty slice and [`Error::InvalidConfig`]
/// for a zero sample rate.
pub fn check(samples: &[f32], sample_rate: u32) -> Result<Vec<Warning>, Error> {
    if samples.is_empty() {
        return Err(Error::EmptyInput);
    }
    if sample_rate == 0 {
        return Err(Error::InvalidConfig("sample rate must be positive"));
    }
    let mut out = Vec::new();
    if (samples.len() as f64 / f64::from(sample_rate)) < MIN_DURATION_S {
        out.push(Warning::TooShort);
    }
    let clipped = samples.iter().filter(|s| s.abs() >= CLIP_LEVEL).count();
    if clipped as f64 / samples.len() as f64 >= CLIP_FRACTION {
        out.push(Warning::Clipped);
    }
    if rms_db(samples) < QUIET_RMS_DBFS {
        out.push(Warning::TooQuiet);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 16_000;

    fn sine(amp: f32, secs: f32) -> Vec<f32> {
        (0..(SR as f32 * secs) as usize)
            .map(|i| amp * (2.0 * std::f32::consts::PI * 200.0 * i as f32 / SR as f32).sin())
            .collect()
    }

    #[test]
    fn clean_recording_has_no_warnings() {
        assert!(check(&sine(0.3, 2.0), SR).unwrap().is_empty());
    }

    #[test]
    fn short_recording_is_flagged() {
        assert_eq!(check(&sine(0.3, 0.5), SR).unwrap(), vec![Warning::TooShort]);
    }

    #[test]
    fn hard_clipped_recording_is_flagged() {
        let x: Vec<f32> = sine(3.0, 2.0).iter().map(|s| s.clamp(-1.0, 1.0)).collect();
        assert_eq!(check(&x, SR).unwrap(), vec![Warning::Clipped]);
    }

    #[test]
    fn a_few_stray_full_scale_samples_are_not_clipping() {
        let mut x = sine(0.3, 2.0);
        x[100] = 1.0;
        assert!(check(&x, SR).unwrap().is_empty());
    }

    #[test]
    fn silence_is_too_quiet_and_quiet_tone_too() {
        assert_eq!(
            check(&vec![0.0; 2 * SR as usize], SR).unwrap(),
            vec![Warning::TooQuiet]
        );
        assert_eq!(check(&sine(0.001, 2.0), SR).unwrap(), vec![Warning::TooQuiet]);
    }

    #[test]
    fn warnings_come_in_variant_order() {
        let x: Vec<f32> = sine(3.0, 0.5).iter().map(|s| s.clamp(-1.0, 1.0)).collect();
        assert_eq!(check(&x, SR).unwrap(), vec![Warning::TooShort, Warning::Clipped]);
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(check(&[], SR), Err(Error::EmptyInput));
        assert!(check(&[0.1; 100], 0).is_err());
    }
}
