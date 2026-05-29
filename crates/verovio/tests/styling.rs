//! Tests for the typed option wrappers: [`MidiOptions`] and [`SvgOptions`].

use verovio::{MidiOptions, SvgOptions, Toolkit};

const SAMPLE_PAE: &str = "\
@start:s
@clef:G-2
@keysig:xF
@key:
@timesig:
@data:'4G/4-
@end:s
";

// -- MidiOptions ------------------------------------------------------------

#[test]
fn midi_options_default_is_identity() {
    let opts = MidiOptions::default();
    assert_eq!(opts.tempo_adjustment, 1.0);
    assert!(!opts.omit_cue_notes);
}

#[test]
fn midi_tempo_adjustment_changes_timemap_tstamps() {
    let mut tk_normal = Toolkit::from_data(SAMPLE_PAE).expect("PAE load");
    let normal = tk_normal.timemap().expect("timemap");

    let mut tk_slow = Toolkit::from_data(SAMPLE_PAE).expect("PAE load");
    tk_slow
        .set_midi_options(&MidiOptions {
            tempo_adjustment: 0.5,
            ..MidiOptions::default()
        })
        .expect("set midi options");
    let slow = tk_slow.timemap().expect("slow timemap");

    // 0.5× tempo means every tstamp is 2× larger.
    for (a, b) in normal.iter().zip(slow.iter()) {
        if a.tstamp == 0.0 {
            assert_eq!(b.tstamp, 0.0);
            continue;
        }
        let ratio = b.tstamp / a.tstamp;
        assert!(
            (ratio - 2.0).abs() < 0.01,
            "expected 2× tstamps at half tempo, got ratio={ratio} ({} vs {})",
            a.tstamp,
            b.tstamp
        );
    }
}

#[test]
fn set_midi_options_invalid_setter_is_unreachable() {
    // Hard to construct an invalid MidiOptions through the type, but verify
    // round-trip: set, then options() should reflect midiTempoAdjustment.
    let mut tk = Toolkit::from_data(SAMPLE_PAE).expect("PAE load");
    tk.set_midi_options(&MidiOptions {
        tempo_adjustment: 0.75,
        omit_cue_notes: true,
    })
    .expect("set");
    let opts_json = tk.options();
    assert!(
        opts_json.contains("\"midiTempoAdjustment\": 0.75")
            || opts_json.contains("\"midiTempoAdjustment\": 0.750"),
        "options should reflect 0.75 adjustment, got: {opts_json}"
    );
}

// -- SvgOptions -------------------------------------------------------------

#[test]
fn svg_options_default_emits_empty_css() {
    let opts = SvgOptions::default();
    assert_eq!(opts.css, "");
    assert!(!opts.show_bounding_boxes);
    assert!(!opts.format_raw);
}

#[test]
fn svg_options_custom_css_embeds_in_rendered_svg() {
    let mut tk = Toolkit::from_data(SAMPLE_PAE).expect("PAE load");
    let css = "g.note { fill: #ff0000; }";
    tk.set_svg_options(&SvgOptions {
        css: css.into(),
        ..SvgOptions::default()
    })
    .expect("set svg options");

    let svg = tk.render_to_svg(1).expect("render");
    assert!(
        svg.contains(css),
        "rendered SVG missing the embedded CSS; got first 400 chars: {}",
        &svg[..svg.len().min(400)]
    );
}

#[test]
fn svg_options_bounding_boxes_overlay_appears_in_svg() {
    let mut tk = Toolkit::from_data(SAMPLE_PAE).expect("PAE load");
    tk.set_svg_options(&SvgOptions {
        show_bounding_boxes: true,
        ..SvgOptions::default()
    })
    .expect("set");
    let svg = tk.render_to_svg(1).expect("render");
    // Verovio emits debug rectangles inside the bounding-box overlay; the
    // exact class name is `bounding-box` upstream.
    assert!(
        svg.contains("bounding-box"),
        "expected bounding-box markers in SVG with svgBoundingBoxes=true"
    );
}

#[test]
fn svg_options_css_with_quotes_is_escaped() {
    // CSS containing a quote character has to escape cleanly through the
    // JSON layer or set_options would fail.
    let mut tk = Toolkit::from_data(SAMPLE_PAE).expect("PAE load");
    let res = tk.set_svg_options(&SvgOptions {
        css: r#"g.note[id="treble-1"] { fill: red; }"#.into(),
        ..SvgOptions::default()
    });
    assert!(
        res.is_ok(),
        "set_svg_options with quotes in css failed: {res:?}"
    );
}
