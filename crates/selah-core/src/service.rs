//! Core request handling: authorise, validate, apply, then tell the engine and subscribers.
use selah_proto::v1::{
    Actor, ActorOrigin, Envelope, Event, RenderCommand, Reply, Scene, Status, TestPattern, command,
    envelope, event, query, render_command, reply,
};

use crate::live::{LiveState, is_live};
use crate::permissions::{self, DENIED_CODE};
use crate::settings::SettingsStore;
use crate::update::{self, Release, UpdateError};

/// Error catalogue codes used by core (content/errors/errors.yaml).
pub mod codes {
    pub const UNSUPPORTED: &str = "SEL-IPC-0001";
    pub const INVALID: &str = "SEL-IPC-0002";
    pub const SETTING: &str = "SEL-SET-0001";
    pub const HELP_NOT_FOUND: &str = "SEL-HELP-0001";
}

/// Help results are capped (help.proto SearchHelp).
const HELP_LIMIT: usize = 10;

/// What core knows about the engine.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EngineView {
    pub connected: bool,
    pub scene_revision: u64,
    pub frame_checksum: Vec<u8>,
}

/// Slow work the host runs away from the request path (network, disk). When it's done, the host
/// calls the matching `finish_*` method, which builds the reply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Work {
    /// Read the update feed (M1.11). Only when the person asked and allowed it (SEC-13).
    CheckForUpdate { feed_url: String },
    /// Download and verify this release, then install it if nothing is on the screens.
    InstallUpdate { feed_url: String, release: Release },
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
    /// Run this, then call `finish_*` for the reply (`reply` is `None` meanwhile).
    pub work: Option<Work>,
}

pub struct CoreService {
    settings: SettingsStore,
    live: LiveState,
    pub engine: EngineView,
    safe_mode: bool,
    actor: Actor,
    /// kb.db, when it was found (KNOWLEDGE-BASE §6.4). Help queries answer empty without it.
    help: Option<selah_kb::HelpIndex>,
    /// The release the last update check offered.
    available_update: Option<Release>,
    /// The only way out to the network (SEC-13). Its policy follows the settings.
    net: std::sync::Arc<selah_net::Net>,
    /// Keeps the computer awake while live (SAFE-06).
    keep_awake: Box<dyn selah_runtime::power::KeepAwake>,
    awake: Option<bool>,
}

/// Used by [`CoreService::new`]: every request fails, so tests can never reach the network.
struct NoTransport;

impl selah_net::Transport for NoTransport {
    fn get(&self, _url: &str, _limit: u64) -> Result<Vec<u8>, selah_net::NetError> {
        Err(selah_net::NetError::Failed(
            "no network in this build".into(),
        ))
    }
}

impl CoreService {
    /// Core with no network and no keep-awake (tests). The host uses [`CoreService::with_platform`].
    pub fn new(config_dir: &std::path::Path, safe_mode: bool) -> Self {
        Self::with_platform(
            config_dir,
            safe_mode,
            std::sync::Arc::new(selah_net::Net::new(Box::new(NoTransport))),
            Box::new(selah_runtime::power::NoKeepAwake),
        )
    }

    pub fn with_platform(
        config_dir: &std::path::Path,
        safe_mode: bool,
        net: std::sync::Arc<selah_net::Net>,
        keep_awake: Box<dyn selah_runtime::power::KeepAwake>,
    ) -> Self {
        let mut service = Self {
            settings: SettingsStore::open(config_dir),
            live: LiveState::open(config_dir),
            engine: EngineView::default(),
            safe_mode,
            help: None,
            available_update: None,
            net,
            keep_awake,
            awake: None,
            actor: Actor {
                user_id: "selah".into(),
                device_id: "core".into(),
                origin: ActorOrigin::Core as i32,
            },
        };
        service.apply_settings_side_effects();
        // The live-state snapshot may say something is on the screens (core restarted).
        service.sync_keep_awake();
        service
    }

