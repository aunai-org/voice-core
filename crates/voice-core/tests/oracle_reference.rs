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

fn harmonic(f0: f32, amp: f32, secs: f32) -> Vec<f32> {
    // Same stack as tools/gen_fixtures.py: 8 harmonics at 1/k amplitude, peak-normalised.
    let mut out: Vec<f32> = (0..(SR * secs) as usize)
        .map(|i| {
            (1..=8)
                .map(|k| {
                    (1.0 / k as f32) * (2.0 * std::f32::consts::PI * f0 * k as f32 * i as f32 / SR).sin()
                })
                .sum()
        })
        .collect();
    let peak = out.iter().fold(0.0_f32, |m, x| m.max(x.abs()));
    out.iter_mut().for_each(|x| *x *= amp / peak);
    out
}

/// Median f0 of our pYIN track against Praat's median f0 (autocorrelation
/// method, 75 to 600 Hz) for the same synthetic clips. Tolerance 0.5 %.
#[test]
fn median_f0_matches_praat() {
    use voice_core::pitch::{median_f0, AutocorrEstimator, PitchEstimator};
    let r = reference();
    let clips: [(&str, Vec<f32>); 5] = [
        ("sine_100hz", sine(100.0, 0.5, 2.0)),
        ("sine_220hz", sine(220.0, 0.5, 2.0)),
        ("sine_440hz", sine(440.0, 0.5, 2.0)),
        ("harmonic_110hz", harmonic(110.0, 0.5, 2.0)),
        ("harmonic_200hz", harmonic(200.0, 0.5, 2.0)),
    ];
    for (name, samples) in clips {
        let praat = r["clips"][name]["f0_median_hz"].as_f64().unwrap();
        let track = AutocorrEstimator::default().estimate(&samples, 16_000).unwrap();
        let ours = f64::from(median_f0(&track).expect(name));
        let err = (ours - praat) / praat * 100.0;
        println!("{name}: praat {praat:.3} Hz, ours {ours:.3} Hz, err {err:+.3} %");
        assert!(err.abs() < 0.5, "{name}: ours {ours} vs praat {praat}");
    }
}

/// Voiced share of the 0.5 s tone, 1.5 s pause, 0.5 s tone clip against Praat's
/// voiced fraction, and the no-pitch cases. Tolerance 0.05 (absolute).
#[test]
fn voiced_fraction_matches_praat() {
    use voice_core::pitch::{voiced_fraction, AutocorrEstimator, PitchEstimator};
    let r = reference();
    let mut x = sine(150.0, 0.4, 0.5);
    x.extend(vec![0.0; (SR * 1.5) as usize]);
    x.extend(sine(150.0, 0.4, 0.5));
    let est = AutocorrEstimator::default();
    let praat = r["clips"]["tone_pause_tone"]["voiced_fraction"].as_f64().unwrap();
    let ours = f64::from(voiced_fraction(&est.estimate(&x, 16_000).unwrap()));
    println!(
        "tone_pause_tone voiced fraction: praat {praat:.3}, ours {ours:.3}, err {:+.3}",
        ours - praat
    );
    assert!((ours - praat).abs() < 0.05, "ours {ours} vs praat {praat}");
    let silent = voiced_fraction(&est.estimate(&vec![0.0; 32_000], 16_000).unwrap());
    assert_eq!(
        f64::from(silent),
        r["clips"]["silence"]["voiced_fraction"].as_f64().unwrap()
    );
}

/// `analyze()` against Praat for the synthetic clips: median f0 (tolerance
/// 0.5 %), voiced fraction (0.05), and RMS level against Praat's intensity
/// converted to dBFS (0.05 dB).
#[test]
fn analyze_matches_praat() {
    use voice_core::analyze;
    let r = reference();
    let offset = r["_meta"]["intensity_ref_db_offset"].as_f64().unwrap();
    let clips: [(&str, Vec<f32>); 3] = [
        ("sine_100hz", sine(100.0, 0.5, 2.0)),
        ("sine_220hz", sine(220.0, 0.5, 2.0)),
        ("sine_440hz", sine(440.0, 0.5, 2.0)),
    ];
    for (name, samples) in clips {
        let c = &r["clips"][name];
        let rep = analyze(&samples, 16_000).unwrap();
        let f0 = f64::from(rep.f0_median_hz.expect(name));
        let praat_f0 = c["f0_median_hz"].as_f64().unwrap();
        let praat_level = c["intensity_mean_db"].as_f64().unwrap() - offset;
        let praat_voiced = c["voiced_fraction"].as_f64().unwrap();
        println!(
            "{name}: f0 praat {praat_f0:.3} ours {f0:.3} | level praat {praat_level:.4} ours {:.4} | voiced praat {praat_voiced:.3} ours {:.3}",
            rep.rms_dbfs, rep.voiced_fraction
        );
        assert!(((f0 - praat_f0) / praat_f0).abs() < 0.005, "{name} f0");
        assert!(
            (f64::from(rep.rms_dbfs) - praat_level).abs() < 0.05,
            "{name} level"
        );
        assert!(
            (f64::from(rep.voiced_fraction) - praat_voiced).abs() < 0.05,
            "{name} voiced"
        );
    }
}
