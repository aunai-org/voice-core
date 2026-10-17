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
| `jitter` | `local_jitter`, `local_jitter_with_track` | Local jitter (mean absolute difference of consecutive voice periods over the mean period, as a fraction) and the number of period pairs behind it. Periods come from normalised cross-correlation of one cycle against the next inside the voiced stretches of the pitch track; pairs more than a factor of 1.3 apart are left out; `None` under 20 pairs |
| `shimmer` | `local_shimmer`, `local_shimmer_with_track` | Local shimmer (mean absolute difference of consecutive voice-period peak-to-peak amplitudes over the mean amplitude, as a fraction) and the number of period pairs behind it. Same period tracking as `jitter`; pairs more than a factor of 1.3 apart in period or 1.6 in amplitude are left out; `None` under 20 pairs. Reads about 2 points below Praat on compressed speech |
| `hnr` | `mean_hnr`, `mean_hnr_with_track` | Harmonics-to-noise ratio in dB (Boersma 1993): per voiced frame `10 log10(r / (1 - r))` from the autocorrelation peak height `r` the pitch tracker already measures, averaged over the voiced frames; `None` under 10 voiced frames. Reads about 1 dB below Praat on average |
| `phonation` | `max_phonation`, `max_phonation_with_track` | Maximum phonation time: the longest unbroken voiced stretch (gaps up to 20 ms allowed) with its start, mean f0, f0 spread in semitones and level; `None` under 10 voiced frames. Same stretch as Praat's pitch track on 16 of 20 real clips (within 0.02 s); Praat has no such measure, see PROVENANCE |
| `ladder` | `level_ladder` | Level ladder for a volume exercise: for each step window, the energy-mean level of its speech in dBFS and relative to the first step (or a given baseline); speech is found inside each step alone, so a whisper step next to a loud one is not taken for silence. 20 real clips cut into three steps with gains of 0, +10 and -8 dB: mean absolute difference from Praat 0.13 dB |
| `window_stats` | `window_stats`, `window_stats_with_track` | Level and pitch of given time windows (for example the words of a script from word timestamps): energy-mean level in dBFS, mean f0, 10th-to-90th percentile f0 range in semitones, and the level and pitch relative to the recording's own speech level and median f0, so a caller can tell how much a word stood out. Pitch needs 3 voiced frames. Against Praat's pitch track on 281 windows of 0.4 s: median mean-f0 difference 0.05 semitone, 87% within 1 semitone (the rest are octave disagreements between the two trackers) |
| `hesitations` | `hesitations`, `hesitations_with_track` | Hesitation events with start and end: silent pauses of at least 0.5 s between speech segments, and held sounds (voiced for at least 0.4 s with pitch within 1 semitone and level within 8 dB, such as a held "uhm"). The thresholds are plain choices (see `HesitationConfig`); repetitions and restarts need the words and are not found. 20 fluent read-aloud clips: all 5 pauses match Praat, held sounds differ on 2 stretches; no recording with real filled pauses is in the test set yet |
| `emphasis` | `emphasis` | How much each word of a reading stood out, from word timestamps: level lift (dB above the median level of the other words), pitch lift (semitones above the median word pitch of the others) and the pause before the word. Needs 3 other words with a level or pitch. A lift is a distance from the speaker's own other words; what counts as strong is not set here (no recordings with deliberate emphasis yet). Against Praat's window levels and pitches on 255 and 188 words: level lift identical, pitch lift median difference 0.21 semitone; +6 dB on a real word adds 6.00 dB of lift on 19 clips |
| `pace` | `syllable_nuclei`, `syllables_per_second` | Syllable nuclei (intensity peaks that are voiced and separated by a 2 dB dip, after de Jong and Wempe 2009) and syllables per second over the recording; the intensity contour is a 64 ms Gaussian level, as Praat's for 50 Hz |
| `pauses` | `speech_segments`, `pause_stats` | Speech/silence segmentation: frames within 25 dB of the loudest frame (and above the quietest 5% of frames, unless the level curve is nearly flat) are sounding, sounding runs under 0.1 s are dropped and silent gaps under 0.1 s are filled; pause count, total and longest pause, speech ratio. Leading and trailing silence is not a pause |
| `stream` | `Analyzer` (`push`, `finish`) | Live feedback: push audio in chunks of any size and get a level (dBFS), a speech/silence flag, a pitch (f0), a syllable-nucleus flag and a syllable rate (syllables per second over the last 3 s, from the first second on) for every 10 ms frame, about 20 ms after its centre (170 ms for all three, which look 150 ms ahead). Levels equal the batch ones; the speech flag ignores sounds and gaps shorter than 0.1 s but is still judged against the loudest frame heard so far, so it is rougher than `pauses` (see limits below) |
| `quality` | `check`, `Warning`, `snr_db` | Cheap recording checks: too short (under 1 s), clipped (0.1% of samples at full scale), too quiet (RMS under -50 dBFS), noisy (speech-to-noise ratio under 20 dB). `snr_db` is the 95th minus the 5th percentile of the 10 ms levels, so it needs a pause or quiet stretch to read the background from; it returns nothing for steady signals. Thresholds are practical defaults, not from a standard, and there is no Praat measure to compare them with: on the twenty real clips the ratio runs 17 to 105 dB and one clip (17.4 dB) is flagged |
| crate root | `analyze` returning `VoiceReport` | One call for a whole recording: duration, RMS level (dBFS), integrated loudness (LUFS), median f0, voiced fraction, syllable count, syllables per second, pause count, total and longest pause, and quality warnings; serializable to JSON with serde |

