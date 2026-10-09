//! One-call batch analysis of a mono recording.

use serde::{Deserialize, Serialize};

use crate::loudness::{integrated_lufs, rms_db};
use crate::pitch::{median_f0, voiced_fraction, AutocorrEstimator, PitchEstimator};
use crate::Error;

/// Summary measurements for one recording.
///
/// Fields are added as new measures land (pace, pauses, steadiness); serialise
/// with serde to JSON for the app layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VoiceReport {
    /// Length of the recording in seconds.
    pub duration_s: f64,
    /// Sample rate of the analysed audio in Hz.
    pub sample_rate_hz: u32,
    /// Overall RMS level in dBFS (pauses count as quiet).
    pub rms_dbfs: f32,
    /// Integrated loudness in LUFS (ITU-R BS.1770, gated).
    pub lufs: f64,
    /// Median fundamental frequency over voiced frames, if any frame is voiced.
    pub f0_median_hz: Option<f32>,
    /// Share of 10 ms frames that are voiced, 0 to 1.
    pub voiced_fraction: f32,
}

/// Analyses mono `samples` recorded at `sample_rate` Hz.
///
/// Returns [`Error::EmptyInput`] for an empty slice and [`Error::InvalidConfig`]
/// for a zero or unsupported sample rate.
pub fn analyze(samples: &[f32], sample_rate: u32) -> Result<VoiceReport, Error> {
    if samples.is_empty() {
        return Err(Error::EmptyInput);
    }
    let lufs = integrated_lufs(samples, sample_rate)?;
    let track = AutocorrEstimator::default().estimate(samples, sample_rate)?;
    Ok(VoiceReport {
        duration_s: samples.len() as f64 / f64::from(sample_rate),
        sample_rate_hz: sample_rate,
        rms_dbfs: rms_db(samples),
        lufs,
        f0_median_hz: median_f0(&track),
        voiced_fraction: voiced_fraction(&track),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 16_000;

    fn sine(freq: f32, secs: f32) -> Vec<f32> {
        (0..(SR as f32 * secs) as usize)
            .map(|i| 0.5 * (2.0 * std::f32::consts::PI * freq * i as f32 / SR as f32).sin())
            .collect()
    }

    #[test]
    fn sine_report_has_expected_fields() {
        let r = analyze(&sine(220.0, 2.0), SR).unwrap();
        assert!((r.duration_s - 2.0).abs() < 1e-9);
        assert_eq!(r.sample_rate_hz, SR);
        assert!((r.rms_dbfs + 9.03).abs() < 0.05);
        assert!((r.f0_median_hz.unwrap() - 220.0).abs() < 1.0);
        assert!(r.voiced_fraction > 0.9);
    }

    #[test]
    fn silence_has_no_pitch() {
        let r = analyze(&vec![0.0; SR as usize], SR).unwrap();
        assert_eq!(r.f0_median_hz, None);
        assert_eq!(r.voiced_fraction, 0.0);
    }

    #[test]
    fn report_round_trips_through_json() {
        let r = analyze(&sine(150.0, 1.0), SR).unwrap();
        let text = serde_json::to_string(&r).unwrap();
        let back: VoiceReport = serde_json::from_str(&text).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(analyze(&[], SR), Err(Error::EmptyInput));
        assert!(analyze(&[0.1; 100], 0).is_err());
    }
}
