//! Window statistics: level and pitch of given time windows (for example the words of a
//! script, from word timestamps), also relative to the rest of the recording, so a
//! caller can tell how much a word stood out.
//!
//! Method: for each window `(start_s, end_s)` the level is the energy mean,
//! `10 log10(mean(x^2))`, of its samples, and the pitch is taken from the voiced frames of
//! the pitch track ([`crate::pitch`]) whose centre lies in the window: mean f0 in Hz and
//! the 10th-to-90th percentile range in semitones (the percentiles ignore the odd octave
//! error). The recording's own baseline is the energy-mean level of its speech segments
//! ([`crate::pauses`]) and the median f0 of its voiced frames; `relative_level_db` and
//! `relative_pitch_semitones` are the window's distance from that baseline. Phones are
//! uncalibrated, so only the relative numbers are comparable between recordings.

use crate::pauses::{speech_segments, PauseConfig};
use crate::pitch::{median_f0, AutocorrEstimator, PitchEstimator, PitchFrame};
use crate::Error;

/// Fewest voiced frames (10 ms each) a window needs to have a pitch.
const MIN_VOICED_FRAMES: usize = 3;
/// Shortest window (in seconds) that has a level.
const MIN_WINDOW_S: f64 = 0.03;
/// Below this level (dBFS) a window counts as silent.
const SILENT_DBFS: f32 = -80.0;

/// Statistics of one window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowStats {
    /// Start of the window in seconds.
    pub start_s: f64,
    /// End of the window in seconds.
    pub end_s: f64,
    /// Energy-mean level of the window in dBFS, `None` for a window under 30 ms or below -80 dBFS.
    pub level_dbfs: Option<f32>,
    /// Level minus the recording's speech level, in dB.
    pub relative_level_db: Option<f32>,
    /// Number of voiced pitch frames in the window.
    pub voiced_frames: u32,
    /// Mean f0 of the voiced frames in Hz, `None` with fewer than 3 voiced frames.
    pub mean_f0_hz: Option<f32>,
    /// 10th-to-90th percentile range of f0 in semitones, same condition as `mean_f0_hz`.
    pub f0_range_semitones: Option<f32>,
    /// Mean f0 against the recording's median f0, in semitones.
    pub relative_pitch_semitones: Option<f32>,
}

/// Statistics of the windows given as `(start_s, end_s)`, in the order given.
pub fn window_stats(
    samples: &[f32],
    sample_rate: u32,
    windows: &[(f64, f64)],
) -> Result<Vec<WindowStats>, Error> {
    let track = AutocorrEstimator::default().estimate(samples, sample_rate)?;
    window_stats_with_track(samples, sample_rate, windows, &track)
}

/// Like [`window_stats`], reusing a pitch track already computed for `samples`.
pub fn window_stats_with_track(
    samples: &[f32],
    sample_rate: u32,
    windows: &[(f64, f64)],
    track: &[PitchFrame],
) -> Result<Vec<WindowStats>, Error> {
    if samples.is_empty() {
        return Err(Error::EmptyInput);
    }
    if sample_rate == 0 {
        return Err(Error::InvalidConfig("sample_rate must be positive"));
    }
    let sr = f64::from(sample_rate);
    let speech_level = speech_level_dbfs(samples, sample_rate)?;
    let median = median_f0(track);
    let mut out = Vec::with_capacity(windows.len());
    for &(start, end) in windows {
        if start.is_nan() || end.is_nan() || end <= start || start < 0.0 {
            return Err(Error::InvalidConfig("each window needs 0 <= start < end"));
        }
        let a = ((start * sr) as usize).min(samples.len());
        let b = ((end * sr) as usize).min(samples.len());
        let level = (((b - a) as f64 / sr) >= MIN_WINDOW_S)
            .then(|| energy_db(&samples[a..b]))
            .filter(|&l| l > SILENT_DBFS);
        let mut hz: Vec<f64> = track
            .iter()
            .filter(|f| f.time_s >= start && f.time_s < end)
            .filter_map(|f| f.f0_hz.map(f64::from))
            .collect();
        let voiced_frames = hz.len() as u32;
        let (mean_f0_hz, f0_range, rel_pitch) = if hz.len() >= MIN_VOICED_FRAMES {
            let mean = hz.iter().sum::<f64>() / hz.len() as f64;
            hz.sort_by(f64::total_cmp);
            let range = 12.0 * (percentile(&hz, 0.9) / percentile(&hz, 0.1)).log2();
            let rel = median.map(|m| 12.0 * (mean / f64::from(m)).log2());
            (Some(mean as f32), Some(range as f32), rel.map(|r| r as f32))
        } else {
            (None, None, None)
        };
        out.push(WindowStats {
            start_s: start,
            end_s: end,
            level_dbfs: level,
            relative_level_db: level.zip(speech_level).map(|(l, s)| l - s),
            voiced_frames,
            mean_f0_hz,
            f0_range_semitones: f0_range,
            relative_pitch_semitones: rel_pitch,
        });
    }
    Ok(out)
}

