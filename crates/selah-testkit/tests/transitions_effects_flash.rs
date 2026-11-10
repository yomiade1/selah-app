//! M3.5 / M3-A8 / SAFE-03 / SAFE-04 / SEC-04 end-to-end process-stack tests:
//! built-in shader chain effects, invalid custom shader refusal keeping live output untouched,
//! timed GPU transitions emitting multiple intermediate frames, live WCAG 2.3.1 flash monitoring on
//! `wcag-flash-360p60.mp4`, and emergency clear cancelling an active transition + clearing flash warnings.
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use selah_core::library::MediaStore;
use selah_ipc::Service;
use selah_proto::v1::{
    ClearEverything, Color, Command, EffectKind, EffectSpec, LayerKind, LayerSpec, MediaLayerSpec,
    OutputConfig, OutputRole, OutputScene, PresentScene, Query, RenderCommand, Resolution, Scene,
    SetOutputConfigs, Subscribe, TransitionDirection, TransitionEasing, TransitionKind,
    TransitionSpec, command, envelope, event, layer_spec, query, render_command,
};
use selah_render::Compositor;
use selah_testkit::{Stack, StackOptions, eventually, freeze, test_actor};

fn color(r: f32, g: f32, b: f32, a: f32) -> Option<Color> {
    Some(Color {
        red: r,
        green: g,
        blue: b,
        alpha: a,
    })
}

