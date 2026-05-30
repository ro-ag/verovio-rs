//! Post-processing of Verovio's rendered SMF for genuine multi-track control.
//!
//! # Why this module exists
//!
//! Verovio emits a Format-1 (parallel) SMF with one track per staff plus
//! one meta track at index 0. Verified by inspection of the actual bytes:
//!
//! ```text
//! Format: Parallel (1)
//! Timing: 120 ticks per quarter note
//! Track 0: Tempo, EndOfTrack
//! Track 1: <staff 1 notes>    ← all on channel 0
//! Track 2: <staff 2 notes>    ← all on channel 0
//! ```
//!
//! **Every Midi event is on channel 0.** A downstream synth (FluidSynth,
//! `rustysynth`, a hardware MIDI port) treats channel 0 as one voice;
//! all staves play through the same instrument, the same volume, and
//! channel-level controls like mute / solo can't distinguish them.
//!
//! No program-change, no controller events, no per-track instrument or
//! volume hints either.
//!
//! This module rewrites the SMF in-process to fix that. Apply a
//! [`MidiTrackPolicy`] and you get back bytes a synth will treat as
//! independent voices: each track on its own channel, with optional
//! per-track instrument, volume, and mute.
//!
//! # Pattern
//!
//! ```ignore
//! use verovio::midi::{MidiTrackPolicy, TrackOverride};
//! use std::collections::BTreeMap;
//!
//! let policy = MidiTrackPolicy {
//!     auto_distribute_channels: true,   // track 1 → ch 0, track 2 → ch 1, …
//!     overrides: BTreeMap::from([
//!         (1, TrackOverride { program: Some(0), .. Default::default() }),     // staff 1 → piano
//!         (2, TrackOverride { program: Some(42), volume: Some(96), .. Default::default() }), // staff 2 → cello, quieter
//!     ]),
//! };
//! let bytes = tk.render_to_midi_bytes_with_policy(&policy)?;
//! # Ok::<(), verovio::Error>(())
//! ```

use std::collections::BTreeMap;

use midly::{
    num::{u24, u4, u7},
    MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind,
};

use crate::{MeasureInfo, TempoMap};

pub mod gm {
    //! General MIDI program and percussion-key name lookup tables.
    //!
    //! Pair with [`TrackOverride::program`](super::TrackOverride::program)
    //! and `MidiMessage::NoteOn` on channel 9 (the GM drum channel) to
    //! build instrument-picker UIs without baking the name list at the
    //! call site.
    //!
    //! Program numbers are 0-indexed (GM spec) — UI presentations that
    //! show "Program 1" should add 1 at display time.
    //!
    //! Source: General MIDI Level 1 spec (MMA, 1991).
    const PROGRAMS: [&str; 128] = [
        "Acoustic Grand Piano", "Bright Acoustic Piano", "Electric Grand Piano", "Honky-tonk Piano",
        "Electric Piano 1", "Electric Piano 2", "Harpsichord", "Clavi",
        "Celesta", "Glockenspiel", "Music Box", "Vibraphone",
        "Marimba", "Xylophone", "Tubular Bells", "Dulcimer",
        "Drawbar Organ", "Percussive Organ", "Rock Organ", "Church Organ",
        "Reed Organ", "Accordion", "Harmonica", "Tango Accordion",
        "Acoustic Guitar (nylon)", "Acoustic Guitar (steel)", "Electric Guitar (jazz)", "Electric Guitar (clean)",
        "Electric Guitar (muted)", "Overdriven Guitar", "Distortion Guitar", "Guitar harmonics",
        "Acoustic Bass", "Electric Bass (finger)", "Electric Bass (pick)", "Fretless Bass",
        "Slap Bass 1", "Slap Bass 2", "Synth Bass 1", "Synth Bass 2",
        "Violin", "Viola", "Cello", "Contrabass",
        "Tremolo Strings", "Pizzicato Strings", "Orchestral Harp", "Timpani",
        "String Ensemble 1", "String Ensemble 2", "SynthStrings 1", "SynthStrings 2",
        "Choir Aahs", "Voice Oohs", "Synth Voice", "Orchestra Hit",
        "Trumpet", "Trombone", "Tuba", "Muted Trumpet",
        "French Horn", "Brass Section", "SynthBrass 1", "SynthBrass 2",
        "Soprano Sax", "Alto Sax", "Tenor Sax", "Baritone Sax",
        "Oboe", "English Horn", "Bassoon", "Clarinet",
        "Piccolo", "Flute", "Recorder", "Pan Flute",
        "Blown Bottle", "Shakuhachi", "Whistle", "Ocarina",
        "Lead 1 (square)", "Lead 2 (sawtooth)", "Lead 3 (calliope)", "Lead 4 (chiff)",
        "Lead 5 (charang)", "Lead 6 (voice)", "Lead 7 (fifths)", "Lead 8 (bass + lead)",
        "Pad 1 (new age)", "Pad 2 (warm)", "Pad 3 (polysynth)", "Pad 4 (choir)",
        "Pad 5 (bowed)", "Pad 6 (metallic)", "Pad 7 (halo)", "Pad 8 (sweep)",
        "FX 1 (rain)", "FX 2 (soundtrack)", "FX 3 (crystal)", "FX 4 (atmosphere)",
        "FX 5 (brightness)", "FX 6 (goblins)", "FX 7 (echoes)", "FX 8 (sci-fi)",
        "Sitar", "Banjo", "Shamisen", "Koto",
        "Kalimba", "Bagpipe", "Fiddle", "Shanai",
        "Tinkle Bell", "Agogo", "Steel Drums", "Woodblock",
        "Taiko Drum", "Melodic Tom", "Synth Drum", "Reverse Cymbal",
        "Guitar Fret Noise", "Breath Noise", "Seashore", "Bird Tweet",
        "Telephone Ring", "Helicopter", "Applause", "Gunshot",
    ];

