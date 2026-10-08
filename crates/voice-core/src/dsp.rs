//! Framing helpers.

use crate::Error;
use audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};

/// Split `samples` into frames of `frame_len` advancing by `hop`.
///
/// Only complete frames are produced; a trailing partial frame is dropped.
pub fn frames(samples: &[f32], frame_len: usize, hop: usize) -> Result<Vec<&[f32]>, Error> {
    if frame_len == 0 {
        return Err(Error::InvalidConfig("frame_len must be > 0"));
    }
    if hop == 0 {
        return Err(Error::InvalidConfig("hop must be > 0"));
    }
    if samples.len() < frame_len {
        return Ok(Vec::new());
    }
    Ok((0..=samples.len() - frame_len)
        .step_by(hop)
        .map(|s| &samples[s..s + frame_len])
        .collect())
}

/// Resample a mono signal from `from_hz` to `to_hz` (band-limited, FFT based).
///
/// The resampler delay is trimmed, so the output has about
/// `samples.len() * to_hz / from_hz` samples and is aligned with the input.
pub fn resample(samples: &[f32], from_hz: u32, to_hz: u32) -> Result<Vec<f32>, Error> {
    if samples.is_empty() {
        return Err(Error::EmptyInput);
    }
    if from_hz == 0 || to_hz == 0 {
        return Err(Error::InvalidConfig("sample rates must be > 0"));
    }
    if from_hz == to_hz {
        return Ok(samples.to_vec());
    }
    fn failed<E>(_: E) -> Error {
        Error::InvalidConfig("resampling failed for these sample rates")
    }
    let mut resampler =
        Fft::<f32>::new(from_hz as usize, to_hz as usize, 1024, 1, 1, FixedSync::Input).map_err(failed)?;
    let n = samples.len();
    let mut out = vec![0.0f32; resampler.process_all_needed_output_len(n)];
    let out_cap = out.len();
    let input = InterleavedSlice::new(samples, 1, n).map_err(failed)?;
    let mut output = InterleavedSlice::new_mut(&mut out, 1, out_cap).map_err(failed)?;
    let (_, written) = resampler
        .process_all_into_buffer(&input, &mut output, n, None)
        .map_err(failed)?;
    out.truncate(written);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_frames_only() {
        let x = [0.0f32; 10];
        assert_eq!(frames(&x, 4, 2).unwrap().len(), 4); // starts 0,2,4,6
    }

    #[test]
    fn short_input_gives_no_frames() {
        assert!(frames(&[0.0; 3], 4, 2).unwrap().is_empty());
    }

    #[test]
    fn zero_sizes_rejected() {
        assert!(frames(&[0.0; 8], 0, 1).is_err());
        assert!(frames(&[0.0; 8], 4, 0).is_err());
    }

    fn tone(freq: f32, secs: f32, sr: u32) -> Vec<f32> {
        (0..(sr as f32 * secs) as usize)
            .map(|i| 0.5 * (2.0 * std::f32::consts::PI * freq * i as f32 / sr as f32).sin())
            .collect()
    }

    #[test]
    fn resample_48k_to_16k_keeps_length_and_frequency() {
        let out = resample(&tone(440.0, 2.0, 48_000), 48_000, 16_000).unwrap();
        assert!((out.len() as i64 - 32_000).abs() <= 2, "len {}", out.len());
        // Count rising zero crossings in the middle to estimate frequency.
        let mid = &out[2048..out.len() - 2048];
        let crossings = mid.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        let hz = crossings as f32 * 16_000.0 / mid.len() as f32;
        assert!((hz - 440.0).abs() < 2.0, "got {hz} Hz");
    }

    #[test]
    fn resample_same_rate_is_identity_and_bad_input_errors() {
        let x = tone(200.0, 0.1, 16_000);
        assert_eq!(resample(&x, 16_000, 16_000).unwrap(), x);
        assert_eq!(resample(&[], 48_000, 16_000), Err(Error::EmptyInput));
        assert!(resample(&x, 0, 16_000).is_err());
        assert!(resample(&x, 16_000, 0).is_err());
    }
}
