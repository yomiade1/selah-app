//! Golden check for the WGSL test patterns (M1.5): the GPU output must equal the CPU
//! reference exactly on the software adapter (lavapipe, WARP; Metal on macOS).
use selah_proto::v1::{Resolution, Scene, TestPattern};
use selah_render::Compositor;
use selah_render::cpu::CpuCompositor;
use selah_render::gpu::{AdapterChoice, GpuCompositor};

fn scene(pattern: TestPattern, cleared: bool) -> Scene {
    Scene {
        revision: 1,
        test_pattern: pattern as i32,
        text: String::new(),
        cleared,
        resolution: Some(Resolution {
            width: 1920,
            height: 1080,
        }),
        ..Default::default()
    }
}

#[test]
fn gpu_test_patterns_match_the_cpu_reference_exactly() {
    let mut gpu = GpuCompositor::new(AdapterChoice::Software)
        .expect("a software GPU adapter (lavapipe, WARP or Metal) must be available");
    eprintln!("adapter: {}", gpu.name());
    let mut cpu = CpuCompositor::default();
    for (width, height) in [(1920, 1080), (1366, 768), (17, 9)] {
        for (pattern, cleared) in [
            (TestPattern::SmpteBars, false),
            (TestPattern::Grid, false),
            (TestPattern::Gradient, false),
            (TestPattern::SmpteBars, true),
            (TestPattern::Unspecified, false),
        ] {
            let scene = scene(pattern, cleared);
            let expected = cpu.render(&scene, width, height).unwrap();
            let actual = gpu.render(&scene, width, height).unwrap();
            if actual != expected {
                let first = actual
                    .pixels
                    .chunks_exact(4)
                    .zip(expected.pixels.chunks_exact(4))
                    .position(|(a, b)| a != b)
                    .unwrap();
                panic!(
                    "{pattern:?} cleared={cleared} at {width}x{height} differs first at pixel \
                     ({}, {}): gpu {:?} cpu {:?}",
                    first % width as usize,
                    first / width as usize,
                    &actual.pixels[first * 4..first * 4 + 4],
                    &expected.pixels[first * 4..first * 4 + 4],
                );
            }
        }
    }
}

#[test]
fn gpu_records_when_the_frame_finished_rendering() {
    let mut gpu = GpuCompositor::new(AdapterChoice::Software).expect("software adapter");
    let before = std::time::Instant::now();
    gpu.render(&scene(TestPattern::Grid, false), 640, 360)
        .unwrap();
    let rendered = gpu.last_rendered().expect("timing recorded");
    assert!(rendered >= before);
}
