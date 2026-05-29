//! Safe Rust bindings to [Verovio](https://www.verovio.org/), RISM's C++ music
//! notation engraver.
//!
//! This crate is in its first vertical slice — only `Toolkit::version()` is
//! exposed. The full API surface will land as it is wired through the cxx
//! bridge.
//!
//! # Thread safety
//!
//! [`Toolkit`] is `Send` but not `Sync`. Verovio's render/layout methods mutate
//! internal state even when shaped as read calls; sharing a `&Toolkit` between
//! threads is unsound. For concurrent rendering, construct one `Toolkit` per
//! thread or use a single worker thread fronted by a channel.

use cxx::UniquePtr;
use verovio_sys::ffi;

/// A Verovio engraving toolkit.
///
/// One toolkit owns an MEI document, an option set, and the render state for
/// that document. Construct one per score you want to engrave.
pub struct Toolkit {
    inner: UniquePtr<ffi::Toolkit>,
}

// SAFETY: `Toolkit` is `Send` only because this crate deliberately does *not*
// expose any Verovio surface that touches process-global state — specifically
// no Humdrum methods (would race on `static Toolkit::m_humdrumBuffer`), no log
// methods without mutex gating, and no `SetLocale`. See the safety contract.
unsafe impl Send for Toolkit {}

impl Toolkit {
    /// Construct a new toolkit without initializing the SMuFL font registry.
    ///
    /// Font init requires resource files staged on disk; until the
    /// `verovio-data` crate ships, callers must arrange resources themselves
    /// before any rendering.
    pub fn new() -> Self {
        Self {
            inner: ffi::new_toolkit(false),
        }
    }

    /// Return the upstream Verovio version string (e.g. `"6.2.1"`).
    pub fn version(&self) -> String {
        ffi::get_version(&self.inner)
    }
}

impl Default for Toolkit {
    fn default() -> Self {
        Self::new()
    }
}
