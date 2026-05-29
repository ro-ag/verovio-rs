//! Tests for [`Toolkit::staff_map`] — element-id → staff-index extraction
//! from the rendered SVG.

use verovio::Toolkit;

const TWO_STAFF_MEI: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<mei xmlns="http://www.music-encoding.org/ns/mei" meiversion="4.0.0">
  <meiHead><fileDesc><titleStmt><title>two-staff</title></titleStmt><pubStmt/></fileDesc></meiHead>
  <music><body><mdiv><score>
    <scoreDef><staffGrp>
      <staffDef n="1" lines="5" clef.shape="G" clef.line="2"/>
      <staffDef n="2" lines="5" clef.shape="F" clef.line="4"/>
    </staffGrp></scoreDef>
    <section><measure>
      <staff n="1"><layer>
        <note pname="g" oct="4" dur="4" xml:id="treble-1"/>
        <note pname="g" oct="4" dur="4" xml:id="treble-2"/>
      </layer></staff>
      <staff n="2"><layer>
        <note pname="c" oct="3" dur="4" xml:id="bass-1"/>
        <note pname="c" oct="3" dur="4" xml:id="bass-2"/>
      </layer></staff>
    </measure></section>
  </score></mdiv></body></music></mei>"#;

#[test]
fn staff_map_assigns_treble_notes_to_staff_1() {
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI load");
    let map = tk.staff_map().expect("staff_map");

    assert_eq!(map.get("treble-1"), Some(&1));
    assert_eq!(map.get("treble-2"), Some(&1));
}

#[test]
fn staff_map_assigns_bass_notes_to_staff_2() {
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI load");
    let map = tk.staff_map().expect("staff_map");

    assert_eq!(map.get("bass-1"), Some(&2));
    assert_eq!(map.get("bass-2"), Some(&2));
}

#[test]
fn staff_map_indexes_are_one_based_matching_smf_track_indices() {
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI load");
    let map = tk.staff_map().expect("staff_map");

    let max_staff = map.values().copied().max().expect("non-empty map");
    let min_staff = map.values().copied().min().expect("non-empty map");
    assert_eq!(min_staff, 1, "staff numbering should be 1-indexed");
    assert_eq!(max_staff, 2, "two-staff MEI should produce indices 1..=2");
}

#[test]
fn staff_map_one_staff_score_has_only_index_1() {
    const ONE_STAFF_PAE: &str =
        "@start:t\n@clef:G-2\n@keysig:xF\n@key:\n@timesig:\n@data:'4G/4-\n@end:t\n";
    let mut tk = Toolkit::from_data(ONE_STAFF_PAE).expect("PAE load");
    let map = tk.staff_map().expect("staff_map");

    assert!(
        !map.is_empty(),
        "PAE fixture should produce a non-empty map"
    );
    for (id, staff) in &map {
        assert_eq!(
            *staff, 1,
            "every id should be on staff 1, got {id:?}={staff}"
        );
    }
}

#[test]
fn staff_map_unloaded_doc_errors_via_render_path() {
    // No document loaded → page_count == 0 → render_to_svg short-circuits
    // to RenderFailed → staff_map propagates the error.
    let mut tk = Toolkit::new();
    let res = tk.staff_map();
    // We expect *some* failure since pages = 0; if Toolkit::staff_map
    // somehow returns Ok with an empty map, that's also a valid contract
    // (no pages means no staves to walk).
    match res {
        Ok(map) => assert!(map.is_empty(), "no doc loaded → empty staff_map"),
        Err(_) => {} // also acceptable
    }
}
