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
//! Benchmarked at ~**100× faster** than the FFI round-trip on a one-bar PAE
//! fixture (release build, Verovio 6.2.1, criterion `--quick`):
//!
//! | Path                            | Per call |
//! |---------------------------------|----------|
//! | `Toolkit::elements_at_time(ms)` | 2.21 µs  |
//! | `lookup::sounding_at(&tm, ms)`  | 26 ns    |
//! | `lookup::sounding_at_into(…)`   | 22 ns    |
//!
//! The gap widens as the score grows: FFI + JSON cost is fixed per call,
//! while `sounding_at` only walks events with `tstamp <= ms` and amortizes
//! across many lookups of similar timestamps.

use std::collections::BTreeSet;

use crate::{MeasureInfo, TimemapEvent, TimemapEventExact};

/// Find a measure by its MEI id in a cached `Vec<MeasureInfo>`. O(N) —
/// for occasional lookups during seek/loop UI. Cache the result if
/// querying inside a hot path.
pub fn measure_by_id<'a>(measures: &'a [MeasureInfo], id: &str) -> Option<&'a MeasureInfo> {
    measures.iter().find(|m| m.id == id)
}

/// Find the measure ID enclosing `ms` in a cached exact-timemap. Returns
/// the most recent `measure_on` whose `tstamp <= ms`, or `None` if the
/// time is before the first measure marker.
///
/// O(events ≤ ms). For the common "what measure am I in" UI query at
/// playback rate this is well under a microsecond.
pub fn measure_at_in(events: &[TimemapEventExact], ms: f64) -> Option<&str> {
    let mut current: Option<&str> = None;
    for ev in events.iter().take_while(|e| e.tstamp <= ms) {
        if let Some(m) = ev.measure_on.as_deref() {
            current = Some(m);
        }
    }
    current
}

/// Build the measure-level timeline from a cached exact-timemap.
/// Walks every event whose `measure_on` field is set and produces one
/// [`MeasureInfo`] per measure encountered. Each measure's `end_ms` /
/// `end_qfrac` is the next measure's start, or the timemap's last event
/// for the final measure.
///
/// Pairs with the upstream `includeMeasures` option that
/// [`Toolkit::timemap_exact`](crate::Toolkit::timemap_exact) sets by
/// default.
pub fn measures_from_events(events: &[TimemapEventExact]) -> Vec<MeasureInfo> {
    let mut measures: Vec<MeasureInfo> = Vec::new();
    for ev in events {
        if let Some(id) = &ev.measure_on {
            if let Some(prev) = measures.last_mut() {
                prev.end_ms = ev.tstamp;
                prev.end_qfrac = ev.qfrac;
            }
            measures.push(MeasureInfo {
                id: id.clone(),
                start_ms: ev.tstamp,
                end_ms: ev.tstamp,
                start_qfrac: ev.qfrac,
                end_qfrac: ev.qfrac,
            });
        }
    }
    if let (Some(last_event), Some(last_measure)) = (events.last(), measures.last_mut()) {
        last_measure.end_ms = last_event.tstamp;
        last_measure.end_qfrac = last_event.qfrac;
    }
    measures
}

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

/// Count of element IDs sounding at `ms` without materializing the IDs
/// themselves. Same semantics as [`sounding_at`] — useful for UI badges
/// ("3 voices playing") that don't need the actual id list.
pub fn sounding_count_at(timemap: &[TimemapEvent], ms: f64) -> usize {
    let mut active = BTreeSet::new();
    walk_to(timemap, ms, &mut active);
    active.len()
}

/// Total number of distinct element IDs that ever fire an `on` event in
/// the timemap. The score's "note count" by another name (also covers
/// chord / rest ids when those are present).
pub fn distinct_element_count(timemap: &[TimemapEvent]) -> usize {
    let mut seen = BTreeSet::new();
    for ev in timemap {
        for id in &ev.on {
            seen.insert(id.as_str());
        }
    }
    seen.len()
}

/// Element IDs that share the same onset as the *latest* `on` event with
/// `tstamp <= ms`. Models "the chord struck most recently."
///
/// Distinct from [`sounding_at`], which returns *every* id currently
/// sounding (including notes whose onsets are in the past but haven't
/// released yet). A pianist striking a four-note chord at t=1000 ms with
/// each note sustained for 2s: at t=1500, `sounding_at` returns all four;
/// `chord_at(1500)` also returns those four because they all share the
/// last onset. If a single grace note then plays at t=1600, `chord_at`
/// returns just the grace note from that point until the next onset.
pub fn chord_at(timemap: &[TimemapEvent], ms: f64) -> Vec<String> {
    timemap
        .iter()
        .rev()
        .find(|ev| ev.tstamp <= ms && !ev.on.is_empty())
        .map(|ev| ev.on.clone())
        .unwrap_or_default()
}

