//! M3.7 / M3-A5 custom slide elements tests:
//! shapes, text, clocks, timers, images, videos, z-order, alignment, bounds/SEC-04 validation,
//! and CPU/GPU compositor parity without stale overlay cache.
use selah_proto::v1::{
    Color, LayerKind, LayerSpec, MediaLayerSpec, OutputScene, Scene, SlideLayerSpec, layer_spec,
};
use selah_render::Compositor;
use selah_render::cpu::CpuCompositor;
use selah_render::gpu::{AdapterChoice, GpuCompositor};
use selah_render::slide_elements::{
    AlignMode, MAX_ELEMENTS, ShapeKind, SlideElement, SlideElementKind, SlideElementsDocument,
    format_dynamic_tokens,
};
use selah_render::video::{Nv12Frame, YuvMatrix};

static GPU_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn pixel(frame: &selah_render::Frame, x: u32, y: u32) -> [u8; 4] {
    let at = (y as usize * frame.width as usize + x as usize) * 4;
    frame.pixels[at..at + 4].try_into().unwrap()
}

fn assert_frames_close(a: &selah_render::Frame, b: &selah_render::Frame, max_diff: u8) {
    assert_eq!((a.width, a.height), (b.width, b.height));
    for (idx, (pa, pb)) in a
        .pixels
        .chunks_exact(4)
        .zip(b.pixels.chunks_exact(4))
        .enumerate()
    {
        for ch in 0..4 {
            assert!(
                pa[ch].abs_diff(pb[ch]) <= max_diff,
                "pixel {idx} ch {ch} differs: a={pa:?} b={pb:?}"
            );
        }
    }
}

struct TempMediaDir(std::path::PathBuf);

impl TempMediaDir {
    fn new(prefix: &str) -> Self {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "selah-elements-test-{prefix}-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn store_png(&self, frame: &selah_render::Frame) -> String {
        let png = selah_render::png::encode(frame).unwrap();
        let hash = blake3::hash(&png).to_hex().to_string();
        let shard = self.0.join(&hash[..2]);
        std::fs::create_dir_all(&shard).unwrap();
        std::fs::write(shard.join(format!("{hash}.png")), png).unwrap();
        hash
    }
}

impl Drop for TempMediaDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn count_diff_pixels(a: &selah_render::Frame, b: &selah_render::Frame) -> usize {
    a.pixels
        .chunks_exact(4)
        .zip(b.pixels.chunks_exact(4))
        .filter(|(pa, pb)| {
            pa[0].abs_diff(pb[0]) > 4 || pa[1].abs_diff(pb[1]) > 4 || pa[2].abs_diff(pb[2]) > 4
        })
        .count()
}

