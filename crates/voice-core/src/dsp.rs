//! Framing helpers.

use crate::Error;

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
}
