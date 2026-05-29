//! Tests for `verovio::midi` SMF post-processing — verifying that the
//! rewritten SMF bytes really do contain the channels / program / volume
//! / mute we asked for.

use std::collections::BTreeMap;

use midly::{MetaMessage, MidiMessage, Smf, TrackEventKind};
use verovio::midi::{MidiTrackPolicy, TrackOverride};
use verovio::Toolkit;

const TWO_STAFF_MEI: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<mei xmlns="http://www.music-encoding.org/ns/mei" meiversion="4.0.0">
  <meiHead><fileDesc><titleStmt><title>two-staff</title></titleStmt><pubStmt/></fileDesc></meiHead>
  <music><body><mdiv><score>
    <scoreDef><staffGrp>
      <staffDef n="1" lines="5" clef.shape="G" clef.line="2"/>
      <staffDef n="2" lines="5" clef.shape="F" clef.line="4"/>
    </staffGrp></scoreDef>
    <section><measure>
      <staff n="1"><layer>
        <note pname="g" oct="4" dur="4" xml:id="t1"/>
        <note pname="g" oct="4" dur="4" xml:id="t2"/>
        <note pname="g" oct="4" dur="4" xml:id="t3"/>
        <note pname="g" oct="4" dur="4" xml:id="t4"/>
      </layer></staff>
      <staff n="2"><layer>
        <note pname="c" oct="3" dur="4" xml:id="b1"/>
        <note pname="c" oct="3" dur="4" xml:id="b2"/>
        <note pname="c" oct="3" dur="4" xml:id="b3"/>
        <note pname="c" oct="3" dur="4" xml:id="b4"/>
      </layer></staff>
    </measure></section>
  </score></mdiv></body></music></mei>"#;

/// Collect the unique MIDI channels used by Midi events on a given track.
fn channels_on_track(smf: &Smf, track_idx: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for ev in &smf.tracks[track_idx] {
        if let TrackEventKind::Midi { channel, .. } = &ev.kind {
            let ch = channel.as_int();
            if !out.contains(&ch) {
                out.push(ch);
            }
        }
    }
    out
}

/// Return the first ProgramChange program on a track, if any.
fn first_program_on_track(smf: &Smf, track_idx: usize) -> Option<u8> {
    for ev in &smf.tracks[track_idx] {
        if let TrackEventKind::Midi {
            message: MidiMessage::ProgramChange { program },
            ..
        } = &ev.kind
        {
            return Some(program.as_int());
        }
    }
    None
}

/// Return the first CC#7 value on a track, if any.
fn first_volume_on_track(smf: &Smf, track_idx: usize) -> Option<u8> {
    for ev in &smf.tracks[track_idx] {
        if let TrackEventKind::Midi {
            message: MidiMessage::Controller { controller, value },
            ..
        } = &ev.kind
        {
            if controller.as_int() == 7 {
                return Some(value.as_int());
            }
        }
    }
    None
}

/// Return every NoteOn velocity on a track (vel==0 is the SMF convention
/// for NoteOff). Used to verify mute.
fn note_on_velocities(smf: &Smf, track_idx: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for ev in &smf.tracks[track_idx] {
        if let TrackEventKind::Midi {
            message: MidiMessage::NoteOn { vel, .. },
            ..
        } = &ev.kind
        {
            out.push(vel.as_int());
        }
    }
    out
}

#[test]
fn verovio_default_smf_puts_everything_on_channel_0() {
    // Pin the upstream behavior this module exists to fix.
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI load");
    let bytes = tk.render_to_midi_bytes().expect("midi bytes");
    let smf = Smf::parse(&bytes).expect("smf parse");

    assert_eq!(smf.tracks.len(), 3, "expected meta + 2 staff tracks");
    // Track 0 is the meta track — no Midi events.
    assert!(channels_on_track(&smf, 0).is_empty());
    // Tracks 1 and 2 both use channel 0 by default — this is the upstream gap.
    assert_eq!(channels_on_track(&smf, 1), vec![0]);
    assert_eq!(channels_on_track(&smf, 2), vec![0]);
}

#[test]
fn auto_distribute_channels_assigns_one_per_track() {
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI load");
    let policy = MidiTrackPolicy {
        auto_distribute_channels: true,
        ..MidiTrackPolicy::default()
    };
    let bytes = tk
        .render_to_midi_bytes_with_policy(&policy)
        .expect("policy");
    let smf = Smf::parse(&bytes).expect("smf parse");

    // Track 1 → channel 0, track 2 → channel 1.
    assert_eq!(channels_on_track(&smf, 1), vec![0]);
    assert_eq!(channels_on_track(&smf, 2), vec![1]);
}

