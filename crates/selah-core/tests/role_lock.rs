//! M4.5 / M4-A6 / M4-A12 (SEC-14, SAFE-01, SAFE-06): the Simple-mode role lock and the booth lock
//! are core's own state. What the client says about itself never matters.
use std::io::Write;
use std::sync::{Arc, Mutex};

use selah_core::service::CoreService;
use selah_proto::v1::{
    Actor, ActorOrigin, ClearLayer, ClearOutputs, Command, Envelope, LayerKind, Look, NextSlide,
    PolicyLevel, PreviousSlide, SaveLook, SendBoothMessage, SetBoothLock, SetLook,
    SetPolicySetting, SetRoleLock, ShowLogoCommand, TriggerSectionHotkey, command, envelope, query,
    reply,
};

static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn send(
    core: &mut CoreService,
    user_id: &str,
    action: command::Action,
) -> Result<reply::Result, String> {
    let request = Envelope {
        id: NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        correlation_id: 0,
        actor: Some(Actor {
            user_id: user_id.into(),
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

fn live_role_locked(core: &mut CoreService) -> bool {
    let request = Envelope {
        id: NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        correlation_id: 0,
        actor: Some(Actor {
            user_id: "operator".into(),
            device_id: "booth".into(),
            origin: ActorOrigin::Ui as i32,
        }),
        body: Some(envelope::Body::Query(selah_proto::v1::Query {
            request: Some(query::Request::GetLiveState(Default::default())),
        })),
    };
    match core.handle(&request).reply.expect("reply").body {
        Some(envelope::Body::Reply(selah_proto::v1::Reply {
            result: Some(reply::Result::LiveStatus(status)),
            ..
        })) => status.automation.first().is_some_and(|a| a.role_locked),
        other => panic!("unexpected {other:?}"),
    }
}

fn lock(pin: &str) -> command::Action {
    command::Action::SetRoleLock(SetRoleLock {
        locked: true,
        pin: pin.into(),
    })
}

fn unlock(pin: &str) -> command::Action {
    command::Action::SetRoleLock(SetRoleLock {
        locked: false,
        pin: pin.into(),
    })
}

fn producer_commands(look: &Look) -> Vec<command::Action> {
    vec![
        command::Action::SetLook(SetLook {
            look_id: look.id.clone(),
        }),
        command::Action::SaveLook(SaveLook {
            look: Some(look.clone()),
        }),
        command::Action::SetPolicySetting(SetPolicySetting {
            key: "templates.default.song".into(),
            value_json: "\"x\"".into(),
            level: PolicyLevel::User as i32,
            locked: false,
        }),
    ]
}

fn simple_commands() -> Vec<command::Action> {
    vec![
        command::Action::NextSlide(NextSlide {}),
        command::Action::PreviousSlide(PreviousSlide {}),
        command::Action::TriggerSectionHotkey(TriggerSectionHotkey { hotkey: "1".into() }),
        command::Action::ClearLayer(ClearLayer {
            kind: LayerKind::Slide as i32,
        }),
        command::Action::ShowLogo(ShowLogoCommand { show_logo: true }),
        command::Action::ClearOutputs(ClearOutputs {}),
        command::Action::SendBoothMessage(SendBoothMessage {
            text: "Mic 2 is out".into(),
        }),
    ]
}

/// Commands that fail for reasons other than permission (nothing loaded) still prove they got
/// past authorisation: only SEL-SEC-0001 means "refused".
fn allowed(result: &Result<reply::Result, String>) -> bool {
    !matches!(result, Err(code) if code == "SEL-SEC-0001")
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_role_locked_volunteer_cannot_run_producer_commands_whatever_the_client_claims() {
    // sec: SEC-14, SAFE-01
    let home = tempfile::tempdir().unwrap();
    let mut core = CoreService::new(home.path(), false);
    let look = Look {
        id: Some(selah_core::library::new_id()),
        name: "Worship".into(),
        ..Look::default()
    };
    send(
        &mut core,
        "operator",
        command::Action::SaveLook(SaveLook {
            look: Some(look.clone()),
        }),
    )
    .unwrap();
    assert!(!live_role_locked(&mut core));

    // A bad PIN never locks.
    assert_eq!(
        send(&mut core, "operator", lock("12")),
        Err("SEL-SEC-0004".into())
    );
    send(&mut core, "operator", lock("2468")).unwrap();
    assert!(core.is_role_locked());
    assert!(live_role_locked(&mut core));

    let logs = Capture::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer({
            let logs = logs.clone();
            move || logs.clone()
        })
        .with_ansi(false)
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        // Producer commands are refused for every user id the client might claim.
        for user_id in ["operator", "admin", "producer", "local-owner"] {
            for action in producer_commands(&look) {
                assert_eq!(
                    send(&mut core, user_id, action.clone()),
                    Err("SEL-SEC-0001".into()),
                    "{user_id} {action:?}"
                );
            }
        }
        // Simple-mode commands still work.
        for action in simple_commands() {
            let result = send(&mut core, "operator", action.clone());
            assert!(allowed(&result), "{action:?} → {result:?}");
        }
        // The lock's own setting can't be changed around it.
        let bypass = command::Action::SetPolicySetting(SetPolicySetting {
            key: selah_core::role_lock::POLICY_KEY.into(),
            value_json: "false".into(),
            level: PolicyLevel::Machine as i32,
            locked: false,
        });
        assert_eq!(
            send(&mut core, "operator", bypass),
            Err("SEL-SEC-0001".into())
        );
    });
    // M4-A12: core logs each refusal.
    let text = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(text.matches("command refused").count() >= 12, "{text}");
    assert!(text.contains("role_locked=true"), "{text}");

    // A wrong PIN doesn't unlock; the right one does.
    assert_eq!(
        send(&mut core, "operator", unlock("1357")),
        Err("SEL-SEC-0002".into())
    );
    assert!(core.is_role_locked());
    send(&mut core, "operator", unlock("2468")).unwrap();
    assert!(!core.is_role_locked());
    assert!(allowed(&send(
        &mut core,
        "operator",
        producer_commands(&look).remove(0)
    )));

    // Even unlocked, nobody sets the lock's key directly (only SetRoleLock may).
    let sneak = command::Action::SetPolicySetting(SetPolicySetting {
        key: selah_core::role_lock::POLICY_KEY.into(),
        value_json: "true".into(),
        level: PolicyLevel::Machine as i32,
        locked: false,
    });
    assert_eq!(
        send(&mut core, "operator", sneak),
        Err("SEL-SEC-0001".into())
    );
}

#[test]
fn five_wrong_pins_make_core_wait_and_the_lock_survives_a_restart() {
    // sec: SEC-14
    let home = tempfile::tempdir().unwrap();
    {
        let mut core = CoreService::new(home.path(), false);
        send(&mut core, "operator", lock("8080")).unwrap();
        for _ in 0..4 {
            assert_eq!(
                send(&mut core, "operator", unlock("0000")),
                Err("SEL-SEC-0002".into())
            );
        }
        assert_eq!(
            send(&mut core, "operator", unlock("0000")),
            Err("SEL-SEC-0003".into())
        );
        // Waiting: even the right PIN is refused for now.
        assert_eq!(
            send(&mut core, "operator", unlock("8080")),
            Err("SEL-SEC-0003".into())
        );
        assert!(core.is_role_locked());
    }
    // Core restarts (crash, update, reboot): the lock is still on.
    let mut core = CoreService::new(home.path(), false);
    assert!(core.is_role_locked());
    assert!(live_role_locked(&mut core));
    let look = Look {
        id: Some(selah_core::library::new_id()),
        name: "Sermon".into(),
        ..Look::default()
    };
    assert_eq!(
        send(
            &mut core,
            "operator",
            command::Action::SaveLook(SaveLook { look: Some(look) })
        ),
        Err("SEL-SEC-0001".into())
    );
}

#[test]
fn booth_lock_leaves_only_next_previous_and_clear() {
    // safe: SAFE-06
    let home = tempfile::tempdir().unwrap();
    let mut core = CoreService::new(home.path(), false);
    let look = Look {
        id: Some(selah_core::library::new_id()),
        name: "Camera".into(),
        ..Look::default()
    };
    send(
        &mut core,
        "operator",
        command::Action::SaveLook(SaveLook {
            look: Some(look.clone()),
        }),
    )
    .unwrap();
    send(
        &mut core,
        "operator",
        command::Action::SetBoothLock(SetBoothLock { locked: true }),
    )
    .unwrap();
    for action in [
        command::Action::NextSlide(NextSlide {}),
        command::Action::PreviousSlide(PreviousSlide {}),
        command::Action::ClearOutputs(ClearOutputs {}),
        command::Action::ClearLayer(ClearLayer {
            kind: LayerKind::Media as i32,
        }),
    ] {
        let result = send(&mut core, "operator", action.clone());
        assert!(allowed(&result), "{action:?} → {result:?}");
    }
    for action in producer_commands(&look).into_iter().chain([
        command::Action::TriggerSectionHotkey(TriggerSectionHotkey { hotkey: "2".into() }),
        command::Action::SendBoothMessage(SendBoothMessage { text: "hi".into() }),
        lock("2468"),
    ]) {
        assert_eq!(
            send(&mut core, "operator", action.clone()),
            Err("SEL-SEC-0001".into()),
            "{action:?}"
        );
    }
    send(
        &mut core,
        "operator",
        command::Action::SetBoothLock(SetBoothLock { locked: false }),
    )
    .unwrap();
    assert!(allowed(&send(
        &mut core,
        "operator",
        producer_commands(&look).remove(0)
    )));
}
