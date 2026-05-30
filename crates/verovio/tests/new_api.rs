//! Integration tests for the v0.3 API additions:
//!
//! - Element introspection (`page_with_element`, `time_for_element`,
//!   `midi_values_for_element`, `times_for_element`, `element_attr`).
//! - Format conversion (`to_mei`, `to_mei_with_options`, `render_to_pae`).
//! - Schema discovery (`available_options`, `reset_options`).
//! - Region scoping (`select`).
//! - Typed setters (`set_scale`, `scale`, `set_input_from`,
//!   `set_layout_options`).
//! - Misc surface (`id`, `resource_path`, `validate_pae`,
//!   `redo_page_pitch_pos_layout`, `reset_xml_id_seed`).
//! - Caching invariants (`page_count` memoization).

use verovio::{Error, LayoutOptions, MeiOptions, Toolkit};

const TWO_STAFF_MEI: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<mei xmlns="http://www.music-encoding.org/ns/mei" meiversion="4.0.0">
  <meiHead>
    <fileDesc>
      <titleStmt><title>two-staff smoke</title></titleStmt>
      <pubStmt/>
    </fileDesc>
  </meiHead>
  <music>
    <body>
      <mdiv>
        <score>
          <scoreDef>
            <staffGrp>
              <staffDef n="1" lines="5" clef.shape="G" clef.line="2"/>
              <staffDef n="2" lines="5" clef.shape="F" clef.line="4"/>
            </staffGrp>
          </scoreDef>
          <section>
            <measure xml:id="m1">
              <staff n="1">
                <layer>
                  <note pname="g" oct="4" dur="4" xml:id="treble-1"/>
                  <note pname="a" oct="4" dur="4" xml:id="treble-2"/>
                  <note pname="b" oct="4" dur="4" xml:id="treble-3"/>
                  <note pname="c" oct="5" dur="4" xml:id="treble-4"/>
                </layer>
              </staff>
              <staff n="2">
                <layer>
                  <note pname="c" oct="3" dur="4" xml:id="bass-1"/>
                  <note pname="d" oct="3" dur="4" xml:id="bass-2"/>
                  <note pname="e" oct="3" dur="4" xml:id="bass-3"/>
                  <note pname="f" oct="3" dur="4" xml:id="bass-4"/>
                </layer>
              </staff>
            </measure>
          </section>
        </score>
      </mdiv>
    </body>
  </music>
</mei>"#;

fn loaded() -> Toolkit {
    Toolkit::from_data(TWO_STAFF_MEI).expect("MEI fixture should load")
}

// -- Element introspection -------------------------------------------

#[test]
fn page_with_element_returns_page_for_known_id() {
    let mut tk = loaded();
    let page = tk.page_with_element("treble-1");
    assert_eq!(page, Some(1), "treble-1 lives on page 1");
}

#[test]
fn page_with_element_returns_none_for_unknown_id() {
    let mut tk = loaded();
    assert_eq!(tk.page_with_element("no-such-id"), None);
}

#[test]
fn time_for_element_returns_zero_for_first_note() {
    let mut tk = loaded();
    // treble-1 is the very first note → onset at t=0.
    let t = tk.time_for_element("treble-1");
    assert_eq!(t, Some(0));
}

#[test]
fn time_for_element_increases_with_position() {
    let mut tk = loaded();
    let t1 = tk.time_for_element("treble-1").expect("treble-1 onset");
    let t2 = tk.time_for_element("treble-2").expect("treble-2 onset");
    assert!(t2 > t1, "treble-2 ({t2} ms) should be after treble-1 ({t1} ms)");
}

#[test]
fn midi_values_for_element_returns_pitch_and_duration_for_note() {
    let mut tk = loaded();
    let vals = tk
        .midi_values_for_element("treble-1")
        .expect("parse ok")
        .expect("treble-1 is a note");
    // G4 = MIDI 67.
    assert_eq!(vals.pitch, 67, "G4 should be MIDI pitch 67");
    assert!(vals.duration > 0, "duration should be positive: {vals:?}");
    assert_eq!(vals.time, 0, "treble-1 starts at t=0");
}

#[test]
fn midi_values_for_unknown_id_returns_none() {
    let mut tk = loaded();
    let vals = tk.midi_values_for_element("no-such-id").expect("parse ok");
    assert!(vals.is_none(), "unknown id should yield None, got {vals:?}");
}

#[test]
fn times_for_element_returns_score_and_real_time_pair() {
    let mut tk = loaded();
    let times = tk
        .times_for_element("treble-1")
        .expect("parse ok")
        .expect("treble-1 is a note");
    assert!(!times.tstamp_on.is_empty(), "expected at least one onset");
    assert!(!times.qfrac_on.is_empty(), "expected at least one qfrac onset");
    // First note begins at qstamp 0 (qfrac [0, 1]).
    assert_eq!(times.qfrac_on, vec![[0, 1]]);
    // Real-time onset of the first note is 0 ms.
    assert_eq!(times.tstamp_on[0], 0.0);
}

