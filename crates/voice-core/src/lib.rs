//! voice-core: offline measurement of how someone speaks.
//!
//! Pre-alpha. See `docs/spec.md` in the repository for the planned scope.

pub mod dsp;
pub mod loudness;
pub mod pace;
pub mod pauses;
pub mod pitch;
pub mod quality;
pub mod report;
pub mod stream;

pub use report::{analyze, VoiceReport};

/// Errors returned by analysis functions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The input contained no samples.
    EmptyInput,
    /// A configuration value was invalid (zero rate, zero frame length, ...).
    InvalidConfig(&'static str),
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::EmptyInput => write!(f, "input contained no samples"),
            Error::InvalidConfig(m) => write!(f, "invalid configuration: {m}"),
        }
    }
}

impl std::error::Error for Error {}
