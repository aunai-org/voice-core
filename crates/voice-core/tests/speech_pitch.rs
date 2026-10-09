//! Pitch on real speech against Praat's tracks (`tests/data/speech_pitch_oracle.json`,
//! produced offline by `tools/oracle_speech.py`) for the clips in `tests/data/speech/`
//! (source and licence in `PROVENANCE.md`). Real speech has no known true f0, so
//! Praat's track is the reference.

use serde_json::Value;
use std::path::PathBuf;
use voice_core::pitch::{AutocorrEstimator, PitchEstimator};

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

fn cents(a: f64, b: f64) -> f64 {
    1200.0 * (a / b).log2()
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

#[derive(Default)]
struct Totals {
    frames: usize,
    agree: usize,
    both_voiced: usize,
    octave: usize,
    octave_up: usize,
    diffs: Vec<f64>,
}

fn compare(id: &str, c: &Value, t: &mut Totals) {
    let col = |k: &str| -> Vec<f64> {
        c[k].as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect()
    };
    let (times, praat) = (col("times_s"), col("praat_hz"));
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/speech")
        .join(format!("{id}.wav"));
    let ours = AutocorrEstimator::default()
        .estimate(&read_wav(&path), SR)
        .unwrap();
    let (mut frames, mut agree, mut both, mut octave, mut up) = (0, 0, 0, 0, 0);
    let mut diffs = vec![];
    for (i, &time) in times.iter().enumerate() {
        let o = ours
            .iter()
            .min_by(|a, b| (a.time_s - time).abs().total_cmp(&(b.time_s - time).abs()))
            .unwrap();
        let our_f0 = o.f0_hz.map(f64::from);
        let pr_f0 = (praat[i] > 0.0).then_some(praat[i]);
        frames += 1;
        match (our_f0, pr_f0) {
            (Some(a), Some(b)) => {
                agree += 1;
                both += 1;
                let d = cents(a, b).abs();
                if d > 300.0 {
                    octave += 1;
                    if a > b {
                        up += 1;
                    }
                }
                diffs.push(d);
            }
            (None, None) => agree += 1,
            _ => {}
        }
    }
    let mut sorted = diffs.clone();
    sorted.sort_by(f64::total_cmp);
    println!(
        "{id}: voicing agreement {:.3} | both voiced {both} | median {:.1} ct, p95 {:.1} ct | off by more than 300 ct: {octave} (ours higher: {up})",
        agree as f64 / frames as f64,
        percentile(&sorted, 0.5),
        percentile(&sorted, 0.95),
    );
    t.frames += frames;
    t.agree += agree;
    t.both_voiced += both;
    t.octave += octave;
    t.octave_up += up;
    t.diffs.extend(diffs);
}

#[test]
fn real_speech_pitch_against_praat() {
    let r: Value = serde_json::from_str(include_str!("data/speech_pitch_oracle.json")).unwrap();
    let mut t = Totals::default();
    for (id, c) in r["clips"].as_object().unwrap() {
        compare(id, c, &mut t);
    }
    t.diffs.sort_by(f64::total_cmp);
    println!(
        "ALL: voicing agreement {:.3} | both voiced {} | median {:.1} ct, p95 {:.1} ct | off by more than 300 ct: {} ({:.1}% of both-voiced, ours higher in {})",
        t.agree as f64 / t.frames as f64,
        t.both_voiced,
        percentile(&t.diffs, 0.5),
        percentile(&t.diffs, 0.95),
        t.octave,
        100.0 * t.octave as f64 / t.both_voiced as f64,
        t.octave_up
    );
    // Targets from docs/spec.md section 7: median f0 within 2% (about 34 cents) on
    // voiced frames and voicing agreement above 90%. Gross errors (more than 300
    // cents, mostly octave jumps) are reported but not yet bounded tightly: the
    // tracker has no cross-frame path search. The loose cap only guards regressions.
    let agree = t.agree as f64 / t.frames as f64;
    assert!(agree > 0.90, "voicing agreement {agree}");
    assert!(
        percentile(&t.diffs, 0.5) < 34.0,
        "median {} ct",
        percentile(&t.diffs, 0.5)
    );
    assert!(
        (t.octave as f64 / t.both_voiced as f64) < 0.15,
        "gross errors {} of {}",
        t.octave,
        t.both_voiced
    );
}
