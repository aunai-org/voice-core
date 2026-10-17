//! Level ladder: the average speech level of each step of a volume exercise (for example
//! whisper, normal, projected), in dBFS and relative to the speaker's own baseline.
//!
//! Method: the caller gives the time window of each step. Inside each window the speech
//! segments are found with the pause detector ([`crate::pauses`]), run on that window
//! alone so a quiet step is not mistaken for silence next to a loud one. The step's level
//! is the energy mean, `10 log10(mean(x^2))`, of the samples inside those segments. The
//! baseline is the first step that has speech (or a level the caller supplies), and
//! each step's `relative_db` is its level minus the baseline. Phones are uncalibrated, so
//! only the relative numbers are comparable between recordings.

use crate::pauses::{speech_segments, PauseConfig};
use crate::Error;

/// Least speech (in seconds) a step needs to have a level.
const MIN_SPEECH_S: f64 = 0.2;
/// Below this level (dBFS) a step counts as silent (digital silence, no signal).
const SILENT_DBFS: f32 = -80.0;

/// One step of the ladder.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LadderStep {
    /// Start of the step window in seconds.
    pub start_s: f64,
    /// End of the step window in seconds.
    pub end_s: f64,
    /// Seconds of speech found in the window.
    pub speech_s: f32,
    /// Energy-mean level of the speech in dBFS, `None` with under 0.2 s of speech or below -80 dBFS.
    pub level_dbfs: Option<f32>,
    /// Level minus the baseline, in dB; `None` when the step or the baseline has no level.
    pub relative_db: Option<f32>,
}

/// Levels of the steps given as `(start_s, end_s)` windows. The baseline is the level of
/// the first step with speech unless `baseline_dbfs` is given.
pub fn level_ladder(
    samples: &[f32],
    sample_rate: u32,
    steps: &[(f64, f64)],
    baseline_dbfs: Option<f32>,
) -> Result<Vec<LadderStep>, Error> {
    if samples.is_empty() {
        return Err(Error::EmptyInput);
    }
    if sample_rate == 0 {
        return Err(Error::InvalidConfig("sample_rate must be positive"));
    }
    let sr = f64::from(sample_rate);
    let mut out = Vec::with_capacity(steps.len());
    for &(start, end) in steps {
        if start.is_nan() || end.is_nan() || end <= start || start < 0.0 {
            return Err(Error::InvalidConfig("each step needs 0 <= start < end"));
        }
        let a = ((start * sr) as usize).min(samples.len());
        let b = ((end * sr) as usize).min(samples.len());
        let window = &samples[a..b];
        let (mut energy, mut n) = (0.0f64, 0usize);
        if window.len() >= 2 {
            for seg in speech_segments(window, sample_rate, &PauseConfig::default())? {
                let s = (seg.start_s * sr) as usize;
                let e = ((seg.end_s * sr) as usize).min(window.len());
                for &x in &window[s.min(e)..e] {
                    energy += f64::from(x) * f64::from(x);
                    n += 1;
                }
            }
        }
        let speech_s = n as f64 / sr;
        let level = (speech_s >= MIN_SPEECH_S)
            .then(|| (10.0 * (energy / n as f64).max(1e-12).log10()) as f32)
            .filter(|&l| l > SILENT_DBFS);
        out.push(LadderStep {
            start_s: start,
            end_s: end,
            speech_s: speech_s as f32,
            level_dbfs: level,
            relative_db: None,
        });
    }
    let baseline = baseline_dbfs.or_else(|| out.iter().find_map(|s| s.level_dbfs));
    if let Some(base) = baseline {
        for s in &mut out {
            s.relative_db = s.level_dbfs.map(|l| l - base);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 16_000;

    fn tone(amp: f32, secs: f32) -> Vec<f32> {
        (0..(SR as f32 * secs) as usize)
            .map(|i| amp * (std::f32::consts::TAU * 150.0 * i as f32 / SR as f32).sin())
            .collect()
    }

    #[test]
    fn three_steps_are_6_db_apart() {
        let mut x = tone(0.05, 1.0);
        x.extend(tone(0.1, 1.0));
        x.extend(tone(0.2, 1.0));
        let steps = [(0.0, 1.0), (1.0, 2.0), (2.0, 3.0)];
        let l = level_ladder(&x, SR, &steps, None).unwrap();
        assert!((l[0].level_dbfs.unwrap() + 29.03).abs() < 0.3, "{:?}", l[0]);
        assert_eq!(l[0].relative_db, Some(0.0));
        assert!((l[1].relative_db.unwrap() - 6.02).abs() < 0.3, "{:?}", l[1]);
        assert!((l[2].relative_db.unwrap() - 12.04).abs() < 0.3, "{:?}", l[2]);
    }

    #[test]
    fn quiet_step_next_to_loud_one_is_not_silence() {
        // 40 dB apart: a recording-wide pause threshold would drop the quiet step.
        let mut x = tone(0.001, 1.0);
        x.extend(tone(0.1, 1.0));
        let l = level_ladder(&x, SR, &[(0.0, 1.0), (1.0, 2.0)], None).unwrap();
        assert!(l[0].level_dbfs.is_some());
        assert!((l[1].relative_db.unwrap() - 40.0).abs() < 0.5, "{:?}", l[1]);
    }

    #[test]
    fn pauses_inside_a_step_do_not_lower_its_level() {
        let mut x = tone(0.1, 0.6);
        x.extend(vec![0.0; SR as usize / 2]);
        x.extend(tone(0.1, 0.6));
        let l = level_ladder(&x, SR, &[(0.0, 1.7)], None).unwrap();
        assert!((l[0].level_dbfs.unwrap() + 23.0).abs() < 0.5, "{:?}", l[0]);
    }

    #[test]
    fn silent_step_has_no_level_and_given_baseline_is_used() {
        let mut x = vec![0.0; SR as usize];
        x.extend(tone(0.1, 1.0));
        let l = level_ladder(&x, SR, &[(0.0, 1.0), (1.0, 2.0)], Some(-30.0)).unwrap();
        assert_eq!(l[0].level_dbfs, None);
        assert_eq!(l[0].relative_db, None);
        assert!((l[1].relative_db.unwrap() - 7.0).abs() < 0.5, "{:?}", l[1]);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(level_ladder(&[], SR, &[(0.0, 1.0)], None).is_err());
        assert!(level_ladder(&[0.1; 100], SR, &[(1.0, 0.5)], None).is_err());
        assert!(level_ladder(&[0.1; 100], 0, &[(0.0, 1.0)], None).is_err());
    }
}
