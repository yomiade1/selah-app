//! M3.6 / M3-A3 offscreen scene thumbnail renderer & bounded disk cache tests:
//! GPU/CPU pixel fidelity across layers & effects, canonical BLAKE3 cache hits,
//! corrupt-cache self-healing, bounded eviction, video poster fallback, flash_warning
//! propagation, and SEC-03/SEC-04 input validation.
use std::path::{Path, PathBuf};

use selah_core::CoreService;
use selah_core::library::new_id;
use selah_core::thumbnails::MAX_SCENE_THUMB_FILES;
use selah_proto::v1::*;
use selah_render::Compositor;
use selah_render::gpu::{AdapterChoice, GpuCompositor};
use selah_render::png::{decode as decode_png, encode as encode_png, encode_rgba_thumbnail};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn ui_cmd(id: u64, action: command::Action) -> Envelope {
    Envelope {
        id,
        correlation_id: 0,
        actor: Some(Actor {
            user_id: "local-owner".into(),
            device_id: "thumb-test".into(),
            origin: ActorOrigin::Ui as i32,
        }),
        body: Some(envelope::Body::Command(Command {
            action: Some(action),
            undo_group: None,
        })),
    }
}

fn ui_query(id: u64, request: query::Request) -> Envelope {
    Envelope {
        id,
        correlation_id: 0,
        actor: Some(Actor {
            user_id: "local-owner".into(),
            device_id: "thumb-test".into(),
            origin: ActorOrigin::Ui as i32,
        }),
        body: Some(envelope::Body::Query(Query {
            request: Some(request),
        })),
    }
}

fn unwrap_reply(outcome: selah_core::Outcome) -> reply::Result {
    match outcome.reply.and_then(|e| e.body) {
        Some(envelope::Body::Reply(r)) => r.result.expect("reply missing result"),
        Some(envelope::Body::Error(err)) => {
            panic!("unexpected error {} ({})", err.code, err.message_key)
        }
        other => panic!("unexpected outcome: {other:?}"),
    }
}

fn expect_error(outcome: selah_core::Outcome) -> selah_proto::v1::Error {
    match outcome.reply.and_then(|e| e.body) {
        Some(envelope::Body::Error(err)) => err,
        other => panic!("expected error, got {other:?}"),
    }
}

fn make_gradient_frame(w: u32, h: u32) -> selah_render::Frame {
    let mut f = selah_render::Frame::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let i = (y as usize * w as usize + x as usize) * 4;
            let r = ((x * 200) / w.max(1)).min(255) as u8;
            let g = ((y * 180) / h.max(1)).min(255) as u8;
            f.pixels[i..i + 4].copy_from_slice(&[r, g, 160, 255]);
        }
    }
    f
}

fn color(r: f32, g: f32, b: f32, a: f32) -> Option<Color> {
    Some(Color {
        red: r,
        green: g,
        blue: b,
        alpha: a,
    })
}

fn mean_channel_abs_diff(a: &selah_render::Frame, b: &selah_render::Frame) -> f64 {
    assert_eq!((a.width, a.height), (b.width, b.height));
    let mut sum = 0_u64;
    for (&pa, &pb) in a.pixels.iter().zip(&b.pixels) {
        sum += u64::from(pa.abs_diff(pb));
    }
    sum as f64 / a.pixels.len().max(1) as f64
}

