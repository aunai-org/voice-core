//! Frame-by-frame pitch comparison on moving-pitch synthetic signals against
//! Praat's tracks (`tests/data/pitch_tracks_oracle.json`, produced offline by
//! `tools/oracle_tracks.py`). The signals are rebuilt here from the same
//! closed-form phase formulas the script uses.

use serde_json::Value;
use std::f64::consts::TAU;
use voice_core::pitch::{AutocorrEstimator, PitchEstimator};

const SR: u32 = 16_000;
const SECS: f64 = 2.0;

fn signal(name: &str) -> Vec<f32> {
    let n = (f64::from(SR) * SECS) as usize;
    (0..n)
        .map(|i| {
            let t = i as f64 / f64::from(SR);
            let v = match name {
                "glide" => 0.5 * (TAU * (100.0 * t + 25.0 * t * t)).sin(),
                "vibrato" => 0.5 * (TAU * 150.0 * t - 1.5 * (TAU * 5.0 * t).cos()).sin(),
                "harmonic_glide" => {
                    let ph = TAU * (120.0 * t + 30.0 * t * t);
                    0.3 * (1..=5)
                        .map(|k| (f64::from(k) * ph).sin() / f64::from(k))
                        .sum::<f64>()
                }
                _ => unreachable!(),
            };
            v as f32
        })
        .collect()
}

fn cents(a: f64, b: f64) -> f64 {
    1200.0 * (a / b).log2()
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

struct Stats {
    agree: f64,
    median_vs_praat: f64,
    p95_vs_praat: f64,
    median_ours_vs_true: f64,
    median_praat_vs_true: f64,
}

fn compare(name: &str) -> Stats {
    let r: Value = serde_json::from_str(include_str!("data/pitch_tracks_oracle.json")).unwrap();
    let c = &r["clips"][name];
    let col = |k: &str| -> Vec<f64> {
        c[k].as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect()
    };
    let (times, praat, truth) = (col("times_s"), col("praat_hz"), col("true_hz"));
    let ours = AutocorrEstimator::default().estimate(&signal(name), SR).unwrap();

    let mut agree_frames = 0usize;
    let (mut d_praat, mut d_ours_true, mut d_praat_true) = (vec![], vec![], vec![]);
    for (i, &t) in times.iter().enumerate() {
        // nearest of our frames in time
        let o = ours
            .iter()
            .min_by(|a, b| (a.time_s - t).abs().total_cmp(&(b.time_s - t).abs()))
            .unwrap();
        let our_f0 = o.f0_hz.map(f64::from);
        let pr_f0 = (praat[i] > 0.0).then_some(praat[i]);
        match (our_f0, pr_f0) {
            (Some(a), Some(b)) => {
                agree_frames += 1;
                d_praat.push(cents(a, b).abs());
                d_ours_true.push(cents(a, truth[i]).abs());
                d_praat_true.push(cents(b, truth[i]).abs());
            }
            (None, None) => agree_frames += 1,
            _ => {}
        }
    }
    for v in [&mut d_praat, &mut d_ours_true, &mut d_praat_true] {
        v.sort_by(f64::total_cmp);
    }
    Stats {
        agree: agree_frames as f64 / times.len() as f64,
        median_vs_praat: percentile(&d_praat, 0.5),
        p95_vs_praat: percentile(&d_praat, 0.95),
        median_ours_vs_true: percentile(&d_ours_true, 0.5),
        median_praat_vs_true: percentile(&d_praat_true, 0.5),
    }
}

/// Tolerances: voicing agreement at least 0.97 of frames, median f0 difference
/// from Praat under 1 cent, 95th percentile under 5 cents (frames voiced in both).
#[test]
fn moving_pitch_tracks_match_praat() {
    for name in ["glide", "vibrato", "harmonic_glide"] {
        let s = compare(name);
        println!(
            "{name}: voicing agreement {:.3} | ours vs praat median {:.2} ct, p95 {:.2} ct | vs true: ours {:.2} ct, praat {:.2} ct",
            s.agree, s.median_vs_praat, s.p95_vs_praat, s.median_ours_vs_true, s.median_praat_vs_true
        );
        assert!(s.agree >= 0.97, "{name}: voicing agreement {}", s.agree);
        assert!(s.median_vs_praat < 1.0, "{name}: median {} ct", s.median_vs_praat);
        assert!(s.p95_vs_praat < 5.0, "{name}: p95 {} ct", s.p95_vs_praat);
    }
}
