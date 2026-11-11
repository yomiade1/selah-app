use std::collections::HashMap;

use selah_core::library::new_id;
use selah_core::service::CoreService;
use selah_proto::v1::{
    Actor, ActorOrigin, AdvanceLiveClock, ClearGroup, ClearLayer, ClearOutputs, Color, Command,
    Envelope, HideMessage, HideProp, LayerKind, LayerSpec, LiveMessage, LoadLiveService, Look,
    LookRule, Macro, MacroAction, MacroActionKind, NextSlide, OutputConfig, OutputRole, PauseTimer,
    PreviousSlide, Prop, Query, ResetTimer, Resolution, RunMacro, SaveClearGroup, SaveLook,
    SaveMacro, SaveMessage, SaveProp, SaveService, SaveSong, SaveTemplate, SaveTimer, Section,
    SendBoothMessage, Service, ServiceItem, ServiceItemKind, SetBoothLock, SetLayerState, SetLook,
    SetOutputConfigs, ShowLogoCommand, ShowMessage, ShowProp, Song, StartTimer, Template, Timer,
    TimerKind, TimerOverrunMode, TriggerClearGroup, TriggerSlide, VideoInputLayerSpec, command,
    envelope, layer_spec, query, reply,
};
use selah_render::template::TemplateStyle;

fn send_cmd_with_actor(
    core: &mut CoreService,
    user_id: &str,
    origin: ActorOrigin,
    action: command::Action,
) -> Envelope {
    static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let req = Envelope {
        id: NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        correlation_id: 0,
        actor: Some(Actor {
            user_id: user_id.into(),
            device_id: "desk".into(),
            origin: origin as i32,
        }),
        body: Some(envelope::Body::Command(Command {
            undo_group: None,
            action: Some(action),
        })),
    };
    core.handle(&req).reply.expect("command reply")
}

fn send_cmd_as(core: &mut CoreService, origin: ActorOrigin, action: command::Action) -> Envelope {
    send_cmd_with_actor(core, "operator", origin, action)
}

fn send_cmd(core: &mut CoreService, action: command::Action) -> reply::Result {
    let env = send_cmd_as(core, ActorOrigin::Ui, action);
    match env.body {
        Some(envelope::Body::Reply(r)) => r.result.expect("reply result"),
        other => panic!("expected Reply, got {other:?}"),
    }
}

fn send_query(core: &mut CoreService, request: query::Request) -> reply::Result {
    let req = Envelope {
        id: 999,
        correlation_id: 0,
        actor: Some(Actor {
            user_id: "operator".into(),
            device_id: "desk".into(),
            origin: ActorOrigin::Ui as i32,
        }),
        body: Some(envelope::Body::Query(Query {
            request: Some(request),
        })),
    };
    let reply_env = core.handle(&req).reply.expect("query reply");
    match reply_env.body {
        Some(envelope::Body::Reply(r)) => r.result.expect("query result"),
        other => panic!("expected query Reply, got {other:?}"),
    }
}

