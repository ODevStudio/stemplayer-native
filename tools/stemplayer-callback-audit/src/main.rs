#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;
use stemplayer_domain::StemRole;
use stemplayer_mixer::MixerRenderer;
use stemplayer_output::{PostMixResampler, StereoFrame, output_ring};

struct CountingAllocator;

static TRACKING: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if TRACKING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if TRACKING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if TRACKING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.realloc(pointer, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn main() -> ExitCode {
    const FRAMES: usize = 256;
    const ITERATIONS: u64 = 100_000;
    let (mut producer, mut consumer) = output_ring(FRAMES).unwrap();
    let mut output = [0.0_f32; FRAMES * 2];
    reset_allocations();
    TRACKING.store(true, Ordering::SeqCst);
    for _ in 0..ITERATIONS {
        for _ in 0..FRAMES {
            assert!(producer.push(StereoFrame::default()));
        }
        consumer.write(&mut output, 2);
    }
    TRACKING.store(false, Ordering::SeqCst);
    let callback_allocations = ALLOCATIONS.load(Ordering::SeqCst);

    let source = [0.25_f32; 10];
    let mut mixed = [StereoFrame::default(); FRAMES];
    let mut mixer = MixerRenderer::new(48_000, true);
    let mut resampler = PostMixResampler::new(48_000, 44_100, FRAMES, u64::MAX / 2).unwrap();
    reset_allocations();
    TRACKING.store(true, Ordering::SeqCst);
    for _ in 0..1_000 {
        mixer.begin_block();
        for frame in &mut mixed {
            let sample = mixer.render(&source);
            *frame = StereoFrame {
                left: sample[0],
                right: sample[1],
            };
        }
        resampler.process(&mixed, false).unwrap();
    }
    TRACKING.store(false, Ordering::SeqCst);
    let render_allocations = ALLOCATIONS.load(Ordering::SeqCst);
    let mut automation_latency_ns = [0_u64; 1_000];
    for (index, latency) in automation_latency_ns.iter_mut().enumerate() {
        let started = Instant::now();
        mixer.set_mute(StemRole::Vocals, index % 2 == 0);
        std::hint::black_box(mixer.render(&source));
        *latency = started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
    }
    automation_latency_ns.sort_unstable();
    let automation_latency_p95_ns = automation_latency_ns[949];
    let passed = callback_allocations == 0
        && render_allocations == 0
        && automation_latency_p95_ns <= 30_000_000;
    println!(
        "{}",
        serde_json::json!({
            "schema": "stemplayer-render-allocation-audit-v2",
            "callback_iterations": ITERATIONS,
            "render_iterations": 1_000,
            "frames_per_callback": FRAMES,
            "callback_allocations": callback_allocations,
            "render_allocations": render_allocations,
            "automation_latency_p95_ns": automation_latency_p95_ns,
            "automation_latency_budget_ns": 30_000_000,
            "status": if passed { "pass" } else { "fail" }
        })
    );
    if passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn reset_allocations() {
    ALLOCATIONS.store(0, Ordering::SeqCst);
}
