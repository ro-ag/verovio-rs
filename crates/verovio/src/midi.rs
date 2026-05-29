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
    num::{u4, u7},
    MetaMessage, MidiMessage, Smf, TrackEvent, TrackEventKind,
};

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

fn apply_policy_to_parsed<'a>(mut smf: Smf<'a>, policy: &MidiTrackPolicy) -> Smf<'a> {
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

        // 1) Reassign channel + apply mute on existing events.
        if target_channel.is_some() || override_.map(|o| o.mute).unwrap_or(false) {
            for ev in track.iter_mut() {
                if let TrackEventKind::Midi { channel, message } = &mut ev.kind {
                    if let Some(ch) = target_channel {
                        *channel = u4::from(ch & 0x0F);
                    }
                    if override_.map(|o| o.mute).unwrap_or(false) {
                        if let MidiMessage::NoteOn { vel, .. } = message {
                            *vel = u7::from(0);
                        }
                    }
                }
            }
        }

        // 2) Insert ProgramChange and Volume CC at the start, if requested.
        if let Some(o) = override_ {
            let mut prepend: Vec<TrackEvent<'a>> = Vec::new();
            let ch = target_channel.unwrap_or(0);
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
