//! Feature-gated offline audio rendering of Verovio's SMF output via
//! `rustysynth` (a pure-Rust SoundFont 2 synthesizer).
//!
//! Behind the `audio` Cargo feature. The caller provides the SoundFont
//! (SF2) bytes — this crate does NOT bundle one, since usable SoundFonts
//! are usually 50+ MB. Free GM-compatible SoundFonts: TimGM6mb (~6 MB),
//! FluidR3 (~140 MB), GeneralUser GS (~30 MB).
//!
//! # Pattern
//!
//! ```ignore
//! let mut tk = verovio::Toolkit::from_data(score)?;
//! let sf2 = std::fs::read("TimGM6mb.sf2")?;
//! let wav = tk.render_to_wav(&sf2, 44_100)?;
//! std::fs::write("out.wav", wav)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Threading
//!
//! Audio rendering happens entirely in Rust on the SMF bytes Verovio
//! already produced — it does not touch any Verovio C++ state. Safe to
//! call from any thread.

use std::io::Cursor;
use std::sync::Arc;

use rustysynth::{MidiFile, MidiFileSequencer, SoundFont, Synthesizer, SynthesizerSettings};

use crate::{Error, Result};

/// One-shot stereo audio render. The caller owns the buffer; mono
/// summing or device dispatch is their responsibility.
#[derive(Debug, Clone, PartialEq)]
pub struct Pcm {
    /// Sample rate in Hz (e.g. 44_100, 48_000).
    pub sample_rate: u32,
    /// Left channel samples, f32 in `[-1.0, 1.0]` (clipping is the
    /// caller's responsibility — rustysynth may exceed unity briefly).
    pub left: Vec<f32>,
    /// Right channel samples, same length as `left`.
    pub right: Vec<f32>,
}

impl Pcm {
    /// Total wall-clock duration of the render, in seconds.
    pub fn duration_secs(&self) -> f64 {
        self.left.len() as f64 / self.sample_rate as f64
    }

    /// Interleave the stereo channels into a single `Vec<f32>` —
    /// `[L0, R0, L1, R1, …]`. Suitable for feeding APIs that expect
    /// interleaved PCM (cpal, miniaudio, WebAudio scriptProcessor).
    pub fn interleaved(&self) -> Vec<f32> {
        let n = self.left.len().min(self.right.len());
        let mut out = Vec::with_capacity(n * 2);
        for i in 0..n {
            out.push(self.left[i]);
            out.push(self.right[i]);
        }
        out
    }
}

/// Render `midi_bytes` to stereo PCM using `sf2_bytes` as the
/// SoundFont. `sample_rate` is the output rate in Hz.
///
/// Buffer length is determined from the MIDI file's reported length;
/// add a tail of silence yourself if you want the synth's release stage
/// to ring out fully.
///
/// Behind the `audio` feature.
pub fn render_pcm(midi_bytes: &[u8], sf2_bytes: &[u8], sample_rate: u32) -> Result<Pcm> {
    let mut sf2_cursor = Cursor::new(sf2_bytes);
    let sound_font = Arc::new(
        SoundFont::new(&mut sf2_cursor).map_err(|e| Error::Audio(format!("SoundFont: {e}")))?,
    );

    let mut midi_cursor = Cursor::new(midi_bytes);
    let midi_file = Arc::new(
        MidiFile::new(&mut midi_cursor).map_err(|e| Error::Audio(format!("MidiFile: {e}")))?,
    );

    let settings = SynthesizerSettings::new(sample_rate as i32);
    let synth = Synthesizer::new(&sound_font, &settings)
        .map_err(|e| Error::Audio(format!("Synthesizer: {e}")))?;
    let mut sequencer = MidiFileSequencer::new(synth);
    sequencer.play(&midi_file, false);

    let length_secs = midi_file.get_length();
    let sample_count = (sample_rate as f64 * length_secs).ceil() as usize;
    let mut left = vec![0_f32; sample_count];
    let mut right = vec![0_f32; sample_count];
    sequencer.render(&mut left[..], &mut right[..]);

    Ok(Pcm {
        sample_rate,
        left,
        right,
    })
}