    /// General MIDI Level 1 program name for the given 0-127 program
    /// number. Returns `None` for programs outside the 7-bit range.
    pub fn program_name(program: u8) -> Option<&'static str> {
        PROGRAMS.get(program as usize).copied()
    }

    /// All 128 General MIDI program names in order. Use for populating
    /// an instrument picker.
    pub fn all_programs() -> &'static [&'static str; 128] {
        &PROGRAMS
    }

    const NOTE_NAMES_SHARP: [&str; 12] =
        ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

    /// Pitch name for a MIDI key number in scientific pitch notation
    /// ("C-1" through "G9"). MIDI key 60 = "C4" (middle C). Returns
    /// `None` for keys outside the 0..=127 range — but every u8 fits,
    /// so this currently never returns None at the type level.
    pub fn note_name(key: u8) -> Option<String> {
        let pc = (key % 12) as usize;
        let octave = (key as i32 / 12) - 1;
        Some(format!("{}{}", NOTE_NAMES_SHARP[pc], octave))
    }

    /// Parse a scientific pitch notation string ("C4", "F#5", "Bb3") to
    /// a MIDI key number. Accepts both '#' and 'b' accidentals. Returns
    /// `None` on malformed input or out-of-range octaves.
    pub fn midi_key_from_name(name: &str) -> Option<u8> {
        let bytes = name.as_bytes();
        if bytes.is_empty() {
            return None;
        }
        let letter = bytes[0].to_ascii_uppercase();
        let mut pc: i32 = match letter {
            b'C' => 0,
            b'D' => 2,
            b'E' => 4,
            b'F' => 5,
            b'G' => 7,
            b'A' => 9,
            b'B' => 11,
            _ => return None,
        };
        let mut idx = 1;
        if idx < bytes.len() {
            match bytes[idx] {
                b'#' => {
                    pc += 1;
                    idx += 1;
                }
                b'b' => {
                    pc -= 1;
                    idx += 1;
                }
                _ => {}
            }
        }
        if idx >= bytes.len() {
            return None;
        }
        let octave: i32 = name[idx..].parse().ok()?;
        let key = (octave + 1) * 12 + pc;
        if (0..=127).contains(&key) {
            Some(key as u8)
        } else {
            None
        }
    }

    /// General MIDI Percussion key name (channel 10 / 0-indexed channel 9)
    /// for the given MIDI note number. Returns `None` for keys outside
    /// the standard 35-81 range.
    pub fn drum_key_name(key: u8) -> Option<&'static str> {
        match key {
            35 => Some("Acoustic Bass Drum"),
            36 => Some("Bass Drum 1"),
            37 => Some("Side Stick"),
            38 => Some("Acoustic Snare"),
            39 => Some("Hand Clap"),
            40 => Some("Electric Snare"),
            41 => Some("Low Floor Tom"),
            42 => Some("Closed Hi Hat"),
            43 => Some("High Floor Tom"),
            44 => Some("Pedal Hi-Hat"),
            45 => Some("Low Tom"),
            46 => Some("Open Hi-Hat"),
            47 => Some("Low-Mid Tom"),
            48 => Some("Hi-Mid Tom"),
            49 => Some("Crash Cymbal 1"),
            50 => Some("High Tom"),
            51 => Some("Ride Cymbal 1"),
            52 => Some("Chinese Cymbal"),
            53 => Some("Ride Bell"),
            54 => Some("Tambourine"),
            55 => Some("Splash Cymbal"),
            56 => Some("Cowbell"),
            57 => Some("Crash Cymbal 2"),
            58 => Some("Vibraslap"),
            59 => Some("Ride Cymbal 2"),
            60 => Some("Hi Bongo"),
            61 => Some("Low Bongo"),
            62 => Some("Mute Hi Conga"),
            63 => Some("Open Hi Conga"),
            64 => Some("Low Conga"),
            65 => Some("High Timbale"),
            66 => Some("Low Timbale"),
            67 => Some("High Agogo"),
            68 => Some("Low Agogo"),
            69 => Some("Cabasa"),
            70 => Some("Maracas"),
            71 => Some("Short Whistle"),
            72 => Some("Long Whistle"),
            73 => Some("Short Guiro"),
            74 => Some("Long Guiro"),
            75 => Some("Claves"),
            76 => Some("Hi Wood Block"),
            77 => Some("Low Wood Block"),
            78 => Some("Mute Cuica"),
            79 => Some("Open Cuica"),
            80 => Some("Mute Triangle"),
            81 => Some("Open Triangle"),
            _ => None,
        }
    }
}

/// Per-track overrides applied to a Verovio SMF by [`apply_track_policy`].
///
/// Track index `0` is the meta track upstream — applying overrides to it
/// is allowed (you'd typically only set `mute` to silence the entire piece
/// for testing) but won't affect note channels.
#[derive(Debug, Clone, Default)]
pub struct TrackOverride {
    /// MIDI channel (0-15) to reassign every Midi event in this track to.
    /// `None` keeps the channel Verovio emitted (typically 0).
    pub channel: Option<u8>,

    /// General MIDI program (0-127) to set as the instrument for this
    /// track's channel. Inserted as a ProgramChange at the start of the
    /// track. `None` leaves the synth default (program 0 / Acoustic Grand).
    pub program: Option<u8>,

    /// CC#7 (Channel Volume) value (0-127) to insert at the start of the
    /// track. `None` leaves the synth default (typically 100).
    pub volume: Option<u8>,

    /// If `true`, zero every NoteOn velocity in this track — effectively
    /// muting the track without removing its events (timing preserved for
    /// scheduling).
    pub mute: bool,

    /// CC#10 (Pan) value (0-127) to insert at the start of the track.
    /// `0` = hard left, `64` = center, `127` = hard right. `None` leaves
    /// the synth default (typically center 64).
    pub pan: Option<u8>,

    /// `MetaMessage::TrackName` inserted at the start of the track. DAWs
    /// surface this as the track's display name; useful for importing
    /// Verovio's output into Logic / Ableton / Reaper / etc.
    pub name: Option<String>,

    /// `MetaMessage::InstrumentName` inserted at the start of the track.
    /// Some DAWs prefer this over the GM program number for display.
    pub instrument_name: Option<String>,

