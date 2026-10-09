# Provenance

Every algorithm in voice-core is implemented from a published paper or public formula. No source code from Praat, Parselmouth, rustmouth, aubio, Essentia or openSMILE is read-copied, vendored or linked (see `docs/spec.md`, section 9). Praat/Parselmouth appear only in `tools/` as an offline test oracle and are never shipped.

**Praat's role.** Praat (via Parselmouth) is the validation reference only: its numbers are compared against ours in tests, offline. It is never a dependency and never a source for algorithms; every algorithm is implemented from the papers and standards named below. Before the first major release the oracle and its reference numbers move to a separate public repository (see `docs/decisions.md`, "Oracle split"); until then they live under `tools/` and `tests/data/` and are excluded from the published crate.

Add a row when an algorithm lands in the code. A row with status `planned` means nothing is implemented yet.

| Metric / component | Module | Basis (paper or public formula) | Third-party crate used | Status |
|---|---|---|---|---|
| Frame RMS level (dBFS) | `loudness` | Root-mean-square definition, 10·log10(mean square) | none | implemented |
| Framing | `dsp` | Standard fixed-length, fixed-hop windowing | none | implemented |
| Resampling to 16 kHz | `dsp` | Band-limited FFT resampling | `rubato` (MIT) | implemented |
| Integrated loudness (LUFS) | `loudness` | ITU-R BS.1770 / EBU R128 | `ebur128` (MIT) | implemented (mono, gated) |
| Pitch (f0) | `pitch` | Boersma (1993), "Accurate short-term analysis of the fundamental frequency and the harmonics-to-noise ratio of a sampled sound": windowed normalised autocorrelation, peak picking, parabolic interpolation (no cross-frame path search yet) | none | implemented (checked on synthetic signals and eight real clips) |
| Speech/silence segmentation, pauses | `pauses` | Energy-based: Gaussian-windowed mean-square level (32 ms) against a threshold 25 dB below the loudest frame, minimum sounding and silent run lengths of 0.1 s; same settings as Praat's silence detection, used as the offline reference | none | implemented (checked on eight clean clips) |
| Syllable-rate pace | `pace` | de Jong and Wempe (2009) | none | implemented (synthetic bursts only) |
| Recording-quality warnings | `quality` | Practical thresholds (duration, clipped-sample share, RMS level); no published basis | none | implemented |
| Jitter (local, RAP, PPQ5) | `perturbation` | Boersma (1993) style definitions | none | planned |
| Shimmer (local, APQ3, APQ5) | `perturbation` | Public perturbation-measure definitions | none | planned |
| HNR | `perturbation` | Autocorrelation method (Boersma 1993) | `realfft` (MIT/Apache) | planned |

Dependency licenses are enforced in CI by `cargo deny` (MIT, Apache-2.0, BSD, Unlicense only).

## Test audio

Real-speech clips under `crates/voice-core/tests/data/speech/` (excluded from the published crate). Source: Mozilla Common Voice 17.0, English test split, a dataset Mozilla releases under CC0 1.0. The clips were fetched on 2026-10-09 as MP3 through the `fixie-ai/common_voice_17_0` re-upload on Hugging Face (Mozilla's own Hugging Face copy has been withdrawn), then converted with ffmpeg to 16 kHz mono 16-bit WAV. The re-upload's dataset card states no licence, and the CC0 status here comes from the Common Voice project, not from a page re-checked at fetch time. Please confirm it before the first release.

| Clip (Common Voice id) | Seconds | Speaker notes (from the dataset) | Sentence |
|---|---|---|---|
| common_voice_en_18188256 | 3.75 | male, fifties, United States English | We are going to the football game tonight. |
| common_voice_en_18365693 | 3.10 | male, thirties, England English | You are so rude! |
| common_voice_en_665631 | 4.95 | not given | There was nothing to hold him back except himself. |
| common_voice_en_36734620 | 5.33 | India and South Asia | It might be in this writer's top ten! |
| common_voice_en_18179121 | 4.18 | not given | The ladder on the fire truck was not long enough. |
| common_voice_en_18373309 | 6.63 | not given | I like apples, pears, and Pomegranate, but I do like strawberries or grapes. |
| common_voice_en_39751075 | 6.01 | United States English, New York English | Madin was a significant figure of post-war Birmingham architecture. |
| common_voice_en_34382925 | 2.27 | male, twenties | Rapidan campaign May-June. |
