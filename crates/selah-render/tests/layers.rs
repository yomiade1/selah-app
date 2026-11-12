//! M3.1 compositor tests: canonical 5-layer z-order, multi-output resolution independence,
//! CPU/GPU pixel parity, and pre-rasterisation cache warming.
use selah_proto::v1::{
    Color, LayerKind, LayerSpec, MediaLayerSpec, MessagesLayerSpec, OutputScene, PreloadHint,
    PropsLayerSpec, Resolution, Scene, SlideLayerSpec, VideoInputLayerSpec, layer_spec,
};
use selah_render::Compositor;
use selah_render::cpu::CpuCompositor;
use selah_render::gpu::{AdapterChoice, GpuCompositor};

fn color(r: f32, g: f32, b: f32, a: f32) -> Option<Color> {
    Some(Color {
        red: r,
        green: g,
        blue: b,
        alpha: a,
    })
}

static GPU_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
                "pixel {} ch {} differs: gpu={:?} cpu={:?}",
                idx,
                ch,
                pa,
                pb
            );
        }
    }
}

#[test]
fn canonical_five_layer_stack_renders_in_z_order_and_matches_cpu_gpu() {
    let _gpu_guard = GPU_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut cpu = CpuCompositor::default();
    let mut gpu = GpuCompositor::new(AdapterChoice::Software).expect("software GPU adapter");

    // Deliberately provide layers in scrambled order; normalization must sort:
    // video_input -> media -> slide -> props -> messages.
    let scrambled = vec![
        LayerSpec {
            kind: LayerKind::Messages as i32,
            enabled: true,
            content: Some(layer_spec::Content::Messages(MessagesLayerSpec {
                text: "Nursery room 2".into(),
                background_color: color(140.0 / 255.0, 32.0 / 255.0, 32.0 / 255.0, 1.0),
                text_color: color(1.0, 1.0, 1.0, 1.0),
                ..Default::default()
            })),
            ..Default::default()
        },
        LayerSpec {
            kind: LayerKind::Slide as i32,
            enabled: true,
            content: Some(layer_spec::Content::Slide(SlideLayerSpec {
                text: "Holy".into(),
                ..Default::default()
            })),
            ..Default::default()
        },
        LayerSpec {
            kind: LayerKind::Props as i32,
            enabled: true,
            content: Some(layer_spec::Content::Props(PropsLayerSpec {
                label: "LIVE".into(),
                background_color: color(20.0 / 255.0, 40.0 / 255.0, 80.0 / 255.0, 1.0),
                text_color: color(1.0, 1.0, 1.0, 1.0),
                ..Default::default()
            })),
            ..Default::default()
        },
        LayerSpec {
            kind: LayerKind::Media as i32,
            enabled: true,
            use_opacity: true,
            opacity: 0.5,
            content: Some(layer_spec::Content::Media(MediaLayerSpec {
                solid_color: color(1.0, 0.0, 0.0, 1.0),
                ..Default::default()
            })),
            ..Default::default()
        },
        LayerSpec {
            kind: LayerKind::VideoInput as i32,
            enabled: true,
            content: Some(layer_spec::Content::VideoInput(VideoInputLayerSpec {
                source_id: "cam-1".into(),
                fallback_color: color(0.0, 0.0, 1.0, 1.0),
                ..Default::default()
            })),
            ..Default::default()
        },
    ];

    let scene = Scene {
        revision: 1,
        outputs: vec![OutputScene {
            output_index: 0,
            resolution: Some(Resolution {
                width: 320,
                height: 180,
            }),
            layers: scrambled,
            ..Default::default()
        }],
        ..Default::default()
    };

    let cpu_frame = cpu.render_output(&scene, 0, 320, 180).unwrap();
    let gpu_frame = gpu.render_output(&scene, 0, 320, 180).unwrap();
    assert_frames_close(&gpu_frame, &cpu_frame, 1);

    // Top-left backdrop blends 50% red media over 100% blue video_input -> ~purple [128, 0, 127, 255].
    let corner = pixel(&gpu_frame, 2, 2);
    assert!(
        corner[0] > 100 && corner[1] < 10 && corner[2] > 100,
        "expected blended media+video backdrop, got {corner:?}"
    );
    // Bottom-left strip belongs to the Messages bar cleanly above all lower layers.
    let bar = pixel(&gpu_frame, 4, 176);
    assert!(
        bar[0].abs_diff(140) <= 2 && bar[1].abs_diff(32) <= 2 && bar[2].abs_diff(32) <= 2,
        "expected messages bar on top at bottom edge, got {bar:?}"
    );

    // Explicit z-order check when props and messages overlap at the bottom: messages win over props.
    let overlap_scene = Scene {
        revision: 2,
        outputs: vec![OutputScene {
            output_index: 0,
            layers: vec![
                LayerSpec {
                    kind: LayerKind::Messages as i32,
                    enabled: true,
                    content: Some(layer_spec::Content::Messages(MessagesLayerSpec {
                        text: "Notice".into(),
                        background_color: color(0.0, 1.0, 0.0, 1.0),
                        text_color: color(0.0, 0.0, 0.0, 1.0),
                        ..Default::default()
                    })),
                    ..Default::default()
                },
                LayerSpec {
                    kind: LayerKind::Props as i32,
                    enabled: true,
                    content: Some(layer_spec::Content::Props(PropsLayerSpec {
                        // Empty label with solid full-frame background_color tests pure full-screen overlap.
                        background_color: color(1.0, 0.0, 0.0, 1.0),
                        ..Default::default()
                    })),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }],
        ..Default::default()
    };
    let overlap_gpu = gpu.render_output(&overlap_scene, 0, 320, 180).unwrap();
    assert_eq!(pixel(&overlap_gpu, 2, 2), [255, 0, 0, 255]);
    assert_eq!(pixel(&overlap_gpu, 2, 178), [0, 255, 0, 255]);
}

#[test]
fn multiple_outputs_render_at_independent_resolutions_simultaneously() {
    let _gpu_guard = GPU_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut cpu = CpuCompositor::default();
    let mut gpu = GpuCompositor::new(AdapterChoice::Software).expect("software GPU adapter");

    let scene = Scene {
        revision: 5,
        outputs: vec![
            OutputScene {
                output_index: 0,
                resolution: Some(Resolution {
                    width: 320,
                    height: 180,
                }),
                layers: vec![LayerSpec {
                    kind: LayerKind::Media as i32,
                    enabled: true,
                    content: Some(layer_spec::Content::Media(MediaLayerSpec {
                        solid_color: color(0.2, 0.4, 0.8, 1.0),
                        ..Default::default()
                    })),
                    ..Default::default()
                }],
                ..Default::default()
            },
            OutputScene {
                output_index: 1,
                resolution: Some(Resolution {
                    width: 640,
                    height: 180,
                }),
                layers: vec![LayerSpec {
                    kind: LayerKind::Slide as i32,
                    enabled: true,
                    content: Some(layer_spec::Content::Slide(SlideLayerSpec {
                        text: "LED Wall".into(),
                        ..Default::default()
                    })),
                    ..Default::default()
                }],
                ..Default::default()
            },
            OutputScene {
                output_index: 2,
                resolution: Some(Resolution {
                    width: 240,
                    height: 180,
                }),
                layers: vec![LayerSpec {
                    kind: LayerKind::Media as i32,
                    enabled: true,
                    content: Some(layer_spec::Content::Media(MediaLayerSpec {
                        solid_color: color(0.1, 0.6, 0.2, 1.0),
                        ..Default::default()
                    })),
                    ..Default::default()
                }],
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    for (output_index, w, h) in [(0, 320, 180), (1, 640, 180), (2, 240, 180)] {
        let gf = gpu.render_output(&scene, output_index, w, h).unwrap();
        let cf = cpu.render_output(&scene, output_index, w, h).unwrap();
        assert_frames_close(&gf, &cf, 1);
    }
    assert!(gpu.output_view_for(0).is_some());
    assert!(gpu.output_view_for(1).is_some());
    assert!(gpu.output_view_for(2).is_some());
}

#[test]
fn prerasterise_hints_warm_caches_for_upcoming_slides() {
    let mut cpu = CpuCompositor::default();
    let warmed = cpu
        .prerasterise_hints(&[PreloadHint {
            key: "next-verse".into(),
            output_index: 0,
            resolution: Some(Resolution {
                width: 320,
                height: 180,
            }),
            text: "How great is our God".into(),
            ..Default::default()
        }])
        .unwrap();
    assert_eq!(warmed, 1);
    let (hits_before, misses_before) = cpu.cache_stats();

    let scene = Scene {
        revision: 1,
        outputs: vec![OutputScene {
            output_index: 0,
            layers: vec![LayerSpec {
                kind: LayerKind::Slide as i32,
                enabled: true,
                content: Some(layer_spec::Content::Slide(SlideLayerSpec {
                    text: "How great is our God".into(),
                    ..Default::default()
                })),
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    };
    cpu.render_output(&scene, 0, 320, 180).unwrap();
    let (hits_after, misses_after) = cpu.cache_stats();
    assert!(hits_after > hits_before, "expected cache hit after preload");
    assert_eq!(misses_after, misses_before);
}

#[test]
fn prerasterise_hints_draw_each_upcoming_slide_only_once() {
    // Core resends the upcoming slides with every change; the engine must not redraw them.
    let mut cpu = CpuCompositor::default();
    let hint = PreloadHint {
        key: "next-verse".into(),
        output_index: 0,
        resolution: Some(Resolution {
            width: 320,
            height: 180,
        }),
        text: "Great is Thy faithfulness".into(),
        ..Default::default()
    };
    assert_eq!(
        cpu.prerasterise_hints(std::slice::from_ref(&hint)).unwrap(),
        1
    );
    let stats = cpu.cache_stats();
    assert_eq!(cpu.prerasterise_hints(&[hint.clone(), hint]).unwrap(), 0);
    assert_eq!(
        cpu.cache_stats(),
        stats,
        "a cached hint is neither a hit nor a miss"
    );
}

struct TestDir(std::path::PathBuf);

impl TestDir {
    fn new(prefix: &str) -> Self {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "selah-render-test-{prefix}-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn store_media(&self, ext: &str, bytes: &[u8]) -> String {
        let hash = blake3::hash(bytes).to_hex().to_string();
        let shard = self.0.join(&hash[..2]);
        std::fs::create_dir_all(&shard).unwrap();
        std::fs::write(shard.join(format!("{hash}.{ext}")), bytes).unwrap();
        hash
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn image_background_supports_fit_fill_stretch_blur_and_opacity_with_cpu_gpu_parity() {
    let _gpu_guard = GPU_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    use selah_proto::v1::MediaFitMode;
    let store = TestDir::new("img-bg");

    // 160x40 image (4:1 aspect): left half pure red, right half pure green.
    let mut src = selah_render::Frame::new(160, 40);
    for y in 0..40 {
        for x in 0..160 {
            let off = (y * 160 + x) * 4;
            if x < 80 {
                src.pixels[off..off + 4].copy_from_slice(&[255, 0, 0, 255]);
            } else {
                src.pixels[off..off + 4].copy_from_slice(&[0, 255, 0, 255]);
            }
        }
    }
    let png = selah_render::png::encode(&src).unwrap();
    let hash = store.store_media("png", &png);

    let mut cpu = CpuCompositor::default();
    cpu.set_media_root(Some(store.0.clone()));
    let mut gpu = GpuCompositor::new(AdapterChoice::Software).expect("software GPU");
    gpu.set_media_root(Some(store.0.clone()));

    let make_scene = |fit: MediaFitMode, blur_radius: f32, opacity: f32| Scene {
        revision: 1,
        outputs: vec![OutputScene {
            output_index: 0,
            layers: vec![
                LayerSpec {
                    kind: LayerKind::VideoInput as i32,
                    enabled: true,
                    content: Some(layer_spec::Content::VideoInput(VideoInputLayerSpec {
                        fallback_color: color(0.0, 0.0, 1.0, 1.0),
                        ..Default::default()
                    })),
                    ..Default::default()
                },
                LayerSpec {
                    kind: LayerKind::Media as i32,
                    enabled: true,
                    use_opacity: true,
                    opacity,
                    content: Some(layer_spec::Content::Media(MediaLayerSpec {
                        content_hash: hash.clone(),
                        media_kind: "image".into(),
                        media_extension: "png".into(),
                        fit: fit as i32,
                        blur_radius,
                        use_opacity: true,
                        opacity,
                        ..Default::default()
                    })),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }],
        ..Default::default()
    };

    // 1. Fit (4:1 inside 16:9 160x90 -> image occupies y=25..65; top/bottom transparent bars show blue).
    let fit_cpu = cpu
        .render_output(&make_scene(MediaFitMode::Fit, 0.0, 1.0), 0, 160, 90)
        .unwrap();
    let fit_gpu = gpu
        .render_output(&make_scene(MediaFitMode::Fit, 0.0, 1.0), 0, 160, 90)
        .unwrap();
    assert_frames_close(&fit_gpu, &fit_cpu, 1);
    assert_eq!(pixel(&fit_gpu, 10, 5), [0, 0, 255, 255]);
    assert_eq!(pixel(&fit_gpu, 10, 45), [255, 0, 0, 255]);
    assert_eq!(pixel(&fit_gpu, 140, 45), [0, 255, 0, 255]);

    // 2. Fill (crops left & right of 4:1 inside 16:9 so center boundary remains visible and edges are full-bleed).
    let fill_cpu = cpu
        .render_output(&make_scene(MediaFitMode::Fill, 0.0, 1.0), 0, 160, 90)
        .unwrap();
    let fill_gpu = gpu
        .render_output(&make_scene(MediaFitMode::Fill, 0.0, 1.0), 0, 160, 90)
        .unwrap();
    assert_frames_close(&fill_gpu, &fill_cpu, 1);
    assert_eq!(pixel(&fill_gpu, 10, 5), [255, 0, 0, 255]);
    assert_eq!(pixel(&fill_gpu, 150, 5), [0, 255, 0, 255]);

    // 3. Stretch + 50% opacity blends red over blue -> purple (~128, 0, 127).
    let st_cpu = cpu
        .render_output(&make_scene(MediaFitMode::Stretch, 0.0, 0.5), 0, 160, 90)
        .unwrap();
    let st_gpu = gpu
        .render_output(&make_scene(MediaFitMode::Stretch, 0.0, 0.5), 0, 160, 90)
        .unwrap();
    assert_frames_close(&st_gpu, &st_cpu, 1);
    let blended = pixel(&st_gpu, 10, 5);
    assert!(
        blended[0].abs_diff(128) <= 2 && blended[1] == 0 && blended[2].abs_diff(127) <= 2,
        "expected 50% red over blue, got {blended:?}"
    );

    // 4. Blur mixes the red/green seam smoothly.
    let blur_cpu = cpu
        .render_output(&make_scene(MediaFitMode::Stretch, 10.0, 1.0), 0, 160, 90)
        .unwrap();
    let blur_gpu = gpu
        .render_output(&make_scene(MediaFitMode::Stretch, 10.0, 1.0), 0, 160, 90)
        .unwrap();
    assert_frames_close(&blur_gpu, &blur_cpu, 1);
    let mid = pixel(&blur_gpu, 80, 45);
    assert!(
        mid[0] > 80 && mid[1] > 80 && mid[2] == 0,
        "expected blurred seam mix at center, got {mid:?}"
    );
}

#[test]
fn video_layer_supports_fit_blur_opacity_z_order_and_cpu_gpu_parity() {
    let _gpu_guard = GPU_TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    use selah_proto::v1::MediaFitMode;
    use selah_render::video::{Nv12Frame, YuvMatrix};

    // 64x32 NV12: left half white (Y=235,U=128,V=128), right half BT.709 red (Y=51,U=109,V=212).
    let (vw, vh) = (64_u32, 32_u32);
    let mut y_plane = vec![0_u8; (vw * vh) as usize];
    for row in 0..vh as usize {
        for col in 0..vw as usize {
            y_plane[row * vw as usize + col] = if col < 32 { 235 } else { 51 };
        }
    }
    let mut uv_plane = vec![0_u8; (vw / 2 * 2 * (vh / 2)) as usize];
    for row in 0..(vh / 2) as usize {
        for col in 0..(vw / 2) as usize {
            let off = (row * (vw as usize / 2) + col) * 2;
            if col < 16 {
                uv_plane[off] = 128;
                uv_plane[off + 1] = 128;
            } else {
                uv_plane[off] = 109;
                uv_plane[off + 1] = 212;
            }
        }
    }
    let nv12 = Nv12Frame {
        width: vw,
        height: vh,
        y: y_plane,
        uv: uv_plane,
        matrix: YuvMatrix::Bt709,
        full_range: false,
    };
    let hash = "a".repeat(64);

    let mut cpu = CpuCompositor::default();
    cpu.set_video_for_hash(&hash, Some(nv12.clone())).unwrap();
    let mut gpu = GpuCompositor::new(AdapterChoice::Software).expect("software GPU");
    gpu.set_video_for_hash(&hash, Some(nv12)).unwrap();

    let scene = Scene {
        revision: 10,
        outputs: vec![OutputScene {
            output_index: 0,
            layers: vec![
                LayerSpec {
                    kind: LayerKind::VideoInput as i32,
                    enabled: true,
                    content: Some(layer_spec::Content::VideoInput(VideoInputLayerSpec {
                        fallback_color: color(0.0, 0.0, 1.0, 1.0),
                        ..Default::default()
                    })),
                    ..Default::default()
                },
                LayerSpec {
                    kind: LayerKind::Media as i32,
                    enabled: true,
                    use_opacity: true,
                    opacity: 0.75,
                    content: Some(layer_spec::Content::Media(MediaLayerSpec {
                        content_hash: hash.clone(),
                        media_kind: "video".into(),
                        media_extension: "mp4".into(),
                        fit: MediaFitMode::Fit as i32,
                        blur_radius: 6.0,
                        use_opacity: true,
                        opacity: 0.75,
                        ..Default::default()
                    })),
                    ..Default::default()
                },
                LayerSpec {
                    kind: LayerKind::Messages as i32,
                    enabled: true,
                    content: Some(layer_spec::Content::Messages(MessagesLayerSpec {
                        text: "LIVE".into(),
                        background_color: color(0.0, 1.0, 0.0, 1.0),
                        text_color: color(0.0, 0.0, 0.0, 1.0),
                        ..Default::default()
                    })),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }],
        ..Default::default()
    };

    // 160x160 square target: 2:1 video in Fit leaves blue bands top/bottom, blurred seam in center,
    // and green Messages bar cleanly on top at the bottom edge.
    let f_cpu = cpu.render_output(&scene, 0, 160, 160).unwrap();
    let f_gpu = gpu.render_output(&scene, 0, 160, 160).unwrap();
    assert_frames_close(&f_gpu, &f_cpu, 4);

    assert!(
        pixel(&f_gpu, 10, 8)[2] > 200,
        "top band should show blue VideoInput"
    );
    let center = pixel(&f_gpu, 80, 80);
    assert!(
        center[0] > 100 && center[1] > 40,
        "blurred seam should mix white+red: {center:?}"
    );
    let msg = pixel(&f_gpu, 4, 156);
    assert!(
        msg[1] > 240 && msg[0] < 10,
        "Messages layer must stay above video: {msg:?}"
    );
}

#[test]
fn background_image_loader_refuses_corrupt_hash_and_oversize_bomb() {
    let store = TestDir::new("sec04");
    let valid_hash = "b".repeat(64);
    let shard = store.0.join("bb");
    std::fs::create_dir_all(&shard).unwrap();
    // Corrupt content under hash 'bb...'.
    let png = selah_render::png::encode(&selah_render::Frame::new(4, 4)).unwrap();
    std::fs::write(shard.join(format!("{valid_hash}.png")), &png).unwrap();

    let mut cpu = CpuCompositor::default();
    cpu.set_media_root(Some(store.0.clone()));
    let bad_scene = |hash: &str| Scene {
        revision: 1,
        outputs: vec![OutputScene {
            output_index: 0,
            layers: vec![LayerSpec {
                kind: LayerKind::Media as i32,
                enabled: true,
                content: Some(layer_spec::Content::Media(MediaLayerSpec {
                    content_hash: hash.into(),
                    media_kind: "image".into(),
                    media_extension: "png".into(),
                    ..Default::default()
                })),
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    };

    assert!(
        cpu.render_output(&bad_scene(&valid_hash), 0, 64, 64)
            .is_err(),
        "corrupted image hash must be refused (SEC-15)"
    );
    assert!(
        cpu.render_output(&bad_scene("../etc/passwd"), 0, 64, 64)
            .is_err(),
        "path traversal hash must be refused (SEC-04)"
    );

    // 9000x9000 IHDR bomb stored under its real BLAKE3 hash must still be refused cleanly (SEC-04).
    let mut bomb = selah_render::png::encode(&selah_render::Frame::new(1, 1)).unwrap();
    bomb[16..20].copy_from_slice(&9000u32.to_be_bytes());
    bomb[20..24].copy_from_slice(&9000u32.to_be_bytes());
    let mut crc = 0xFFFF_FFFF_u32;
    for &byte in &bomb[12..29] {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    bomb[29..33].copy_from_slice(&(!crc).to_be_bytes());
    let bomb_hash = store.store_media("png", &bomb);
    assert!(
        cpu.render_output(&bad_scene(&bomb_hash), 0, 64, 64)
            .is_err(),
        "9000x9000 image bomb must be refused (SEC-04)"
    );
}
