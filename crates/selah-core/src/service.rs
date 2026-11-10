//! Core request handling: authorise, validate, apply, then tell the engine and subscribers.
use prost::Message;
use selah_proto::v1::{
    Actor, ActorOrigin, BibleFormat, Command, CopyrightPlacement, Envelope, Event,
    GetPolicySettingReply, LibraryChanged, MediaItemList, MediaKind, ParsePastedSongReply,
    ParseScriptureReferenceReply, PolicySetting, PresentationList, RenderCommand, Reply, Scene,
    SearchScope, Song, SongList, SongPasteFormat, Status, TagList, TemplateList, TestPattern,
    UndoStateChanged, command, envelope, event, query, render_command, reply,
};

use crate::library::{
    CommandBusError, CommandMeta, Document, DocumentKind, ExecutionOutcome, Library, new_id,
    validate_id,
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
    pub const LIB_NOT_FOUND: &str = "SEL-LIB-0001";
    pub const LIB_INVALID: &str = "SEL-LIB-0002";
    pub const SETTING_LOCKED: &str = "SEL-LIB-0003";
    pub const NOTHING_TO_UNDO: &str = "SEL-LIB-0004";
}

fn fix_article_for(code: &str) -> &'static str {
    match code {
        "SEL-SEC-0001" => "troubleshooting.you-dont-have-permission",
        "SEL-IPC-0001" | "SEL-IPC-0002" | "SEL-SET-0001" | "SEL-HELP-0001" => {
            "troubleshooting.selah-cant-do-that"
        }
        "SEL-UPD-0001" | "SEL-UPD-0002" | "SEL-UPD-0003" | "SEL-UPD-0004" | "SEL-UPD-0005" => {
            "getting-started.update-selah"
        }
        "SEL-SUP-0001" => "getting-started.report-a-problem",
        "SEL-DIAG-0001" | "SEL-DIAG-0002" => "getting-started.run-diagnostics",
        "SEL-LIB-0001" => "troubleshooting.library-item-not-found",
        "SEL-LIB-0002" => "troubleshooting.cant-save-library-item",
        "SEL-LIB-0003" => "troubleshooting.setting-is-locked",
        "SEL-LIB-0004" => "troubleshooting.nothing-to-undo",
        _ => "",
    }
}

