use std::path::Path;
use std::sync::atomic::AtomicBool;
use stemplayer_decode::{CompressedTrackDecoder, decode_aac_segment, inspect_audio_file};

#[test]
fn segment_edits_match_local_presentation_before_producer_preroll() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/stereo-aac.m4a");
    let info = inspect_audio_file(&path).unwrap();
    assert_eq!(info.declared_frames, Some(22_050));
    let timeline = info.timeline.as_ref().unwrap();
    assert_eq!(timeline.segments[0].media_start_frame.unwrap().get(), 1024);
    let cancelled = AtomicBool::new(false);
    let mut local = CompressedTrackDecoder::open_audio(&path, info.track_id).unwrap();
    let windows = (0..22_050_u64)
        .step_by(4096)
        .map(|start| {
            local
                .decode_window(start, (22_050 - start).min(4096) as u32, &cancelled)
                .unwrap()
        })
        .collect::<Vec<_>>();
    let left = windows
        .iter()
        .flat_map(|window| window.left.iter().copied())
        .collect::<Vec<_>>();
    let right = windows
        .iter()
        .flat_map(|window| window.right.iter().copied())
        .collect::<Vec<_>>();
    for trim in [0, 1024] {
        let segment = decode_aac_segment(
            include_bytes!("fixtures/stereo-aac.m4a").to_vec(),
            44_100,
            trim,
            22_050 - trim,
            &cancelled,
        )
        .unwrap();
        assert_eq!(segment.left.len(), 22_050 - trim);
        assert_eq!(&segment.left[..4096], &left[trim..trim + 4096]);
        assert_eq!(&segment.right[..4096], &right[trim..trim + 4096]);
        assert_eq!(
            &segment.left[segment.left.len() - 4096..],
            &left[left.len() - 4096..]
        );
        assert_eq!(
            &segment.right[segment.right.len() - 4096..],
            &right[right.len() - 4096..]
        );
        assert_eq!(segment.left, left[trim..]);
        assert_eq!(segment.right, right[trim..]);
    }
}
