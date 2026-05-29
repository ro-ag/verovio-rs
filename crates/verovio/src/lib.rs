//! Safe Rust bindings to [Verovio](https://www.verovio.org/), RISM's C++ music
//! notation engraver.
//!
//! # Status
//!
//! API surface for xpart's needs: [`Toolkit::new`], [`Toolkit::load_data`],
//! [`Toolkit::page_count`], the option getters/setters, the rendering surface
//! ([`Toolkit::render_to_svg`], [`Toolkit::render_to_timemap`],
//! [`Toolkit::redo_layout`], [`Toolkit::elements_at_time`]), plus `_into`
//! buffer-reuse variants for every allocating method.
//!
//! # Resource files
//!
//! On first [`Toolkit::new`] call, the [`verovio_data`] crate's bundled SMuFL
//! resources are extracted to a process-lifetime temporary directory and
//! handed to Verovio via `SetResourcePath`. Subsequent toolkit constructions
//! reuse the same extraction. Verovio refuses to parse any input until
//! resources are available.
//!
//! # Thread safety
//!
//! [`Toolkit`] is `Send` but not `Sync`. Verovio's render/layout methods mutate
//! internal state even when shaped as read calls; sharing a `&Toolkit` between
//! threads is unsound. For concurrent rendering, construct one `Toolkit` per
//! thread or use a single worker thread fronted by a channel.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use cxx::UniquePtr;
use verovio_sys::ffi;

/// Stage the bundled `verovio-data` resources into a process-lifetime tempdir
/// the first time the toolkit is constructed; reuse on subsequent calls.
///
/// The tempdir is intentionally leaked (`TempDir::keep`) — it lives for the
/// process lifetime so any `Toolkit` that points at it stays valid.
fn resource_path() -> &'static Path {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        let tmp = tempfile::Builder::new()
            .prefix("verovio-data-")
            .tempdir()
            .expect("failed to create tempdir for Verovio resources");
        verovio_data::extract(tmp.path()).expect("failed to extract bundled Verovio resources");
        tmp.keep()
    })
    .as_path()
}

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
    /// A render call returned an empty string. Typically means no document is
    /// loaded or the requested page is out of range.
    RenderFailed {
        /// 1-indexed page that was requested. `0` for whole-document renders
        /// (e.g. timemap).
        page: u32,
    },
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
        }
    }
}

impl std::error::Error for Error {}

/// Result alias for fallible Toolkit operations.
pub type Result<T> = std::result::Result<T, Error>;

impl Toolkit {
    /// Construct a new toolkit.
    ///
    /// Stages the bundled `verovio-data` SMuFL resources on disk (once per
    /// process) and points Verovio at them via `SetResourcePath`. Returns a
    /// toolkit that is ready to accept [`Self::load_data`].
    ///
    /// # Panics
    ///
    /// Panics if the bundled resources can't be extracted to a tempdir or
    /// Verovio rejects the resource path. Both are environment-level failures
    /// (e.g. read-only `$TMPDIR`) — there's nothing meaningful a caller could
    /// do with a `Result` here.
    pub fn new() -> Self {
        let mut inner = ffi::new_toolkit(false);
        let path = resource_path()
            .to_str()
            .expect("Verovio resource path must be UTF-8");
        let ok = ffi::set_resource_path(inner.pin_mut(), path);
        assert!(ok, "Verovio rejected SetResourcePath({path})");
        Self { inner }
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

    /// Render a single page to SVG. `page` is 1-indexed (Verovio's convention).
    ///
    /// Returns [`Error::RenderFailed`] if no document is loaded or the
    /// requested page is out of range. (Upstream's degenerate-but-valid
    /// `<svg width="0px" …>` response is detected via the page-count check;
    /// the layout pass it triggers is cached after the first call.)
    pub fn render_to_svg(&mut self, page: u32) -> Result<String> {
        if page == 0 || page > self.page_count() {
            return Err(Error::RenderFailed { page });
        }
        Ok(ffi::render_to_svg(self.inner.pin_mut(), page as i32, false))
    }

    /// Render a single page to SVG, reusing the caller's buffer.
    ///
    /// `out` is cleared then filled. The C++ side still allocates its own
    /// `std::string` per call (Verovio has no streaming overload), but
    /// repeated calls in a render loop avoid Rust-side `String` reallocation.
    pub fn render_to_svg_into(&mut self, page: u32, out: &mut String) -> Result<()> {
        out.clear();
        if page == 0 || page > self.page_count() {
            return Err(Error::RenderFailed { page });
        }
        let svg = ffi::render_to_svg(self.inner.pin_mut(), page as i32, false);
        out.push_str(&svg);
        Ok(())
    }

    /// Render the document's playback timemap as a JSON string.
    ///
    /// The timemap is the playhead-sync map xpart needs:
    /// `[{tstamp_ms, on:[ids], off:[ids]}, ...]`. Parse with `serde_json`.
    pub fn render_to_timemap(&mut self) -> Result<String> {
        let json = ffi::render_to_timemap(self.inner.pin_mut(), "");
        if json.is_empty() {
            Err(Error::RenderFailed { page: 0 })
        } else {
            Ok(json)
        }
    }

    /// Render the timemap, reusing the caller's buffer.
    pub fn render_to_timemap_into(&mut self, out: &mut String) -> Result<()> {
        out.clear();
        let json = ffi::render_to_timemap(self.inner.pin_mut(), "");
        if json.is_empty() {
            return Err(Error::RenderFailed { page: 0 });
        }
        out.push_str(&json);
        Ok(())
    }

    /// Force a layout pass on the currently-loaded document.
    ///
    /// Layout happens lazily on the first render call; explicit
    /// `redo_layout` is only needed after option changes that affect layout.
    pub fn redo_layout(&mut self) {
        ffi::redo_layout(self.inner.pin_mut(), "");
    }

    /// Force a layout pass with a JSON options overlay applied for this
    /// pass only.
    pub fn redo_layout_with_options(&mut self, options: &str) {
        ffi::redo_layout(self.inner.pin_mut(), options);
    }

    /// Return the element IDs active at the given playback time, as a JSON
    /// document. The shape upstream is roughly `{notes: [...], page: N}`.
    /// Parse with `serde_json`.
    pub fn elements_at_time(&mut self, millis: u32) -> String {
        ffi::get_elements_at_time(self.inner.pin_mut(), millis as i32)
    }

    /// Return the element IDs active at the given playback time, written
    /// into the caller's buffer.
    pub fn elements_at_time_into(&mut self, millis: u32, out: &mut String) {
        out.clear();
        let json = ffi::get_elements_at_time(self.inner.pin_mut(), millis as i32);
        out.push_str(&json);
    }
}

impl Default for Toolkit {
    fn default() -> Self {
        Self::new()
    }
}
