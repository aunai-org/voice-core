# Provenance

Every algorithm in voice-core is implemented from a published paper or public formula. No source code from Praat, Parselmouth, rustmouth, aubio, Essentia or openSMILE is read-copied, vendored or linked (see `docs/spec.md`, section 9). Praat/Parselmouth appear only in `tools/` as an offline test oracle and are never shipped.

Add a row when an algorithm lands in the code. A row with status `planned` means nothing is implemented yet.

| Metric / component | Module | Basis (paper or public formula) | Third-party crate used | Status |
|---|---|---|---|---|
| Frame RMS level (dBFS) | `loudness` | Root-mean-square definition, 10·log10(mean square) | none | implemented |
| Framing | `dsp` | Standard fixed-length, fixed-hop windowing | none | implemented |
| Resampling to 16 kHz | `dsp` | Band-limited sinc interpolation | `rubato` (MIT) | planned |
| Integrated loudness (LUFS) | `loudness` | ITU-R BS.1770 / EBU R128 | `ebur128` (MIT) | planned |
| Pitch (f0) | `pitch` | pYIN: Mauch and Dixon (2014) | `pyin` (to be confirmed) | planned |
| Speech/silence detection | `vad` | Voice activity detection | `earshot` (to be confirmed) | planned |
| Syllable-rate pace | `syllables` | de Jong and Wempe (2009) | none | planned |
| Jitter (local, RAP, PPQ5) | `perturbation` | Boersma (1993) style definitions | none | planned |
| Shimmer (local, APQ3, APQ5) | `perturbation` | Public perturbation-measure definitions | none | planned |
| HNR | `perturbation` | Autocorrelation method (Boersma 1993) | `realfft` (MIT/Apache) | planned |

Dependency licenses are enforced in CI by `cargo deny` (MIT, Apache-2.0, BSD, Unlicense only).
