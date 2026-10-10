//! Speech/silence segmentation on real speech against Praat's silence detection
//! (`tests/data/speech_pauses_oracle.json`, produced offline by
//! `tools/oracle_pauses.py`) for the clips in `tests/data/speech/` (source and
//! licence in `PROVENANCE.md`).

use serde_json::Value;
use std::path::PathBuf;
use voice_core::pauses::{pause_stats, speech_segments, PauseConfig, Segment};

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

fn inside(segs: &[Segment], t: f64) -> bool {
    segs.iter().any(|s| t >= s.start_s && t < s.end_s)
}

#[derive(Default)]
struct Totals {
    grid: usize,
    agree: usize,
    clips: usize,
    count_match: usize,
    boundary_errors: Vec<f64>,
    pause_total_err: Vec<f64>,
}

fn compare(id: &str, c: &Value, t: &mut Totals) {
    let praat: Vec<Segment> = c["sounding"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| Segment {
            start_s: p[0].as_f64().unwrap(),
            end_s: p[1].as_f64().unwrap(),
        })
        .collect();
    let duration = c["duration_s"].as_f64().unwrap();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/speech")
        .join(format!("{id}.wav"));
    let ours = speech_segments(&read_wav(&path), SR, &PauseConfig::default()).unwrap();

    // Agreement on a 10 ms grid.
    let (mut grid, mut agree) = (0, 0);
    let mut time = 0.005;
    while time < duration {
        grid += 1;
        agree += usize::from(inside(&ours, time) == inside(&praat, time));
        time += 0.01;
    }
    let (ps, os) = (pause_stats(&praat), pause_stats(&ours));
    let same_count = ours.len() == praat.len();
    let mut errs = vec![];
    if same_count {
        for (a, b) in ours.iter().zip(&praat) {
            errs.push((a.start_s - b.start_s).abs());
            errs.push((a.end_s - b.end_s).abs());
        }
    }
    println!(
        "{id}: segments ours {} / Praat {} | grid agreement {:.3} | pauses ours {} ({:.2} s) / Praat {} ({:.2} s) | max boundary error {}",
        ours.len(),
        praat.len(),
        agree as f64 / grid as f64,
        os.count,
        os.total_s,
        ps.count,
        ps.total_s,
        errs.iter()
            .copied()
            .reduce(f64::max)
            .map_or("n/a".to_string(), |e| format!("{:.0} ms", e * 1000.0)),
    );
    t.clips += 1;
    t.grid += grid;
    t.agree += agree;
    t.count_match += usize::from(same_count);
    t.boundary_errors.extend(errs);
    t.pause_total_err.push((os.total_s - ps.total_s).abs());
}

#[test]
fn real_speech_pauses_against_praat() {
    let r: Value = serde_json::from_str(include_str!("data/speech_pauses_oracle.json")).unwrap();
    let mut t = Totals::default();
    for (id, c) in r["clips"].as_object().unwrap() {
        compare(id, c, &mut t);
    }
    t.boundary_errors.sort_by(f64::total_cmp);
    let agreement = t.agree as f64 / t.grid as f64;
    let max_boundary = t.boundary_errors.last().copied().unwrap_or(0.0);
    let max_pause_err = t.pause_total_err.iter().copied().fold(0.0, f64::max);
    println!(
        "ALL: grid agreement {agreement:.3} | same segment count in {}/{} clips | max boundary error {:.0} ms over {} boundaries | max total-pause difference {:.0} ms",
        t.count_match,
        t.clips,
        max_boundary * 1000.0,
        t.boundary_errors.len(),
        max_pause_err * 1000.0,
    );
    // Tolerances: the two level curves are computed on different grids (10 ms against
    // Praat's 8 ms), so boundaries may differ by a few frames; pause totals by a
    // fraction of a minimum pause.
    assert!(agreement > 0.97, "grid agreement {agreement}");
    // Known misses: with the noisy clips, two of 20 differ in segment count (one has
    // almost no level contrast, in the other a 0.11 s run sits on the 0.1 s limit).
    assert!(
        t.count_match + 3 > t.clips,
        "segment counts differ from Praat in too many clips"
    );
    assert!(max_boundary < 0.2, "boundary error {max_boundary} s");
    assert!(max_pause_err < 0.2, "total pause difference {max_pause_err} s");
}
