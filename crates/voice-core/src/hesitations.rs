//! Hesitation events: long silent pauses inside speech and held sounds (a filled pause
//! such as "uhm", or a stretched syllable), with their times.
//!
//! Method, two rules on measured tracks:
//! * a **pause** is the silence between two speech segments ([`crate::pauses`]) that lasts
//!   at least [`HesitationConfig::min_pause_s`]. Silence before the first and after the
//!   last segment is not a hesitation;
//! * a **held sound** is a voiced stretch of the pitch track ([`crate::pitch`], gaps up
//!   to 20 ms bridged as in [`crate::phonation`]) that lasts at least
//!   [`HesitationConfig::min_held_s`], keeps its pitch within
//!   [`HesitationConfig::max_held_sd_semitones`] standard deviation and its level within
//!   [`HesitationConfig::max_held_level_range_db`] (largest minus smallest 32 ms level,
//!   not counting the first and last 30 ms): running speech moves its pitch and has a dip
//!   in level at every syllable, a held "uhm" has neither.
//!
//! The thresholds are plain choices, not a standard: the defaults are 0.5 s, 0.4 s,
//! 1 semitone and 8 dB. Repetitions and restarts need the words and are not found here.

use crate::pauses::{speech_segments, PauseConfig};
use crate::phonation::voiced_stretches;
use crate::pitch::{AutocorrEstimator, PitchEstimator, PitchFrame};
use crate::Error;

/// What kind of hesitation an event is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HesitationKind {
    /// A silent gap between two stretches of speech.
    Pause,
    /// A voiced sound held at a steady pitch.
    Held,
}

/// One hesitation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hesitation {
    pub kind: HesitationKind,
    /// Start in seconds from the start of the recording.
    pub start_s: f64,
    /// End in seconds.
    pub end_s: f64,
}

impl Hesitation {
    /// Length of the event in seconds.
    pub fn duration_s(&self) -> f64 {
        self.end_s - self.start_s
    }
}

/// Thresholds of the two rules.
#[derive(Debug, Clone, Copy)]
pub struct HesitationConfig {
    /// Shortest silent gap between speech segments that counts, in seconds.
    pub min_pause_s: f64,
    /// Shortest steady voiced stretch that counts, in seconds.
    pub min_held_s: f64,
    /// Largest pitch standard deviation (semitones) of a steady stretch.
    pub max_held_sd_semitones: f64,
    /// Largest spread of the level (dB) of a steady stretch.
    pub max_held_level_range_db: f64,
}

impl Default for HesitationConfig {
    /// 0.5 s pause, 0.4 s held sound, 1 semitone, 8 dB.
    fn default() -> Self {
        Self {
            min_pause_s: 0.5,
            min_held_s: 0.4,
            max_held_sd_semitones: 1.0,
            max_held_level_range_db: 8.0,
        }
    }
}

/// Hesitations of `samples` (mono, `sample_rate` Hz) in time order.
pub fn hesitations(
    samples: &[f32],
    sample_rate: u32,
    config: &HesitationConfig,
) -> Result<Vec<Hesitation>, Error> {
    let track = AutocorrEstimator::default().estimate(samples, sample_rate)?;
    hesitations_with_track(samples, sample_rate, config, &track)
}

/// Like [`hesitations`], reusing a pitch track already computed for `samples`.
pub fn hesitations_with_track(
    samples: &[f32],
    sample_rate: u32,
    config: &HesitationConfig,
    track: &[PitchFrame],
) -> Result<Vec<Hesitation>, Error> {
    if config.min_pause_s.is_nan()
        || config.min_held_s.is_nan()
        || config.max_held_sd_semitones.is_nan()
        || config.max_held_level_range_db.is_nan()
        || config.min_pause_s <= 0.0
        || config.min_held_s <= 0.0
        || config.max_held_sd_semitones < 0.0
        || config.max_held_level_range_db < 0.0
    {
        return Err(Error::InvalidConfig("hesitation thresholds must be positive"));
    }
    let mut out: Vec<Hesitation> = speech_segments(samples, sample_rate, &PauseConfig::default())?
        .windows(2)
        .map(|w| (w[0].end_s, w[1].start_s))
        .filter(|&(a, b)| b - a >= config.min_pause_s)
        .map(|(start_s, end_s)| Hesitation {
            kind: HesitationKind::Pause,
            start_s,
            end_s,
        })
        .collect();
    if track.len() >= 2 {
        let hop = track[1].time_s - track[0].time_s;
        for (first, last) in voiced_stretches(track) {
            let start = (track[first].time_s - hop / 2.0).max(0.0);
            let end = track[last].time_s + hop / 2.0;
            if end - start >= config.min_held_s
                && pitch_sd_semitones(&track[first..=last]) <= config.max_held_sd_semitones
                && level_range_db(samples, sample_rate, start, end) <= config.max_held_level_range_db
            {
                out.push(Hesitation {
                    kind: HesitationKind::Held,
                    start_s: start,
                    end_s: end,
                });
            }
        }
    }
    out.sort_by(|a, b| a.start_s.total_cmp(&b.start_s));
    Ok(out)
}

