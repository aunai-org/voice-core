//! Maximum phonation time: the longest unbroken voiced stretch of a recording, as
//! produced by a sustained vowel or hum.
//!
//! Method: the voiced frames of the pitch track ([`crate::pitch`]) are joined into
//! stretches, and gaps of up to [`MAX_GAP_FRAMES`] unvoiced frames (20 ms, shorter than a
//! stop or a creaky glitch) do not break a stretch. The longest stretch is reported with
//! its start, mean pitch, pitch spread in semitones and level. Praat has no such
//! measure; the stretch itself is checked against the longest voiced run of Praat's own
//! pitch track.

use crate::loudness::rms_db;
use crate::pitch::{AutocorrEstimator, PitchEstimator, PitchFrame};
use crate::Error;

/// Unvoiced frames in a row that still count as part of one stretch.
const MAX_GAP_FRAMES: usize = 2;
/// Shortest stretch (in voiced frames) that is reported.
const MIN_FRAMES: usize = 10;

/// The longest voiced stretch of a recording.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Phonation {
    /// Length of the stretch in seconds.
    pub duration_s: f32,
    /// Where it starts, in seconds from the start of the recording.
    pub start_s: f32,
    /// Mean f0 over its voiced frames, in Hz.
    pub mean_f0_hz: f32,
    /// Standard deviation of f0 over its voiced frames, in semitones (pitch steadiness).
    pub f0_sd_semitones: f32,
    /// RMS level of the stretch in dBFS.
    pub rms_dbfs: f32,
}

/// Longest voiced stretch of `samples` (mono, `sample_rate` Hz), or `None` when no
/// stretch has at least 10 voiced frames.
pub fn max_phonation(samples: &[f32], sample_rate: u32) -> Result<Option<Phonation>, Error> {
    let track = AutocorrEstimator::default().estimate(samples, sample_rate)?;
    Ok(max_phonation_with_track(samples, sample_rate, &track))
}

/// Like [`max_phonation`], reusing a pitch track already computed for `samples`.
pub fn max_phonation_with_track(
    samples: &[f32],
    sample_rate: u32,
    track: &[PitchFrame],
) -> Option<Phonation> {
    if track.len() < 2 {
        return None;
    }
    let hop = track[1].time_s - track[0].time_s;
    let best = voiced_stretches(track)
        .into_iter()
        .fold(None, |b, c| longer(b, Some(c)));
    let (first, last) = best?;
    let hz: Vec<f64> = track[first..=last]
        .iter()
        .filter_map(|f| f.f0_hz.map(f64::from))
        .collect();
    if hz.len() < MIN_FRAMES {
        return None;
    }
    let n = hz.len() as f64;
    let mean = hz.iter().sum::<f64>() / n;
    let st: Vec<f64> = hz.iter().map(|h| 12.0 * (h / mean).log2()).collect();
    let st_mean = st.iter().sum::<f64>() / n;
    let sd = (st.iter().map(|s| (s - st_mean).powi(2)).sum::<f64>() / n).sqrt();
    let start = track[first].time_s - hop / 2.0;
    let end = track[last].time_s + hop / 2.0;
    let a = ((start * f64::from(sample_rate)).max(0.0) as usize).min(samples.len());
    let b = ((end * f64::from(sample_rate)) as usize).min(samples.len());
    Some(Phonation {
        duration_s: (end - start) as f32,
        start_s: start.max(0.0) as f32,
        mean_f0_hz: mean as f32,
        f0_sd_semitones: sd as f32,
        rms_dbfs: rms_db(&samples[a..b]),
    })
}

/// Voiced stretches of a pitch track as `(first, last)` indices of voiced frames, in
/// time order. Gaps of up to [`MAX_GAP_FRAMES`] unvoiced frames do not end a stretch.
pub(crate) fn voiced_stretches(track: &[PitchFrame]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut cur: Option<(usize, usize)> = None;
    let mut gap = 0usize;
    for (i, f) in track.iter().enumerate() {
        if f.f0_hz.is_some() {
            cur = Some(match cur {
                Some((s, _)) => (s, i),
                None => (i, i),
            });
            gap = 0;
        } else if cur.is_some() {
            gap += 1;
            if gap > MAX_GAP_FRAMES {
                out.extend(cur.take());
            }
        }
    }
    out.extend(cur);
    out
}