#[test]
fn scene_thumbnail_matches_gpu_compositor_for_layers_and_effects() {
    // req: M3-A3
    let dir = tempfile::tempdir().unwrap();
    let mut core = CoreService::new(dir.path(), false);

    let bg_src = dir.path().join("backdrop.png");
    let bg_png = encode_png(&make_gradient_frame(640, 360)).unwrap();
    std::fs::write(&bg_src, bg_png).unwrap();

    let reply::Result::MediaItem(media_item) = unwrap_reply(core.handle(&ui_cmd(
        1,
        command::Action::ImportMedia(ImportMedia {
            source_path: bg_src.to_string_lossy().into_owned(),
            name: "Blue Backdrop".into(),
            tags: vec![],
        }),
    ))) else {
        panic!("expected MediaItem");
    };

    let scene = Scene {
        revision: 12,
        outputs: vec![OutputScene {
            output_index: 0,
            role: OutputRole::Main as i32,
            resolution: Some(Resolution {
                width: 1280,
                height: 720,
            }),
            effects: vec![EffectSpec {
                kind: EffectKind::Vignette as i32,
                enabled: true,
                intensity: 0.4,
                params_json: r#"{"inner_radius":0.5,"softness":0.4}"#.into(),
            }],
            layers: vec![
                LayerSpec {
                    kind: LayerKind::Media as i32,
                    enabled: true,
                    content: Some(layer_spec::Content::Media(MediaLayerSpec {
                        content_hash: media_item.content_hash.clone(),
                        media_kind: "image".into(),
                        media_extension: media_item.extension.clone(),
                        fit: MediaFitMode::Fill as i32,
                        effects: vec![EffectSpec {
                            kind: EffectKind::ColourAdjust as i32,
                            enabled: true,
                            intensity: 0.6,
                            params_json: r#"{"saturation":1.2,"contrast":1.05}"#.into(),
                        }],
                        ..Default::default()
                    })),
                    ..Default::default()
                },
                LayerSpec {
                    kind: LayerKind::Slide as i32,
                    enabled: true,
                    content: Some(layer_spec::Content::Slide(SlideLayerSpec {
                        title_text: "Amazing Grace".into(),
                        text: "How sweet the sound".into(),
                        footer_text: "CCLI 22025".into(),
                        style_json: r##"{"body":{"font_family":"Inter","size":68,"align_h":"center","color":"#FFFFFF"}}"##.into(),
                        ..Default::default()
                    })),
                    ..Default::default()
                },
                LayerSpec {
                    kind: LayerKind::Props as i32,
                    enabled: true,
                    content: Some(layer_spec::Content::Props(PropsLayerSpec {
                        label: "SANCTUARY".into(),
                        background_color: color(0.1, 0.25, 0.55, 0.9),
                        text_color: color(1.0, 1.0, 1.0, 1.0),
                        ..Default::default()
                    })),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }],
        ..Default::default()
    };

    let reply::Result::SceneThumbnail(thumb) = unwrap_reply(core.handle(&ui_query(
        2,
        query::Request::RenderSceneThumbnail(RenderSceneThumbnail {
            scene: Some(scene.clone()),
            output_index: 0,
            max_width: 320,
            max_height: 180,
        }),
    ))) else {
        panic!("expected SceneThumbnail");
    };

    assert_eq!(thumb.width, 320);
    assert_eq!(thumb.height, 180);
    assert!(!thumb.flash_warning);
    assert!(!thumb.cache_key.is_empty());

    let thumb_frame = decode_png(&thumb.png_bytes).expect("valid thumbnail PNG");
    assert_eq!((thumb_frame.width, thumb_frame.height), (320, 180));

    let mut gpu =
        GpuCompositor::new(AdapterChoice::Software).expect("software GPU adapter available");
    gpu.set_media_root(Some(core.library().paths().media.clone()));
    let gpu_full = gpu
        .render_output(&scene, 0, 1280, 720)
        .expect("GPU output render");
    let (_, _, gpu_thumb_png) =
        encode_rgba_thumbnail(&gpu_full, 320, 180).expect("downscale GPU frame");
    let gpu_thumb_frame = decode_png(&gpu_thumb_png).expect("decode GPU thumb");

    let mae = mean_channel_abs_diff(&thumb_frame, &gpu_thumb_frame);
    assert!(
        mae <= 3.0,
        "offscreen scene thumbnail drifted from GPU compositor (MAE={mae:.3})"
    );
}

#[test]
fn scene_thumbnail_caches_canonically_heals_corrupt_files_and_evicts_lru() {
    let dir = tempfile::tempdir().unwrap();
    let mut core = CoreService::new(dir.path(), false);

    let scene_v1 = Scene {
        revision: 1,
        is_preview: false,
        preload_items: vec![PreloadHint {
            key: "next".into(),
            text: "Ignored Hint".into(),
            ..Default::default()
        }],
        outputs: vec![
            OutputScene {
                output_index: 0,
                resolution: Some(Resolution {
                    width: 640,
                    height: 360,
                }),
                layers: vec![LayerSpec {
                    kind: LayerKind::Media as i32,
                    enabled: true,
                    content: Some(layer_spec::Content::Media(MediaLayerSpec {
                        solid_color: color(0.15, 0.35, 0.65, 1.0),
                        ..Default::default()
                    })),
                    ..Default::default()
                }],
                ..Default::default()
            },
            OutputScene {
                output_index: 1,
                resolution: Some(Resolution {
                    width: 1280,
                    height: 360,
                }),
                layers: vec![LayerSpec {
                    kind: LayerKind::Slide as i32,
                    enabled: true,
                    content: Some(layer_spec::Content::Slide(SlideLayerSpec {
                        text: "Ultra-Wide LED".into(),
                        ..Default::default()
                    })),
                    ..Default::default()
                }],
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    let reply::Result::SceneThumbnail(t1) = unwrap_reply(core.handle(&ui_query(
        10,
        query::Request::RenderSceneThumbnail(RenderSceneThumbnail {
            scene: Some(scene_v1.clone()),
            output_index: 0,
            max_width: 320,
            max_height: 180,
        }),
    ))) else {
        panic!("expected SceneThumbnail");
    };

    let scenes_dir = core.library().paths().thumbnails.join("scenes");
    let cache_path = scenes_dir.join(format!("{}_{}x{}.png", t1.cache_key, t1.width, t1.height));
    assert!(
        cache_path.is_file(),
        "expected cache file at {cache_path:?}"
    );

    // Changing volatile fields OR unrelated output 1 must hit the exact same canonical cache key.
    let mut scene_v2 = scene_v1.clone();
    scene_v2.revision = 444;
    scene_v2.is_preview = true;
    scene_v2.preload_items.clear();
    scene_v2.outputs[1].cleared = true;
    let reply::Result::SceneThumbnail(t2) = unwrap_reply(core.handle(&ui_query(
        11,
        query::Request::RenderSceneThumbnail(RenderSceneThumbnail {
            scene: Some(scene_v2),
            output_index: 0,
            max_width: 320,
            max_height: 180,
        }),
    ))) else {
        panic!("expected SceneThumbnail");
    };
    assert_eq!(t1.cache_key, t2.cache_key);
    assert_eq!(t1.png_bytes, t2.png_bytes);

    // Output 1 (32:9 ultra-wide) must preserve aspect ratio cleanly (`320x90`).
    let reply::Result::SceneThumbnail(led) = unwrap_reply(core.handle(&ui_query(
        12,
        query::Request::RenderSceneThumbnail(RenderSceneThumbnail {
            scene: Some(scene_v1.clone()),
            output_index: 1,
            max_width: 320,
            max_height: 180,
        }),
    ))) else {
        panic!("expected SceneThumbnail for output 1");
    };
    assert_eq!((led.width, led.height), (320, 90));

    // Corrupt cached PNG (valid 24-byte IHDR header followed by garbage IDAT) -> must self-heal.
    let mut corrupted = t1.png_bytes[..36].to_vec();
    corrupted.extend_from_slice(b"corrupt-idat-payload");
    std::fs::write(&cache_path, &corrupted).unwrap();
    let reply::Result::SceneThumbnail(healed) = unwrap_reply(core.handle(&ui_query(
        13,
        query::Request::RenderSceneThumbnail(RenderSceneThumbnail {
            scene: Some(scene_v1),
            output_index: 0,
            max_width: 320,
            max_height: 180,
        }),
    ))) else {
        panic!("expected healed SceneThumbnail");
    };
    let decoded = decode_png(&healed.png_bytes).expect("healed PNG must decode cleanly");
    assert_eq!((decoded.width, decoded.height), (320, 180));
    assert_eq!(healed.png_bytes, t1.png_bytes);

    // Populate extra dummy cache entries beyond MAX_SCENE_THUMB_FILES and verify bounded eviction.
    for i in 0..(MAX_SCENE_THUMB_FILES + 5) {
        std::fs::write(scenes_dir.join(format!("dummy_{i:04}.png")), b"placeholder").unwrap();
    }
    let scene_distinct = Scene {
        revision: 2,
        outputs: vec![OutputScene {
            output_index: 0,
            resolution: Some(Resolution {
                width: 640,
                height: 360,
            }),
            layers: vec![LayerSpec {
                kind: LayerKind::Media as i32,
                enabled: true,
                content: Some(layer_spec::Content::Media(MediaLayerSpec {
                    solid_color: color(0.7, 0.2, 0.1, 1.0),
                    ..Default::default()
                })),
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    };
    unwrap_reply(core.handle(&ui_query(
        14,
        query::Request::RenderSceneThumbnail(RenderSceneThumbnail {
            scene: Some(scene_distinct),
            output_index: 0,
            max_width: 320,
            max_height: 180,
        }),
    )));
    let png_count = std::fs::read_dir(&scenes_dir)
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("png"))
        .count();
    assert!(
        png_count <= MAX_SCENE_THUMB_FILES,
        "scene thumbnail cache grew to {png_count} > {MAX_SCENE_THUMB_FILES}"
    );
}

#[test]
fn scene_thumbnail_renders_video_poster_and_propagates_flash_warning() {
    // req: M3-A3, SAFE-04
    let dir = tempfile::tempdir().unwrap();
    let mut core = CoreService::new(dir.path(), false);

    let flash_clip = repo_root().join("tests/fixtures/media/committed/wcag-flash-360p60.mp4");
    let reply::Result::MediaItem(flash_item) = unwrap_reply(core.handle(&ui_cmd(
        1,
        command::Action::ImportMedia(ImportMedia {
            source_path: flash_clip.to_string_lossy().into_owned(),
            name: "Strobe Video".into(),
            tags: vec![],
        }),
    ))) else {
        panic!("expected MediaItem");
    };
    assert!(flash_item.flash_warning);

    // Delete pre-extracted poster PNG so RenderSceneThumbnail exercises worker poster fallback.
    let expected_poster = core
        .library()
        .paths()
        .thumbnails
        .join(format!("{}_320x180.png", flash_item.content_hash));
    if expected_poster.exists() {
        std::fs::remove_file(&expected_poster).unwrap();
    }

    let scene = Scene {
        revision: 1,
        outputs: vec![OutputScene {
            output_index: 0,
            resolution: Some(Resolution {
                width: 640,
                height: 360,
            }),
            layers: vec![
                LayerSpec {
                    kind: LayerKind::Media as i32,
                    enabled: true,
                    content: Some(layer_spec::Content::Media(MediaLayerSpec {
                        content_hash: flash_item.content_hash.clone(),
                        media_kind: "video".into(),
                        media_extension: flash_item.extension.clone(),
                        fit: MediaFitMode::Fill as i32,
                        ..Default::default()
                    })),
                    ..Default::default()
                },
                LayerSpec {
                    kind: LayerKind::Slide as i32,
                    enabled: true,
                    content: Some(layer_spec::Content::Slide(SlideLayerSpec {
                        text: "Hallelujah".into(),
                        ..Default::default()
                    })),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }],
        ..Default::default()
    };

    let reply::Result::SceneThumbnail(thumb) = unwrap_reply(core.handle(&ui_query(
        2,
        query::Request::RenderSceneThumbnail(RenderSceneThumbnail {
            scene: Some(scene),
            output_index: 0,
            max_width: 320,
            max_height: 180,
        }),
    ))) else {
        panic!("expected SceneThumbnail");
    };

    assert!(
        thumb.flash_warning,
        "SceneThumbnail must propagate flash_warning from flashing background video"
    );
    assert!(
        expected_poster.is_file(),
        "expected video poster fallback cached on disk"
    );
    let frame = decode_png(&thumb.png_bytes).expect("decode video scene thumbnail");
    let non_black = frame
        .pixels
        .chunks_exact(4)
        .filter(|px| px[0] > 10 || px[1] > 10 || px[2] > 10)
        .count();
    assert!(
        non_black > 500,
        "video poster + text thumbnail should have visible pixels (got {non_black})"
    );
}

#[test]
fn scene_thumbnail_rejects_malformed_and_unsafe_inputs() {
    // sec: SEC-03, SEC-04
    let dir = tempfile::tempdir().unwrap();
    let mut core = CoreService::new(dir.path(), false);

    // 1. Missing scene
    let err = expect_error(core.handle(&ui_query(
        1,
        query::Request::RenderSceneThumbnail(RenderSceneThumbnail {
            scene: None,
            output_index: 0,
            max_width: 320,
            max_height: 180,
        }),
    )));
    assert_eq!(
        err.code,
        selah_core::library::CommandBusError::Invalid(String::new()).code()
    );

    // 2. Oversized output resolution
    let err = expect_error(core.handle(&ui_query(
        2,
        query::Request::RenderSceneThumbnail(RenderSceneThumbnail {
            scene: Some(Scene {
                resolution: Some(Resolution {
                    width: 16384,
                    height: 16384,
                }),
                ..Default::default()
            }),
            output_index: 0,
            max_width: 320,
            max_height: 180,
        }),
    )));
    assert_eq!(
        err.code,
        selah_core::library::CommandBusError::Invalid(String::new()).code()
    );

    // 3. Forbidden custom WGSL payload in effects
    let err = expect_error(core.handle(&ui_query(
        3,
        query::Request::RenderSceneThumbnail(RenderSceneThumbnail {
            scene: Some(Scene {
                outputs: vec![OutputScene {
                    output_index: 0,
                    effects: vec![EffectSpec {
                        kind: EffectKind::Blur as i32,
                        enabled: true,
                        intensity: 0.5,
                        params_json: r#"{"wgsl":"fn evil() {}"}"#.into(),
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }),
            output_index: 0,
            max_width: 320,
            max_height: 180,
        }),
    )));
    assert_eq!(
        err.code,
        selah_core::library::CommandBusError::Invalid(String::new()).code()
    );

    // 4. Malformed style_json
    let err = expect_error(core.handle(&ui_query(
        4,
        query::Request::RenderSceneThumbnail(RenderSceneThumbnail {
            scene: Some(Scene {
                outputs: vec![OutputScene {
                    output_index: 0,
                    layers: vec![LayerSpec {
                        kind: LayerKind::Slide as i32,
                        enabled: true,
                        content: Some(layer_spec::Content::Slide(SlideLayerSpec {
                            text: "Hello".into(),
                            style_json: "{not json".into(),
                            ..Default::default()
                        })),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }),
            output_index: 0,
            max_width: 320,
            max_height: 180,
        }),
    )));
    assert_eq!(
        err.code,
        selah_core::library::CommandBusError::Invalid(String::new()).code()
    );

    // 5. Path traversal / invalid media hash
    let err = expect_error(core.handle(&ui_query(
        5,
        query::Request::RenderSceneThumbnail(RenderSceneThumbnail {
            scene: Some(Scene {
                outputs: vec![OutputScene {
                    output_index: 0,
                    layers: vec![LayerSpec {
                        kind: LayerKind::Media as i32,
                        enabled: true,
                        content: Some(layer_spec::Content::Media(MediaLayerSpec {
                            content_hash: "../../secret".into(),
                            media_kind: "image".into(),
                            media_extension: "png".into(),
                            ..Default::default()
                        })),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }),
            output_index: 0,
            max_width: 320,
            max_height: 180,
        }),
    )));
    assert_eq!(
        err.code,
        selah_core::library::CommandBusError::Invalid(String::new()).code()
    );

    // 6. Valid-shaped hash missing from media store must fail cleanly (not silently succeed)
    let _ = new_id();
    let err = expect_error(core.handle(&ui_query(
        6,
        query::Request::RenderSceneThumbnail(RenderSceneThumbnail {
            scene: Some(Scene {
                outputs: vec![OutputScene {
                    output_index: 0,
                    layers: vec![LayerSpec {
                        kind: LayerKind::Media as i32,
                        enabled: true,
                        content: Some(layer_spec::Content::Media(MediaLayerSpec {
                            content_hash: "a".repeat(64),
                            media_kind: "image".into(),
                            media_extension: "png".into(),
                            ..Default::default()
                        })),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }),
            output_index: 0,
            max_width: 320,
            max_height: 180,
        }),
    )));
    assert_eq!(
        err.code,
        selah_core::library::CommandBusError::Invalid(String::new()).code()
    );
}

#[test]
fn scene_thumbnail_renders_elements_clock_shapes_and_propagates_video_element_flash_warning() {
    let dir = tempfile::tempdir().unwrap();
    let mut core = CoreService::new(dir.path(), false);

    let flash_clip = repo_root().join("tests/fixtures/media/committed/wcag-flash-360p60.mp4");
    let reply::Result::MediaItem(flash_item) = unwrap_reply(core.handle(&ui_cmd(
        1,
        command::Action::ImportMedia(ImportMedia {
            source_path: flash_clip.to_string_lossy().into_owned(),
            name: "Element Flash Clip".into(),
            tags: vec![],
        }),
    ))) else {
        panic!("expected MediaItem");
    };
    assert!(flash_item.flash_warning);

    let doc = selah_render::slide_elements::SlideElementsDocument {
        elements: vec![
            selah_render::slide_elements::SlideElement {
                id: "card".into(),
                name: "Banner".into(),
                kind: selah_render::slide_elements::SlideElementKind::Shape,
                x: 0.08,
                y: 0.12,
                width: 0.84,
                height: 0.26,
                z_index: 1,
                shape_kind: selah_render::slide_elements::ShapeKind::RoundedRect,
                fill_color: [25, 95, 175, 255],
                ..Default::default()
            },
            selah_render::slide_elements::SlideElement {
                id: "clock".into(),
                name: "Clock".into(),
                kind: selah_render::slide_elements::SlideElementKind::Text,
                x: 0.10,
                y: 0.14,
                width: 0.80,
                height: 0.22,
                z_index: 2,
                text: "Start {clock}".into(),
                font_size: 46.0,
                weight: 700,
                color: [255, 255, 255, 255],
                ..Default::default()
            },
            selah_render::slide_elements::SlideElement {
                id: "vid".into(),
                name: "Strobe Video".into(),
                kind: selah_render::slide_elements::SlideElementKind::Video,
                x: 0.15,
                y: 0.45,
                width: 0.70,
                height: 0.45,
                z_index: 3,
                content_hash: flash_item.content_hash.clone(),
                media_extension: flash_item.extension.clone(),
                fit: "fill".into(),
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    let scene = Scene {
        revision: 3,
        outputs: vec![OutputScene {
            output_index: 0,
            resolution: Some(Resolution {
                width: 640,
                height: 360,
            }),
            layers: vec![LayerSpec {
                kind: LayerKind::Slide as i32,
                enabled: true,
                content: Some(layer_spec::Content::Slide(SlideLayerSpec {
                    elements_json: doc.to_json().unwrap(),
                    ..Default::default()
                })),
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    };

    let reply::Result::SceneThumbnail(t1) = unwrap_reply(core.handle(&ui_query(
        2,
        query::Request::RenderSceneThumbnail(RenderSceneThumbnail {
            scene: Some(scene.clone()),
            output_index: 0,
            max_width: 320,
            max_height: 180,
        }),
    ))) else {
        panic!("expected SceneThumbnail");
    };
    let reply::Result::SceneThumbnail(t2) = unwrap_reply(core.handle(&ui_query(
        3,
        query::Request::RenderSceneThumbnail(RenderSceneThumbnail {
            scene: Some(scene),
            output_index: 0,
            max_width: 320,
            max_height: 180,
        }),
    ))) else {
        panic!("expected SceneThumbnail");
    };

    assert!(
        t1.flash_warning,
        "video element flash_warning must propagate through SceneThumbnail (SAFE-04)"
    );
    assert_eq!(
        t1.png_bytes, t2.png_bytes,
        "thumbnail clock rendering must be deterministic across identical calls"
    );
}
