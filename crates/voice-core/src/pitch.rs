//! Pitch (f0) estimation behind a small trait, so the backend can be swapped.
//!
//! The first backend is an autocorrelation tracker written from the published
//! method: Boersma, P. (1993), "Accurate short-term analysis of the fundamental
//! frequency and the harmonics-to-noise ratio of a sampled sound", Proc. Institute
//! of Phonetic Sciences 17, 97-110.
//!
//! Per frame: Hann window, normalised autocorrelation divided by the window's own
//! autocorrelation, the strongest peaks in the pitch range as candidates (parabolic
//! interpolation), plus an "unvoiced" candidate. A Viterbi search over the whole
//! recording then picks one candidate per frame, trading candidate strength against
//! costs for octave jumps and for voiced/unvoiced switches, as in the paper.
//! Limits: checked on synthetic signals and eight clean read-speech clips; noisy,
//! breathy and creaky voices are untested.

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

/// One pitch candidate for a frame; `f0_hz` is `None` for the unvoiced candidate.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Candidate {
    pub(crate) f0_hz: Option<f64>,
    pub(crate) strength: f64,
}

/// Picks the best candidate sequence (maximum summed strength minus transition costs).
fn best_path(frames: &[Vec<Candidate>], octave_jump_cost: f64, voiced_unvoiced_cost: f64) -> Vec<usize> {
    let transition = |a: &Candidate, b: &Candidate| match (a.f0_hz, b.f0_hz) {
        (Some(x), Some(y)) => octave_jump_cost * (x / y).log2().abs(),
        (None, None) => 0.0,
        _ => voiced_unvoiced_cost,
    };
    let mut score: Vec<f64> = frames[0].iter().map(|c| c.strength).collect();
    let mut back: Vec<Vec<usize>> = Vec::with_capacity(frames.len());
    back.push(vec![0; frames[0].len()]);
    for t in 1..frames.len() {
        let mut next = Vec::with_capacity(frames[t].len());
        let mut from = Vec::with_capacity(frames[t].len());
        for cur in &frames[t] {
            let (best_i, best_v) = frames[t - 1]
                .iter()
                .enumerate()
                .map(|(i, prev)| (i, score[i] - transition(prev, cur)))
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .expect("every frame has an unvoiced candidate");
            next.push(best_v + cur.strength);
            from.push(best_i);
        }
        score = next;
        back.push(from);
    }
    let mut idx = score
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map(|(i, _)| i)
        .expect("non-empty");
    let mut path = vec![0; frames.len()];
    for t in (0..frames.len()).rev() {
        path[t] = idx;
        idx = back[t][idx];
    }
    path
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
    /// Cost per octave of preferring a longer lag, so the shortest plausible period wins ties.
    pub octave_cost: f32,
    /// Cost per octave of jumping in f0 between neighbouring voiced frames.
    pub octave_jump_cost: f32,
    /// Cost of switching between voiced and unvoiced between neighbouring frames.
    pub voiced_unvoiced_cost: f32,
    /// Most candidates kept per frame (the strongest autocorrelation peaks).
    pub max_candidates: usize,
}