fn validate_song_preview(song: &Song) -> anyhow::Result<()> {
    let mut copy = song.clone();
    if copy.id.is_none() {
        copy.id = Some(new_id());
    }
    if copy.title.trim().is_empty() {
        copy.title = "Draft Song".into();
    }
    for (idx, sec) in copy.sections.iter_mut().enumerate() {
        if sec.id.is_none() {
            sec.id = Some(new_id());
        }
        if sec.label.trim().is_empty() {
            sec.label = format!("Section {}", idx + 1);
        }
    }
    for (idx, arr) in copy.arrangements.iter_mut().enumerate() {
        if arr.id.is_none() {
            arr.id = Some(new_id());
        }
        if arr.name.trim().is_empty() {
            arr.name = format!("Arrangement {}", idx + 1);
        }
    }
    copy.validate()
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
    /// Help ▸ Report a problem: save a redacted support bundle (M1.13).
    CreateSupportBundle,
    /// Help ▸ Diagnostics: ask the engine for its details, then reply.
    GetDiagnostics,
    /// Help ▸ Diagnostics ▸ Run benchmark.
    RunDiagnostics { quick: bool },
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
    library: Library,
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
        Self::open_with_platform(
            config_dir,
            &config_dir.join("library"),
            safe_mode,
            net,
            keep_awake,
        )
        .expect("open core library")
    }

    pub fn open_with_platform(
        config_dir: &std::path::Path,
        library_dir: &std::path::Path,
        safe_mode: bool,
        net: std::sync::Arc<selah_net::Net>,
        keep_awake: Box<dyn selah_runtime::power::KeepAwake>,
    ) -> anyhow::Result<Self> {
        let library = Library::open(library_dir)?;
        let mut service = Self {
            settings: SettingsStore::open(config_dir),
            live: LiveState::open(config_dir),
            library,
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
        Ok(service)
    }

    pub fn library(&self) -> &Library {
        &self.library
    }

    pub fn library_mut(&mut self) -> &mut Library {
        &mut self.library
    }

    pub fn ensure_bundled_bibles(&self) -> anyhow::Result<bool> {
        crate::scripture::ensure_bundled_bibles(&self.library.paths().bibles)
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
            ..Status::default()
        }
    }

    pub fn safe_mode(&self) -> bool {
        self.safe_mode
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
                fix_article_id: fix_article_for(code).into(),
                ..Default::default()
            })),
        }
    }

    fn bus_error(&self, request: &Envelope, error: &CommandBusError) -> Envelope {
        tracing::warn!(%error, code = error.code(), "library request failed");
        Envelope {
            id: request.id,
            correlation_id: request.id,
            actor: Some(self.actor.clone()),
            body: Some(envelope::Body::Error(selah_proto::v1::Error {
                code: error.code().into(),
                message_key: error.message_key().into(),
                fix_article_id: error.fix_article_id().into(),
                ..Default::default()
            })),
        }
    }

    fn library_outcome(
        &mut self,
        request: &Envelope,
        result: Result<ExecutionOutcome, CommandBusError>,
    ) -> Outcome {
        match result {
            Ok(ExecutionOutcome::Replayed(body)) => Outcome {
                reply: Some(Envelope {
                    id: request.id,
                    correlation_id: request.id,
                    actor: Some(self.actor.clone()),
                    body: Some(body),
                }),
                ..Outcome::default()
            },
            Ok(ExecutionOutcome::Applied {
                reply,
                changes,
                undo_doc_id,
            }) => {
                let mut broadcast = Vec::new();
                if !changes.is_empty() {
                    for change in &changes {
                        broadcast.push(Event {
                            notification: Some(event::Notification::LibraryChanged(
                                LibraryChanged {
                                    doc_type: change.doc_type.to_proto() as i32,
                                    doc_id: Some(change.doc_id.clone()),
                                    change: change.change as i32,
                                    op_seq: change.op_seq,
                                    undo_group: Some(change.undo_group.clone()),
                                    from_undo_redo: change.from_undo_redo,
                                },
                            )),
                        });
                    }
                    let single_doc = undo_doc_id.as_ref().or_else(|| {
                        let first = &changes[0].doc_id;
                        changes.iter().all(|c| c.doc_id == *first).then_some(first)
                    });
                    let global = self.library.undo_state(None).ok();
                    let document = single_doc.and_then(|id| self.library.undo_state(Some(id)).ok());
                    broadcast.push(Event {
                        notification: Some(event::Notification::UndoStateChanged(
                            UndoStateChanged {
                                global,
                                doc_id: single_doc.cloned(),
                                document,
                            },
                        )),
                    });
                }
                Outcome {
                    reply: Some(self.reply(request, reply)),
                    broadcast,
                    ..Outcome::default()
                }
            }
            Err(error) => Outcome {
                reply: Some(self.bus_error(request, &error)),
                ..Outcome::default()
            },
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
                Some(action) => self.command(request, command, action),
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

    fn command(
        &mut self,
        request: &Envelope,
        command: &Command,
        action: &command::Action,
    ) -> Outcome {
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
        let command_hash: [u8; 32] = *blake3::hash(&command.encode_to_vec()).as_bytes();
        let meta = CommandMeta {
            actor: &actor,
            envelope_id: request.id,
            command_hash,
            undo_group: command.undo_group.as_ref(),
        };
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
            command::Action::RecordFeedback(feedback) => {
                let result =
                    self.library
                        .record_feedback_op(&feedback.article_id, feedback.helpful, &meta);
                self.library_outcome(request, result)
            }
            command::Action::SaveSong(cmd) => {
                let result = match cmd.song.as_ref() {
                    Some(doc) => self.library.save_document_op(doc, &meta),
                    None => Err(CommandBusError::Invalid("missing song".into())),
                };
                self.library_outcome(request, result)
            }
            command::Action::DeleteSong(cmd) => {
                let result = match cmd.id.as_ref() {
                    Some(id) => self
                        .library
                        .delete_document_op(DocumentKind::Song, id, &meta),
                    None => Err(CommandBusError::Invalid("missing song id".into())),
                };
                self.library_outcome(request, result)
            }
            command::Action::SaveTemplate(cmd) => {
                let result = match cmd.template.as_ref() {
                    Some(doc) => self.library.save_document_op(doc, &meta),
                    None => Err(CommandBusError::Invalid("missing template".into())),
                };
                self.library_outcome(request, result)
            }
            command::Action::DeleteTemplate(cmd) => {
                let result = match cmd.id.as_ref() {
                    Some(id) => self
                        .library
                        .delete_document_op(DocumentKind::Template, id, &meta),
                    None => Err(CommandBusError::Invalid("missing template id".into())),
                };
                self.library_outcome(request, result)
            }
            command::Action::SaveMediaItem(cmd) => {
                let result = match cmd.media_item.as_ref() {
                    Some(doc) => self.library.save_document_op(doc, &meta),
                    None => Err(CommandBusError::Invalid("missing media item".into())),
                };
                self.library_outcome(request, result)
            }
            command::Action::DeleteMediaItem(cmd) => {
                let result = match cmd.id.as_ref() {
                    Some(id) => self
                        .library
                        .delete_document_op(DocumentKind::Media, id, &meta),
                    None => Err(CommandBusError::Invalid("missing media item id".into())),
                };
                self.library_outcome(request, result)
            }
            command::Action::SavePresentation(cmd) => {
                let result = match cmd.presentation.as_ref() {
                    Some(doc) => self.library.save_document_op(doc, &meta),
                    None => Err(CommandBusError::Invalid("missing presentation".into())),
                };
                self.library_outcome(request, result)
            }
            command::Action::DeletePresentation(cmd) => {
                let result = match cmd.id.as_ref() {
                    Some(id) => {
                        self.library
                            .delete_document_op(DocumentKind::Presentation, id, &meta)
                    }
                    None => Err(CommandBusError::Invalid("missing presentation id".into())),
                };
                self.library_outcome(request, result)
            }
            command::Action::SaveTag(cmd) => {
                let result = match cmd.tag.as_ref() {
                    Some(doc) => self.library.save_document_op(doc, &meta),
                    None => Err(CommandBusError::Invalid("missing tag".into())),
                };
                self.library_outcome(request, result)
            }
            command::Action::DeleteTag(cmd) => {
                let result = match cmd.id.as_ref() {
                    Some(id) => self
                        .library
                        .delete_document_op(DocumentKind::Tag, id, &meta),
                    None => Err(CommandBusError::Invalid("missing tag id".into())),
                };
                self.library_outcome(request, result)
            }
            command::Action::Undo(cmd) => {
                let result = self.library.undo_op(cmd.doc_id.as_ref(), &meta);
                self.library_outcome(request, result)
            }
            command::Action::Redo(cmd) => {
                let result = self.library.redo_op(cmd.doc_id.as_ref(), &meta);
                self.library_outcome(request, result)
            }
            command::Action::SaveEditorBuffer(cmd) => {
                let result = match cmd.doc_id.as_ref() {
                    Some(id) => {
                        self.library
                            .save_editor_buffer_op(id, cmd.doc_type, &cmd.content, &meta)
                    }
                    None => Err(CommandBusError::Invalid("missing document id".into())),
                };
                self.library_outcome(request, result)
            }
            command::Action::DiscardEditorBuffer(cmd) => {
                let result = match cmd.doc_id.as_ref() {
                    Some(id) => self.library.discard_editor_buffer_op(id, &meta),
                    None => Err(CommandBusError::Invalid("missing document id".into())),
                };
                self.library_outcome(request, result)
            }
            command::Action::SetPolicySetting(cmd) => {
                let result = self.library.set_policy_setting_op(
                    &cmd.key,
                    &cmd.value_json,
                    cmd.level,
                    cmd.locked,
                    &meta,
                );
                self.library_outcome(request, result)
            }
            command::Action::RemovePolicySetting(cmd) => {
                let result = self
                    .library
                    .remove_policy_setting_op(&cmd.key, cmd.level, &meta);
                self.library_outcome(request, result)
            }
            command::Action::ImportBible(cmd) => {
                let format = match BibleFormat::try_from(cmd.format) {
                    Ok(fmt) => fmt,
                    Err(_) => {
                        return Outcome {
                            reply: Some(self.bus_error(
                                request,
                                &CommandBusError::Invalid("unknown Bible format".into()),
                            )),
                            ..Outcome::default()
                        };
                    }
                };
                let req = crate::scripture::BibleImportRequest {
                    source_path: &cmd.source_path,
                    raw_bytes: &cmd.raw_bytes,
                    format,
                    code: &cmd.code,
                    name: &cmd.name,
                    language: &cmd.language,
                    replace_existing: cmd.replace_existing,
                };
                match crate::scripture::import_bible(&self.library, &req) {
                    Ok(translation) => Outcome {
                        reply: Some(
                            self.reply(request, reply::Result::BibleTranslation(translation)),
                        ),
                        ..Outcome::default()
                    },
                    Err(error) => Outcome {
                        reply: Some(self.bus_error(request, &error)),
                        ..Outcome::default()
                    },
                }
            }
            command::Action::DeleteBible(cmd) => {
                match crate::scripture::delete_bible(&self.library, &cmd.code) {
                    Ok(_) => Outcome {
                        reply: ack(self),
                        ..Outcome::default()
                    },
                    Err(error) => Outcome {
                        reply: Some(self.bus_error(request, &error)),
                        ..Outcome::default()
                    },
                }
            }
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
            command::Action::CreateSupportBundle(_) => Outcome {
                work: Some(Work::CreateSupportBundle),
                ..Outcome::default()
            },
            command::Action::RunDiagnostics(run) => {
                // The benchmark loads the GPU and CPU: never during a service (SAFE-06).
                if self.is_live() {
                    let error = crate::diagnostics::DiagnosticsError::Live;
                    return Outcome {
                        reply: Some(self.error(request, error.code(), error.message_key())),
                        ..Outcome::default()
                    };
                }
                Outcome {
                    work: Some(Work::RunDiagnostics { quick: run.quick }),
                    ..Outcome::default()
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
            Some(query::Request::GetDiagnostics(_)) => {
                return Outcome {
                    work: Some(Work::GetDiagnostics),
                    ..Outcome::default()
                };
            }
            Some(query::Request::GetSong(get)) => {
                self.get_document(request, get.id.as_ref(), reply::Result::Song)
            }
            Some(query::Request::ListSongs(list)) => {
                self.list_documents(request, list.limit, list.offset, |songs| {
                    reply::Result::SongList(SongList { songs })
                })
            }
            Some(query::Request::GetTemplate(get)) => {
                self.get_document(request, get.id.as_ref(), reply::Result::Template)
            }
            Some(query::Request::ListTemplates(list)) => {
                self.list_documents(request, list.limit, list.offset, |templates| {
                    reply::Result::TemplateList(TemplateList { templates })
                })
            }
            Some(query::Request::GetMediaItem(get)) => {
                self.get_document(request, get.id.as_ref(), reply::Result::MediaItem)
            }
            Some(query::Request::ListMediaItems(list)) => {
                self.list_documents(request, list.limit, list.offset, |media_items| {
                    reply::Result::MediaItemList(MediaItemList { media_items })
                })
            }
            Some(query::Request::GetPresentation(get)) => {
                self.get_document(request, get.id.as_ref(), reply::Result::Presentation)
            }
            Some(query::Request::ListPresentations(list)) => {
                self.list_documents(request, list.limit, list.offset, |presentations| {
                    reply::Result::PresentationList(PresentationList { presentations })
                })
            }
            Some(query::Request::GetTag(get)) => {
                self.get_document(request, get.id.as_ref(), reply::Result::Tag)
            }
            Some(query::Request::ListTags(list)) => {
                self.list_documents(request, list.limit, list.offset, |tags| {
                    reply::Result::TagList(TagList { tags })
                })
            }
            Some(query::Request::GetUndoState(q)) => {
                match self.library.undo_state(q.doc_id.as_ref()) {
                    Ok(state) => self.reply(request, reply::Result::UndoState(state)),
                    Err(error) => self.bus_error(request, &error),
                }
            }
            Some(query::Request::GetEditorBuffer(q)) => match q.doc_id.as_ref() {
                Some(id) => match self.library.get_editor_buffer(id) {
                    Ok(reply_msg) => self.reply(request, reply::Result::EditorBuffer(reply_msg)),
                    Err(error) => self.bus_error(request, &error),
                },
                None => self.bus_error(
                    request,
                    &CommandBusError::Invalid("missing document id".into()),
                ),
            },
            Some(query::Request::ListEditorBuffers(q)) => {
                match self.library.list_editor_buffers(q.limit, q.offset) {
                    Ok(list) => self.reply(request, reply::Result::EditorBufferList(list)),
                    Err(error) => self.bus_error(request, &error),
                }
            }
            Some(query::Request::ListOps(q)) => {
                match self
                    .library
                    .list_ops(q.after_seq, q.limit, q.doc_id.as_ref())
                {
                    Ok(list) => self.reply(request, reply::Result::OpList(list)),
                    Err(error) => self.bus_error(request, &error),
                }
            }
            Some(query::Request::GetPolicySetting(q)) => match self.library.setting(&q.key) {
                Ok(Some(effective)) => self.reply(
                    request,
                    reply::Result::PolicySetting(GetPolicySettingReply {
                        found: true,
                        setting: Some(PolicySetting {
                            key: q.key.clone(),
                            value_json: serde_json::to_string(&effective.value).unwrap_or_default(),
                            level: effective.level.to_proto() as i32,
                            locked: effective.locked,
                        }),
                    }),
                ),
                Ok(None) => self.reply(
                    request,
                    reply::Result::PolicySetting(GetPolicySettingReply {
                        found: false,
                        setting: None,
                    }),
                ),
                Err(error) => self.bus_error(request, &CommandBusError::Invalid(error.to_string())),
            },
            Some(query::Request::SearchSongs(q)) => {
                match self.library.search_songs(&q.query, q.limit, q.offset) {
                    Ok(results) => self.reply(request, reply::Result::SongSearchResults(results)),
                    Err(error) => self.bus_error(request, &error),
                }
            }
            Some(query::Request::SearchMedia(q)) => match MediaKind::try_from(q.kind) {
                Ok(kind) => match self.library.search_media(&q.query, kind, q.limit, q.offset) {
                    Ok(results) => self.reply(request, reply::Result::MediaSearchResults(results)),
                    Err(error) => self.bus_error(request, &error),
                },
                Err(_) => self.bus_error(
                    request,
                    &CommandBusError::Invalid("unknown media kind".into()),
                ),
            },
            Some(query::Request::SearchScripture(q)) => {
                match self
                    .library
                    .search_scripture(&q.query, &q.translation, q.limit, q.offset)
                {
                    Ok(results) => {
                        self.reply(request, reply::Result::ScriptureSearchResults(results))
                    }
                    Err(error) => self.bus_error(request, &error),
                }
            }
            Some(query::Request::SearchLibrary(q)) => match SearchScope::try_from(q.scope) {
                Ok(scope) => {
                    match self
                        .library
                        .search_library(&q.query, scope, &q.translation, q.limit)
                    {
                        Ok(results) => {
                            self.reply(request, reply::Result::LibrarySearchResults(results))
                        }
                        Err(error) => self.bus_error(request, &error),
                    }
                }
                Err(_) => self.bus_error(
                    request,
                    &CommandBusError::Invalid("unknown search scope".into()),
                ),
            },
            Some(query::Request::ParsePastedSong(q)) => match SongPasteFormat::try_from(q.format) {
                Ok(format) => {
                    match crate::songs::parse_song(&q.raw_text, format, &q.default_title) {
                        Ok(parsed) => self.reply(
                            request,
                            reply::Result::ParsedSong(ParsePastedSongReply {
                                song: Some(parsed.song),
                                detected_format: parsed.detected_format as i32,
                                warnings: parsed.warnings,
                            }),
                        ),
                        Err(error) => {
                            self.bus_error(request, &CommandBusError::Invalid(error.to_string()))
                        }
                    }
                }
                Err(_) => self.bus_error(
                    request,
                    &CommandBusError::Invalid("unknown paste format".into()),
                ),
            },
            Some(query::Request::TransposeSong(q)) => {
                let Some(mut song) = q.song.clone() else {
                    return Outcome {
                        reply: Some(
                            self.bus_error(
                                request,
                                &CommandBusError::Invalid("missing song".into()),
                            ),
                        ),
                        ..Outcome::default()
                    };
                };
                let prefer_flats =
                    match selah_proto::v1::AccidentalSpelling::try_from(q.accidental_spelling) {
                        Ok(selah_proto::v1::AccidentalSpelling::Unspecified) => None,
                        Ok(selah_proto::v1::AccidentalSpelling::Sharps) => Some(false),
                        Ok(selah_proto::v1::AccidentalSpelling::Flats) => Some(true),
                        Err(_) => {
                            return Outcome {
                                reply: Some(self.bus_error(
                                    request,
                                    &CommandBusError::Invalid("unknown accidental spelling".into()),
                                )),
                                ..Outcome::default()
                            };
                        }
                    };
                match validate_song_preview(&song).and_then(|()| {
                    crate::songs::transpose_song(&mut song, q.semitones, prefer_flats)
                }) {
                    Ok(()) => self.reply(request, reply::Result::Song(song)),
                    Err(error) => {
                        self.bus_error(request, &CommandBusError::Invalid(error.to_string()))
                    }
                }
            }
            Some(query::Request::SplitSongSlides(q)) => {
                let Some(song) = q.song.as_ref() else {
                    return Outcome {
                        reply: Some(
                            self.bus_error(
                                request,
                                &CommandBusError::Invalid("missing song".into()),
                            ),
                        ),
                        ..Outcome::default()
                    };
                };
                let placement = match CopyrightPlacement::try_from(q.copyright_placement) {
                    Ok(p) => p,
                    Err(_) => {
                        return Outcome {
                            reply: Some(self.bus_error(
                                request,
                                &CommandBusError::Invalid("unknown copyright placement".into()),
                            )),
                            ..Outcome::default()
                        };
                    }
                };
                let options = crate::songs::SplitOptions {
                    arrangement_id: q.arrangement_id.clone(),
                    max_lines: q.max_lines,
                    copyright_placement: placement,
                    title_slide: q.title_slide,
                    blank_slide_at_end: q.blank_slide_at_end,
                    translation_language: (!q.translation_language.trim().is_empty())
                        .then(|| q.translation_language.clone()),
                };
                match validate_song_preview(song)
                    .and_then(|()| crate::songs::generate_slides(song, &options))
                {
                    Ok(slides) => self.reply(request, reply::Result::GeneratedSongSlides(slides)),
                    Err(error) => {
                        self.bus_error(request, &CommandBusError::Invalid(error.to_string()))
                    }
                }
            }
            Some(query::Request::ListBibles(q)) => {
                match crate::scripture::list_bibles(&self.library, q.include_books) {
                    Ok(list) => self.reply(request, reply::Result::BibleList(list)),
                    Err(error) => self.bus_error(request, &error),
                }
            }
            Some(query::Request::ParseScriptureReference(q)) => {
                let hint = (!q.language_hint.trim().is_empty()).then_some(q.language_hint.as_str());
                match crate::scripture::parse_reference(&q.reference, hint) {
                    Ok(parsed) => self.reply(
                        request,
                        reply::Result::ParsedScriptureReference(ParseScriptureReferenceReply {
                            spans: parsed.spans,
                            normalized: parsed.normalized,
                        }),
                    ),
                    Err(error) => {
                        self.bus_error(request, &CommandBusError::Invalid(error.to_string()))
                    }
                }
            }
            Some(query::Request::GetScripturePassages(q)) => {
                match crate::scripture::get_passages(
                    &self.library,
                    &q.translation,
                    &q.reference,
                    &q.spans,
                    &q.language_hint,
                ) {
                    Ok(passages) => self.reply(request, reply::Result::ScripturePassages(passages)),
                    Err(error) => self.bus_error(request, &error),
                }
            }
            Some(query::Request::SplitScriptureSlides(q)) => {
                match crate::scripture::split_scripture_slides(q) {
                    Ok(slides) => {
                        self.reply(request, reply::Result::GeneratedScriptureSlides(slides))
                    }
                    Err(error) => {
                        self.bus_error(request, &CommandBusError::Invalid(error.to_string()))
                    }
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

    fn get_document<T: crate::library::Document>(
        &self,
        request: &Envelope,
        id: Option<&selah_proto::v1::Id>,
        wrap: fn(T) -> reply::Result,
    ) -> Envelope {
        let Some(id) = id else {
            return self.bus_error(request, &CommandBusError::Invalid("missing id".into()));
        };
        if let Err(error) = validate_id(id) {
            return self.bus_error(request, &CommandBusError::Invalid(error.to_string()));
        }
        match self.library.get::<T>(id) {
            Ok(Some(doc)) => self.reply(request, wrap(doc)),
            Ok(None) => self.bus_error(
                request,
                &CommandBusError::NotFound("library item not found".into()),
            ),
            Err(error) => self.bus_error(request, &CommandBusError::Invalid(error.to_string())),
        }
    }

    fn list_documents<T: crate::library::Document>(
        &self,
        request: &Envelope,
        limit: u32,
        offset: u32,
        wrap: impl FnOnce(Vec<T>) -> reply::Result,
    ) -> Envelope {
        match self.library.list::<T>(limit, offset) {
            Ok(items) => self.reply(request, wrap(items)),
            Err(error) => self.bus_error(request, &CommandBusError::Invalid(error.to_string())),
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

    /// The reply to Help ▸ Report a problem.
    pub fn finish_support_bundle(
        &self,
        request: &Envelope,
        saved: std::io::Result<std::path::PathBuf>,
    ) -> Envelope {
        match saved {
            Ok(path) => self.reply(
                request,
                reply::Result::SupportBundle(selah_proto::v1::SupportBundle {
                    path: path.to_string_lossy().into_owned(),
                    report_url: crate::support::report_url(selah_runtime::VERSION),
                }),
            ),
            Err(error) => {
                tracing::warn!(%error, "support bundle not saved");
                self.error(
                    request,
                    crate::diagnostics::codes::BUNDLE,
                    "error-support-bundle",
                )
            }
        }
    }

    /// What Help ▸ Diagnostics shows. `engine` is the engine's own status, if it answered.
    pub fn diagnostics(&self, engine: Option<&Status>) -> selah_proto::v1::Diagnostics {
        selah_proto::v1::Diagnostics {
            version: selah_runtime::VERSION.into(),
            os: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
            safe_mode: self.safe_mode,
            engine_connected: engine.is_some(),
            renderer: engine.map(|s| s.renderer.clone()).unwrap_or_default(),
            output_resolution: engine.and_then(|s| s.output_resolution),
        }
    }

    pub fn finish_diagnostics(&self, request: &Envelope, engine: Option<&Status>) -> Envelope {
        self.reply(
            request,
            reply::Result::Diagnostics(self.diagnostics(engine)),
        )
    }

    /// The reply to Run benchmark.
    pub fn finish_run_diagnostics(
        &self,
        request: &Envelope,
        result: Result<
            (std::path::PathBuf, crate::diagnostics::Summary),
            crate::diagnostics::DiagnosticsError,
        >,
    ) -> Envelope {
        match result {
            Ok((path, summary)) => self.reply(
                request,
                reply::Result::DiagnosticsReport(selah_proto::v1::DiagnosticsReport {
                    path: path.to_string_lossy().into_owned(),
                    ipc_p99_ms: summary.ipc_p99_ms,
                    ipc_pass: summary.ipc_pass,
                    video_clip: summary.video_clip,
                    video_decode: summary.video_decode,
                    video_frames: summary.video_frames,
                    video_dropped: summary.video_dropped,
                    video_composite_p99_ms: summary.video_composite_p99_ms,
                    video_pass: summary.video_pass,
                }),
            ),
            Err(error) => {
                tracing::warn!(%error, "diagnostics failed");
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
            ..Default::default()
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
            ..Default::default()
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
                ..Default::default()
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

    fn song_fixture(title: &str) -> selah_proto::v1::Song {
        let sec_id = crate::library::new_id();
        selah_proto::v1::Song {
            id: Some(crate::library::new_id()),
            title: title.into(),
            authors: vec!["Writer".into()],
            tags: vec!["Hymn".into()],
            public_domain: true,
            sections: vec![selah_proto::v1::Section {
                id: Some(sec_id),
                label: "Verse 1".into(),
                kind: selah_proto::v1::SectionKind::Verse as i32,
                lines: vec![selah_proto::v1::LyricLine {
                    text: "Grace how sweet the sound".into(),
                    chords: vec![],
                }],
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    fn request_with_id(id: u64, origin: ActorOrigin, body: envelope::Body) -> Envelope {
        Envelope {
            id,
            correlation_id: 0,
            actor: Some(Actor {
                user_id: "local-owner".into(),
                device_id: "ui-1".into(),
                origin: origin as i32,
            }),
            body: Some(body),
        }
    }

    fn command_with_group(
        action: command::Action,
        undo_group: Option<selah_proto::v1::Id>,
    ) -> envelope::Body {
        envelope::Body::Command(Command {
            action: Some(action),
            undo_group,
        })
    }

    #[test]
    fn library_commands_emit_events_undo_redo_and_replay_idempotently() {
        let dir = tempfile::tempdir().unwrap();
        let mut core = CoreService::new(dir.path(), false);
        let song = song_fixture("Amazing Grace");
        let song_id = song.id.clone().unwrap();

        let save_req = request_with_id(
            101,
            ActorOrigin::Ui,
            command_with_group(
                command::Action::SaveSong(selah_proto::v1::SaveSong {
                    song: Some(song.clone()),
                }),
                None,
            ),
        );
        let save_out = core.handle(&save_req);
        assert!(matches!(
            save_out.reply.as_ref().and_then(|r| r.body.as_ref()),
            Some(envelope::Body::Reply(Reply {
                result: Some(reply::Result::Acknowledged(true))
            }))
        ));
        assert_eq!(save_out.broadcast.len(), 2);
        assert!(matches!(
            save_out.broadcast[0].notification,
            Some(event::Notification::LibraryChanged(ref ev))
                if ev.doc_id.as_ref() == Some(&song_id)
                    && ev.change == selah_proto::v1::LibraryChangeKind::Saved as i32
                    && !ev.from_undo_redo
        ));
        assert!(matches!(
            save_out.broadcast[1].notification,
            Some(event::Notification::UndoStateChanged(ref ev))
                if ev.global.as_ref().is_some_and(|s| s.can_undo && !s.can_redo)
                    && ev.doc_id.as_ref() == Some(&song_id)
        ));

        // Exact retry returns the cached reply without emitting duplicate subscriber events.
        let replay = core.handle(&save_req);
        assert!(replay.broadcast.is_empty());
        assert_eq!(replay.reply.as_ref().unwrap().correlation_id, 101);
        assert!(matches!(
            replay.reply.as_ref().and_then(|r| r.body.as_ref()),
            Some(envelope::Body::Reply(Reply {
                result: Some(reply::Result::Acknowledged(true))
            }))
        ));

        // Reusing the same envelope id with different command bytes returns SEL-IPC-0002.
        let other_song = song_fixture("Different Song");
        let conflict = core.handle(&request_with_id(
            101,
            ActorOrigin::Ui,
            command_with_group(
                command::Action::SaveSong(selah_proto::v1::SaveSong {
                    song: Some(other_song),
                }),
                None,
            ),
        ));
        assert_eq!(error_code(&conflict), codes::INVALID);

        // Undo removes the song and emits from_undo_redo=true.
        let undo_out = core.handle(&request_with_id(
            102,
            ActorOrigin::Ui,
            command_with_group(
                command::Action::Undo(selah_proto::v1::Undo { doc_id: None }),
                None,
            ),
        ));
        assert!(matches!(
            undo_out.broadcast[0].notification,
            Some(event::Notification::LibraryChanged(ref ev))
                if ev.change == selah_proto::v1::LibraryChangeKind::Deleted as i32
                    && ev.from_undo_redo
        ));
        let get_after_undo = core.handle(&request_with_id(
            103,
            ActorOrigin::Ui,
            envelope::Body::Query(Query {
                request: Some(query::Request::GetSong(selah_proto::v1::GetSong {
                    id: Some(song_id.clone()),
                })),
            }),
        ));
        assert_eq!(error_code(&get_after_undo), codes::LIB_NOT_FOUND);

        // Redo brings the song back.
        let redo_out = core.handle(&request_with_id(
            104,
            ActorOrigin::Ui,
            command_with_group(
                command::Action::Redo(selah_proto::v1::Redo { doc_id: None }),
                None,
            ),
        ));
        assert!(matches!(
            redo_out.broadcast[0].notification,
            Some(event::Notification::LibraryChanged(ref ev))
                if ev.change == selah_proto::v1::LibraryChangeKind::Saved as i32
                    && ev.from_undo_redo
        ));
        let get_after_redo = core.handle(&request_with_id(
            105,
            ActorOrigin::Ui,
            envelope::Body::Query(Query {
                request: Some(query::Request::GetSong(selah_proto::v1::GetSong {
                    id: Some(song_id.clone()),
                })),
            }),
        ));
        assert!(matches!(
            get_after_redo.reply.and_then(|r| r.body),
            Some(envelope::Body::Reply(Reply {
                result: Some(reply::Result::Song(s))
            })) if s == song
        ));

        // Op log holds save + undo + redo (3 ops) and ignores the replayed command.
        let ops_reply = core.handle(&request_with_id(
            106,
            ActorOrigin::Ui,
            envelope::Body::Query(Query {
                request: Some(query::Request::ListOps(selah_proto::v1::ListOps {
                    after_seq: 0,
                    limit: 10,
                    doc_id: Some(song_id),
                })),
            }),
        ));
        let Some(envelope::Body::Reply(Reply {
            result: Some(reply::Result::OpList(list)),
        })) = ops_reply.reply.and_then(|r| r.body)
        else {
            panic!("expected op list");
        };
        assert_eq!(list.ops.len(), 3);
    }

    #[test]
    fn unauthorized_library_commands_are_refused_before_touching_database() {
        // sec: SEC-14
        let dir = tempfile::tempdir().unwrap();
        let mut core = CoreService::new(dir.path(), false);
        let song = song_fixture("Refused Song");

        for origin in [
            ActorOrigin::Unspecified,
            ActorOrigin::Api,
            ActorOrigin::Midi,
            ActorOrigin::Macro,
            ActorOrigin::Timeline,
            ActorOrigin::Sync,
            ActorOrigin::Core,
            ActorOrigin::Engine,
            ActorOrigin::Watchdog,
        ] {
            let outcome = core.handle(&request_with_id(
                200,
                origin,
                command_with_group(
                    command::Action::SaveSong(selah_proto::v1::SaveSong {
                        song: Some(song.clone()),
                    }),
                    None,
                ),
            ));
            assert_eq!(error_code(&outcome), DENIED_CODE);
            assert!(outcome.broadcast.is_empty());
        }

        // Database and op log remain empty.
        assert!(
            core.library()
                .list::<selah_proto::v1::Song>(10, 0)
                .unwrap()
                .is_empty()
        );
        assert!(core.library().list_ops(0, 10, None).unwrap().ops.is_empty());

        // A subsequent authorized request using the same id (for UI origin) still applies cleanly.
        let allowed = core.handle(&request_with_id(
            200,
            ActorOrigin::Ui,
            command_with_group(
                command::Action::SaveSong(selah_proto::v1::SaveSong { song: Some(song) }),
                None,
            ),
        ));
        assert_eq!(allowed.broadcast.len(), 2);
    }

    #[test]
    fn editor_buffers_policies_and_help_feedback_round_trip_through_core_service() {
        let dir = tempfile::tempdir().unwrap();
        let mut core = CoreService::new(dir.path(), false);
        let doc_id = crate::library::new_id();

        let save_buf = core.handle(&request_with_id(
            301,
            ActorOrigin::Ui,
            command_with_group(
                command::Action::SaveEditorBuffer(selah_proto::v1::SaveEditorBuffer {
                    doc_id: Some(doc_id.clone()),
                    doc_type: selah_proto::v1::DocumentType::Song as i32,
                    content: "Verse 1\nHow great Thou art".into(),
                }),
                None,
            ),
        ));
        assert!(save_buf.broadcast.is_empty());

        let got_buf = core.handle(&request_with_id(
            302,
            ActorOrigin::Ui,
            envelope::Body::Query(Query {
                request: Some(query::Request::GetEditorBuffer(
                    selah_proto::v1::GetEditorBuffer {
                        doc_id: Some(doc_id.clone()),
                    },
                )),
            }),
        ));
        assert!(matches!(
            got_buf.reply.and_then(|r| r.body),
            Some(envelope::Body::Reply(Reply {
                result: Some(reply::Result::EditorBuffer(b))
            })) if b.found && b.buffer.as_ref().unwrap().content == "Verse 1\nHow great Thou art"
        ));

        let discard_buf = core.handle(&request_with_id(
            303,
            ActorOrigin::Ui,
            command_with_group(
                command::Action::DiscardEditorBuffer(selah_proto::v1::DiscardEditorBuffer {
                    doc_id: Some(doc_id.clone()),
                }),
                None,
            ),
        ));
        assert!(matches!(
            discard_buf.reply.and_then(|r| r.body),
            Some(envelope::Body::Reply(Reply {
                result: Some(reply::Result::Acknowledged(true))
            }))
        ));

        core.handle(&request_with_id(
            304,
            ActorOrigin::Ui,
            command_with_group(
                command::Action::SetPolicySetting(selah_proto::v1::SetPolicySetting {
                    key: "appearance.text-size".into(),
                    value_json: "130".into(),
                    level: selah_proto::v1::PolicyLevel::Machine as i32,
                    locked: true,
                }),
                None,
            ),
        ));
        let user_override = core.handle(&request_with_id(
            305,
            ActorOrigin::Ui,
            command_with_group(
                command::Action::SetPolicySetting(selah_proto::v1::SetPolicySetting {
                    key: "appearance.text-size".into(),
                    value_json: "100".into(),
                    level: selah_proto::v1::PolicyLevel::User as i32,
                    locked: false,
                }),
                None,
            ),
        ));
        assert_eq!(error_code(&user_override), codes::SETTING_LOCKED);

        let policy_get = core.handle(&request_with_id(
            306,
            ActorOrigin::Ui,
            envelope::Body::Query(Query {
                request: Some(query::Request::GetPolicySetting(
                    selah_proto::v1::GetPolicySetting {
                        key: "appearance.text-size".into(),
                    },
                )),
            }),
        ));
        assert!(matches!(
            policy_get.reply.and_then(|r| r.body),
            Some(envelope::Body::Reply(Reply {
                result: Some(reply::Result::PolicySetting(p))
            })) if p.found && p.setting.as_ref().is_some_and(|s| s.locked && s.value_json == "130")
        ));

        core.handle(&request_with_id(
            307,
            ActorOrigin::Ui,
            command_with_group(
                command::Action::RecordFeedback(selah_proto::v1::RecordFeedback {
                    article_id: "troubleshooting.selah-started-in-safe-mode".into(),
                    helpful: true,
                }),
                None,
            ),
        ));
        assert_eq!(
            core.library()
                .help_feedback_count("troubleshooting.selah-started-in-safe-mode", Some(true))
                .unwrap(),
            1
        );
    }

    #[test]
    fn search_queries_round_trip_through_core_service_and_map_errors() {
        let dir = tempfile::tempdir().unwrap();
        let mut core = CoreService::new(dir.path(), false);

        let song = song_fixture("Amazing Grace");
        core.handle(&request_with_id(
            401,
            ActorOrigin::Ui,
            command_with_group(
                command::Action::SaveSong(selah_proto::v1::SaveSong {
                    song: Some(song.clone()),
                }),
                None,
            ),
        ));

        let payload = b"synthetic-search-image";
        let blob = core
            .library()
            .media_store()
            .unwrap()
            .import_reader(payload.as_slice(), "png", 1024)
            .unwrap();
        let media = selah_proto::v1::MediaItem {
            id: Some(crate::library::new_id()),
            name: "Grace Sunset Background".into(),
            kind: MediaKind::Image as i32,
            content_hash: blob.content_hash,
            extension: blob.extension,
            size_bytes: blob.size_bytes,
            tags: vec!["Worship".into()],
            ..Default::default()
        };
        core.handle(&request_with_id(
            402,
            ActorOrigin::Ui,
            command_with_group(
                command::Action::SaveMediaItem(selah_proto::v1::SaveMediaItem {
                    media_item: Some(media),
                }),
                None,
            ),
        ));

        crate::library::write_bible_database(
            &core.library().paths().bibles.join("KJV.db"),
            "KJV",
            "King James Version",
            "en",
            &[crate::library::BibleVerseRecord {
                book_num: 49,
                book_code: "EPH",
                book_name: "Ephesians",
                chapter: 2,
                verse: 8,
                text: "For by grace are ye saved through faith.",
            }],
        )
        .unwrap();

        let songs_out = core.handle(&request_with_id(
            403,
            ActorOrigin::Ui,
            envelope::Body::Query(Query {
                request: Some(query::Request::SearchSongs(selah_proto::v1::SearchSongs {
                    query: "amzing grace".into(),
                    limit: 10,
                    offset: 0,
                })),
            }),
        ));
        assert!(matches!(
            songs_out.reply.and_then(|r| r.body),
            Some(envelope::Body::Reply(Reply {
                result: Some(reply::Result::SongSearchResults(r))
            })) if r.total == 1 && r.hits[0].song.as_ref().unwrap().title == "Amazing Grace"
        ));

        let media_out = core.handle(&request_with_id(
            404,
            ActorOrigin::Ui,
            envelope::Body::Query(Query {
                request: Some(query::Request::SearchMedia(selah_proto::v1::SearchMedia {
                    query: "backdrop".into(),
                    kind: MediaKind::Image as i32,
                    limit: 10,
                    offset: 0,
                })),
            }),
        ));
        assert!(matches!(
            media_out.reply.and_then(|r| r.body),
            Some(envelope::Body::Reply(Reply {
                result: Some(reply::Result::MediaSearchResults(r))
            })) if r.total == 1 && r.hits[0].media_item.as_ref().unwrap().name == "Grace Sunset Background"
        ));

        let bible_out = core.handle(&request_with_id(
            405,
            ActorOrigin::Ui,
            envelope::Body::Query(Query {
                request: Some(query::Request::SearchScripture(
                    selah_proto::v1::SearchScripture {
                        query: "grace".into(),
                        translation: "KJV".into(),
                        limit: 10,
                        offset: 0,
                    },
                )),
            }),
        ));
        assert!(matches!(
            bible_out.reply.and_then(|r| r.body),
            Some(envelope::Body::Reply(Reply {
                result: Some(reply::Result::ScriptureSearchResults(r))
            })) if r.total == 1 && r.hits[0].book == "EPH"
        ));

        let library_out = core.handle(&request_with_id(
            406,
            ActorOrigin::Ui,
            envelope::Body::Query(Query {
                request: Some(query::Request::SearchLibrary(
                    selah_proto::v1::SearchLibrary {
                        query: "grace".into(),
                        scope: SearchScope::All as i32,
                        translation: String::new(),
                        limit: 10,
                    },
                )),
            }),
        ));
        assert!(matches!(
            library_out.reply.and_then(|r| r.body),
            Some(envelope::Body::Reply(Reply {
                result: Some(reply::Result::LibrarySearchResults(r))
            })) if r.songs.len() == 1 && r.media.len() == 1 && r.scripture.len() == 1
        ));

        // Missing translation -> SEL-LIB-0001; invalid limit -> SEL-LIB-0002
        let missing = core.handle(&request_with_id(
            407,
            ActorOrigin::Ui,
            envelope::Body::Query(Query {
                request: Some(query::Request::SearchScripture(
                    selah_proto::v1::SearchScripture {
                        query: "grace".into(),
                        translation: "MISSING".into(),
                        limit: 10,
                        offset: 0,
                    },
                )),
            }),
        ));
        assert_eq!(error_code(&missing), codes::LIB_NOT_FOUND);

        let bad_limit = core.handle(&request_with_id(
            408,
            ActorOrigin::Ui,
            envelope::Body::Query(Query {
                request: Some(query::Request::SearchSongs(selah_proto::v1::SearchSongs {
                    query: "grace".into(),
                    limit: 0,
                    offset: 0,
                })),
            }),
        ));
        assert_eq!(error_code(&bad_limit), codes::LIB_INVALID);
    }

    #[test]
    fn song_parse_transpose_and_split_queries_round_trip_through_core_service() {
        let dir = tempfile::tempdir().unwrap();
        let mut core = CoreService::new(dir.path(), false);

        let raw = "\
{title: Amazing Grace}
{key: G}
{sov}
[G]Amazing grace! How [C]sweet the [G]sound
That saved a wretch like [D]me!
[G]I once was lost, but [C]now am [G]found;
Was [Em]blind, but [D]now I [G]see.
{eov}";

        let parsed_out = core.handle(&request_with_id(
            501,
            ActorOrigin::Ui,
            envelope::Body::Query(Query {
                request: Some(query::Request::ParsePastedSong(
                    selah_proto::v1::ParsePastedSong {
                        raw_text: raw.into(),
                        format: SongPasteFormat::Unspecified as i32,
                        default_title: String::new(),
                    },
                )),
            }),
        ));
        let parsed_reply = match parsed_out.reply.and_then(|r| r.body) {
            Some(envelope::Body::Reply(Reply {
                result: Some(reply::Result::ParsedSong(reply)),
            })) => reply,
            other => panic!("unexpected parsed reply: {other:?}"),
        };
        assert_eq!(
            parsed_reply.detected_format,
            SongPasteFormat::ChordPro as i32
        );
        let parsed_song = parsed_reply.song.unwrap();
        assert_eq!(parsed_song.title, "Amazing Grace");

        let transposed_out = core.handle(&request_with_id(
            502,
            ActorOrigin::Ui,
            envelope::Body::Query(Query {
                request: Some(query::Request::TransposeSong(
                    selah_proto::v1::TransposeSong {
                        song: Some(parsed_song),
                        semitones: 2,
                        accidental_spelling: selah_proto::v1::AccidentalSpelling::Unspecified
                            as i32,
                    },
                )),
            }),
        ));
        let transposed_song = match transposed_out.reply.and_then(|r| r.body) {
            Some(envelope::Body::Reply(Reply {
                result: Some(reply::Result::Song(song)),
            })) => song,
            other => panic!("unexpected transpose reply: {other:?}"),
        };
        assert_eq!(transposed_song.key, "A");
        assert_eq!(transposed_song.sections[0].lines[0].chords[0].text, "A");

        let split_out = core.handle(&request_with_id(
            503,
            ActorOrigin::Ui,
            envelope::Body::Query(Query {
                request: Some(query::Request::SplitSongSlides(
                    selah_proto::v1::SplitSongSlides {
                        song: Some(transposed_song),
                        arrangement_id: None,
                        max_lines: 2,
                        copyright_placement: CopyrightPlacement::FirstAndLast as i32,
                        title_slide: false,
                        blank_slide_at_end: true,
                        translation_language: String::new(),
                    },
                )),
            }),
        ));
        let slides = match split_out.reply.and_then(|r| r.body) {
            Some(envelope::Body::Reply(Reply {
                result: Some(reply::Result::GeneratedSongSlides(slides)),
            })) => slides,
            other => panic!("unexpected split reply: {other:?}"),
        };
        assert_eq!(slides.slides.len(), 3);
        assert_eq!(slides.slides[0].part_index, 1);
        assert_eq!(slides.slides[0].part_count, 2);
        assert!(slides.slides[2].is_blank);
    }

    #[test]
    fn scripture_commands_and_queries_round_trip_and_enforce_permissions() {
        // sec: SEC-14, SEC-03
        let dir = tempfile::tempdir().unwrap();
        let mut core = CoreService::new(dir.path(), false);

        let usfm_bytes = b"\\id GEN\n\\h Genesis\n\\c 1\n\\v 1 In the beginning God created the heaven and the earth.\n\\id JHN\n\\h John\n\\c 3\n\\v 16 For God so loved the world.".to_vec();

        // Non-UI origins are refused before modifying library/bibles/.
        for origin in [ActorOrigin::Api, ActorOrigin::Midi, ActorOrigin::Macro] {
            let denied = core.handle(&request_with_id(
                601,
                origin,
                command_with_group(
                    command::Action::ImportBible(selah_proto::v1::ImportBible {
                        raw_bytes: usfm_bytes.clone(),
                        format: selah_proto::v1::BibleFormat::Usfm as i32,
                        code: "DENIED".into(),
                        name: "Denied Bible".into(),
                        language: "en".into(),
                        ..Default::default()
                    }),
                    None,
                ),
            ));
            assert_eq!(error_code(&denied), DENIED_CODE);
        }

        // UI origin imports USFM, lists books, parses a reference, fetches passages, splits slides, and deletes.
        let imported = core.handle(&request_with_id(
            602,
            ActorOrigin::Ui,
            command_with_group(
                command::Action::ImportBible(selah_proto::v1::ImportBible {
                    raw_bytes: usfm_bytes,
                    format: selah_proto::v1::BibleFormat::Usfm as i32,
                    code: "SAMP".into(),
                    name: "Sample USFM".into(),
                    language: "en".into(),
                    ..Default::default()
                }),
                None,
            ),
        ));
        assert!(matches!(
            imported.reply.and_then(|r| r.body),
            Some(envelope::Body::Reply(Reply {
                result: Some(reply::Result::BibleTranslation(t))
            })) if t.code == "SAMP" && t.book_count == 2 && t.verse_count == 2
        ));

        let listed = core.handle(&request_with_id(
            603,
            ActorOrigin::Ui,
            envelope::Body::Query(Query {
                request: Some(query::Request::ListBibles(selah_proto::v1::ListBibles {
                    include_books: true,
                })),
            }),
        ));
        assert!(matches!(
            listed.reply.and_then(|r| r.body),
            Some(envelope::Body::Reply(Reply {
                result: Some(reply::Result::BibleList(list))
            })) if list.bibles.len() == 1 && list.bibles[0].books.len() == 2
        ));

        let parsed = core.handle(&request_with_id(
            604,
            ActorOrigin::Ui,
            envelope::Body::Query(Query {
                request: Some(query::Request::ParseScriptureReference(
                    selah_proto::v1::ParseScriptureReference {
                        reference: "Jean 3, 16".into(),
                        language_hint: "fr".into(),
                    },
                )),
            }),
        ));
        assert!(matches!(
            parsed.reply.and_then(|r| r.body),
            Some(envelope::Body::Reply(Reply {
                result: Some(reply::Result::ParsedScriptureReference(p))
            })) if p.normalized == "John 3:16" && p.spans.len() == 1
        ));

        let passage = core.handle(&request_with_id(
            605,
            ActorOrigin::Ui,
            envelope::Body::Query(Query {
                request: Some(query::Request::GetScripturePassages(
                    selah_proto::v1::GetScripturePassages {
                        translation: "SAMP".into(),
                        reference: "Jn 3:16".into(),
                        spans: Vec::new(),
                        language_hint: String::new(),
                    },
                )),
            }),
        ));
        let passages_reply = match passage.reply.and_then(|r| r.body) {
            Some(envelope::Body::Reply(Reply {
                result: Some(reply::Result::ScripturePassages(p)),
            })) => p,
            other => panic!("unexpected passage reply: {other:?}"),
        };
        assert_eq!(passages_reply.verses.len(), 1);

        let split = core.handle(&request_with_id(
            606,
            ActorOrigin::Ui,
            envelope::Body::Query(Query {
                request: Some(query::Request::SplitScriptureSlides(
                    selah_proto::v1::SplitScriptureSlides {
                        translation: "SAMP".into(),
                        translation_name: "Sample USFM".into(),
                        reference: passages_reply.normalized_reference,
                        verses: passages_reply.verses,
                        max_lines: 4,
                        max_chars_per_line: 48,
                        verses_per_slide: 1,
                        one_verse_per_slide: true,
                        show_verse_numbers: true,
                        reference_placement: selah_proto::v1::ReferencePlacement::SlideFooter
                            as i32,
                        title_slide: true,
                        blank_slide_at_end: true,
                    },
                )),
            }),
        ));
        assert!(matches!(
            split.reply.and_then(|r| r.body),
            Some(envelope::Body::Reply(Reply {
                result: Some(reply::Result::GeneratedScriptureSlides(s))
            })) if s.slides.len() == 3 && s.slides[0].is_title && s.slides[2].is_blank
        ));

        let delete_denied = core.handle(&request_with_id(
            607,
            ActorOrigin::Api,
            command_with_group(
                command::Action::DeleteBible(selah_proto::v1::DeleteBible {
                    code: "SAMP".into(),
                }),
                None,
            ),
        ));
        assert_eq!(error_code(&delete_denied), DENIED_CODE);

        let deleted = core.handle(&request_with_id(
            608,
            ActorOrigin::Ui,
            command_with_group(
                command::Action::DeleteBible(selah_proto::v1::DeleteBible {
                    code: "SAMP".into(),
                }),
                None,
            ),
        ));
        assert!(matches!(
            deleted.reply.and_then(|r| r.body),
            Some(envelope::Body::Reply(Reply {
                result: Some(reply::Result::Acknowledged(true))
            }))
        ));
    }
}
