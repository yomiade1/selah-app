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

// ---------------------------------------------------------------------------------------------
// M4.6 / M4-A7: the stage display, rendered by the engine's compositors from core's composition.
// ---------------------------------------------------------------------------------------------

/// 10:12:33 UTC on a fixed day: the stage clock and the timer read from here.
const STAGE_WALL_MS: i64 = 1_762_855_953_000;

fn live_status(core: &mut CoreService) -> selah_proto::v1::LiveStatus {
    let req = Envelope {
        id: 9_000,
        correlation_id: 0,
        actor: Some(Actor {
            user_id: "operator".into(),
            device_id: "desk".into(),
            origin: ActorOrigin::Ui as i32,
        }),
        body: Some(envelope::Body::Query(selah_proto::v1::Query {
            request: Some(selah_proto::v1::query::Request::GetLiveState(
                Default::default(),
            )),
        })),
    };
    match core.handle(&req).reply.expect("reply").body {
        Some(envelope::Body::Reply(selah_proto::v1::Reply {
            result: Some(selah_proto::v1::reply::Result::LiveStatus(status)),
            ..
        })) => status,
        other => panic!("unexpected {other:?}"),
    }
}

fn line(text: &str, chords: &[(u32, &str)]) -> selah_proto::v1::LyricLine {
    selah_proto::v1::LyricLine {
        text: text.into(),
        chords: chords
            .iter()
            .map(|(position, chord)| selah_proto::v1::Chord {
                position: *position,
                text: (*chord).into(),
            })
            .collect(),
    }
}

