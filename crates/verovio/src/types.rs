//! Typed projections of Verovio's JSON outputs.
//!
//! These structs are returned by the typed accessors
//! [`Toolkit::timemap`](crate::Toolkit::timemap) and
//! [`Toolkit::elements_at`](crate::Toolkit::elements_at). The raw JSON-string
//! variants (`render_to_timemap`, `elements_at_time`) remain available for
//! callers that want to forward the payload verbatim — e.g. across a web
//! protocol that already does its own deserialization.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

/// Structural classification of a single MEI element ID, as reported by
/// [`Toolkit::classified_elements`](crate::Toolkit::classified_elements).
///
/// Lets a playback driver filter highlights by element type ("color only
/// notes; ignore the chord-frame and rest IDs").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum ElementKind {
    /// A pitched note.
    Note,
    /// A chord container holding several notes.
    Chord,
    /// A rest of any duration.
    Rest,
    /// A measure / barline ID.
    Measure,
}

/// Side-table mapping element ID → its [`ElementKind`].
///
/// Build with [`Toolkit::classified_elements`](crate::Toolkit::classified_elements).
/// Pre-computed once per loaded document; lookup is O(1) `HashMap` access.
pub type ClassifiedElements = HashMap<String, ElementKind>;

/// Typed wrapper over the small subset of Verovio options that affect MIDI
/// generation. Apply with
/// [`Toolkit::set_midi_options`](crate::Toolkit::set_midi_options).
///
/// **What's NOT here:** per-staff instrument override, per-track mute, and
/// per-track volume. Verovio's upstream MIDI surface doesn't expose those
/// today; consumers needing them have to post-process the SMF emitted by
/// [`Toolkit::render_to_midi_bytes`](crate::Toolkit::render_to_midi_bytes).
/// See the `project-multi-track-timemap-gap` memory for the broader
/// upstream-constraint context.
#[derive(Debug, Clone, PartialEq)]
pub struct MidiOptions {
    /// Multiplier applied to every tempo in the score before MIDI export.
    /// `0.5` plays at half-speed (typical practice setting), `2.0` plays
    /// at double-speed. Verovio's default is `1.0`.
    pub tempo_adjustment: f64,

    /// If `true`, omit cue-sized notes from the MIDI output. Maps to
    /// `midiNoCue: true` in Verovio's options JSON. Default: `false`.
    pub omit_cue_notes: bool,
}

impl Default for MidiOptions {
    fn default() -> Self {
        Self {
            tempo_adjustment: 1.0,
            omit_cue_notes: false,
        }
    }
}

/// Axis-aligned bounding box in Verovio's SVG coordinate system (the
/// outer `<svg viewBox="0 0 W H">`'s coordinate space — typically a few
/// thousand units per page width). Use for click-to-seek hit testing
/// and "highlight box around currently playing note" overlays.
///
/// Produce with [`Toolkit::bbox_map`](crate::Toolkit::bbox_map).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BBox {
    /// Top-left x coordinate in SVG viewBox units.
    pub x: f64,
    /// Top-left y coordinate in SVG viewBox units.
    pub y: f64,
    /// Width in SVG viewBox units.
    pub width: f64,
    /// Height in SVG viewBox units.
    pub height: f64,
    /// 1-indexed page number the element lives on.
    pub page: u32,
}

impl BBox {
    /// Whether `(x, y)` falls inside the bbox. Used for hit testing
    /// click events: walk the bbox map looking for the first element
    /// that contains the click point.
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x <= self.x + self.width && y >= self.y && y <= self.y + self.height
    }
}

/// Score-level metadata extracted from the loaded MEI or MusicXML input.
///
/// Produce with [`Toolkit::metadata`](crate::Toolkit::metadata). All
/// fields are `None` / empty when the source format doesn't carry the
/// information (PAE / ABC / inline plaintext fixtures typically yield a
/// near-empty struct).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScoreMetadata {
    /// Work title (`<title>` in MEI, `<work-title>` in MusicXML).
    pub title: Option<String>,
    /// Composer name. First `<persName role="composer">` (MEI) or
    /// `<creator type="composer">` (MusicXML).
    pub composer: Option<String>,
    /// Lyricist / librettist name.
    pub lyricist: Option<String>,
    /// Arranger name.
    pub arranger: Option<String>,
    /// Copyright / availability text.
    pub copyright: Option<String>,
    /// Instrument labels in score order — for MEI this comes from
    /// `<staffDef>/<label>`, for MusicXML from `<part-name>`. Empty
    /// for formats that don't carry instrument metadata.
    pub instruments: Vec<String>,
}

