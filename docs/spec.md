# voice-core: Specification

Repo: github.com/aunai-org/voice-core · License: MIT · Status: draft v0.1 (2026-10-06)

## 1. Purpose
A pure-Rust, offline library that measures how someone speaks from raw audio: pitch, loudness, speaking pace, pauses and voice steadiness. It is the measurement engine for mimic, but knows nothing about mimic, Flutter or any network. Other apps can reuse it.

## 2. Goals and non-goals
Goals
- Praat-comparable pitch, jitter, shimmer and HNR, within tolerances in section 7.
- Runs fully on-device, no network, no model files, small binary.
- Compiles for iOS, Android, desktop and `wasm32-unknown-unknown`.
- One batch call and one streaming API, sharing the same code.
- MIT/Apache-only dependency tree. No GPL/AGPL code, and no code copied from Praat.

Non-goals (v1)
- Speech-to-text, filler/word detection, language understanding (these belong in mimic or an optional ML layer).
- Speaker identification, emotion detection, cloud features.
- Real-time hard guarantees on very old devices.

## 3. Input
Mono `f32` samples in [-1, 1] plus sample rate. Internally resampled to 16 kHz (`rubato`). Helpers for decoding WAV (`hound`) behind a feature flag; the core crate takes plain sample slices.

## 4. Metrics (precise definitions)
| Metric | Definition | Source |
|---|---|---|
| `f0` track | Per-frame fundamental frequency (Hz), voiced flag, 10 ms hop | `pyin` (baseline); `pitch-core` evaluated |
| Pitch summary | median, mean, p10/p90 in Hz and semitones re 100 Hz, range (p90-p10), contour slope | derived |
| Loudness | momentary/short-term LUFS, integrated LUFS, frame RMS dB, dynamic range (p95-p10 of RMS dB over voiced frames) | `ebur128` + own RMS |
| Speech segments | start/end of speech vs silence | `earshot` VAD |
| Pauses | silences between speech segments: count, total, mean, longest; "long pause" threshold default 1.5 s (configurable) | derived from VAD |
| Voiced ratio | voiced time / total time | f0 voiced flags |
| Speaking pace | syllables per second over speech time, and articulation rate excluding pauses | own syllable-nuclei detector (de Jong and Wempe 2009 approach) |
| Jitter | local, RAP, PPQ5 | own, from period marks (Boersma 1993 style definitions) |
| Shimmer | local, APQ3, APQ5 | own |
| HNR | dB, voiced-frame mean, autocorrelation method | own on `realfft` |

Added for the mimic voice-gym use case (v0.2):
| Metric | Definition |
|---|---|
| Max phonation time | longest continuous voiced stretch (sustained hum/vowel), plus its loudness and pitch stability |
| Level ladder | per-step average level for a whisper-to-projected volume exercise, in dBFS relative to the user's own baseline |
| Window stats | for any time range supplied by the app (for example a word or sentence): peak and mean pitch (semitones vs the user's median), peak and mean level, pause before and after, and rise/fall flags. This powers the emphasis coach. |
| Hesitation events | pauses and restarts inside speech with timestamps, for the jam tracker and "time to produce the word" |

Caveat: phone microphones are not calibrated, so loudness is relative (dBFS/LUFS) and comparable only on the same device with the same mic. Do not present it as absolute sound pressure (the Week 1 design's "60 dB" target needs a calibration step or a relative target).

Words per minute is not computed here (needs a transcript). Provide `syllables_per_second` and a documented helper `wpm_from_words(words, speech_seconds)` for the app.

## 4a. Live feedback is needed earlier than first planned
The mimic Week 1 recording screen shows a live volume meter and a live stumble/pause count. So the streaming analyzer's basic frame output (level, speech vs silence) is a Week 1 need; pitch and the full report can stay batch until later. Plan: ship a minimal `Analyzer` (level + VAD per frame) in M1 and add pitch frames and the full streaming report in M4. (A plain RMS meter in Dart is an acceptable stopgap.)

## 5. Public API (Rust)
```rust
pub struct Config { /* sample_rate, f0_min/max, pause_threshold_s, ... with Default */ }

pub fn analyze(samples: &[f32], sample_rate: u32, cfg: &Config) -> Result<VoiceReport, Error>;

pub fn window_stats(report: &VoiceReport, start_s: f32, end_s: f32) -> WindowStats; // emphasis coach: caller supplies word/sentence times from its transcript or script

pub struct Analyzer; // streaming
impl Analyzer {
    pub fn new(sample_rate: u32, cfg: Config) -> Self;
    pub fn push(&mut self, chunk: &[f32]) -> Vec<FrameFeatures>;  // pitch, level, speech/silence per frame
    pub fn finish(self) -> VoiceReport;                            // whole-utterance metrics
}
```
`VoiceReport` is a plain serializable struct (serde) with all section 4 metrics, per-frame series optional (`include_frames`), plus `warnings` (too short, too noisy, clipped, low voiced ratio) so callers can tell users why a number is unreliable.

## 6. Crate layout
- `voice-core` (workspace root): `dsp` (framing, resample), `pitch`, `loudness`, `vad`, `syllables`, `perturbation` (jitter/shimmer/HNR), `report`.
- Features: `wav` (hound), `serde`, `std` (default). Aim for `no_std + alloc` where dependencies allow; not a v1 requirement.
- `tests/` and `fixtures/`: reference audio with Praat-generated expected values (Python script in `tools/`, not shipped).
- `bindings` are NOT in this repo: voice-flutter and a future WASM package depend on voice-core.

## 7. Accuracy targets (to be confirmed by the benchmark)
Compared against Praat via Parselmouth on a fixed set of 30+ clips (clean, phone mic, noisy, male/female/child, shouting, whisper):
- Median f0 within 2% on voiced frames, voicing decision agreement above 90%.
- Jitter local and shimmer local within 15% relative on clean sustained vowels (these are noisy measures; looser on speech).
- HNR within 2 dB on clean voiced segments.
- Integrated LUFS within 0.1 LU of reference `ebur128`.
- Syllable count within 10% of manual count.
Targets are starting guesses; the benchmark sets the real numbers and they get published in the README.

## 8. Performance
Batch analysis of 2 minutes of audio under 1 s on a mid-range phone; streaming latency under one hop plus 20 ms per push. Measured in CI on desktop and on-device in the Flutter benchmark app. No allocations in the per-frame hot path after warm-up.

## 9. Licensing and provenance
- MIT. Dependencies MIT/Apache/BSD/Unlicense only; `cargo deny` enforced in CI.
- Praat, Parselmouth, rustmouth, aubio, Essentia, openSMILE are never dependencies. Praat/Parselmouth appear only in `tools/` as a test oracle, not published.
- Algorithms implemented from papers and public formulas. No Praat source code is read-copied. Keep a `PROVENANCE.md` listing the paper behind each algorithm.

## 10. Risks
- Jitter/shimmer on compressed phone audio are unreliable; surface warnings rather than false precision.
- `pyin` is a small crate; may need vendoring or a fork. `pitch-core` is a candidate replacement.
- Noise and clipping degrade every metric.
- Syllable-nuclei pace is language-sensitive; validate beyond English before claiming support.
