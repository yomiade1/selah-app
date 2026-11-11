//! M3.1 end-to-end integration tests: multi-output configuration, canonical 5-layer IPC scenes,
//! staged preview isolation, preload hints, and SAFE-03 emergency clear across every output.
use std::collections::BTreeMap;
use std::time::Duration;

use selah_ipc::Service;
use selah_proto::v1::{
    ClearEverything, Color, Command, GetOutputConfigs, LayerKind, LayerSpec, MediaLayerSpec,
    MessagesLayerSpec, OutputConfig, OutputRole, OutputScene, PreloadHint, PreloadItems,
    PresentScene, PropsLayerSpec, Query, RenderCommand, Resolution, Scene, SetOutputConfigs,
    SlideLayerSpec, StagePreviewScene, Subscribe, VideoInputLayerSpec, command, envelope, event,
    layer_spec, query, render_command, reply,
};
use selah_testkit::{Stack, StackOptions, eventually, freeze, test_actor};

fn color(r: f32, g: f32, b: f32, a: f32) -> Option<Color> {
    Some(Color {
        red: r,
        green: g,
        blue: b,
        alpha: a,
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn multi_output_layer_scenes_present_independently_and_clear_all_on_emergency() {
    // safe: SAFE-03, SAFE-06
    let stack = Stack::start(StackOptions::default());
    stack.wait_ready(Duration::from_secs(30)).await;

    let configs = vec![
        OutputConfig {
            output_index: 0,
            name: "Main 1080p".into(),
            role: OutputRole::Main as i32,
            resolution: Some(Resolution {
                width: 1920,
                height: 1080,
            }),
            enabled: true,
            ..Default::default()
        },
        OutputConfig {
            output_index: 1,
            name: "Ultra-Wide LED".into(),
            role: OutputRole::Main as i32,
            resolution: Some(Resolution {
                width: 3840,
                height: 1080,
            }),
            enabled: true,
            ..Default::default()
        },
        OutputConfig {
            output_index: 2,
            name: "Stage Display".into(),
            role: OutputRole::Stage as i32,
            resolution: Some(Resolution {
                width: 1280,
                height: 720,
            }),
            enabled: true,
            ..Default::default()
        },
    ];

    let cfg_reply = stack
        .core()
        .request(
            envelope::Body::Command(Command {
                action: Some(command::Action::SetOutputConfigs(SetOutputConfigs {
                    outputs: configs.clone(),
                })),
                ..Default::default()
            }),
            Duration::from_secs(5),
        )
        .await
        .expect("SetOutputConfigs reply");
    assert!(matches!(
        cfg_reply.body,
        Some(envelope::Body::Reply(selah_proto::v1::Reply {
            result: Some(reply::Result::Acknowledged(true))
        }))
    ));

    let fetched = stack
        .core()
        .request(
            envelope::Body::Query(Query {
                request: Some(query::Request::GetOutputConfigs(GetOutputConfigs {})),
            }),
            Duration::from_secs(5),
        )
        .await
        .expect("GetOutputConfigs reply");
    let Some(envelope::Body::Reply(selah_proto::v1::Reply {
        result: Some(reply::Result::OutputConfigList(list)),
    })) = fetched.body
    else {
        panic!("expected OutputConfigList reply");
    };
    assert_eq!(list.outputs.len(), 3);
    assert_eq!(list.outputs[1].name, "Ultra-Wide LED");

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
        .expect("core subscribe");

    // Stage a preview scene before going live: it must NEVER mark rooms live, and must write to
    // the isolated stage-0.ring without touching live preview-0.ring (SAFE-06).
    let stage_reply = stack
        .core()
        .request(
            envelope::Body::Command(Command {
                action: Some(command::Action::StagePreviewScene(StagePreviewScene {
                    scene: Some(Scene {
                        outputs: vec![OutputScene {
                            output_index: 0,
                            layers: vec![LayerSpec {
                                kind: LayerKind::Slide as i32,
                                enabled: true,
                                content: Some(layer_spec::Content::Slide(SlideLayerSpec {
                                    text: "Preview Slide Only".into(),
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
        .expect("StagePreviewScene reply");
    assert!(matches!(
        stage_reply.body,
        Some(envelope::Body::Reply(selah_proto::v1::Reply {
            result: Some(reply::Result::Acknowledged(true))
        }))
    ));

    let preview_frame = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let message = core_events.recv().await.expect("core event");
            if let Some(envelope::Body::Event(selah_proto::v1::Event {
                notification: Some(event::Notification::FrameShown(frame)),
            })) = message.body
                && frame.is_preview
                && frame.scene_revision == 1
            {
                return frame;
            }
        }
    })
    .await
    .expect("FrameShown { is_preview: true } for staged preview");
    assert_eq!(preview_frame.output_index, 0);
    assert!(!preview_frame.flash_warning);
    assert_eq!(
        preview_frame.resolution,
        Some(Resolution {
            width: 480,
            height: 270
        })
    );

    let ring_dir = selah_ipc::runtime_dir().expect("runtime dir");
    let stage_ring_path = ring_dir.join(format!("{}-stage-0.ring", stack.namespace));
    let live_ring_path = ring_dir.join(format!("{}-preview-0.ring", stack.namespace));
    let staged_px = selah_ipc::shm::RingReader::open(&stage_ring_path)
        .expect("open stage-0.ring")
        .latest()
        .expect("staged preview ring has frame");
    assert_eq!((staged_px.width, staged_px.height), (480, 270));
    assert!(
        staged_px
            .pixels
            .chunks_exact(4)
            .any(|bgra| bgra[0] > 0 || bgra[1] > 0 || bgra[2] > 0),
        "staged preview ring must contain rendered text pixels"
    );
    if let Ok(live_ring) = selah_ipc::shm::RingReader::open(&live_ring_path)
        && let Some(live_px) = live_ring.latest()
    {
        assert!(
            live_px
                .pixels
                .chunks_exact(4)
                .all(|bgra| bgra[0] == 0 && bgra[1] == 0 && bgra[2] == 0),
            "live preview-0.ring must stay cleared while only staged preview changed"
        );
    }

    let preview_status = stack.core_status().await.expect("status after preview");
    assert!(!preview_status.live);
    assert_eq!(preview_status.preview_scene_revision, 1);
    assert_eq!(preview_status.engine_scene_revision, 0);

    // Warm caches via PreloadItems.
    let preload_reply = stack
        .core()
        .request(
            envelope::Body::Command(Command {
                action: Some(command::Action::PreloadItems(PreloadItems {
                    items: vec![PreloadHint {
                        key: "next-verse".into(),
                        output_index: 0,
                        resolution: Some(Resolution {
                            width: 1920,
                            height: 1080,
                        }),
                        text: "Holy, holy, holy".into(),
                        ..Default::default()
                    }],
                })),
                ..Default::default()
            }),
            Duration::from_secs(5),
        )
        .await
        .expect("PreloadItems reply");
    assert!(matches!(
        preload_reply.body,
        Some(envelope::Body::Reply(selah_proto::v1::Reply {
            result: Some(reply::Result::Acknowledged(true))
        }))
    ));

    // Present 3 distinct scenes with canonical layers (including scrambled order on output 0).
    let scene = Scene {
        outputs: vec![
            OutputScene {
                output_index: 0,
                role: OutputRole::Main as i32,
                layers: vec![
                    LayerSpec {
                        kind: LayerKind::Messages as i32,
                        enabled: true,
                        content: Some(layer_spec::Content::Messages(MessagesLayerSpec {
                            text: "Car 42 lights on".into(),
                            ..Default::default()
                        })),
                        ..Default::default()
                    },
                    LayerSpec {
                        kind: LayerKind::Props as i32,
                        enabled: true,
                        content: Some(layer_spec::Content::Props(PropsLayerSpec {
                            label: "SANCTUARY".into(),
                            ..Default::default()
                        })),
                        ..Default::default()
                    },
                    LayerSpec {
                        kind: LayerKind::Slide as i32,
                        enabled: true,
                        content: Some(layer_spec::Content::Slide(SlideLayerSpec {
                            text: "Holy, holy, holy".into(),
                            ..Default::default()
                        })),
                        ..Default::default()
                    },
                    LayerSpec {
                        kind: LayerKind::Media as i32,
                        enabled: true,
                        use_opacity: true,
                        opacity: 0.6,
                        content: Some(layer_spec::Content::Media(MediaLayerSpec {
                            solid_color: color(0.05, 0.15, 0.4, 1.0),
                            ..Default::default()
                        })),
                        ..Default::default()
                    },
                    LayerSpec {
                        kind: LayerKind::VideoInput as i32,
                        enabled: true,
                        content: Some(layer_spec::Content::VideoInput(VideoInputLayerSpec {
                            source_id: "cam-a".into(),
                            fallback_color: color(0.0, 0.0, 0.2, 1.0),
                            ..Default::default()
                        })),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
            OutputScene {
                output_index: 1,
                role: OutputRole::Main as i32,
                layers: vec![LayerSpec {
                    kind: LayerKind::Slide as i32,
                    enabled: true,
                    content: Some(layer_spec::Content::Slide(SlideLayerSpec {
                        text: "LED Ultra Wide".into(),
                        ..Default::default()
                    })),
                    ..Default::default()
                }],
                ..Default::default()
            },
            OutputScene {
                output_index: 2,
                role: OutputRole::Stage as i32,
                layers: vec![LayerSpec {
                    kind: LayerKind::Slide as i32,
                    enabled: true,
                    content: Some(layer_spec::Content::Slide(SlideLayerSpec {
                        text: "Stage Confidence".into(),
                        ..Default::default()
                    })),
                    ..Default::default()
                }],
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    let present_reply = stack
        .core()
        .request(
            envelope::Body::Command(Command {
                action: Some(command::Action::PresentScene(PresentScene {
                    scene: Some(scene),
                })),
                ..Default::default()
            }),
            Duration::from_secs(5),
        )
        .await
        .expect("PresentScene reply");
    assert!(matches!(
        present_reply.body,
        Some(envelope::Body::Reply(selah_proto::v1::Reply {
            result: Some(reply::Result::Acknowledged(true))
        }))
    ));
    let target_rev = stack.core_status().await.unwrap().scene_revision;

    let mut shown_by_output = BTreeMap::new();
    tokio::time::timeout(Duration::from_secs(15), async {
        while shown_by_output.len() < 3 {
            let message = core_events.recv().await.expect("core event");
            if let Some(envelope::Body::Event(selah_proto::v1::Event {
                notification: Some(event::Notification::FrameShown(frame)),
            })) = message.body
                && !frame.is_preview
                && frame.scene_revision == target_rev
            {
                shown_by_output.insert(frame.output_index, frame);
            }
        }
    })
    .await
    .expect("FrameShown for all 3 outputs");

    assert_eq!(
        shown_by_output[&0].resolution,
        Some(Resolution {
            width: 1920,
            height: 1080
        })
    );
    assert_eq!(
        shown_by_output[&1].resolution,
        Some(Resolution {
            width: 3840,
            height: 1080
        })
    );
    assert_eq!(
        shown_by_output[&2].resolution,
        Some(Resolution {
            width: 1280,
            height: 720
        })
    );
    assert_ne!(
        shown_by_output[&0].frame_checksum,
        shown_by_output[&1].frame_checksum
    );
    assert_ne!(
        shown_by_output[&1].frame_checksum,
        shown_by_output[&2].frame_checksum
    );

    let (live_status, _) = eventually(Duration::from_secs(5), "all 3 output statuses", || async {
        let s = stack.core_status().await?;
        (s.live && s.output_statuses.len() == 3 && s.output_statuses.iter().all(|o| o.live))
            .then_some(s)
    })
    .await;
    assert_eq!(
        live_status.frame_checksum,
        shown_by_output[&0].frame_checksum
    );

    // Stage a second preview while live is active: stage-0.ring updates while live output 0 keeps its checksum.
    stack
        .core()
        .request(
            envelope::Body::Command(Command {
                action: Some(command::Action::StagePreviewScene(StagePreviewScene {
                    scene: Some(Scene {
                        outputs: vec![OutputScene {
                            output_index: 0,
                            layers: vec![LayerSpec {
                                kind: LayerKind::Slide as i32,
                                enabled: true,
                                content: Some(layer_spec::Content::Slide(SlideLayerSpec {
                                    text: "Next Song Item".into(),
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
        .expect("second StagePreviewScene reply");
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let message = core_events.recv().await.expect("core event");
            if let Some(envelope::Body::Event(selah_proto::v1::Event {
                notification: Some(event::Notification::FrameShown(frame)),
            })) = message.body
                && frame.is_preview
                && frame.scene_revision == 2
            {
                break;
            }
        }
    })
    .await
    .expect("second staged preview FrameShown");
    let staged_px2 = selah_ipc::shm::RingReader::open(&stage_ring_path)
        .expect("reopen stage-0.ring")
        .latest()
        .expect("second staged preview ring frame");
    assert_ne!(staged_px2.pixels, staged_px.pixels);
    let live_after_stage = stack.core_status().await.expect("live status after stage");
    assert!(live_after_stage.live);
    assert_eq!(
        live_after_stage.frame_checksum,
        shown_by_output[&0].frame_checksum
    );
    assert_eq!(live_after_stage.engine_scene_revision, target_rev);
    assert_eq!(live_after_stage.preview_scene_revision, 2);

    // SAFE-03: Freeze core, hit emergency ClearEverything, and verify EVERY enabled output clears!
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
    assert!(stack.core_status().await.is_none());

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

    // Wait for all 3 cleared frames (different checksums from their live frames).
    let mut cleared_outputs = BTreeMap::new();
    tokio::time::timeout(Duration::from_secs(10), async {
        while cleared_outputs.len() < 3 {
            let message = engine_events.recv().await.expect("engine event");
            if let Some(envelope::Body::Event(selah_proto::v1::Event {
                notification: Some(event::Notification::FrameShown(frame)),
            })) = message.body
                && !frame.is_preview
                && frame.frame_checksum != shown_by_output[&frame.output_index].frame_checksum
            {
                cleared_outputs.insert(frame.output_index, frame);
            }
        }
    })
    .await
    .expect("cleared frames for all 3 outputs while core was frozen");

    freeze::thaw(core_pid);

    let (cleared_status, _) = eventually(Duration::from_secs(5), "cleared core status", || async {
        let s = stack.core_status().await?;
        (!s.live && s.output_statuses.len() == 3 && s.output_statuses.iter().all(|o| !o.live))
            .then_some(s)
    })
    .await;
    assert!(!cleared_status.live);
    assert!(cleared_status.scene_revision > target_rev);

    stack.shutdown().await;
}