/// Summary of one measure across the loaded score's timeline.
///
/// Build with [`Toolkit::measures`](crate::Toolkit::measures) or
/// [`crate::lookup::measures_from_events`]. Used by player UIs that need
/// to display "Measure N" indicators, support loop-region selection
/// ("loop measures 4-8"), or seek by measure rather than by raw time.
#[derive(Debug, Clone, PartialEq)]
pub struct MeasureInfo {
    /// MEI element ID of the measure.
    pub id: String,
    /// Wall-clock ms when this measure begins.
    pub start_ms: f64,
    /// Wall-clock ms when the next measure begins, or the timemap's last
    /// tstamp for the final measure.
    pub end_ms: f64,
    /// Quarter-beat position where this measure begins, as exact rational
    /// `[numerator, denominator]`.
    pub start_qfrac: [i64; 2],
    /// Quarter-beat position where the next measure begins (or the score's
    /// end), as exact rational `[numerator, denominator]`.
    pub end_qfrac: [i64; 2],
}

impl MidiOptions {
    /// Render to the JSON shape Verovio expects.
    pub(crate) fn to_json(&self) -> String {
        format!(
            r#"{{"midiTempoAdjustment": {}, "midiNoCue": {}}}"#,
            self.tempo_adjustment, self.omit_cue_notes
        )
    }
}

/// Typed wrapper over Verovio's SVG-output options. Apply with
/// [`Toolkit::set_svg_options`](crate::Toolkit::set_svg_options).
///
/// # CSS targets in Verovio's rendered SVG
///
/// Verovio wraps every engraved element in a `<g>` with both an
/// `id="MEI-element-id"` and a `class="kind"`. The class set you can
/// reliably target includes: `note`, `notehead`, `stem`, `chord`,
/// `rest`, `measure`, `staff`, `layer`, `beam`, `tie`, `slur`, `clef`,
/// `keysig`, `metersig`, `barline`. Most elements compose: a note's
/// notehead is `<g class="note">` ⊇ `<g class="notehead">`.
///
/// Use the [`Self::css`] field to inject a CSS block. Common recipes:
///
/// ```css
/// /* page background */
/// svg { background: #fafaf5; }
/// /* default note color */
/// g.note { fill: #14213d; }
/// /* highlighting via a runtime-toggled class */
/// g.note.playing { fill: #fca311; }
/// /* faded measures */
/// g.measure line { stroke: #999; }
/// ```
///
/// For runtime highlighting, the consumer pattern is: bind a CSS class
/// to a transient state (`.playing`, `.selected`) and toggle that class
/// on the `<g>` element via the DOM. The SVG IDs match the MEI element
/// IDs that [`Toolkit::timemap`](crate::Toolkit::timemap) and
/// [`crate::lookup::sounding_at`] return.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SvgOptions {
    /// CSS block embedded inside the rendered SVG. Empty by default.
    /// Use the class/id targets documented on this struct.
    pub css: String,
    /// Overlay element bounding-box rectangles — useful for debugging
    /// layout but visually noisy.
    pub show_bounding_boxes: bool,
    /// Overlay content-only bounding boxes (engraved content, not page
    /// margins).
    pub show_content_bounding_boxes: bool,
    /// Emit minified SVG (no extra whitespace). Useful for serving over
    /// a network where bytes matter.
    pub format_raw: bool,
    /// Emit HTML5-style SVG instead of XML-strict — relaxes some
    /// namespace requirements for inline-in-HTML use.
    pub html5: bool,
    /// Drop the `xlink:` namespace prefix (Verovio uses it for `<use
    /// xlink:href="…">` glyph references). Needed for some HTML5
    /// embedding scenarios.
    pub remove_xlink: bool,
}

