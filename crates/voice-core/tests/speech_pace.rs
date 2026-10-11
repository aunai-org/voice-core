//! Syllable nuclei on real speech against a Praat-based reference
//! (`tests/data/speech_pace_oracle.json`, see `tools/oracle_speech_pace.py`), for the
//! clips in `tests/data/speech/`. The reference is a Praat-intensity detector with the
//! same de Jong & Wempe rules, not a human syllable count.

use serde_json::Value;
use std::path::PathBuf;
use voice_core::pace::{syllable_nuclei, PaceConfig};
use voice_core::stream::Analyzer;

const SR: u32 = 16_000;
/// A nucleus matches a reference nucleus within this many seconds.
const MATCH_S: f64 = 0.05;

fn read_wav(path: &PathBuf) -> Vec<f32> {
    let b = std::fs::read(path).unwrap();
    assert_eq!(&b[0..4], b"RIFF");
    let mut pos = 12;
    while pos + 8 <= b.len() {
        let size = u32::from_le_bytes(b[pos + 4..pos + 8].try_into().unwrap()) as usize;
        if &b[pos..pos + 4] == b"data" {
            return b[pos + 8..pos + 8 + size]
                .chunks_exact(2)
                .map(|c| f32::from(i16::from_le_bytes([c[0], c[1]])) / 32768.0)
                .collect();
        }
        pos += 8 + size + (size & 1);
    }
    panic!("no data chunk in {path:?}");
}

/// Number of reference times with an unused detected time within `MATCH_S` (greedy, in order).
fn matched(ours: &[f64], reference: &[f64]) -> usize {
    let mut used = vec![false; ours.len()];
    let mut n = 0;
    for r in reference {
        let best = ours
            .iter()
            .enumerate()
            .filter(|(i, t)| !used[*i] && (**t - r).abs() <= MATCH_S)
            .min_by(|a, b| (a.1 - r).abs().total_cmp(&(b.1 - r).abs()));
        if let Some((i, _)) = best {
            used[i] = true;
            n += 1;
        }
    }
    n
}

#[test]
fn syllable_nuclei_against_praat_based_reference() {
    let r: Value = serde_json::from_str(include_str!("data/speech_pace_oracle.json")).unwrap();
    let (mut ours_n, mut ref_n, mut hit) = (0usize, 0usize, 0usize);
    let mut rate_errors = Vec::new();
    for (id, c) in r["clips"].as_object().unwrap() {
        let reference: Vec<f64> = c["praat_times_s"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/speech")
            .join(format!("{id}.wav"));
        let samples = read_wav(&path);
        let ours = syllable_nuclei(&samples, SR, &PaceConfig::default()).unwrap();
        let m = matched(&ours, &reference);
        let secs = c["duration_s"].as_f64().unwrap();
        let rate_err = 100.0 * (ours.len() as f64 - reference.len() as f64) / reference.len() as f64;
        println!(
            "{id}: ours {}, reference {}, matched {m}, rate {:.2} vs {:.2} per s ({rate_err:+.0}%)",
            ours.len(),
            reference.len(),
            ours.len() as f64 / secs,
            reference.len() as f64 / secs
        );
        rate_errors.push(rate_err.abs());
        ours_n += ours.len();
        ref_n += reference.len();
        hit += m;
    }
    rate_errors.sort_by(f64::total_cmp);
    let median = rate_errors[rate_errors.len() / 2];
    println!(
        "ALL: ours {ours_n}, reference {ref_n}, matched {hit} (recall {:.3}, precision {:.3}); median clip count difference {median:.1}%; total count difference {:+.1}%",
        hit as f64 / ref_n as f64,
        hit as f64 / ours_n as f64,
        100.0 * (ours_n as f64 - ref_n as f64) / ref_n as f64
    );
    // Regression bounds, set just inside the measured values.
    assert!(hit as f64 / ref_n as f64 > 0.98, "recall");
    assert!(hit as f64 / ours_n as f64 > 0.9, "precision");
}

#[test]
fn streaming_nuclei_against_praat_based_reference_and_batch() {
    let r: Value = serde_json::from_str(include_str!("data/speech_pace_oracle.json")).unwrap();
    // Totals: streaming count, reference count, matched with reference, batch count, matched with batch.
    let mut t = [0usize; 5];
    for (id, c) in r["clips"].as_object().unwrap() {
        let reference: Vec<f64> = c["praat_times_s"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/speech")
            .join(format!("{id}.wav"));
        let samples = read_wav(&path);
        let batch = syllable_nuclei(&samples, SR, &PaceConfig::default()).unwrap();
        let mut a = Analyzer::new(SR).unwrap();
        let mut frames = Vec::new();
        // 20 ms chunks, as a microphone callback would deliver them.
        for chunk in samples.chunks(320) {
            frames.extend(a.push(chunk));
        }
        frames.extend(a.finish());
        let live: Vec<f64> = frames
            .iter()
            .filter(|f| f.syllable_nucleus)
            .map(|f| f.time_s)
            .collect();
        let (mr, mb) = (matched(&live, &reference), matched(&live, &batch));
        println!(
            "{id}: streaming {}, batch {}, reference {}, matched reference {mr}, matched batch {mb}",
            live.len(),
            batch.len(),
            reference.len()
        );
        for (acc, v) in t
            .iter_mut()
            .zip([live.len(), reference.len(), mr, batch.len(), mb])
        {
            *acc += v;
        }
    }
    println!(
        "ALL: streaming {}, batch {}, reference {} | vs reference: recall {:.3}, precision {:.3} | vs batch: recall {:.3}, precision {:.3}",
        t[0],
        t[3],
        t[1],
        t[2] as f64 / t[1] as f64,
        t[2] as f64 / t[0] as f64,
        t[4] as f64 / t[3] as f64,
        t[4] as f64 / t[0] as f64
    );
    // Regression bounds, set just inside the measured values.
    assert!(t[2] as f64 / t[1] as f64 > 0.9, "recall against the reference");
    assert!(
        t[2] as f64 / t[0] as f64 > 0.78,
        "precision against the reference"
    );
    assert!(
        t[4] as f64 / t[3] as f64 > 0.9,
        "recall against the batch detector"
    );
}
