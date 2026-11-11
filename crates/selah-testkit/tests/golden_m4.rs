//! M4-A3 (and M4-A7 in M4.6): golden-frame suite verifying that each screen shows the right
//! layers and per-output template overrides for each look (`Worship`, `Sermon`, `Camera` across
//! Main, Stream, and Lobby outputs).
//! Run with `cargo test -p selah-testkit --features golden --test golden_m4`.
#![cfg(feature = "golden")]

use selah_core::library::new_id;
use selah_core::service::CoreService;
use selah_proto::v1::{
    Actor, ActorOrigin, Color, ColorStop, Command, Envelope, GradientKind, GradientSpec, LayerKind,
    LayerSpec, Look, LookRule, MediaLayerSpec, OutputConfig, OutputRole, Prop, Resolution,
    SaveLook, SaveProp, SaveTemplate, Scene, SetLayerState, SetLook, SetOutputConfigs, ShowMessage,
    ShowProp, SlideLayerSpec, Template, TemplateVariant, VideoInputLayerSpec, command, envelope,
    layer_spec,
};
use selah_render::Compositor;
use selah_render::cpu::CpuCompositor;
use selah_render::gpu::{AdapterChoice, GpuCompositor};
use selah_render::template::TemplateStyle;
use selah_testkit::golden::{self, Tolerance};

const TOLERANCE_STYLE: Tolerance = Tolerance {
    channel: 10,
    max_fraction: 0.02,
};

fn c(red: f32, green: f32, blue: f32, alpha: f32) -> Option<Color> {
    Some(Color {
        red,
        green,
        blue,
        alpha,
    })
}

fn send_cmd(core: &mut CoreService, action: command::Action) {
    static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let req = Envelope {
        id: NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        correlation_id: 0,
        actor: Some(Actor {
            user_id: "operator".into(),
            device_id: "desk".into(),
            origin: ActorOrigin::Ui as i32,
        }),
        body: Some(envelope::Body::Command(Command {
            undo_group: None,
            action: Some(action),
        })),
    };
    let reply = core.handle(&req).reply.expect("command reply");
    if let Some(envelope::Body::Error(err)) = reply.body {
        panic!(
            "unexpected command error: {} ({})",
            err.code, err.message_key
        );
    }
}

struct LookGoldenHarness {
    _dir: tempfile::TempDir,
    core: CoreService,
    worship_look_id: selah_proto::v1::Id,
    sermon_look_id: selah_proto::v1::Id,
    camera_look_id: selah_proto::v1::Id,
}