    /// The network gate, for slow work the host runs.
    pub fn net(&self) -> std::sync::Arc<selah_net::Net> {
        self.net.clone()
    }

    fn apply_settings_side_effects(&self) {
        self.net.set_policy(update::policy_for(self.settings.get()));
    }

    /// SAFE-06: the computer doesn't sleep while something is on the screens.
    fn sync_keep_awake(&mut self) {
        let live = self.is_live();
        if self.awake != Some(live) {
            self.awake = Some(live);
            self.keep_awake.set(live);
        }
    }

    pub fn actor(&self) -> &Actor {
        &self.actor
    }

    pub fn scene(&self) -> &Scene {
        self.live.scene()
    }

    /// Something other than nothing or the logo is on the screens (SAFE-06).
    pub fn is_live(&self) -> bool {
        is_live(self.live.scene())
    }

    pub fn settings(&self) -> &crate::settings::StoredSettings {
        self.settings.get()
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
        let outcome = self.handle_inner(request);
        self.sync_keep_awake();
        outcome
    }

    fn handle_inner(&mut self, request: &Envelope) -> Outcome {
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
                    ..Outcome::default()
                }
            }
            command::Action::ClearOutputs(_) => {
                self.live.change(|scene| scene.cleared = true);
                Outcome {
                    reply: ack(self),
                    to_engine: Some(self.resync_command()),
                    broadcast: vec![self.status_event()],
                    ..Outcome::default()
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
            command::Action::SetUpdateSettings(change) => {
                let feed = change.update_feed_url.trim().to_owned();
                let allow = change.allow_update_check;
                match self.settings.update(|s| {
                    s.update_feed_url = feed;
                    s.allow_update_check = allow;
                    Ok(())
                }) {
                    Ok(()) => {
                        self.apply_settings_side_effects();
                        Outcome {
                            reply: ack(self),
                            ..Outcome::default()
                        }
                    }
                    Err(_) => Outcome {
                        reply: Some(self.error(request, codes::SETTING, "error-invalid-setting")),
                        ..Outcome::default()
                    },
                }
            }
            command::Action::CheckForUpdate(_) => {
                // SEC-13: no connection unless the person turned checks on and gave a feed.
                let settings = self.settings.get();
                if !settings.allow_update_check || settings.update_feed_url.is_empty() {
                    return self.update_error(request, &UpdateError::Off);
                }
                Outcome {
                    work: Some(Work::CheckForUpdate {
                        feed_url: settings.update_feed_url.clone(),
                    }),
                    ..Outcome::default()
                }
            }
            command::Action::InstallUpdate(install) => {
                // SAFE-06, SEC-12: never while something is on the screens.
                if self.is_live() {
                    return self.update_error(request, &UpdateError::Live);
                }
                let settings = self.settings.get();
                if !settings.allow_update_check || settings.update_feed_url.is_empty() {
                    return self.update_error(request, &UpdateError::Off);
                }
                match self.available_update.as_ref() {
                    Some(release) if release.version.to_string() == install.version => Outcome {
                        work: Some(Work::InstallUpdate {
                            feed_url: settings.update_feed_url.clone(),
                            release: release.clone(),
                        }),
                        ..Outcome::default()
                    },
                    _ => Outcome {
                        reply: Some(self.error(request, codes::INVALID, "error-invalid-request")),
                        ..Outcome::default()
                    },
                }
            }
            // The watchdog owns safe mode; the UI sends this to the watchdog.
            command::Action::RestartNormally(_) => Outcome {
                reply: Some(self.error(request, codes::UNSUPPORTED, "error-unsupported-request")),
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
            Some(query::Request::SearchHelp(search)) => {
                let language = language_or_english(&search.language);
                let found = self
                    .help
                    .as_ref()
                    .map(|help| help.search(&search.query, language, &search.anchors, HELP_LIMIT));
                self.help_results(request, found)
            }
            Some(query::Request::HelpForContext(context)) => {
                let language = language_or_english(&context.language);
                let found = self
                    .help
                    .as_ref()
                    .map(|help| help.for_context(&context.anchors, language));
                self.help_results(request, found)
            }
            Some(query::Request::GetArticle(get)) => {
                let language = language_or_english(&get.language);
                match self
                    .help
                    .as_ref()
                    .map(|help| help.article(&get.id, language))
                {
                    Some(Ok(Some(entry))) => {
                        self.reply(request, reply::Result::Article(to_article(entry)))
                    }
                    Some(Err(error)) => {
                        tracing::warn!(%error, "help query failed");
                        self.error(request, codes::HELP_NOT_FOUND, "error-help-not-found")
                    }
                    _ => self.error(request, codes::HELP_NOT_FOUND, "error-help-not-found"),
                }
            }
            Some(query::Request::GetWatchdogStatus(_)) | None => {
                self.error(request, codes::UNSUPPORTED, "error-unsupported-request")
            }
        };
        Outcome {
            reply: Some(reply),
            ..Outcome::default()
        }
    }

    fn update_error(&self, request: &Envelope, error: &UpdateError) -> Outcome {
        tracing::warn!(%error, "update refused or failed");
        Outcome {
            reply: Some(self.error(request, error.code(), error.message_key())),
            ..Outcome::default()
        }
    }

    /// The reply to a finished update check.
    pub fn finish_update_check(
        &mut self,
        request: &Envelope,
        result: Result<Option<Release>, UpdateError>,
    ) -> Envelope {
        match result {
            Ok(found) => {
                let check = selah_proto::v1::UpdateCheck {
                    available: found.is_some(),
                    version: found
                        .as_ref()
                        .map(|r| r.version.to_string())
                        .unwrap_or_default(),
                    notes_markdown: found
                        .as_ref()
                        .map(|r| r.notes_markdown.clone())
                        .unwrap_or_default(),
                    current_version: selah_runtime::VERSION.into(),
                };
                tracing::info!(available = %check.version, "update check finished");
                self.available_update = found;
                self.reply(request, reply::Result::UpdateCheck(check))
            }
            Err(error) => {
                tracing::warn!(%error, "update check failed");
                self.error(request, error.code(), error.message_key())
            }
        }
    }

    /// The reply to a finished download: install it, unless something went on the screens
    /// while it downloaded (SAFE-06, SEC-12).
    pub fn finish_update_install(
        &mut self,
        request: &Envelope,
        downloaded: Result<std::path::PathBuf, UpdateError>,
        installer: Option<&dyn update::Install>,
    ) -> Envelope {
        let result = downloaded.and_then(|package| {
            if self.is_live() {
                return Err(UpdateError::Live);
            }
            let installer = installer
                .ok_or_else(|| UpdateError::Install("Selah wasn't installed by Setup".into()))?;
            installer.install(&package).map_err(UpdateError::Install)
        });
        match result {
            Ok(()) => {
                tracing::warn!("installing an update; Selah will restart");
                self.reply(request, reply::Result::Acknowledged(true))
            }
            Err(error) => {
                tracing::warn!(%error, "update not installed");
                self.error(request, error.code(), error.message_key())
            }
        }
    }

    /// Use this kb.db for help queries.
    pub fn set_help(&mut self, help: selah_kb::HelpIndex) {
        self.help = Some(help);
    }

    fn help_results(
        &self,
        request: &Envelope,
        found: Option<anyhow::Result<Vec<selah_kb::Entry>>>,
    ) -> Envelope {
        let articles = match found {
            Some(Ok(entries)) => entries.into_iter().map(to_article).collect(),
            Some(Err(error)) => {
                tracing::warn!(%error, "help query failed");
                Vec::new()
            }
            None => Vec::new(),
        };
        self.reply(
            request,
            reply::Result::HelpResults(selah_proto::v1::HelpResults { articles }),
        )
    }

    pub fn on_engine_connection(&mut self, connected: bool) -> Vec<Event> {
        self.engine.connected = connected;
        vec![self.status_event()]
    }

    /// Handle an event from the engine. Returns events for subscribers.
    pub fn on_engine_event(&mut self, event: &Event) -> Vec<Event> {
        let events = self.on_engine_event_inner(event);
        self.sync_keep_awake();
        events
    }

    fn on_engine_event_inner(&mut self, event: &Event) -> Vec<Event> {
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

fn language_or_english(language: &str) -> &str {
    if language.is_empty() { "en" } else { language }
}

fn to_article(entry: selah_kb::Entry) -> selah_proto::v1::Article {
    selah_proto::v1::Article {
        id: entry.id,
        language: entry.language,
        title: entry.title,
        summary: entry.summary,
        ast_json: entry.ast_json,
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

    fn command(action: command::Action) -> envelope::Body {
        envelope::Body::Command(Command {
            action: Some(action),
        })
    }

    fn error_code(outcome: &Outcome) -> String {
        match outcome.reply.as_ref().and_then(|r| r.body.as_ref()) {
            Some(envelope::Body::Error(error)) => error.code.clone(),
            other => panic!("expected an error, got {other:?}"),
        }
    }

    fn allow_updates(core: &mut CoreService) {
        let outcome = core.handle(&request(
            ActorOrigin::Ui,
            command(command::Action::SetUpdateSettings(
                selah_proto::v1::SetUpdateSettings {
                    update_feed_url: "https://updates.example.invalid/releases.dev.json".into(),
                    allow_update_check: true,
                },
            )),
        ));
        assert!(
            outcome
                .reply
                .is_some_and(|r| matches!(r.body, Some(envelope::Body::Reply(_))))
        );
    }

    fn release() -> Release {
        Release {
            version: semver::Version::new(9, 0, 0),
            file_name: "SelahApp-9.0.0-dev-full.nupkg".into(),
            sha256: [0; 32],
            size: 1,
            notes_markdown: String::new(),
        }
    }

    struct RecordingInstaller(std::sync::Mutex<Vec<std::path::PathBuf>>);

    impl update::Install for RecordingInstaller {
        fn install(&self, package: &std::path::Path) -> Result<(), String> {
            self.0.lock().unwrap().push(package.to_owned());
            Ok(())
        }
    }

    #[derive(Clone, Default)]
    struct RecordingKeepAwake(std::sync::Arc<std::sync::Mutex<Vec<bool>>>);

    impl selah_runtime::power::KeepAwake for RecordingKeepAwake {
        fn set(&mut self, awake: bool) {
            self.0.lock().unwrap().push(awake);
        }
    }

    #[test]
    fn the_computer_stays_awake_only_while_something_is_on_the_screens() {
        // safe: SAFE-06
        let dir = tempfile::tempdir().unwrap();
        let keep = RecordingKeepAwake::default();
        let net = std::sync::Arc::new(selah_net::Net::new(Box::new(NoTransport)));
        let mut core =
            CoreService::with_platform(dir.path(), false, net.clone(), Box::new(keep.clone()));
        core.handle(&request(ActorOrigin::Ui, show_bars()));
        core.handle(&request(ActorOrigin::Ui, show_bars()));
        core.handle(&request(
            ActorOrigin::Ui,
            command(command::Action::ClearOutputs(ClearOutputs {})),
        ));
        core.handle(&request(ActorOrigin::Ui, show_bars()));
        // The emergency lane clears the screens: sleep is allowed again.
        core.on_engine_event(&Event {
            notification: Some(event::Notification::EmergencyApplied(
                selah_proto::v1::EmergencyApplied {
                    show_logo: false,
                    scene_revision: 99,
                    monotonic_nanoseconds: 1,
                },
            )),
        });
        assert_eq!(
            *keep.0.lock().unwrap(),
            vec![false, true, false, true, false]
        );

        // After a restart with something still live, core keeps the computer awake at once.
        core.handle(&request(ActorOrigin::Ui, show_bars()));
        drop(core);
        let after = RecordingKeepAwake::default();
        let _core = CoreService::with_platform(dir.path(), false, net, Box::new(after.clone()));
        assert_eq!(*after.0.lock().unwrap(), vec![true]);
    }

    #[test]
    fn update_checks_are_off_at_default_settings() {
        // sec: SEC-13 (no outgoing connection unless the person allows it)
        let dir = tempfile::tempdir().unwrap();
        let mut core = CoreService::new(dir.path(), false);
        assert!(core.net().policy().is_offline());
        let outcome = core.handle(&request(
            ActorOrigin::Ui,
            command(command::Action::CheckForUpdate(Default::default())),
        ));
        assert!(outcome.work.is_none());
        assert_eq!(error_code(&outcome), update::codes::OFF);
        assert!(core.net().connections().is_empty());
        allow_updates(&mut core);
        assert!(core.net().policy().allows(selah_net::Purpose::UpdateCheck));
        let outcome = core.handle(&request(
            ActorOrigin::Ui,
            command(command::Action::CheckForUpdate(Default::default())),
        ));
        assert_eq!(
            outcome.work,
            Some(Work::CheckForUpdate {
                feed_url: "https://updates.example.invalid/releases.dev.json".into()
            })
        );
        assert!(outcome.reply.is_none());
    }

    #[test]
    fn a_plain_http_feed_is_refused() {
        // sec: SEC-12
        let dir = tempfile::tempdir().unwrap();
        let mut core = CoreService::new(dir.path(), false);
        let outcome = core.handle(&request(
            ActorOrigin::Ui,
            command(command::Action::SetUpdateSettings(
                selah_proto::v1::SetUpdateSettings {
                    update_feed_url: "http://updates.example.invalid/releases.dev.json".into(),
                    allow_update_check: true,
                },
            )),
        ));
        assert_eq!(error_code(&outcome), codes::SETTING);
        assert!(!core.settings().allow_update_check);
    }

    #[test]
    fn updates_never_install_while_something_is_on_the_screens() {
        // safe: SAFE-06; sec: SEC-12
        let dir = tempfile::tempdir().unwrap();
        let mut core = CoreService::new(dir.path(), false);
        allow_updates(&mut core);
        let check = request(
            ActorOrigin::Ui,
            command(command::Action::CheckForUpdate(Default::default())),
        );
        core.finish_update_check(&check, Ok(Some(release())));
        let install = || {
            request(
                ActorOrigin::Ui,
                command(command::Action::InstallUpdate(
                    selah_proto::v1::InstallUpdate {
                        version: "9.0.0".into(),
                    },
                )),
            )
        };

        // Live: refused before anything is downloaded.
        core.handle(&request(ActorOrigin::Ui, show_bars()));
        let outcome = core.handle(&install());
        assert!(outcome.work.is_none());
        assert_eq!(error_code(&outcome), update::codes::LIVE);

        // Cleared: the download may start...
        core.handle(&request(
            ActorOrigin::Ui,
            command(command::Action::ClearOutputs(ClearOutputs {})),
        ));
        let outcome = core.handle(&install());
        assert!(matches!(outcome.work, Some(Work::InstallUpdate { .. })));

        // ...but if something went on the screens meanwhile, it isn't installed.
        let installer = RecordingInstaller(Default::default());
        core.handle(&request(ActorOrigin::Ui, show_bars()));
        let reply =
            core.finish_update_install(&install(), Ok("pkg.nupkg".into()), Some(&installer));
        assert!(
            matches!(reply.body, Some(envelope::Body::Error(ref e)) if e.code == update::codes::LIVE)
        );
        assert!(installer.0.lock().unwrap().is_empty());

        core.handle(&request(
            ActorOrigin::Ui,
            command(command::Action::ClearOutputs(ClearOutputs {})),
        ));
        let reply =
            core.finish_update_install(&install(), Ok("pkg.nupkg".into()), Some(&installer));
        assert!(matches!(reply.body, Some(envelope::Body::Reply(_))));
        assert_eq!(installer.0.lock().unwrap().len(), 1);
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
