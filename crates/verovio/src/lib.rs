//! Safe Rust bindings to [Verovio](https://www.verovio.org/), RISM's C++ music
//! notation engraver.
//!
//! # Status
//!
//! Pre-rendering slice: [`Toolkit::new`], [`Toolkit::load_data`],
//! [`Toolkit::page_count`], and the option getters/setters are exposed.
//! `render_to_svg` and friends land once the `verovio-data` crate ships the
//! Bravura font on disk for `SetResourcePath`.
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
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::LoadFailed => f.write_str("Verovio failed to load data"),
            Error::OptionsRejected => f.write_str("Verovio rejected options"),
        }
    }
}

impl std::error::Error for Error {}

/// Result alias for fallible Toolkit operations.
pub type Result<T> = std::result::Result<T, Error>;

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

    /// Load a score document. Verovio auto-detects the format from content
    /// (MEI, MusicXML, Humdrum, ABC, PAE, ...).
    ///
    /// Returns [`Error::LoadFailed`] if Verovio's parser rejects the input.
    pub fn load_data(&mut self, data: &str) -> Result<()> {
        if ffi::load_data(self.inner.pin_mut(), data) {
            Ok(())
        } else {
            Err(Error::LoadFailed)
        }
    }

    /// Number of layout pages for the currently-loaded document.
    ///
    /// Takes `&mut self` because Verovio's `GetPageCount` is non-`const`
    /// upstream — it triggers lazy layout computation on first call.
    /// Returns `0` if no document is loaded or the upstream call returned
    /// a negative value.
    pub fn page_count(&mut self) -> u32 {
        let n = ffi::page_count(self.inner.pin_mut());
        n.max(0) as u32
    }

    /// Current option set as a JSON document.
    pub fn options(&self) -> String {
        ffi::get_options(&self.inner)
    }

    /// Default option values as a JSON document. Useful for discovering the
    /// option schema without mutating the toolkit.
    pub fn default_options(&self) -> String {
        ffi::get_default_options(&self.inner)
    }

    /// Apply the given options. `json` is a JSON document; see
    /// [`Self::default_options`] for the schema.
    ///
    /// Returns [`Error::OptionsRejected`] if Verovio fails to parse the JSON
    /// or it names an unrecognized option.
    pub fn set_options(&mut self, json: &str) -> Result<()> {
        if ffi::set_options(self.inner.pin_mut(), json) {
            Ok(())
        } else {
            Err(Error::OptionsRejected)
        }
    }
}

impl Default for Toolkit {
    fn default() -> Self {
        Self::new()
    }
}
