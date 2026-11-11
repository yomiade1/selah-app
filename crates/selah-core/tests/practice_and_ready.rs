//! M4.7: practice mode keeps the real screens as they were (SAFE-01), and Ready for Sunday runs
//! its checks for a saved service (M4-A8).
use selah_core::service::{CoreService, Outcome};
use selah_proto::v1::{
    Actor, ActorOrigin, Command, Envelope, LayerKind, LayerSpec, PolicyLevel, Query,
    ReadyCheckKind, ReadyCheckStatus, RenderCommand, RunReadyCheck, SaveService, SaveSong, Service,
    ServiceItem, ServiceItemKind, SetBoothLock, SetLayerState, SetPolicySetting, SetPracticeMode,
    SlideLayerSpec, Song, command, envelope, layer_spec, query, render_command, reply,
};

static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn actor() -> Actor {
    Actor {
        user_id: "operator".into(),
        device_id: "booth".into(),
        origin: ActorOrigin::Ui as i32,
    }
}

fn run(core: &mut CoreService, action: command::Action) -> Outcome {
    core.handle(&Envelope {
        id: NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        correlation_id: 0,
        actor: Some(actor()),
        body: Some(envelope::Body::Command(Command {
            undo_group: None,
            action: Some(action),
        })),
    })
}

fn error_code(outcome: &Outcome) -> Option<String> {
    match outcome.reply.as_ref()?.body.as_ref()? {
        envelope::Body::Error(e) => Some(e.code.clone()),
        _ => None,
    }
}

fn show_words(core: &mut CoreService, words: &str) -> Outcome {
    run(
        core,
        command::Action::SetLayerState(SetLayerState {
            layer: Some(LayerSpec {
                kind: LayerKind::Slide as i32,
                enabled: true,
                content: Some(layer_spec::Content::Slide(SlideLayerSpec {
                    text: words.into(),
                    ..SlideLayerSpec::default()
                })),
                ..LayerSpec::default()
            }),
        }),
    )
}

fn practice(core: &mut CoreService, on: bool) -> Outcome {
    run(
        core,
        command::Action::SetPracticeMode(SetPracticeMode { on }),
    )
}

