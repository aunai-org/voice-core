//! Hesitation events on real speech against Praat (`tests/data/speech_hesitations_oracle.json`,
//! produced offline by `tools/oracle_hesitations.py`) for the clips in `tests/data/speech/`
//! (source and licence in `PROVENANCE.md`).

use serde_json::Value;
use std::path::PathBuf;
use voice_core::hesitations::{hesitations, HesitationConfig, HesitationKind};

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

/// An event of ours and one of Praat's are the same when they have the same kind and
/// their starts and ends are within this many seconds.
const MATCH_S: f64 = 0.15;

#[test]
fn real_speech_hesitations_against_praat() {
    let r: Value = serde_json::from_str(include_str!("data/speech_hesitations_oracle.json")).unwrap();
    let (mut matched, mut ours_only, mut praat_only) = (0, 0, 0);
    for (id, c) in r["clips"].as_object().unwrap() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/speech")
            .join(format!("{id}.wav"));
        let ours = hesitations(&read_wav(&path), SR, &HesitationConfig::default()).unwrap();
        let mut praat: Vec<(String, f64, f64)> = c["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                (
                    e[0].as_str().unwrap().to_string(),
                    e[1].as_f64().unwrap(),
                    e[2].as_f64().unwrap(),
                )
            })
            .collect();
        for o in &ours {
            let kind = match o.kind {
                HesitationKind::Pause => "pause",
                HesitationKind::Held => "held",
            };
            println!("{id}: ours {kind} {:.2}-{:.2}", o.start_s, o.end_s);
            if let Some(i) = praat.iter().position(|p| {
                p.0 == kind && (p.1 - o.start_s).abs() <= MATCH_S && (p.2 - o.end_s).abs() <= MATCH_S
            }) {
                praat.remove(i);
                matched += 1;
            } else {
                ours_only += 1;
            }
        }
        for p in &praat {
            println!("{id}: praat only {} {:.2}-{:.2}", p.0, p.1, p.2);
        }
        praat_only += praat.len();
    }
    println!("{matched} events in both | {ours_only} ours only | {praat_only} Praat only");
    // Every pause is found by both; the held sounds differ on two stretches (clips
    // 17848293 and 579883), where the pitch trackers disagree about the voiced stretch.
    assert!(matched >= 6, "{matched} events in both");
    assert!(
        ours_only + praat_only <= 2,
        "{ours_only} ours only, {praat_only} Praat only"
    );
}
