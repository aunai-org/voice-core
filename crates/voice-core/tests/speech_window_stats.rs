//! Window statistics on real speech against Praat (`tests/data/speech_window_stats_oracle.json`,
//! produced offline by `tools/oracle_window_stats.py`) for the clips in `tests/data/speech/`
//! (source and licence in `PROVENANCE.md`).

use serde_json::Value;
use std::path::PathBuf;
use voice_core::window_stats::window_stats;

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

fn semitones(a: f64, b: f64) -> f64 {
    12.0 * (a / b).log2()
}

#[test]
fn real_speech_windows_against_praat() {
    let r: Value = serde_json::from_str(include_str!("data/speech_window_stats_oracle.json")).unwrap();
    let (mut lvl, mut f0, mut range, mut rel_l, mut rel_p) = (vec![], vec![], vec![], vec![], vec![]);
    let (mut windows_n, mut pitch_missing) = (0, 0);
    for (id, c) in r["clips"].as_object().unwrap() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/speech")
            .join(format!("{id}.wav"));
        let x = read_wav(&path);
        let praat = c["windows"].as_array().unwrap();
        let wins: Vec<(f64, f64)> = (0..praat.len())
            .map(|k| (k as f64 * WINDOW_S, (k + 1) as f64 * WINDOW_S))
            .collect();
        let ours = window_stats(&x, SR, &wins).unwrap();
        let speech_level = c["speech_level_dbfs"].as_f64().unwrap();
        let median_f0 = c["median_f0_hz"].as_f64().unwrap();
        for (o, p) in ours.iter().zip(praat) {
            windows_n += 1;
            let p_level = p["level_dbfs"].as_f64().unwrap();
            if let Some(l) = o.level_dbfs {
                lvl.push(f64::from(l) - p_level);
                rel_l.push(f64::from(o.relative_level_db.unwrap()) - (p_level - speech_level));
            }
            match (o.mean_f0_hz, p.get("mean_f0_hz").and_then(Value::as_f64)) {
                (Some(m), Some(pm)) => {
                    let (p10, p90) = (p["p10_hz"].as_f64().unwrap(), p["p90_hz"].as_f64().unwrap());
                    f0.push(semitones(f64::from(m), pm));
                    range.push(f64::from(o.f0_range_semitones.unwrap()) - semitones(p90, p10));
                    rel_p.push(f64::from(o.relative_pitch_semitones.unwrap()) - semitones(pm, median_f0));
                }
                (None, None) => {}
                _ => pitch_missing += 1,
            }
        }
    }
    // Returns (mean abs, median abs, share within `tol`, max abs).
    let stat = |name: &str, e: &[f64], tol: f64| {
        let n = e.len() as f64;
        let mean = e.iter().sum::<f64>() / n;
        let mut abs: Vec<f64> = e.iter().map(|v| v.abs()).collect();
        abs.sort_by(f64::total_cmp);
        let mae = abs.iter().sum::<f64>() / n;
        let median = abs[abs.len() / 2];
        let within = abs.iter().filter(|&&v| v <= tol).count() as f64 / n;
        let max = abs[abs.len() - 1];
        println!("{name}: {n} windows | mean {mean:+.3} | mean abs {mae:.3} | median abs {median:.3} | within {tol}: {:.1}% | max {max:.3}", 100.0 * within);
        (mae, median, within, max)
    };
    println!("{windows_n} windows, {pitch_missing} with a pitch in only one of the two");
    // The level of a window is the same energy mean on both sides; it is listed to show the
    // wiring, the relative level adds the difference of the two speech-level baselines.
    let (_, _, _, l_max) = stat("level (dB)", &lvl, 0.01);
    let (_, rl_median, rl_within, _) = stat("relative level (dB)", &rel_l, 0.5);
    let (_, f_median, f_within, _) = stat("mean f0 (semitones)", &f0, 1.0);
    let (_, r_median, _, _) = stat("f0 range (semitones)", &range, 1.0);
    let (_, rp_median, rp_within, _) = stat("relative pitch (semitones)", &rel_p, 1.0);
    // The outliers are windows where one tracker lands an octave from the other.
    assert!(l_max < 0.05, "window level differs by {l_max} dB");
    assert!(
        pitch_missing <= 3,
        "{pitch_missing} windows with a pitch in only one"
    );
    assert!(rl_median < 0.1 && rl_within > 0.9, "relative level");
    assert!(
        f_median < 0.25 && f_within > 0.85,
        "mean f0: median {f_median}, within 1 st {f_within}"
    );
    assert!(r_median < 0.5, "f0 range median {r_median}");
    assert!(rp_median < 0.25 && rp_within > 0.85, "relative pitch");
}
