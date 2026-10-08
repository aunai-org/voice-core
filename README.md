# voice-core

[![CI](https://github.com/aunai-org/voice-core/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/aunai-org/voice-core/actions/workflows/ci.yml?query=branch%3Amain)
[![CI nightly](https://github.com/aunai-org/voice-core/actions/workflows/ci.yml/badge.svg?event=schedule)](https://github.com/aunai-org/voice-core/actions/workflows/ci.yml?query=event%3Aschedule)
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

Levels are **relative** (dBFS / LUFS, full scale = 0): phone microphones are not calibrated, so values are comparable on the same device and mic, never absolute sound pressure.

## What is planned

Pitch (f0 track and summary), speech/silence detection and pauses, speaking pace (syllables per second), jitter, shimmer and HNR, a one-call `analyze` that returns a report with reliability warnings (too short, noisy, clipped), and a streaming `Analyzer` for live feedback. Accuracy targets and how they are checked are in [docs/spec.md](docs/spec.md) section 7; measured numbers will be published here once the benchmark exists.

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
