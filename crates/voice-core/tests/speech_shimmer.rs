//! Local shimmer on real speech against Praat (`tests/data/speech_shimmer_oracle.json`,
//! produced offline by `tools/oracle_shimmer.py`) for the clips in `tests/data/speech/`
//! (source and licence in `PROVENANCE.md`).

use serde_json::Value;
use std::path::PathBuf;
use voice_core::shimmer::local_shimmer;

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
fn real_speech_shimmer_against_praat() {
    let r: Value = serde_json::from_str(include_str!("data/speech_shimmer_oracle.json")).unwrap();
    let mut errs = vec![];
    for (id, c) in r["clips"].as_object().unwrap() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/speech")
            .join(format!("{id}.wav"));
        let ours = local_shimmer(&read_wav(&path), SR).unwrap();
        let praat = c["shimmer_local"].as_f64();
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
    // Praat's shimmer on this compressed speech is 7 to 18%. Ours reads about 2 points
    // lower on average (its amplitudes come from period windows at any phase, Praat's
    // from pulse positions), so the bounds are in percentage points: mean absolute
    // error under 2.5. The worst clip (17848293) is clipped at full scale, which
    // flattens the peaks we measure, so it is allowed up to 8.5.
    assert!(mae < 2.5, "mean absolute error {mae} points");
    assert!(max < 8.5, "worst clip {max} points off");
}
