//! Render a score with theming applied — custom background, note color,
//! and a `.playing` class wired in so a player can toggle it on the DOM
//! at runtime for playhead highlighting.
//!
//! Run with:
//!     cargo run --example styled_render -- /tmp/styled.svg
//!
//! Open the resulting SVG in a browser — you should see a beige page,
//! navy noteheads, and the staff drawn in a warmer gray. Toggle the
//! `.playing` class on any `<g class="note">` to flash it amber.

use std::env;
use std::error::Error;
use std::fs;

use verovio::{SvgOptions, Toolkit};

const SAMPLE_PAE: &str = "\
@start:s
@clef:G-2
@keysig:xF
@key:
@timesig:4/4
@data:'4G/4-
@end:s
";

const THEME_CSS: &str = "
svg { background: #fafaf5; }
g.note { fill: #14213d; }
g.note .stem path { stroke: #14213d; }
g.staff line { stroke: #999; }
g.barline line { stroke: #666; }
g.measure text { fill: #444; }

/* Runtime-toggled by the player to flash a note while it sounds. */
g.note.playing { fill: #fca311; }
g.note.playing .stem path { stroke: #fca311; }
";

fn main() -> Result<(), Box<dyn Error>> {
    let out_path = env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/verovio-rs-styled.svg".to_string());

    let mut tk = Toolkit::from_data(SAMPLE_PAE)?;
    tk.set_svg_options(&SvgOptions {
        css: THEME_CSS.into(),
        ..SvgOptions::default()
    })?;

    let svg = tk.render_to_svg(1)?;
    fs::write(&out_path, &svg)?;

    println!(
        "verovio {}: wrote themed SVG ({} bytes) to {}",
        tk.version(),
        svg.len(),
        out_path
    );
    println!("Open in a browser to see the theming applied.");

    Ok(())
}
