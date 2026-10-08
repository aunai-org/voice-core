//! Checks the committed Praat reference values (`tests/data/synthetic_oracle.json`,
//! produced offline by `tools/oracle.py`) and compares voice-core against them.

use serde_json::Value;
use voice_core::dsp::resample;
use voice_core::loudness::rms_db;

const SR: f32 = 16_000.0;

fn reference() -> Value {
    let text = include_str!("data/synthetic_oracle.json");
    serde_json::from_str(text).expect("oracle json parses")
}

fn sine(freq: f32, amp: f32, secs: f32) -> Vec<f32> {
    (0..(SR * secs) as usize)
        .map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / SR).sin())
        .collect()
}

#[test]
fn reference_f0_matches_known_answers() {
    let r = reference();
    let truth = [
        ("sine_100hz", 100.0),
        ("sine_220hz", 220.0),
        ("sine_440hz", 440.0),
        ("harmonic_110hz", 110.0),
        ("harmonic_200hz", 200.0),
        ("tone_pause_tone", 150.0),
    ];
    for (name, f0) in truth {
        let got = r["clips"][name]["f0_median_hz"].as_f64().expect(name);
        assert!(
            (got - f0).abs() / f0 < 0.001,
            "{name}: praat f0 {got} vs true {f0}"
        );
    }
    for name in ["silence", "white_noise"] {
        assert!(
            r["clips"][name]["f0_median_hz"].is_null(),
            "{name} must be unvoiced"
        );
        assert_eq!(r["clips"][name]["voiced_fraction"].as_f64(), Some(0.0));
    }
}

/// Our RMS level of a 0.5-amplitude sine against Praat's mean intensity, after
/// converting Praat's dB SPL scale (re 2e-5) to dBFS. Tolerance 0.05 dB.
#[test]
fn rms_level_matches_praat_intensity() {
    let r = reference();
    let offset = r["_meta"]["intensity_ref_db_offset"].as_f64().unwrap();
    for (name, freq) in [
        ("sine_100hz", 100.0),
        ("sine_220hz", 220.0),
        ("sine_440hz", 440.0),
    ] {
        let praat_dbfs = r["clips"][name]["intensity_mean_db"].as_f64().unwrap() - offset;
        let ours = f64::from(rms_db(&sine(freq, 0.5, 2.0)));
        println!(
            "{name}: praat {praat_dbfs:.4} dBFS, ours {ours:.4} dBFS, err {:.4} dB",
            ours - praat_dbfs
        );
        assert!(
            (ours - praat_dbfs).abs() < 0.05,
            "{name}: ours {ours} vs praat {praat_dbfs}"
        );
    }
}

/// 440 Hz sine at 48 kHz resampled to 16 kHz by us, compared with Praat's own
/// resampling of the same clip (RMS over the middle, edges trimmed). Tolerance 0.05 dB.
#[test]
fn resampled_level_matches_praat_resample() {
    let r = reference();
    let clip = &r["clips"]["sine_440hz_48k"];
    assert_eq!(clip["sample_rate_hz"].as_f64(), Some(48_000.0));
    let praat = clip["resampled_16k_rms_dbfs"].as_f64().unwrap();
    let orig =
        clip["intensity_mean_db"].as_f64().unwrap() - r["_meta"]["intensity_ref_db_offset"].as_f64().unwrap();
    let input: Vec<f32> = (0..96_000)
        .map(|i| 0.5 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48_000.0).sin())
        .collect();
    let out = resample(&input, 48_000, 16_000).unwrap();
    let ours = f64::from(rms_db(&out[2048..out.len() - 2048]));
    println!(
        "praat resampled {praat:.4} dBFS, ours {ours:.4} dBFS, err {:.4} dB; praat original {orig:.4} dBFS",
        ours - praat
    );
    assert!((ours - praat).abs() < 0.05, "ours {ours} vs praat {praat}");
    assert!((ours - orig).abs() < 0.05, "ours {ours} vs original {orig}");
}
