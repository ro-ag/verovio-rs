//! Render a 2-staff score to a *playable* multi-track MIDI file.
//!
//! Demonstrates `verovio::midi::MidiTrackPolicy` on top of
//! `Toolkit::render_to_midi_bytes_with_policy`:
//!
//! - **Auto-distribute channels** so each staff lives on its own MIDI
//!   channel (treble → ch 0, bass → ch 1).
//! - **Per-track instrument** via General MIDI program numbers (treble →
//!   Acoustic Grand Piano = 0, bass → Cello = 42).
//! - **Per-track volume** so the bass sits slightly under the treble.
//!
//! Run with:
//!     cargo run --example multi_track_playback -- /tmp/multi.mid
//!
//! The resulting `.mid` opens in any DAW or `aplaymidi`/`timidity`-style
//! player and plays as **two distinct voices**, not one piano playing
//! every note. Compare against `render_to_midi.rs` which uses Verovio's
//! default output — that one plays everything on channel 0 with the
//! synth's default instrument.

use std::collections::BTreeMap;
use std::env;
use std::error::Error;
use std::fs;
use std::path::PathBuf;

use verovio::midi::{MidiTrackPolicy, TrackOverride};
use verovio::Toolkit;

const TWO_STAFF_MEI: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<mei xmlns="http://www.music-encoding.org/ns/mei" meiversion="4.0.0">
  <meiHead><fileDesc><titleStmt><title>multi-track demo</title></titleStmt><pubStmt/></fileDesc></meiHead>
  <music><body><mdiv><score>
    <scoreDef><staffGrp>
      <staffDef n="1" lines="5" clef.shape="G" clef.line="2"/>
      <staffDef n="2" lines="5" clef.shape="F" clef.line="4"/>
    </staffGrp></scoreDef>
    <section><measure>
      <staff n="1"><layer>
        <note pname="g" oct="4" dur="4" xml:id="treble-1"/>
        <note pname="a" oct="4" dur="4" xml:id="treble-2"/>
        <note pname="b" oct="4" dur="4" xml:id="treble-3"/>
        <note pname="c" oct="5" dur="4" xml:id="treble-4"/>
      </layer></staff>
      <staff n="2"><layer>
        <note pname="c" oct="3" dur="4" xml:id="bass-1"/>
        <note pname="c" oct="3" dur="4" xml:id="bass-2"/>
        <note pname="g" oct="2" dur="4" xml:id="bass-3"/>
        <note pname="c" oct="3" dur="4" xml:id="bass-4"/>
      </layer></staff>
    </measure></section>
  </score></mdiv></body></music></mei>"#;

fn main() -> Result<(), Box<dyn Error>> {
    let out_path: PathBuf = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| env::temp_dir().join("verovio-rs-multi-track.mid"));

    let mut tk = Toolkit::from_data(TWO_STAFF_MEI)?;

    // Auto-distribute puts each non-meta track on its own MIDI channel
    // (track 1 → ch 0, track 2 → ch 1). Then we layer per-track
    // instrument + volume on top.
    let mut overrides = BTreeMap::new();
    overrides.insert(
        1, // staff 1 = treble
        TrackOverride {
            program: Some(0), // Acoustic Grand Piano
            volume: Some(110),
            ..Default::default()
        },
    );
    overrides.insert(
        2, // staff 2 = bass
        TrackOverride {
            program: Some(42), // Cello
            volume: Some(85),
            ..Default::default()
        },
    );
    let policy = MidiTrackPolicy {
        overrides,
        auto_distribute_channels: true,
        ..MidiTrackPolicy::default()
    };

    let bytes = tk.render_to_midi_bytes_with_policy(&policy)?;
    fs::write(&out_path, &bytes)?;

    println!(
        "verovio {}: wrote {} bytes of multi-track SMF to {}",
        tk.version(),
        bytes.len(),
        out_path.display()
    );
    println!("  staff 1 → ch 0 / Piano / vol 110");
    println!("  staff 2 → ch 1 / Cello / vol  85");
    println!("Play with: aplaymidi -p <port> {}", out_path.display());

    Ok(())
}
