//! Walk a loaded score's timemap and report which note IDs are sounding at
//! each event boundary. Demonstrates the typed [`Timemap`] consumer shape
//! that xpart needs for playhead-to-notation sync.
//!
//! Run with:
//!     cargo run --example playback_simulation
//!
//! Sample output for a one-bar PAE fixture:
//!     [0.0 ms,   q=0]  on:[n2690c7]                tempo=120
//!     [500.0 ms, q=1]  off:[n2690c7]
//!     [1000.0 ms, q=2] (silence)

use std::collections::BTreeSet;
use std::error::Error;

use verovio::{TimemapEvent, Toolkit};

const SAMPLE_PAE: &str = "\
@start:s
@clef:G-2
@keysig:xF
@key:
@timesig:
@data:'4G/4-
@end:s
";

fn main() -> Result<(), Box<dyn Error>> {
    let mut tk = Toolkit::from_data(SAMPLE_PAE)?;
    let timemap = tk.timemap()?;

    let mut sounding: BTreeSet<String> = BTreeSet::new();
    for ev in &timemap {
        for id in &ev.on {
            sounding.insert(id.clone());
        }
        for id in &ev.off {
            sounding.remove(id);
        }
        render_event(ev, &sounding);
    }

    Ok(())
}

fn render_event(ev: &TimemapEvent, sounding: &BTreeSet<String>) {
    let pos = format!("[{:.1} ms, q={}]", ev.tstamp, ev.qstamp);
    let now: Vec<&str> = sounding.iter().map(String::as_str).collect();
    let onset = if ev.on.is_empty() {
        String::new()
    } else {
        format!("on:[{}] ", ev.on.join(","))
    };
    let offset = if ev.off.is_empty() {
        String::new()
    } else {
        format!("off:[{}] ", ev.off.join(","))
    };
    let tempo = ev
        .tempo
        .map(|bpm| format!("tempo={bpm:.0}"))
        .unwrap_or_default();
    let body = if now.is_empty() {
        "(silence)".to_string()
    } else {
        format!("sounding=[{}]", now.join(","))
    };
    println!("  {pos}  {onset}{offset}{body} {tempo}");
}