/// Render `midi_bytes` to a RIFF WAV (16-bit signed PCM, stereo) at
/// `sample_rate` Hz. Self-contained `.wav` file bytes — write directly
/// to disk or hand to a network response.
///
/// Output format: standard RIFF/WAVE container with `fmt ` chunk
/// (PCM tag `0x0001`, 2 channels, 16 bits/sample) and a `data` chunk
/// of interleaved L/R samples.
///
/// Behind the `audio` feature.
pub fn render_wav(midi_bytes: &[u8], sf2_bytes: &[u8], sample_rate: u32) -> Result<Vec<u8>> {
    let pcm = render_pcm(midi_bytes, sf2_bytes, sample_rate)?;
    Ok(pcm_to_wav(&pcm))
}

/// Encode a [`Pcm`] buffer as a RIFF WAV (16-bit signed PCM, stereo).
///
/// Samples outside the `[-1.0, 1.0]` range are clipped to `i16::MIN` /
/// `i16::MAX`.
pub fn pcm_to_wav(pcm: &Pcm) -> Vec<u8> {
    let n = pcm.left.len().min(pcm.right.len());
    let bytes_per_sample: u32 = 2;
    let channels: u32 = 2;
    let byte_rate = pcm.sample_rate * channels * bytes_per_sample;
    let block_align: u16 = (channels * bytes_per_sample) as u16;
    let data_size: u32 = (n as u32) * channels * bytes_per_sample;
    let chunk_size: u32 = 36 + data_size;

    let mut out = Vec::with_capacity(44 + data_size as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&chunk_size.to_le_bytes());
    out.extend_from_slice(b"WAVE");
    // fmt chunk
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // PCM fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM tag
    out.extend_from_slice(&(channels as u16).to_le_bytes());
    out.extend_from_slice(&pcm.sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
                                                 // data chunk
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_size.to_le_bytes());
    for i in 0..n {
        let l = clamp_to_i16(pcm.left[i]);
        let r = clamp_to_i16(pcm.right[i]);
        out.extend_from_slice(&l.to_le_bytes());
        out.extend_from_slice(&r.to_le_bytes());
    }
    out
}

fn clamp_to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
}

impl crate::Toolkit {
    /// Render the loaded document to a stereo [`Pcm`] buffer using
    /// `sf2_bytes` as the SoundFont. Equivalent to
    /// `render_pcm(&render_to_midi_bytes()?, sf2_bytes, sample_rate)`.
    ///
    /// Behind the `audio` feature.
    pub fn render_to_pcm(&mut self, sf2_bytes: &[u8], sample_rate: u32) -> Result<Pcm> {
        let midi = self.render_to_midi_bytes()?;
        render_pcm(&midi, sf2_bytes, sample_rate)
    }

    /// Render the loaded document to a RIFF WAV byte buffer using
    /// `sf2_bytes` as the SoundFont. See [`render_wav`] for the format
    /// details.
    pub fn render_to_wav(&mut self, sf2_bytes: &[u8], sample_rate: u32) -> Result<Vec<u8>> {
        let midi = self.render_to_midi_bytes()?;
        render_wav(&midi, sf2_bytes, sample_rate)
    }

    /// Render with a [`MidiTrackPolicy`](crate::midi::MidiTrackPolicy)
    /// applied first — the audio counterpart of
    /// [`render_to_midi_bytes_with_policy`](crate::Toolkit::render_to_midi_bytes_with_policy).
    pub fn render_to_wav_with_policy(
        &mut self,
        sf2_bytes: &[u8],
        sample_rate: u32,
        policy: &crate::midi::MidiTrackPolicy,
    ) -> Result<Vec<u8>> {
        let midi = self.render_to_midi_bytes_with_policy(policy)?;
        render_wav(&midi, sf2_bytes, sample_rate)
    }
}
