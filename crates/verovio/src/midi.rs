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

use crate::TempoMap;

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
