# voice-core

[![Linux](https://github.com/aunai-org/voice-core/actions/workflows/ci-linux.yml/badge.svg?branch=main)](https://github.com/aunai-org/voice-core/actions/workflows/ci-linux.yml?query=branch%3Amain)
[![Windows](https://github.com/aunai-org/voice-core/actions/workflows/ci-windows.yml/badge.svg?branch=main)](https://github.com/aunai-org/voice-core/actions/workflows/ci-windows.yml?query=branch%3Amain)
[![macOS](https://github.com/aunai-org/voice-core/actions/workflows/ci-macos.yml/badge.svg?branch=main)](https://github.com/aunai-org/voice-core/actions/workflows/ci-macos.yml?query=branch%3Amain)
[![wasm, Android, iOS, deny](https://github.com/aunai-org/voice-core/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/aunai-org/voice-core/actions/workflows/ci.yml?query=branch%3Amain)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Pure-Rust, offline library that measures how someone speaks from raw audio: pitch, loudness, pace, pauses and voice steadiness (jitter, shimmer, HNR).

It takes plain mono `f32` samples (in [-1, 1]) plus a sample rate and returns numbers. There is no network access, no model file and no speech-to-text, and it is meant to build for desktop, Android, iOS and `wasm32-unknown-unknown`. It is the measurement engine behind [mimic](https://github.com/aunai-org/mimic) (through [voice-flutter](https://github.com/aunai-org/voice-flutter)), but knows nothing about either and can be used on its own.

> **Status: pre-alpha, not published to crates.io yet.** Only the pieces listed under "What works today" exist. The rest is planned; see [docs/spec.md](docs/spec.md), [docs/plan.md](docs/plan.md) and [docs/roadmap.md](docs/roadmap.md).

## What works today

| Module | Function | What it does |
|---|---|---|
| `loudness` | `integrated_lufs` | Integrated loudness in LUFS (ITU-R BS.1770 / EBU R128 gating) via `ebur128`, so pauses do not pull the value down |
| `loudness` | `rms_db`, `frame_rms_db` | RMS level in dBFS for a block, or per frame |
| `dsp` | `resample` | Band-limited resampling (via `rubato`), e.g. any input rate to 16 kHz |
| `dsp` | `frames` | Split a signal into fixed-length, overlapping frames |
| `pitch` | `AutocorrEstimator` (via the `PitchEstimator` trait), `median_f0`, `voiced_fraction` | Fundamental frequency (f0) track, 10 ms frames, 75 to 600 Hz, autocorrelation method after Boersma 1993; median f0 and voiced fraction summaries |
| `pace` | `syllable_nuclei`, `syllables_per_second` | Syllable nuclei (intensity peaks that are voiced and separated by a 2 dB dip, after de Jong and Wempe 2009) and syllables per second over the recording |
| crate root | `analyze` returning `VoiceReport` | One call for a whole recording: duration, RMS level (dBFS), integrated loudness (LUFS), median f0, voiced fraction, syllable count and syllables per second; serializable to JSON with serde |

Pitch is checked against Praat 6.1.38 on synthetic signals only (steady sines, glides, vibrato; within 0.1 cent of Praat on the moving-pitch clips). It has not been tested on real speech yet, so octave errors on real voices are possible. Pace is checked on synthetic tone bursts only (counts equal the true count and a Praat-intensity-based reference); fast or noisy real speech is untested.

Levels are **relative** (dBFS / LUFS, full scale = 0): phone microphones are not calibrated, so values are comparable on the same device and mic, never absolute sound pressure.

## What is planned

Speech/silence detection and pauses, jitter, shimmer and HNR, reliability warnings in the `analyze` report (too short, noisy, clipped), and a streaming `Analyzer` for live feedback. Accuracy targets and how they are checked are in [docs/spec.md](docs/spec.md) section 7; measured numbers will be published here once the benchmark exists.

## Usage

Until the crate is published, depend on it from git:

```toml
[dependencies]
voice-core = { git = "https://github.com/aunai-org/voice-core" }
```

```rust
use voice_core::{dsp, loudness, Error};

fn main() -> Result<(), Error> {
    // One second of a quiet 220 Hz tone at 44.1 kHz. In an app this is your
    // recording: mono f32 samples in [-1, 1].
    let sample_rate = 44_100u32;
    let samples: Vec<f32> = (0..sample_rate)
        .map(|n| 0.1 * (2.0 * std::f32::consts::PI * 220.0 * n as f32 / sample_rate as f32).sin())
        .collect();

    // Overall loudness of the take, relative to digital full scale.
    let lufs = loudness::integrated_lufs(&samples, sample_rate)?;
    println!("integrated loudness: {lufs:.1} LUFS");

    // Normalise to 16 kHz, then get a level curve (25 ms frames, 10 ms hop)
    // for a live meter or to spot quiet stretches.
    let mono_16k = dsp::resample(&samples, sample_rate, 16_000)?;
    let levels = loudness::frame_rms_db(&mono_16k, 400, 160)?;
    let peak = levels.iter().cloned().fold(f32::MIN, f32::max);
    println!("{} frames, loudest frame {peak:.1} dBFS", levels.len());

    Ok(())
}
```

To get everything that exists today in one call, use `voice_core::analyze(&samples, sample_rate)?`, which returns a `VoiceReport`.

All functions return `Result<_, voice_core::Error>`; empty input and invalid settings (zero rate, zero frame length) are errors, not panics.

## Development

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

CI runs these on Linux and Windows for every pull request. macOS runs only on `main`, tags and the nightly schedule, to keep pull requests fast. It also type-checks the crate for `wasm32-unknown-unknown`, `aarch64-linux-android` and `aarch64-apple-ios`.

## License

MIT. Dependencies are limited to MIT, Apache, BSD and similar licenses, enforced by `cargo deny`. Every algorithm is implemented from a published paper or standard and recorded in [PROVENANCE.md](PROVENANCE.md).