/// Largest minus smallest level, in dB, of the 32 ms windows (10 ms hop) between `start`
/// and `end` seconds, leaving out the first and last 30 ms.
fn level_range_db(samples: &[f32], sample_rate: u32, start: f64, end: f64) -> f64 {
    const EDGE_S: f64 = 0.03;
    let sr = f64::from(sample_rate);
    let win = (0.032 * sr) as usize;
    let hop = (0.01 * sr) as usize;
    let a = ((start + EDGE_S) * sr) as usize;
    let b = (((end - EDGE_S) * sr) as usize).min(samples.len());
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    let mut i = a;
    while win > 0 && hop > 0 && i + win <= b {
        let e: f64 = samples[i..i + win]
            .iter()
            .map(|&x| f64::from(x) * f64::from(x))
            .sum();
        let l = 10.0 * (e / win as f64).max(1e-12).log10();
        lo = lo.min(l);
        hi = hi.max(l);
        i += hop;
    }
    if hi >= lo {
        hi - lo
    } else {
        0.0
    }
}

/// Standard deviation of the voiced f0 of `frames` in semitones around their mean.
pub(crate) fn pitch_sd_semitones(frames: &[PitchFrame]) -> f64 {
    let hz: Vec<f64> = frames.iter().filter_map(|f| f.f0_hz.map(f64::from)).collect();
    if hz.len() < 2 {
        return 0.0;
    }
    let mean = hz.iter().sum::<f64>() / hz.len() as f64;
    let st: Vec<f64> = hz.iter().map(|h| 12.0 * (h / mean).log2()).collect();
    let m = st.iter().sum::<f64>() / st.len() as f64;
    (st.iter().map(|s| (s - m).powi(2)).sum::<f64>() / st.len() as f64).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 16_000;

    fn tone(freq: f32, secs: f32) -> Vec<f32> {
        (0..(SR as f32 * secs) as usize)
            .map(|i| 0.3 * (std::f32::consts::TAU * freq * i as f32 / SR as f32).sin())
            .collect()
    }

    /// A tone whose pitch glides from `a` to `b` Hz, like a moving syllable.
    fn glide(a: f32, b: f32, secs: f32) -> Vec<f32> {
        let n = (SR as f32 * secs) as usize;
        let mut phase = 0.0f32;
        (0..n)
            .map(|i| {
                let f = a + (b - a) * i as f32 / n as f32;
                phase += std::f32::consts::TAU * f / SR as f32;
                0.3 * phase.sin()
            })
            .collect()
    }

    fn silence(secs: f32) -> Vec<f32> {
        vec![0.0; (SR as f32 * secs) as usize]
    }

    #[test]
    fn long_gap_is_a_pause_and_short_gap_is_not() {
        let mut x = glide(150.0, 220.0, 0.6);
        x.extend(silence(0.8));
        x.extend(glide(220.0, 150.0, 0.6));
        x.extend(silence(0.25));
        x.extend(glide(150.0, 200.0, 0.6));
        let h = hesitations(&x, SR, &HesitationConfig::default()).unwrap();
        let pauses: Vec<_> = h.iter().filter(|e| e.kind == HesitationKind::Pause).collect();
        assert_eq!(pauses.len(), 1, "{h:?}");
        assert!((pauses[0].start_s - 0.6).abs() < 0.05, "{:?}", pauses[0]);
        assert!((pauses[0].duration_s() - 0.8).abs() < 0.1, "{:?}", pauses[0]);
    }

    #[test]
    fn leading_and_trailing_silence_are_not_hesitations() {
        let mut x = silence(1.0);
        x.extend(glide(150.0, 220.0, 0.6));
        x.extend(silence(1.0));
        assert!(hesitations(&x, SR, &HesitationConfig::default())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn steady_sound_is_held_and_a_moving_one_is_not() {
        let held = hesitations(&tone(150.0, 0.8), SR, &HesitationConfig::default()).unwrap();
        assert_eq!(held.len(), 1, "{held:?}");
        assert_eq!(held[0].kind, HesitationKind::Held);
        assert!((held[0].duration_s() - 0.8).abs() < 0.1, "{:?}", held[0]);
        let moving = glide(120.0, 240.0, 0.8);
        assert!(hesitations(&moving, SR, &HesitationConfig::default())
            .unwrap()
            .is_empty());
        // Too short to count.
        assert!(hesitations(&tone(150.0, 0.3), SR, &HesitationConfig::default())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn events_are_in_time_order_and_config_is_checked() {
        let mut x = tone(150.0, 0.6);
        x.extend(silence(0.7));
        x.extend(glide(150.0, 250.0, 0.5));
        let h = hesitations(&x, SR, &HesitationConfig::default()).unwrap();
        assert!(h.windows(2).all(|w| w[0].start_s <= w[1].start_s), "{h:?}");
        assert_eq!(h.len(), 2, "{h:?}");
        let bad = HesitationConfig {
            min_pause_s: 0.0,
            ..HesitationConfig::default()
        };
        assert!(hesitations(&x, SR, &bad).is_err());
        assert!(hesitations(&[], SR, &HesitationConfig::default()).is_err());
    }
}
