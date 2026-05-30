//! Tests for `verovio::audio` (gated on the `audio` Cargo feature).

#![cfg(feature = "audio")]

use verovio::audio::{pcm_to_wav, render_pcm, Pcm};
use verovio::Error;

#[test]
fn pcm_to_wav_writes_valid_riff_header() {
    let pcm = Pcm {
        sample_rate: 44_100,
        left: vec![0.0; 1000],
        right: vec![0.0; 1000],
    };
    let wav = pcm_to_wav(&pcm);
    assert!(wav.starts_with(b"RIFF"));
    assert_eq!(&wav[8..12], b"WAVE");
    assert_eq!(&wav[12..16], b"fmt ");
    assert_eq!(&wav[36..40], b"data");

    // Data length = 1000 samples * 2 channels * 2 bytes/sample = 4000.
    let data_len = u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]);
    assert_eq!(data_len, 4000);
}

#[test]
fn pcm_to_wav_clips_out_of_range_samples() {
    let pcm = Pcm {
        sample_rate: 8000,
        left: vec![2.0, -2.0, 0.5],
        right: vec![0.0, 0.0, 0.0],
    };
    let wav = pcm_to_wav(&pcm);
    // First sample (left) should be clamped to i16::MAX.
    let sample0 = i16::from_le_bytes([wav[44], wav[45]]);
    assert_eq!(sample0, i16::MAX);
    // Second left sample clamped to ~ -i16::MAX (we use *i16::MAX, not MIN).
    let sample1 = i16::from_le_bytes([wav[48], wav[49]]);
    assert!(sample1 < -32000);
}

#[test]
fn pcm_duration_secs_matches_sample_count() {
    let pcm = Pcm {
        sample_rate: 44_100,
        left: vec![0.0; 44_100],
        right: vec![0.0; 44_100],
    };
    assert!((pcm.duration_secs() - 1.0).abs() < 1e-9);
}

#[test]
fn pcm_interleaved_alternates_l_r() {
    let pcm = Pcm {
        sample_rate: 8000,
        left: vec![0.1, 0.2, 0.3],
        right: vec![0.4, 0.5, 0.6],
    };
    let inter = pcm.interleaved();
    assert_eq!(inter, vec![0.1, 0.4, 0.2, 0.5, 0.3, 0.6]);
}

#[test]
fn render_pcm_rejects_garbage_soundfont() {
    let res = render_pcm(b"some midi bytes", b"not a soundfont", 44_100);
    assert!(matches!(res, Err(Error::Audio(_))), "got {res:?}");
}
