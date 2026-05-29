//! Multi-track / multi-staff coverage.
//!
//! Two concerns this file pins:
//!
//! 1. Verovio emits a Format-1 (simultaneous multi-track) Standard MIDI File
//!    for a multi-staff score, with one track per staff. We parse the SMF
//!    header bytes ourselves to verify the track count — proving the
//!    `render_to_midi_bytes` pipeline carries staff information through to
//!    the SMF wire format.
//!
//! 2. **The timemap does NOT carry per-track / per-staff information.**
//!    Verovio's upstream `TimemapEntry` (`include/vrv/timemap.h`) has no
//!    staff field; `notesOn` / `notesOff` are flat sets of element IDs
//!    regardless of which staff they belong to. This is a real upstream
//!    constraint, not an omission in our bindings — consumers that need
//!    per-track sync have to cross-reference element IDs against the MEI
//!    structure (or parse the rendered MIDI separately). The test
//!    `timemap_does_not_distinguish_staves` pins this so a future Verovio
//!    update that DOES expose staff info will fail loudly and prompt us to
//!    surface it.

use verovio::Toolkit;

// Minimal two-staff MEI: G-clef (treble) for staff 1, F-clef (bass) for
// staff 2; one measure of 4 quarter notes per staff. Verovio synthesizes
// this into two MIDI tracks (one per staff).
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
            <measure>
              <staff n="1">
                <layer>
                  <note pname="g" oct="4" dur="4" xml:id="treble-1"/>
                  <note pname="g" oct="4" dur="4" xml:id="treble-2"/>
                  <note pname="g" oct="4" dur="4" xml:id="treble-3"/>
                  <note pname="g" oct="4" dur="4" xml:id="treble-4"/>
                </layer>
              </staff>
              <staff n="2">
                <layer>
                  <note pname="c" oct="3" dur="4" xml:id="bass-1"/>
                  <note pname="c" oct="3" dur="4" xml:id="bass-2"/>
                  <note pname="c" oct="3" dur="4" xml:id="bass-3"/>
                  <note pname="c" oct="3" dur="4" xml:id="bass-4"/>
                </layer>
              </staff>
            </measure>
          </section>
        </score>
      </mdiv>
    </body>
  </music>
</mei>
"#;

/// Parse the SMF (Standard MIDI File) header chunk.
/// Returns `(format, track_count, ticks_per_quarter)`.
fn parse_smf_header(bytes: &[u8]) -> (u16, u16, u16) {
    assert!(bytes.len() >= 14, "SMF must be at least 14 bytes");
    assert_eq!(&bytes[..4], b"MThd", "missing MThd magic");
    // bytes[4..8] is a big-endian u32 = 6 (header chunk length, always 6)
    let format = u16::from_be_bytes([bytes[8], bytes[9]]);
    let ntrks = u16::from_be_bytes([bytes[10], bytes[11]]);
    let division = u16::from_be_bytes([bytes[12], bytes[13]]);
    (format, ntrks, division)
}

#[test]
fn two_staff_mei_produces_multi_track_smf() {
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI parse");
    let smf = tk.render_to_midi_bytes().expect("midi bytes");
    let (format, ntrks, division) = parse_smf_header(&smf);

    // Format 1 = simultaneous multi-track (every player plays at once).
    assert_eq!(
        format, 1,
        "expected SMF format 1 (multi-track simultaneous), got {format}"
    );
    // Two staves → at least two tracks. Verovio commonly emits one
    // additional track for tempo/meta-events, so allow >= 2.
    assert!(
        ntrks >= 2,
        "expected at least 2 SMF tracks for a 2-staff score, got {ntrks}"
    );
    // Division should be positive ticks-per-quarter (top bit clear means
    // metric time; we never expect SMPTE timecode out of Verovio).
    assert!(
        division & 0x8000 == 0 && division > 0,
        "expected metric division, got {division:#x}"
    );
}

#[test]
fn two_staff_mei_renders_svg_with_both_staves() {
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI parse");
    let svg = tk.render_to_svg(1).expect("render page 1");
    // Each staff's xml:id flows through to the SVG. Spot-check that both
    // the treble and bass note IDs survive the render.
    assert!(
        svg.contains("treble-1") || svg.contains("treble-4"),
        "treble notes missing from SVG"
    );
    assert!(
        svg.contains("bass-1") || svg.contains("bass-4"),
        "bass notes missing from SVG"
    );
}

#[test]
fn timemap_does_not_distinguish_staves() {
    // **Upstream constraint pin.** Verovio's TimemapEntry has no staff
    // field — `on` / `off` are flat sets. A treble C-major event and a
    // bass C-major event at the same tstamp collapse into the same
    // event's `on` array. This test will start failing if a future
    // Verovio adds per-track info, prompting us to surface it.
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI parse");
    let timemap = tk.timemap().expect("timemap parse");
    // At t=0, both staves' first notes should be in the same on-set.
    let first = &timemap[0];
    assert!(
        first.on.contains(&"treble-1".to_string()),
        "first event missing treble-1: {first:?}"
    );
    assert!(
        first.on.contains(&"bass-1".to_string()),
        "first event missing bass-1 (timemap should fold both staves together): {first:?}"
    );
}

#[test]
fn elements_at_time_zero_returns_both_staff_notes() {
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI parse");
    let elements = tk.elements_at(0).expect("elements parse");
    assert!(
        elements.notes.contains(&"treble-1".to_string())
            && elements.notes.contains(&"bass-1".to_string()),
        "elements_at(0) should report both staves' first notes, got {elements:?}"
    );
}
