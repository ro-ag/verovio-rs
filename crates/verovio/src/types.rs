//! Typed projections of Verovio's JSON outputs.
//!
//! These structs are returned by the typed accessors
//! [`Toolkit::timemap`](crate::Toolkit::timemap) and
//! [`Toolkit::elements_at`](crate::Toolkit::elements_at). The raw JSON-string
//! variants (`render_to_timemap`, `elements_at_time`) remain available for
//! callers that want to forward the payload verbatim — e.g. across a web
//! protocol that already does its own deserialization.

use serde::{Deserialize, Serialize};

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
