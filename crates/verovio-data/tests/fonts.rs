//! Tests for the bundled font listing.

#[test]
fn available_fonts_lists_bravura_first() {
    let fonts = verovio_data::AVAILABLE_FONTS;
    assert!(!fonts.is_empty(), "should list at least one font");
    assert_eq!(fonts[0], "Bravura", "Bravura is Verovio's default");
}

#[test]
fn available_fonts_directories_exist_in_bundle() {
    let data = verovio_data::DATA;
    for font in verovio_data::AVAILABLE_FONTS {
        let found = data.get_dir(font);
        assert!(
            found.is_some(),
            "AVAILABLE_FONTS lists '{font}' but no '{font}/' dir in bundle"
        );
    }
}
