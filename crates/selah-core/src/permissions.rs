//! Authorisation for every command (SEC-14, SAFE-01).
//!
//! The `Actor` on an envelope is attribution, not proof. Core binds it to the verified
//! transport peer (same user, checked by `selah-ipc`) before deciding. M1's policy: the local
//! owner may do everything the UI offers. Roles (M4), API scopes (M7) and organisation roles
//! (M9) extend [`authorize`]; every new command must be added to [`CommandKind`], and the
//! exhaustive match below makes forgetting a compile error.
use selah_proto::v1::{Actor, ActorOrigin, command};

/// Every command core accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CommandKind {
    /// Raw engine commands. Only core itself produces these; clients may not.
    Render,
    SetAppearance,
    RecordFeedback,
    ShowTestPattern,
    ClearOutputs,
    SetUpdateSettings,
    CheckForUpdate,
    InstallUpdate,
    /// Sent to the watchdog, not core; core answers that it can't do it.
    RestartNormally,
    CreateSupportBundle,
    RunDiagnostics,
    SaveSong,
    DeleteSong,
    SaveTemplate,
    DeleteTemplate,
    SaveMediaItem,
    DeleteMediaItem,
    SavePresentation,
    DeletePresentation,
    SaveTag,
    DeleteTag,
    Undo,
    Redo,
    SaveEditorBuffer,
    DiscardEditorBuffer,
    SetPolicySetting,
    RemovePolicySetting,
    ImportBible,
    DeleteBible,
}

pub const ALL_COMMANDS: [CommandKind; 29] = [
    CommandKind::Render,
    CommandKind::SetAppearance,
    CommandKind::RecordFeedback,
    CommandKind::ShowTestPattern,
    CommandKind::ClearOutputs,
    CommandKind::SetUpdateSettings,
    CommandKind::CheckForUpdate,
    CommandKind::InstallUpdate,
    CommandKind::RestartNormally,
    CommandKind::CreateSupportBundle,
    CommandKind::RunDiagnostics,
    CommandKind::SaveSong,
    CommandKind::DeleteSong,
    CommandKind::SaveTemplate,
    CommandKind::DeleteTemplate,
    CommandKind::SaveMediaItem,
    CommandKind::DeleteMediaItem,
    CommandKind::SavePresentation,
    CommandKind::DeletePresentation,
    CommandKind::SaveTag,
    CommandKind::DeleteTag,
    CommandKind::Undo,
    CommandKind::Redo,
    CommandKind::SaveEditorBuffer,
    CommandKind::DiscardEditorBuffer,
    CommandKind::SetPolicySetting,
    CommandKind::RemovePolicySetting,
    CommandKind::ImportBible,
    CommandKind::DeleteBible,
];

pub fn kind_of(action: &command::Action) -> CommandKind {
    match action {
        command::Action::Render(_) => CommandKind::Render,
        command::Action::SetAppearance(_) => CommandKind::SetAppearance,
        command::Action::RecordFeedback(_) => CommandKind::RecordFeedback,
        command::Action::ShowTestPattern(_) => CommandKind::ShowTestPattern,
        command::Action::ClearOutputs(_) => CommandKind::ClearOutputs,
        command::Action::SetUpdateSettings(_) => CommandKind::SetUpdateSettings,
        command::Action::CheckForUpdate(_) => CommandKind::CheckForUpdate,
        command::Action::InstallUpdate(_) => CommandKind::InstallUpdate,
        command::Action::RestartNormally(_) => CommandKind::RestartNormally,
        command::Action::CreateSupportBundle(_) => CommandKind::CreateSupportBundle,
        command::Action::RunDiagnostics(_) => CommandKind::RunDiagnostics,
        command::Action::SaveSong(_) => CommandKind::SaveSong,
        command::Action::DeleteSong(_) => CommandKind::DeleteSong,
        command::Action::SaveTemplate(_) => CommandKind::SaveTemplate,
        command::Action::DeleteTemplate(_) => CommandKind::DeleteTemplate,
        command::Action::SaveMediaItem(_) => CommandKind::SaveMediaItem,
        command::Action::DeleteMediaItem(_) => CommandKind::DeleteMediaItem,
        command::Action::SavePresentation(_) => CommandKind::SavePresentation,
        command::Action::DeletePresentation(_) => CommandKind::DeletePresentation,
        command::Action::SaveTag(_) => CommandKind::SaveTag,
        command::Action::DeleteTag(_) => CommandKind::DeleteTag,
        command::Action::Undo(_) => CommandKind::Undo,
        command::Action::Redo(_) => CommandKind::Redo,
        command::Action::SaveEditorBuffer(_) => CommandKind::SaveEditorBuffer,
        command::Action::DiscardEditorBuffer(_) => CommandKind::DiscardEditorBuffer,
        command::Action::SetPolicySetting(_) => CommandKind::SetPolicySetting,
        command::Action::RemovePolicySetting(_) => CommandKind::RemovePolicySetting,
        command::Action::ImportBible(_) => CommandKind::ImportBible,
        command::Action::DeleteBible(_) => CommandKind::DeleteBible,
    }
}

