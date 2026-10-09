//! One-call batch analysis of a mono recording.

use serde::{Deserialize, Serialize};

use crate::loudness::{integrated_lufs, rms_db};
use crate::pace::{syllable_nuclei, PaceConfig};
use crate::pauses::{pause_stats, speech_segments, PauseConfig};
use crate::pitch::{median_f0, voiced_fraction, AutocorrEstimator, PitchEstimator};
use crate::quality::{self, Warning};
use crate::Error;

/// Summary measurements for one recording.
///
/// Fields are added as new measures land (pauses, steadiness); serialise
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
    /// Number of detected syllable nuclei.
    pub syllable_count: u32,
    /// Syllables per second over the whole recording, pauses included.
    pub syllables_per_second: f32,
    /// Number of pauses (silent gaps of at least 0.1 s between speech segments;
    /// leading and trailing silence does not count).
    pub pause_count: u32,
    /// Total pause time in seconds.
    pub pause_total_s: f32,
    /// Longest pause in seconds, 0 when there is none.
    pub longest_pause_s: f32,
    /// Quality warnings for this recording; empty when nothing is wrong.
    pub warnings: Vec<Warning>,
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
    let duration_s = samples.len() as f64 / f64::from(sample_rate);
    let syllables = syllable_nuclei(samples, sample_rate, &PaceConfig::default())?.len();
    let pauses = pause_stats(&speech_segments(samples, sample_rate, &PauseConfig::default())?);
    Ok(VoiceReport {
        duration_s,
        sample_rate_hz: sample_rate,
        rms_dbfs: rms_db(samples),
        lufs,
        f0_median_hz: median_f0(&track),
        voiced_fraction: voiced_fraction(&track),
        syllable_count: syllables as u32,
        syllables_per_second: (syllables as f64 / duration_s) as f32,
        pause_count: pauses.count as u32,
        pause_total_s: pauses.total_s as f32,
        longest_pause_s: pauses.longest_s as f32,
        warnings: quality::check(samples, sample_rate)?,
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
        assert_eq!(r.syllable_count, 0);
        assert_eq!(r.syllables_per_second, 0.0);
        assert_eq!(r.pause_count, 0);
        assert_eq!(r.longest_pause_s, 0.0);
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn short_quiet_input_carries_warnings() {
        let r = analyze(&vec![0.0; SR as usize / 2], SR).unwrap();
        assert_eq!(r.warnings, vec![Warning::TooShort, Warning::TooQuiet]);
    }

    #[test]
    fn burst_report_counts_syllables() {
        // 150 Hz tone in raised-cosine bursts at 4 Hz for 3 s: 12 syllable-like bursts.
        let x: Vec<f32> = (0..3 * SR as usize)
            .map(|i| {
                let t = i as f64 / f64::from(SR);
                let env = 0.5 * (1.0 - (std::f64::consts::TAU * 4.0 * t).cos());
                (0.5 * env * (std::f64::consts::TAU * 150.0 * t).sin()) as f32
            })
            .collect();
        let r = analyze(&x, SR).unwrap();
        assert_eq!(r.syllable_count, 12);
        assert!((r.syllables_per_second - 4.0).abs() < 1e-6);
    }

    #[test]
    fn gap_between_tones_is_one_pause() {
        let mut x = sine(150.0, 1.0);
        x.extend(vec![0.0; SR as usize / 2]);
        x.extend(sine(150.0, 1.0));
        let r = analyze(&x, SR).unwrap();
        assert_eq!(r.pause_count, 1);
        assert!((r.pause_total_s - 0.5).abs() < 0.05, "{}", r.pause_total_s);
        assert!((r.longest_pause_s - 0.5).abs() < 0.05);
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
        // serde_json's default f64 parsing can differ from the written value
        // in the last digit (seen on macOS), so f64 fields get a tiny tolerance.
        assert!((r.duration_s - back.duration_s).abs() < 1e-12);
        assert!((r.lufs - back.lufs).abs() < 1e-12);
        let exact = VoiceReport {
            duration_s: r.duration_s,
            lufs: r.lufs,
            ..back
        };
        assert_eq!(r, exact);
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(analyze(&[], SR), Err(Error::EmptyInput));
        assert!(analyze(&[0.1; 100], 0).is_err());
    }
}
