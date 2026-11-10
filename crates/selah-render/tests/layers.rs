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
