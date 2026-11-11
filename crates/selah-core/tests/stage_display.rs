//! M4.6 (SAFE-01, SEC-03): stage screens get their own composition from core; the booth message
//! reaches only them; stage layouts from outside are checked; time passes for timers.
use selah_core::service::CoreService;
use selah_proto::v1::{
    Actor, ActorOrigin, Command, Envelope, OutputConfig, OutputRole, Rect, Resolution,
    SaveStageLayout, SaveTimer, SendBoothMessage, SetOutputConfigs, SetStageLayout, StageElement,
    StageElementKind, StageLayout, StartTimer, Timer, TimerKind, TimerOverrunMode, command,
    envelope, event, layer_spec, reply,
};

static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn send(core: &mut CoreService, action: command::Action) -> Result<reply::Result, String> {
    let request = Envelope {
        id: NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        correlation_id: 0,
        actor: Some(Actor {
            user_id: "operator".into(),
            device_id: "booth".into(),
            origin: ActorOrigin::Ui as i32,
        }),
        body: Some(envelope::Body::Command(Command {
            undo_group: None,
            action: Some(action),
        })),
    };
    match core.handle(&request).reply.expect("reply").body {
        Some(envelope::Body::Reply(r)) => Ok(r.result.expect("result")),
        Some(envelope::Body::Error(e)) => Err(e.code),
        other => panic!("unexpected {other:?}"),
    }
}

fn with_stage_output(core: &mut CoreService) {
    let res = Some(Resolution {
        width: 1280,
        height: 720,
    });
    send(
        core,
        command::Action::SetOutputConfigs(SetOutputConfigs {
            outputs: vec![
                OutputConfig {
                    output_index: 0,
                    name: "Main".into(),
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
    )
    .unwrap();
}

fn stage_text(core: &CoreService) -> String {
    let scene = core.scene();
    let stage = scene
        .outputs
        .iter()
        .find(|o| o.role == OutputRole::Stage as i32)
        .expect("stage output");
    format!("{:?}", stage.layers)
}

fn audience_text(core: &CoreService) -> String {
    let scene = core.scene();
    scene
        .outputs
        .iter()
        .filter(|o| o.role != OutputRole::Stage as i32)
        .map(|o| format!("{:?}", o.layers))
        .collect()
}

#[test]
fn the_booth_message_and_stage_layout_reach_only_the_stage_screens() {
    // safe: SAFE-01
    let home = tempfile::tempdir().unwrap();
    let mut core = CoreService::new(home.path(), false);
    with_stage_output(&mut core);

    let before = core.scene().revision;
    send(
        &mut core,
        command::Action::SendBoothMessage(SendBoothMessage {
            text: "Mic 2 is out".into(),
        }),
    )
    .unwrap();
    assert!(
        core.scene().revision > before,
        "the engine gets a new scene"
    );
    assert!(stage_text(&core).contains("From the booth: Mic 2 is out"));
    assert!(!audience_text(&core).contains("Mic 2"));
    // The stage shows the clock on a separate, time-varying layer.
    let stage = &core.scene().outputs[1];
    assert!(stage.layers.iter().any(|l| matches!(
        &l.content,
        Some(layer_spec::Content::Props(p)) if p.elements_json.contains("{clock}")
    )));

    // A layout that hides the booth message takes it off the stage.
    let id = selah_core::library::new_id();
    send(
        &mut core,
        command::Action::SaveStageLayout(SaveStageLayout {
            stage_layout: Some(StageLayout {
                id: Some(id.clone()),
                name: "Quiet".into(),
                show_clock: true,
                ..StageLayout::default()
            }),
        }),
    )
    .unwrap();
    let before = core.scene().revision;
    send(
        &mut core,
        command::Action::SetStageLayout(SetStageLayout {
            stage_layout_id: Some(id),
        }),
    )
    .unwrap();
    assert!(core.scene().revision > before);
    assert!(!stage_text(&core).contains("Mic 2"));
}

#[test]
fn stage_layouts_with_elements_off_the_screen_are_refused() {
    // sec: SEC-03
    let home = tempfile::tempdir().unwrap();
    let mut core = CoreService::new(home.path(), false);
    let bad = StageLayout {
        id: Some(selah_core::library::new_id()),
        name: "Broken".into(),
        elements: vec![StageElement {
            id: Some(selah_core::library::new_id()),
            kind: StageElementKind::Clock as i32,
            rect: Some(Rect {
                x: 0.9,
                y: 0.0,
                width: 0.6,
                height: 0.1,
            }),
            visible: true,
            font_scale: 1.0,
            show_chords: false,
        }],
        ..StageLayout::default()
    };
    assert!(
        send(
            &mut core,
            command::Action::SaveStageLayout(SaveStageLayout {
                stage_layout: Some(bad),
            }),
        )
        .is_err()
    );
}

#[test]
fn time_passes_for_running_timers_and_stays_quiet_otherwise() {
    let home = tempfile::tempdir().unwrap();
    let mut core = CoreService::new(home.path(), false);
    with_stage_output(&mut core);

    // Nothing counting: the tick does nothing at all.
    let idle = core.tick_live_clock(1_000, 1_700_000_000_000);
    assert!(idle.broadcast.is_empty() && idle.to_engine.is_none());

    let timer_id = selah_core::library::new_id();
    send(
        &mut core,
        command::Action::SaveTimer(SaveTimer {
            timer: Some(Timer {
                id: Some(timer_id.clone()),
                name: "Welcome".into(),
                kind: TimerKind::Countdown as i32,
                duration_ms: 3_000,
                overrun_mode: TimerOverrunMode::StopAtZero as i32,
                ..Default::default()
            }),
        }),
    )
    .unwrap();
    send(
        &mut core,
        command::Action::StartTimer(StartTimer {
            timer_id: Some(timer_id),
        }),
    )
    .unwrap();
    assert!(stage_text(&core).contains("{timer}"));

    let tick = core.tick_live_clock(1_000, 1_700_000_001_000);
    assert!(tick.reply.is_none());
    let status = tick
        .broadcast
        .iter()
        .find_map(|e| match &e.notification {
            Some(event::Notification::LiveStateChanged(c)) => c.status.first().cloned(),
            _ => None,
        })
        .expect("the UI hears about it");
    assert_eq!(status.automation[0].timers[0].remaining_ms, 2_000);
    assert!(
        tick.to_engine.is_none(),
        "the engine counts down on its own"
    );

    // Running out changes what the stage shows (stopped, red), so the engine gets a new scene.
    let done = core.tick_live_clock(2_500, 1_700_000_003_500);
    assert!(done.to_engine.is_some());
    assert!(!stage_text(&core).contains("{timer}"));
    assert!(stage_text(&core).contains("00:00"));
    // And then it is quiet again.
    assert!(
        core.tick_live_clock(1_000, 1_700_000_004_500)
            .broadcast
            .is_empty()
    );
}
