//! SMuFL font and resource files for the [Verovio](https://www.verovio.org/)
//! music notation engraver.
//!
//! Verovio refuses to parse any input until `Resources::Ok()` returns true,
//! which requires the font data on disk at the path passed to
//! `Toolkit::SetResourcePath`. This crate bundles the resource tree at compile
//! time and offers [`extract`] to stage it onto a real filesystem so Verovio
//! can read it.
//!
//! The bundled contents are an exact mirror of the `data/` directory in the
//! Verovio source tree pinned by the `verovio-sys` crate. Both Bravura
//! (mandatory) and Leipzig (eager-loaded) are included, plus the optional
//! Gootville, Leland, Petaluma, and Liberation fonts.

use include_dir::{include_dir, Dir};
use std::io;
use std::path::Path;

/// Bundled resource directory — a compile-time snapshot of Verovio's
/// `data/`.
pub const DATA: Dir<'static> = include_dir!("$VEROVIO_DATA_DIR");

/// Extract all bundled resource files into `dest`. The destination is
/// expected to already exist (e.g. a freshly-created tempdir). Any files
/// already present at the same paths are overwritten.
pub fn extract(dest: &Path) -> io::Result<()> {
    DATA.extract(dest)
}

/// SMuFL font families bundled in this crate, as Verovio's `font`
/// option understands them. Useful for populating a font picker in a UI
/// without hard-coding the list at the call site.
///
/// The first entry (Bravura) is Verovio's default and the SMuFL
/// reference font.
pub const AVAILABLE_FONTS: &[&str] = &["Bravura", "Leipzig", "Gootville", "Leland", "Petaluma"];