fn longer(best: Option<(usize, usize)>, cand: Option<(usize, usize)>) -> Option<(usize, usize)> {
    match (best, cand) {
        (Some(b), Some(c)) if c.1 - c.0 > b.1 - b.0 => Some(c),
        (None, c) => c,
        (b, _) => b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 16_000;

    fn tone(freq: f32, secs: f32, amp: f32) -> Vec<f32> {
        (0..(SR as f32 * secs) as usize)
            .map(|i| amp * (std::f32::consts::TAU * freq * i as f32 / SR as f32).sin())
            .collect()
    }

    #[test]
    fn steady_tone_is_one_stretch() {
        let p = max_phonation(&tone(200.0, 3.0, 0.5), SR).unwrap().unwrap();
        assert!((p.duration_s - 3.0).abs() < 0.1, "{}", p.duration_s);
        assert!(p.start_s < 0.1);
        assert!((p.mean_f0_hz - 200.0).abs() < 1.0);
        assert!(p.f0_sd_semitones < 0.1);
        assert!((p.rms_dbfs + 9.03).abs() < 0.3, "{}", p.rms_dbfs);
    }

    #[test]
    fn longest_of_two_stretches_wins() {
        let mut x = tone(150.0, 1.0, 0.5);
        x.extend(vec![0.0; SR as usize / 2]);
        x.extend(tone(250.0, 2.0, 0.25));
        let p = max_phonation(&x, SR).unwrap().unwrap();
        assert!((p.duration_s - 2.0).abs() < 0.1, "{}", p.duration_s);
        assert!((p.start_s - 1.5).abs() < 0.1, "{}", p.start_s);
        assert!((p.mean_f0_hz - 250.0).abs() < 2.0);
        assert!((p.rms_dbfs + 15.05).abs() < 0.5, "{}", p.rms_dbfs);
    }

    #[test]
    fn short_dropout_does_not_split_but_long_gap_does() {
        let mut x = tone(200.0, 1.0, 0.5);
        x.extend(vec![0.0; SR as usize / 100]); // 10 ms
        x.extend(tone(200.0, 1.0, 0.5));
        let joined = max_phonation(&x, SR).unwrap().unwrap();
        assert!(joined.duration_s > 1.9, "{}", joined.duration_s);
        let mut y = tone(200.0, 1.0, 0.5);
        y.extend(vec![0.0; SR as usize / 5]); // 200 ms
        y.extend(tone(200.0, 1.0, 0.5));
        let split = max_phonation(&y, SR).unwrap().unwrap();
        assert!(split.duration_s < 1.2, "{}", split.duration_s);
    }

    #[test]
    fn vibrato_raises_the_pitch_spread() {
        // 200 Hz with a 5 Hz vibrato of +-0.5 semitone (sd about 0.35 semitone).
        let mut phase = 0.0f32;
        let x: Vec<f32> = (0..2 * SR as usize)
            .map(|i| {
                let t = i as f32 / SR as f32;
                let f = 200.0 * 2f32.powf(0.5 * (std::f32::consts::TAU * 5.0 * t).sin() / 12.0);
                phase += std::f32::consts::TAU * f / SR as f32;
                0.5 * phase.sin()
            })
            .collect();
        let p = max_phonation(&x, SR).unwrap().unwrap();
        assert!((0.2..0.5).contains(&p.f0_sd_semitones), "{}", p.f0_sd_semitones);
    }

    #[test]
    fn silence_and_blips_give_none() {
        assert_eq!(max_phonation(&vec![0.0; SR as usize], SR).unwrap(), None);
        let mut x = vec![0.0; SR as usize];
        x[4000..4800].copy_from_slice(&tone(200.0, 0.05, 0.5));
        assert_eq!(max_phonation(&x, SR).unwrap(), None);
    }
}
