# To-do (routine-driven)

Source of truth for the scheduled routine. Order follows the roadmaps: voice-core first, then voice-flutter, then mimic. Each item = one small draft PR on its own branch. Never push to main after the initial commit, never merge. If an item needs a decision (see "Blocked on decision"), the routine stops and reports instead of guessing. Praat/GPL rule applies: no Praat code or GPL dependencies; Parselmouth only in `tools/` as an offline oracle.

Mark items `[x]` with the PR link when the draft PR is open. Last updated: 2026-10-08.

## Setup (done by the first thread)
- [x] Initial commit on main in all three repos (README, LICENSE, .claude/settings.json, docs/ from roadmaps)

## voice-core
### M0 Bootstrap
- [ ] Cargo workspace scaffold (`voice-core` crate, MIT, rustfmt, clippy config, empty modules `dsp`, `pitch`, `loudness`, `vad`, `syllables`, `perturbation`, `report`)
- [ ] CI: fmt, clippy, test on Linux/macOS/Windows
- [x] `cargo deny` license allowlist (MIT/Apache/BSD/Unlicense) in CI — PR claude/ci-cargo-deny (deny.toml + CI job; not validated locally, cargo-deny not installed, CI is the check)
- [x] CI check builds: wasm32-unknown-unknown, aarch64-linux-android, aarch64-apple-ios — PR claude/ci-target-checks
- [x] `PROVENANCE.md` skeleton — PR claude/provenance-skeleton
- [x] Synthetic fixture generator (sines, noise, known-answer signals) and `tools/oracle.py` (Parselmouth, offline only) emitting expected JSON — PR claude/tools-fixtures-oracle (generator run and works; oracle.py compiles but NOT run: Parselmouth not installed)
- [x] Run `tools/oracle.py` on the synthetic clips with Parselmouth (0.4.7, Praat 6.1.38; pip-installs fine in the routine env, ~4 MB), commit the reference JSON, add a Rust test that loads it — PR claude/run-20261008-12-oracle-reference (reference = pure sines/harmonics only; jitter/shimmer/HNR on pure tones are ~0/>75 dB and not meaningful as accuracy targets, real speech clips still needed)
- [ ] Real fixture set: 30+ CC0/consented clips (needs audio source, see blocked)
### M1 Basic measures
- [ ] `dsp`: framing + resample to 16 kHz (`rubato`), unit tests
- [ ] First measurement with tests: frame RMS dB + loudness (`ebur128` integrated LUFS)
- [ ] Pitch behind a trait (`pyin` backend), tests on synthetic sines
- [ ] VAD/pauses (`earshot`), pause stats
- [ ] `analyze()` batch API + `VoiceReport` (serde)
- [ ] First benchmark vs Praat; re-evaluate `pyin` vs `pitch-core` (gate)
### M1b Live basics
- [ ] Minimal streaming `Analyzer` (level + speech/silence per frame)
### M2 Pace and quality
- [ ] Syllable-rate pace (de Jong & Wempe), quality warnings, accuracy table v1
### M3 Voice steadiness (gate: M1/M2 match Praat on pitch and loudness)
- [ ] Jitter, shimmer, HNR; max phonation time; level ladder; hesitation events; `window_stats`; oracle tolerances; limits docs
### M4 Streaming and release
- [ ] Streaming `Analyzer` push/finish with streaming-vs-batch tests, latency numbers
- [ ] Criterion benchmarks
- [ ] 0.1.0 prep: README + accuracy table (real data only), PROVENANCE complete

## voice-flutter
- [ ] F0 spike: FRB plugin with stub `analyze`, Windows + Android (versions recorded)
- [ ] CI: Rust fmt/clippy/test, flutter analyze/test, codegen drift check
- [ ] F1 real bridge to voice-core (after voice-core M1), Dart facade, basic frame stream
- [ ] F2 example + benchmark app
- [ ] F3 streaming (after voice-core M4)

## mimic
- [ ] Phase 0: Flutter project scaffold (Android + Windows), analysis options, CI
- [ ] Week 1 screens from canvas (Today, Recording/teleprompter, Results, Progress) with voice_flutter stub
- [ ] Bundled scripts (about 10 sales/demo scripts)
- [ ] Week 2: local storage, warm-ups, streaks, notifications
- [ ] Week 3: emphasis coach, vocabulary drill (roleplay waits on LLM decision)

## Blocked on decision (routine must stop and ask)
- Transcription path for Week 1 (cloud, bundled small model, script alignment)
- Roleplay LLM (cloud proxy vs local) and TTS voice
- Source and license of fixture audio and reference "role" speech
- Legal check of GPL position before any release; model licenses before 1.1
- Real-device Android/Windows benchmarks (needs the user's devices)