/// Energy-mean level of the speech segments of a recording in dBFS.
fn speech_level_dbfs(samples: &[f32], sample_rate: u32) -> Result<Option<f32>, Error> {
    let sr = f64::from(sample_rate);
    let (mut energy, mut n) = (0.0f64, 0usize);
    for seg in speech_segments(samples, sample_rate, &PauseConfig::default())? {
        let s = (seg.start_s * sr) as usize;
        let e = ((seg.end_s * sr) as usize).min(samples.len());
        for &x in &samples[s.min(e)..e] {
            energy += f64::from(x) * f64::from(x);
            n += 1;
        }
    }
    Ok((n > 0)
        .then(|| (10.0 * (energy / n as f64).max(1e-12).log10()) as f32)
        .filter(|&l| l > SILENT_DBFS))
}

fn energy_db(x: &[f32]) -> f32 {
    let e: f64 = x.iter().map(|&v| f64::from(v) * f64::from(v)).sum();
    (10.0 * (e / x.len() as f64).max(1e-12).log10()) as f32
}

/// Percentile `q` (0..1) of sorted values, linear interpolation between neighbours.
pub(crate) fn percentile(sorted: &[f64], q: f64) -> f64 {
    let pos = q * (sorted.len() - 1) as f64;
    let (lo, hi) = (pos.floor() as usize, pos.ceil() as usize);
    sorted[lo] + (sorted[hi] - sorted[lo]) * (pos - lo as f64)
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

    /// Three 0.5 s words: plain, louder and higher (200 Hz, a pure tone at 300 Hz would read an octave low), plain.
    fn three_words() -> Vec<f32> {
        let mut x = tone(150.0, 0.1, 0.5);
        x.extend(tone(200.0, 0.2, 0.5));
        x.extend(tone(150.0, 0.1, 0.5));
        x
    }

    #[test]
    fn middle_word_stands_out() {
        let w = [(0.0, 0.5), (0.5, 1.0), (1.0, 1.5)];
        let s = window_stats(&three_words(), SR, &w).unwrap();
        assert!((s[0].level_dbfs.unwrap() + 23.0).abs() < 0.3, "{:?}", s[0]);
        assert!((s[1].level_dbfs.unwrap() - s[0].level_dbfs.unwrap() - 6.02).abs() < 0.3);
        assert!((s[1].mean_f0_hz.unwrap() - 200.0).abs() < 2.0, "{:?}", s[1]);
        // The whole take is 2/3 plain: the loud, high word is above it, the plain ones below or level.
        assert!(s[1].relative_level_db.unwrap() > 2.0, "{:?}", s[1]);
        assert!(s[1].relative_pitch_semitones.unwrap() > 4.0, "{:?}", s[1]);
        assert!(s[0].relative_pitch_semitones.unwrap().abs() < 0.5, "{:?}", s[0]);
        assert!(s[0].f0_range_semitones.unwrap() < 0.5);
    }

    #[test]
    fn silence_window_has_no_level_or_pitch() {
        let mut x = tone(150.0, 0.1, 0.5);
        x.extend(vec![0.0; SR as usize / 2]);
        let s = window_stats(&x, SR, &[(0.6, 1.0)]).unwrap();
        assert_eq!(s[0].level_dbfs, None);
        assert_eq!(s[0].mean_f0_hz, None);
        assert_eq!(s[0].relative_level_db, None);
        assert_eq!(s[0].voiced_frames, 0);
    }

    #[test]
    fn window_past_the_end_and_bad_input() {
        let x = tone(150.0, 0.1, 0.5);
        let s = window_stats(&x, SR, &[(0.4, 2.0)]).unwrap();
        assert!(s[0].level_dbfs.is_some());
        assert!(window_stats(&x, SR, &[(0.5, 0.5)]).is_err());
        assert!(window_stats(&x, SR, &[(-1.0, 0.5)]).is_err());
        assert!(window_stats(&[], SR, &[(0.0, 0.5)]).is_err());
        assert!(window_stats(&x, 0, &[(0.0, 0.5)]).is_err());
    }

    #[test]
    fn percentile_interpolates() {
        let v = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(percentile(&v, 0.5), 3.0);
        assert!((percentile(&v, 0.1) - 1.4).abs() < 1e-9);
    }
}
