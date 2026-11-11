//! M1-A7: the committed 1080p H.264 clip decodes and composites under text, with frames in order
//! and none dropped on a 60 fps virtual clock. Software decode and the software GPU adapter, as
//! in cloud sessions and CI; hardware decode is measured with `selah-bench video` (M1.13).
use std::path::PathBuf;

use selah_media::{DecodeMode, VideoDecoder, VideoFrame};
use selah_proto::v1::{Scene, TestPattern};
use selah_render::Compositor;
use selah_render::cpu::CpuCompositor;
use selah_render::gpu::{AdapterChoice, GpuCompositor};

const FPS: f64 = 60.0;
const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;
/// The clip's white frame (tools/generate-media.py --committed).
const FLASH: usize = 30;

fn clip() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/media/committed/h264-1080p60.mp4")
}

fn scene(text: &str) -> Scene {
    Scene {
        revision: 1,
        test_pattern: TestPattern::Unspecified as i32,
        text: text.into(),
        cleared: false,
        resolution: None,
        ..Default::default()
    }
}

fn decode_all() -> Vec<VideoFrame> {
    let mut decoder = VideoDecoder::open(&clip(), DecodeMode::Software).expect("the clip opens");
    assert_eq!((decoder.width(), decoder.height()), (WIDTH, HEIGHT));
    assert_eq!(decoder.decoder_name(), "h264");
    let mut frames = Vec::new();
    while let Some(frame) = decoder.next_frame().expect("decoding succeeds") {
        frames.push(frame);
    }
    frames
}

fn pixel(frame: &selah_render::Frame, x: u32, y: u32) -> [u8; 4] {
    let at = (y as usize * frame.width as usize + x as usize) * 4;
    frame.pixels[at..at + 4].try_into().unwrap()
}

#[test]
fn clip_plays_in_order_with_no_drops_on_a_60_fps_virtual_clock() {
    let frames = decode_all();
    assert_eq!(frames.len(), 60, "every frame decodes");
    for pair in frames.windows(2) {
        assert!(
            pair[1].seconds > pair[0].seconds,
            "presentation times increase"
        );
    }
    // Each tick shows the newest frame whose time has come. With no drops and no repeats, tick n
    // shows frame n.
    let shown: Vec<usize> = (0..frames.len())
        .map(|tick| {
            let now = tick as f64 / FPS + 0.5 / FPS;
            frames
                .iter()
                .rposition(|f| f.seconds <= now)
                .expect("a frame is due")
        })
        .collect();
    assert_eq!(shown, (0..frames.len()).collect::<Vec<_>>());

    let mut gpu = GpuCompositor::new(AdapterChoice::Software).expect("a software GPU adapter");
    let mut previous = None;
    for (tick, frame) in frames.into_iter().enumerate() {
        gpu.set_video(Some(frame.into_nv12())).unwrap();
        let output = gpu.render(&scene("Amazing grace"), WIDTH, HEIGHT).unwrap();
        let corner = pixel(&output, 8, 8);
        if tick == FLASH {
            assert!(
                corner[..3].iter().all(|c| *c >= 250),
                "the flash frame is white: {corner:?}"
            );
        } else {
            assert!(
                !corner[..3].iter().all(|c| *c >= 250),
                "tick {tick} isn't the flash"
            );
        }
        let checksum = output.checksum();
        assert_ne!(Some(checksum), previous, "tick {tick} shows a new frame");
        previous = Some(checksum);
    }
}

#[test]
fn text_composites_over_video_and_gpu_matches_the_cpu_reference() {
    let first = decode_all().swap_remove(0).into_nv12();
    let mut gpu = GpuCompositor::new(AdapterChoice::Software).expect("a software GPU adapter");
    let mut cpu = CpuCompositor::default();
    gpu.set_video(Some(first.clone())).unwrap();
    cpu.set_video(Some(first)).unwrap();

    let gpu_video = gpu.render(&scene(""), WIDTH, HEIGHT).unwrap();
    let cpu_video = cpu.render(&scene(""), WIDTH, HEIGHT).unwrap();
    let worst = gpu_video
        .pixels
        .iter()
        .zip(&cpu_video.pixels)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap();
    assert!(
        worst <= 2,
        "GPU and CPU colour conversion differ by {worst}"
    );

    let with_text = gpu.render(&scene("Amazing grace"), WIDTH, HEIGHT).unwrap();
    assert_ne!(
        with_text.checksum(),
        gpu_video.checksum(),
        "the text is drawn"
    );
    assert_eq!(
        pixel(&with_text, 8, 8),
        pixel(&gpu_video, 8, 8),
        "the video shows around the text"
    );

    // A cleared scene shows no video.
    let mut cleared = scene("");
    cleared.cleared = true;
    let black = gpu.render(&cleared, WIDTH, HEIGHT).unwrap();
    assert!(black.pixels.chunks_exact(4).all(|p| p == [0, 0, 0, 255]));
}