fn solid_output_scene(
    r: f32,
    g: f32,
    b: f32,
    output_effects: Vec<EffectSpec>,
    transition: Option<TransitionSpec>,
) -> Scene {
    Scene {
        outputs: vec![OutputScene {
            output_index: 0,
            role: OutputRole::Main as i32,
            effects: output_effects,
            transition,
            layers: vec![LayerSpec {
                kind: LayerKind::Media as i32,
                enabled: true,
                content: Some(layer_spec::Content::Media(MediaLayerSpec {
                    solid_color: color(r, g, b, 1.0),
                    ..Default::default()
                })),
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn transitions_effects_flash_guard_and_emergency_cancel_end_to_end() {
    // safe: SAFE-03, SAFE-04, SEC-04
    let stack = Stack::start(StackOptions::default());
    stack.wait_ready(Duration::from_secs(30)).await;

    let media_root = stack.home().join("library").join("media");
    let store = MediaStore::open(&media_root).expect("open media store");
    let flash_clip_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/media/committed/wcag-flash-360p60.mp4");
    let flash_blob = store
        .import(&flash_clip_path)
        .expect("import wcag-flash clip");

    stack
        .core()
        .request(
            envelope::Body::Command(Command {
                action: Some(command::Action::SetOutputConfigs(SetOutputConfigs {
                    outputs: vec![OutputConfig {
                        output_index: 0,
                        name: "Main 360p".into(),
                        role: OutputRole::Main as i32,
                        resolution: Some(Resolution {
                            width: 640,
                            height: 360,
                        }),
                        enabled: true,
                        ..Default::default()
                    }],
                })),
                ..Default::default()
            }),
            Duration::from_secs(5),
        )
        .await
        .expect("SetOutputConfigs");

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
        .expect("subscribe core frames");

    // 1. Present plain solid blue scene and capture its baseline checksum.
    stack
        .core()
        .request(
            envelope::Body::Command(Command {
                action: Some(command::Action::PresentScene(PresentScene {
                    scene: Some(solid_output_scene(0.0, 0.25, 0.9, vec![], None)),
                })),
                ..Default::default()
            }),
            Duration::from_secs(5),
        )
        .await
        .expect("present blue scene");
    let blue_rev = stack.core_status().await.expect("status").scene_revision;
    let (blue_status, _) = eventually(Duration::from_secs(8), "blue baseline frame", || async {
        let st = stack.core_status().await?;
        (st.engine_scene_revision >= blue_rev && st.live && !st.frame_checksum.is_empty())
            .then_some(st)
    })
    .await;

    // 2. Custom WGSL shader in PresentScene is refused (SEL-IPC-0002), keeping live blue output untouched.
    let bad_reply = stack
        .core()
        .request(
            envelope::Body::Command(Command {
                action: Some(command::Action::PresentScene(PresentScene {
                    scene: Some(solid_output_scene(
                        1.0,
                        0.0,
                        0.0,
                        vec![EffectSpec {
                            kind: EffectKind::Blur as i32,
                            enabled: true,
                            intensity: 1.0,
                            params_json: r#"{"wgsl":"@fragment fn fs_main() {}"}"#.into(),
                        }],
                        None,
                    )),
                })),
                ..Default::default()
            }),
            Duration::from_secs(5),
        )
        .await
        .expect("custom shader reply");
    match bad_reply.body {
        Some(envelope::Body::Error(err)) => assert_eq!(err.code, "SEL-IPC-0002"),
        other => panic!("expected SEL-IPC-0002 error for custom shader, got {other:?}"),
    }
    let after_refusal = stack.core_status().await.expect("status after refusal");
    assert_eq!(after_refusal.frame_checksum, blue_status.frame_checksum);
    assert_eq!(after_refusal.scene_revision, blue_rev);

    // 3. Valid closed built-in effect chain renders and updates checksum cleanly.
    let effects_chain = vec![
        EffectSpec {
            kind: EffectKind::Blur as i32,
            enabled: true,
            intensity: 0.6,
            params_json: r#"{"sigma":2.0}"#.into(),
        },
        EffectSpec {
            kind: EffectKind::ColourAdjust as i32,
            enabled: true,
            intensity: 1.0,
            params_json: r#"{"brightness":0.1,"contrast":1.15,"saturation":1.1}"#.into(),
        },
        EffectSpec {
            kind: EffectKind::Vignette as i32,
            enabled: true,
            intensity: 0.85,
            params_json: r##"{"inner_radius":0.3,"softness":0.4,"color":"#000000"}"##.into(),
        },
        EffectSpec {
            kind: EffectKind::Mask as i32,
            enabled: true,
            intensity: 1.0,
            params_json: r#"{"shape":"rounded_rect","bounds":[0.05,0.05,0.9,0.9],"radius":0.06}"#
                .into(),
        },
    ];
    stack
        .core()
        .request(
            envelope::Body::Command(Command {
                action: Some(command::Action::PresentScene(PresentScene {
                    scene: Some(solid_output_scene(0.0, 0.25, 0.9, effects_chain, None)),
                })),
                ..Default::default()
            }),
            Duration::from_secs(5),
        )
        .await
        .expect("present effects scene");
    let fx_rev = stack.core_status().await.expect("status").scene_revision;
    let (fx_status, _) = eventually(Duration::from_secs(8), "effected frame", || async {
        let st = stack.core_status().await?;
        (st.engine_scene_revision >= fx_rev
            && !st.frame_checksum.is_empty()
            && st.frame_checksum != blue_status.frame_checksum)
            .then_some(st)
    })
    .await;
    assert_ne!(fx_status.frame_checksum, blue_status.frame_checksum);

    // 4. Timed Wipe transition (420 ms) emits multiple distinct intermediate FrameShown checksums.
    stack
        .core()
        .request(
            envelope::Body::Command(Command {
                action: Some(command::Action::PresentScene(PresentScene {
                    scene: Some(solid_output_scene(
                        0.1,
                        0.8,
                        0.3,
                        vec![],
                        Some(TransitionSpec {
                            kind: TransitionKind::Wipe as i32,
                            direction: TransitionDirection::Left as i32,
                            easing: TransitionEasing::Linear as i32,
                            duration_ms: 420,
                        }),
                    )),
                })),
                ..Default::default()
            }),
            Duration::from_secs(5),
        )
        .await
        .expect("present wipe transition");
    let wipe_rev = stack.core_status().await.expect("status").scene_revision;
    let mut wipe_checksums: HashSet<Vec<u8>> = HashSet::new();
    tokio::time::timeout(Duration::from_secs(8), async {
        while wipe_checksums.len() < 3 {
            let msg = core_events.recv().await.expect("core event");
            if let Some(envelope::Body::Event(selah_proto::v1::Event {
                notification: Some(event::Notification::FrameShown(frame)),
            })) = msg.body
                && frame.scene_revision == wipe_rev
                && frame.output_index == 0
            {
                wipe_checksums.insert(frame.frame_checksum);
            }
        }
    })
    .await
    .expect("timed Wipe transition must emit >= 3 distinct frame checksums");

    // 5. Real-time WCAG 2.3.1 flash analyser flags `wcag-flash-360p60.mp4` during live playback.
    let strobe_scene = Scene {
        outputs: vec![OutputScene {
            output_index: 0,
            role: OutputRole::Main as i32,
            layers: vec![LayerSpec {
                kind: LayerKind::Media as i32,
                enabled: true,
                content: Some(layer_spec::Content::Media(MediaLayerSpec {
                    content_hash: flash_blob.content_hash.clone(),
                    media_kind: "video".into(),
                    media_extension: "mp4".into(),
                    loop_media: true,
                    speed: 1.0,
                    muted: true,
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
                    scene: Some(strobe_scene),
                })),
                ..Default::default()
            }),
            Duration::from_secs(5),
        )
        .await
        .expect("present strobe video");
    let strobe_rev = stack.core_status().await.expect("status").scene_revision;

    let mut strobe_flagged_frame = false;
    tokio::time::timeout(Duration::from_secs(12), async {
        while !strobe_flagged_frame {
            let msg = core_events.recv().await.expect("core event");
            if let Some(envelope::Body::Event(selah_proto::v1::Event {
                notification: Some(event::Notification::FrameShown(frame)),
            })) = msg.body
                && frame.scene_revision >= strobe_rev
                && frame.output_index == 0
                && frame.flash_warning
            {
                strobe_flagged_frame = true;
            }
        }
    })
    .await
    .expect("live engine FlashAnalyzer must flag wcag-flash-360p60.mp4");

    let (flagged_status, _) = eventually(
        Duration::from_secs(5),
        "status flash_warning true",
        || async {
            let st = stack.core_status().await?;
            (st.flash_warning
                && st
                    .output_statuses
                    .iter()
                    .any(|o| o.output_index == 0 && o.flash_warning))
            .then_some(st)
        },
    )
    .await;
    assert!(flagged_status.flash_warning);

    // 6. SAFE-03: Start a 3000 ms Zoom transition, freeze core, fire Emergency ClearEverything ->
    // transition cancels immediately, screen clears within 1 software frame budget, flash_warning resets.
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
        .expect("subscribe engine frames");
    let lane = selah_ipc::Client::start(stack.endpoint(Service::Emergency), test_actor());
    lane.wait_connected(Duration::from_secs(5))
        .await
        .expect("emergency lane");

    stack
        .core()
        .request(
            envelope::Body::Command(Command {
                action: Some(command::Action::PresentScene(PresentScene {
                    scene: Some(solid_output_scene(
                        1.0,
                        0.0,
                        0.0,
                        vec![],
                        Some(TransitionSpec {
                            kind: TransitionKind::Zoom as i32,
                            direction: TransitionDirection::Right as i32,
                            easing: TransitionEasing::EaseInOut as i32,
                            duration_ms: 3000,
                        }),
                    )),
                })),
                ..Default::default()
            }),
            Duration::from_secs(5),
        )
        .await
        .expect("start slow transition");
    let slow_rev = stack.core_status().await.expect("status").scene_revision;
    let _ = eventually(
        Duration::from_secs(5),
        "slow transition started",
        || async {
            let st = stack.core_status().await?;
            (st.engine_scene_revision >= slow_rev).then_some(st)
        },
    )
    .await;

    let cleared_black = selah_render::cpu::CpuCompositor::default()
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

    let (cleared_frame, applied) = tokio::time::timeout(Duration::from_secs(5), async {
        let mut cleared = None;
        let mut emergency_ev = None;
        while cleared.is_none() || emergency_ev.is_none() {
            let msg = engine_events.recv().await.expect("engine event");
            match msg.body {
                Some(envelope::Body::Event(selah_proto::v1::Event {
                    notification: Some(event::Notification::EmergencyApplied(ev)),
                })) => {
                    emergency_ev = Some(ev);
                }
                Some(envelope::Body::Event(selah_proto::v1::Event {
                    notification: Some(event::Notification::FrameShown(frame)),
                })) if emergency_ev.is_some()
                    && frame.output_index == 0
                    && frame.frame_checksum == cleared_black =>
                {
                    cleared = Some(frame);
                }
                _ => {}
            }
        }
        (cleared.unwrap(), emergency_ev.unwrap())
    })
    .await
    .expect("emergency clear cancelled long transition cleanly");
    freeze::thaw(core_pid);

    assert!(!cleared_frame.flash_warning);
    assert!(applied.scene_revision >= slow_rev);
    let latency = cleared_frame
        .monotonic_nanoseconds
        .saturating_sub(cleared_frame.command_received_nanoseconds);
    let software_limit_ns = Duration::from_millis(150).as_nanos() as u64;
    assert!(
        latency <= software_limit_ns,
        "emergency clear took {:.2} ms (limit {:.2} ms)",
        latency as f64 / 1e6,
        software_limit_ns as f64 / 1e6
    );

    let (settled, _) = eventually(
        Duration::from_secs(5),
        "core resets live and flash_warning after thaw",
        || async {
            let st = stack.core_status().await?;
            (!st.live && !st.flash_warning).then_some(st)
        },
    )
    .await;
    assert!(!settled.flash_warning);

    stack.shutdown().await;
}
