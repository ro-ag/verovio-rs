//! Load a score and write a Standard MIDI File (`.mid`) you can play back.
//!
//! Run with:
//!     cargo run --example render_to_midi -- /tmp/out.mid
//!
//! If no path is given, writes to `$TMPDIR/verovio-rs-example.mid`.
//! Verovio synthesizes a single-track MIDI file with the tempo and notes
//! from the loaded score; the resulting `.mid` opens in any DAW or
//! `aplaymidi` / `timidity` style player.

use std::env;
use std::error::Error;
use std::fs;
use std::path::PathBuf;

use verovio::Toolkit;

const SAMPLE_PAE: &str = "\
@start:s
@clef:G-2
@keysig:xF
@key:
@timesig:4/4
@data:'4G/4-
@end:s
";

fn main() -> Result<(), Box<dyn Error>> {
    let out_path: PathBuf = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| env::temp_dir().join("verovio-rs-example.mid"));

    let mut tk = Toolkit::from_data(SAMPLE_PAE)?;
    let midi = tk.render_to_midi_bytes()?;

    fs::write(&out_path, &midi)?;
    println!(
        "verovio {}: wrote {} bytes of SMF to {}",
        tk.version(),
        midi.len(),
        out_path.display()
    );
    println!("  first 4 bytes: {:?} (expect b\"MThd\")", &midi[..4]);

    Ok(())
}
