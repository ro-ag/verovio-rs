//! Typed projections of Verovio's JSON outputs.
//!
//! These structs are returned by the typed accessors
//! [`Toolkit::timemap`](crate::Toolkit::timemap) and
//! [`Toolkit::elements_at`](crate::Toolkit::elements_at). The raw JSON-string
//! variants (`render_to_timemap`, `elements_at_time`) remain available for
//! callers that want to forward the payload verbatim — e.g. across a web
//! protocol that already does its own deserialization.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Expansion map: original MEI element ID → ordered list of expanded IDs
/// as they appear in playback. An id that's played twice appears twice in
/// the value array. Empty for scores without `<expansion>` markers.
///
/// Use [`Toolkit::expansion_map`](crate::Toolkit::expansion_map) to obtain
/// one. The shape matches upstream `vrv::ExpansionMap::ToJson`.
pub type ExpansionMap = BTreeMap<String, Vec<String>>;

/// A single tempo change in a score: "from this quarter-beat onward, the
/// tempo is `bpm`."
#[derive(Debug, Clone, PartialEq)]
pub struct TempoChange {
    /// Quarter-note position where this tempo takes effect.
    pub at_qstamp: f64,
    /// Tempo in beats per minute, effective from `at_qstamp` until the
    /// next [`TempoChange`] in the parent [`TempoMap`].
    pub bpm: f64,
}

/// Ordered tempo changes through a score. Use
/// [`Toolkit::tempo_map`](crate::Toolkit::tempo_map) (or
/// [`TempoMap::from_timemap`]) to build one, then [`Self::qstamp_to_ms`] /
/// [`Self::ms_to_qstamp`] to convert between musical and wall-clock
/// positions under tempo changes.
///
/// This is the primitive consumers need when:
/// - **Driving playback under tempo overrides** — slow down for practice
///   by passing a modified `TempoMap` to your scheduler.
/// - **Converting `qfrac`-exact positions to wall-clock ms** without
///   suffering Verovio's f64 `tstamp` rounding.
/// - **Pre-computing per-tick wall-clock times** ahead of an audio loop.
#[derive(Debug, Clone, PartialEq)]
pub struct TempoMap {
    /// Sorted by `at_qstamp` ascending. The first change is at `qstamp=0`
    /// when extracted from a Verovio timemap (Verovio always publishes
    /// tempo on its first event).
    pub changes: Vec<TempoChange>,
}

impl TempoMap {
    /// Extract tempo changes from a parsed timemap. Returns `None` for an
    /// empty timemap or one without any tempo info on its first event.
    pub fn from_timemap(timemap: &[TimemapEvent]) -> Option<Self> {
        let first = timemap.first()?;
        let initial_bpm = first.tempo?;
        let mut changes = vec![TempoChange {
            at_qstamp: first.qstamp,
            bpm: initial_bpm,
        }];
        let mut last_bpm = initial_bpm;
        for ev in timemap.iter().skip(1) {
            if let Some(bpm) = ev.tempo {
                if (bpm - last_bpm).abs() > f64::EPSILON {
                    changes.push(TempoChange {
                        at_qstamp: ev.qstamp,
                        bpm,
                    });
                    last_bpm = bpm;
                }
            }
        }
        Some(Self { changes })
    }

    /// Construct from an explicit sequence of `(qstamp, bpm)` pairs. The
    /// caller is responsible for ensuring `at_qstamp` is non-decreasing.
    pub fn new(changes: Vec<TempoChange>) -> Self {
        Self { changes }
    }

    /// Convert a quarter-beat position to wall-clock milliseconds under
    /// this tempo map. Handles tempo changes correctly: each segment
    /// contributes `(q_end - q_start) * 60_000 / bpm` ms.
    pub fn qstamp_to_ms(&self, qstamp: f64) -> f64 {
        if qstamp <= 0.0 || self.changes.is_empty() {
            return 0.0;
        }
        let mut ms = 0.0;
        for i in 0..self.changes.len() {
            let q_start = self.changes[i].at_qstamp;
            let bpm = self.changes[i].bpm;
            let q_end = self
                .changes
                .get(i + 1)
                .map(|c| c.at_qstamp)
                .unwrap_or(f64::INFINITY);
            let segment_end = q_end.min(qstamp);
            if segment_end > q_start {
                ms += (segment_end - q_start) * 60_000.0 / bpm;
            }
            if segment_end >= qstamp {
                return ms;
            }
        }
        ms
    }

    /// Convert wall-clock milliseconds to a quarter-beat position — the
    /// inverse of [`Self::qstamp_to_ms`].
    pub fn ms_to_qstamp(&self, ms: f64) -> f64 {
        if ms <= 0.0 || self.changes.is_empty() {
            return 0.0;
        }
        let mut accumulated_ms = 0.0;
        let mut last_q = self.changes[0].at_qstamp;
        for i in 0..self.changes.len() {
            let q_start = self.changes[i].at_qstamp;
            let bpm = self.changes[i].bpm;
            let q_end = self
                .changes
                .get(i + 1)
                .map(|c| c.at_qstamp)
                .unwrap_or(f64::INFINITY);
            let segment_q_duration = q_end - q_start;
            let segment_ms_duration = segment_q_duration * 60_000.0 / bpm;
            if accumulated_ms + segment_ms_duration >= ms {
                let elapsed_in_segment_ms = ms - accumulated_ms;
                return q_start + elapsed_in_segment_ms / 60_000.0 * bpm;
            }
            accumulated_ms += segment_ms_duration;
            last_q = q_end;
        }
        last_q
    }
}