/// "Be Thou My Vision" (Irish, tr. Mary Byrne 1905, versified Eleanor Hull 1912: public domain).
fn stage_harness(w: u32, h: u32) -> (tempfile::TempDir, CoreService) {
    use selah_proto::v1::{
        AdvanceLiveClock, LoadLiveService, SaveService, SaveSong, SaveTimer, Section, SectionKind,
        SendBoothMessage, Service, ServiceItem, ServiceItemKind, Song, StartTimer, Timer,
        TimerKind, TriggerSlide,
    };
    let dir = tempfile::tempdir().unwrap();
    let mut core = CoreService::new(dir.path(), false);
    let res = Some(Resolution {
        width: w,
        height: h,
    });
    send_cmd(
        &mut core,
        command::Action::SetOutputConfigs(SetOutputConfigs {
            outputs: vec![
                OutputConfig {
                    output_index: 0,
                    name: "Main Hall".into(),
                    role: OutputRole::Main as i32,
                    resolution: res,
                    enabled: true,
                    ..Default::default()
                },
                OutputConfig {
                    output_index: 1,
                    name: "Stage".into(),
                    role: OutputRole::Stage as i32,
                    resolution: res,
                    enabled: true,
                    ..Default::default()
                },
            ],
        }),
    );
    let section = |kind: SectionKind, label: &str, lines| Section {
        id: Some(new_id()),
        kind: kind as i32,
        label: label.into(),
        lines,
        ..Default::default()
    };
    let song_id = new_id();
    send_cmd(
        &mut core,
        command::Action::SaveSong(SaveSong {
            song: Some(Song {
                id: Some(song_id.clone()),
                title: "Be Thou My Vision".into(),
                key: "Eb".into(),
                sections: vec![
                    section(
                        SectionKind::Verse,
                        "Verse 1",
                        vec![
                            line(
                                "Be Thou my Vision, O Lord of my heart;",
                                &[(0, "Eb"), (11, "Ab"), (25, "Eb")],
                            ),
                            line(
                                "Naught be all else to me, save that Thou art.",
                                &[(0, "Cm"), (19, "Bb"), (34, "Eb")],
                            ),
                        ],
                    ),
                    section(
                        SectionKind::Verse,
                        "Verse 2",
                        vec![
                            line(
                                "Be Thou my Wisdom, and Thou my true Word;",
                                &[(0, "Eb"), (11, "Ab"), (23, "Eb")],
                            ),
                            line(
                                "I ever with Thee and Thou with me, Lord;",
                                &[(2, "Cm"), (17, "Bb"), (26, "Eb")],
                            ),
                        ],
                    ),
                    section(
                        SectionKind::Verse,
                        "Verse 3",
                        vec![
                            line(
                                "Thou my great Father, I Thy true son;",
                                &[(0, "Eb"), (14, "Ab")],
                            ),
                            line(
                                "Thou in me dwelling, and I with Thee one.",
                                &[(0, "Cm"), (25, "Eb")],
                            ),
                        ],
                    ),
                ],
                ..Default::default()
            }),
        }),
    );
    let service_id = new_id();
    send_cmd(
        &mut core,
        command::Action::SaveService(SaveService {
            service: Some(Service {
                id: Some(service_id.clone()),
                name: "Sunday Morning".into(),
                items: vec![
                    ServiceItem {
                        id: Some(new_id()),
                        kind: ServiceItemKind::Song as i32,
                        content_id: Some(song_id),
                        title: "Be Thou My Vision".into(),
                        song_key: "Eb".into(),
                        notes: "Slow down on the last line. Invite everyone to stand.".into(),
                        ..Default::default()
                    },
                    ServiceItem {
                        id: Some(new_id()),
                        kind: ServiceItemKind::Header as i32,
                        title: "Sermon".into(),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }),
        }),
    );
    send_cmd(
        &mut core,
        command::Action::AdvanceLiveClock(AdvanceLiveClock {
            delta_ms: 0,
            wall_unix_ms: STAGE_WALL_MS,
        }),
    );
    send_cmd(
        &mut core,
        command::Action::LoadLiveService(LoadLiveService {
            service_id: Some(service_id),
            trigger_first_item: true,
        }),
    );
    let status = live_status(&mut core);
    let verse_two = status
        .current_item_slides
        .iter()
        .find(|s| s.label == "Verse 2")
        .expect("verse 2 slide")
        .index;
    send_cmd(
        &mut core,
        command::Action::TriggerSlide(TriggerSlide {
            item_index: 0,
            slide_index: verse_two,
        }),
    );
    let timer_id = new_id();
    send_cmd(
        &mut core,
        command::Action::SaveTimer(SaveTimer {
            timer: Some(Timer {
                id: Some(timer_id.clone()),
                name: "Worship set".into(),
                kind: TimerKind::Countdown as i32,
                duration_ms: 180_000,
                ..Default::default()
            }),
        }),
    );
    send_cmd(
        &mut core,
        command::Action::StartTimer(StartTimer {
            timer_id: Some(timer_id),
        }),
    );
    send_cmd(
        &mut core,
        command::Action::SendBoothMessage(SendBoothMessage {
            text: "Repeat verse 4, then go straight into Romans 8".into(),
        }),
    );
    (dir, core)
}

fn check_stage_frame(
    name: &str,
    scene: &Scene,
    output: u32,
    (w, h): (u32, u32),
    gpu: &mut GpuCompositor,
    cpu: &mut CpuCompositor,
) {
    gpu.set_clock_unix_ms(Some(STAGE_WALL_MS));
    cpu.set_clock_unix_ms(Some(STAGE_WALL_MS));
    let gpu_frame = gpu.render_output(scene, output, w, h).unwrap();
    let cpu_frame = cpu.render_output(scene, output, w, h).unwrap();
    golden::check(name, &gpu_frame, TOLERANCE_STYLE).unwrap_or_else(|e| panic!("{name}: {e}"));
    let diff = golden::compare(&gpu_frame, &cpu_frame, TOLERANCE_STYLE).unwrap();
    assert!(
        diff.fraction() <= TOLERANCE_STYLE.max_fraction,
        "{name} GPU/CPU parity failed: {diff:?}"
    );
}

#[test]
fn stage_display_matches_golden_including_chord_alignment() {
    use selah_proto::v1::{
        Rect, SaveStageLayout, SetStageLayout, StageElement, StageElementKind, StageLayout,
    };
    let mut gpu = GpuCompositor::new(AdapterChoice::Software).expect("software GPU adapter");
    let mut cpu = CpuCompositor::default();
    let size = (960, 540);
    let (_dir, mut core) = stage_harness(size.0, size.1);

    let scene = core.scene().clone();
    // SAFE-01: the booth message is on the stage output and nowhere else.
    let booth = "Repeat verse 4";
    let stage_layers = format!("{:?}", scene.outputs[1].layers);
    assert!(
        stage_layers.contains(booth),
        "stage shows the booth message"
    );
    let main_layers = format!("{:?}", scene.outputs[0].layers);
    assert!(!main_layers.contains(booth), "audience screens never do");
    // The chords travel with the words to the stage.
    assert!(stage_layers.contains("Ab"), "{stage_layers}");

    // Prototype screen 06: song, key and sections; words with chords; next; timer; clock; booth.
    check_stage_frame("m4-stage-standard", &scene, 1, size, &mut gpu, &mut cpu);

    // Chords off: the same words without the chord row (the alignment reference).
    let layout_id = new_id();
    send_cmd(
        &mut core,
        command::Action::SaveStageLayout(SaveStageLayout {
            stage_layout: Some(StageLayout {
                id: Some(layout_id.clone()),
                name: "Lyrics only".into(),
                show_chords: false,
                show_key_badge: true,
                show_arrangement_strip: true,
                show_clock: true,
                show_next_slide: true,
                show_timer: false,
                show_speaker_notes: false,
                show_booth_message: false,
                elements: Vec::new(),
            }),
        }),
    );
    send_cmd(
        &mut core,
        command::Action::SetStageLayout(SetStageLayout {
            stage_layout_id: Some(layout_id),
        }),
    );
    let scene = core.scene().clone();
    check_stage_frame("m4-stage-lyrics-only", &scene, 1, size, &mut gpu, &mut cpu);

    // A speaker's layout: notes beside smaller words, a big timer, no next slide.
    let speaker_id = new_id();
    let element = |kind: StageElementKind, x, y, width, height, font_scale| StageElement {
        id: Some(new_id()),
        kind: kind as i32,
        rect: Some(Rect {
            x,
            y,
            width,
            height,
        }),
        visible: true,
        font_scale,
        show_chords: false,
    };
    send_cmd(
        &mut core,
        command::Action::SaveStageLayout(SaveStageLayout {
            stage_layout: Some(StageLayout {
                id: Some(speaker_id.clone()),
                name: "Speaker".into(),
                show_chords: false,
                show_key_badge: false,
                show_arrangement_strip: false,
                show_clock: true,
                show_next_slide: false,
                show_timer: true,
                show_speaker_notes: true,
                show_booth_message: true,
                elements: vec![
                    element(
                        StageElementKind::CurrentSlide,
                        0.0,
                        0.0667,
                        0.55,
                        0.65,
                        0.75,
                    ),
                    element(StageElementKind::SpeakerNotes, 0.56, 0.09, 0.42, 0.6, 1.2),
                    element(StageElementKind::Timer, 0.0, 0.72, 0.5, 0.22, 1.4),
                ],
            }),
        }),
    );
    send_cmd(
        &mut core,
        command::Action::SetStageLayout(SetStageLayout {
            stage_layout_id: Some(speaker_id),
        }),
    );
    let scene = core.scene().clone();
    check_stage_frame("m4-stage-speaker", &scene, 1, size, &mut gpu, &mut cpu);
}
