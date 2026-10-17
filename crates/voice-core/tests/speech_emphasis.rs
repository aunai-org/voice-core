//! Emphasis lifts on real speech: against Praat (`tests/data/speech_window_stats_oracle.json`,
//! produced offline by `tools/oracle_window_stats.py`) for the clips in `tests/data/speech/`
//! (source and licence in `PROVENANCE.md`).

use serde_json::Value;
use std::path::PathBuf;
use voice_core::emphasis::{emphasis, MIN_CONTEXT};

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

const WINDOW_S: f64 = 0.4;
const SILENT_DBFS: f64 = -80.0;

fn median_of_others(values: &[Option<f64>], skip: usize) -> Option<f64> {
    let mut v: Vec<f64> = values
        .iter()
        .enumerate()
        .filter(|&(i, _)| i != skip)
        .filter_map(|(_, &x)| x)
        .collect();
    if v.len() < MIN_CONTEXT {
        return None;
    }
    v.sort_by(f64::total_cmp);
    let pos = 0.5 * (v.len() - 1) as f64;
    let (lo, hi) = (pos.floor() as usize, pos.ceil() as usize);
    Some(v[lo] + (v[hi] - v[lo]) * (pos - lo as f64))
}

fn windows(n: usize) -> Vec<(f64, f64)> {
    (0..n)
        .map(|k| (k as f64 * WINDOW_S, (k + 1) as f64 * WINDOW_S))
        .collect()
}

/// Lifts from Praat's window levels and mean f0 (same rule, Praat's numbers) against ours.
#[test]
fn real_speech_lifts_against_praat() {
    let r: Value = serde_json::from_str(include_str!("data/speech_window_stats_oracle.json")).unwrap();
    let (mut lvl, mut pit) = (vec![], vec![]);
    let mut missing = 0;
    for (id, c) in r["clips"].as_object().unwrap() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/speech")
            .join(format!("{id}.wav"));
        let x = read_wav(&path);
        let praat = c["windows"].as_array().unwrap();
        let ours = emphasis(&x, SR, &windows(praat.len())).unwrap();
        let p_level: Vec<Option<f64>> = praat
            .iter()
            .map(|w| w["level_dbfs"].as_f64().filter(|&l| l > SILENT_DBFS))
            .collect();
        let p_f0: Vec<Option<f64>> = praat.iter().map(|w| w["mean_f0_hz"].as_f64()).collect();
        for (i, o) in ours.iter().enumerate() {
            let ref_l = median_of_others(&p_level, i).zip(p_level[i]).map(|(m, l)| l - m);
            let ref_p = median_of_others(&p_f0, i)
                .zip(p_f0[i])
                .map(|(m, p)| 12.0 * (p / m).log2());
            match (o.level_lift_db, ref_l) {
                (Some(a), Some(b)) => lvl.push(f64::from(a) - b),
                (None, None) => {}
                _ => missing += 1,
            }
            match (o.pitch_lift_semitones, ref_p) {
                (Some(a), Some(b)) => pit.push(f64::from(a) - b),
                (None, None) => {}
                _ => missing += 1,
            }
        }
    }
    // (mean abs, median abs, share within tol, max abs)
    let stat = |name: &str, e: &[f64], tol: f64| {
        let n = e.len() as f64;
        let mut abs: Vec<f64> = e.iter().map(|v| v.abs()).collect();
        abs.sort_by(f64::total_cmp);
        let mae = abs.iter().sum::<f64>() / n;
        let median = abs[abs.len() / 2];
        let within = abs.iter().filter(|&&v| v <= tol).count() as f64 / n;
        println!(
            "{name}: {n} words | mean abs {mae:.3} | median abs {median:.3} | within {tol}: {:.1}% | max {:.3}",
            100.0 * within,
            abs[abs.len() - 1]
        );
        (median, within)
    };
    println!("{missing} lifts present for only one of the two");
    let (l_median, l_within) = stat("level lift (dB)", &lvl, 0.5);
    let (p_median, p_within) = stat("pitch lift (semitones)", &pit, 1.0);
    assert!(missing <= 12, "{missing} lifts present for only one");
    assert!(l_median < 0.1 && l_within > 0.9, "level lift");
    assert!(p_median < 0.25 && p_within > 0.8, "pitch lift");
}

/// Known answer on real speech: +6 dB on one word raises its level lift by 6 dB. Its pitch
/// lift should stay put, and does on 16 of 19 clips; on three (17714250, 18188256, 579883)
/// it moves by 1.5, 5.2 and 1.7 semitones because the loudest sample sets the pitch
/// tracker's silence threshold and changes which quiet frames count as voiced. Praat's
/// pitch track, given the same boosted word, moves by 1.12, 5.20 and 1.74 semitones on
/// those same three clips (checked offline).
#[test]
fn six_db_on_a_real_word_adds_six_db_of_lift() {
    let r: Value = serde_json::from_str(include_str!("data/speech_window_stats_oracle.json")).unwrap();
    let gain = 10f32.powf(6.0 / 20.0);
    let (mut tested, mut level_err, mut pitch_moved) = (0, 0.0f64, 0);
    for (id, c) in r["clips"].as_object().unwrap() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/speech")
            .join(format!("{id}.wav"));
        let x = read_wav(&path);
        let wins = windows(c["windows"].as_array().unwrap().len());
        let before = emphasis(&x, SR, &wins).unwrap();
        // First word with both lifts present.
        let Some(k) = before
            .iter()
            .position(|e| e.level_lift_db.is_some() && e.pitch_lift_semitones.is_some())
        else {
            continue;
        };
        let mut y = x.clone();
        let (a, b) = (
            (wins[k].0 * f64::from(SR)) as usize,
            (wins[k].1 * f64::from(SR)) as usize,
        );
        y[a..b.min(x.len())].iter_mut().for_each(|s| *s *= gain);
        let after = emphasis(&y, SR, &wins).unwrap();
        let d_level = f64::from(after[k].level_lift_db.unwrap() - before[k].level_lift_db.unwrap());
        let d_pitch =
            f64::from(after[k].pitch_lift_semitones.unwrap() - before[k].pitch_lift_semitones.unwrap());
        println!("{id} word {k}: level lift +{d_level:.3} dB, pitch lift {d_pitch:+.3} st");
        level_err = level_err.max((d_level - 6.0206).abs());
        pitch_moved += usize::from(d_pitch.abs() > 1.0);
        tested += 1;
    }
    println!("{tested} clips | worst level error {level_err:.4} dB | {pitch_moved} clips with a pitch lift moved by over 1 st");
    assert!(tested >= 15, "{tested} clips tested");
    assert!(level_err < 0.1, "level lift off by {level_err} dB");
    assert!(
        pitch_moved <= 3,
        "{pitch_moved} clips with a pitch lift moved by over 1 st"
    );
}
