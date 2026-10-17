//! Emphasis: how much given words (windows of a recording, for example the key words of a
//! script from word timestamps) stood out in level and pitch, and whether a pause sat
//! next to them, against the same recording's ordinary speech.
//!
//! Method: the level and pitch of each word window come from [`crate::window_stats`]
//! (energy-mean level and mean f0 of its voiced frames, relative to the recording's speech
//! level and median f0). The recording's typical range is the middle half
//! (25th to 75th percentile) of the same two numbers over consecutive 0.4 s windows that
//! lie inside its speech segments ([`crate::pauses`]), so a word is judged against how
//! this speaker sounds in this take, not against a population. A pause counts as
//! "before" or "after" a word when a silent gap between two speech segments ends or
//! starts within 0.2 s of the word's edge; its length is the gap's. Phones are
//! uncalibrated, so only relative numbers are returned. This is a descriptive measure
//! with no listener test behind it: it says how far a word's level and pitch sat from
//! the rest of the take, not whether a listener heard emphasis.

use crate::pauses::{speech_segments, PauseConfig, Segment};
use crate::pitch::{AutocorrEstimator, PitchEstimator};
use crate::window_stats::{percentile, window_stats_with_track};
use crate::Error;

/// Length of the windows that make up the typical range.
const TYPICAL_WINDOW_S: f64 = 0.4;
/// Fewest typical windows with a value for a band to be returned.
const MIN_TYPICAL_WINDOWS: usize = 5;
/// How far from a word's edge a silent gap may start or end and still count as next to it.
const PAUSE_REACH_S: f64 = 0.2;

/// The middle half of a value over the ordinary speech of a recording.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Band {
    /// 25th percentile.
    pub low: f32,
    /// Median.
    pub median: f32,
    /// 75th percentile.
    pub high: f32,
}

/// The typical range of a recording's ordinary speech.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Typical {
    /// Number of 0.4 s windows inside speech segments that were measured.
    pub windows: u32,
    /// Level relative to the speech level in dB; `None` with fewer than 5 windows with a level.
    pub level_db: Option<Band>,
    /// Mean f0 relative to the median f0 in semitones; `None` with fewer than 5 windows with a pitch.
    pub pitch_semitones: Option<Band>,
}

/// How one word stood out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WordEmphasis {
    /// Start of the word window in seconds.
    pub start_s: f64,
    /// End of the word window in seconds.
    pub end_s: f64,
    /// Level minus the recording's speech level, in dB (`None`: window too short or silent).
    pub relative_level_db: Option<f32>,
    /// Mean f0 against the recording's median f0 in semitones (`None`: under 3 voiced frames).
    pub relative_pitch_semitones: Option<f32>,
    /// Length of a silent gap ending within 0.2 s of the word's start; 0 when none.
    pub pause_before_s: f32,
    /// Length of a silent gap starting within 0.2 s of the word's end; 0 when none.
    pub pause_after_s: f32,
}

/// Emphasis of the given words and the typical range they are judged against.
#[derive(Debug, Clone, PartialEq)]
pub struct EmphasisReport {
    /// The recording's typical range.
    pub typical: Typical,
    /// One entry per word window, in the order given.
    pub words: Vec<WordEmphasis>,
}

/// Emphasis of the windows given as `(start_s, end_s)`.
pub fn emphasis(samples: &[f32], sample_rate: u32, words: &[(f64, f64)]) -> Result<EmphasisReport, Error> {
    let track = AutocorrEstimator::default().estimate(samples, sample_rate)?;
    let segments = speech_segments(samples, sample_rate, &PauseConfig::default())?;
    let typical_windows = typical_windows(&segments);
    let all: Vec<(f64, f64)> = typical_windows.iter().chain(words).copied().collect();
    let stats = window_stats_with_track(samples, sample_rate, &all, &track)?;
    let (typical_stats, word_stats) = stats.split_at(typical_windows.len());
    let band = |values: Vec<f64>| {
        (values.len() >= MIN_TYPICAL_WINDOWS).then(|| {
            let mut v = values;
            v.sort_by(f64::total_cmp);
            Band {
                low: percentile(&v, 0.25) as f32,
                median: percentile(&v, 0.5) as f32,
                high: percentile(&v, 0.75) as f32,
            }
        })
    };
    let typical = Typical {
        windows: typical_windows.len() as u32,
        level_db: band(
            typical_stats
                .iter()
                .filter_map(|s| s.relative_level_db.map(f64::from))
                .collect(),
        ),
        pitch_semitones: band(
            typical_stats
                .iter()
                .filter_map(|s| s.relative_pitch_semitones.map(f64::from))
                .collect(),
        ),
    };
    let gaps: Vec<Segment> = segments
        .windows(2)
        .map(|w| Segment {
            start_s: w[0].end_s,
            end_s: w[1].start_s,
        })
        .collect();
    let words = word_stats
        .iter()
        .map(|s| WordEmphasis {
            start_s: s.start_s,
            end_s: s.end_s,
            relative_level_db: s.relative_level_db,
            relative_pitch_semitones: s.relative_pitch_semitones,
            pause_before_s: gap_near(&gaps, s.start_s, |g| g.end_s),
            pause_after_s: gap_near(&gaps, s.end_s, |g| g.start_s),
        })
        .collect();
    Ok(EmphasisReport { typical, words })
}