/// One row of the playback timemap: a moment where elements turn on or off.
///
/// `tstamp` is in **milliseconds**; `qstamp` is in **quarter-note beats**.
/// `on` / `off` are MEI element IDs (the same IDs Verovio embeds as `xml:id`
/// in the SVG output and that
/// [`Toolkit::elements_at_time`](crate::Toolkit::elements_at_time) reports).
/// `tempo` is BPM at this moment (present on the first event and any
/// subsequent tempo change).
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct TimemapEvent {
    /// Timestamp in milliseconds from the start of playback.
    pub tstamp: f64,
    /// Timestamp in quarter-note beats from the start of playback.
    pub qstamp: f64,
    /// Element IDs whose articulations begin at this moment.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub on: Vec<String>,
    /// Element IDs whose articulations end at this moment.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub off: Vec<String>,
    /// Tempo (BPM) effective from this event onward, when Verovio
    /// publishes one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tempo: Option<f64>,
}

/// The playhead-sync map for a loaded score: a chronological sequence of
/// note-on / note-off events with tempo metadata.
pub type Timemap = Vec<TimemapEvent>;

/// The elements active at a given playback time, as reported by
/// [`Toolkit::elements_at`](crate::Toolkit::elements_at).
///
/// All vec fields hold MEI element IDs (matching the `xml:id` attributes in
/// the SVG output). `measure` is the single enclosing measure ID, if any.
/// `page` is the 1-indexed page number Verovio resolved the time to.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct ElementsAtTime {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chords: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measure: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rests: Vec<String>,
}

/// Higher-precision timemap event — quarter-note position as an **exact
/// rational** instead of an f64 millisecond, with optional rest and measure
/// markers turned on.
///
/// Returned by [`Toolkit::timemap_exact`](crate::Toolkit::timemap_exact).
/// Use this when you care about accumulated precision (long scores, tight
/// rhythmic detail like tuplets at fast tempos) — the f64 `tstamp` in
/// [`TimemapEvent`] is fine for casual playback but Verovio computes it
/// from `qfrac × 60_000 / tempo` and float rounding can drift over time.
///
/// The `qfrac` pair never drifts: `[3, 2]` is exactly 1.5 quarter beats
/// regardless of how it's transported.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct TimemapEventExact {
    /// Quarter-note timestamp as `[numerator, denominator]`. Always reduced
    /// (denominator > 0). E.g. `[3, 2]` is 1.5 quarter beats from the start
    /// of playback.
    pub qfrac: [i64; 2],

    /// Verovio's f64 millisecond timestamp, computed as
    /// `(qfrac.0 / qfrac.1) × 60_000 / tempo`. Exact for simple ratios and
    /// integer tempos; may carry float rounding for irregular ratios.
    /// Recompute from your own tempo with [`Self::tstamp_ms_at_tempo`] if
    /// you need it under a tempo map you control.
    pub tstamp: f64,

    /// Element IDs whose articulations begin at this moment.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub on: Vec<String>,

    /// Element IDs whose articulations end at this moment.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub off: Vec<String>,

    /// Rest element IDs beginning at this moment. Populated when Verovio's
    /// `includeRests` option is set (which [`Toolkit::timemap_exact`] does
    /// automatically).
    #[serde(default, skip_serializing_if = "Vec::is_empty", rename = "restsOn")]
    pub rests_on: Vec<String>,

    /// Rest element IDs ending at this moment.
    #[serde(default, skip_serializing_if = "Vec::is_empty", rename = "restsOff")]
    pub rests_off: Vec<String>,

    /// Enclosing measure ID when this event marks a barline crossing.
    /// Populated when Verovio's `includeMeasures` option is set.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "measureOn")]
    pub measure_on: Option<String>,

    /// Tempo (BPM) effective from this event onward, when Verovio publishes
    /// one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tempo: Option<f64>,
}

impl TimemapEventExact {
    /// Returns the quarter-note position as an exact `(numerator, denominator)`
    /// pair, for callers that want to do rational arithmetic in their own
    /// numeric type (`num-rational`, `fraction`, hand-rolled, …).
    pub fn quarter_beats(&self) -> (i64, i64) {
        (self.qfrac[0], self.qfrac[1])
    }

    /// Compute the wall-clock ms for this event under an arbitrary tempo
    /// (BPM). Uses f64 arithmetic — `(qfrac.0 / qfrac.1) × 60_000 / bpm`.
    /// For sub-millisecond playback timing you'd want bigint or
    /// `num-rational` instead; this helper is the practical answer for
    /// `Duration::from_secs_f64` style scheduling.
    pub fn tstamp_ms_at_tempo(&self, bpm: f64) -> f64 {
        (self.qfrac[0] as f64 / self.qfrac[1] as f64) * 60_000.0 / bpm
    }
}
