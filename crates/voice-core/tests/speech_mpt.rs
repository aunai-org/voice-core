//! Maximum phonation time on real speech against the longest voiced stretch of Praat's
//! pitch track (`tests/data/speech_mpt_oracle.json`, produced offline by
//! `tools/oracle_mpt.py`) for the clips in `tests/data/speech/`
//! (source and licence in `PROVENANCE.md`).

use serde_json::Value;
use std::path::PathBuf;
use voice_core::phonation::max_phonation;

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
fn real_speech_phonation_against_praat() {
    let r: Value = serde_json::from_str(include_str!("data/speech_mpt_oracle.json")).unwrap();
    let (mut dur, mut f0, mut sd) = (vec![], vec![], vec![]);
    for (id, c) in r["clips"].as_object().unwrap() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/speech")
            .join(format!("{id}.wav"));
        let ours = max_phonation(&read_wav(&path), SR).unwrap();
        println!(
            "{id}: praat {:.2} s at {:.2} | ours {:?}",
            c["duration_s"].as_f64().unwrap_or(f64::NAN),
            c["start_s"].as_f64().unwrap_or(f64::NAN),
            ours.map(|p| (p.duration_s, p.start_s, p.mean_f0_hz, p.f0_sd_semitones))
        );
        let (Some(p), Some(o)) = (c.as_object(), ours) else {
            panic!("{id}: Praat {c:?} but ours {ours:?}");
        };
        dur.push(f64::from(o.duration_s) - p["duration_s"].as_f64().unwrap());
        f0.push(1200.0 * (f64::from(o.mean_f0_hz) / p["mean_f0_hz"].as_f64().unwrap()).log2() / 100.0);
        sd.push(f64::from(o.f0_sd_semitones) - p["f0_sd_semitones"].as_f64().unwrap());
    }
    let stats = |v: &[f64]| {
        let n = v.len() as f64;
        (
            v.iter().sum::<f64>() / n,
            v.iter().map(|e| e.abs()).sum::<f64>() / n,
            v.iter().fold(0.0f64, |m, e| m.max(e.abs())),
        )
    };
    let (dm, dmae, dmax) = stats(&dur);
    let (fm, fmae, fmax) = stats(&f0);
    let (sm, smae, smax) = stats(&sd);
    println!("DURATION s: mean {dm:+.3} mae {dmae:.3} max {dmax:.3}");
    println!("MEAN F0 semitones: mean {fm:+.3} mae {fmae:.3} max {fmax:.3}");
    println!("F0 SD semitones: mean {sm:+.3} mae {smae:.3} max {smax:.3}");
    let median = |v: &[f64]| {
        let mut a: Vec<f64> = v.iter().map(|e| e.abs()).collect();
        a.sort_by(f64::total_cmp);
        a[a.len() / 2]
    };
    let close = dur.iter().filter(|e| e.abs() <= 0.02).count();
    println!(
        "{close} of {} clips within 0.02 s; medians: duration {:.3} s, f0 {:.3} st, sd {:.3} st",
        dur.len(),
        median(&dur),
        median(&f0),
        median(&sd)
    );
    // Our track and Praat's agree on the stretch for most clips; the others pick another
    // stretch or bridge more (clip 665631, 17848293, 35061606, 579883 are documented in the PR).
    assert!(close >= 15, "only {close} clips within 0.02 s");
    assert!(
        median(&f0) < 0.1,
        "median mean-f0 error {} semitones",
        median(&f0)
    );
    assert!(
        median(&sd) < 0.1,
        "median f0-spread error {} semitones",
        median(&sd)
    );
    assert!(dmax < 1.5, "worst duration {dmax} s off");
}