#[test]
fn slide_elements_render_in_z_order_across_shapes_image_video_text_and_match_cpu_gpu() {
    let _guard = GPU_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let store = TempMediaDir::new("zorder");

    // Solid green 40x40 image element.
    let mut img = selah_render::Frame::new(40, 40);
    for px in img.pixels.chunks_exact_mut(4) {
        px.copy_from_slice(&[0, 220, 40, 255]);
    }
    let img_hash = store.store_png(&img);

    // Solid red NV12 video element.
    let (vw, vh) = (32_u32, 32_u32);
    let y_plane = vec![81_u8; (vw * vh) as usize];
    let mut uv_plane = vec![0_u8; (vw / 2 * 2 * (vh / 2)) as usize];
    for uv in uv_plane.chunks_exact_mut(2) {
        uv[0] = 90;
        uv[1] = 240;
    }
    let video_frame = Nv12Frame {
        width: vw,
        height: vh,
        y: y_plane,
        uv: uv_plane,
        matrix: YuvMatrix::Bt709,
        full_range: false,
    };
    let vid_hash = "c".repeat(64);

    // Deliberately declare elements out of z_index order to test sorting & visibility.
    let doc = SlideElementsDocument {
        elements: vec![
            // Highest z_index: yellow rounded card on top-left corner of overlap
            SlideElement {
                id: "top-shape".into(),
                name: "Top Card".into(),
                kind: SlideElementKind::Shape,
                x: 0.10,
                y: 0.10,
                width: 0.30,
                height: 0.30,
                z_index: 20,
                shape_kind: ShapeKind::RoundedRect,
                fill_color: [245, 200, 20, 255],
                corner_radius: 4.0,
                ..Default::default()
            },
            // Hidden shape should never paint even with highest z_index
            SlideElement {
                id: "hidden".into(),
                name: "Hidden".into(),
                kind: SlideElementKind::Shape,
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
                z_index: 99,
                visible: false,
                fill_color: [255, 0, 255, 255],
                ..Default::default()
            },
            // Middle z_index: green image covering (0.20..0.70, 0.20..0.70)
            SlideElement {
                id: "img-mid".into(),
                name: "Image".into(),
                kind: SlideElementKind::Image,
                x: 0.20,
                y: 0.20,
                width: 0.50,
                height: 0.50,
                z_index: 10,
                content_hash: img_hash,
                media_extension: "png".into(),
                fit: "stretch".into(),
                ..Default::default()
            },
            // Lowest z_index: red video covering left half (0.05..0.55, 0.05..0.55)
            SlideElement {
                id: "vid-low".into(),
                name: "Video".into(),
                kind: SlideElementKind::Video,
                x: 0.05,
                y: 0.05,
                width: 0.50,
                height: 0.50,
                z_index: 1,
                content_hash: vid_hash.clone(),
                media_extension: "mp4".into(),
                fit: "stretch".into(),
                ..Default::default()
            },
            // Ellipse + line + text on bottom band
            SlideElement {
                id: "ellipse".into(),
                name: "Badge".into(),
                kind: SlideElementKind::Shape,
                x: 0.72,
                y: 0.68,
                width: 0.22,
                height: 0.22,
                z_index: 15,
                shape_kind: ShapeKind::Ellipse,
                fill_color: [40, 120, 240, 255],
                ..Default::default()
            },
            SlideElement {
                id: "divider".into(),
                name: "Divider".into(),
                kind: SlideElementKind::Shape,
                x: 0.10,
                y: 0.85,
                width: 0.55,
                height: 0.02,
                z_index: 15,
                shape_kind: ShapeKind::Line,
                stroke_color: Some([255, 255, 255, 255]),
                stroke_width: 4.0,
                ..Default::default()
            },
            SlideElement {
                id: "title".into(),
                name: "Headline".into(),
                kind: SlideElementKind::Text,
                x: 0.10,
                y: 0.72,
                width: 0.55,
                height: 0.12,
                z_index: 25,
                text: "Sunday Gathering".into(),
                font_family: "Noto Sans".into(),
                font_size: 44.0,
                weight: 700,
                color: [255, 255, 255, 255],
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let elements_json = doc.to_json().unwrap();
    // Round-trip strict parser accepts our serialized document.
    SlideElementsDocument::from_json_strict(&elements_json).unwrap();

    let scene = Scene {
        revision: 1,
        outputs: vec![OutputScene {
            output_index: 0,
            layers: vec![
                LayerSpec {
                    kind: LayerKind::Media as i32,
                    enabled: true,
                    content: Some(layer_spec::Content::Media(MediaLayerSpec {
                        solid_color: Some(Color {
                            red: 0.05,
                            green: 0.07,
                            blue: 0.12,
                            alpha: 1.0,
                        }),
                        ..Default::default()
                    })),
                    ..Default::default()
                },
                LayerSpec {
                    kind: LayerKind::Slide as i32,
                    enabled: true,
                    content: Some(layer_spec::Content::Slide(SlideLayerSpec {
                        elements_json,
                        ..Default::default()
                    })),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }],
        ..Default::default()
    };

    let mut cpu = CpuCompositor::default();
    cpu.set_media_root(Some(store.0.clone()));
    cpu.set_video_for_hash(&vid_hash, Some(video_frame.clone()))
        .unwrap();

    let mut gpu = GpuCompositor::new(AdapterChoice::Software).unwrap();
    gpu.set_media_root(Some(store.0.clone()));
    gpu.set_video_for_hash(&vid_hash, Some(video_frame))
        .unwrap();

    let cpu_frame = cpu.render_output(&scene, 0, 320, 180).unwrap();
    let gpu_frame = gpu.render_output(&scene, 0, 320, 180).unwrap();
    assert_frames_close(&gpu_frame, &cpu_frame, 3);

    // At (0.07, 0.07) -> (22, 12): only red video (z=1) is here.
    let vid_px = pixel(&gpu_frame, 22, 12);
    assert!(
        vid_px[0] > 180 && vid_px[1] < 50 && vid_px[2] < 50,
        "expected red video, got {vid_px:?}"
    );

    // At (0.25, 0.25) -> (80, 45): yellow top-shape (z=20) beats green image (z=10) and red video (z=1).
    let top_px = pixel(&gpu_frame, 80, 45);
    assert!(
        top_px[0] > 210 && top_px[1] > 170 && top_px[2] < 50,
        "expected yellow top card, got {top_px:?}"
    );

    // At (0.55, 0.55) -> (176, 99): green image (z=10) beats dark backdrop & red video.
    let img_px = pixel(&gpu_frame, 176, 99);
    assert!(
        img_px[1] > 180 && img_px[0] < 30,
        "expected green image, got {img_px:?}"
    );

    // At (0.83, 0.79) -> (265, 142): blue ellipse (z=15).
    let ell_px = pixel(&gpu_frame, 265, 142);
    assert!(
        ell_px[2] > 200 && ell_px[0] < 70,
        "expected blue ellipse, got {ell_px:?}"
    );
}

#[test]
fn clock_and_timer_tokens_update_live_without_stale_overlay_cache_on_cpu_and_gpu() {
    let _guard = GPU_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let t0_ms: i64 = 1_700_000_000_000;

    let doc = SlideElementsDocument {
        elements: vec![SlideElement {
            id: "clock".into(),
            name: "Countdown".into(),
            kind: SlideElementKind::Text,
            x: 0.1,
            y: 0.2,
            width: 0.8,
            height: 0.6,
            z_index: 1,
            text: "Time {clock} · Left {timer}".into(),
            font_family: "JetBrains Mono".into(),
            font_size: 48.0,
            weight: 700,
            color: [255, 255, 255, 255],
            clock_show_seconds: true,
            clock_24h: true,
            timer_target_unix_ms: Some(t0_ms + 90_000),
            timer_count_up: true,
            ..Default::default()
        }],
        ..Default::default()
    };

    let scene = Scene {
        revision: 7,
        outputs: vec![OutputScene {
            output_index: 0,
            layers: vec![
                LayerSpec {
                    kind: LayerKind::Media as i32,
                    enabled: true,
                    content: Some(layer_spec::Content::Media(MediaLayerSpec {
                        solid_color: Some(Color {
                            red: 0.0,
                            green: 0.0,
                            blue: 0.0,
                            alpha: 1.0,
                        }),
                        ..Default::default()
                    })),
                    ..Default::default()
                },
                LayerSpec {
                    kind: LayerKind::Slide as i32,
                    enabled: true,
                    content: Some(layer_spec::Content::Slide(SlideLayerSpec {
                        elements_json: doc.to_json().unwrap(),
                        ..Default::default()
                    })),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }],
        ..Default::default()
    };

    let mut cpu = CpuCompositor::default();
    let mut gpu = GpuCompositor::new(AdapterChoice::Software).unwrap();

    // Render identical scene revision with two timestamps 5 seconds apart -> pixels MUST change!
    cpu.set_clock_unix_ms(Some(t0_ms));
    gpu.set_clock_unix_ms(Some(t0_ms));
    let cpu_a = cpu.render_output(&scene, 0, 480, 160).unwrap();
    let gpu_a = gpu.render_output(&scene, 0, 480, 160).unwrap();
    assert_frames_close(&gpu_a, &cpu_a, 2);

    cpu.set_clock_unix_ms(Some(t0_ms + 5_000));
    gpu.set_clock_unix_ms(Some(t0_ms + 5_000));
    let cpu_b = cpu.render_output(&scene, 0, 480, 160).unwrap();
    let gpu_b = gpu.render_output(&scene, 0, 480, 160).unwrap();
    assert_frames_close(&gpu_b, &cpu_b, 2);

    assert!(
        count_diff_pixels(&cpu_a, &cpu_b) > 20,
        "CPU clock frame remained cached across timestamp change"
    );
    assert!(
        count_diff_pixels(&gpu_a, &gpu_b) > 20,
        "GPU clock frame remained cached across timestamp change"
    );

    // Check token formatter edge cases (12h AM/PM, offset, count-up overrun).
    let base_midnight = 1_700_006_400_000_i64; // 00:00:00 UTC if aligned? Use exact div check:
    let midnight_utc = (base_midnight / 86_400_000) * 86_400_000;
    assert_eq!(
        format_dynamic_tokens(
            "{clock}",
            midnight_utc,
            -300,
            true,
            false,
            None,
            None,
            false
        ),
        "7:00:00 PM"
    );
    assert_eq!(
        format_dynamic_tokens(
            "{timer}",
            t0_ms + 125_000,
            0,
            true,
            true,
            Some(t0_ms + 90_000),
            None,
            true
        ),
        "+00:35"
    );
    assert_eq!(
        format_dynamic_tokens(
            "{timer}",
            t0_ms + 125_000,
            0,
            true,
            true,
            Some(t0_ms + 90_000),
            None,
            false
        ),
        "00:00"
    );
}

#[test]
fn slide_elements_enforce_bounds_sec04_and_alignment_locks() {
    // 1. Forbidden custom shader key rejected strictly
    assert!(
        SlideElementsDocument::from_json_strict(r#"{"wgsl":"@fragment fn main() {}"}"#).is_err()
    );
    assert!(
        SlideElementsDocument::from_json_strict(
            r#"{"elements":[],"effects":[{"kind":"blur","params_json":"{\"shader_code\":\"x\"}"}]}"#
        )
        .is_err()
    );
    assert!(
        SlideElementsDocument::from_json_strict(
            r#"{"elements":[],"effects":[{"kind":"custom_shader","intensity":1.0}]}"#
        )
        .is_err()
    );

    // 2. Over >MAX_ELEMENTS rejected
    let too_many = SlideElementsDocument {
        elements: (0..=MAX_ELEMENTS)
            .map(|i| SlideElement {
                id: format!("e-{i}"),
                kind: SlideElementKind::Shape,
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    };
    let too_many_json = serde_json::to_string(&too_many).unwrap();
    assert!(SlideElementsDocument::from_json_strict(&too_many_json).is_err());

    // 3. Out-of-bounds geometry & path traversal content_hash rejected
    assert!(
        SlideElementsDocument::from_json_strict(
            r#"[{"id":"1","kind":"shape","x":2.5,"y":0.1,"width":0.2,"height":0.2}]"#
        )
        .is_err()
    );
    assert!(
        SlideElementsDocument::from_json_strict(
            r#"[{"id":"1","kind":"image","x":0.1,"y":0.1,"width":0.2,"height":0.2,"content_hash":"../secret","media_extension":"png"}]"#
        )
        .is_err()
    );

    // 4. Multi-element & single-element alignment respects locked state
    let mut elements = vec![
        SlideElement {
            id: "a".into(),
            x: 0.1,
            y: 0.2,
            width: 0.2,
            height: 0.1,
            ..Default::default()
        },
        SlideElement {
            id: "b".into(),
            x: 0.5,
            y: 0.4,
            width: 0.3,
            height: 0.2,
            locked: true,
            ..Default::default()
        },
        SlideElement {
            id: "c".into(),
            x: 0.4,
            y: 0.3,
            width: 0.2,
            height: 0.1,
            ..Default::default()
        },
    ];
    SlideElementsDocument::aligned_positions(&mut elements, &[0, 1, 2], AlignMode::Right);
    // Max right edge across selection is b.x + b.width = 0.8
    assert!((elements[0].x - 0.6).abs() < 1e-4);
    assert!(
        (elements[1].x - 0.5).abs() < 1e-4,
        "locked element must not move"
    );
    assert!((elements[2].x - 0.6).abs() < 1e-4);

    SlideElementsDocument::aligned_positions(&mut elements, &[0], AlignMode::Center);
    assert!((elements[0].x - 0.4).abs() < 1e-4);
}
