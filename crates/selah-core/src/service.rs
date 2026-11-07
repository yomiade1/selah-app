//! Core request handling: authorise, validate, apply, then tell the engine and subscribers.
use selah_proto::v1::{
    Actor, ActorOrigin, Envelope, Event, RenderCommand, Reply, Scene, Status, TestPattern, command,
    envelope, event, query, render_command, reply,
};

use crate::live::{LiveState, is_live};
use crate::permissions::{self, DENIED_CODE};
use crate::settings::SettingsStore;

/// Error catalogue codes used by core (content/errors/errors.yaml).
pub mod codes {
    pub const UNSUPPORTED: &str = "SEL-IPC-0001";
    pub const INVALID: &str = "SEL-IPC-0002";
    pub const SETTING: &str = "SEL-SET-0001";
}

/// What core knows about the engine.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EngineView {
    pub connected: bool,
    pub scene_revision: u64,
    pub frame_checksum: Vec<u8>,
}

/// The result of handling one request.
#[derive(Debug, Default)]
pub struct Outcome {
    pub reply: Option<Envelope>,
    /// Send this to the engine.
    pub to_engine: Option<RenderCommand>,
    /// Send these to every subscriber.
    pub broadcast: Vec<Event>,
    /// The requester asked for events.
    pub subscribe: bool,
}

pub struct CoreService {
    settings: SettingsStore,
    live: LiveState,
    pub engine: EngineView,
    safe_mode: bool,
    actor: Actor,
}

impl CoreService {
    pub fn new(config_dir: &std::path::Path, safe_mode: bool) -> Self {
        Self {
            settings: SettingsStore::open(config_dir),
            live: LiveState::open(config_dir),
            engine: EngineView::default(),
            safe_mode,
            actor: Actor {
                user_id: "selah".into(),
                device_id: "core".into(),
                origin: ActorOrigin::Core as i32,
            },
        }
    }

    pub fn actor(&self) -> &Actor {
        &self.actor
    }

    pub fn scene(&self) -> &Scene {
        self.live.scene()
    }

    pub fn status(&self) -> Status {
        Status {
            connected: true,
            safe_mode: self.safe_mode,
            scene_revision: self.live.scene().revision,
            engine_connected: self.engine.connected,
            engine_scene_revision: self.engine.scene_revision,
            frame_checksum: self.engine.frame_checksum.clone(),
            version: selah_runtime::VERSION.into(),
            live: is_live(self.live.scene()),
        }
    }

    fn status_event(&self) -> Event {
        Event {
            notification: Some(event::Notification::Status(self.status())),
        }
    }

    /// The command that brings a (re)connected engine up to date.
    pub fn resync_command(&self) -> RenderCommand {
        RenderCommand {
            action: Some(render_command::Action::SetScene(self.live.scene().clone())),
        }
    }

    fn reply(&self, request: &Envelope, result: reply::Result) -> Envelope {
        Envelope {
            id: request.id,
            correlation_id: request.id,
            actor: Some(self.actor.clone()),
            body: Some(envelope::Body::Reply(Reply {
                result: Some(result),
            })),
        }
    }

    fn error(&self, request: &Envelope, code: &str, key: &str) -> Envelope {
        Envelope {
            id: request.id,
            correlation_id: request.id,
            actor: Some(self.actor.clone()),
            body: Some(envelope::Body::Error(selah_proto::v1::Error {
                code: code.into(),
                message_key: key.into(),
                ..Default::default()
            })),
        }
    }

    pub fn handle(&mut self, request: &Envelope) -> Outcome {
        match request.body.as_ref() {
            Some(envelope::Body::Command(command)) => match command.action.as_ref() {
                Some(action) => self.command(request, action),
                None => Outcome {
                    reply: Some(self.error(request, codes::INVALID, "error-invalid-request")),
                    ..Outcome::default()
                },
            },
            Some(envelope::Body::Query(query)) => self.query(request, query.request.as_ref()),
            // Clients don't send events, replies or errors to core; ignore them.
            _ => Outcome::default(),
        }
    }