impl SvgOptions {
    /// Render to the JSON shape Verovio expects.
    pub(crate) fn to_json(&self) -> String {
        // `css` may contain quotes/backslashes — escape minimally.
        let css_escaped = self
            .css
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n");
        format!(
            r#"{{"svgCss": "{}", "svgBoundingBoxes": {}, "svgContentBoundingBoxes": {}, "svgFormatRaw": {}, "svgHtml5": {}, "svgRemoveXlink": {}}}"#,
            css_escaped,
            self.show_bounding_boxes,
            self.show_content_bounding_boxes,
            self.format_raw,
            self.html5,
            self.remove_xlink,
        )
    }
}

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

    /// Returns a new tempo map with every BPM multiplied by `factor`.
    /// `factor < 1.0` slows playback; `> 1.0` speeds it up; `1.0` is a
    /// no-op (returns a clone). Useful for "play at 50% for practice"
    /// without mutating the original cached map.
    ///
    /// Non-positive factors clamp to a near-zero BPM to avoid division
    /// by zero in [`Self::qstamp_to_ms`].
    pub fn scaled(&self, factor: f64) -> Self {
        let f = factor.max(1e-9);
        let changes = self
            .changes
            .iter()
            .map(|c| TempoChange {
                at_qstamp: c.at_qstamp,
                bpm: c.bpm * f,
            })
            .collect();
        Self { changes }
    }

    /// Tempo (BPM) in effect at the given quarter-beat position. Returns
    /// the BPM of the latest tempo change with `at_qstamp <= q`, or `None`
    /// if the map is empty or `q` precedes the first tempo change.
    pub fn bpm_at_qstamp(&self, q: f64) -> Option<f64> {
        let mut current: Option<f64> = None;
        for change in &self.changes {
            if change.at_qstamp <= q {
                current = Some(change.bpm);
            } else {
                break;
            }
        }
        current
    }

    /// Tempo (BPM) in effect at the given wall-clock position. Equivalent
    /// to `bpm_at_qstamp(ms_to_qstamp(ms))` but avoids the round-trip
    /// through quarter beats.
    pub fn bpm_at_ms(&self, ms: f64) -> Option<f64> {
        if self.changes.is_empty() {
            return None;
        }
        let mut accumulated_ms = 0.0;
        for i in 0..self.changes.len() {
            let q_start = self.changes[i].at_qstamp;
            let bpm = self.changes[i].bpm;
            let q_end = self
                .changes
                .get(i + 1)
                .map(|c| c.at_qstamp)
                .unwrap_or(f64::INFINITY);
            let segment_ms_duration = (q_end - q_start) * 60_000.0 / bpm;
            // Last segment extends to infinity, so this branch always
            // matches eventually — the loop is guaranteed to return.
            if ms < accumulated_ms + segment_ms_duration {
                return Some(bpm);
            }
            accumulated_ms += segment_ms_duration;
        }
        Some(self.changes.last()?.bpm)
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
    /// `includeRests` option is set (which [`crate::Toolkit::timemap_exact`] does
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

/// MIDI values for a single note element, as reported by
/// [`Toolkit::midi_values_for_element`](crate::Toolkit::midi_values_for_element).
///
/// Upstream emits an empty object for non-note elements (chord wrappers,
/// rests, measures, missing IDs); we surface that as `None` from the safe
/// wrapper so the type only exists when a note was actually found.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct MidiValues {
    /// Wall-clock millisecond onset of the note.
    pub time: i32,
    /// MIDI pitch number (0-127, middle C = 60).
    pub pitch: i32,
    /// Note duration in milliseconds.
    pub duration: i32,
}

/// Score-time and wall-clock onset/offset for a single note, as reported
/// by [`Toolkit::times_for_element`](crate::Toolkit::times_for_element).
///
/// Each field is an array upstream — Verovio plans to support repeats /
/// expansion clones per element, but currently each array carries at most
/// one entry. `qfrac_*` fields are exact `[numerator, denominator]`
/// quarter-beat positions; `tstamp_*` fields are f64 milliseconds.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct ElementTimes {
    /// Score-time onset(s) as exact `[num, den]` quarter-beat fractions.
    #[serde(default, rename = "qfracOn")]
    pub qfrac_on: Vec<[i64; 2]>,
    /// Score-time offset(s) as exact `[num, den]` quarter-beat fractions.
    #[serde(default, rename = "qfracOff")]
    pub qfrac_off: Vec<[i64; 2]>,
    /// Untied duration(s) as exact `[num, den]` quarter-beat fractions.
    #[serde(default, rename = "qfracDuration")]
    pub qfrac_duration: Vec<[i64; 2]>,
    /// Total duration including following ties.
    #[serde(default, rename = "qfracTiedDuration")]
    pub qfrac_tied_duration: Vec<[i64; 2]>,
    /// Wall-clock millisecond onset(s).
    #[serde(default, rename = "tstampOn")]
    pub tstamp_on: Vec<f64>,
    /// Wall-clock millisecond offset(s).
    #[serde(default, rename = "tstampOff")]
    pub tstamp_off: Vec<f64>,
}