#[test]
fn looks_filter_layers_and_override_templates_per_output() {
    let dir = tempfile::tempdir().unwrap();
    let mut core = CoreService::new(dir.path(), false);

    // Configure 3 outputs: 0 = Main, 1 = Stream, 2 = Lobby.
    send_cmd(
        &mut core,
        command::Action::SetOutputConfigs(SetOutputConfigs {
            outputs: vec![
                OutputConfig {
                    output_index: 0,
                    name: "Main Hall".into(),
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
                    name: "Broadcast Stream".into(),
                    role: OutputRole::Stream as i32,
                    resolution: Some(Resolution {
                        width: 1920,
                        height: 1080,
                    }),
                    enabled: true,
                    ..Default::default()
                },
                OutputConfig {
                    output_index: 2,
                    name: "Lobby TV".into(),
                    role: OutputRole::Lobby as i32,
                    resolution: Some(Resolution {
                        width: 1920,
                        height: 1080,
                    }),
                    enabled: true,
                    ..Default::default()
                },
            ],
        }),
    );

    // Save a Lower Third template for Stream override.
    let lower_third_id = new_id();
    let lower_third_json = TemplateStyle::preset_lower_third().to_json().unwrap();
    send_cmd(
        &mut core,
        command::Action::SaveTemplate(SaveTemplate {
            template: Some(Template {
                id: Some(lower_third_id.clone()),
                name: "Stream Lower Third".into(),
                variants: vec![selah_proto::v1::TemplateVariant {
                    id: Some(new_id()),
                    resolution: Some(Resolution {
                        width: 1920,
                        height: 1080,
                    }),
                    max_lines: 2,
                    style_json: lower_third_json.clone(),
                }],
            }),
        }),
    );

    // Save Worship Look:
    // - Main (0): Media + Slide + Props + Messages
    // - Stream (1): VideoInput + Slide (with Lower Third template override)
    // - Lobby (2): Media + Slide + Messages
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
                        enabled_layers: vec![LayerKind::VideoInput as i32, LayerKind::Slide as i32],
                        template_override_id: Some(lower_third_id),
                        mask_enabled: true,
                        ..Default::default()
                    },
                    LookRule {
                        id: Some(new_id()),
                        output_index: 2,
                        enabled_layers: vec![LayerKind::Media as i32, LayerKind::Messages as i32],
                        ..Default::default()
                    },
                ],
            }),
        }),
    );

    // Save Camera Look:
    // - Main (0): VideoInput only
    // - Stream (1): VideoInput + Props
    // - Lobby (2): VideoInput only
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
                        enabled_layers: vec![LayerKind::VideoInput as i32],
                        ..Default::default()
                    },
                ],
            }),
        }),
    );

    // Activate VideoInput, Slide (via service), Props, and Messages.
    let song_id = new_id();
    send_cmd(
        &mut core,
        command::Action::SaveSong(SaveSong {
            song: Some(Song {
                id: Some(song_id.clone()),
                title: "Holy Holy Holy".into(),
                sections: vec![Section {
                    id: Some(new_id()),
                    kind: selah_proto::v1::SectionKind::Verse as i32,
                    label: "Verse 1".into(),
                    lines: vec![
                        selah_proto::v1::LyricLine {
                            text: "Holy, holy, holy!".into(),
                            ..Default::default()
                        },
                        selah_proto::v1::LyricLine {
                            text: "Lord God Almighty!".into(),
                            ..Default::default()
                        },
                    ],
                    ..Default::default()
                }],
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
                name: "Sunday Service".into(),
                items: vec![ServiceItem {
                    id: Some(new_id()),
                    kind: ServiceItemKind::Song as i32,
                    title: "Holy Holy Holy".into(),
                    content_id: Some(song_id),
                    ..Default::default()
                }],
                ..Default::default()
            }),
        }),
    );
    send_cmd(
        &mut core,
        command::Action::LoadLiveService(LoadLiveService {
            service_id: Some(service_id),
            trigger_first_item: true,
        }),
    );
    send_cmd(
        &mut core,
        command::Action::SetLayerState(SetLayerState {
            layer: Some(LayerSpec {
                kind: LayerKind::VideoInput as i32,
                enabled: true,
                content: Some(layer_spec::Content::VideoInput(VideoInputLayerSpec {
                    source_id: "cam-1".into(),
                    fallback_color: Some(Color {
                        red: 0.1,
                        green: 0.2,
                        blue: 0.35,
                        alpha: 1.0,
                    }),
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
                name: "Church Bug".into(),
                label: "SELAH LIVE".into(),
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
            custom_text: "Welcome to Sunday Service".into(),
            ..Default::default()
        }),
    );

    // Apply Worship Look.
    let reply::Result::LiveStatus(status) = send_cmd(
        &mut core,
        command::Action::SetLook(SetLook {
            look_id: Some(worship_look_id),
        }),
    ) else {
        panic!("expected LiveStatus");
    };
    assert_eq!(status.automation[0].active_look_name, "Worship");

    let scene = core.scene();
    assert_eq!(scene.outputs.len(), 3);
    let main_kinds: Vec<i32> = scene.outputs[0].layers.iter().map(|l| l.kind).collect();
    assert_eq!(
        main_kinds,
        vec![
            LayerKind::Slide as i32,
            LayerKind::Props as i32,
            LayerKind::Messages as i32,
        ]
    );
    let stream_kinds: Vec<i32> = scene.outputs[1].layers.iter().map(|l| l.kind).collect();
    assert_eq!(
        stream_kinds,
        vec![LayerKind::VideoInput as i32, LayerKind::Slide as i32]
    );
    // Stream Slide layer has the Lower Third template override applied and mask effect enabled.
    let stream_slide = scene.outputs[1]
        .layers
        .iter()
        .find_map(|l| match &l.content {
            Some(layer_spec::Content::Slide(s)) => Some(s),
            _ => None,
        })
        .unwrap();
    let resolved_stream_style = TemplateStyle::from_json(&stream_slide.style_json).unwrap();
    assert_eq!(resolved_stream_style.name, "Stream Lower Third");
    assert_eq!(scene.outputs[1].effects.len(), 1);

    let lobby_kinds: Vec<i32> = scene.outputs[2].layers.iter().map(|l| l.kind).collect();
    assert_eq!(lobby_kinds, vec![LayerKind::Messages as i32]);

    // Switch to Camera Look.
    let reply::Result::LiveStatus(status) = send_cmd(
        &mut core,
        command::Action::SetLook(SetLook {
            look_id: Some(camera_look_id),
        }),
    ) else {
        panic!("expected LiveStatus");
    };
    assert_eq!(status.automation[0].active_look_name, "Camera");
    let scene = core.scene();
    assert_eq!(
        scene.outputs[0]
            .layers
            .iter()
            .map(|l| l.kind)
            .collect::<Vec<_>>(),
        vec![LayerKind::VideoInput as i32]
    );
    assert_eq!(
        scene.outputs[1]
            .layers
            .iter()
            .map(|l| l.kind)
            .collect::<Vec<_>>(),
        vec![LayerKind::VideoInput as i32, LayerKind::Props as i32]
    );
}

#[test]
fn props_and_messages_interpolate_tokens_and_auto_hide_on_clock() {
    let dir = tempfile::tempdir().unwrap();
    let mut core = CoreService::new(dir.path(), false);

    // Save a countdown timer (5 minutes) and start it so {countdown} resolves to 05:00.
    let timer_id = new_id();
    send_cmd(
        &mut core,
        command::Action::SaveTimer(SaveTimer {
            timer: Some(Timer {
                id: Some(timer_id.clone()),
                name: "Pre-service Countdown".into(),
                kind: TimerKind::Countdown as i32,
                duration_ms: 300_000,
                overrun_mode: TimerOverrunMode::StopAtZero as i32,
                ..Default::default()
            }),
        }),
    );
    send_cmd(
        &mut core,
        command::Action::StartTimer(StartTimer {
            timer_id: Some(timer_id.clone()),
        }),
    );

    // Save a message template with {code}, {room}, and {countdown} tokens and 15s auto-hide.
    let msg_id = new_id();
    let mut default_tokens = HashMap::new();
    default_tokens.insert("room".into(), "Nursery B".into());
    send_cmd(
        &mut core,
        command::Action::SaveMessage(SaveMessage {
            message: Some(LiveMessage {
                id: Some(msg_id.clone()),
                name: "Parent Pager".into(),
                template_text: "Child #{code} needed in {room} ({countdown})".into(),
                default_tokens,
                duration_seconds: 15,
                timer_id: Some(timer_id),
                stage_only: false,
                ..Default::default()
            }),
        }),
    );

    let mut token_values = HashMap::new();
    token_values.insert("code".into(), "142".into());
    let reply::Result::LiveStatus(status) = send_cmd(
        &mut core,
        command::Action::ShowMessage(ShowMessage {
            message_id: Some(msg_id),
            token_values,
            ..Default::default()
        }),
    ) else {
        panic!("expected LiveStatus");
    };
    let auto = &status.automation[0];
    assert_eq!(
        auto.active_message_text,
        "Child #142 needed in Nursery B (05:00)"
    );
    assert_eq!(auto.active_message_remaining_ms, 15_000);
    assert!(
        status
            .layers
            .iter()
            .any(|l| l.kind == LayerKind::Messages as i32 && l.active)
    );

    // Advance virtual clock by 10s -> message still visible with 5s left.
    let reply::Result::LiveStatus(status) = send_cmd(
        &mut core,
        command::Action::AdvanceLiveClock(AdvanceLiveClock {
            delta_ms: 10_000,
            wall_unix_ms: 0,
        }),
    ) else {
        panic!("expected LiveStatus");
    };
    assert_eq!(status.automation[0].active_message_remaining_ms, 5_000);
    assert!(
        status
            .layers
            .iter()
            .any(|l| l.kind == LayerKind::Messages as i32 && l.active)
    );

    // Advance virtual clock by 5s -> message auto-hides at 0s!
    let reply::Result::LiveStatus(status) = send_cmd(
        &mut core,
        command::Action::AdvanceLiveClock(AdvanceLiveClock {
            delta_ms: 5_000,
            wall_unix_ms: 0,
        }),
    ) else {
        panic!("expected LiveStatus");
    };
    assert_eq!(status.automation[0].active_message_remaining_ms, 0);
    assert!(status.automation[0].active_message_text.is_empty());
    assert!(
        !status
            .layers
            .iter()
            .any(|l| l.kind == LayerKind::Messages as i32 && l.active)
    );

    // SAFE-01: SendBoothMessage reaches only booth_message, never audience layers.
    let reply::Result::LiveStatus(status) = send_cmd(
        &mut core,
        command::Action::SendBoothMessage(SendBoothMessage {
            text: "Pastor skipped Verse 3".into(),
        }),
    ) else {
        panic!("expected LiveStatus");
    };
    assert_eq!(status.automation[0].booth_message, "Pastor skipped Verse 3");
    assert!(
        !status
            .layers
            .iter()
            .any(|l| l.kind == LayerKind::Messages as i32 && l.active)
    );
    assert!(core.scene().outputs[0].layers.is_empty());
}

#[test]
fn timers_stay_within_one_frame_over_sixty_minutes_and_handle_overrun_modes() {
    let dir = tempfile::tempdir().unwrap();
    let mut core = CoreService::new(dir.path(), false);

    // 1. M4-A4: 60-minute countdown timer (3,600,000 ms) with CountUpNegative overrun.
    let sermon_timer_id = new_id();
    send_cmd(
        &mut core,
        command::Action::SaveTimer(SaveTimer {
            timer: Some(Timer {
                id: Some(sermon_timer_id.clone()),
                name: "60m Sermon Timer".into(),
                kind: TimerKind::Countdown as i32,
                duration_ms: 3_600_000,
                overrun_mode: TimerOverrunMode::CountUpNegative as i32,
                show_hours: true,
                ..Default::default()
            }),
        }),
    );

    // 2. StopAtZero timer (10s).
    let stop_timer_id = new_id();
    send_cmd(
        &mut core,
        command::Action::SaveTimer(SaveTimer {
            timer: Some(Timer {
                id: Some(stop_timer_id.clone()),
                name: "10s Intro".into(),
                kind: TimerKind::Countdown as i32,
                duration_ms: 10_000,
                overrun_mode: TimerOverrunMode::StopAtZero as i32,
                ..Default::default()
            }),
        }),
    );

    // 3. TriggerMacro timer (20s) that fires a macro showing a Prop on overrun.
    let prop_id = new_id();
    send_cmd(
        &mut core,
        command::Action::SaveProp(SaveProp {
            prop: Some(Prop {
                id: Some(prop_id.clone()),
                name: "Wrap Up Alert".into(),
                label: "TIME UP".into(),
                ..Default::default()
            }),
        }),
    );
    let overrun_macro_id = new_id();
    send_cmd(
        &mut core,
        command::Action::SaveMacro(SaveMacro {
            r#macro: Some(Macro {
                id: Some(overrun_macro_id.clone()),
                name: "On Overrun Show Prop".into(),
                actions: vec![MacroAction {
                    id: Some(new_id()),
                    kind: MacroActionKind::ShowProp as i32,
                    target_id: Some(prop_id.clone()),
                    ..Default::default()
                }],
                ..Default::default()
            }),
        }),
    );
    let macro_timer_id = new_id();
    send_cmd(
        &mut core,
        command::Action::SaveTimer(SaveTimer {
            timer: Some(Timer {
                id: Some(macro_timer_id.clone()),
                name: "20s Macro Countdown".into(),
                kind: TimerKind::Countdown as i32,
                duration_ms: 20_000,
                overrun_mode: TimerOverrunMode::TriggerMacro as i32,
                overrun_macro_id: Some(overrun_macro_id),
                ..Default::default()
            }),
        }),
    );

    // Start all 3 timers.
    for tid in [&sermon_timer_id, &stop_timer_id, &macro_timer_id] {
        send_cmd(
            &mut core,
            command::Action::StartTimer(StartTimer {
                timer_id: Some(tid.clone()),
            }),
        );
    }

    // Advance virtual clock across 60 minutes (3,600 steps of 1,000 ms).
    let frame_ms_budget = 16_i64; // 1 frame at 60 fps is 16.67 ms.
    let mut expected_elapsed_ms = 0_u64;
    for step in 1..=3_600 {
        expected_elapsed_ms += 1_000;
        let reply::Result::LiveStatus(status) = send_cmd(
            &mut core,
            command::Action::AdvanceLiveClock(AdvanceLiveClock {
                delta_ms: 1_000,
                wall_unix_ms: 1_700_000_000_000 + expected_elapsed_ms as i64,
            }),
        ) else {
            panic!("expected LiveStatus");
        };
        let auto = &status.automation[0];
        let sermon = auto
            .timers
            .iter()
            .find(|t| t.timer_id.as_ref() == Some(&sermon_timer_id))
            .unwrap();
        let expected_rem = 3_600_000_i64 - expected_elapsed_ms as i64;
        let drift_ms = (sermon.remaining_ms - expected_rem).abs();
        assert!(
            drift_ms <= frame_ms_budget,
            "step {step}: drift {drift_ms} ms exceeded 1 frame ({frame_ms_budget} ms)"
        );

        if step == 12 {
            // StopAtZero timer reached 0 at 10s and stopped at 0.
            let stop_t = auto
                .timers
                .iter()
                .find(|t| t.timer_id.as_ref() == Some(&stop_timer_id))
                .unwrap();
            assert!(!stop_t.running);
            assert!(stop_t.overrun);
            assert_eq!(stop_t.remaining_ms, 0);
            assert_eq!(stop_t.formatted_time, "00:00");
        }
        if step == 21 {
            // TriggerMacro timer reached 0 at 20s and fired the overrun macro (showing the Prop).
            let mac_t = auto
                .timers
                .iter()
                .find(|t| t.timer_id.as_ref() == Some(&macro_timer_id))
                .unwrap();
            assert!(!mac_t.running);
            assert!(mac_t.overrun);
            assert!(mac_t.overrun_macro_fired);
            assert_eq!(auto.active_prop_id, Some(prop_id.clone()));
        }
    }

    // At 60 minutes + 5 seconds (overrun on CountUpNegative timer), verify negative count-up.
    let reply::Result::LiveStatus(status) = send_cmd(
        &mut core,
        command::Action::AdvanceLiveClock(AdvanceLiveClock {
            delta_ms: 5_000,
            wall_unix_ms: 0,
        }),
    ) else {
        panic!("expected LiveStatus");
    };
    let sermon = status.automation[0]
        .timers
        .iter()
        .find(|t| t.timer_id.as_ref() == Some(&sermon_timer_id))
        .unwrap();
    assert!(sermon.running);
    assert!(sermon.overrun);
    assert_eq!(sermon.remaining_ms, -5_000);
    assert_eq!(sermon.formatted_time, "-00:05");

    // Pause and Reset timer work cleanly.
    send_cmd(
        &mut core,
        command::Action::PauseTimer(PauseTimer {
            timer_id: Some(sermon_timer_id.clone()),
        }),
    );
    let reply::Result::LiveStatus(status) = send_cmd(
        &mut core,
        command::Action::ResetTimer(ResetTimer {
            timer_id: Some(sermon_timer_id.clone()),
        }),
    ) else {
        panic!("expected LiveStatus");
    };
    let reset_sermon = status.automation[0]
        .timers
        .iter()
        .find(|t| t.timer_id.as_ref() == Some(&sermon_timer_id))
        .unwrap();
    assert!(!reset_sermon.running);
    assert!(!reset_sermon.overrun);
    assert_eq!(reset_sermon.remaining_ms, 3_600_000);
}

#[test]
fn macros_slide_actions_clear_groups_booth_lock_and_volunteer_role_lock() {
    let dir = tempfile::tempdir().unwrap();
    let mut core = CoreService::new(dir.path(), false);

    let look_id = new_id();
    send_cmd(
        &mut core,
        command::Action::SaveLook(SaveLook {
            look: Some(Look {
                id: Some(look_id.clone()),
                name: "Sermon Notes".into(),
                hotkey: "Shift+2".into(),
                rules: vec![LookRule {
                    id: Some(new_id()),
                    output_index: 0,
                    enabled_layers: vec![LayerKind::Slide as i32, LayerKind::Props as i32],
                    ..Default::default()
                }],
            }),
        }),
    );

    let prop_id = new_id();
    send_cmd(
        &mut core,
        command::Action::SaveProp(SaveProp {
            prop: Some(Prop {
                id: Some(prop_id.clone()),
                name: "Speaker Lower Third".into(),
                label: "Pastor Grace".into(),
                ..Default::default()
            }),
        }),
    );

    let mac_id = new_id();
    send_cmd(
        &mut core,
        command::Action::SaveMacro(SaveMacro {
            r#macro: Some(Macro {
                id: Some(mac_id.clone()),
                name: "Start Sermon".into(),
                hotkey: "M1".into(),
                actions: vec![
                    MacroAction {
                        id: Some(new_id()),
                        kind: MacroActionKind::SetLook as i32,
                        target_id: Some(look_id.clone()),
                        ..Default::default()
                    },
                    MacroAction {
                        id: Some(new_id()),
                        kind: MacroActionKind::ShowProp as i32,
                        target_id: Some(prop_id.clone()),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }),
        }),
    );

    // Song with 2 sections; slide 1 has a slide action that runs the "Start Sermon" macro.
    let song_id = new_id();
    send_cmd(
        &mut core,
        command::Action::SaveSong(SaveSong {
            song: Some(Song {
                id: Some(song_id.clone()),
                title: "Be Thou My Vision".into(),
                sections: vec![
                    Section {
                        id: Some(new_id()),
                        kind: selah_proto::v1::SectionKind::Verse as i32,
                        label: "Verse 1".into(),
                        lines: vec![
                            selah_proto::v1::LyricLine {
                                text: "Be Thou my Vision".into(),
                                ..Default::default()
                            },
                            selah_proto::v1::LyricLine {
                                text: "O Lord of my heart".into(),
                                ..Default::default()
                            },
                        ],
                        ..Default::default()
                    },
                    Section {
                        id: Some(new_id()),
                        kind: selah_proto::v1::SectionKind::Verse as i32,
                        label: "Verse 2".into(),
                        lines: vec![
                            selah_proto::v1::LyricLine {
                                text: "Riches I heed not".into(),
                                ..Default::default()
                            },
                            selah_proto::v1::LyricLine {
                                text: "Nor man's empty praise".into(),
                                ..Default::default()
                            },
                        ],
                        ..Default::default()
                    },
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
                name: "Morning Service".into(),
                items: vec![ServiceItem {
                    id: Some(new_id()),
                    kind: ServiceItemKind::Song as i32,
                    title: "Be Thou My Vision".into(),
                    content_id: Some(song_id),
                    slide_action_index: 2, // 1-based index 2 => slide index 1
                    slide_macro_id: Some(mac_id.clone()),
                    ..Default::default()
                }],
                ..Default::default()
            }),
        }),
    );

    send_cmd(
        &mut core,
        command::Action::LoadLiveService(LoadLiveService {
            service_id: Some(service_id),
            trigger_first_item: true,
        }),
    );

    // Triggering slide 1 fires the slide macro automatically!
    let reply::Result::LiveStatus(status) = send_cmd(
        &mut core,
        command::Action::TriggerSlide(TriggerSlide {
            item_index: 0,
            slide_index: 1,
        }),
    ) else {
        panic!("expected LiveStatus");
    };
    assert_eq!(status.automation[0].active_look_name, "Sermon Notes");
    assert_eq!(status.automation[0].active_prop_name, "Speaker Lower Third");

    // Clear group that clears Slide + Props layers.
    let cg_id = new_id();
    send_cmd(
        &mut core,
        command::Action::SaveClearGroup(SaveClearGroup {
            clear_group: Some(ClearGroup {
                id: Some(cg_id.clone()),
                name: "Clear Graphics".into(),
                layers_to_clear: vec![LayerKind::Slide as i32, LayerKind::Props as i32],
                stop_audio: true,
                stop_timers: true,
                clear_show_logo: true,
                ..Default::default()
            }),
        }),
    );
    let reply::Result::LiveStatus(status) = send_cmd(
        &mut core,
        command::Action::TriggerClearGroup(TriggerClearGroup {
            clear_group_id: Some(cg_id.clone()),
        }),
    ) else {
        panic!("expected LiveStatus");
    };
    assert!(status.cleared);
    assert!(status.automation[0].active_prop_id.is_none());

    // M4-A12 / SAFE-06: Booth lock leaves only Next, Previous and Clear working.
    let reply::Result::LiveStatus(status) = send_cmd(
        &mut core,
        command::Action::SetBoothLock(SetBoothLock { locked: true }),
    ) else {
        panic!("expected LiveStatus");
    };
    assert!(status.automation[0].booth_locked);

    // NextSlide, PreviousSlide, ClearOutputs, ClearLayer, TriggerClearGroup, ShowLogo succeed while locked.
    assert!(matches!(
        send_cmd_as(
            &mut core,
            ActorOrigin::Ui,
            command::Action::NextSlide(NextSlide {})
        )
        .body,
        Some(envelope::Body::Reply(_))
    ));
    assert!(matches!(
        send_cmd_as(
            &mut core,
            ActorOrigin::Ui,
            command::Action::PreviousSlide(PreviousSlide {})
        )
        .body,
        Some(envelope::Body::Reply(_))
    ));
    assert!(matches!(
        send_cmd_as(
            &mut core,
            ActorOrigin::Ui,
            command::Action::ClearOutputs(ClearOutputs {})
        )
        .body,
        Some(envelope::Body::Reply(_))
    ));
    assert!(matches!(
        send_cmd_as(
            &mut core,
            ActorOrigin::Ui,
            command::Action::ClearLayer(ClearLayer {
                kind: LayerKind::Slide as i32,
            })
        )
        .body,
        Some(envelope::Body::Reply(_))
    ));
    assert!(matches!(
        send_cmd_as(
            &mut core,
            ActorOrigin::Ui,
            command::Action::ShowLogo(ShowLogoCommand { show_logo: true })
        )
        .body,
        Some(envelope::Body::Reply(_))
    ));

    // Other commands (RunMacro, SetLook, SaveSong, TriggerSlide) are refused while booth is locked!
    let denied_env = send_cmd_as(
        &mut core,
        ActorOrigin::Ui,
        command::Action::RunMacro(RunMacro {
            macro_id: Some(mac_id.clone()),
        }),
    );
    match denied_env.body {
        Some(envelope::Body::Error(err)) => assert_eq!(err.code, "SEL-SEC-0001"),
        other => panic!("expected SEL-SEC-0001 Error while booth locked, got {other:?}"),
    }

    // Unlock booth.
    send_cmd(
        &mut core,
        command::Action::SetBoothLock(SetBoothLock { locked: false }),
    );

    // M4-A12 / SEC-14 / SAFE-01: Volunteer role lock ("volunteer" actor) allows Simple-mode
    // triggers, Clear, and SendBoothMessage, but refuses Producer/Library commands (SetLook, RunMacro, SaveSong).
    assert!(matches!(
        send_cmd_with_actor(
            &mut core,
            "volunteer",
            ActorOrigin::Ui,
            command::Action::NextSlide(NextSlide {})
        )
        .body,
        Some(envelope::Body::Reply(_))
    ));
    assert!(matches!(
        send_cmd_with_actor(
            &mut core,
            "volunteer",
            ActorOrigin::Ui,
            command::Action::SendBoothMessage(SendBoothMessage {
                text: "Need help on wireless mic 2".into(),
            })
        )
        .body,
        Some(envelope::Body::Reply(_))
    ));

    let vol_denied = send_cmd_with_actor(
        &mut core,
        "volunteer",
        ActorOrigin::Ui,
        command::Action::SetLook(SetLook {
            look_id: Some(look_id.clone()),
        }),
    );
    match vol_denied.body {
        Some(envelope::Body::Error(err)) => assert_eq!(err.code, "SEL-SEC-0001"),
        other => panic!("expected SEL-SEC-0001 for volunteer SetLook, got {other:?}"),
    }

    // Verify List queries return our saved M4.3 documents.
    let reply::Result::LookList(looks) = send_query(
        &mut core,
        query::Request::ListLooks(selah_proto::v1::ListLooks {
            limit: 50,
            offset: 0,
        }),
    ) else {
        panic!("expected LookList");
    };
    assert_eq!(looks.looks.len(), 1);
    let _ = (HideProp { prop_id: None }, HideMessage {});
}
