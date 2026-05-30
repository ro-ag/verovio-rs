//! Tests for the `verovio::styling` CSS generators.

use std::collections::HashMap;

use verovio::styling::{fade_others, stripe_tracks_by_id};

#[test]
fn stripe_tracks_by_id_emits_one_rule_per_id() {
    let mut sm = HashMap::new();
    sm.insert("n1".to_string(), 1u32);
    sm.insert("n2".to_string(), 2u32);
    let css = stripe_tracks_by_id(&sm, &["red", "blue"]);
    assert!(css.contains(r#"g[id="n1"] { fill: red; }"#));
    assert!(css.contains(r#"g[id="n2"] { fill: blue; }"#));
}

#[test]
fn stripe_tracks_by_id_wraps_palette_modulo_length() {
    let mut sm = HashMap::new();
    sm.insert("n1".to_string(), 1u32);
    sm.insert("n2".to_string(), 2u32);
    sm.insert("n3".to_string(), 3u32);
    let css = stripe_tracks_by_id(&sm, &["red", "blue"]);
    // Staff 3 wraps to palette[0] = red.
    assert!(css.contains(r#"g[id="n3"] { fill: red; }"#));
}

#[test]
fn stripe_tracks_by_id_empty_inputs_yield_empty_string() {
    let sm = HashMap::<String, u32>::new();
    assert!(stripe_tracks_by_id(&sm, &["red"]).is_empty());
    let mut sm = HashMap::new();
    sm.insert("n1".to_string(), 1u32);
    assert!(stripe_tracks_by_id(&sm, &[]).is_empty());
}

#[test]
fn fade_others_emits_default_fade_then_keep_rules() {
    let keep = vec!["n1".to_string(), "n2".to_string()];
    let css = fade_others(&keep, "#888");
    assert!(css.contains("opacity: 0.3"));
    assert!(css.contains(r#"g[id="n1"]"#));
    assert!(css.contains(r#"g[id="n2"]"#));
}

#[test]
fn fade_others_empty_keep_is_no_op() {
    let css = fade_others(&[], "#888");
    assert!(css.is_empty(), "no-op fade should produce empty CSS");
}
