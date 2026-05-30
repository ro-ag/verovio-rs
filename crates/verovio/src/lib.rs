//! Safe Rust bindings to [Verovio](https://www.verovio.org/), RISM's C++ music
//! notation engraver.
//!
//! # Quick start
//!
//! ```no_run
//! use verovio::Toolkit;
//!
//! let mut tk = Toolkit::new();
//! tk.load_data(
//!     "@start:s\n@clef:G-2\n@keysig:xF\n@key:\n@timesig:\n@data:'4G/4-\n@end:s\n"
//! )?;
//!
//! for page in 1..=tk.page_count() {
//!     let svg = tk.render_to_svg(page)?;
//!     // … write to disk or render in a UI
//!     # let _ = svg;
//! }
//! # Ok::<(), verovio::Error>(())
//! ```
//!
//! See also [`Toolkit::render_to_svg_into`] for the buffer-reuse variant
//! recommended for tight render loops.
//!
//! # Status
//!
//! Saturated `Toolkit` surface — every upstream method that's safe under
//! the project's contract (Humdrum, `SetLocale`, unmutexed `GetLog`
//! deliberately excluded):
//!
//! - **Loading**: [`Toolkit::load_data`], [`Toolkit::load_file`] (delegates
//!   to upstream, handles UTF-16 + `.mxl`), [`Toolkit::load_zip_data_buffer`].
//! - **Rendering**: [`Toolkit::render_to_svg`] / [`Toolkit::render_to_midi_bytes`]
//!   (primary) / [`Toolkit::render_to_timemap`] / [`Toolkit::render_to_pae`],
//!   plus `_into` buffer-reuse and `_writer` streaming variants.
//! - **Format conversion**: [`Toolkit::to_mei`] /
//!   [`Toolkit::to_mei_with_options`], [`Toolkit::validate_pae`].
//! - **Element introspection** (inverse of `elements_at_time`):
//!   [`Toolkit::page_with_element`], [`Toolkit::time_for_element`],
//!   [`Toolkit::times_for_element`], [`Toolkit::midi_values_for_element`],
//!   [`Toolkit::element_attr`], [`Toolkit::notated_id_for_element`],
//!   [`Toolkit::expansion_ids_for_element`].
//! - **Options**: [`Toolkit::available_options`] (schema),
//!   [`Toolkit::reset_options`], [`Toolkit::select`],
//!   [`Toolkit::set_layout_options`], typed [`Toolkit::set_scale`] /
//!   [`Toolkit::scale`], [`Toolkit::set_input_from`] /
//!   [`Toolkit::set_output_to`], [`Toolkit::reset_xml_id_seed`].
//! - **Score reading** (Rust-side, not in upstream `Toolkit`):
//!   [`Toolkit::metadata`], [`Toolkit::measures`], [`Toolkit::staff_map`],
//!   [`Toolkit::bbox_map`], [`Toolkit::classified_elements`],
//!   [`Toolkit::tempo_map`], [`Toolkit::expansion_map`].
//! - **Diagnostics**: [`Toolkit::id`], [`Toolkit::resource_path`],
//!   [`Toolkit::version`], [`Toolkit::page_count`] (cached after first call).
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

#[cfg(feature = "audio")]
pub mod audio;
mod error;
mod log;
pub mod lookup;
pub mod midi;
#[cfg(any(feature = "png", feature = "pdf"))]
pub mod raster;
pub mod styling;
mod toolkit;
mod types;

pub use error::{Error, Result};
pub use log::{set_log_level, LogLevel};
pub use toolkit::Toolkit;
pub use types::{
    BBox, ClassifiedElements, ElementKind, ElementTimes, ElementsAtTime, ExpansionMap,
    LayoutOptions, MeasureInfo, MeiOptions, MidiOptions, MidiValues, ScoreMetadata, SvgOptions,
    TempoChange, TempoMap, Timemap, TimemapEvent, TimemapEventExact,
};
