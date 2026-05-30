//! CSS-block generators for common per-staff / per-track styling
//! patterns. The output strings are intended to be passed via
//! [`SvgOptions::css`](crate::SvgOptions::css) or embedded into a host
//! HTML page that overlays the rendered SVG.
//!
//! All generators are pure functions; they don't touch a [`Toolkit`]
//! and can run in a non-Rendering thread.

use std::collections::HashMap;

/// Produce a CSS block that colors every element whose MEI id maps to a
/// given staff number a distinct color from `palette`. Pair with
/// [`Toolkit::staff_map`](crate::Toolkit::staff_map) to get the side
/// table and pass the result through [`SvgOptions::css`].
///
/// Palette wraps modulo length: more staves than colors means colors
/// repeat. Each rule is emitted as a CSS attribute selector
/// `g[id="<mei-id>"]` so it overrides Verovio's default `.note` fill.
///
/// # Example
///
/// ```ignore
/// let staff_map = tk.staff_map()?;
/// let css = verovio::styling::stripe_tracks_by_id(&staff_map, &["#1f77b4", "#ff7f0e"]);
/// tk.set_svg_options(&verovio::SvgOptions { css, ..Default::default() })?;
/// # Ok::<(), verovio::Error>(())
/// ```
pub fn stripe_tracks_by_id(staff_map: &HashMap<String, u32>, palette: &[&str]) -> String {
    if palette.is_empty() || staff_map.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    // Sort by id for deterministic CSS output (helps tests and diffs).
    let mut entries: Vec<(&String, &u32)> = staff_map.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    for (id, staff) in entries {
        let color = palette[((staff.saturating_sub(1)) as usize) % palette.len()];
        // Escape any double quotes in ids defensively — MEI ids are
        // normally `[A-Za-z0-9._-]+`, but be safe.
        let safe_id = id.replace('"', "\\\"");
        out.push_str(&format!(r#"g[id="{safe_id}"] {{ fill: {color}; }} "#));
    }
    out
}

/// Produce a CSS block that fades every element whose id is NOT in
/// `keep_ids`. The faded elements get `fill: <fade_color>; opacity: 0.3`.
/// Useful for visualizing a single track among many — pair with a
/// [`crate::Toolkit::staff_map`] filtered to one staff.
pub fn fade_others(keep_ids: &[String], fade_color: &str) -> String {
    if keep_ids.is_empty() {
        // Without a keep list, fade nothing — return empty CSS.
        return String::new();
    }
    let mut out = String::new();
    out.push_str(&format!(
        "g.note, g.rest, g.chord {{ fill: {fade_color}; opacity: 0.3; }} "
    ));
    for id in keep_ids {
        let safe_id = id.replace('"', "\\\"");
        out.push_str(&format!(
            r#"g[id="{safe_id}"] {{ fill: currentColor; opacity: 1.0; }} "#
        ));
    }
    out
}