/// Typed wrapper for `GetMEI`'s JSON options. Apply with
/// [`Toolkit::to_mei_with_options`](crate::Toolkit::to_mei_with_options) — or
/// use [`Toolkit::to_mei`] for the all-defaults path.
#[derive(Debug, Clone, PartialEq)]
pub struct MeiOptions {
    /// 1-based page number to export. `None` (or `0`) exports the whole
    /// document.
    pub page_no: Option<u32>,
    /// Score-based MEI (true) vs page-based MEI (false). Default true,
    /// matching upstream's default; required for [`Self::basic`].
    pub score_based: bool,
    /// Emit MEI-Basic (a restricted subset). Default false. Requires
    /// [`Self::score_based`] = true upstream — page-based MEI Basic
    /// is not supported.
    pub basic: bool,
    /// Strip `@xml:id` attributes Verovio added but doesn't reference.
    /// Default false.
    pub remove_ids: bool,
}

impl Default for MeiOptions {
    fn default() -> Self {
        Self {
            page_no: None,
            score_based: true,
            basic: false,
            remove_ids: false,
        }
    }
}

impl MeiOptions {
    /// Render to the JSON shape Verovio expects.
    pub(crate) fn to_json(&self) -> String {
        let page_no = self.page_no.unwrap_or(0);
        format!(
            r#"{{"pageNo": {page_no}, "scoreBased": {sb}, "basic": {b}, "removeIds": {ri}}}"#,
            sb = self.score_based,
            b = self.basic,
            ri = self.remove_ids,
        )
    }
}

/// Typed wrapper batching the most commonly tweaked layout options into
/// one [`Toolkit::set_options`](crate::Toolkit::set_options) call. Each
/// `None` field is omitted from the emitted JSON so unset fields keep
/// their previous values.
///
/// Apply with [`Toolkit::set_layout_options`](crate::Toolkit::set_layout_options).
/// One call = one Verovio layout invalidation, vs N for the per-field
/// `set_font` / `set_zoom` / … helpers.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LayoutOptions {
    /// SMuFL engraving font directory name (e.g. `"Bravura"`, `"Leipzig"`).
    pub font: Option<String>,
    /// Scale as a percent (`100` = 1×, `200` = 2×).
    pub scale: Option<u32>,
    /// Page width in Verovio's internal units.
    pub page_width: Option<u32>,
    /// Page height in Verovio's internal units.
    pub page_height: Option<u32>,
    /// `breaks` mode — one of `"auto"`, `"none"`, `"encoded"`, `"smart"`,
    /// `"line"`. Other strings are rejected by Verovio.
    pub breaks: Option<String>,
    /// Swap page width/height on the next layout pass.
    pub landscape: Option<bool>,
    /// 1-based first measure to render (clamped). `Some("0")` ≡ none.
    pub measure_from: Option<String>,
    /// 1-based last measure to render (clamped).
    pub measure_to: Option<String>,
}

impl LayoutOptions {
    /// Render to the JSON shape Verovio expects — only set fields appear.
    pub(crate) fn to_json(&self) -> String {
        let mut fields: Vec<String> = Vec::new();
        if let Some(font) = &self.font {
            let escaped = font.replace('\\', "\\\\").replace('"', "\\\"");
            fields.push(format!(r#""font": "{escaped}""#));
        }
        if let Some(scale) = self.scale {
            fields.push(format!(r#""scale": {scale}"#));
        }
        if let Some(w) = self.page_width {
            fields.push(format!(r#""pageWidth": {w}"#));
        }
        if let Some(h) = self.page_height {
            fields.push(format!(r#""pageHeight": {h}"#));
        }
        if let Some(breaks) = &self.breaks {
            let escaped = breaks.replace('\\', "\\\\").replace('"', "\\\"");
            fields.push(format!(r#""breaks": "{escaped}""#));
        }
        if let Some(landscape) = self.landscape {
            fields.push(format!(r#""landscape": {landscape}"#));
        }
        if let Some(from) = &self.measure_from {
            let escaped = from.replace('\\', "\\\\").replace('"', "\\\"");
            fields.push(format!(r#""measureFrom": "{escaped}""#));
        }
        if let Some(to) = &self.measure_to {
            let escaped = to.replace('\\', "\\\\").replace('"', "\\\"");
            fields.push(format!(r#""measureTo": "{escaped}""#));
        }
        format!("{{{}}}", fields.join(", "))
    }
}
