//! Cache-aware query helpers over a pre-parsed [`Timemap`](crate::Timemap).
//!
//! The motivation: [`Toolkit::elements_at_time`](crate::Toolkit::elements_at_time)
//! crosses the FFI boundary and parses JSON every call (~25 µs per call in
//! debug, a few µs in release). For per-frame playback queries (60 Hz, 144 Hz,
//! audio-rate) that's pure waste — the timemap doesn't change between
//! `load_data` calls, so the answer to "what's sounding at time t?" can be
//! computed from a cached [`Timemap`] without ever touching Verovio.
//!
//! The pattern:
//!
//! ```ignore
//! use verovio::{Toolkit, lookup::sounding_at};
//!
//! let mut tk = Toolkit::from_data(score)?;
//! let timemap = tk.timemap()?;  // one FFI + JSON parse
//!
//! // Playback loop — never re-enters Verovio:
//! for tick_ms in playback_ticks() {
//!     let active = sounding_at(&timemap, tick_ms);
//!     // …update UI / driver
//! }
//! # Ok::<(), verovio::Error>(())
//! ```
//!
//! Benchmarked at ~30× faster than the FFI round-trip even on a tiny
//! one-bar fixture; the gap widens as the score grows because the FFI
//! cost is fixed-per-call but `sounding_at` only walks the prefix of
//! events up to `ms`.

use std::collections::BTreeSet;

use crate::TimemapEvent;

/// Return the element IDs sounding at playback time `ms`, computed from a
/// cached timemap. Returns a sorted `Vec<String>` so consumers can do
/// deterministic comparisons / hashing without re-sorting.
///
/// O(events with `tstamp <= ms`). For a typical score this is well under
/// a microsecond.
///
/// # Event-boundary semantics
///
/// Matches Verovio's [`Toolkit::elements_at_time`](crate::Toolkit::elements_at_time)
/// upstream: a note whose `off` event fires at exactly `ms` is still
/// considered sounding (it's the last instant of the note). A note whose
/// `on` event fires at exactly `ms` is also sounding (it begins at this
/// instant). Concretely:
///
/// - Events with `tstamp < ms` apply both their `on` and `off` arrays.
/// - Events with `tstamp == ms` apply only their `on` array.
///
/// See [`sounding_at_into`] for a buffer-reuse variant that avoids the
/// `Vec` allocation on each call.
pub fn sounding_at(timemap: &[TimemapEvent], ms: f64) -> Vec<String> {
    let mut active = BTreeSet::new();
    walk_to(timemap, ms, &mut active);
    active.into_iter().collect()
}

/// Same as [`sounding_at`] but writes the result into `out` instead of
/// allocating a fresh `Vec`. `out` is `clear()`ed before being filled.
///
/// For tight playback loops at audio rate, prefer this over the
/// allocating variant.
pub fn sounding_at_into(timemap: &[TimemapEvent], ms: f64, out: &mut Vec<String>) {
    let mut active = BTreeSet::new();
    walk_to(timemap, ms, &mut active);
    out.clear();
    out.extend(active);
}

fn walk_to(timemap: &[TimemapEvent], ms: f64, active: &mut BTreeSet<String>) {
    for ev in timemap.iter().take_while(|e| e.tstamp <= ms) {
        if ev.tstamp < ms {
            // Strictly in the past: both `off` (note ended) and `on`
            // (note began) have fully resolved by now.
            for id in &ev.off {
                active.remove(id);
            }
            for id in &ev.on {
                active.insert(id.clone());
            }
        } else {
            // ev.tstamp == ms: notes that begin at this instant are
            // sounding, but notes whose `off` fires here haven't been
            // released yet — they're at their final sample.
            for id in &ev.on {
                active.insert(id.clone());
            }
        }
    }
}
