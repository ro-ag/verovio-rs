//! Error type for fallible [`Toolkit`](crate::Toolkit) operations.

/// Error returned by safe Toolkit methods that can fail.
#[derive(Debug)]
pub enum Error {
    /// `LoadData` returned `false`. Verovio's internal log holds the reason;
    /// the log surface is not yet exposed (it is process-global and requires
    /// mutex gating per the safety contract).
    LoadFailed,
    /// `SetOptions` returned `false`. The JSON either failed to parse or named
    /// an option Verovio doesn't recognize.
    OptionsRejected,
    /// A render call returned an empty string. Typically means no document is
    /// loaded or the requested page is out of range.
    RenderFailed {
        /// 1-indexed page that was requested. `0` for whole-document renders
        /// (e.g. timemap).
        page: u32,
    },
    /// File-IO failure raised by [`Toolkit::load_file`](crate::Toolkit::load_file)
    /// / [`Toolkit::from_file`](crate::Toolkit::from_file).
    Io(std::io::Error),
    /// JSON parse failure on a typed accessor
    /// ([`Toolkit::timemap`](crate::Toolkit::timemap),
    /// [`Toolkit::elements_at`](crate::Toolkit::elements_at)). Indicates a
    /// shape mismatch between what Verovio produced and the Rust struct we
    /// expected.
    Json(serde_json::Error),
    /// Base64 decode failure inside
    /// [`Toolkit::render_to_midi_bytes`](crate::Toolkit::render_to_midi_bytes).
    /// Verovio is expected to emit well-formed base64; this would be a bug
    /// upstream rather than a user error.
    Base64(base64::DecodeError),
    /// XML parse failure inside
    /// [`Toolkit::staff_map`](crate::Toolkit::staff_map). The rendered SVG
    /// failed to parse — would be a Verovio bug, not a user error.
    Xml(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::LoadFailed => f.write_str("Verovio failed to load data"),
            Error::OptionsRejected => f.write_str("Verovio rejected options"),
            Error::RenderFailed { page: 0 } => f.write_str("Verovio render returned empty"),
            Error::RenderFailed { page } => {
                write!(f, "Verovio render returned empty for page {page}")
            }
            Error::Io(e) => write!(f, "I/O error reading score file: {e}"),
            Error::Json(e) => write!(f, "JSON parse error from Verovio output: {e}"),
            Error::Base64(e) => write!(f, "base64 decode error from Verovio MIDI output: {e}"),
            Error::Xml(s) => write!(f, "XML parse error from Verovio SVG output: {s}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            Error::Json(e) => Some(e),
            Error::Base64(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Json(e)
    }
}

impl From<base64::DecodeError> for Error {
    fn from(e: base64::DecodeError) -> Self {
        Error::Base64(e)
    }
}

/// Convenience alias for `std::result::Result<T, crate::Error>`.
pub type Result<T> = std::result::Result<T, Error>;
