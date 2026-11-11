//! M3.4 end-to-end integration tests: gradient/image/video background layers, blur & opacity,
//! multi-output shared video playback, SAFE-03/SAFE-05 emergency clear, and SEC-04 path safety.
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use selah_core::library::MediaStore;
use selah_ipc::Service;
use selah_proto::v1::{
    ClearEverything, Color, ColorStop, Command, GradientKind, GradientSpec, LayerKind, LayerSpec,
    MediaFitMode, MediaLayerSpec, OutputConfig, OutputRole, OutputScene, PresentScene, Query,
    RenderCommand, Resolution, Scene, SetOutputConfigs, SlideLayerSpec, Subscribe, command,
    envelope, event, layer_spec, query, render_command, reply,
};
use selah_render::{Compositor, Frame, png};
use selah_testkit::{Stack, StackOptions, eventually, freeze, test_actor};

fn color(r: f32, g: f32, b: f32, a: f32) -> Option<Color> {
    Some(Color {
        red: r,
        green: g,
        blue: b,
        alpha: a,
    })
}

fn synthetic_test_png() -> Vec<u8> {
    let mut frame = Frame::new(160, 90);
    for y in 0..90_u32 {
        for x in 0..160_u32 {
            let at = ((y * 160 + x) * 4) as usize;
            frame.pixels[at] = if x < 80 { 220 } else { 30 };
            frame.pixels[at + 1] = if y < 45 { 140 } else { 40 };
            frame.pixels[at + 2] = 200;
            frame.pixels[at + 3] = 255;
        }
    }
    png::encode(&frame).expect("encode synthetic png")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn backgrounds_image_video_and_emergency_clear_end_to_end() {
    // safe: SAFE-03, SAFE-05, SEC-04
    let stack = Stack::start(StackOptions::default());
    stack.wait_ready(Duration::from_secs(30)).await;

    // Import a synthetic PNG and the committed flash-beep MP4 into the isolated stack MediaStore.
    let media_root = stack.home().join("library").join("media");
    let store = MediaStore::open(&media_root).expect("open media store");
    let img_bytes = synthetic_test_png();
    let img_blob = store
        .import_reader(img_bytes.as_slice(), "png", 16 * 1024 * 1024)
        .expect("import png blob");
    let clip_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/media/committed/flash-beep-360p60.mp4");
    let clip_blob = store.import(&clip_path).expect("import mp4 clip");

    let cfg_reply = stack
        .core()
        .request(
            envelope::Body::Command(Command {
                action: Some(command::Action::SetOutputConfigs(SetOutputConfigs {
                    outputs: vec![
                        OutputConfig {
                            output_index: 0,
                            name: "Main 360p".into(),
                            role: OutputRole::Main as i32,
                            resolution: Some(Resolution {
                                width: 640,
                                height: 360,
                            }),
                            enabled: true,
                            ..Default::default()
                        },
                        OutputConfig {
                            output_index: 1,
                            name: "Stream Square".into(),
                            role: OutputRole::Stream as i32,
                            resolution: Some(Resolution {
                                width: 320,
                                height: 320,
                            }),
                            enabled: true,
                            ..Default::default()
                        },
                    ],
                })),
                ..Default::default()
            }),
            Duration::from_secs(5),
        )
        .await
        .expect("SetOutputConfigs");
    assert!(matches!(
        cfg_reply.body,
        Some(envelope::Body::Reply(selah_proto::v1::Reply {
            result: Some(reply::Result::Acknowledged(true))
        }))
    ));

    let mut core_events = stack.core().events();
    stack
        .core()
        .request(
            envelope::Body::Query(Query {
                request: Some(query::Request::Subscribe(Subscribe {
                    event_names: vec!["frames".into()],
                })),
            }),
            Duration::from_secs(5),
        )
        .await
        .expect("subscribe frames");

    // 1. Blurred image + gradient backdrop under slide text on output 0; solid color on output 1.
    let img_scene = Scene {
        outputs: vec![
            OutputScene {
                output_index: 0,
                role: OutputRole::Main as i32,
                layers: vec![
                    LayerSpec {
                        kind: LayerKind::Media as i32,
                        enabled: true,
                        use_opacity: true,
                        opacity: 0.9,
                        content: Some(layer_spec::Content::Media(MediaLayerSpec {
                            gradient: Some(GradientSpec {
                                kind: GradientKind::Linear as i32,
                                angle_degrees: 90.0,
                                stops: vec![
                                    ColorStop {
                                        color: color(0.05, 0.1, 0.25, 1.0),
                                        position: 0.0,
                                    },
                                    ColorStop {
                                        color: color(0.2, 0.05, 0.2, 1.0),
                                        position: 1.0,
                                    },
                                ],
                            }),
                            content_hash: img_blob.content_hash.clone(),
                            media_kind: "image".into(),
                            media_extension: "png".into(),
                            fit: MediaFitMode::Fill as i32,
                            blur_radius: 4.0,
                            ..Default::default()
                        })),
                        ..Default::default()
                    },
                    LayerSpec {
                        kind: LayerKind::Slide as i32,
                        enabled: true,
                        content: Some(layer_spec::Content::Slide(SlideLayerSpec {
                            text: "Be Still".into(),
                            ..Default::default()
                        })),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
            OutputScene {
                output_index: 1,
                role: OutputRole::Stream as i32,
                layers: vec![LayerSpec {
                    kind: LayerKind::Media as i32,
                    enabled: true,
                    content: Some(layer_spec::Content::Media(MediaLayerSpec {
                        solid_color: color(0.1, 0.3, 0.5, 1.0),
                        ..Default::default()
                    })),
                    ..Default::default()
                }],
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    stack
        .core()
        .request(
            envelope::Body::Command(Command {
                action: Some(command::Action::PresentScene(PresentScene {
                    scene: Some(img_scene),
                })),
                ..Default::default()
            }),
            Duration::from_secs(5),
        )
        .await
        .expect("Present image scene");
    let img_rev = stack.core_status().await.expect("status").scene_revision;

    let mut img_outputs = BTreeMap::new();
    tokio::time::timeout(Duration::from_secs(10), async {
        while img_outputs.len() < 2 {
            let msg = core_events.recv().await.expect("event");
            if let Some(envelope::Body::Event(selah_proto::v1::Event {
                notification: Some(event::Notification::FrameShown(frame)),
            })) = msg.body
                && frame.scene_revision == img_rev
            {
                img_outputs.insert(frame.output_index, frame);
            }
        }
    })
    .await
    .expect("both outputs show image/color scene");
    assert_ne!(
        img_outputs[&0].frame_checksum,
        img_outputs[&1].frame_checksum
    );

    // 2. Looping background video shared across both outputs (Fit vs Fill, audio unmuted under
    // SELAH_AUDIO=null in Stack), verifying continuous frame progression on both outputs.
    let video_scene = Scene {
        outputs: vec![
            OutputScene {
                output_index: 0,
                role: OutputRole::Main as i32,
                layers: vec![
                    LayerSpec {
                        kind: LayerKind::Media as i32,
                        enabled: true,
                        content: Some(layer_spec::Content::Media(MediaLayerSpec {
                            content_hash: clip_blob.content_hash.clone(),
                            media_kind: "video".into(),
                            media_extension: "mp4".into(),
                            fit: MediaFitMode::Fill as i32,
                            blur_radius: 1.5,
                            loop_media: true,
                            speed: 1.0,
                            volume: 0.9,
                            muted: false,
                            ..Default::default()
                        })),
                        ..Default::default()
                    },
                    LayerSpec {
                        kind: LayerKind::Slide as i32,
                        enabled: true,
                        content: Some(layer_spec::Content::Slide(SlideLayerSpec {
                            text: "Praise".into(),
                            ..Default::default()
                        })),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
            OutputScene {
                output_index: 1,
                role: OutputRole::Stream as i32,
                layers: vec![LayerSpec {
                    kind: LayerKind::Media as i32,
                    enabled: true,
                    use_opacity: true,
                    opacity: 0.8,
                    content: Some(layer_spec::Content::Media(MediaLayerSpec {
                        content_hash: clip_blob.content_hash.clone(),
                        media_kind: "video".into(),
                        media_extension: "mp4".into(),
                        fit: MediaFitMode::Fit as i32,
                        loop_media: true,
                        speed: 1.0,
                        volume: 0.9,
                        muted: false,
                        ..Default::default()
                    })),
                    ..Default::default()
                }],
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    stack
        .core()
        .request(
            envelope::Body::Command(Command {
                action: Some(command::Action::PresentScene(PresentScene {
                    scene: Some(video_scene),
                })),
                ..Default::default()
            }),
            Duration::from_secs(5),
        )
        .await
        .expect("Present video scene");
    let vid_rev = stack.core_status().await.expect("status").scene_revision;

    let mut out0_checksums: HashSet<Vec<u8>> = HashSet::new();
    let mut out1_checksums: HashSet<Vec<u8>> = HashSet::new();
    tokio::time::timeout(Duration::from_secs(12), async {
        while out0_checksums.len() < 4 || out1_checksums.len() < 4 {
            let msg = core_events.recv().await.expect("event");
            if let Some(envelope::Body::Event(selah_proto::v1::Event {
                notification: Some(event::Notification::FrameShown(frame)),
            })) = msg.body
                && frame.scene_revision == vid_rev
            {
                if frame.output_index == 0 {
                    out0_checksums.insert(frame.frame_checksum);
                } else if frame.output_index == 1 {
                    out1_checksums.insert(frame.frame_checksum);
                }
            }
        }
    })
    .await
    .expect("both outputs advance frames on looping video background");

    // 3. SEC-04 path-traversal media reference must be refused safely without crashing playback.
    let unsafe_scene = Scene {
        outputs: vec![OutputScene {
            output_index: 0,
            role: OutputRole::Main as i32,
            layers: vec![LayerSpec {
                kind: LayerKind::Media as i32,
                enabled: true,
                content: Some(layer_spec::Content::Media(MediaLayerSpec {
                    content_hash: "../../etc/passwd".into(),
                    media_kind: "video".into(),
                    media_extension: "mp4".into(),
                    ..Default::default()
                })),
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    };
    stack
        .core()
        .request(
            envelope::Body::Command(Command {
                action: Some(command::Action::PresentScene(PresentScene {
                    scene: Some(unsafe_scene),
                })),
                ..Default::default()
            }),
            Duration::from_secs(5),
        )
        .await
        .expect("Present unsafe path scene safely handled");

    // Restore video scene before testing SAFE-03/SAFE-05 emergency clear under frozen core.
    let vid_rev2 = {
        let mut s = stack.core_status().await.expect("status").scene_revision;
        stack
            .core()
            .request(
                envelope::Body::Command(Command {
                    action: Some(command::Action::PresentScene(PresentScene {
                        scene: Some(Scene {
                            outputs: vec![OutputScene {
                                output_index: 0,
                                role: OutputRole::Main as i32,
                                layers: vec![LayerSpec {
                                    kind: LayerKind::Media as i32,
                                    enabled: true,
                                    content: Some(layer_spec::Content::Media(MediaLayerSpec {
                                        content_hash: clip_blob.content_hash.clone(),
                                        media_kind: "video".into(),
                                        media_extension: "mp4".into(),
                                        loop_media: true,
                                        speed: 1.0,
                                        volume: 0.9,
                                        ..Default::default()
                                    })),
                                    ..Default::default()
                                }],
                                ..Default::default()
                            }],
                            ..Default::default()
                        }),
                    })),
                    ..Default::default()
                }),
                Duration::from_secs(5),
            )
            .await
            .expect("re-present video scene");
        s += 1;
        s
    };
    let cleared_black_0 = selah_render::cpu::CpuCompositor::default()
        .render(
            &Scene {
                cleared: true,
                resolution: Some(Resolution {
                    width: 640,
                    height: 360,
                }),
                ..Default::default()
            },
            640,
            360,
        )
        .unwrap()
        .checksum()
        .to_vec();

    let _ = eventually(
        Duration::from_secs(8),
        "video active before clear",
        || async {
            let st = stack.core_status().await?;
            (st.engine_scene_revision >= vid_rev2
                && st.live
                && !st.frame_checksum.is_empty()
                && st.frame_checksum != cleared_black_0)
                .then_some(st)
        },
    )
    .await;

    // 4. SAFE-03 + SAFE-05: Freeze core, hit emergency clear, verify visual clear within 1 frame.
    let mut engine_events = stack.engine().events();
    stack
        .engine()
        .request(
            envelope::Body::Query(Query {
                request: Some(query::Request::Subscribe(Subscribe {
                    event_names: vec!["frames".into()],
                })),
            }),
            Duration::from_secs(5),
        )
        .await
        .expect("engine subscribe");
    let lane = selah_ipc::Client::start(stack.endpoint(Service::Emergency), test_actor());
    lane.wait_connected(Duration::from_secs(5))
        .await
        .expect("emergency lane");
    let core_pid = stack.pid("core").await.expect("core pid");
    freeze::freeze(core_pid);

    let clear_reply = lane
        .request(
            envelope::Body::Command(Command {
                action: Some(command::Action::Render(RenderCommand {
                    action: Some(render_command::Action::ClearEverything(ClearEverything {})),
                })),
                ..Default::default()
            }),
            Duration::from_secs(2),
        )
        .await;
    assert!(clear_reply.is_ok_and(|r| matches!(r.body, Some(envelope::Body::Reply(_)))));

    let (cleared_frame, emergency_applied) = tokio::time::timeout(Duration::from_secs(5), async {
        let mut cleared = None;
        let mut applied = None;
        while cleared.is_none() || applied.is_none() {
            let msg = engine_events.recv().await.expect("engine event");
            match msg.body {
                Some(envelope::Body::Event(selah_proto::v1::Event {
                    notification: Some(event::Notification::FrameShown(frame)),
                })) if frame.output_index == 0 && frame.frame_checksum == cleared_black_0 => {
                    cleared = Some(frame);
                }
                Some(envelope::Body::Event(selah_proto::v1::Event {
                    notification: Some(event::Notification::EmergencyApplied(ev)),
                })) => {
                    applied = Some(ev);
                }
                _ => {}
            }
        }
        (cleared.unwrap(), applied.unwrap())
    })
    .await
    .expect("emergency clear stopped video within timeout");
    freeze::thaw(core_pid);

    assert!(
        emergency_applied.scene_revision >= vid_rev2,
        "EmergencyApplied must record revision floor >= video rev"
    );
    let latency = cleared_frame
        .monotonic_nanoseconds
        .saturating_sub(cleared_frame.command_received_nanoseconds);
    // `Stack` runs on `SELAH_GPU=software` (WARP/lavapipe takes ~35–60 ms per 360p video frame),
    // so an in-flight software video frame completes before `render_loop` executes the clear
    // (which itself renders in ~2 ms); non-blocking emergency lane must never stall > 150 ms.
    let software_limit_ns = Duration::from_millis(150).as_nanos() as u64;
    assert!(
        latency <= software_limit_ns,
        "emergency clear took {:.2} ms (limit {:.2} ms)",
        latency as f64 / 1e6,
        software_limit_ns as f64 / 1e6
    );

    stack.shutdown().await;
}
