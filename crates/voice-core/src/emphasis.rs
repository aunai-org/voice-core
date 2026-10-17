//! Emphasis: how much each word of a reading stood out from the other words, in level
//! and in pitch, and how long a pause came before it.
//!
//! Method: the caller gives the words as `(start_s, end_s)` windows in time order (word
//! timestamps from a recogniser or an aligner). Each word is measured with
//! [`crate::window_stats`] (energy-mean level, mean f0 of its voiced frames). The
//! context of a word is every *other* word. Its **level lift** is its level minus the
//! median level of the context, in dB. Its **pitch lift** is `12 log2(f0 / median f0 of
//! the context)`, in semitones. Its **pause before** is the gap between the end of the
//! previous word and its own start. Using the median and leaving the word itself out keeps
//! one stressed word from hiding itself. A lift needs at least [`MIN_CONTEXT`] other words
//! with a level (or a pitch). The numbers are lifts above the speaker's own other words,
//! not a verdict: what lift counts as strong is for the caller to decide.

use crate::pitch::{AutocorrEstimator, PitchEstimator};
use crate::window_stats::{percentile, window_stats_with_track};
use crate::Error;

/// Fewest other words needed to say what is usual for the speaker.
pub const MIN_CONTEXT: usize = 3;

/// How much one word stood out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Emphasis {
    /// Position of the word in the list given.
    pub index: usize,
    /// Start of the word in seconds.
    pub start_s: f64,
    /// End of the word in seconds.
    pub end_s: f64,
    /// Level minus the median level of the other words, in dB.
    pub level_lift_db: Option<f32>,
    /// Mean f0 against the median word f0 of the other words, in semitones.
    pub pitch_lift_semitones: Option<f32>,
    /// Silence between the end of the previous word and the start of this one, in seconds;
    /// `None` for the first word.
    pub pause_before_s: Option<f32>,
}

/// Emphasis of each word, in the order given. `words` are `(start_s, end_s)` windows in
/// time order.
pub fn emphasis(samples: &[f32], sample_rate: u32, words: &[(f64, f64)]) -> Result<Vec<Emphasis>, Error> {
    let track = AutocorrEstimator::default().estimate(samples, sample_rate)?;
    let stats = window_stats_with_track(samples, sample_rate, words, &track)?;
    let levels: Vec<Option<f64>> = stats.iter().map(|s| s.level_dbfs.map(f64::from)).collect();
    let pitches: Vec<Option<f64>> = stats.iter().map(|s| s.mean_f0_hz.map(f64::from)).collect();
    Ok(words
        .iter()
        .enumerate()
        .map(|(i, &(start_s, end_s))| Emphasis {
            index: i,
            start_s,
            end_s,
            level_lift_db: median_of_others(&levels, i)
                .zip(levels[i])
                .map(|(m, l)| (l - m) as f32),
            pitch_lift_semitones: median_of_others(&pitches, i)
                .zip(pitches[i])
                .map(|(m, p)| (12.0 * (p / m).log2()) as f32),
            pause_before_s: (i > 0).then(|| (start_s - words[i - 1].1).max(0.0) as f32),
        })
        .collect())
}

/// Median of the values other than `values[skip]`, `None` with fewer than [`MIN_CONTEXT`].
fn median_of_others(values: &[Option<f64>], skip: usize) -> Option<f64> {
    let mut v: Vec<f64> = values
        .iter()
        .enumerate()
        .filter(|&(i, _)| i != skip)
        .filter_map(|(_, &x)| x)
        .collect();
    if v.len() < MIN_CONTEXT {
        return None;
    }
    v.sort_by(f64::total_cmp);
    Some(percentile(&v, 0.5))
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

    /// Five 0.4 s words at 150 Hz, the third one 6 dB louder and at 200 Hz, with 0.1 s of
    /// silence before the fourth.
    fn reading() -> (Vec<f32>, Vec<(f64, f64)>) {
        let mut x = Vec::new();
        let mut words = Vec::new();
        for i in 0..5 {
            if i == 3 {
                x.extend(vec![0.0; SR as usize / 10]);
            }
            let start = x.len() as f64 / f64::from(SR);
            if i == 2 {
                x.extend(tone(200.0, 0.2, 0.4));
            } else {
                x.extend(tone(150.0, 0.1, 0.4));
            }
            words.push((start, x.len() as f64 / f64::from(SR)));
        }
        (x, words)
    }

    #[test]
    fn stressed_word_has_the_lift_and_the_others_have_none() {
        let (x, words) = reading();
        let e = emphasis(&x, SR, &words).unwrap();
        assert!((e[2].level_lift_db.unwrap() - 6.02).abs() < 0.3, "{:?}", e[2]);
        assert!(
            (e[2].pitch_lift_semitones.unwrap() - 4.98).abs() < 0.3,
            "{:?}",
            e[2]
        );
        for i in [0, 1, 3, 4] {
            assert!(e[i].level_lift_db.unwrap().abs() < 0.3, "{:?}", e[i]);
            assert!(e[i].pitch_lift_semitones.unwrap().abs() < 0.3, "{:?}", e[i]);
        }
    }

    #[test]
    fn pause_before_is_the_gap_to_the_previous_word() {
        let (x, words) = reading();
        let e = emphasis(&x, SR, &words).unwrap();
        assert_eq!(e[0].pause_before_s, None);
        assert!(e[1].pause_before_s.unwrap() < 0.001);
        assert!((e[3].pause_before_s.unwrap() - 0.1).abs() < 0.001, "{:?}", e[3]);
    }

    #[test]
    fn too_few_other_words_gives_no_lift() {
        let (x, words) = reading();
        let e = emphasis(&x, SR, &words[..3]).unwrap();
        assert_eq!(e.len(), 3);
        assert!(e
            .iter()
            .all(|w| w.level_lift_db.is_none() && w.pitch_lift_semitones.is_none()));
    }

    #[test]
    fn a_silent_word_has_no_lift_but_does_not_break_the_others() {
        let (mut x, words) = reading();
        let (a, b) = (
            (words[4].0 * f64::from(SR)) as usize,
            (words[4].1 * f64::from(SR)) as usize,
        );
        x[a..b].iter_mut().for_each(|s| *s = 0.0);
        let e = emphasis(&x, SR, &words).unwrap();
        assert_eq!(e[4].level_lift_db, None);
        assert!(e[2].level_lift_db.unwrap() > 5.0);
    }

    #[test]
    fn empty_words_and_bad_input() {
        let (x, words) = reading();
        assert!(emphasis(&x, SR, &[]).unwrap().is_empty());
        assert!(emphasis(&x, SR, &[(0.5, 0.2)]).is_err());
        assert!(emphasis(&[], SR, &words).is_err());
        assert!(emphasis(&x, 0, &words).is_err());
    }
}
