# voice-core: Implementation Plan

Companion to [spec.md](spec.md). Status: draft v0.1 (2026-10-06).

## Approach
Wire existing crates first, measure against Praat early, then write the missing metrics. Benchmarks drive crate choices, not preference.

## Steps
1. **Repo setup.** Cargo workspace, MIT license, `cargo deny` (license allowlist), clippy, rustfmt, CI on Linux/macOS/Windows plus `wasm32-unknown-unknown` and `aarch64-linux-android`/`aarch64-apple-ios` check builds.
2. **Fixture set and oracle.** Collect 30+ consented/CC0 clips (or record own). `tools/oracle.py` runs Parselmouth to emit expected f0, jitter, shimmer, HNR, intensity as JSON. Rust tests load the JSON and assert tolerances. The oracle never ships.
3. **Plumbing.** Framing, resample to 16 kHz, optional WAV reading. Unit tests with synthetic sine/noise signals.
4. **Pitch.** Integrate `pyin`; in parallel evaluate `pitch-core` (pYIN/SWIPE'/Praat-style). Pick the backend with the best oracle agreement and mobile speed; hide it behind a trait so it can be swapped.
5. **Loudness.** `ebur128` for LUFS plus own frame RMS dB and dynamic-range summary.
6. **VAD and pauses.** `earshot` segments, merge/hangover rules, pause stats. Test on noisy clips and phone audio.
7. **Syllable pace.** Implement intensity-peak syllable nuclei in voiced regions; calibrate thresholds on fixtures; compare with manual syllable counts.
8. **Perturbation metrics.** Period marking from the f0 track, jitter (local/RAP/PPQ5), shimmer (local/APQ3/APQ5), HNR via autocorrelation. Compare to Parselmouth, record tolerances.
9. **Report and warnings.** Assemble `VoiceReport`, add quality warnings (too short, clipped, low SNR, few voiced frames).
10. **Streaming `Analyzer`.** Frame-level features while pushing audio; `finish()` produces the full report. Test that streaming and batch results agree.
11. **Performance.** Criterion benchmarks; profile and optimize on a real phone via the voice-flutter benchmark app.
12. **Docs and release.** README with accuracy table, examples, `PROVENANCE.md`, semver, publish to crates.io as 0.1.0.

## Changes for mimic (v0.2)
- Step 6 (VAD) also delivers a minimal streaming `Analyzer` giving level and speech/silence per frame, because the Week 1 recording screen needs a live meter and pause count.
- Add step 8b: max phonation time, level ladder, hesitation events and `window_stats` (emphasis coach), tested against Praat on sustained vowels and scripted sentences with marked words.
- Add a calibration note and relative-loudness reporting to the README.

## Definition of done (v0.1)
- All spec section 4 metrics return values; accuracy table published; every target in spec section 7 met or the gap documented.
- CI green on all targets, including wasm and mobile compile checks.
- 2 minutes of audio analyzed in under 1 s on a mid-range phone.
- License audit clean.

## Test strategy
Unit (synthetic signals with known answers), golden/oracle tests (Praat values), property tests for streaming vs batch equivalence, fuzzing on malformed input (empty, NaN, very short, clipped), benchmark regression in CI.

## Ownership and tools
Mostly solo-developer sized. Python only for `tools/`. No GPU, no model files.
