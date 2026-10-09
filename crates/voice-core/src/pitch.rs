//! Pitch (f0) estimation behind a small trait, so the backend can be swapped.
//!
//! The first backend is an autocorrelation tracker written from the published
//! method: Boersma, P. (1993), "Accurate short-term analysis of the fundamental
//! frequency and the harmonics-to-noise ratio of a sampled sound", Proc. Institute
//! of Phonetic Sciences 17, 97-110.
//!
//! Per frame: Hann window, normalised autocorrelation divided by the window's own
//! autocorrelation, strongest peak in the pitch range (the shortest lag within 10 %
//! of it, to avoid octave-down errors), parabolic interpolation of the peak.
//! Limits: there is no path search across frames, so octave jumps are possible on
//! real speech; only synthetic signals are validated so far.

use crate::Error;

/// One analysis frame of a pitch track.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PitchFrame {
    /// Centre of the frame in seconds from the start of the signal.
    pub time_s: f64,
    /// Estimated fundamental frequency in Hz, or `None` when the frame is unvoiced.
    pub f0_hz: Option<f32>,
    /// Height of the autocorrelation peak, 0 to 1 (higher means more periodic).
    pub strength: f32,
}

/// Anything that turns audio into a pitch track.
pub trait PitchEstimator {
    /// Estimates f0 for every frame of `samples` (mono, `sample_rate` Hz).
    fn estimate(&self, samples: &[f32], sample_rate: u32) -> Result<Vec<PitchFrame>, Error>;
}

/// Autocorrelation pitch tracker (Boersma 1993).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutocorrEstimator {
    /// Lowest f0 to search for, in Hz. The window spans three periods of it.
    pub fmin_hz: f64,
    /// Highest f0 to search for, in Hz.
    pub fmax_hz: f64,
    /// Time between frames, in seconds.
    pub hop_s: f64,
    /// Minimum peak height for a frame to count as voiced.
    pub voicing_threshold: f32,
    /// Frames quieter than this share of the loudest sample are unvoiced.
    pub silence_threshold: f32,
}

impl Default for AutocorrEstimator {
    /// 75 to 600 Hz, 10 ms hop, voicing 0.45, silence 0.03.
    fn default() -> Self {
        Self {
            fmin_hz: 75.0,
            fmax_hz: 600.0,
            hop_s: 0.01,
            voicing_threshold: 0.45,
            silence_threshold: 0.03,
        }
    }
}

fn autocorr(x: &[f32], max_lag: usize) -> Vec<f64> {
    (0..=max_lag)
        .map(|lag| {
            x.iter()
                .zip(&x[lag..])
                .map(|(a, b)| f64::from(*a) * f64::from(*b))
                .sum()
        })
        .collect()
}

impl PitchEstimator for AutocorrEstimator {
    fn estimate(&self, samples: &[f32], sample_rate: u32) -> Result<Vec<PitchFrame>, Error> {
        if samples.is_empty() {
            return Err(Error::EmptyInput);
        }
        if sample_rate == 0 {
            return Err(Error::InvalidConfig("sample rate must be positive"));
        }
        let sr = f64::from(sample_rate);
        if !(self.fmin_hz > 0.0 && self.fmin_hz < self.fmax_hz && self.fmax_hz <= sr / 2.0) {
            return Err(Error::InvalidConfig(
                "pitch range must satisfy 0 < fmin < fmax <= sample rate / 2",
            ));
        }
        if self.hop_s.is_nan() || self.hop_s <= 0.0 {
            return Err(Error::InvalidConfig("hop must be positive"));
        }
        let min_lag = ((sr / self.fmax_hz).floor() as usize).max(1);
        let max_lag = (sr / self.fmin_hz).ceil() as usize;
        if max_lag < min_lag + 2 {
            return Err(Error::InvalidConfig(
                "pitch range is too narrow for this sample rate",
            ));
        }
        let win = ((3.0 * sr / self.fmin_hz).round() as usize).max(2 * max_lag + 2) | 1;
        let hop = ((self.hop_s * sr).round() as usize).max(1);
        let hann: Vec<f32> = (0..win)
            .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * (i as f32 + 0.5) / win as f32).cos())
            .collect();
        let win_ac = autocorr(&hann, max_lag);
        let peak = samples.iter().fold(0.0_f32, |m, x| m.max(x.abs()));

        let mut track = Vec::new();
        let mut centre = 0usize;
        let mut buf = vec![0.0_f32; win];
        while centre < samples.len() {
            // Window centred on `centre`, zero-padded outside the signal.
            let start = centre as isize - (win / 2) as isize;
            let mut local_peak = 0.0_f32;
            let mut sum = 0.0_f32;
            let mut n = 0usize;
            for (i, b) in buf.iter_mut().enumerate() {
                let j = start + i as isize;
                *b = if j >= 0 && (j as usize) < samples.len() {
                    samples[j as usize]
                } else {
                    0.0
                };
                if j >= 0 && (j as usize) < samples.len() {
                    sum += *b;
                    n += 1;
                    local_peak = local_peak.max(b.abs());
                }
            }
            let mean = if n > 0 { sum / n as f32 } else { 0.0 };
            let mut f0_hz = None;
            let mut strength = 0.0_f32;
            if peak > 0.0 && local_peak >= self.silence_threshold * peak {
                for (b, w) in buf.iter_mut().zip(&hann) {
                    *b = (*b - mean) * w;
                }
                let ac = autocorr(&buf, max_lag);
                if ac[0] > 0.0 {
                    let r: Vec<f64> = (0..=max_lag)
                        .map(|l| (ac[l] / ac[0]) / (win_ac[l] / win_ac[0]))
                        .collect();
                    let mut best = 0.0_f64;
                    for l in min_lag..max_lag {
                        if r[l] > r[l - 1] && r[l] >= r[l + 1] && r[l] > best {
                            best = r[l];
                        }
                    }
                    if best > 0.0 {
                        // Shortest peak within 10 % of the best one avoids octave-down picks.
                        let lag = (min_lag..max_lag)
                            .find(|&l| r[l] > r[l - 1] && r[l] >= r[l + 1] && r[l] >= 0.9 * best)
                            .unwrap_or(min_lag);
                        let (a, b, c) = (r[lag - 1], r[lag], r[lag + 1]);
                        let denom = a - 2.0 * b + c;
                        let shift = if denom.abs() > 1e-12 {
                            0.5 * (a - c) / denom
                        } else {
                            0.0
                        };
                        let peak_r = b - 0.25 * (a - c) * shift;
                        strength = peak_r.clamp(0.0, 1.0) as f32;
                        if strength >= self.voicing_threshold {
                            f0_hz = Some((sr / (lag as f64 + shift)) as f32);
                        }
                    }
                }
            }
            track.push(PitchFrame {
                time_s: centre as f64 / sr,
                f0_hz,
                strength,
            });
            centre += hop;
        }
        Ok(track)
    }
}