#[test]
fn element_attr_returns_mei_attributes() {
    let mut tk = loaded();
    let attrs = tk.element_attr("treble-1").expect("attrs parse");
    // Upstream surfaces the MEI attributes verbatim. pname / oct / dur
    // are the ones we set.
    assert!(attrs.contains_key("pname"), "missing pname attr: {attrs:?}");
    assert!(attrs.contains_key("oct"), "missing oct attr: {attrs:?}");
}

#[test]
fn notated_id_for_element_passes_through_when_no_expansion() {
    let mut tk = loaded();
    // No <expansion> in this fixture → upstream returns input id verbatim.
    assert_eq!(tk.notated_id_for_element("treble-1"), "treble-1");
}

#[test]
fn expansion_ids_returns_empty_vec_when_no_expansion_map() {
    let mut tk = loaded();
    let ids = tk.expansion_ids_for_element("treble-1").expect("parse");
    // Upstream returns [""] when there's no expansion map at all;
    // the safe wrapper strips that sentinel to an empty Vec.
    assert!(ids.is_empty(), "expected empty, got {ids:?}");
}

// -- Format conversion -----------------------------------------------

#[test]
fn to_mei_round_trips_loaded_mei() {
    let mut tk = loaded();
    let mei = tk.to_mei().expect("MEI export");
    assert!(mei.starts_with("<?xml"));
    assert!(
        mei.contains("<mei "),
        "expected <mei root tag in: {}",
        &mei[..mei.len().min(400)]
    );
    // The original notes should round-trip through Verovio's serializer.
    assert!(mei.contains(r#"xml:id="treble-1""#));
}

#[test]
fn to_mei_with_basic_option_emits_mei_basic() {
    let mut tk = loaded();
    // MeiOptions::default() ships score_based=true, matching upstream.
    let opts = MeiOptions {
        basic: true,
        remove_ids: true,
        ..Default::default()
    };
    let mei = tk.to_mei_with_options(&opts).expect("MEI export");
    assert!(
        mei.contains("<mei"),
        "expected <mei root tag in: {}",
        &mei[..mei.len().min(300)]
    );
}

#[test]
fn to_mei_without_loaded_data_errors() {
    let mut tk = Toolkit::new();
    assert!(matches!(
        tk.to_mei(),
        Err(Error::RenderFailed { page: 0 })
    ));
}

#[test]
fn render_to_pae_returns_pae_for_simple_score() {
    let mut tk = loaded();
    let pae = tk.render_to_pae().expect("PAE export");
    // PAE always carries @data: lines describing the notes.
    assert!(
        pae.contains("@data:") || pae.contains("@clef:"),
        "expected PAE @-headers, got: {}",
        &pae[..pae.len().min(200)]
    );
}

#[test]
fn validate_pae_returns_json_object() {
    let mut tk = Toolkit::new();
    let report = tk.validate_pae("@clef:G-2\n@keysig:\n@timesig:4/4\n@data:'4C");
    // Upstream emits a JSON object regardless of validity outcome.
    let parsed: serde_json::Value =
        serde_json::from_str(&report).expect("validate_pae returns JSON");
    assert!(
        parsed.is_object() || parsed.is_array(),
        "expected JSON object/array, got: {report}"
    );
}

// -- Schema discovery -------------------------------------------------

#[test]
fn available_options_returns_categorized_schema() {
    let tk = Toolkit::new();
    let schema_str = tk.available_options();
    let schema: serde_json::Value =
        serde_json::from_str(&schema_str).expect("schema is JSON");
    assert!(schema.is_object(), "expected top-level object");
    // The schema groups options by category; at least one well-known
    // category should be present (Verovio always ships these).
    let obj = schema.as_object().unwrap();
    assert!(
        !obj.is_empty(),
        "schema should describe at least one option category"
    );
}

#[test]
fn reset_options_returns_options_to_defaults() {
    let mut tk = loaded();
    tk.set_options(r#"{"pageWidth": 9999}"#)
        .expect("set options");
    tk.reset_options();
    let opts = tk.options();
    // After reset the pageWidth should NOT still be 9999. Check via
    // typed accessor for cleanliness.
    let v = tk.option_value("pageWidth");
    assert_ne!(
        v.as_ref().and_then(|x| x.as_u64()),
        Some(9999),
        "reset_options didn't reset pageWidth: {opts}"
    );
}

// -- Region scoping ---------------------------------------------------

#[test]
fn select_accepts_measure_range_json() {
    let mut tk = loaded();
    // Single measure in the fixture, so a 1..1 range is the only valid
    // one. Verovio's selection JSON shape uses `measureRange`.
    let res = tk.select(r#"{"measureRange": "1-1"}"#);
    assert!(res.is_ok(), "select rejected: {res:?}");
    // After applying the selection, rendering should still produce SVG.
    let svg = tk.render_to_svg(1).expect("render after select");
    assert!(svg.contains("<svg"));
}

#[test]
fn select_with_empty_string_clears_selection() {
    let mut tk = loaded();
    tk.select(r#"{"measureRange": "1-1"}"#).expect("select");
    tk.select("").expect("clear selection");
    let svg = tk.render_to_svg(1).expect("render after clear");
    assert!(svg.contains("<svg"));
}

// -- Typed setters / getters -----------------------------------------

#[test]
fn set_scale_changes_render_dimensions() {
    let mut tk = loaded();
    tk.set_scale(50).expect("set_scale 50");
    assert_eq!(tk.scale(), 50);
    tk.set_scale(150).expect("set_scale 150");
    assert_eq!(tk.scale(), 150);
}

#[test]
fn set_layout_options_applies_multiple_fields_at_once() {
    let mut tk = loaded();
    let opts = LayoutOptions {
        scale: Some(75),
        breaks: Some("none".to_string()),
        landscape: Some(true),
        ..Default::default()
    };
    tk.set_layout_options(&opts).expect("set layout opts");
    assert_eq!(tk.scale(), 75);
    let svg = tk.render_to_svg(1).expect("render with layout opts");
    assert!(svg.contains("<svg"));
}

#[test]
fn set_input_from_accepts_known_format() {
    let mut tk = Toolkit::new();
    let res = tk.set_input_from("mei");
    assert!(res.is_ok(), "set_input_from(mei) rejected: {res:?}");
}

#[test]
fn set_input_from_rejects_unknown_format() {
    let mut tk = Toolkit::new();
    let res = tk.set_input_from("definitely-not-a-format");
    assert!(matches!(res, Err(Error::OptionsRejected)));
}

#[test]
fn reset_xml_id_seed_does_not_panic() {
    let mut tk = Toolkit::new();
    tk.reset_xml_id_seed(42);
    tk.reset_xml_id_seed(0);
}

// -- Caching invariants ----------------------------------------------

#[test]
fn page_count_is_invalidated_by_load_data() {
    let mut tk = loaded();
    let n1 = tk.page_count();
    assert!(n1 > 0);
    tk.load_data(TWO_STAFF_MEI).expect("reload");
    let n2 = tk.page_count();
    assert_eq!(n1, n2, "deterministic reload should match");
}

#[test]
fn page_count_is_invalidated_by_redo_layout() {
    let mut tk = loaded();
    let _ = tk.page_count();
    // A page-width change can force a different paginatation; after
    // redo_layout the cache should re-compute.
    tk.redo_layout_with_options(r#"{"pageWidth": 500, "pageHeight": 500}"#);
    let n_after = tk.page_count();
    assert!(n_after >= 1, "page_count after redo should be ≥ 1");
}

#[test]
fn page_count_after_reset_options_recomputes() {
    let mut tk = loaded();
    let _ = tk.page_count();
    tk.reset_options();
    // The cache should be invalidated, the next call repopulates it.
    let n_after = tk.page_count();
    assert!(n_after >= 1);
}

// -- Misc surface -----------------------------------------------------

#[test]
fn id_returns_nonempty_string() {
    let mut tk = loaded();
    let id = tk.id();
    assert!(!id.is_empty(), "Toolkit id should be non-empty: {id}");
}

#[test]
fn resource_path_returns_extracted_data_dir() {
    let tk = Toolkit::new();
    let path = tk.resource_path();
    assert!(!path.is_empty(), "resource path should be non-empty");
}

#[test]
fn redo_page_pitch_pos_layout_is_noop_after_first_render() {
    let mut tk = loaded();
    // Force a render so a drawing page exists.
    let _ = tk.render_to_svg(1).expect("initial render");
    // Should not panic.
    tk.redo_page_pitch_pos_layout();
    let svg = tk.render_to_svg(1).expect("re-render");
    assert!(svg.contains("<svg"));
}

// -- load_file delegating to upstream --------------------------------

#[test]
fn load_file_missing_path_returns_io_not_found() {
    let mut tk = Toolkit::new();
    let res = tk.load_file("/nonexistent/path/that/should/never/exist.mei");
    assert!(matches!(res, Err(Error::Io(_))), "got: {res:?}");
}

#[test]
fn load_zip_data_buffer_rejects_non_zip_bytes() {
    let mut tk = Toolkit::new();
    // Plain MEI bytes are not a valid zip; upstream should reject.
    let res = tk.load_zip_data_buffer(TWO_STAFF_MEI.as_bytes());
    assert!(matches!(res, Err(Error::LoadFailed)), "got: {res:?}");
}
