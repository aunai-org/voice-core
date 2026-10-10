# To-do (routine-driven)

Source of truth for the scheduled routine. Order follows the roadmaps: voice-core first, then voice-flutter, then mimic. Each item = one small draft PR on its own branch. Never push to main after the initial commit, never merge. If an item needs a decision (see "Blocked on decision"), the routine stops and reports instead of guessing. Praat/GPL rule applies: no Praat code or GPL dependencies; Parselmouth only in `tools/` as an offline oracle.

Mark items `[x]` with the PR link when the draft PR is open. Last updated: 2026-10-08.

**Standing rule:** any PR that adds or changes a public feature or metric in voice-core updates the README's "What works today" / "What is planned" tables in the same PR, and the routine then refreshes `/mnt/project-files/voice-lib/STATUS.md` (what exists on main per repo) so other threads can read current state.

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
- [ ] Real fixture set: 30+ CC0/consented clips (source chosen: Common Voice CC0 via a Hugging Face re-upload; 20 clips in so far, PR claude/run-20261009-15-real-speech and claude/run-20261010-06-more-clips; still missing: whisper, child, breathy, other accents and languages)
### M1 Basic measures
- [x] `dsp`: framing + resample to 16 kHz (`rubato`), unit tests — PR claude/run-20261008-14-resample (stacked on the LUFS PR; Unicode-3.0 allowed in deny.toml per omr; level matches Praat's own resample within 0.0003 dB)
- [x] First measurement with tests: frame RMS dB (voice-core PR #1) + integrated LUFS via `ebur128` — PR claude/run-20261008-13-lufs (stacked on the oracle-reference PR; checked against the BS.1770 known answer, -3.01 LUFS for a full-scale 997 Hz sine; Praat has no LUFS; real speech clips and EBU 3341 files still to add)
- [x] Pitch behind a trait, own autocorrelation backend (Boersma 1993), tests on synthetic sines — PR claude/run-20261008-17-pitch (`pyin` 1.2.0 dropped: panics on ~20% of sine inputs at a 75 Hz floor and does not compile for wasm32). Still to do: cross-frame path search, real speech clips
- [x] Speech/silence segmentation and pause stats (own energy-based `pauses` module, no `earshot`): same segment count as Praat's silence detection on 8 of 8 clips, 99.0% grid agreement, boundaries within 83 ms, total pause within 90 ms — PR claude/run-20261009-15-pauses
- [ ] Pauses on noisy and whispered clips (threshold is relative to the loudest frame, so a loud click or steady noise breaks it); decide whether to add a noise floor
- [x] `analyze()` batch API + `VoiceReport` (serde) — PR claude/run-20261009-05-analyze (level, LUFS, median f0, voiced fraction; pauses and pace join when VAD and pace land)
- [x] First benchmark vs Praat on moving-pitch synthetic signals (glide, vibrato, harmonic glide): median f0 difference from Praat 0.02 to 0.06 cents, 95th percentile under 0.1 cents, voicing agreement 1.000 on all three; the pyin vs pitch-core gate is settled (own Boersma 1993 backend, see pitch row) — PR claude/run-20261009-09-pitch-benchmark
- [x] First benchmark vs Praat on real speech: 8 Common Voice clips (CC0), median 4.3 ct from Praat, voicing agreement 0.94, 9.4% gross (octave) errors — PR claude/run-20261009-15-real-speech
- [x] Cross-frame path search (Viterbi, Boersma 1993 costs) in `pitch`: gross errors 9.4% to 1.5% of both-voiced frames on the 8 clips, median 3.5 ct, voicing agreement 0.97; synthetic results unchanged — PR claude/run-20261009-17-pitch-path
- [ ] Pitch path search on more clips (14 of the 25 remaining gross errors are in one clip, common_voice_en_665631) and on noisy/breathy voices
- [x] Noisy voices: 12 more clips (high noise floor, teens, female, other speakers), all 20 re-checked against Praat: pitch median 2.9 ct, agreement 0.96 (0.98 after the silence-check fix below), gross errors 1.1%; pauses 18 of 20 same segment count; stream 85.7% plain, 94.4% seeded. Level meter now removes the window mean (fixed a DC offset case) — PR claude/run-20261010-06-more-clips
- [ ] Whisper, child and breathy clips; pause miss on clip 17848293 (almost no level contrast)
- [x] Pitch silence check uses the peak of the windowed, mean-removed frame (as Praat does): quiet periodic background was being called voiced at the 75 Hz floor and near 600 Hz. 20 clips: voicing agreement 0.957 to 0.979, clip 579883 0.85 to 0.95, clip 17714250 0.90 to 0.95, gross errors 1.1% to 0.9%, median unchanged at 2.9 ct — PR claude/run-20261010-11-pitch-voicing
### M1b Live basics
- [x] Minimal streaming `Analyzer` (level + speech/silence per 10 ms frame, `push`/`finish`, chunk-size independent): levels equal the batch ones, speech flag agrees with Praat's silence detection on 88.5% of frames (95.4% when seeded with a reference level) — PR claude/run-20261010-05-stream
- [x] Streaming `Analyzer`: live pitch per frame (strongest candidate, no path search): voicing agreement with Praat 90.8% (batch 97.9%), median 3.3 ct, 7.0% of both-voiced frames more than 300 ct off (batch 0.9%) — PR claude/run-20261010-12-stream-pitch
- [x] Live pitch: transition costs from the previous frame (batch octave-jump and voiced/unvoiced costs, no lookahead): voicing agreement with Praat 90.8% to 92.2%, gross errors 7.0% to 6.0%, median 3.2 ct — PR claude/run-20261010-14-stream-pitch-jump
- [x] Live pitch: fixed-lag Viterbi, 100 ms lookahead (`PITCH_LAG_FRAMES`): voicing agreement with Praat 92.2% to 94.9%, gross errors 6.0% to 1.4%, median 2.9 ct — PR claude/run-20261010-16-fixed-lag
- [ ] Streaming `Analyzer`: live syllable pace; causal minimum-run smoothing for the speech flag
### M2 Pace and quality
- [x] Syllable-rate pace (de Jong & Wempe) — PR claude/run-20261009-11-pace (synthetic tone bursts only; counts match truth and a Praat-intensity-based reference; no real speech yet)
- [x] Quality warnings: too short, clipped, too quiet in `analyze` report — PR claude/run-20261009-12-quality (thresholds are defaults, no Praat equivalent)
- [ ] Noise (SNR) warning, accuracy table v1
### M3 Voice steadiness (gate: M1/M2 match Praat on pitch and loudness)
- [ ] Jitter, shimmer, HNR; max phonation time; level ladder; hesitation events; `window_stats`; oracle tolerances; limits docs
### M4 Streaming and release
- [ ] Streaming `Analyzer` push/finish with streaming-vs-batch tests, latency numbers
- [ ] Criterion benchmarks
- [ ] 0.1.0 prep: README + accuracy table (real data only), PROVENANCE complete
- [ ] Before the first major release: move `tools/oracle.py`, the fixture generator and the Praat reference numbers into a separate public repo; settle that repo's licence with the GPL legal check (decision in docs/decisions.md, "Oracle split")

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