/// Does this command change what the room sees (SAFE-01)?
pub fn changes_screens(kind: CommandKind) -> bool {
    match kind {
        CommandKind::Render | CommandKind::ShowTestPattern | CommandKind::ClearOutputs => true,
        // An update restarts every process, but core refuses it while anything is live.
        CommandKind::SetAppearance
        | CommandKind::RecordFeedback
        | CommandKind::SetUpdateSettings
        | CommandKind::CheckForUpdate
        | CommandKind::InstallUpdate
        | CommandKind::RestartNormally
        | CommandKind::CreateSupportBundle
        | CommandKind::RunDiagnostics
        | CommandKind::SaveSong
        | CommandKind::DeleteSong
        | CommandKind::SaveTemplate
        | CommandKind::DeleteTemplate
        | CommandKind::SaveMediaItem
        | CommandKind::DeleteMediaItem
        | CommandKind::SavePresentation
        | CommandKind::DeletePresentation
        | CommandKind::SaveTag
        | CommandKind::DeleteTag
        | CommandKind::Undo
        | CommandKind::Redo
        | CommandKind::SaveEditorBuffer
        | CommandKind::DiscardEditorBuffer
        | CommandKind::SetPolicySetting
        | CommandKind::RemovePolicySetting
        | CommandKind::ImportBible
        | CommandKind::DeleteBible => false,
    }
}

/// Who is asking, once the actor is bound to the verified peer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Principal {
    /// The person at this computer, through the Selah UI (or a tour it runs).
    LocalOwner,
    /// One of Selah's own processes.
    Internal,
    /// An origin whose feature isn't built yet (API, MIDI, macros, timelines, sync).
    NotYetSupported,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("{origin:?} may not run {kind:?}")]
pub struct Denied {
    pub origin: ActorOrigin,
    pub kind: CommandKind,
}

/// The error catalogue code for a refused command.
pub const DENIED_CODE: &str = "SEL-SEC-0001";

pub fn principal(origin: ActorOrigin) -> Principal {
    match origin {
        ActorOrigin::Ui | ActorOrigin::Tour => Principal::LocalOwner,
        ActorOrigin::Core | ActorOrigin::Engine | ActorOrigin::Watchdog => Principal::Internal,
        ActorOrigin::Api
        | ActorOrigin::Midi
        | ActorOrigin::Macro
        | ActorOrigin::Timeline
        | ActorOrigin::Sync
        | ActorOrigin::Unspecified => Principal::NotYetSupported,
    }
}

/// Decide before the command is validated or applied. The peer is already verified as the
/// same OS user by the transport.
pub fn authorize(actor: &Actor, kind: CommandKind) -> Result<(), Denied> {
    let origin = ActorOrigin::try_from(actor.origin).unwrap_or(ActorOrigin::Unspecified);
    let allowed = match (principal(origin), kind) {
        // Nobody outside core may send raw engine commands.
        (_, CommandKind::Render) => false,
        (Principal::LocalOwner, _) => true,
        (Principal::Internal, _) | (Principal::NotYetSupported, _) => false,
    };
    if allowed {
        Ok(())
    } else {
        Err(Denied { origin, kind })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGINS: [ActorOrigin; 11] = [
        ActorOrigin::Unspecified,
        ActorOrigin::Ui,
        ActorOrigin::Api,
        ActorOrigin::Midi,
        ActorOrigin::Macro,
        ActorOrigin::Timeline,
        ActorOrigin::Sync,
        ActorOrigin::Tour,
        ActorOrigin::Core,
        ActorOrigin::Engine,
        ActorOrigin::Watchdog,
    ];

    /// The expected matrix for M1. Changing a cell is a security decision: say why in the PR.
    fn expected(origin: ActorOrigin, kind: CommandKind) -> bool {
        matches!(origin, ActorOrigin::Ui | ActorOrigin::Tour) && kind != CommandKind::Render
    }

    #[test]
    fn permission_matrix_covers_every_origin_and_command() {
        // sec: SEC-14, SAFE-01
        let mut cells = 0;
        for origin in ORIGINS {
            for kind in ALL_COMMANDS {
                let actor = Actor {
                    user_id: "local-owner".into(),
                    device_id: "test".into(),
                    origin: origin as i32,
                };
                assert_eq!(
                    authorize(&actor, kind).is_ok(),
                    expected(origin, kind),
                    "{origin:?} × {kind:?}"
                );
                cells += 1;
            }
        }
        assert_eq!(cells, ORIGINS.len() * ALL_COMMANDS.len());
    }

    #[test]
    fn every_command_variant_is_listed() {
        // Exhaustive without a wildcard: a new CommandKind fails to compile until it's here.
        for kind in ALL_COMMANDS {
            match kind {
                CommandKind::Render
                | CommandKind::SetAppearance
                | CommandKind::RecordFeedback
                | CommandKind::ShowTestPattern
                | CommandKind::ClearOutputs
                | CommandKind::SetUpdateSettings
                | CommandKind::CheckForUpdate
                | CommandKind::InstallUpdate
                | CommandKind::RestartNormally
                | CommandKind::CreateSupportBundle
                | CommandKind::RunDiagnostics
                | CommandKind::SaveSong
                | CommandKind::DeleteSong
                | CommandKind::SaveTemplate
                | CommandKind::DeleteTemplate
                | CommandKind::SaveMediaItem
                | CommandKind::DeleteMediaItem
                | CommandKind::SavePresentation
                | CommandKind::DeletePresentation
                | CommandKind::SaveTag
                | CommandKind::DeleteTag
                | CommandKind::Undo
                | CommandKind::Redo
                | CommandKind::SaveEditorBuffer
                | CommandKind::DiscardEditorBuffer
                | CommandKind::SetPolicySetting
                | CommandKind::RemovePolicySetting
                | CommandKind::ImportBible
                | CommandKind::DeleteBible => {}
            }
        }
        let unique: std::collections::HashSet<_> = ALL_COMMANDS.iter().collect();
        assert_eq!(unique.len(), ALL_COMMANDS.len());
    }

    #[test]
    fn unknown_origin_numbers_are_denied() {
        let actor = Actor {
            user_id: "x".into(),
            device_id: "y".into(),
            origin: 4242,
        };
        assert!(authorize(&actor, CommandKind::ClearOutputs).is_err());
    }
}
