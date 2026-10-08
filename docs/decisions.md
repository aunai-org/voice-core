# Decisions and notes: voice library thread (2026-10-06)

Running log of what omr and Claude agreed. mimic is its own concept; the older MyVoice docs are for a different product and are not a source for mimic.

## Repos (GitHub org aunai-org)
- `voice-core`: pure-Rust measurement library, MIT, open source.
- `voice-flutter`: flutter_rust_bridge crate + Flutter package wrapping voice-core.
- `mimic`: the app (mimic any role and learn its jargon).

## Decisions
1. **Stay on Rust** for the measurement core. Reuse `pyin` (pitch), `ebur128` (loudness), `earshot` (speech/pauses), `rustfft`/`realfft`, `rubato`, `symphonia`/`hound`. Evaluate `pitch-core` as a pitch backend.
2. **Build ourselves:** jitter, shimmer, HNR, syllable-based speaking pace, the streaming analyzer and the app-facing API.
3. **Praat is GPL-3 (C/C++).** Not a dependency in any shipped code. `rustmouth`/`praat-sys` are only wrappers and also GPL-3. Praat/Parselmouth is used only as an offline test oracle. Algorithms are reimplemented from papers; no Praat source copied. Get a quick legal check before commercial release.
4. **Quality:** the Rust crates are good enough but not state of the art and unbenchmarked; build a 20-30 clip test set compared against Parselmouth.
5. **Flutter:** `flutter_rust_bridge`, not UniFFI (no official Dart support). Web via WASM later.
6. **Streaming:** v1 analyzes the whole recording after each take; design the core around a frame-by-frame analyzer so live feedback can be added later (live pitch/level/pace hints, mimic-vs-target in real time).
7. **Offline:** all measuring is on-device with no network. Speech-to-text and LLM, if added, are optional in-app downloads behind a subscription: `whisper-rs` (batch) or `sherpa-onnx` (streaming), `llama-cpp-2` for local coaching text.
8. **Model delivery:** optional download, show size, Wi-Fi only, resumable, hash-checked. Gating is a soft entitlement check. Check model licenses for commercial use and Apple/Google policy before launch.

## Open questions
- Which on-device STT/LLM models and licenses.
- Whether to offer a cloud fallback for older phones.
- Legal review of the GPL position before release.
- Where mimic gets its reference "role" speech from (licensing of reference audio).

## Product definition for mimic (added 2026-10-06, from the Week 1 Screens thread)
mimic is a "voice gym" for working professionals: a 10-15 minute daily session (warm-up, vocabulary drill, AI roleplay, review) to build vocal power, vocabulary and confidence for meetings and sales/demo calls. Signature features: emphasis coach and filler/jam tracker. Week 1 = record, transcribe, measure with a script-reading teleprompter. Build order: Week 1 record/measure, Week 2 warm-ups and streaks, Week 3 roleplay/emphasis/vocabulary. Flutter on Android and Windows first. Not a medical tool. See mimic/spec.md v0.2.

## Tensions to settle
- Week 1 needs transcripts (accuracy, fillers) but offline-first says STT is an optional paid download: cloud, bundled small model, or script alignment.
- AI roleplay needs an LLM: cloud via backend proxy or local via llama-cpp-2.

## Library changes from the voice-gym product (2026-10-06)
- voice-core adds: max phonation time, level ladder, hesitation events, `window_stats` for the emphasis coach. Loudness is relative (phones are uncalibrated), so the "60 dB" target in the Week 1 design needs a calibration step or a relative target.
- Live volume meter and pause count are needed in Week 1, so the minimal streaming analyzer (level + speech/silence) moves up to M1b; pitch streaming stays later.
- voice-flutter targets Windows desktop first-class and Android; iOS later.

## Shelved ideas (2026-10-06)
- **Video self-review** (record video while speaking, watch back with audio metrics overlaid; later optional face/gesture analysis). Shelved by omr; may become a standalone app or a paid add-on. Not in mimic v1. If revisited, let a session hold optional attached tracks. Reasoning: mimic/validation.md and the thread.

## Oracle split (decided 2026-10-08, omr)
Before the first major release the Praat/Parselmouth oracle (`tools/oracle.py`, the synthetic fixture generator, and the committed reference numbers) moves into a separate public repository, so voice-core itself stays free of any Praat-derived material. Until then it stays under `tools/` in this repo and is kept out of the published crate (`tools/` is outside the package directory, and the reference JSON and its test are listed in the crate's `exclude`). The new repository's licence is to be settled together with the legal check on the GPL position listed under open questions; the oracle is validation only and never a source for algorithms, which are implemented from published papers and standards.
