# voice-core: Roadmap

Durations are rough targets for one developer, not commitments. Status: draft v0.1 (2026-10-06).

| Milestone | Scope | Rough effort |
|---|---|---|
| **M0 Bootstrap** | Repo, CI, license checks, fixture set, Parselmouth oracle script | 1 week |
| **M1 Basic measures** | Resample, pitch, loudness, VAD/pauses, `analyze()` batch API, first benchmark vs Praat | 2 weeks |
| **M1b Live basics** | Minimal streaming `Analyzer` (level + speech/silence frames) for the live meter and pause count | 3-4 days, needed for mimic Week 1 |
| **M2 Pace and quality** | Syllable-rate pace, quality warnings, report struct, accuracy table v1 | 1-2 weeks |
| **M3 Voice steadiness** | Max phonation time, level ladder, hesitation events, `window_stats` for the emphasis coach, jitter, shimmer, HNR, oracle tolerances, documentation of limits | 2 weeks |
| **M4 Streaming** | `Analyzer` push/finish, streaming-vs-batch tests, latency numbers | 1-2 weeks |
| **0.1.0 release** | README, provenance, crates.io publish, used by voice-flutter | 1 week |
| **M5 WASM package** | `wasm-bindgen` crate for web, size budget, demo page | 1-2 weeks |
| **M6 Quality and languages** | Noise robustness, more fixtures (languages, ages), pitch backend swap if benchmark favors it | ongoing |
| **Later / optional** | `no_std`, formant tracking, spectral features, optional ML pitch backend (ONNX) as a separate feature crate | when needed |

## Gates
- Do not start M3 until M1/M2 match Praat on pitch and loudness within tolerance.
- Do not publish 0.1.0 until the accuracy table is real data, not targets.
- Re-evaluate crate choices (`pyin` vs `pitch-core`) at the end of M1.

## Dependencies on other repos
voice-flutter consumes voice-core from 0.1.0 (can use a git dependency before that). mimic consumes it only through voice-flutter.
