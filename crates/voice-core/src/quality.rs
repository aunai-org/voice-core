//! Recording-quality warnings: cheap checks that say when the other numbers
//! should be read with care.
//!
//! The thresholds are practical defaults, not values from a standard, and
//! there is no Praat measurement to compare them against.

use serde::{Deserialize, Serialize};

use crate::loudness::rms_db;
use crate::pauses::smoothed_levels;
use crate::Error;

/// Shorter than this many seconds, pitch and pace summaries are unreliable.
pub const MIN_DURATION_S: f64 = 1.0;
/// Absolute sample value from which a sample counts as clipped.
pub const CLIP_LEVEL: f32 = 0.999;
/// Share of clipped samples from which the recording is flagged.
pub const CLIP_FRACTION: f64 = 0.001;
/// Overall RMS below this level (dBFS) is flagged as too quiet.
pub const QUIET_RMS_DBFS: f32 = -50.0;

/// A speech-to-noise ratio below this many dB is flagged as noisy.
pub const MIN_SNR_DB: f32 = 20.0;
/// Level spread (dB) below which a recording has no loud/quiet contrast to estimate from.
const MIN_SPREAD_DB: f32 = 3.0;

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
    /// The speech-to-noise ratio from [`snr_db`] is below [`MIN_SNR_DB`].
    Noisy,
}

/// Rough speech-to-noise ratio in dB: the level of the loud frames (95th percentile)
/// minus the level of the quiet frames (5th percentile), on 32 ms Gaussian-window
/// levels every 10 ms (the levels used for pauses).
///
/// Assumes the recording holds both speech and a stretch of background; returns
/// `None` when the loud and quiet frames differ by under 3 dB (a steady tone, pure
/// noise, silence), where there is nothing to separate. With no pauses in the
/// recording the quiet frames are the quietest speech, so the value reads too high.
pub fn snr_db(samples: &[f32], sample_rate: u32) -> Option<f32> {
    let len = ((0.032 * f64::from(sample_rate)).round() as usize).max(2);
    let hop = ((0.01 * f64::from(sample_rate)).round() as usize).max(1);
    let mut levels = smoothed_levels(samples, len, hop);
    if levels.len() < 20 {
        return None;
    }
    levels.sort_by(f32::total_cmp);
    let at = |q: f64| levels[((levels.len() - 1) as f64 * q).round() as usize];
    let spread = at(0.95) - at(0.05);
    (spread >= MIN_SPREAD_DB).then_some(spread)
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
    if snr_db(samples, sample_rate).is_some_and(|snr| snr < MIN_SNR_DB) {
        out.push(Warning::Noisy);
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

    /// Tone bursts over a constant background tone at 1.3 kHz.
    fn speech_over_noise(speech_amp: f32, noise_amp: f32) -> Vec<f32> {
        (0..3 * SR as usize)
            .map(|i| {
                let t = i as f32 / SR as f32;
                let on = (t % 1.0) < 0.5;
                let speech = if on {
                    speech_amp * (2.0 * std::f32::consts::PI * 200.0 * t).sin()
                } else {
                    0.0
                };
                speech + noise_amp * (2.0 * std::f32::consts::PI * 1300.0 * t).sin()
            })
            .collect()
    }

    #[test]
    fn snr_is_recovered_within_a_few_db() {
        // 0.3 vs 0.01 amplitude: 29.5 dB between speech and background.
        let snr = snr_db(&speech_over_noise(0.3, 0.01), SR).unwrap();
        assert!((snr - 29.5).abs() < 3.0, "{snr}");
    }

    #[test]
    fn noisy_recording_is_flagged_and_clean_one_is_not() {
        assert!(check(&speech_over_noise(0.3, 0.01), SR).unwrap().is_empty());
        assert_eq!(
            check(&speech_over_noise(0.3, 0.1), SR).unwrap(),
            vec![Warning::Noisy]
        );
    }

    #[test]
    fn steady_signals_have_no_snr_estimate() {
        assert_eq!(snr_db(&sine(0.3, 2.0), SR), None);
        assert_eq!(snr_db(&vec![0.0; 2 * SR as usize], SR), None);
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
