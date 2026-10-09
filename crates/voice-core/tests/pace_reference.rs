//! Syllable-nucleus detection against the Praat-based reference
//! (`tests/data/pace_oracle.json`, produced offline by `tools/oracle_pace.py`):
//! Praat's intensity and pitch plus the de Jong & Wempe (2009) peak rules written
//! independently in Python. The clips are raised-cosine tone bursts rebuilt here
//! from the same formulas.

use serde_json::Value;
use std::f64::consts::TAU;
use voice_core::pace::{syllable_nuclei, PaceConfig};

const SR: u32 = 16_000;

/// Clip name, burst rate in Hz, burst segments in seconds, total length in seconds.
type Spec = (&'static str, f64, Vec<(f64, f64)>, f64);

fn clip(rate: f64, segments: &[(f64, f64)], secs: f64) -> Vec<f32> {
    (0..(f64::from(SR) * secs) as usize)
        .map(|i| {
            let t = i as f64 / f64::from(SR);
            let env = segments
                .iter()
                .find(|(a, b)| t >= *a && t < *b)
                .map_or(0.0, |(a, _)| 0.5 * (1.0 - (TAU * rate * (t - a)).cos()));
            (0.5 * env * (TAU * 150.0 * t).sin()) as f32
        })
        .collect()
}

/// Count must equal both the true count and the Praat-based count; each nucleus
/// time must be within 30 ms of the matching Praat-based time (the two intensity
/// windows differ in shape, so a few ms of offset is expected).
#[test]
fn pace_matches_praat_based_reference() {
    let r: Value = serde_json::from_str(include_str!("data/pace_oracle.json")).unwrap();
    let specs: [Spec; 5] = [
        ("bursts_3hz", 3.0, vec![(0.0, 3.0)], 3.0),
        ("bursts_4hz", 4.0, vec![(0.0, 3.0)], 3.0),
        ("bursts_5hz", 5.0, vec![(0.0, 3.0)], 3.0),
        ("bursts_6hz", 6.0, vec![(0.0, 3.0)], 3.0),
        ("bursts_pause", 4.0, vec![(0.0, 1.0), (2.5, 3.5)], 3.5),
    ];
    for (name, rate, segs, secs) in specs {
        let c = &r["clips"][name];
        let truth = c["true_count"].as_u64().unwrap() as usize;
        let praat_count = c["praat_count"].as_u64().unwrap() as usize;
        let praat: Vec<f64> = c["praat_times_s"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        let ours = syllable_nuclei(&clip(rate, &segs, secs), SR, &PaceConfig::default()).unwrap();
        let worst = ours
            .iter()
            .zip(&praat)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f64::max);
        println!(
            "{name}: true {truth}, praat-based {praat_count}, ours {}, worst time offset {:.1} ms",
            ours.len(),
            worst * 1000.0
        );
        assert_eq!(ours.len(), truth, "{name}: count vs truth");
        assert_eq!(ours.len(), praat_count, "{name}: count vs Praat-based");
        assert!(worst < 0.03, "{name}: worst nucleus offset {worst} s");
    }
}