/// Median f0 over the voiced frames, or `None` if no frame is voiced.
pub fn median_f0(track: &[PitchFrame]) -> Option<f32> {
    let mut v: Vec<f32> = track.iter().filter_map(|f| f.f0_hz).collect();
    if v.is_empty() {
        return None;
    }
    v.sort_by(f32::total_cmp);
    Some(v[v.len() / 2])
}

/// Share of frames that are voiced, 0 to 1 (0 for an empty track).
pub fn voiced_fraction(track: &[PitchFrame]) -> f32 {
    if track.is_empty() {
        return 0.0;
    }
    track.iter().filter(|f| f.f0_hz.is_some()).count() as f32 / track.len() as f32
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
    fn sines_across_the_range_are_accurate() {
        let mut f = 80.0_f32;
        while f <= 580.0 {
            let track = AutocorrEstimator::default().estimate(&sine(f, 0.5), SR).unwrap();
            let got = median_f0(&track).unwrap_or(0.0);
            assert!((got - f).abs() / f < 0.01, "{f}: got {got}");
            f += 20.0;
        }
    }

    #[test]
    fn silence_and_noise_are_mostly_unvoiced() {
        let e = AutocorrEstimator::default();
        let silent = e.estimate(&vec![0.0; SR as usize / 2], SR).unwrap();
        assert_eq!(voiced_fraction(&silent), 0.0);
        let mut s = 12345_u64;
        let noise: Vec<f32> = (0..SR as usize / 2)
            .map(|_| {
                s = s
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                ((s >> 33) as f32 / (1u64 << 31) as f32) - 0.5
            })
            .collect();
        assert!(voiced_fraction(&e.estimate(&noise, SR).unwrap()) < 0.2);
    }

    #[test]
    fn pause_between_tones_is_unvoiced() {
        let mut x = sine(150.0, 0.5);
        x.extend(vec![0.0; SR as usize / 2]);
        x.extend(sine(150.0, 0.5));
        let track = AutocorrEstimator::default().estimate(&x, SR).unwrap();
        let frac = voiced_fraction(&track);
        assert!((0.55..0.75).contains(&frac), "voiced fraction {frac}");
    }

    #[test]
    fn rejects_bad_input() {
        let e = AutocorrEstimator::default();
        assert_eq!(e.estimate(&[], SR), Err(Error::EmptyInput));
        assert!(e.estimate(&[0.0; 100], 0).is_err());
        let bad = AutocorrEstimator {
            fmin_hz: 500.0,
            fmax_hz: 100.0,
            ..e
        };
        assert!(bad.estimate(&[0.0; 4000], SR).is_err());
        let high = AutocorrEstimator { fmax_hz: 9000.0, ..e };
        assert!(high.estimate(&[0.0; 4000], SR).is_err());
        let hop = AutocorrEstimator { hop_s: 0.0, ..e };
        assert!(hop.estimate(&[0.0; 4000], SR).is_err());
    }
}