    /// CC#64 (Damper / Sustain Pedal). `Some(true)` inserts a "pedal
    /// down" event at the start (value 127) for sustain on a piano-like
    /// track; `Some(false)` inserts an explicit "pedal up" (value 0);
    /// `None` leaves the synth default.
    pub sustain: Option<bool>,

    /// Transpose every NoteOn / NoteOff / Aftertouch key on this track by
    /// the given number of semitones. Useful for octave substitution
    /// (e.g., bass on guitar one octave higher) or capo-style shifts.
    /// Resulting MIDI keys are clamped to `0..=127`.
    pub transpose: Option<i8>,

    /// CC#11 (Expression Controller) value (0-127) inserted at start of
    /// the track. Distinct from volume (CC#7): expression is meant for
    /// gradual swells / dynamic shaping, while volume sets a track's
    /// nominal level.
    pub expression: Option<u8>,

    /// CC#1 (Modulation) value (0-127) inserted at start of the track.
    /// Usually controls vibrato depth on synths.
    pub modulation: Option<u8>,

    /// CC#91 (Reverb Send / Effects 1 Depth) value (0-127) at start.
    /// On GM synths sets the wet-mix amount for the reverb effect.
    pub reverb: Option<u8>,

    /// CC#93 (Chorus Send / Effects 3 Depth) value (0-127) at start.
    /// On GM synths sets the wet-mix amount for the chorus effect.
    pub chorus: Option<u8>,

    /// Bank select (CC#0 MSB + CC#32 LSB) inserted at start of track,
    /// immediately before the ProgramChange. Use to address instruments
    /// beyond the GM 128 — e.g. GS / XG variation banks or SoundFont
    /// banks. Tuple is `(msb, lsb)`. If `program` is also set, the bank
    /// select pair precedes the program change so synths apply both.
    pub bank_select: Option<(u8, u8)>,

    /// `MetaMessage::MidiPort` port number inserted at start of the track.
    /// SMF convention for routing a track to a specific MIDI output port
    /// when the file is played through a multi-port interface. Range 0-127.
    pub midi_port: Option<u8>,
}

impl MidiTrackPolicy {
    /// Construct a policy that **solos** the given track indices: every
    /// non-meta track NOT in `audible` is muted (NoteOn velocities zeroed).
    /// Track 0 (meta) is always left alone.
    ///
    /// Convenience for the common practice-mode pattern "play only the
    /// piano track" without building the `BTreeMap` by hand.
    ///
    /// Combine with other fields via the standard `..Default::default()`
    /// pattern, or chain with `with_auto_distribute_channels()`.
    pub fn with_solo(audible: &[u32]) -> Self {
        let mut overrides = BTreeMap::new();
        // We mute everything; the caller-loaded SMF reveals N at apply
        // time. Until then we set mute on every plausibly-existing track
        // index up to 32 except the audible ones. Apply-time tracks past
        // 32 (rare) would still play — acceptable, since the policy can
        // be regenerated after `summarize` reports the real count.
        for idx in 1u32..32 {
            if !audible.contains(&idx) {
                overrides.insert(
                    idx,
                    TrackOverride {
                        mute: true,
                        ..Default::default()
                    },
                );
            }
        }
        Self {
            overrides,
            ..Default::default()
        }
    }

    /// Construct a policy that **mutes** the given track indices.
    /// Inverse of [`Self::with_solo`].
    pub fn with_mute(silent: &[u32]) -> Self {
        let mut overrides = BTreeMap::new();
        for idx in silent {
            overrides.insert(
                *idx,
                TrackOverride {
                    mute: true,
                    ..Default::default()
                },
            );
        }
        Self {
            overrides,
            ..Default::default()
        }
    }

    /// Turn on per-track channel distribution. Chainable with the
    /// construction helpers above.
    pub fn with_auto_distribute_channels(mut self) -> Self {
        self.auto_distribute_channels = true;
        self
    }

    /// Construct a policy that assigns each `(track_index, program)`
    /// pair as a `TrackOverride.program` setting. Existing entries in
    /// `overrides` for the same track index are replaced.
    ///
    /// Pair with [`gm::program_name`] to surface a human label in the UI.
    pub fn with_programs(assignments: &[(u32, u8)]) -> Self {
        let mut overrides = BTreeMap::new();
        for (idx, prog) in assignments {
            overrides.insert(
                *idx,
                TrackOverride {
                    program: Some(*prog),
                    ..Default::default()
                },
            );
        }
        Self {
            overrides,
            ..Default::default()
        }
    }
}

/// Per-track policy applied to a Verovio SMF. Combine with
/// [`apply_track_policy`] or
/// [`Toolkit::render_to_midi_bytes_with_policy`](crate::Toolkit::render_to_midi_bytes_with_policy).
#[derive(Debug, Clone, Default)]
pub struct MidiTrackPolicy {
    /// Per-track-index overrides. Tracks not in the map are kept as
    /// Verovio emitted them.
    pub overrides: BTreeMap<u32, TrackOverride>,

    /// If `true`, every non-meta track is reassigned to its own channel
    /// (track 1 → ch 0, track 2 → ch 1, …). Channels > 15 wrap. Useful for
    /// turning Verovio's "everything on channel 0" output into real
    /// multi-channel MIDI without writing per-track channel overrides.
    /// Per-track `channel` overrides in [`Self::overrides`] take precedence.
    pub auto_distribute_channels: bool,

    /// Replace Verovio's `MetaMessage::Tempo` events on the meta track
    /// with events derived from this [`TempoMap`]. Useful for "render at
    /// constant 80 BPM regardless of score markings" (pass a single-entry
    /// TempoMap) or applying a custom tempo curve for practice playback.
    ///
    /// Empty / `None` leaves Verovio's tempo events untouched.
    pub tempo_override: Option<TempoMap>,

    /// Insert a `MetaMessage::TimeSignature` on the meta track at t=0.
    /// Tuple is `(numerator, denominator)`; e.g. `(4, 4)` for common time
    /// or `(6, 8)` for compound duple. Verovio's SMF output doesn't emit
    /// a time-signature meta event, so DAWs importing the file fall back
    /// to 4/4; setting this fixes that.
    pub time_signature: Option<(u8, u8)>,

