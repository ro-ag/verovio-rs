//! Tests for `verovio::midi` SMF post-processing — verifying that the
//! rewritten SMF bytes really do contain the channels / program / volume
//! / mute we asked for.

use std::collections::BTreeMap;

use midly::{MetaMessage, MidiMessage, Smf, TrackEventKind};
use verovio::midi::{summarize, MidiTrackPolicy, TrackOverride};
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
        ..MidiTrackPolicy::default()
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
        ..MidiTrackPolicy::default()
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

// -- TrackName / InstrumentName / sustain -----------------------------------

#[test]
fn name_override_inserts_track_name_meta() {
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI load");
    let mut overrides = BTreeMap::new();
    overrides.insert(
        1,
        TrackOverride {
            name: Some("Treble".to_string()),
            ..Default::default()
        },
    );
    overrides.insert(
        2,
        TrackOverride {
            name: Some("Bass".to_string()),
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

    // Walk each track's meta events and find a TrackName.
    let find_name = |track_idx: usize| -> Option<String> {
        for ev in &smf.tracks[track_idx] {
            if let TrackEventKind::Meta(MetaMessage::TrackName(bytes)) = &ev.kind {
                return Some(String::from_utf8_lossy(bytes).into_owned());
            }
        }
        None
    };
    assert_eq!(find_name(1).as_deref(), Some("Treble"));
    assert_eq!(find_name(2).as_deref(), Some("Bass"));
}

#[test]
fn instrument_name_override_inserts_instrument_name_meta() {
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI load");
    let mut overrides = BTreeMap::new();
    overrides.insert(
        1,
        TrackOverride {
            instrument_name: Some("Grand Piano".to_string()),
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

    let mut found = None;
    for ev in &smf.tracks[1] {
        if let TrackEventKind::Meta(MetaMessage::InstrumentName(bytes)) = &ev.kind {
            found = Some(String::from_utf8_lossy(bytes).into_owned());
            break;
        }
    }
    assert_eq!(found.as_deref(), Some("Grand Piano"));
}

#[test]
fn sustain_override_inserts_cc64() {
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI load");
    let mut overrides = BTreeMap::new();
    overrides.insert(
        1,
        TrackOverride {
            sustain: Some(true),
            ..Default::default()
        },
    );
    overrides.insert(
        2,
        TrackOverride {
            sustain: Some(false),
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

    let find_cc64 = |track_idx: usize| -> Option<u8> {
        for ev in &smf.tracks[track_idx] {
            if let TrackEventKind::Midi {
                message: MidiMessage::Controller { controller, value },
                ..
            } = &ev.kind
            {
                if controller.as_int() == 64 {
                    return Some(value.as_int());
                }
            }
        }
        None
    };
    assert_eq!(
        find_cc64(1),
        Some(127),
        "sustain Some(true) → pedal down (127)"
    );
    assert_eq!(find_cc64(2), Some(0), "sustain Some(false) → pedal up (0)");
}

// -- transpose / expression --------------------------------------------------

#[test]
fn transpose_shifts_every_note_on_pitch() {
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI load");

    // Default: treble notes are G4 = 67. Transposed up an octave: 79.
    let mut overrides = BTreeMap::new();
    overrides.insert(
        1,
        TrackOverride {
            transpose: Some(12),
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

    let mut treble_keys = Vec::new();
    for ev in &smf.tracks[1] {
        if let TrackEventKind::Midi {
            message: MidiMessage::NoteOn { key, vel },
            ..
        } = &ev.kind
        {
            if vel.as_int() > 0 {
                treble_keys.push(key.as_int());
            }
        }
    }
    assert!(
        treble_keys.iter().all(|&k| k == 79),
        "all treble notes should be 67+12=79 after +12 transpose, got {treble_keys:?}"
    );
}

#[test]
fn transpose_clamps_to_midi_range() {
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI load");
    // +200 semitones would overflow past 127; expect clamp.
    let mut overrides = BTreeMap::new();
    overrides.insert(
        1,
        TrackOverride {
            transpose: Some(127),
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

    for ev in &smf.tracks[1] {
        if let TrackEventKind::Midi {
            message: MidiMessage::NoteOn { key, .. },
            ..
        } = &ev.kind
        {
            assert!(key.as_int() <= 127, "key exceeded midi range");
        }
    }
}

#[test]
fn expression_override_inserts_cc11() {
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI load");
    let mut overrides = BTreeMap::new();
    overrides.insert(
        1,
        TrackOverride {
            expression: Some(96),
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

    let mut found = None;
    for ev in &smf.tracks[1] {
        if let TrackEventKind::Midi {
            message: MidiMessage::Controller { controller, value },
            ..
        } = &ev.kind
        {
            if controller.as_int() == 11 {
                found = Some(value.as_int());
                break;
            }
        }
    }
    assert_eq!(found, Some(96));
}

#[test]
fn modulation_reverb_chorus_overrides_insert_their_ccs() {
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI load");
    let mut overrides = BTreeMap::new();
    overrides.insert(
        1,
        TrackOverride {
            modulation: Some(20),
            reverb: Some(40),
            chorus: Some(50),
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

    let mut found_cc = std::collections::HashMap::new();
    for ev in &smf.tracks[1] {
        if let TrackEventKind::Midi {
            message: MidiMessage::Controller { controller, value },
            ..
        } = &ev.kind
        {
            found_cc
                .entry(controller.as_int())
                .or_insert(value.as_int());
        }
    }
    assert_eq!(found_cc.get(&1), Some(&20), "CC#1 modulation");
    assert_eq!(found_cc.get(&91), Some(&40), "CC#91 reverb");
    assert_eq!(found_cc.get(&93), Some(&50), "CC#93 chorus");
}

#[test]
fn measure_markers_inserts_one_marker_per_measure() {
    // PAE with multiple measures, each marker should land on the meta track.
    let mut tk = Toolkit::from_data(MANY_MEASURE_PAE).expect("PAE load");
    let measures = tk.measures().expect("measures");
    let policy = MidiTrackPolicy {
        measure_markers: Some(measures.clone()),
        ..MidiTrackPolicy::default()
    };
    let bytes = tk
        .render_to_midi_bytes_with_policy(&policy)
        .expect("policy");
    let smf = Smf::parse(&bytes).expect("smf parse");

    let mut markers: Vec<String> = Vec::new();
    for ev in &smf.tracks[0] {
        if let TrackEventKind::Meta(MetaMessage::Marker(b)) = &ev.kind {
            markers.push(String::from_utf8_lossy(b).into_owned());
        }
    }
    assert_eq!(
        markers.len(),
        measures.len(),
        "expected one marker per measure, got markers={markers:?}, measures={}",
        measures.len()
    );
    // Markers should be in the same order as the measures (ascending tstamp).
    for (m, marker_text) in measures.iter().zip(markers.iter()) {
        assert_eq!(*marker_text, m.id);
    }
}

const MANY_MEASURE_PAE: &str =
    "@start:s\n@clef:G-2\n@keysig:xF\n@key:\n@timesig:4/4\n@data:'4G/4A/4B/4c\n@end:s\n";

// -- SMF-level meta overrides -----------------------------------------------

#[test]
fn time_signature_override_inserts_meta() {
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI load");
    let policy = MidiTrackPolicy {
        time_signature: Some((6, 8)),
        ..MidiTrackPolicy::default()
    };
    let bytes = tk
        .render_to_midi_bytes_with_policy(&policy)
        .expect("policy");
    let smf = Smf::parse(&bytes).expect("smf parse");

    let mut found = None;
    for ev in &smf.tracks[0] {
        if let TrackEventKind::Meta(MetaMessage::TimeSignature(num, denom_power, _, _)) = &ev.kind {
            found = Some((*num, 1u8 << *denom_power));
            break;
        }
    }
    assert_eq!(found, Some((6, 8)));
}

#[test]
fn key_signature_override_inserts_meta() {
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI load");
    let policy = MidiTrackPolicy {
        key_signature: Some(2), // D major (or B minor with key_signature_minor=true)
        key_signature_minor: false,
        ..MidiTrackPolicy::default()
    };
    let bytes = tk
        .render_to_midi_bytes_with_policy(&policy)
        .expect("policy");
    let smf = Smf::parse(&bytes).expect("smf parse");

    let mut found = None;
    for ev in &smf.tracks[0] {
        if let TrackEventKind::Meta(MetaMessage::KeySignature(sf, minor)) = &ev.kind {
            found = Some((*sf, *minor));
            break;
        }
    }
    assert_eq!(found, Some((2, false)));
}

#[test]
fn tempo_override_replaces_existing_tempo_events() {
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI load");
    let custom_tempo = verovio::TempoMap::new(vec![verovio::TempoChange {
        at_qstamp: 0.0,
        bpm: 60.0,
    }]);
    let policy = MidiTrackPolicy {
        tempo_override: Some(custom_tempo),
        ..MidiTrackPolicy::default()
    };
    let bytes = tk
        .render_to_midi_bytes_with_policy(&policy)
        .expect("policy");
    let smf = Smf::parse(&bytes).expect("smf parse");

    // Default Verovio tempo is 120 BPM → 500_000 µs/qtr.
    // Our override is 60 BPM → 1_000_000 µs/qtr.
    let mut tempos_us = Vec::new();
    for ev in &smf.tracks[0] {
        if let TrackEventKind::Meta(MetaMessage::Tempo(t)) = &ev.kind {
            tempos_us.push(t.as_int());
        }
    }
    assert_eq!(
        tempos_us,
        vec![1_000_000],
        "expected exactly one Tempo at 60 BPM"
    );
}

// -- pan ---------------------------------------------------------------------

#[test]
fn pan_override_inserts_cc10() {
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI load");
    let mut overrides = BTreeMap::new();
    overrides.insert(
        1,
        TrackOverride {
            pan: Some(32),
            ..Default::default()
        },
    ); // left-of-center
    overrides.insert(
        2,
        TrackOverride {
            pan: Some(96),
            ..Default::default()
        },
    ); // right-of-center
    let policy = MidiTrackPolicy {
        overrides,
        ..MidiTrackPolicy::default()
    };
    let bytes = tk
        .render_to_midi_bytes_with_policy(&policy)
        .expect("policy");

    let infos = summarize(&bytes).expect("summarize");
    assert_eq!(infos[1].pan, Some(32));
    assert_eq!(infos[2].pan, Some(96));
}

// -- summarize ---------------------------------------------------------------

#[test]
fn summarize_reports_meta_track_and_two_staff_tracks() {
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI load");
    let bytes = tk.render_to_midi_bytes().expect("midi bytes");
    let infos = summarize(&bytes).expect("summarize");

    assert_eq!(infos.len(), 3, "expected meta + 2 staff tracks");

    // Track 0 is the meta track — no audible notes, no channels.
    assert_eq!(infos[0].track_index, 0);
    assert_eq!(infos[0].audible_note_count, 0);
    assert!(infos[0].channels.is_empty());

    // Tracks 1 + 2 are staff tracks — each has 4 audible note onsets
    // (one quarter note per beat in the one-measure fixture) and
    // everything on channel 0 by Verovio default.
    assert_eq!(infos[1].audible_note_count, 4);
    assert_eq!(infos[1].channels, vec![0]);
    assert_eq!(infos[2].audible_note_count, 4);
    assert_eq!(infos[2].channels, vec![0]);
}

#[test]
fn summarize_reflects_policy_after_application() {
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI load");
    let mut overrides = BTreeMap::new();
    overrides.insert(
        1,
        TrackOverride {
            program: Some(0),
            volume: Some(110),
            pan: Some(32),
            ..Default::default()
        },
    );
    overrides.insert(
        2,
        TrackOverride {
            program: Some(42),
            volume: Some(85),
            pan: Some(96),
            ..Default::default()
        },
    );
    let policy = MidiTrackPolicy {
        overrides,
        auto_distribute_channels: true,
        ..MidiTrackPolicy::default()
    };
    let bytes = tk
        .render_to_midi_bytes_with_policy(&policy)
        .expect("policy");

    let infos = summarize(&bytes).expect("summarize");
    assert_eq!(infos[1].channels, vec![0]);
    assert_eq!(infos[1].program, Some(0));
    assert_eq!(infos[1].volume, Some(110));
    assert_eq!(infos[1].pan, Some(32));
    assert_eq!(infos[2].channels, vec![1]);
    assert_eq!(infos[2].program, Some(42));
    assert_eq!(infos[2].volume, Some(85));
    assert_eq!(infos[2].pan, Some(96));
}

#[test]
fn summarize_invalid_bytes_returns_none() {
    let garbage = b"\x00\x00\x00\x00not an smf";
    assert!(summarize(garbage).is_none());
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

// -- bank_select / midi_port -------------------------------------------------

#[test]
fn bank_select_inserts_cc0_and_cc32_before_program_change() {
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI load");
    let mut overrides = BTreeMap::new();
    overrides.insert(
        1,
        TrackOverride {
            bank_select: Some((121, 3)),
            program: Some(48),
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

    // Walk the track collecting controller events and program changes in
    // order so we can assert the relative ordering: CC#0 then CC#32 then
    // ProgramChange.
    let mut sequence: Vec<(u8, u8)> = Vec::new(); // (kind, value): 0 = CC#0, 1 = CC#32, 2 = ProgramChange
    for ev in &smf.tracks[1] {
        match &ev.kind {
            TrackEventKind::Midi {
                message: MidiMessage::Controller { controller, value },
                ..
            } => match controller.as_int() {
                0 => sequence.push((0, value.as_int())),
                32 => sequence.push((1, value.as_int())),
                _ => {}
            },
            TrackEventKind::Midi {
                message: MidiMessage::ProgramChange { program },
                ..
            } => sequence.push((2, program.as_int())),
            _ => {}
        }
    }
    assert!(
        sequence.starts_with(&[(0, 121), (1, 3), (2, 48)]),
        "expected CC#0=121, CC#32=3, Program=48 in that order, got {sequence:?}"
    );
}

#[test]
fn midi_port_override_inserts_meta() {
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI load");
    let mut overrides = BTreeMap::new();
    overrides.insert(
        2,
        TrackOverride {
            midi_port: Some(2),
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

    let mut found_port = None;
    for ev in &smf.tracks[2] {
        if let TrackEventKind::Meta(MetaMessage::MidiPort(p)) = &ev.kind {
            found_port = Some(p.as_int());
            break;
        }
    }
    assert_eq!(found_port, Some(2));
}

// -- lyrics / cue_points -----------------------------------------------------

#[test]
fn lyrics_inserted_at_quarter_stamps_on_meta_track() {
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI load");
    let policy = MidiTrackPolicy {
        lyrics: Some(vec![
            (0.0, "do".to_string()),
            (1.0, "re".to_string()),
            (2.0, "mi".to_string()),
        ]),
        ..MidiTrackPolicy::default()
    };
    let bytes = tk
        .render_to_midi_bytes_with_policy(&policy)
        .expect("policy");
    let smf = Smf::parse(&bytes).expect("smf parse");

    let mut lyrics: Vec<String> = Vec::new();
    for ev in &smf.tracks[0] {
        if let TrackEventKind::Meta(MetaMessage::Lyric(b)) = &ev.kind {
            lyrics.push(String::from_utf8_lossy(b).into_owned());
        }
    }
    assert_eq!(lyrics, vec!["do", "re", "mi"]);
}

#[test]
fn cue_points_inserted_at_quarter_stamps_on_meta_track() {
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI load");
    let policy = MidiTrackPolicy {
        cue_points: Some(vec![(0.0, "intro".to_string()), (2.0, "verse".to_string())]),
        ..MidiTrackPolicy::default()
    };
    let bytes = tk
        .render_to_midi_bytes_with_policy(&policy)
        .expect("policy");
    let smf = Smf::parse(&bytes).expect("smf parse");

    let mut cues: Vec<String> = Vec::new();
    for ev in &smf.tracks[0] {
        if let TrackEventKind::Meta(MetaMessage::CuePoint(b)) = &ev.kind {
            cues.push(String::from_utf8_lossy(b).into_owned());
        }
    }
    assert_eq!(cues, vec!["intro", "verse"]);
}

// -- panic SMF ---------------------------------------------------------------

#[test]
fn panic_smf_emits_all_sound_off_and_all_notes_off_on_all_16_channels() {
    let bytes = verovio::midi::build_panic_smf();
    let smf = Smf::parse(&bytes).expect("panic smf parses");

    assert_eq!(smf.tracks.len(), 1, "panic SMF is single-track");

    let mut cc120_channels: Vec<u8> = Vec::new();
    let mut cc123_channels: Vec<u8> = Vec::new();
    for ev in &smf.tracks[0] {
        if let TrackEventKind::Midi {
            channel,
            message: MidiMessage::Controller { controller, value },
        } = &ev.kind
        {
            assert_eq!(value.as_int(), 0, "panic SMF only sends value=0");
            match controller.as_int() {
                120 => cc120_channels.push(channel.as_int()),
                123 => cc123_channels.push(channel.as_int()),
                _ => panic!(
                    "unexpected controller in panic SMF: {}",
                    controller.as_int()
                ),
            }
        }
    }
    cc120_channels.sort();
    cc123_channels.sort();
    let expected: Vec<u8> = (0..16).collect();
    assert_eq!(cc120_channels, expected, "All Sound Off on all 16 channels");
    assert_eq!(cc123_channels, expected, "All Notes Off on all 16 channels");
}

// -- iter_smf_events ---------------------------------------------------------

#[test]
fn iter_smf_events_yields_chronologically_sorted_events() {
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI load");
    let bytes = tk.render_to_midi_bytes().expect("midi");
    let events = verovio::midi::iter_smf_events(&bytes).expect("iter");

    assert!(!events.is_empty(), "expected at least one event");

    for w in events.windows(2) {
        assert!(
            w[0].at_ms <= w[1].at_ms,
            "events not sorted by at_ms: {} > {}",
            w[0].at_ms,
            w[1].at_ms
        );
    }

    // First non-meta event should be a NoteOn at ms 0.
    let first_note = events
        .iter()
        .find(|e| matches!(e.message, verovio::midi::TimedMessage::NoteOn { .. }))
        .expect("expected at least one NoteOn");
    assert!(first_note.at_ms < 1.0, "first NoteOn should land near t=0");
}

#[test]
fn iter_smf_events_respects_tempo_override() {
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI load");
    let custom_tempo = verovio::TempoMap::new(vec![verovio::TempoChange {
        at_qstamp: 0.0,
        bpm: 60.0,
    }]);
    let policy = MidiTrackPolicy {
        tempo_override: Some(custom_tempo),
        ..MidiTrackPolicy::default()
    };
    let bytes = tk
        .render_to_midi_bytes_with_policy(&policy)
        .expect("policy");
    let events = verovio::midi::iter_smf_events(&bytes).expect("iter");

    // At 60 BPM, a quarter-note onset lands at 1000 ms.
    let note_ons: Vec<f64> = events
        .iter()
        .filter_map(|e| {
            if let verovio::midi::TimedMessage::NoteOn { vel, .. } = e.message {
                if vel > 0 {
                    return Some(e.at_ms);
                }
            }
            None
        })
        .collect();
    assert!(!note_ons.is_empty(), "expected audible note-ons");
    // Onsets at q=0,1,2,3 at 60 BPM → 0, 1000, 2000, 3000 ms (allowing slack).
    let unique: std::collections::BTreeSet<i64> =
        note_ons.iter().map(|x| x.round() as i64).collect();
    assert!(unique.contains(&0), "no note-on at 0 ms: unique={unique:?}");
    assert!(
        unique.contains(&1000),
        "no note-on at ~1000 ms (60 BPM expected): unique={unique:?}"
    );
}

#[test]
fn iter_smf_events_marks_tempo_meta() {
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI load");
    let bytes = tk.render_to_midi_bytes().expect("midi");
    let events = verovio::midi::iter_smf_events(&bytes).expect("iter");

    let tempo_events: Vec<u32> = events
        .iter()
        .filter_map(|e| {
            if let verovio::midi::TimedMessage::Tempo { usec_per_quarter } = e.message {
                Some(usec_per_quarter)
            } else {
                None
            }
        })
        .collect();
    assert!(
        !tempo_events.is_empty(),
        "expected at least one tempo meta event"
    );
    // Verovio's default is 120 BPM → 500_000 µs/qtr.
    assert_eq!(tempo_events[0], 500_000);
}

#[test]
fn iter_smf_events_returns_none_for_invalid_bytes() {
    assert!(verovio::midi::iter_smf_events(b"not an smf").is_none());
}

#[test]
fn with_solo_silences_non_audible_tracks() {
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI load");
    // Solo only track 1 (treble); track 2 (bass) should be silent.
    let policy = MidiTrackPolicy::with_solo(&[1]);
    let bytes = tk
        .render_to_midi_bytes_with_policy(&policy)
        .expect("policy");
    let smf = Smf::parse(&bytes).expect("smf parse");

    let t1 = note_on_velocities(&smf, 1);
    let t2 = note_on_velocities(&smf, 2);
    assert!(
        t1.iter().any(|&v| v > 0),
        "track 1 should have audible notes"
    );
    assert!(!t2.is_empty(), "track 2 should still have note events");
    assert!(
        t2.iter().all(|&v| v == 0),
        "track 2 should be muted under with_solo([1]), got vels={t2:?}"
    );
}

#[test]
fn with_mute_silences_only_listed_tracks() {
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI load");
    let policy = MidiTrackPolicy::with_mute(&[2]);
    let bytes = tk
        .render_to_midi_bytes_with_policy(&policy)
        .expect("policy");
    let smf = Smf::parse(&bytes).expect("smf parse");

    let t1 = note_on_velocities(&smf, 1);
    let t2 = note_on_velocities(&smf, 2);
    assert!(t1.iter().any(|&v| v > 0), "track 1 should be audible");
    assert!(t2.iter().all(|&v| v == 0), "track 2 should be muted");
}

#[test]
fn with_auto_distribute_channels_chains() {
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI load");
    let policy = MidiTrackPolicy::with_mute(&[]).with_auto_distribute_channels();
    let bytes = tk
        .render_to_midi_bytes_with_policy(&policy)
        .expect("policy");
    let smf = Smf::parse(&bytes).expect("smf parse");
    assert_eq!(channels_on_track(&smf, 1), vec![0]);
    assert_eq!(channels_on_track(&smf, 2), vec![1]);
}

#[test]
fn gm_program_name_returns_expected_text() {
    use verovio::midi::gm;
    assert_eq!(gm::program_name(0), Some("Acoustic Grand Piano"));
    assert_eq!(gm::program_name(40), Some("Violin"));
    assert_eq!(gm::program_name(127), Some("Gunshot"));
    // Out-of-range: u8 can only go up to 255; 128+ is invalid.
    assert_eq!(gm::program_name(200), None);
    assert_eq!(gm::all_programs().len(), 128);
}

#[test]
fn gm_drum_key_name_returns_expected_text() {
    use verovio::midi::gm;
    assert_eq!(gm::drum_key_name(36), Some("Bass Drum 1"));
    assert_eq!(gm::drum_key_name(38), Some("Acoustic Snare"));
    assert_eq!(gm::drum_key_name(42), Some("Closed Hi Hat"));
    assert!(gm::drum_key_name(30).is_none());
    assert!(gm::drum_key_name(127).is_none());
}

#[test]
fn with_programs_assigns_programs_per_track() {
    let mut tk = Toolkit::from_data(TWO_STAFF_MEI).expect("MEI load");
    let policy = MidiTrackPolicy::with_programs(&[(1, 0), (2, 42)]);
    let bytes = tk
        .render_to_midi_bytes_with_policy(&policy)
        .expect("policy");
    let smf = Smf::parse(&bytes).expect("smf parse");
    assert_eq!(first_program_on_track(&smf, 1), Some(0));
    assert_eq!(first_program_on_track(&smf, 2), Some(42));
}

#[test]
fn gm_note_name_round_trips_with_midi_key_from_name() {
    use verovio::midi::gm;
    for key in 0u8..=127 {
        let name = gm::note_name(key).expect("name");
        let back = gm::midi_key_from_name(&name).unwrap_or_else(|| {
            panic!("midi_key_from_name failed on {name}");
        });
        assert_eq!(back, key, "{name} round-trip mismatch");
    }
}

#[test]
fn gm_note_name_middle_c() {
    use verovio::midi::gm;
    assert_eq!(gm::note_name(60).as_deref(), Some("C4"));
    assert_eq!(gm::note_name(69).as_deref(), Some("A4"));
}

#[test]
fn gm_midi_key_from_name_accepts_flats() {
    use verovio::midi::gm;
    // Bb3 == A#3 == midi 58.
    assert_eq!(gm::midi_key_from_name("Bb3"), Some(58));
    assert_eq!(gm::midi_key_from_name("A#3"), Some(58));
}

#[test]
fn gm_midi_key_from_name_rejects_garbage() {
    use verovio::midi::gm;
    assert!(gm::midi_key_from_name("").is_none());
    assert!(gm::midi_key_from_name("X4").is_none());
    assert!(gm::midi_key_from_name("C").is_none());
    assert!(gm::midi_key_from_name("Cabc").is_none());
}