#[test]
fn explicit_channel_override_takes_precedence() {
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI load");
    let mut overrides = BTreeMap::new();
    overrides.insert(
        2,
        TrackOverride {
            channel: Some(9), // GM percussion channel
            ..Default::default()
        },
    );
    let policy = MidiTrackPolicy {
        overrides,
        auto_distribute_channels: true, // would put track 2 on ch 1 — overridden
    };
    let bytes = tk
        .render_to_midi_bytes_with_policy(&policy)
        .expect("policy");
    let smf = Smf::parse(&bytes).expect("smf parse");

    // Track 1 still gets auto-distributed to ch 0; track 2 honors the explicit override.
    assert_eq!(channels_on_track(&smf, 1), vec![0]);
    assert_eq!(channels_on_track(&smf, 2), vec![9]);
}

#[test]
fn program_override_inserts_program_change() {
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI load");
    let mut overrides = BTreeMap::new();
    overrides.insert(
        1,
        TrackOverride {
            program: Some(0),
            ..Default::default()
        },
    ); // Piano
    overrides.insert(
        2,
        TrackOverride {
            program: Some(42),
            ..Default::default()
        },
    ); // Cello
    let policy = MidiTrackPolicy {
        overrides,
        auto_distribute_channels: true,
    };
    let bytes = tk
        .render_to_midi_bytes_with_policy(&policy)
        .expect("policy");
    let smf = Smf::parse(&bytes).expect("smf parse");

    assert_eq!(first_program_on_track(&smf, 1), Some(0));
    assert_eq!(first_program_on_track(&smf, 2), Some(42));
}

#[test]
fn volume_override_inserts_cc7() {
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI load");
    let mut overrides = BTreeMap::new();
    overrides.insert(
        1,
        TrackOverride {
            volume: Some(100),
            ..Default::default()
        },
    );
    overrides.insert(
        2,
        TrackOverride {
            volume: Some(64),
            ..Default::default()
        },
    );
    let policy = MidiTrackPolicy {
        overrides,
        ..MidiTrackPolicy::default()
    };
    let bytes = tk
        .render_to_midi_bytes_with_policy(&policy)
        .expect("policy");
    let smf = Smf::parse(&bytes).expect("smf parse");

    assert_eq!(first_volume_on_track(&smf, 1), Some(100));
    assert_eq!(first_volume_on_track(&smf, 2), Some(64));
}

#[test]
fn mute_zeros_every_note_on_velocity_on_that_track() {
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI load");
    let mut overrides = BTreeMap::new();
    overrides.insert(
        2,
        TrackOverride {
            mute: true,
            ..Default::default()
        },
    );
    let policy = MidiTrackPolicy {
        overrides,
        ..MidiTrackPolicy::default()
    };
    let bytes = tk
        .render_to_midi_bytes_with_policy(&policy)
        .expect("policy");
    let smf = Smf::parse(&bytes).expect("smf parse");

    // Track 1 keeps its original velocities (a mix of note-on velocities
    // and the velocity-0 note-offs Verovio uses).
    let t1_vels = note_on_velocities(&smf, 1);
    assert!(
        t1_vels.iter().any(|&v| v > 0),
        "track 1 should still have audible notes"
    );

    // Track 2 has every velocity zeroed.
    let t2_vels = note_on_velocities(&smf, 2);
    assert!(!t2_vels.is_empty(), "track 2 should have NoteOn events");
    assert!(
        t2_vels.iter().all(|&v| v == 0),
        "track 2 mute should zero all velocities, got {t2_vels:?}"
    );
}

#[test]
fn apply_track_policy_is_pure_and_idempotent() {
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI load");
    let bytes = tk.render_to_midi_bytes().expect("midi bytes");

    let policy = MidiTrackPolicy {
        auto_distribute_channels: true,
        ..MidiTrackPolicy::default()
    };
    let once = verovio::midi::apply_track_policy(&bytes, &policy).expect("apply 1");
    let twice = verovio::midi::apply_track_policy(&once, &policy).expect("apply 2");

    // Channel reassignment is idempotent (channels are already what the
    // policy wants on the second pass).
    let s1 = Smf::parse(&once).unwrap();
    let s2 = Smf::parse(&twice).unwrap();
    assert_eq!(channels_on_track(&s1, 1), channels_on_track(&s2, 1));
    assert_eq!(channels_on_track(&s1, 2), channels_on_track(&s2, 2));
}

#[test]
fn empty_policy_passes_smf_through_unchanged_semantically() {
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI load");
    let original = tk.render_to_midi_bytes().expect("midi bytes");
    let passthrough = tk
        .render_to_midi_bytes_with_policy(&MidiTrackPolicy::default())
        .expect("policy");

    // Bytes may differ (midly's writer doesn't necessarily produce
    // byte-identical output), but the semantic content should match —
    // same channels, same notes, same track count.
    let so = Smf::parse(&original).unwrap();
    let sp = Smf::parse(&passthrough).unwrap();
    assert_eq!(so.tracks.len(), sp.tracks.len());
    for i in 0..so.tracks.len() {
        assert_eq!(channels_on_track(&so, i), channels_on_track(&sp, i));
        assert_eq!(note_on_velocities(&so, i), note_on_velocities(&sp, i));
    }

    let _meta = MetaMessage::EndOfTrack; // touch the import so unused warnings don't fire
}