fn setup_look_harness(w: u32, h: u32) -> LookGoldenHarness {
    let dir = tempfile::tempdir().unwrap();
    let mut core = CoreService::new(dir.path(), false);

    send_cmd(
        &mut core,
        command::Action::SetOutputConfigs(SetOutputConfigs {
            outputs: vec![
                OutputConfig {
                    output_index: 0,
                    name: "Main Hall".into(),
                    role: OutputRole::Main as i32,
                    resolution: Some(Resolution {
                        width: w,
                        height: h,
                    }),
                    enabled: true,
                    ..Default::default()
                },
                OutputConfig {
                    output_index: 1,
                    name: "Broadcast Stream".into(),
                    role: OutputRole::Stream as i32,
                    resolution: Some(Resolution {
                        width: w,
                        height: h,
                    }),
                    enabled: true,
                    ..Default::default()
                },
                OutputConfig {
                    output_index: 2,
                    name: "Lobby Display".into(),
                    role: OutputRole::Lobby as i32,
                    resolution: Some(Resolution {
                        width: w,
                        height: h,
                    }),
                    enabled: true,
                    ..Default::default()
                },
            ],
        }),
    );

    let lower_third_id = new_id();
    let lower_third_json = TemplateStyle::preset_lower_third().to_json().unwrap();
    send_cmd(
        &mut core,
        command::Action::SaveTemplate(SaveTemplate {
            template: Some(Template {
                id: Some(lower_third_id.clone()),
                name: "Stream Lower Third".into(),
                variants: vec![TemplateVariant {
                    id: Some(new_id()),
                    resolution: Some(Resolution {
                        width: w,
                        height: h,
                    }),
                    max_lines: 2,
                    style_json: lower_third_json,
                }],
            }),
        }),
    );

    let worship_look_id = new_id();
    send_cmd(
        &mut core,
        command::Action::SaveLook(SaveLook {
            look: Some(Look {
                id: Some(worship_look_id.clone()),
                name: "Worship".into(),
                hotkey: "Shift+1".into(),
                rules: vec![
                    LookRule {
                        id: Some(new_id()),
                        output_index: 0,
                        enabled_layers: vec![
                            LayerKind::Media as i32,
                            LayerKind::Slide as i32,
                            LayerKind::Props as i32,
                            LayerKind::Messages as i32,
                        ],
                        ..Default::default()
                    },
                    LookRule {
                        id: Some(new_id()),
                        output_index: 1,
                        enabled_layers: vec![
                            LayerKind::VideoInput as i32,
                            LayerKind::Slide as i32,
                            LayerKind::Props as i32,
                        ],
                        template_override_id: Some(lower_third_id.clone()),
                        ..Default::default()
                    },
                    LookRule {
                        id: Some(new_id()),
                        output_index: 2,
                        enabled_layers: vec![
                            LayerKind::Media as i32,
                            LayerKind::Slide as i32,
                            LayerKind::Messages as i32,
                        ],
                        ..Default::default()
                    },
                ],
            }),
        }),
    );

    let sermon_look_id = new_id();
    send_cmd(
        &mut core,
        command::Action::SaveLook(SaveLook {
            look: Some(Look {
                id: Some(sermon_look_id.clone()),
                name: "Sermon".into(),
                hotkey: "Shift+2".into(),
                rules: vec![
                    LookRule {
                        id: Some(new_id()),
                        output_index: 0,
                        enabled_layers: vec![
                            LayerKind::Media as i32,
                            LayerKind::Slide as i32,
                            LayerKind::Props as i32,
                        ],
                        ..Default::default()
                    },
                    LookRule {
                        id: Some(new_id()),
                        output_index: 1,
                        enabled_layers: vec![LayerKind::VideoInput as i32, LayerKind::Slide as i32],
                        template_override_id: Some(lower_third_id),
                        ..Default::default()
                    },
                    LookRule {
                        id: Some(new_id()),
                        output_index: 2,
                        enabled_layers: vec![
                            LayerKind::VideoInput as i32,
                            LayerKind::Messages as i32,
                        ],
                        ..Default::default()
                    },
                ],
            }),
        }),
    );

    let camera_look_id = new_id();
    send_cmd(
        &mut core,
        command::Action::SaveLook(SaveLook {
            look: Some(Look {
                id: Some(camera_look_id.clone()),
                name: "Camera".into(),
                hotkey: "Shift+3".into(),
                rules: vec![
                    LookRule {
                        id: Some(new_id()),
                        output_index: 0,
                        enabled_layers: vec![LayerKind::VideoInput as i32],
                        ..Default::default()
                    },
                    LookRule {
                        id: Some(new_id()),
                        output_index: 1,
                        enabled_layers: vec![LayerKind::VideoInput as i32, LayerKind::Props as i32],
                        ..Default::default()
                    },
                    LookRule {
                        id: Some(new_id()),
                        output_index: 2,
                        enabled_layers: vec![
                            LayerKind::VideoInput as i32,
                            LayerKind::Messages as i32,
                        ],
                        ..Default::default()
                    },
                ],
            }),
        }),
    );

    // Populate all 5 layers: VideoInput, Media, Slide, Props, Messages.
    send_cmd(
        &mut core,
        command::Action::SetLayerState(SetLayerState {
            layer: Some(LayerSpec {
                kind: LayerKind::VideoInput as i32,
                enabled: true,
                content: Some(layer_spec::Content::VideoInput(VideoInputLayerSpec {
                    source_id: "cam-main".into(),
                    fallback_color: c(0.08, 0.22, 0.34, 1.0),
                    ..Default::default()
                })),
                ..Default::default()
            }),
        }),
    );

    send_cmd(
        &mut core,
        command::Action::SetLayerState(SetLayerState {
            layer: Some(LayerSpec {
                kind: LayerKind::Media as i32,
                enabled: true,
                content: Some(layer_spec::Content::Media(MediaLayerSpec {
                    gradient: Some(GradientSpec {
                        kind: GradientKind::Linear as i32,
                        angle_degrees: 135.0,
                        stops: vec![
                            ColorStop {
                                color: c(0.11, 0.14, 0.28, 1.0),
                                position: 0.0,
                            },
                            ColorStop {
                                color: c(0.24, 0.17, 0.36, 1.0),
                                position: 0.55,
                            },
                            ColorStop {
                                color: c(0.08, 0.10, 0.20, 1.0),
                                position: 1.0,
                            },
                        ],
                    }),
                    ..Default::default()
                })),
                ..Default::default()
            }),
        }),
    );

    let sanctuary_json = TemplateStyle::preset_sanctuary().to_json().unwrap();
    send_cmd(
        &mut core,
        command::Action::SetLayerState(SetLayerState {
            layer: Some(LayerSpec {
                kind: LayerKind::Slide as i32,
                enabled: true,
                content: Some(layer_spec::Content::Slide(SlideLayerSpec {
                    text: "Holy, holy, holy! Lord God Almighty!\nEarly in the morning our song shall rise to Thee.".into(),
                    title_text: "Holy, Holy, Holy".into(),
                    reference_text: "Verse 1".into(),
                    footer_text: "Reginald Heber · Public Domain".into(),
                    style_json: sanctuary_json,
                    ..Default::default()
                })),
                ..Default::default()
            }),
        }),
    );

    let prop_id = new_id();
    send_cmd(
        &mut core,
        command::Action::SaveProp(SaveProp {
            prop: Some(Prop {
                id: Some(prop_id.clone()),
                name: "Live Bug".into(),
                label: "SELAH CHURCH".into(),
                background_color: c(0.14, 0.28, 0.52, 0.92),
                text_color: c(1.0, 0.96, 0.84, 1.0),
                ..Default::default()
            }),
        }),
    );
    send_cmd(
        &mut core,
        command::Action::ShowProp(ShowProp {
            prop_id: Some(prop_id),
        }),
    );

    send_cmd(
        &mut core,
        command::Action::ShowMessage(ShowMessage {
            custom_text: "Child #142 needed in Nursery B".into(),
            ..Default::default()
        }),
    );

    LookGoldenHarness {
        _dir: dir,
        core,
        worship_look_id,
        sermon_look_id,
        camera_look_id,
    }
}