Pitch is checked against Praat 6.1.38 on synthetic signals (steady sines, glides, vibrato; within 0.1 cent of Praat on the moving-pitch clips) and on twenty real Common Voice clips, twelve of them with high noise floors (median 2.9 cents from Praat, voicing agreement 0.98, and 0.9% of the frames voiced in both are more than 300 cents away; the worst clip has 0.95 voicing agreement). Speech/silence segmentation is checked against Praat 6.1.38's silence detection on the same twenty clips (same segment count in 19 of 20, 98.7% agreement on a 10 ms grid, boundaries within 181 ms, total pause time within 158 ms; the miss is a 0.11 s run on the 0.1 s limit); the threshold is relative to the loudest frame, so one loud click in a quiet recording can hide the speech. The streaming speech flag agrees with Praat's silence detection on 86.8% of 10 ms frames on the twenty clips (95.8% when the analyzer is seeded with the speaker's usual loudest level, because the first words are otherwise judged against themselves); it changes only after 0.1 s of the new value, so use `pauses` for final numbers. The streaming pitch runs the batch path search with a 150 ms lookahead: on the same twenty clips its voicing agrees with Praat on 95.0% of frames (batch track 97.9%), the median difference is 2.9 cents, and 1.3% of the frames voiced in both are more than 300 cents away (batch 0.9%); use `pitch` for final numbers. Jitter is checked on synthetic pulse trains with known period differences (1% and 2% recovered within 0.4 points) and on the same twenty real clips against Praat 6.1.38's local jitter on its periodic pulses (clips run 1.5 to 4.2% jitter; mean absolute difference 0.23 points, worst clip 0.55 points, ours 0.17 points lower on average); there is no ground truth for real speech, and compressed Common Voice audio inflates every jitter value. Pace is checked on synthetic tone bursts (counts equal the true count and a Praat-intensity-based reference) and on the same twenty real clips against a detector built on Praat's intensity and pitch with the same rules: 99.2% of its nuclei are found within 50 ms, 92.1% of ours match one, and we find 7.7% more nuclei in total. There is no human syllable count, so this says we agree with that detector, not that the count is right; fast speech and vowel-less syllables are untested. The streaming nuclei, on the same clips, find 92.3% of that detector's nuclei within 50 ms and 80.8% of ours match one, with 14% more nuclei in total (batch pace 99.2%, 92.1%, +7.7%), because a live nucleus cannot be taken back when a higher peak follows more than 100 ms later.

Levels are **relative** (dBFS / LUFS, full scale = 0): phone microphones are not calibrated, so values are comparable on the same device and mic, never absolute sound pressure.

## What is planned

Jitter, shimmer and HNR are in the batch report (`jitter_local`, `shimmer_local`, `hnr_db`, each empty when the voiced speech is too short) but not yet in the streaming `Analyzer`, which today reports level, speech/silence, pitch, syllable nuclei and a 3 s syllable rate. Accuracy targets and how they are checked are in [docs/spec.md](docs/spec.md) section 7; measured numbers will be published here once the benchmark exists.

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
cargo bench -p voice-core
```

Speed on 10 s of synthetic speech-like audio at 16 kHz (`cargo bench`, one x86-64 cloud core, release build): the full report takes about 129 ms (78 times faster than real time); the streaming `Analyzer` fed 10 ms blocks takes about 64 ms (157 times faster); loudness alone about 1 ms. Numbers on a phone will differ, so rerun the benchmark on the target device.

CI runs these on Linux and Windows for every pull request. macOS runs only on `main`, tags and the nightly schedule, to keep pull requests fast. It also type-checks the crate for `wasm32-unknown-unknown`, `aarch64-linux-android` and `aarch64-apple-ios`.

## License

MIT. Dependencies are limited to MIT, Apache, BSD and similar licenses, enforced by `cargo deny`. Every algorithm is implemented from a published paper or standard and recorded in [PROVENANCE.md](PROVENANCE.md). Praat is used only as an offline reference for validation, never as a dependency (see PROVENANCE.md).
