//! Level ladder on real speech against Praat (`tests/data/speech_ladder_oracle.json`,
//! produced offline by `tools/oracle_ladder.py`) for the clips in `tests/data/speech/`
//! (source and licence in `PROVENANCE.md`).

use serde_json::Value;
use std::path::PathBuf;
use voice_core::ladder::level_ladder;

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

const GAINS_DB: [f32; 3] = [0.0, 10.0, -8.0];

#[test]
fn real_speech_ladder_against_praat() {
    let r: Value = serde_json::from_str(include_str!("data/speech_ladder_oracle.json")).unwrap();
    let mut errs = vec![];
    let mut missing = 0;
    for (id, c) in r["clips"].as_object().unwrap() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/speech")
            .join(format!("{id}.wav"));
        let mut x = read_wav(&path);
        let third = x.len() / 3;
        let mut steps = vec![];
        for (k, g) in GAINS_DB.iter().enumerate() {
            let gain = 10f32.powf(g / 20.0);
            let (a, b) = (k * third, (k + 1) * third);
            x[a..b].iter_mut().for_each(|s| *s *= gain);
            steps.push((a as f64 / f64::from(SR), b as f64 / f64::from(SR)));
        }
        let ours = level_ladder(&x, SR, &steps, None).unwrap();
        let praat = c["level_dbfs"].as_array().unwrap();
        for (k, (o, p)) in ours.iter().zip(praat).enumerate() {
            println!("{id} step {k}: praat {:?} ours {:?}", p.as_f64(), o.level_dbfs);
            match (o.level_dbfs, p.as_f64()) {
                (Some(o), Some(p)) => errs.push(f64::from(o) - p),
                (None, None) => {}
                _ => missing += 1,
            }
        }
    }
    let n = errs.len() as f64;
    let mean = errs.iter().sum::<f64>() / n;
    let mae = errs.iter().map(|e| e.abs()).sum::<f64>() / n;
    let max = errs.iter().fold(0.0f64, |m, e| m.max(e.abs()));
    println!("ALL: {n} steps | {missing} with a level in only one of the two | mean error {mean:+.2} dB | mean abs error {mae:.2} dB | max {max:.2} dB");
    // Clip 18188256, last step, has a level for Praat but under 0.2 s of speech for us.
    assert!(
        missing <= 1,
        "{missing} steps with a level in only one of the two"
    );
    assert!(mae < 0.3, "mean absolute error {mae} dB");
    assert!(max < 2.5, "worst step {max} dB off");
}
