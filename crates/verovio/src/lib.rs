//! Safe Rust bindings to [Verovio](https://www.verovio.org/), RISM's C++ music
//! notation engraver.
//!
//! # Quick start
//!
//! ```
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
//! API surface for xpart's needs: [`Toolkit::new`], [`Toolkit::load_data`],
//! [`Toolkit::page_count`], the option getters/setters, the rendering surface
//! ([`Toolkit::render_to_svg`], [`Toolkit::render_to_timemap`],
//! [`Toolkit::redo_layout`], [`Toolkit::elements_at_time`]), plus `_into`
//! buffer-reuse variants for every allocating method, plus typed JSON access
//! through [`Toolkit::timemap`] and [`Toolkit::elements_at`].
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

mod error;
mod log;
mod toolkit;
mod types;

pub use error::{Error, Result};
pub use log::{set_log_level, LogLevel};
pub use toolkit::Toolkit;
pub use types::{ElementsAtTime, Timemap, TimemapEvent, TimemapEventExact};
