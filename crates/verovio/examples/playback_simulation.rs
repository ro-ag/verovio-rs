//! Simulate playback: walk a loaded score at fixed tick intervals and report
//! which note IDs are sounding at each tick.
//!
//! This is the shape an actual playback driver takes — call `tk.timemap()`
//! exactly once at load time, then query the cached `Vec<TimemapEvent>` at
//! the audio / UI thread's rate using
//! [`verovio::lookup::sounding_at_into`]. Zero FFI calls and zero JSON
//! parsing per tick.
//!
//! Run with:
//!     cargo run --example playback_simulation
//!
//! Sample output for a one-bar PAE fixture (one G quarter note, one rest):
//!     Playing back 1000 ms in 100 ms ticks…
//!       [    0 ms] sounding=["n2690c7"]
//!       [  100 ms] sounding=["n2690c7"]
//!       [  200 ms] sounding=["n2690c7"]
//!       …
//!       [  500 ms] sounding=["n2690c7"]   ← note still sounds at its off-event
//!       [  600 ms] (silence)
//!       …

use std::error::Error;

use verovio::lookup::sounding_at_into;
use verovio::Toolkit;

const SAMPLE_PAE: &str = "\
@start:s
@clef:G-2
@keysig:xF
@key:
@timesig:
@data:'4G/4-
@end:s
";

/// Playback tick interval. 10 Hz is slow enough to read in a terminal;
/// production drivers would run at 60 Hz (UI) or 44.1 kHz (audio).
const PLAYBACK_TICK_MS: f64 = 100.0;

fn main() -> Result<(), Box<dyn Error>> {
    let mut tk = Toolkit::from_data(SAMPLE_PAE)?;

    // ← The only FFI + JSON crossing in this whole program.
    let timemap = tk.timemap()?;

    let end_ms = timemap.last().map(|e| e.tstamp).unwrap_or(0.0);

    println!(
        "verovio {}: playing back {:.0} ms in {:.0} ms ticks…",
        tk.version(),
        end_ms,
        PLAYBACK_TICK_MS
    );

    // Reuse one allocation across every tick — sounding_at_into clears and
    // refills the buffer in place.
    let mut active = Vec::with_capacity(16);
    let mut t = 0.0;
    while t <= end_ms {
        sounding_at_into(&timemap, t, &mut active);
        if active.is_empty() {
            println!("  [{t:>5.0} ms] (silence)");
        } else {
            println!("  [{t:>5.0} ms] sounding={active:?}");
        }
        t += PLAYBACK_TICK_MS;
    }

    Ok(())
}