/// Locate the `(on_ms, off_ms)` lifespan of a single element id in the
/// timemap. Returns `None` if the id never appears in any `on` array.
/// `off_ms` falls back to the timemap's last tstamp if the id never
/// appears in any `off` array (handles ties / unresolved hangs).
pub fn note_duration(timemap: &[TimemapEvent], id: &str) -> Option<(f64, f64)> {
    let on_ms = timemap
        .iter()
        .find(|ev| ev.on.iter().any(|s| s == id))
        .map(|ev| ev.tstamp)?;
    let off_ms = timemap
        .iter()
        .find(|ev| ev.off.iter().any(|s| s == id))
        .map(|ev| ev.tstamp)
        .or_else(|| timemap.last().map(|ev| ev.tstamp))
        .unwrap_or(on_ms);
    Some((on_ms, off_ms))
}

/// Variant of [`PlaybackCursor`] that auto-wraps inside `[start_ms, end_ms)`
/// — for loop-region practice ("play measures 4-8 on repeat"). On each
/// call, if the requested `ms` exceeds `end_ms`, the cursor seeks back
/// to `start_ms` plus the overflow amount, computing the corresponding
/// active set as it goes.
///
/// Use when the consumer wants the scheduler to think in unbounded ms
/// (`now += dt`) but visually highlight notes inside a bounded region.
///
/// # Example
///
/// ```ignore
/// use verovio::lookup::LoopCursor;
///
/// let timemap = tk.timemap()?;
/// let mut loop_cursor = LoopCursor::new(&timemap, 4000.0, 8000.0);
///
/// for tick_ms in monotonic_clock_ticks() {
///     let active = loop_cursor.advance_to(tick_ms);
///     // …
/// }
/// # Ok::<(), verovio::Error>(())
/// ```
pub struct LoopCursor<'a> {
    inner: PlaybackCursor<'a>,
    start_ms: f64,
    end_ms: f64,
}

impl<'a> LoopCursor<'a> {
    /// Construct a loop cursor over `[start_ms, end_ms)`. Position starts
    /// at `start_ms`. Panics if `start_ms >= end_ms`.
    pub fn new(timemap: &'a [TimemapEvent], start_ms: f64, end_ms: f64) -> Self {
        assert!(
            start_ms < end_ms,
            "LoopCursor requires start_ms < end_ms ({start_ms} >= {end_ms})"
        );
        let mut inner = PlaybackCursor::new(timemap);
        let _ = inner.seek_to(start_ms);
        Self {
            inner,
            start_ms,
            end_ms,
        }
    }

    /// Advance to `ms`, wrapping inside `[start_ms, end_ms)` using modular
    /// arithmetic. Walks the inner cursor across the loop boundary, so
    /// `off` events at `end_ms` get released before `on` events at the
    /// new position fire.
    pub fn advance_to(&mut self, ms: f64) -> &BTreeSet<String> {
        let region = self.end_ms - self.start_ms;
        let phase = if ms <= self.start_ms {
            0.0
        } else {
            ((ms - self.start_ms) % region).max(0.0)
        };
        let target = self.start_ms + phase;
        if target < self.inner.position_ms() {
            // Wrapped — restart from the loop start.
            let _ = self.inner.seek_to(self.start_ms);
        }
        self.inner.advance_to(target)
    }

    /// Current logical position (always inside `[start_ms, end_ms)`).
    pub fn position_ms(&self) -> f64 {
        self.inner.position_ms()
    }

    /// Currently-sounding element ids inside the loop region.
    pub fn sounding(&self) -> &BTreeSet<String> {
        self.inner.sounding()
    }
}

/// Total wall-clock duration of the loaded score, in milliseconds. Returns
/// `0.0` for an empty timemap.
///
/// Equivalent to `timemap.last().map(|e| e.tstamp).unwrap_or(0.0)`, but
/// named for self-documentation in player code: `progress = now / duration`.
///
/// Note: this is the **last event's tstamp**, which for typical scores is
/// the moment the last note ends. Some scores may add trailing meta-events
/// (final barlines, etc.); the duration here reflects whatever Verovio
/// places last.
pub fn duration_ms(timemap: &[TimemapEvent]) -> f64 {
    timemap.last().map(|e| e.tstamp).unwrap_or(0.0)
}

/// Return the slice of events with `start_ms <= tstamp <= end_ms`.
/// Uses binary search on the sorted-by-tstamp invariant (`partition_point`),
/// O(log n).
///
/// Intended for loop-region playback ("play measures 4–8"): combine with
/// [`sounding_at`] called at `start_ms` to seed the initial state, then
/// step through the returned slice.
///
/// `start_ms > end_ms` returns an empty slice. `start_ms < 0` is clamped to
/// the start; `end_ms > duration` to the end.
pub fn events_in_range(timemap: &[TimemapEvent], start_ms: f64, end_ms: f64) -> &[TimemapEvent] {
    if start_ms > end_ms {
        return &[];
    }
    let start = timemap.partition_point(|e| e.tstamp < start_ms);
    let end = timemap.partition_point(|e| e.tstamp <= end_ms);
    &timemap[start..end]
}

