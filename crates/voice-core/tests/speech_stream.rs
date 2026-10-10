//! Streaming speech/silence flags on real speech against Praat's silence detection
//! (`tests/data/speech_pauses_oracle.json`, see `tools/oracle_pauses.py`) and against
//! the batch `pauses` segmentation, for the clips in `tests/data/speech/`.

use serde_json::Value;
use std::path::PathBuf;
use voice_core::pauses::{speech_segments, PauseConfig, Segment};
use voice_core::stream::Analyzer;

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

fn stream(samples: &[f32], reference: Option<f32>) -> Vec<voice_core::stream::FrameInfo> {
    let mut a = Analyzer::new(SR).unwrap();
    if let Some(r) = reference {
        a = a.with_reference_level(r);
    }
    let mut frames = Vec::new();
    // 20 ms chunks, as a microphone callback would deliver them.
    for chunk in samples.chunks(320) {
        frames.extend(a.push(chunk));
    }
    frames.extend(a.finish());
    frames
}

#[test]
fn streaming_flags_against_praat_and_batch() {
    let r: Value = serde_json::from_str(include_str!("data/speech_pauses_oracle.json")).unwrap();
    // Totals: frames, then agreement with Praat / batch for the plain stream and for
    // a stream seeded with the loudest level of the same clip.
    let mut t = [0usize; 5];
    for (id, c) in r["clips"].as_object().unwrap() {
        let praat: Vec<Segment> = c["sounding"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| Segment {
                start_s: p[0].as_f64().unwrap(),
                end_s: p[1].as_f64().unwrap(),
            })
            .collect();
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/speech")
            .join(format!("{id}.wav"));
        let samples = read_wav(&path);
        let batch = speech_segments(&samples, SR, &PauseConfig::default()).unwrap();
        let plain = stream(&samples, None);
        let loudest = plain.iter().map(|f| f.level_dbfs).fold(f32::MIN, f32::max);
        let seeded = stream(&samples, Some(loudest));
        let score = |frames: &[voice_core::stream::FrameInfo], to: &[Segment]| {
            frames.iter().filter(|f| f.speech == inside(to, f.time_s)).count()
        };
        let n = plain.len();
        let (pp, pb, sp, sb) = (
            score(&plain, &praat),
            score(&plain, &batch),
            score(&seeded, &praat),
            score(&seeded, &batch),
        );
        println!(
            "{id}: {n} frames | plain: Praat {:.3}, batch {:.3} | seeded with the loudest level: Praat {:.3}, batch {:.3}",
            pp as f64 / n as f64,
            pb as f64 / n as f64,
            sp as f64 / n as f64,
            sb as f64 / n as f64
        );
        for (acc, v) in t.iter_mut().zip([n, pp, pb, sp, sb]) {
            *acc += v;
        }
    }
    let f = |v: usize| v as f64 / t[0] as f64;
    println!(
        "ALL: {} frames | plain: Praat {:.3}, batch {:.3} | seeded: Praat {:.3}, batch {:.3}",
        t[0],
        f(t[1]),
        f(t[2]),
        f(t[3]),
        f(t[4])
    );
    // Frame-by-frame decisions with no minimum run lengths cannot match the batch
    // segmentation exactly; these bounds guard against regressions.
    assert!(f(t[1]) > 0.85, "plain agreement with Praat {}", f(t[1]));
    assert!(f(t[3]) > 0.93, "seeded agreement with Praat {}", f(t[3]));
}

#[test]
fn streaming_pitch_against_praat_and_batch() {
    use voice_core::pitch::{AutocorrEstimator, PitchEstimator};
    let r: Value = serde_json::from_str(include_str!("data/speech_pitch_oracle.json")).unwrap();
    let cents = |a: f64, b: f64| 1200.0 * (a / b).log2();
    // Frames, voicing agreement with Praat / batch, voiced in both with Praat,
    // more than 300 ct away, and the cent differences.
    let (mut frames, mut agree_p, mut agree_b, mut both, mut gross) =
        (0usize, 0usize, 0usize, 0usize, 0usize);
    let mut diffs: Vec<f64> = vec![];
    for (id, c) in r["clips"].as_object().unwrap() {
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
        let samples = read_wav(&path);
        let live = stream(&samples, None);
        let batch = AutocorrEstimator::default().estimate(&samples, SR).unwrap();
        for (i, &time) in times.iter().enumerate() {
            let nearest = |t: f64, times_of: &dyn Fn(usize) -> f64, n: usize| {
                (0..n)
                    .min_by(|&a, &b| (times_of(a) - t).abs().total_cmp(&(times_of(b) - t).abs()))
                    .unwrap()
            };
            let k = nearest(time, &|j| live[j].time_s, live.len());
            let m = nearest(time, &|j| batch[j].time_s, batch.len());
            let ours = live[k].f0_hz.map(f64::from);
            let reference = (praat[i] > 0.0).then_some(praat[i]);
            frames += 1;
            agree_p += usize::from(ours.is_some() == reference.is_some());
            agree_b += usize::from(ours.is_some() == batch[m].f0_hz.is_some());
            if let (Some(a), Some(b)) = (ours, reference) {
                both += 1;
                let d = cents(a, b).abs();
                diffs.push(d);
                gross += usize::from(d > 300.0);
            }
        }
    }
    diffs.sort_by(f64::total_cmp);
    let median = diffs[diffs.len() / 2];
    println!(
        "ALL: {frames} frames | voicing agreement with Praat {:.3}, with the batch track {:.3} | both voiced {both}, median {median:.1} ct, off by more than 300 ct: {gross} ({:.1}%)",
        agree_p as f64 / frames as f64,
        agree_b as f64 / frames as f64,
        100.0 * gross as f64 / both as f64
    );
    assert!(
        agree_p as f64 / frames as f64 > 0.94,
        "voicing agreement with Praat"
    );
    assert!(median < 3.5, "median {median} ct");
    assert!(
        (gross as f64) / (both as f64) < 0.025,
        "gross errors {gross} of {both}"
    );
}