fn scene_words(command: &RenderCommand) -> (bool, String) {
    match command.action.as_ref() {
        Some(render_command::Action::SetScene(scene)) => (false, format!("{scene:?}")),
        Some(render_command::Action::SetPreviewScene(p)) => {
            (true, format!("{:?}", p.scene.as_ref().expect("scene")))
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn practice_mode_sends_changes_to_the_preview_and_puts_everything_back() {
    // safe: SAFE-01
    let home = tempfile::tempdir().unwrap();
    let mut core = CoreService::new(home.path(), false);
    let before = show_words(&mut core, "Amazing grace");
    let (preview, words) = scene_words(before.to_engine.as_ref().unwrap());
    assert!(!preview && words.contains("Amazing grace"));

    let on = practice(&mut core, true);
    assert!(
        on.to_engine.is_none(),
        "starting practice changes nothing on the screens"
    );
    assert!(core.is_practicing());

    let practising = show_words(&mut core, "Practice words");
    let (preview, words) = scene_words(practising.to_engine.as_ref().unwrap());
    assert!(preview, "practice goes to the preview window only");
    assert!(words.contains("Practice words") && words.contains("is_preview: true"));
    assert!(format!("{:?}", core.screen_scene()).contains("Amazing grace"));
    assert!(core.is_live(), "the screens still show the song");
    // A reconnecting engine gets the real screens plus the practice preview.
    let resync = core.resync_commands();
    let shown: Vec<(bool, String)> = resync
        .iter()
        .filter(|c| {
            matches!(
                c.action,
                Some(
                    render_command::Action::SetScene(_)
                        | render_command::Action::SetPreviewScene(_)
                )
            )
        })
        .map(scene_words)
        .collect();
    assert!(shown.iter().any(|(p, w)| !p && w.contains("Amazing grace")));
    assert!(
        shown
            .iter()
            .any(|(p, w)| *p && w.contains("Practice words"))
    );

    // A restart mid-practice never brings practice to the screens: nothing was saved.
    let restarted = CoreService::new(home.path(), false);
    assert!(format!("{:?}", restarted.scene()).contains("Amazing grace"));
    assert!(!format!("{:?}", restarted.scene()).contains("Practice words"));
    drop(restarted);

    let practice_revision = core.scene().revision;
    let off = practice(&mut core, false);
    let (preview, words) = scene_words(off.to_engine.as_ref().unwrap());
    assert!(!preview && words.contains("Amazing grace") && !words.contains("Practice words"));
    assert!(core.scene().revision > practice_revision);
    assert!(!core.is_practicing());
}

#[test]
fn volunteers_may_practise_but_booth_lock_refuses_it() {
    let home = tempfile::tempdir().unwrap();
    let mut core = CoreService::new(home.path(), false);
    run(
        &mut core,
        command::Action::SetBoothLock(SetBoothLock { locked: true }),
    );
    assert_eq!(
        error_code(&practice(&mut core, true)).as_deref(),
        Some("SEL-SEC-0001")
    );
    run(
        &mut core,
        command::Action::SetBoothLock(SetBoothLock { locked: false }),
    );
    run(
        &mut core,
        command::Action::SetRoleLock(selah_proto::v1::SetRoleLock {
            locked: true,
            pin: "2468".into(),
        }),
    );
    assert!(core.is_role_locked());
    assert_eq!(error_code(&practice(&mut core, true)), None);
    assert_eq!(error_code(&practice(&mut core, false)), None);
    assert!(
        core.is_role_locked(),
        "leaving practice keeps the role lock"
    );
}

fn ready(
    core: &mut CoreService,
    service: &Service,
) -> Result<selah_proto::v1::ReadyReport, String> {
    let outcome = core.handle(&Envelope {
        id: NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        correlation_id: 0,
        actor: Some(actor()),
        body: Some(envelope::Body::Query(Query {
            request: Some(query::Request::RunReadyCheck(RunReadyCheck {
                service_id: service.id.clone(),
            })),
        })),
    });
    match outcome.reply.expect("reply").body {
        Some(envelope::Body::Reply(r)) => match r.result {
            Some(reply::Result::ReadyReport(report)) => Ok(report),
            other => panic!("unexpected {other:?}"),
        },
        Some(envelope::Body::Error(e)) => Err(e.code),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn ready_for_sunday_reports_every_check_with_fix_articles() {
    let home = tempfile::tempdir().unwrap();
    let mut core = CoreService::new(home.path(), false);
    let song_id = selah_core::library::new_id();
    run(
        &mut core,
        command::Action::SaveSong(SaveSong {
            song: Some(Song {
                id: Some(song_id.clone()),
                title: "New Song".into(),
                ..Song::default()
            }),
        }),
    );
    let service = Service {
        id: Some(selah_core::library::new_id()),
        name: "Sunday".into(),
        items: vec![ServiceItem {
            id: Some(selah_core::library::new_id()),
            kind: ServiceItemKind::Song as i32,
            content_id: Some(song_id),
            title: "New Song".into(),
            ..ServiceItem::default()
        }],
        ..Service::default()
    };
    run(
        &mut core,
        command::Action::SaveService(SaveService {
            service: Some(service.clone()),
        }),
    );

    let report = ready(&mut core, &service).unwrap();
    for kind in [
        ReadyCheckKind::Media,
        ReadyCheckKind::Fonts,
        ReadyCheckKind::TextFits,
        ReadyCheckKind::Copyright,
        ReadyCheckKind::Outputs,
        ReadyCheckKind::Backup,
        ReadyCheckKind::Flash,
        ReadyCheckKind::Sound,
    ] {
        assert!(
            report.checks.iter().any(|c| c.kind == kind as i32),
            "{kind:?} is checked"
        );
    }
    for check in &report.checks {
        if check.status != ReadyCheckStatus::Pass as i32 {
            assert!(!check.fix_article_id.is_empty(), "{check:?} links to a fix");
        }
    }
    // No engine in this test: the screens check fails, so the service isn't ready.
    assert!(!report.ready);
    let codes: Vec<&str> = report.checks.iter().map(|c| c.code.as_str()).collect();
    assert!(codes.contains(&"SEL-RDY-0004") && codes.contains(&"SEL-RDY-0005"));

    run(
        &mut core,
        command::Action::SetPolicySetting(SetPolicySetting {
            key: selah_core::ready::CCLI_LICENCE_KEY.into(),
            value_json: "\"1234567\"".into(),
            level: PolicyLevel::Machine as i32,
            locked: false,
        }),
    );
    let report = ready(&mut core, &service).unwrap();
    assert!(!report.checks.iter().any(|c| c.code == "SEL-RDY-0005"));

    let missing = Service {
        id: Some(selah_core::library::new_id()),
        ..Service::default()
    };
    assert_eq!(ready(&mut core, &missing), Err("SEL-LIB-0001".into()));
}