    /// Insert a `MetaMessage::KeySignature` on the meta track at t=0.
    /// Value is the SMF convention: `0` for C major / A minor, positive
    /// for sharps (+1 = G major / E minor), negative for flats
    /// (-1 = F major / D minor). Range -7..=7.
    pub key_signature: Option<i8>,

    /// If `true`, sets [`Self::key_signature`]'s mode bit to minor.
    /// Ignored if `key_signature` is `None`. Default `false` (major).
    pub key_signature_minor: bool,

    /// Auto-insert a `MetaMessage::Marker` on the meta track at the start
    /// of every measure, labeled with the measure's MEI ID. Useful for
    /// navigation in DAWs that surface SMF markers as jump points.
    /// Build the vec with [`crate::Toolkit::measures`] and pass it here.
    pub measure_markers: Option<Vec<MeasureInfo>>,

    /// Insert `MetaMessage::Lyric` events on the meta track at the given
    /// `(quarter_stamp, text)` positions. Quarter-note timestamps are
    /// converted to SMF ticks using the file's ticks-per-quarter. Most
    /// karaoke players and DAWs recognize these as the lyric stream.
    pub lyrics: Option<Vec<(f64, String)>>,

    /// Insert `MetaMessage::CuePoint` events on the meta track at the
    /// given `(quarter_stamp, label)` positions. Conventionally used for
    /// section labels (verse / chorus / coda) consumed by DAWs and
    /// notation editors.
    pub cue_points: Option<Vec<(f64, String)>>,
}

/// Apply `policy` to a Verovio-rendered SMF (or any Format-1 SMF) and
/// return the modified bytes. Pure function — no Toolkit required.
///
/// Returns `None` if the input isn't a valid SMF.
pub fn apply_track_policy(smf_bytes: &[u8], policy: &MidiTrackPolicy) -> Option<Vec<u8>> {
    let smf = Smf::parse(smf_bytes).ok()?;
    let new_smf = apply_policy_to_parsed(smf, policy);
    let mut out = Vec::new();
    new_smf.write_std(&mut out).ok()?;
    Some(out)
}

/// Convert a u32 tick count into an SMF `Smf`-friendly delta-encoded
/// `(absolute_tick, kind)` tuple. Used by tempo-override rewriting.
fn absolute_ticks<'a>(track: &[TrackEvent<'a>]) -> Vec<(u64, TrackEventKind<'a>)> {
    let mut out = Vec::with_capacity(track.len());
    let mut tick: u64 = 0;
    for ev in track {
        tick += u32::from(ev.delta) as u64;
        out.push((tick, ev.kind.clone()));
    }
    out
}

/// Convert back from absolute ticks to a delta-encoded track.
fn delta_encode<'a>(events: Vec<(u64, TrackEventKind<'a>)>) -> Vec<TrackEvent<'a>> {
    let mut out = Vec::with_capacity(events.len());
    let mut last_tick: u64 = 0;
    for (tick, kind) in events {
        let delta = (tick - last_tick) as u32;
        out.push(TrackEvent {
            delta: delta.into(),
            kind,
        });
        last_tick = tick;
    }
    out
}

fn apply_tempo_override<'a>(meta_track: &mut Vec<TrackEvent<'a>>, tempo_map: &TempoMap, tpq: u64) {
    if tempo_map.changes.is_empty() {
        return;
    }
    let mut events_abs = absolute_ticks(meta_track);
    // Drop any existing tempo events.
    events_abs.retain(|(_, kind)| !matches!(kind, TrackEventKind::Meta(MetaMessage::Tempo(_))));
    // Insert one Tempo meta event per TempoChange at the matching tick.
    for change in &tempo_map.changes {
        let tick = (change.at_qstamp * tpq as f64).round() as u64;
        let uspq = (60_000_000.0 / change.bpm).round().max(1.0) as u32;
        events_abs.push((
            tick,
            TrackEventKind::Meta(MetaMessage::Tempo(u24::from(uspq.min(0x00FF_FFFF)))),
        ));
    }
    // Stable sort by tick keeps EndOfTrack at the tail among same-tick events.
    events_abs.sort_by_key(|(t, _)| *t);
    *meta_track = delta_encode(events_abs);
}

