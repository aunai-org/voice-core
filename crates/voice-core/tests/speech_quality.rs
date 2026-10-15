//! Speech-to-noise ratio and the `Noisy` warning on the real clips in
//! `tests/data/speech/`. There is no Praat reference for this number (Praat has no
//! equivalent measure of recording noise), so this test only pins the values the
//! current method gives, to catch accidental changes.

use std::path::PathBuf;
use voice_core::quality::{check, snr_db, Warning};

const SR: u32 = 16_000;

fn read_wav(path: &PathBuf) -> Vec<f32> {
    let b = std::fs::read(path).unwrap();
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
fn real_clips_snr() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/speech");
    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "wav"))
        .collect();
    paths.sort();
    let mut noisy = 0;
    for p in &paths {
        let x = read_wav(p);
        let snr = snr_db(&x, SR).expect("real speech has an estimate");
        let flagged = check(&x, SR).unwrap().contains(&Warning::Noisy);
        println!(
            "{}: snr {snr:.1} dB{}",
            p.file_stem().unwrap().to_string_lossy(),
            if flagged { "  NOISY" } else { "" }
        );
        assert!((5.0..120.0).contains(&snr), "{p:?}: {snr}");
        noisy += usize::from(flagged);
    }
    println!("{noisy} of {} clips flagged noisy", paths.len());
    // Only the clip with the least contrast between speech and background (17.4 dB).
    assert_eq!(noisy, 1);
}