    fn command(&mut self, request: &Envelope, action: &command::Action) -> Outcome {
        let kind = permissions::kind_of(action);
        let actor = request.actor.clone().unwrap_or_default();
        // SEC-14: authorise before validating or applying anything.
        if let Err(denied) = permissions::authorize(&actor, kind) {
            tracing::warn!(origin = ?denied.origin, kind = ?denied.kind, "command refused");
            return Outcome {
                reply: Some(self.error(request, DENIED_CODE, "error-not-permitted")),
                ..Outcome::default()
            };
        }
        let ack = |service: &Self| Some(service.reply(request, reply::Result::Acknowledged(true)));
        match action {
            command::Action::ShowTestPattern(show) => {
                let pattern =
                    TestPattern::try_from(show.pattern).unwrap_or(TestPattern::Unspecified);
                if pattern == TestPattern::Unspecified {
                    return Outcome {
                        reply: Some(self.error(request, codes::INVALID, "error-invalid-request")),
                        ..Outcome::default()
                    };
                }
                self.live.change(|scene| {
                    scene.cleared = false;
                    scene.test_pattern = pattern as i32;
                    scene.text.clear();
                });
                Outcome {
                    reply: ack(self),
                    to_engine: Some(self.resync_command()),
                    broadcast: vec![self.status_event()],
                    subscribe: false,
                }
            }
            command::Action::ClearOutputs(_) => {
                self.live.change(|scene| scene.cleared = true);
                Outcome {
                    reply: ack(self),
                    to_engine: Some(self.resync_command()),
                    broadcast: vec![self.status_event()],
                    subscribe: false,
                }
            }
            command::Action::SetAppearance(change) => {
                let Some(appearance) = change.appearance.as_ref() else {
                    return Outcome {
                        reply: Some(self.error(request, codes::INVALID, "error-invalid-request")),
                        ..Outcome::default()
                    };
                };
                match self.settings.update(|s| s.apply_appearance(appearance)) {
                    Ok(()) => Outcome {
                        reply: ack(self),
                        ..Outcome::default()
                    },
                    Err(_) => Outcome {
                        reply: Some(self.error(request, codes::SETTING, "error-invalid-setting")),
                        ..Outcome::default()
                    },
                }
            }
            // Stored from M2 (help feedback lives in library.db).
            command::Action::RecordFeedback(_) => Outcome {
                reply: ack(self),
                ..Outcome::default()
            },
            // Denied above for every client; kept for exhaustiveness.
            command::Action::Render(_) => Outcome {
                reply: Some(self.error(request, DENIED_CODE, "error-not-permitted")),
                ..Outcome::default()
            },
        }
    }

    fn query(&mut self, request: &Envelope, query: Option<&query::Request>) -> Outcome {
        let reply = match query {
            Some(query::Request::GetStatus(_)) => {
                self.reply(request, reply::Result::Status(self.status()))
            }
            Some(query::Request::GetSettings(_)) => self.reply(
                request,
                reply::Result::Settings(self.settings.get().to_proto()),
            ),
            Some(query::Request::Subscribe(_)) => {
                return Outcome {
                    reply: Some(self.reply(request, reply::Result::Status(self.status()))),
                    subscribe: true,
                    ..Outcome::default()
                };
            }
            // Help queries are answered by the host, which owns kb.db (M1.9).
            Some(query::Request::SearchHelp(_))
            | Some(query::Request::GetArticle(_))
            | Some(query::Request::HelpForContext(_))
            | Some(query::Request::GetWatchdogStatus(_))
            | None => self.error(request, codes::UNSUPPORTED, "error-unsupported-request"),
        };
        Outcome {
            reply: Some(reply),
            ..Outcome::default()
        }
    }

    pub fn on_engine_connection(&mut self, connected: bool) -> Vec<Event> {
        self.engine.connected = connected;
        vec![self.status_event()]
    }