/// Consecutive 0.4 s windows that lie inside a speech segment, each segment from its start.
fn typical_windows(segments: &[Segment]) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for seg in segments {
        let mut t = seg.start_s;
        while t + TYPICAL_WINDOW_S <= seg.end_s + 1e-9 {
            out.push((t, t + TYPICAL_WINDOW_S));
            t += TYPICAL_WINDOW_S;
        }
    }
    out
}

/// Length of the gap whose edge (picked by `edge`) lies nearest `at` and within the reach, else 0.
fn gap_near(gaps: &[Segment], at: f64, edge: impl Fn(&Segment) -> f64) -> f32 {
    gaps.iter()
        .map(|g| ((edge(g) - at).abs(), g))
        .filter(|(d, _)| *d <= PAUSE_REACH_S)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map_or(0.0, |(_, g)| (g.end_s - g.start_s) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 16_000;

    fn tone(freq: f32, amp: f32, secs: f32) -> Vec<f32> {
        (0..(SR as f32 * secs) as usize)
            .map(|i| amp * (std::f32::consts::TAU * freq * i as f32 / SR as f32).sin())
            .collect()
    }

    /// Eight 0.5 s words 0.3 s apart; word 3 (index 3) is louder and higher and followed by a longer gap.
    fn take() -> (Vec<f32>, Vec<(f64, f64)>) {
        let mut x = Vec::new();
        let mut words = Vec::new();
        for i in 0..8 {
            let start = x.len() as f64 / f64::from(SR);
            if i == 3 {
                x.extend(tone(200.0, 0.2, 0.5));
            } else {
                x.extend(tone(150.0, 0.1, 0.5));
            }
            words.push((start, start + 0.5));
            let gap = if i == 3 { 0.6 } else { 0.3 };
            x.extend(vec![0.0; (SR as f32 * gap) as usize]);
        }
        (x, words)
    }

    #[test]
    fn a_louder_higher_word_is_above_the_typical_range() {
        let (x, words) = take();
        let r = emphasis(&x, SR, &words).unwrap();
        let level = r.typical.level_db.unwrap();
        let pitch = r.typical.pitch_semitones.unwrap();
        let w = &r.words[3];
        assert!(w.relative_level_db.unwrap() > level.high + 1.0, "{w:?} {level:?}");
        assert!(
            w.relative_pitch_semitones.unwrap() > pitch.high + 2.0,
            "{w:?} {pitch:?}"
        );
        // Plain words sit inside the band.
        let plain = &r.words[1];
        assert!(plain.relative_level_db.unwrap() <= level.high + 0.5);
        assert!(plain.relative_pitch_semitones.unwrap().abs() < 1.0);
    }

    #[test]
    fn pauses_next_to_a_word_are_found() {
        let (x, words) = take();
        let r = emphasis(&x, SR, &words).unwrap();
        // The gap after word 3 is 0.6 s; the gap before word 4 is the same gap.
        assert!((r.words[3].pause_after_s - 0.6).abs() < 0.1, "{:?}", r.words[3]);
        assert!((r.words[4].pause_before_s - 0.6).abs() < 0.1, "{:?}", r.words[4]);
        assert!((r.words[1].pause_after_s - 0.3).abs() < 0.1, "{:?}", r.words[1]);
        // No gap before the first word or after the last (silence at the edges is not a pause).
        assert_eq!(r.words[0].pause_before_s, 0.0);
    }

    #[test]
    fn too_little_speech_has_no_typical_range() {
        let x = tone(150.0, 0.1, 1.0);
        let r = emphasis(&x, SR, &[(0.1, 0.5)]).unwrap();
        assert_eq!(r.typical.level_db, None);
        assert_eq!(r.typical.pitch_semitones, None);
        assert_eq!(r.words.len(), 1);
    }

    #[test]
    fn bad_input_is_an_error() {
        let x = tone(150.0, 0.1, 1.0);
        assert!(emphasis(&x, SR, &[(0.5, 0.5)]).is_err());
        assert!(emphasis(&[], SR, &[(0.0, 0.5)]).is_err());
        assert!(emphasis(&x, 0, &[(0.0, 0.5)]).is_err());
    }
}
