//! Emphasis on real speech against Praat (`tests/data/speech_emphasis_oracle.json`, produced
//! offline by `tools/oracle_emphasis.py`) for the clips in `tests/data/speech/` (source and
//! licence in `PROVENANCE.md`).

use serde_json::Value;
use std::path::PathBuf;
use voice_core::emphasis::emphasis;

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

/// (mean abs, median abs, share within `tol`, max abs) of a list of errors, printed.
fn stat(name: &str, e: &[f64], tol: f64) -> (f64, f64, f64, f64) {
    let n = e.len() as f64;
    let mean = e.iter().sum::<f64>() / n;
    let mut abs: Vec<f64> = e.iter().map(|v| v.abs()).collect();
    abs.sort_by(f64::total_cmp);
    let mae = abs.iter().sum::<f64>() / n;
    let median = abs[abs.len() / 2];
    let within = abs.iter().filter(|&&v| v <= tol).count() as f64 / n;
    let max = abs[abs.len() - 1];
    println!(
        "{name}: {n} values | mean {mean:+.3} | mean abs {mae:.3} | median abs {median:.3} | within {tol}: {:.1}% | max {max:.3}",
        100.0 * within
    );
    (mae, median, within, max)
}

#[test]
fn real_speech_emphasis_against_praat() {
    let r: Value = serde_json::from_str(include_str!("data/speech_emphasis_oracle.json")).unwrap();
    let (mut band_l, mut band_p) = (vec![], vec![]);
    let (mut rel_l, mut rel_p, mut pb, mut pa) = (vec![], vec![], vec![], vec![]);
    let (mut bands_ours_only, mut bands_praat_only, mut bands_both) = (0, 0, 0);
    let mut pitch_missing = 0;
    for (id, c) in r["clips"].as_object().unwrap() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/speech")
            .join(format!("{id}.wav"));
        let x = read_wav(&path);
        let praat = c["words"].as_array().unwrap();
        let wins: Vec<(f64, f64)> = praat
            .iter()
            .map(|w| (w["start_s"].as_f64().unwrap(), w["end_s"].as_f64().unwrap()))
            .collect();
        let ours = emphasis(&x, SR, &wins).unwrap();
        for (key, o) in [
            ("typical_level_db", ours.typical.level_db),
            ("typical_pitch_st", ours.typical.pitch_semitones),
        ] {
            match (o, c[key].as_array()) {
                (Some(b), Some(p)) => {
                    bands_both += 1;
                    let v = [b.low, b.median, b.high].map(f64::from);
                    let sink = if key == "typical_level_db" {
                        &mut band_l
                    } else {
                        &mut band_p
                    };
                    for (a, q) in v.iter().zip(p) {
                        sink.push(a - q.as_f64().unwrap());
                    }
                }
                (Some(_), None) => bands_ours_only += 1,
                (None, Some(_)) => bands_praat_only += 1,
                (None, None) => {}
            }
        }
        for (o, p) in ours.words.iter().zip(praat) {
            if let Some(l) = o.relative_level_db {
                rel_l.push(f64::from(l) - p["relative_level_db"].as_f64().unwrap());
            }
            match (o.relative_pitch_semitones, p["relative_pitch_st"].as_f64()) {
                (Some(a), Some(b)) => rel_p.push(f64::from(a) - b),
                (None, None) => {}
                _ => pitch_missing += 1,
            }
            pb.push(f64::from(o.pause_before_s) - p["pause_before_s"].as_f64().unwrap());
            pa.push(f64::from(o.pause_after_s) - p["pause_after_s"].as_f64().unwrap());
        }
    }
    println!(
        "bands: {bands_both} in both, {bands_ours_only} only ours, {bands_praat_only} only Praat; {pitch_missing} words with a pitch in only one"
    );
    let (_, bl_median, bl_within, _) = stat("band edges, level (dB)", &band_l, 1.0);
    let (_, bp_median, bp_within, _) = stat("band edges, pitch (semitones)", &band_p, 1.0);
    let (_, rl_median, rl_within, _) = stat("word relative level (dB)", &rel_l, 0.5);
    let (_, rp_median, rp_within, _) = stat("word relative pitch (semitones)", &rel_p, 1.0);
    let (_, _, pb_within, _) = stat("pause before (s)", &pb, 0.05);
    let (_, _, pa_within, _) = stat("pause after (s)", &pa, 0.05);
    assert!(bands_ours_only + bands_praat_only <= 4, "bands only on one side");
    assert!(
        pitch_missing <= 3,
        "{pitch_missing} words with a pitch in only one"
    );
    assert!(
        bl_median < 0.5 && bl_within > 0.8,
        "level band: median {bl_median}, within {bl_within}"
    );
    assert!(
        bp_median < 0.5 && bp_within > 0.7,
        "pitch band: median {bp_median}, within {bp_within}"
    );
    assert!(rl_median < 0.1 && rl_within > 0.9, "word level");
    assert!(rp_median < 0.25 && rp_within > 0.85, "word pitch");
    assert!(
        pb_within > 0.85 && pa_within > 0.85,
        "pauses: {pb_within}, {pa_within}"
    );
}
