//! Local jitter on real speech against Praat (`tests/data/speech_jitter_oracle.json`,
//! produced offline by `tools/oracle_jitter.py`) for the clips in `tests/data/speech/`
//! (source and licence in `PROVENANCE.md`).

use serde_json::Value;
use std::path::PathBuf;
use voice_core::jitter::local_jitter;

const SR: u32 = 16_000;

/// Reads a mono 16-bit PCM WAV file as samples in -1..1.
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

#[test]
fn real_speech_jitter_against_praat() {
    let r: Value = serde_json::from_str(include_str!("data/speech_jitter_oracle.json")).unwrap();
    let mut errs = vec![];
    for (id, c) in r["clips"].as_object().unwrap() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/speech")
            .join(format!("{id}.wav"));
        let ours = local_jitter(&read_wav(&path), SR).unwrap();
        let praat = c["jitter_local"].as_f64();
        println!(
            "{id}: praat {:?} ours {:?}",
            praat.map(|v| (v * 1e4).round() / 1e2),
            ours.map(|j| ((f64::from(j.local) * 1e4).round() / 1e2, j.pairs))
        );
        if let (Some(p), Some(o)) = (praat, ours) {
            errs.push((f64::from(o.local) - p) * 100.0);
        }
    }
    let n = errs.len() as f64;
    let mean = errs.iter().sum::<f64>() / n;
    let mae = errs.iter().map(|e| e.abs()).sum::<f64>() / n;
    let max = errs.iter().fold(0.0f64, |m, e| m.max(e.abs()));
    println!("ALL: {n} clips | mean error {mean:+.2} points | mean abs error {mae:.2} points | max {max:.2} points");
    // Jitter on compressed speech is 1.5 to 4% and a period tracker is noisy, so the
    // bounds are in percentage points of jitter: mean absolute error under 0.4, no
    // clip off by more than 0.8.
    assert!(mae < 0.4, "mean absolute error {mae} points");
    assert!(max < 0.8, "worst clip {max} points off");
}