#[test]
fn each_screen_shows_the_right_layers_for_each_look_match_golden() {
    let mut gpu = GpuCompositor::new(AdapterChoice::Software).expect("software GPU adapter");
    let mut cpu = CpuCompositor::default();
    let (w, h) = (640, 360);
    let mut harness = setup_look_harness(w, h);

    let looks = [
        ("worship", harness.worship_look_id.clone()),
        ("sermon", harness.sermon_look_id.clone()),
        ("camera", harness.camera_look_id.clone()),
    ];
    let screens = [(0usize, "main"), (1usize, "stream"), (2usize, "lobby")];

    for (look_slug, look_id) in looks {
        send_cmd(
            &mut harness.core,
            command::Action::SetLook(SetLook {
                look_id: Some(look_id),
            }),
        );
        let scene: Scene = harness.core.scene().clone();

        for (out_idx, screen_slug) in screens {
            let name = format!("m4-look-{look_slug}-{screen_slug}");
            let gpu_frame = gpu.render_output(&scene, out_idx as u32, w, h).unwrap();
            let cpu_frame = cpu.render_output(&scene, out_idx as u32, w, h).unwrap();
            golden::check(&name, &gpu_frame, TOLERANCE_STYLE)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            let diff = golden::compare(&gpu_frame, &cpu_frame, TOLERANCE_STYLE).unwrap();
            assert!(
                diff.fraction() <= TOLERANCE_STYLE.max_fraction,
                "{name} GPU/CPU parity failed: {diff:?}"
            );
        }
    }
}
