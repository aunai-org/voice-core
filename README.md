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
| `pauses` | `speech_segments`, `pause_stats` | Speech/silence segmentation: frames within 25 dB of the loudest frame are sounding, sounding runs under 0.1 s are dropped and silent gaps under 0.1 s are filled; pause count, total and longest pause, speech ratio. Leading and trailing silence is not a pause |
| `stream` | `Analyzer` (`push`, `finish`) | Live feedback: push audio in chunks of any size and get a level (dBFS), a speech/silence flag and a pitch (f0) for every 10 ms frame, about 20 ms after its centre. Levels equal the batch ones; the flag and the pitch are decided frame by frame (the pitch has no lookahead or path search), so they are rougher than `pauses` and `pitch` (see limits below) |
| `quality` | `check`, `Warning` | Cheap recording checks: too short (under 1 s), clipped (0.1% of samples at full scale), too quiet (RMS under -50 dBFS). Thresholds are practical defaults, not from a standard; "noisy" is not covered yet |
| crate root | `analyze` returning `VoiceReport` | One call for a whole recording: duration, RMS level (dBFS), integrated loudness (LUFS), median f0, voiced fraction, syllable count, syllables per second, pause count, total and longest pause, and quality warnings; serializable to JSON with serde |

Pitch is checked against Praat 6.1.38 on synthetic signals (steady sines, glides, vibrato; within 0.1 cent of Praat on the moving-pitch clips) and on twenty real Common Voice clips, twelve of them with high noise floors (median 2.9 cents from Praat, voicing agreement 0.98, and 0.9% of the frames voiced in both are more than 300 cents away; the worst clip has 0.95 voicing agreement). Speech/silence segmentation is checked against Praat 6.1.38's silence detection on the same twenty clips (same segment count in 18 of 20, 98.7% agreement on a 10 ms grid, boundaries within 181 ms, total pause time within 158 ms; the two misses are a clip with almost no level contrast and a 0.11 s run on the 0.1 s limit); the threshold is relative to the loudest frame, so one loud click in a quiet recording can hide the speech. The streaming speech flag agrees with Praat's silence detection on 85.7% of 10 ms frames on the twenty clips (94.4% when the analyzer is seeded with the speaker's usual loudest level, because the first words are otherwise judged against themselves); it has no minimum run lengths, so use `pauses` for final numbers. The streaming pitch picks each frame's candidate from the past only (transition costs from the previous frame, no lookahead or path search): on the same twenty clips its voicing agrees with Praat on 92.2% of frames (batch track 97.9%), the median difference is 3.2 cents, and 6.0% of the frames voiced in both are more than 300 cents away (batch 0.9%), so expect octave errors; use `pitch` for final numbers. Pace is checked on synthetic tone bursts only (counts equal the true count and a Praat-intensity-based reference); fast or noisy real speech is untested.

Levels are **relative** (dBFS / LUFS, full scale = 0): phone microphones are not calibrated, so values are comparable on the same device and mic, never absolute sound pressure.

## What is planned

Jitter, shimmer and HNR, a noise warning, and live pace in the streaming `Analyzer` (today it reports level, speech/silence and pitch). Accuracy targets and how they are checked are in [docs/spec.md](docs/spec.md) section 7; measured numbers will be published here once the benchmark exists.

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
