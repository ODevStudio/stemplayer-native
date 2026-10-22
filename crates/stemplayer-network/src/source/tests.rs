use super::*;
use crate::client::HttpResponse;
use crate::manifest::{HlsRole, HlsRoute};
use crate::playlist::SegmentDescriptor;
use std::sync::mpsc::sync_channel;
use url::Url;

const GROUP_FRAMES: usize = 3_072;

struct NoRequests;

impl HttpTransport for NoRequests {
    fn get(&self, _: &Url, _: usize) -> Result<HttpResponse, PlayerError> {
        panic!("cached playback must not fetch segments")
    }
}

fn source(has_master: bool, group_count: usize) -> HlsPlaybackSource {
    let role_count = if has_master { 5 } else { 4 };
    let routes = (0..role_count)
        .map(|index| HlsRoute {
            role: if index < 4 {
                HlsRole::STEMS[index]
            } else {
                HlsRole::Master
            },
            playlist_url: Url::parse(&format!("https://example.test/{index}/track.m3u8")).unwrap(),
            waveform_url: Url::parse(&format!("https://example.test/{index}/waveform.json"))
                .unwrap(),
        })
        .collect::<Vec<_>>();
    let playlists = routes
        .iter()
        .map(|route| MediaPlaylist {
            media_sequence: 0,
            segments: (0..group_count)
                .map(|index| SegmentDescriptor {
                    url: route.playlist_url.join(&format!("{index}.m4a")).unwrap(),
                    duration_us: 64_000,
                    start_frame: (index * GROUP_FRAMES) as u64,
                    end_frame_exclusive: ((index + 1) * GROUP_FRAMES) as u64,
                    trim_start_frames: if index == 0 { 0 } else { 1_024 },
                })
                .collect(),
            duration_us: group_count as u64 * 64_000,
        })
        .collect();
    HlsPlaybackSource::new(
        Arc::new(NoRequests),
        HlsManifest {
            redacted_url: "https://example.test/1 path component(s)".into(),
            title: "Synthetic cached playback".into(),
            duration_us: group_count as u64 * 64_000,
            sample_rate: 48_000,
            bitrate_bps: 96_000,
            segment_count: group_count,
            preroll_frames: 1_024,
            routes,
        },
        playlists,
        std::array::from_fn(|_| HlsWaveform {
            samples_per_peak: GROUP_FRAMES as u64,
            minimum: vec![-0.5; group_count],
            maximum: vec![0.5; group_count],
        }),
        Duration::ZERO,
    )
    .unwrap()
}

fn decoded(role_count: usize, capacity: usize) -> Vec<DecodedSegment> {
    let channel = |value| {
        let mut samples = vec![value; capacity];
        samples.truncate(GROUP_FRAMES);
        samples
    };
    (1..=role_count)
        .map(|role| DecodedSegment {
            left: channel(role as f32 / 8.0),
            right: channel(-(role as f32) / 8.0),
            packets: 4,
        })
        .collect()
}

#[test]
fn cache_charges_retained_pcm_capacity_and_evicts_least_recent_groups() {
    let source = source(false, 601);
    for index in 0..600 {
        let state = source
            .cache_group(index, decoded(4, 4_096), Instant::now())
            .unwrap();
        let retained = state
            .groups
            .iter()
            .flat_map(|group| &group.decoded)
            .map(|audio| (audio.left.capacity() + audio.right.capacity()) * size_of::<f32>())
            .sum::<usize>();
        assert_eq!(state.diagnostics.cache_bytes, retained as u64);
        assert!(retained <= CACHE_CAPACITY_BYTES);
        assert!(state.diagnostics.peak_cache_bytes <= CACHE_CAPACITY_BYTES as u64);
    }
    let oldest = source.state.lock().unwrap().groups.back().unwrap().index;
    assert!(oldest > 0);
    drop(
        source
            .ensure_group(oldest, &AtomicBool::new(false))
            .unwrap(),
    );
    let state = source
        .cache_group(600, decoded(4, 4_096), Instant::now())
        .unwrap();
    assert!(state.groups.iter().any(|group| group.index == oldest));
    assert!(!state.groups.iter().any(|group| group.index == oldest + 1));
    drop(state);
    source.clear_cache();
    assert_eq!(source.diagnostics().cache_bytes, 0);
    assert_eq!(source.diagnostics().current_group, None);
    assert!(source.state.lock().unwrap().groups.is_empty());
    let state = source
        .cache_group(0, decoded(4, 4_096), Instant::now())
        .unwrap();
    assert_eq!(state.diagnostics.cache_bytes, state.groups[0].bytes as u64);
}

#[test]
fn group_admission_rejects_spare_pcm_capacity_above_the_cache_limit() {
    let source = source(false, 1);
    let capacity = CACHE_CAPACITY_BYTES / (8 * size_of::<f32>()) + 1;
    let error = source
        .cache_group(0, decoded(4, capacity), Instant::now())
        .err()
        .unwrap();
    assert_eq!(error.code, ErrorCode::HlsResponseTooLarge);
    assert_eq!(source.diagnostics().cache_bytes, 0);
    assert!(source.state.lock().unwrap().groups.is_empty());
}

#[test]
fn clearing_the_cache_waits_for_the_acquired_group_to_be_consumed() {
    let source = source(false, 1);
    drop(
        source
            .cache_group(0, decoded(4, 4_096), Instant::now())
            .unwrap(),
    );
    thread::scope(|scope| {
        let state = source.ensure_group(0, &AtomicBool::new(false)).unwrap();
        let source_ref = &source;
        let (started, ready) = sync_channel(0);
        let clearer = scope.spawn(move || {
            started.send(()).unwrap();
            source_ref.clear_cache();
        });
        ready.recv().unwrap();
        assert!(source.state.try_lock().is_err());
        let group = state.groups.front().unwrap();
        assert_eq!(group.index, 0);
        assert_eq!(group.decoded[0].left[GROUP_FRAMES - 1], 0.125);
        assert_eq!(group.decoded[3].right[GROUP_FRAMES - 1], -0.5);
        drop(state);
        clearer.join().unwrap();
    });
    assert_eq!(source.diagnostics().cache_bytes, 0);
    assert!(source.state.lock().unwrap().groups.is_empty());
}

#[test]
fn cached_blocks_preserve_roles_generation_and_exact_group_boundaries() {
    for has_master in [false, true] {
        let source = source(has_master, 2);
        let role_count = if has_master { 5 } else { 4 };
        for index in 0..2 {
            drop(
                source
                    .cache_group(index, decoded(role_count, 4_096), Instant::now())
                    .unwrap(),
            );
        }
        let generation = GenerationId::INITIAL.next().unwrap();
        let block = source
            .decode_block(
                generation,
                GROUP_FRAMES as u64 - 1,
                2,
                &AtomicBool::new(false),
            )
            .unwrap();
        let expected = (0..2)
            .flat_map(|_| {
                (1..=role_count).flat_map(|role| [role as f32 / 8.0, -(role as f32) / 8.0])
            })
            .collect::<Vec<_>>();
        assert_eq!(block.generation, generation);
        assert_eq!(block.start_frame, GROUP_FRAMES as u64 - 1);
        assert_eq!(block.frames, 2);
        assert_eq!(block.channels as usize, role_count * 2);
        assert_eq!(block.samples.as_ref(), expected.as_slice());
    }
}
