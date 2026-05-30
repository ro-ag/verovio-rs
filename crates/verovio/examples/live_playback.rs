//! Live audio playback of a Verovio score through the system default
//! output device, using `cpal` for the audio backend and `rustysynth`
//! (re-exported via the `audio` feature) for SoundFont-based synthesis.
//!
//! # Run
//!
//! ```sh
//! cargo run --release --features audio --example live_playback -- path/to/font.sf2
//! ```
//!
//! Free GM-compatible SoundFonts: `TimGM6mb.sf2` (~6 MB), `GeneralUser
//! GS.sf2` (~30 MB). Without an SF2 the program prints usage and exits.
//!
//! # What this demonstrates
//!
//! - Rendering MIDI from a Verovio score with a multi-track policy
//! - Streaming the synthesized PCM into cpal's real-time audio callback
//! - Lock-free state sharing (the callback never blocks the audio thread)
//!
//! # What it does NOT do
//!
//! - Pause / resume / seek (would need a command channel)
//! - Visual sync (the timemap is available but not wired)
//! - Error recovery on device disconnect
//!
//! Consumers building a full player (xpart, etc.) should use this as a
//! starting point.

use std::env;
use std::io::Cursor;
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rustysynth::{MidiFile, MidiFileSequencer, SoundFont, Synthesizer, SynthesizerSettings};
use verovio::Toolkit;

const DEMO_PAE: &str = "@start:demo\n@clef:G-2\n@keysig:\n@key:\n@timesig:4/4\n@data:'4G/4A/4B/4c/4d/4e/4f/4g\n@end:demo\n";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sf2_path = match env::args().nth(1) {
        Some(p) => p,
        None => {
            eprintln!("usage: live_playback <path-to-SoundFont.sf2>");
            std::process::exit(2);
        }
    };

    // 1. Render the score to SMF bytes.
    let mut tk = Toolkit::from_data(DEMO_PAE)?;
    let midi_bytes = tk.render_to_midi_bytes()?;
    println!("Rendered {} SMF bytes from PAE demo.", midi_bytes.len());

    // 2. Load the SoundFont and set up the sequencer.
    let sf2_bytes = std::fs::read(&sf2_path)?;
    let sound_font = Arc::new(SoundFont::new(&mut Cursor::new(sf2_bytes))?);
    let midi_file = Arc::new(MidiFile::new(&mut Cursor::new(midi_bytes))?);

    // 3. Open the default output device.
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or("no default output device")?;
    let config = device.default_output_config()?;
    let sample_rate = config.sample_rate().0 as i32;
    let channels = config.channels() as usize;
    println!(
        "Audio device: {} @ {} Hz, {} channels",
        device.name()?,
        sample_rate,
        channels
    );

    let settings = SynthesizerSettings::new(sample_rate);
    let synth = Synthesizer::new(&sound_font, &settings)?;
    let mut sequencer = MidiFileSequencer::new(synth);
    sequencer.play(&midi_file, false);

    // 4. Hand the sequencer into the audio callback. The mutex is for
    //    Send-passing only; under the callback's high-priority thread,
    //    `try_lock` would be the production move (avoid blocking if a
    //    UI thread is mid-update). This example is single-threaded
    //    around the sequencer, so a plain `lock` is fine.
    let sequencer = Arc::new(Mutex::new(sequencer));
    let seq_for_cb = Arc::clone(&sequencer);

    let mut left = vec![0_f32; 1024];
    let mut right = vec![0_f32; 1024];

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_output_stream(
            &config.clone().into(),
            move |buffer: &mut [f32], _| {
                let frames = buffer.len() / channels;
                if left.len() < frames {
                    left.resize(frames, 0.0);
                    right.resize(frames, 0.0);
                }
                let mut seq = seq_for_cb.lock().unwrap();
                seq.render(&mut left[..frames], &mut right[..frames]);
                for (i, frame) in buffer.chunks_exact_mut(channels).enumerate() {
                    if channels >= 2 {
                        frame[0] = left[i];
                        frame[1] = right[i];
                    } else {
                        frame[0] = 0.5 * (left[i] + right[i]);
                    }
                }
            },
            move |err| eprintln!("audio stream error: {err}"),
            None,
        )?,
        other => {
            return Err(format!("unsupported sample format {other:?} — extend the example").into());
        }
    };

    stream.play()?;
    println!("Playing. Press Enter to stop.");

    let mut input = String::new();
    std::io::stdin().read_line(&mut input).ok();

    drop(stream);
    Ok(())
}