fn apply_policy_to_parsed<'a>(mut smf: Smf<'a>, policy: &MidiTrackPolicy) -> Smf<'a> {
    // Resolve ticks-per-quarter from the SMF header — needed for any
    // tick-based meta rewriting below.
    let tpq: u64 = match smf.header.timing {
        Timing::Metrical(t) => t.as_int() as u64,
        Timing::Timecode(_, _) => 480, // Verovio doesn't emit SMPTE; reasonable fallback.
    };

    // Tempo override / time-sig / key-sig affect the meta track (index 0).
    if !smf.tracks.is_empty() {
        let meta_track = &mut smf.tracks[0];

        if let Some(tempo_map) = &policy.tempo_override {
            apply_tempo_override(meta_track, tempo_map, tpq);
        }

        // Prepend time/key signature meta events at the start of the
        // meta track. Order: TimeSig before KeySig (DAW convention).
        let mut prepend_meta: Vec<TrackEvent<'a>> = Vec::new();
        if let Some((num, denom)) = policy.time_signature {
            // SMF denominator is the power: 4 → 2 (2^2 = 4), 8 → 3, etc.
            let denom_power = denom.trailing_zeros().min(7) as u8;
            prepend_meta.push(TrackEvent {
                delta: 0.into(),
                kind: TrackEventKind::Meta(MetaMessage::TimeSignature(num, denom_power, 24, 8)),
            });
        }
        if let Some(sf) = policy.key_signature {
            prepend_meta.push(TrackEvent {
                delta: 0.into(),
                kind: TrackEventKind::Meta(MetaMessage::KeySignature(
                    sf,
                    policy.key_signature_minor,
                )),
            });
        }
        if !prepend_meta.is_empty() {
            for (i, ev) in prepend_meta.into_iter().enumerate() {
                meta_track.insert(i, ev);
            }
        }

        // Insert MetaMessage::Marker events at measure boundaries.
        if let Some(measures) = &policy.measure_markers {
            let mut events_abs = absolute_ticks(meta_track);
            for m in measures {
                let denom = m.start_qfrac[1] as f64;
                if denom == 0.0 {
                    continue;
                }
                let q = m.start_qfrac[0] as f64 / denom;
                let tick = (q * tpq as f64).round() as u64;
                let label: &'static [u8] = Box::leak(m.id.clone().into_bytes().into_boxed_slice());
                events_abs.push((tick, TrackEventKind::Meta(MetaMessage::Marker(label))));
            }
            events_abs.sort_by_key(|(t, _)| *t);
            *meta_track = delta_encode(events_abs);
        }

        // Insert MetaMessage::Lyric events at quarter-note timestamps.
        if let Some(lyrics) = &policy.lyrics {
            let mut events_abs = absolute_ticks(meta_track);
            for (q, text) in lyrics {
                let tick = (q * tpq as f64).round().max(0.0) as u64;
                let label: &'static [u8] =
                    Box::leak(text.clone().into_bytes().into_boxed_slice());
                events_abs.push((tick, TrackEventKind::Meta(MetaMessage::Lyric(label))));
            }
            events_abs.sort_by_key(|(t, _)| *t);
            *meta_track = delta_encode(events_abs);
        }

        // Insert MetaMessage::CuePoint events at quarter-note timestamps.
        if let Some(cues) = &policy.cue_points {
            let mut events_abs = absolute_ticks(meta_track);
            for (q, text) in cues {
                let tick = (q * tpq as f64).round().max(0.0) as u64;
                let label: &'static [u8] =
                    Box::leak(text.clone().into_bytes().into_boxed_slice());
                events_abs.push((tick, TrackEventKind::Meta(MetaMessage::CuePoint(label))));
            }
            events_abs.sort_by_key(|(t, _)| *t);
            *meta_track = delta_encode(events_abs);
        }
    }

    for (idx, track) in smf.tracks.iter_mut().enumerate() {
        let track_index = idx as u32;
        let override_ = policy.overrides.get(&track_index);

        // Decide which channel every Midi event in this track gets.
        let target_channel: Option<u8> = override_.and_then(|o| o.channel).or_else(|| {
            if policy.auto_distribute_channels && track_index > 0 {
                // Track 1 → ch 0, track 2 → ch 1, …. Wrap at 16.
                Some(((track_index - 1) % 16) as u8)
            } else {
                None
            }
        });

        // 1) Reassign channel + apply mute + apply transpose on existing
        //    events.
        let mute = override_.map(|o| o.mute).unwrap_or(false);
        let transpose = override_.and_then(|o| o.transpose).unwrap_or(0);
        if target_channel.is_some() || mute || transpose != 0 {
            for ev in track.iter_mut() {
                if let TrackEventKind::Midi { channel, message } = &mut ev.kind {
                    if let Some(ch) = target_channel {
                        *channel = u4::from(ch & 0x0F);
                    }
                    if mute {
                        if let MidiMessage::NoteOn { vel, .. } = message {
                            *vel = u7::from(0);
                        }
                    }
                    if transpose != 0 {
                        match message {
                            MidiMessage::NoteOn { key, .. }
                            | MidiMessage::NoteOff { key, .. }
                            | MidiMessage::Aftertouch { key, .. } => {
                                let new_key =
                                    (key.as_int() as i16 + transpose as i16).clamp(0, 127) as u8;
                                *key = u7::from(new_key);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        // 2) Insert ProgramChange and Volume CC at the start, if requested.
        if let Some(o) = override_ {
            let mut prepend: Vec<TrackEvent<'a>> = Vec::new();
            let ch = target_channel.unwrap_or(0);
            if let Some((msb, lsb)) = o.bank_select {
                // Bank select MSB (CC#0) then LSB (CC#32) — order matters
                // on most synths: MSB latches the bank "page", LSB picks
                // the variation within it, and the subsequent ProgramChange
                // commits the selection.
                prepend.push(TrackEvent {
                    delta: 0.into(),
                    kind: TrackEventKind::Midi {
                        channel: u4::from(ch & 0x0F),
                        message: MidiMessage::Controller {
                            controller: u7::from(0),
                            value: u7::from(msb & 0x7F),
                        },
                    },
                });
                prepend.push(TrackEvent {
                    delta: 0.into(),
                    kind: TrackEventKind::Midi {
                        channel: u4::from(ch & 0x0F),
                        message: MidiMessage::Controller {
                            controller: u7::from(32),
                            value: u7::from(lsb & 0x7F),
                        },
                    },
                });
            }
            if let Some(port) = o.midi_port {
                prepend.push(TrackEvent {
                    delta: 0.into(),
                    kind: TrackEventKind::Meta(MetaMessage::MidiPort(u7::from(port & 0x7F))),
                });
            }
            if let Some(program) = o.program {
                prepend.push(TrackEvent {
                    delta: 0.into(),
                    kind: TrackEventKind::Midi {
                        channel: u4::from(ch & 0x0F),
                        message: MidiMessage::ProgramChange {
                            program: u7::from(program & 0x7F),
                        },
                    },
                });
            }
            if let Some(volume) = o.volume {
                prepend.push(TrackEvent {
                    delta: 0.into(),
                    kind: TrackEventKind::Midi {
                        channel: u4::from(ch & 0x0F),
                        message: MidiMessage::Controller {
                            controller: u7::from(7),
                            value: u7::from(volume & 0x7F),
                        },
                    },
                });
            }
            if let Some(pan) = o.pan {
                prepend.push(TrackEvent {
                    delta: 0.into(),
                    kind: TrackEventKind::Midi {
                        channel: u4::from(ch & 0x0F),
                        message: MidiMessage::Controller {
                            controller: u7::from(10),
                            value: u7::from(pan & 0x7F),
                        },
                    },
                });
            }
            if let Some(sustain_down) = o.sustain {
                let value: u8 = if sustain_down { 127 } else { 0 };
                prepend.push(TrackEvent {
                    delta: 0.into(),
                    kind: TrackEventKind::Midi {
                        channel: u4::from(ch & 0x0F),
                        message: MidiMessage::Controller {
                            controller: u7::from(64),
                            value: u7::from(value & 0x7F),
                        },
                    },
                });
            }
            if let Some(expression) = o.expression {
                prepend.push(TrackEvent {
                    delta: 0.into(),
                    kind: TrackEventKind::Midi {
                        channel: u4::from(ch & 0x0F),
                        message: MidiMessage::Controller {
                            controller: u7::from(11),
                            value: u7::from(expression & 0x7F),
                        },
                    },
                });
            }
            if let Some(modulation) = o.modulation {
                prepend.push(TrackEvent {
                    delta: 0.into(),
                    kind: TrackEventKind::Midi {
                        channel: u4::from(ch & 0x0F),
                        message: MidiMessage::Controller {
                            controller: u7::from(1),
                            value: u7::from(modulation & 0x7F),
                        },
                    },
                });
            }
            if let Some(reverb) = o.reverb {
                prepend.push(TrackEvent {
                    delta: 0.into(),
                    kind: TrackEventKind::Midi {
                        channel: u4::from(ch & 0x0F),
                        message: MidiMessage::Controller {
                            controller: u7::from(91),
                            value: u7::from(reverb & 0x7F),
                        },
                    },
                });
            }
            if let Some(chorus) = o.chorus {
                prepend.push(TrackEvent {
                    delta: 0.into(),
                    kind: TrackEventKind::Midi {
                        channel: u4::from(ch & 0x0F),
                        message: MidiMessage::Controller {
                            controller: u7::from(93),
                            value: u7::from(chorus & 0x7F),
                        },
                    },
                });
            }
            if let Some(name) = &o.name {
                // midly's MetaMessage variants borrow from the SMF's
                // input buffer, so we leak a Vec into a 'static slice.
                // For our use case the policy is short-lived and the
                // produced SMF is immediately serialized, but keeping
                // ownership through a 'static escape is the cleanest
                // way to satisfy midly's lifetime parameter.
                let leaked: &'static [u8] = Box::leak(name.clone().into_bytes().into_boxed_slice());
                prepend.push(TrackEvent {
                    delta: 0.into(),
                    kind: TrackEventKind::Meta(MetaMessage::TrackName(leaked)),
                });
            }
            if let Some(iname) = &o.instrument_name {
                let leaked: &'static [u8] =
                    Box::leak(iname.clone().into_bytes().into_boxed_slice());
                prepend.push(TrackEvent {
                    delta: 0.into(),
                    kind: TrackEventKind::Meta(MetaMessage::InstrumentName(leaked)),
                });
            }
            if !prepend.is_empty() {
                // Find the first non-meta event (or the EndOfTrack) and
                // splice the prepends right before it so timing is preserved.
                let insert_at = track
                    .iter()
                    .position(|ev| !matches!(ev.kind, TrackEventKind::Meta(_)))
                    .unwrap_or(track.len());
                for (offset, ev) in prepend.into_iter().enumerate() {
                    track.insert(insert_at + offset, ev);
                }
            }
        }

        // Suppress unused-variable warnings on EndOfTrack-only tracks.
        let _ = MetaMessage::EndOfTrack;
    }
    smf
}

/// Per-track summary of an SMF — useful for inspecting what an SMF
/// actually contains before / after applying a [`MidiTrackPolicy`].
///
/// Returned by [`summarize`].
#[derive(Debug, Clone, PartialEq)]
pub struct TrackInfo {
    /// 0-indexed track position in the SMF.
    pub track_index: u32,
    /// Unique MIDI channels referenced by Midi events on this track.
    /// Verovio's default output puts everything on `[0]` for every staff
    /// track; after `auto_distribute_channels`, each non-meta track has
    /// its own channel.
    pub channels: Vec<u8>,
    /// Number of `NoteOn` events with `vel > 0` (i.e. actually audible
    /// onsets, not the velocity-0 form used as `NoteOff`).
    pub audible_note_count: u32,
    /// First `ProgramChange` program number on this track, if any.
    pub program: Option<u8>,
    /// First CC#7 (Channel Volume) value on this track, if any.
    pub volume: Option<u8>,
    /// First CC#10 (Pan) value on this track, if any.
    pub pan: Option<u8>,
    /// Sum of all `delta` ticks on the track — its total length in ticks.
    pub end_tick: u64,
}

/// Build a "MIDI panic" SMF — a single-track Format-0 file that sends
/// CC#123 (All Notes Off) and CC#120 (All Sound Off) on every channel.
/// Useful as an emergency stop when notes have been left hanging by a
/// scheduler crash or paused playback; pipe the bytes through a synth
/// to flush stuck voices without restarting the audio engine.
///
/// The output is a minimal valid SMF: 120 ticks-per-quarter, one track
/// with 32 controller events (16 channels × 2 messages) all at delta 0.
pub fn build_panic_smf() -> Vec<u8> {
    use midly::{Format, Header};

    let mut events: Vec<TrackEvent<'static>> = Vec::with_capacity(33);
    for ch in 0u8..16 {
        events.push(TrackEvent {
            delta: 0.into(),
            kind: TrackEventKind::Midi {
                channel: u4::from(ch),
                message: MidiMessage::Controller {
                    controller: u7::from(120),
                    value: u7::from(0),
                },
            },
        });
        events.push(TrackEvent {
            delta: 0.into(),
            kind: TrackEventKind::Midi {
                channel: u4::from(ch),
                message: MidiMessage::Controller {
                    controller: u7::from(123),
                    value: u7::from(0),
                },
            },
        });
    }
    events.push(TrackEvent {
        delta: 0.into(),
        kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
    });

    let smf = Smf {
        header: Header::new(Format::SingleTrack, Timing::Metrical(120.into())),
        tracks: vec![events],
    };
    let mut out = Vec::new();
    smf.write_std(&mut out).expect("static SMF write");
    out
}

/// One scheduled MIDI event with its wall-clock onset in milliseconds.
/// Produced by [`iter_smf_events`] for direct consumption by a software
/// synth or a `tokio::time::sleep_until`-style scheduler.
///
/// `track_index` is the SMF track this event came from — useful for
/// cross-referencing with a [`crate::Toolkit::staff_map`] or the
/// per-track summary from [`summarize`].
#[derive(Debug, Clone, PartialEq)]
pub struct TimedEvent {
    /// Wall-clock milliseconds from the start of the SMF.
    pub at_ms: f64,
    /// 0-indexed SMF track the event came from.
    pub track_index: u32,
    /// MIDI channel (0-15) the event acts on. `None` for meta events.
    pub channel: Option<u8>,
    /// The MIDI message itself. `MidiMessage::NoteOn { vel: 0 }` is the
    /// SMF convention for NoteOff — left as-is so the caller can decide
    /// whether to normalize.
    pub message: TimedMessage,
}

/// Owned, channel-stripped subset of MIDI messages exposed by
/// [`TimedEvent`]. We project here rather than handing back midly's
/// borrowed types so callers don't have to manage lifetimes against the
/// parsed SMF buffer.
#[derive(Debug, Clone, PartialEq)]
pub enum TimedMessage {
    NoteOn { key: u8, vel: u8 },
    NoteOff { key: u8, vel: u8 },
    Aftertouch { key: u8, vel: u8 },
    Controller { controller: u8, value: u8 },
    ProgramChange { program: u8 },
    ChannelAftertouch { vel: u8 },
    PitchBend { value: i16 },
    /// Tempo change in microseconds-per-quarter (SMF native unit).
    /// Convert to BPM with `60_000_000.0 / usec_per_quarter as f64`.
    Tempo { usec_per_quarter: u32 },
    /// Other meta events (text, marker, lyric, time-sig, key-sig, …)
    /// surfaced as an opaque kind byte and payload. Convert in the caller
    /// if needed.
    Meta { kind: u8, data: Vec<u8> },
    /// SysEx or escape sysex; payload is the raw bytes after F0.
    SysEx { data: Vec<u8> },
}

/// Convert a Verovio-rendered SMF (or any Format-0/1 SMF) into a flat,
/// chronologically-sorted stream of [`TimedEvent`]s, with onset
/// timestamps in wall-clock milliseconds.
///
/// The conversion uses the SMF's own `MetaMessage::Tempo` events on the
/// meta track to map ticks → ms. SMPTE timing is approximated as 480
/// TPQ at 120 BPM (Verovio doesn't emit SMPTE so this branch is
/// defensive).
///
/// Returns `None` if `smf_bytes` isn't a valid SMF.
///
/// # Use case
///
/// Drive a software synth without writing your own SMF parser:
///
/// ```ignore
/// let bytes = tk.render_to_midi_bytes_with_policy(&policy)?;
/// for ev in verovio::midi::iter_smf_events(&bytes).unwrap() {
///     scheduler.dispatch_at(ev.at_ms, ev.channel, ev.message);
/// }
/// # Ok::<(), verovio::Error>(())
/// ```
pub fn iter_smf_events(smf_bytes: &[u8]) -> Option<Vec<TimedEvent>> {
    let smf = Smf::parse(smf_bytes).ok()?;
    let tpq: u32 = match smf.header.timing {
        Timing::Metrical(t) => t.as_int() as u32,
        Timing::Timecode(_, _) => 480,
    };

    // First pass: build a tick → ms function from meta-track Tempo events.
    // Default tempo if the SMF doesn't set one is 500_000 µs/qtr (120 BPM).
    let mut tempo_points: Vec<(u64, u32)> = vec![(0, 500_000)];
    if let Some(meta) = smf.tracks.first() {
        let mut tick: u64 = 0;
        for ev in meta {
            tick += u32::from(ev.delta) as u64;
            if let TrackEventKind::Meta(MetaMessage::Tempo(t)) = &ev.kind {
                let us = t.as_int();
                if tick == 0 && tempo_points.len() == 1 && tempo_points[0].0 == 0 {
                    tempo_points[0].1 = us;
                } else {
                    tempo_points.push((tick, us));
                }
            }
        }
    }
    tempo_points.sort_by_key(|(t, _)| *t);

    // Convert an absolute tick to wall-clock ms by integrating the tempo
    // segments. O(segments) per call; segment count is tiny in practice.
    let ticks_to_ms = |target: u64| -> f64 {
        let mut ms = 0.0;
        for i in 0..tempo_points.len() {
            let (seg_start, usec_per_q) = tempo_points[i];
            let seg_end = tempo_points
                .get(i + 1)
                .map(|(t, _)| *t)
                .unwrap_or(u64::MAX);
            if target <= seg_start {
                break;
            }
            let bound = target.min(seg_end);
            let span_ticks = bound.saturating_sub(seg_start) as f64;
            // ticks * (us/qtr) / TPQ = us; / 1000 = ms.
            ms += span_ticks * usec_per_q as f64 / tpq as f64 / 1000.0;
            if target <= seg_end {
                break;
            }
        }
        ms
    };

    let mut out: Vec<TimedEvent> = Vec::new();
    for (track_idx, track) in smf.tracks.iter().enumerate() {
        let mut tick: u64 = 0;
        for ev in track {
            tick += u32::from(ev.delta) as u64;
            let at_ms = ticks_to_ms(tick);
            match &ev.kind {
                TrackEventKind::Midi { channel, message } => {
                    let ch = Some(channel.as_int());
                    let msg = match message {
                        MidiMessage::NoteOn { key, vel } => TimedMessage::NoteOn {
                            key: key.as_int(),
                            vel: vel.as_int(),
                        },
                        MidiMessage::NoteOff { key, vel } => TimedMessage::NoteOff {
                            key: key.as_int(),
                            vel: vel.as_int(),
                        },
                        MidiMessage::Aftertouch { key, vel } => TimedMessage::Aftertouch {
                            key: key.as_int(),
                            vel: vel.as_int(),
                        },
                        MidiMessage::Controller { controller, value } => TimedMessage::Controller {
                            controller: controller.as_int(),
                            value: value.as_int(),
                        },
                        MidiMessage::ProgramChange { program } => TimedMessage::ProgramChange {
                            program: program.as_int(),
                        },
                        MidiMessage::ChannelAftertouch { vel } => TimedMessage::ChannelAftertouch {
                            vel: vel.as_int(),
                        },
                        MidiMessage::PitchBend { bend } => TimedMessage::PitchBend {
                            value: bend.0.as_int() as i16 - 8192,
                        },
                    };
                    out.push(TimedEvent {
                        at_ms,
                        track_index: track_idx as u32,
                        channel: ch,
                        message: msg,
                    });
                }
                TrackEventKind::Meta(meta) => {
                    let projected = match meta {
                        MetaMessage::Tempo(t) => Some(TimedMessage::Tempo {
                            usec_per_quarter: t.as_int(),
                        }),
                        MetaMessage::EndOfTrack => None, // skip — not useful to a scheduler
                        other => {
                            // Surface every other meta as opaque (kind byte + payload).
                            // Project to bytes by re-serializing the meta event would
                            // require running midly's writer; instead, sniff just the
                            // payload of the variants we care about.
                            let (kind, data) = meta_kind_and_data(other);
                            Some(TimedMessage::Meta { kind, data })
                        }
                    };
                    if let Some(message) = projected {
                        out.push(TimedEvent {
                            at_ms,
                            track_index: track_idx as u32,
                            channel: None,
                            message,
                        });
                    }
                }
                TrackEventKind::SysEx(bytes) | TrackEventKind::Escape(bytes) => {
                    out.push(TimedEvent {
                        at_ms,
                        track_index: track_idx as u32,
                        channel: None,
                        message: TimedMessage::SysEx {
                            data: bytes.to_vec(),
                        },
                    });
                }
            }
        }
    }
    // Stable sort by onset time so concurrent events on different tracks
    // interleave deterministically.
    out.sort_by(|a, b| a.at_ms.partial_cmp(&b.at_ms).unwrap_or(std::cmp::Ordering::Equal));
    Some(out)
}

/// Project a midly `MetaMessage` to a `(kind, payload)` pair using the SMF
/// status byte convention. Only the common variants xpart-class clients
/// might want to react to are projected verbatim; everything else gets
/// kind=0xFF and an empty payload (a no-op marker).
fn meta_kind_and_data(meta: &MetaMessage) -> (u8, Vec<u8>) {
    match meta {
        MetaMessage::TrackNumber(n) => (
            0x00,
            n.map(|x| x.to_be_bytes().to_vec()).unwrap_or_default(),
        ),
        MetaMessage::Text(b) => (0x01, b.to_vec()),
        MetaMessage::Copyright(b) => (0x02, b.to_vec()),
        MetaMessage::TrackName(b) => (0x03, b.to_vec()),
        MetaMessage::InstrumentName(b) => (0x04, b.to_vec()),
        MetaMessage::Lyric(b) => (0x05, b.to_vec()),
        MetaMessage::Marker(b) => (0x06, b.to_vec()),
        MetaMessage::CuePoint(b) => (0x07, b.to_vec()),
        MetaMessage::ProgramName(b) => (0x08, b.to_vec()),
        MetaMessage::DeviceName(b) => (0x09, b.to_vec()),
        MetaMessage::MidiChannel(c) => (0x20, vec![c.as_int()]),
        MetaMessage::MidiPort(p) => (0x21, vec![p.as_int()]),
        MetaMessage::TimeSignature(n, d, c, b) => (0x58, vec![*n, *d, *c, *b]),
        MetaMessage::KeySignature(sf, minor) => (0x59, vec![*sf as u8, *minor as u8]),
        MetaMessage::SequencerSpecific(b) => (0x7F, b.to_vec()),
        MetaMessage::SmpteOffset(_) => (0x54, Vec::new()),
        MetaMessage::Unknown(k, b) => (*k, b.to_vec()),
        _ => (0xFF, Vec::new()),
    }
}

/// Parse `smf_bytes` and return a [`TrackInfo`] for each track. Returns
/// `None` if the input isn't a valid SMF.
///
/// Pure function over [`midly::Smf`]; useful for tests, debugging, and
/// surfacing the actual channel / instrument / volume distribution to a
/// consumer that wants to verify a policy applied correctly.
pub fn summarize(smf_bytes: &[u8]) -> Option<Vec<TrackInfo>> {
    let smf = Smf::parse(smf_bytes).ok()?;
    let mut out = Vec::with_capacity(smf.tracks.len());
    for (idx, track) in smf.tracks.iter().enumerate() {
        let mut channels: Vec<u8> = Vec::new();
        let mut audible_note_count: u32 = 0;
        let mut program: Option<u8> = None;
        let mut volume: Option<u8> = None;
        let mut pan: Option<u8> = None;
        let mut end_tick: u64 = 0;
        for ev in track.iter() {
            end_tick += u32::from(ev.delta) as u64;
            if let TrackEventKind::Midi { channel, message } = &ev.kind {
                let ch = channel.as_int();
                if !channels.contains(&ch) {
                    channels.push(ch);
                }
                match message {
                    MidiMessage::NoteOn { vel, .. } if vel.as_int() > 0 => {
                        audible_note_count += 1;
                    }
                    MidiMessage::ProgramChange { program: p } => {
                        if program.is_none() {
                            program = Some(p.as_int());
                        }
                    }
                    MidiMessage::Controller { controller, value } => match controller.as_int() {
                        7 if volume.is_none() => volume = Some(value.as_int()),
                        10 if pan.is_none() => pan = Some(value.as_int()),
                        _ => {}
                    },
                    _ => {}
                }
            }
        }
        out.push(TrackInfo {
            track_index: idx as u32,
            channels,
            audible_note_count,
            program,
            volume,
            pan,
            end_tick,
        });
    }
    Some(out)
}
