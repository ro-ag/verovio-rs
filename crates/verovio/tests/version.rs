use verovio::Toolkit;

#[test]
fn version_is_nonempty_and_numeric() {
    let tk = Toolkit::new();
    let v = tk.version();
    assert!(!v.is_empty(), "Verovio version string should not be empty");
    assert!(
        v.chars().next().is_some_and(|c| c.is_ascii_digit()),
        "expected a version starting with a digit, got {v:?}"
    );
    println!("Verovio version: {v}");
}

#[test]
fn version_matches_pinned_tag() {
    let tk = Toolkit::new();
    let v = tk.version();
    assert!(
        v.starts_with("6.2"),
        "expected pinned Verovio 6.2.x, got {v:?}"
    );
}