impl Default for AutocorrEstimator {
    /// 75 to 600 Hz, 10 ms hop, voicing 0.45, silence 0.03, octave cost 0.01, octave jump cost
    /// 0.35, voiced/unvoiced cost 0.14, 15 candidates (the values of Boersma 1993).
    fn default() -> Self {
        Self {
            fmin_hz: 75.0,
            fmax_hz: 600.0,
            hop_s: 0.01,
            voicing_threshold: 0.45,
            silence_threshold: 0.03,
            octave_cost: 0.01,
            octave_jump_cost: 0.35,
            voiced_unvoiced_cost: 0.14,
            max_candidates: 15,
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

/// Candidate search for one frame, shared by the batch tracker and the streaming
/// [`crate::stream::Analyzer`]: the constants derived from the config and sample rate.
pub(crate) struct FrameSearch {
    sr: f64,
    min_lag: usize,
    max_lag: usize,
    win: usize,
    hann: Vec<f32>,
    win_ac: Vec<f64>,
    cfg: AutocorrEstimator,
}

impl FrameSearch {
    pub(crate) fn new(cfg: &AutocorrEstimator, sample_rate: u32) -> Result<Self, Error> {
        if sample_rate == 0 {
            return Err(Error::InvalidConfig("sample rate must be positive"));
        }
        let sr = f64::from(sample_rate);
        if !(cfg.fmin_hz > 0.0 && cfg.fmin_hz < cfg.fmax_hz && cfg.fmax_hz <= sr / 2.0) {
            return Err(Error::InvalidConfig(
                "pitch range must satisfy 0 < fmin < fmax <= sample rate / 2",
            ));
        }
        if cfg.hop_s.is_nan() || cfg.hop_s <= 0.0 {
            return Err(Error::InvalidConfig("hop must be positive"));
        }
        let min_lag = ((sr / cfg.fmax_hz).floor() as usize).max(1);
        let max_lag = (sr / cfg.fmin_hz).ceil() as usize;
        if max_lag < min_lag + 2 {
            return Err(Error::InvalidConfig(
                "pitch range is too narrow for this sample rate",
            ));
        }
        if cfg.max_candidates == 0 {
            return Err(Error::InvalidConfig("max_candidates must be positive"));
        }
        let win = ((3.0 * sr / cfg.fmin_hz).round() as usize).max(2 * max_lag + 2) | 1;
        let hann: Vec<f32> = (0..win)
            .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * (i as f32 + 0.5) / win as f32).cos())
            .collect();
        let win_ac = autocorr(&hann, max_lag);
        Ok(Self {
            sr,
            min_lag,
            max_lag,
            win,
            hann,
            win_ac,
            cfg: *cfg,
        })
    }

    /// Samples in the analysis window.
    pub(crate) fn window_len(&self) -> usize {
        self.win
    }

    /// Samples of the window before its centre sample.
    pub(crate) fn half(&self) -> usize {
        self.win / 2
    }

    /// Candidates (unvoiced first) for the frame centred on sample `centre`. `get`
    /// returns the sample at a stream index, or `None` outside the signal (read as
    /// zero and left out of the mean and peak); `peak` is the loudest absolute sample.
    pub(crate) fn candidates(
        &self,
        centre: usize,
        peak: f32,
        get: impl Fn(usize) -> Option<f32>,
    ) -> Vec<Candidate> {
        let vt = f64::from(self.cfg.voicing_threshold);
        let start = centre as isize - self.half() as isize;
        let mut buf = vec![0.0_f32; self.win];
        let mut sum = 0.0_f32;
        let mut n = 0usize;
        for (i, b) in buf.iter_mut().enumerate() {
            let j = start + i as isize;
            if let Some(x) = (j >= 0).then(|| get(j as usize)).flatten() {
                *b = x;
                sum += x;
                n += 1;
            }
        }
        let mean = if n > 0 { sum / n as f32 } else { 0.0 };
        // Loudness of the frame as the window sees it: the peak of the mean-removed,
        // windowed samples (zero padding outside the signal stays zero).
        let mut local_peak = 0.0_f32;
        for (i, (b, w)) in buf.iter().zip(&self.hann).enumerate() {
            let j = start + i as isize;
            if j >= 0 && get(j as usize).is_some() {
                local_peak = local_peak.max(((*b - mean) * w).abs());
            }
        }
        // The unvoiced candidate gets stronger as the frame gets quieter.
        let quiet = if peak > 0.0 {
            f64::from(local_peak / peak) / (f64::from(self.cfg.silence_threshold) / (1.0 + vt))
        } else {
            0.0
        };
        let mut cands = vec![Candidate {
            f0_hz: None,
            strength: vt + (2.0 - quiet).max(0.0),
        }];
        if peak > 0.0 && local_peak >= self.cfg.silence_threshold * peak {
            for (b, w) in buf.iter_mut().zip(&self.hann) {
                *b = (*b - mean) * w;
            }
            let ac = autocorr(&buf, self.max_lag);
            if ac[0] > 0.0 {
                let r: Vec<f64> = (0..=self.max_lag)
                    .map(|l| (ac[l] / ac[0]) / (self.win_ac[l] / self.win_ac[0]))
                    .collect();
                let mut peaks: Vec<Candidate> = Vec::new();
                for l in self.min_lag..self.max_lag {
                    if r[l] > r[l - 1] && r[l] >= r[l + 1] && r[l] > 0.0 {
                        let (a, b, c) = (r[l - 1], r[l], r[l + 1]);
                        let denom = a - 2.0 * b + c;
                        let shift = if denom.abs() > 1e-12 {
                            0.5 * (a - c) / denom
                        } else {
                            0.0
                        };
                        let height = (b - 0.25 * (a - c) * shift).clamp(0.0, 1.0);
                        let lag = l as f64 + shift;
                        peaks.push(Candidate {
                            f0_hz: Some(self.sr / lag),
                            // Longer lags (lower f0) pay a small octave cost.
                            strength: height
                                - f64::from(self.cfg.octave_cost) * (self.cfg.fmin_hz * lag / self.sr).log2(),
                        });
                    }
                }
                peaks.sort_by(|a, b| b.strength.total_cmp(&a.strength));
                peaks.truncate(self.cfg.max_candidates);
                cands.extend(peaks);
            }
        }
        cands
    }

    /// The raw peak height of a chosen candidate, undoing the octave cost.
    pub(crate) fn raw_strength(&self, c: &Candidate) -> f32 {
        c.f0_hz.map_or(0.0, |f| {
            (c.strength + f64::from(self.cfg.octave_cost) * (self.cfg.fmin_hz / f).log2()).clamp(0.0, 1.0)
                as f32
        })
    }
}

impl PitchEstimator for AutocorrEstimator {
    fn estimate(&self, samples: &[f32], sample_rate: u32) -> Result<Vec<PitchFrame>, Error> {
        if samples.is_empty() {
            return Err(Error::EmptyInput);
        }
        let search = FrameSearch::new(self, sample_rate)?;
        let sr = f64::from(sample_rate);
        let hop = ((self.hop_s * sr).round() as usize).max(1);
        let peak = samples.iter().fold(0.0_f32, |m, x| m.max(x.abs()));
        let mut times = Vec::new();
        let mut frames: Vec<Vec<Candidate>> = Vec::new();
        let mut centre = 0usize;
        while centre < samples.len() {
            frames.push(search.candidates(centre, peak, |j| samples.get(j).copied()));
            times.push(centre as f64 / sr);
            centre += hop;
        }
        let path = best_path(
            &frames,
            f64::from(self.octave_jump_cost),
            f64::from(self.voiced_unvoiced_cost),
        );
        Ok(times
            .into_iter()
            .zip(frames.iter().zip(path))
            .map(|(time_s, (cands, i))| {
                let c = cands[i];
                PitchFrame {
                    time_s,
                    f0_hz: c.f0_hz.map(|f| f as f32),
                    strength: search.raw_strength(&c),
                }
            })
            .collect())
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
    fn zero_candidates_is_an_error() {
        let est = AutocorrEstimator {
            max_candidates: 0,
            ..AutocorrEstimator::default()
        };
        assert!(est.estimate(&sine(200.0, 0.5), 16_000).is_err());
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
