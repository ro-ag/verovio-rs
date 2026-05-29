//! Tests for [`Toolkit::classified_elements`] — per-ID structural type
//! lookup over a parsed timemap.

use verovio::{ElementKind, Toolkit};

const SAMPLE_PAE: &str = "\
@start:s
@clef:G-2
@keysig:xF
@key:
@timesig:
@data:'4G/4-
@end:s
";

#[test]
fn classifies_note_and_rest_from_one_bar_pae() {
    let mut tk = Toolkit::from_data(SAMPLE_PAE).expect("PAE load");
    let classified = tk.classified_elements().expect("classified_elements");

    // The fixture has one note + one rest; we should see at least one of
    // each kind.
    let kinds: std::collections::HashSet<ElementKind> = classified.values().copied().collect();
    assert!(
        kinds.contains(&ElementKind::Note),
        "missing Note kind in {classified:?}"
    );
    assert!(
        kinds.contains(&ElementKind::Rest),
        "missing Rest kind in {classified:?}"
    );
    assert!(
        kinds.contains(&ElementKind::Measure),
        "missing Measure kind in {classified:?}"
    );
}

#[test]
fn classified_ids_appear_in_corresponding_timemap_arrays() {
    let mut tk = Toolkit::from_data(SAMPLE_PAE).expect("PAE load");
    let timemap = tk.timemap().expect("timemap");
    let classified = tk.classified_elements().expect("classify");

    // Every id that appears in `on` should be classified — typically as a
    // note for our fixture. (Could be a chord in richer scores; we just
    // assert the id is in the classification table.)
    for ev in &timemap {
        for id in &ev.on {
            assert!(
                classified.contains_key(id),
                "id {id:?} in timemap.on missing from classification"
            );
        }
    }
}

#[test]
fn classification_is_invariant_across_redo_layout() {
    // Layout changes don't change element identity — re-running layout
    // shouldn't change which IDs are classified or how.
    let mut tk = Toolkit::from_data(SAMPLE_PAE).expect("PAE load");
    let before = tk.classified_elements().expect("first classify");
    tk.redo_layout();
    let after = tk.classified_elements().expect("second classify");
    assert_eq!(before, after);
}
