//! Load a Plaine & Easie snippet, render every page to an SVG file.
//!
//! Run with:
//!     cargo run --example render_to_file -- /tmp/out
//!
//! The output dir is created if it doesn't exist; one `page-{n}.svg` is
//! written per layout page.

use std::env;
use std::error::Error;
use std::fs;
use std::path::PathBuf;

use verovio::Toolkit;

// A short PAE snippet. The format is auto-detected from content, so this
// works the same with MusicXML or MEI swapped in.
const SAMPLE_PAE: &str = "\
@start:demo
@clef:G-2
@keysig:xF
@key:
@timesig:4/4
@data:'4G/4-
@end:demo
";

fn main() -> Result<(), Box<dyn Error>> {
    let out_dir: PathBuf = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| env::temp_dir().join("verovio-rs-example"));
    fs::create_dir_all(&out_dir)?;

    let mut tk = Toolkit::new();
    tk.load_data(SAMPLE_PAE)?;

    let pages = tk.page_count();
    println!("verovio {}: rendering {pages} page(s) to {}", tk.version(), out_dir.display());

    // Reuse one buffer across all pages — the `_into` variant clears and
    // refills it on each call, avoiding per-page String allocation.
    let mut svg = String::with_capacity(64 * 1024);
    for page in 1..=pages {
        tk.render_to_svg_into(page, &mut svg)?;
        let path = out_dir.join(format!("page-{page}.svg"));
        fs::write(&path, &svg)?;
        println!("  wrote {}", path.display());
    }

    Ok(())
}