/// Return the first event with `tstamp > ms`. Used for "step to next note"
/// UI affordances. O(log n).
pub fn next_event_after(timemap: &[TimemapEvent], ms: f64) -> Option<&TimemapEvent> {
    let idx = timemap.partition_point(|e| e.tstamp <= ms);
    timemap.get(idx)
}

/// Return the last event with `tstamp < ms`. Used for "step to previous
/// note" UI affordances. O(log n).
pub fn prev_event_before(timemap: &[TimemapEvent], ms: f64) -> Option<&TimemapEvent> {
    let idx = timemap.partition_point(|e| e.tstamp < ms);
    if idx == 0 {
        None
    } else {
        Some(&timemap[idx - 1])
    }
}

/// Stateful cursor for **monotonic playback** — advances through a cached
/// `Timemap` in amortized O(1) per tick (only processes events newly
/// crossed since the last call) instead of [`sounding_at`]'s O(events ≤ ms)
/// per query.
///
/// Use this when your playback driver advances time forward through the
/// score one tick at a time (typical audio / animation loops). For
/// arbitrary-time queries (random seeking, scrubbing), use [`sounding_at`].
///
/// # Example
///
/// ```ignore
/// use verovio::lookup::PlaybackCursor;
///
/// let timemap = tk.timemap()?;
/// let mut cursor = PlaybackCursor::new(&timemap);
///
/// for tick_ms in playback_ticks() {
///     let active = cursor.advance_to(tick_ms);
///     // … render active.iter() as highlights
/// }
/// # Ok::<(), verovio::Error>(())
/// ```
///
/// # Semantics
///
/// Matches [`sounding_at`]'s event-boundary handling: at `ms == event.tstamp`,
/// the event's `on` arrivals are sounding but its `off` departures haven't
/// released yet. A note whose `off` fires at exactly the current `ms` is
/// still in the active set.
///
/// Calling [`Self::advance_to`] with a value strictly less than the cursor's
/// current position is a `debug_assert` failure — use [`Self::seek_to`] for
/// backwards motion (which rewinds to zero and re-walks).
pub struct PlaybackCursor<'a> {
    timemap: &'a [TimemapEvent],
    /// Index of the next event to fully consume. Events with index < `next`
    /// have had both their `on` and `off` arrays applied to `sounding`.
    /// The event at index `next` may have had its `on` applied (boundary)
    /// but not its `off`.
    next: usize,
    /// The ms position the cursor was last advanced to. `f64::NEG_INFINITY`
    /// for a fresh cursor that hasn't moved yet.
    position_ms: f64,
    /// Element IDs currently sounding.
    sounding: BTreeSet<String>,
}

impl<'a> PlaybackCursor<'a> {
    /// Create a cursor positioned before the first event.
    pub fn new(timemap: &'a [TimemapEvent]) -> Self {
        Self {
            timemap,
            next: 0,
            position_ms: f64::NEG_INFINITY,
            sounding: BTreeSet::new(),
        }
    }

    /// Advance to time `ms` and return the currently-sounding element IDs.
    ///
    /// O(events crossed since last call) — amortizes to O(1) per tick over
    /// a full playback.
    ///
    /// Panics in debug builds if `ms` is strictly less than the previous
    /// position — use [`Self::seek_to`] for backwards motion.
    pub fn advance_to(&mut self, ms: f64) -> &BTreeSet<String> {
        debug_assert!(
            ms >= self.position_ms || self.position_ms == f64::NEG_INFINITY,
            "PlaybackCursor::advance_to is monotonic ({ms} < {}); use seek_to to rewind",
            self.position_ms
        );

        while self.next < self.timemap.len() {
            let ev = &self.timemap[self.next];
            if ev.tstamp < ms {
                // Strictly past: apply both off and on. Re-inserting an id
                // that's already in `sounding` is a no-op (BTreeSet dedups).
                for id in &ev.off {
                    self.sounding.remove(id);
                }
                for id in &ev.on {
                    self.sounding.insert(id.clone());
                }
                self.next += 1;
            } else if ev.tstamp == ms {
                // Boundary: apply only `on`. `off` waits until ms moves
                // forward, when this event becomes strictly past. `next`
                // stays pointing here so that next time we revisit it.
                for id in &ev.on {
                    self.sounding.insert(id.clone());
                }
                break;
            } else {
                break;
            }
        }
        self.position_ms = ms;
        &self.sounding
    }

    /// Reset and walk to `ms` from the beginning. O(events ≤ ms).
    pub fn seek_to(&mut self, ms: f64) -> &BTreeSet<String> {
        self.next = 0;
        self.position_ms = f64::NEG_INFINITY;
        self.sounding.clear();
        self.advance_to(ms)
    }

    /// Current ms position (the last value passed to `advance_to` /
    /// `seek_to`, or `f64::NEG_INFINITY` if not yet moved).
    pub fn position_ms(&self) -> f64 {
        self.position_ms
    }

    /// Currently-sounding element IDs, sorted (`BTreeSet` order).
    pub fn sounding(&self) -> &BTreeSet<String> {
        &self.sounding
    }
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