    /// Handle an event from the engine. Returns events for subscribers.
    pub fn on_engine_event(&mut self, event: &Event) -> Vec<Event> {
        match event.notification.as_ref() {
            Some(event::Notification::FrameShown(frame)) => {
                let changed = self.engine.frame_checksum != frame.frame_checksum
                    || self.engine.scene_revision != frame.scene_revision;
                self.engine.scene_revision = frame.scene_revision;
                self.engine.frame_checksum = frame.frame_checksum.clone();
                let mut out = vec![event.clone()];
                if changed {
                    out.push(self.status_event());
                }
                out
            }
            Some(event::Notification::EmergencyApplied(applied)) => {
                // SAFE-03: the engine already acted. Record it and make sure our own next
                // revision is above the engine's floor so stale scenes can't come back.
                tracing::warn!(
                    show_logo = applied.show_logo,
                    "emergency lane changed the screens"
                );
                self.live.raise_revision_above(applied.scene_revision);
                let show_logo = applied.show_logo;
                self.live.change(|scene| {
                    scene.cleared = true;
                    scene.test_pattern = TestPattern::Unspecified as i32;
                    scene.text = if show_logo {
                        "logo".into()
                    } else {
                        String::new()
                    };
                });
                vec![event.clone(), self.status_event()]
            }
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use selah_proto::v1::{ClearOutputs, Command, GetStatus, Query, ShowTestPattern};

    fn request(origin: ActorOrigin, body: envelope::Body) -> Envelope {
        Envelope {
            id: 7,
            correlation_id: 0,
            actor: Some(Actor {
                user_id: "local-owner".into(),
                device_id: "test".into(),
                origin: origin as i32,
            }),
            body: Some(body),
        }
    }

    fn show_bars() -> envelope::Body {
        envelope::Body::Command(Command {
            action: Some(command::Action::ShowTestPattern(ShowTestPattern {
                pattern: TestPattern::SmpteBars as i32,
            })),
        })
    }

    #[test]
    fn show_test_pattern_updates_live_state_and_tells_the_engine() {
        let dir = tempfile::tempdir().unwrap();
        let mut core = CoreService::new(dir.path(), false);
        let outcome = core.handle(&request(ActorOrigin::Ui, show_bars()));
        let Some(render_command::Action::SetScene(scene)) =
            outcome.to_engine.and_then(|c| c.action)
        else {
            panic!("no scene for the engine");
        };
        assert_eq!(scene.test_pattern, TestPattern::SmpteBars as i32);
        assert!(core.status().live);
        assert_eq!(outcome.reply.unwrap().correlation_id, 7);
    }

    #[test]
    fn refused_commands_change_nothing() {
        // sec: SEC-14, SAFE-01
        let dir = tempfile::tempdir().unwrap();
        let mut core = CoreService::new(dir.path(), false);
        let outcome = core.handle(&request(ActorOrigin::Api, show_bars()));
        assert!(outcome.to_engine.is_none());
        let Some(envelope::Body::Error(error)) = outcome.reply.unwrap().body else {
            panic!("expected an error reply");
        };
        assert_eq!(error.code, DENIED_CODE);
        assert!(!core.status().live);
        assert_eq!(core.scene().revision, 0);
    }

    #[test]
    fn emergency_clear_raises_the_revision_above_the_engines_floor() {
        // safe: SAFE-03
        let dir = tempfile::tempdir().unwrap();
        let mut core = CoreService::new(dir.path(), false);
        core.handle(&request(ActorOrigin::Ui, show_bars()));
        let floor = 50;
        core.on_engine_event(&Event {
            notification: Some(event::Notification::EmergencyApplied(
                selah_proto::v1::EmergencyApplied {
                    show_logo: false,
                    scene_revision: floor,
                    monotonic_nanoseconds: 1,
                },
            )),
        });
        assert!(core.scene().revision > floor);
        assert!(!core.status().live);
    }

    #[test]
    fn status_and_clear_work_and_unknown_queries_get_a_catalogue_error() {
        let dir = tempfile::tempdir().unwrap();
        let mut core = CoreService::new(dir.path(), true);
        let status = core.handle(&request(
            ActorOrigin::Ui,
            envelope::Body::Query(Query {
                request: Some(query::Request::GetStatus(GetStatus {})),
            }),
        ));
        let Some(envelope::Body::Reply(Reply {
            result: Some(reply::Result::Status(status)),
        })) = status.reply.unwrap().body
        else {
            panic!()
        };
        assert!(status.safe_mode);
        let cleared = core.handle(&request(
            ActorOrigin::Ui,
            envelope::Body::Command(Command {
                action: Some(command::Action::ClearOutputs(ClearOutputs {})),
            }),
        ));
        assert!(cleared.to_engine.is_some());
        let unknown = core.handle(&request(
            ActorOrigin::Ui,
            envelope::Body::Query(Query { request: None }),
        ));
        let Some(envelope::Body::Error(error)) = unknown.reply.unwrap().body else {
            panic!()
        };
        assert_eq!(error.code, codes::UNSUPPORTED);
    }
}
