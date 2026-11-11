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
    ImportMedia,
    ExportSelahBundle,
    ImportSelahBundle,
    CreateLibraryBackup,
    RestoreLibraryBackup,
    SetOutputConfigs,
    PresentScene,
    StagePreviewScene,
    PreloadItems,
    SaveService,
    DeleteService,
    SaveServiceType,
    DeleteServiceType,
    SaveTeam,
    DeleteTeam,
    CreateServiceFromPrevious,
    ExportServicePrint,
    LoadLiveService,
    TriggerItem,
    TriggerSlide,
    NextSlide,
    PreviousSlide,
    NextItem,
    PreviousItem,
    TriggerSectionHotkey,
    ClearLayer,
    SetLayerState,
    ShowLogo,
    PlayAudioLayer,
    StopAudioLayer,
    LiveUndo,
    LiveRedo,
    SaveLook,
    DeleteLook,
    SaveProp,
    DeleteProp,
    SaveMessage,
    DeleteMessage,
    SaveTimer,
    DeleteTimer,
    SaveMacro,
    DeleteMacro,
    SaveClearGroup,
    DeleteClearGroup,
    SaveStageLayout,
    DeleteStageLayout,
    SetLook,
    ShowProp,
    HideProp,
    ShowMessage,
    HideMessage,
    StartTimer,
    PauseTimer,
    ResetTimer,
    AdvanceLiveClock,
    RunMacro,
    TriggerClearGroup,
    SetStageLayout,
    SendBoothMessage,
    SetBoothLock,
    SetRoleLock,
}

pub const ALL_COMMANDS: [CommandKind; 90] = [
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
    CommandKind::ImportMedia,
    CommandKind::ExportSelahBundle,
    CommandKind::ImportSelahBundle,
    CommandKind::CreateLibraryBackup,
    CommandKind::RestoreLibraryBackup,
    CommandKind::SetOutputConfigs,
    CommandKind::PresentScene,
    CommandKind::StagePreviewScene,
    CommandKind::PreloadItems,
    CommandKind::SaveService,
    CommandKind::DeleteService,
    CommandKind::SaveServiceType,
    CommandKind::DeleteServiceType,
    CommandKind::SaveTeam,
    CommandKind::DeleteTeam,
    CommandKind::CreateServiceFromPrevious,
    CommandKind::ExportServicePrint,
    CommandKind::LoadLiveService,
    CommandKind::TriggerItem,
    CommandKind::TriggerSlide,
    CommandKind::NextSlide,
    CommandKind::PreviousSlide,
    CommandKind::NextItem,
    CommandKind::PreviousItem,
    CommandKind::TriggerSectionHotkey,
    CommandKind::ClearLayer,
    CommandKind::SetLayerState,
    CommandKind::ShowLogo,
    CommandKind::PlayAudioLayer,
    CommandKind::StopAudioLayer,
    CommandKind::LiveUndo,
    CommandKind::LiveRedo,
    CommandKind::SaveLook,
    CommandKind::DeleteLook,
    CommandKind::SaveProp,
    CommandKind::DeleteProp,
    CommandKind::SaveMessage,
    CommandKind::DeleteMessage,
    CommandKind::SaveTimer,
    CommandKind::DeleteTimer,
    CommandKind::SaveMacro,
    CommandKind::DeleteMacro,
    CommandKind::SaveClearGroup,
    CommandKind::DeleteClearGroup,
    CommandKind::SaveStageLayout,
    CommandKind::DeleteStageLayout,
    CommandKind::SetLook,
    CommandKind::ShowProp,
    CommandKind::HideProp,
    CommandKind::ShowMessage,
    CommandKind::HideMessage,
    CommandKind::StartTimer,
    CommandKind::PauseTimer,
    CommandKind::ResetTimer,
    CommandKind::AdvanceLiveClock,
    CommandKind::RunMacro,
    CommandKind::TriggerClearGroup,
    CommandKind::SetStageLayout,
    CommandKind::SendBoothMessage,
    CommandKind::SetBoothLock,
    CommandKind::SetRoleLock,
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
        command::Action::ImportMedia(_) => CommandKind::ImportMedia,
        command::Action::ExportSelahBundle(_) => CommandKind::ExportSelahBundle,
        command::Action::ImportSelahBundle(_) => CommandKind::ImportSelahBundle,
        command::Action::CreateLibraryBackup(_) => CommandKind::CreateLibraryBackup,
        command::Action::RestoreLibraryBackup(_) => CommandKind::RestoreLibraryBackup,
        command::Action::SetOutputConfigs(_) => CommandKind::SetOutputConfigs,
        command::Action::PresentScene(_) => CommandKind::PresentScene,
        command::Action::StagePreviewScene(_) => CommandKind::StagePreviewScene,
        command::Action::PreloadItems(_) => CommandKind::PreloadItems,
        command::Action::SaveService(_) => CommandKind::SaveService,
        command::Action::DeleteService(_) => CommandKind::DeleteService,
        command::Action::SaveServiceType(_) => CommandKind::SaveServiceType,
        command::Action::DeleteServiceType(_) => CommandKind::DeleteServiceType,
        command::Action::SaveTeam(_) => CommandKind::SaveTeam,
        command::Action::DeleteTeam(_) => CommandKind::DeleteTeam,
        command::Action::CreateServiceFromPrevious(_) => CommandKind::CreateServiceFromPrevious,
        command::Action::ExportServicePrint(_) => CommandKind::ExportServicePrint,
        command::Action::LoadLiveService(_) => CommandKind::LoadLiveService,
        command::Action::TriggerItem(_) => CommandKind::TriggerItem,
        command::Action::TriggerSlide(_) => CommandKind::TriggerSlide,
        command::Action::NextSlide(_) => CommandKind::NextSlide,
        command::Action::PreviousSlide(_) => CommandKind::PreviousSlide,
        command::Action::NextItem(_) => CommandKind::NextItem,
        command::Action::PreviousItem(_) => CommandKind::PreviousItem,
        command::Action::TriggerSectionHotkey(_) => CommandKind::TriggerSectionHotkey,
        command::Action::ClearLayer(_) => CommandKind::ClearLayer,
        command::Action::SetLayerState(_) => CommandKind::SetLayerState,
        command::Action::ShowLogo(_) => CommandKind::ShowLogo,
        command::Action::PlayAudioLayer(_) => CommandKind::PlayAudioLayer,
        command::Action::StopAudioLayer(_) => CommandKind::StopAudioLayer,
        command::Action::LiveUndo(_) => CommandKind::LiveUndo,
        command::Action::LiveRedo(_) => CommandKind::LiveRedo,
        command::Action::SaveLook(_) => CommandKind::SaveLook,
        command::Action::DeleteLook(_) => CommandKind::DeleteLook,
        command::Action::SaveProp(_) => CommandKind::SaveProp,
        command::Action::DeleteProp(_) => CommandKind::DeleteProp,
        command::Action::SaveMessage(_) => CommandKind::SaveMessage,
        command::Action::DeleteMessage(_) => CommandKind::DeleteMessage,
        command::Action::SaveTimer(_) => CommandKind::SaveTimer,
        command::Action::DeleteTimer(_) => CommandKind::DeleteTimer,
        command::Action::SaveMacro(_) => CommandKind::SaveMacro,
        command::Action::DeleteMacro(_) => CommandKind::DeleteMacro,
        command::Action::SaveClearGroup(_) => CommandKind::SaveClearGroup,
        command::Action::DeleteClearGroup(_) => CommandKind::DeleteClearGroup,
        command::Action::SaveStageLayout(_) => CommandKind::SaveStageLayout,
        command::Action::DeleteStageLayout(_) => CommandKind::DeleteStageLayout,
        command::Action::SetLook(_) => CommandKind::SetLook,
        command::Action::ShowProp(_) => CommandKind::ShowProp,
        command::Action::HideProp(_) => CommandKind::HideProp,
        command::Action::ShowMessage(_) => CommandKind::ShowMessage,
        command::Action::HideMessage(_) => CommandKind::HideMessage,
        command::Action::StartTimer(_) => CommandKind::StartTimer,
        command::Action::PauseTimer(_) => CommandKind::PauseTimer,
        command::Action::ResetTimer(_) => CommandKind::ResetTimer,
        command::Action::AdvanceLiveClock(_) => CommandKind::AdvanceLiveClock,
        command::Action::RunMacro(_) => CommandKind::RunMacro,
        command::Action::TriggerClearGroup(_) => CommandKind::TriggerClearGroup,
        command::Action::SetStageLayout(_) => CommandKind::SetStageLayout,
        command::Action::SendBoothMessage(_) => CommandKind::SendBoothMessage,
        command::Action::SetBoothLock(_) => CommandKind::SetBoothLock,
        command::Action::SetRoleLock(_) => CommandKind::SetRoleLock,
    }
}

/// Does this command change what the room sees (SAFE-01)?
pub fn changes_screens(kind: CommandKind) -> bool {
    match kind {
        CommandKind::Render
        | CommandKind::ShowTestPattern
        | CommandKind::ClearOutputs
        | CommandKind::SetOutputConfigs
        | CommandKind::PresentScene
        | CommandKind::LoadLiveService
        | CommandKind::TriggerItem
        | CommandKind::TriggerSlide
        | CommandKind::NextSlide
        | CommandKind::PreviousSlide
        | CommandKind::NextItem
        | CommandKind::PreviousItem
        | CommandKind::TriggerSectionHotkey
        | CommandKind::ClearLayer
        | CommandKind::SetLayerState
        | CommandKind::ShowLogo
        | CommandKind::PlayAudioLayer
        | CommandKind::StopAudioLayer
        | CommandKind::LiveUndo
        | CommandKind::LiveRedo
        | CommandKind::SetLook
        | CommandKind::ShowProp
        | CommandKind::HideProp
        | CommandKind::ShowMessage
        | CommandKind::HideMessage
        | CommandKind::StartTimer
        | CommandKind::PauseTimer
        | CommandKind::ResetTimer
        | CommandKind::AdvanceLiveClock
        | CommandKind::RunMacro
        | CommandKind::TriggerClearGroup
        | CommandKind::SetStageLayout => true,
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
        | CommandKind::DeleteBible
        | CommandKind::ImportMedia
        | CommandKind::ExportSelahBundle
        | CommandKind::ImportSelahBundle
        | CommandKind::CreateLibraryBackup
        | CommandKind::RestoreLibraryBackup
        | CommandKind::StagePreviewScene
        | CommandKind::PreloadItems
        | CommandKind::SaveService
        | CommandKind::DeleteService
        | CommandKind::SaveServiceType
        | CommandKind::DeleteServiceType
        | CommandKind::SaveTeam
        | CommandKind::DeleteTeam
        | CommandKind::CreateServiceFromPrevious
        | CommandKind::ExportServicePrint
        | CommandKind::SaveLook
        | CommandKind::DeleteLook
        | CommandKind::SaveProp
        | CommandKind::DeleteProp
        | CommandKind::SaveMessage
        | CommandKind::DeleteMessage
        | CommandKind::SaveTimer
        | CommandKind::DeleteTimer
        | CommandKind::SaveMacro
        | CommandKind::DeleteMacro
        | CommandKind::SaveClearGroup
        | CommandKind::DeleteClearGroup
        | CommandKind::SaveStageLayout
        | CommandKind::DeleteStageLayout
        | CommandKind::SendBoothMessage
        | CommandKind::SetBoothLock
        | CommandKind::SetRoleLock => false,
    }
}

/// Commands that a macro action (`ActorOrigin::Macro`) is permitted to dispatch (M4.3, SEC-03/SEC-14).
/// Macros may never invoke another macro (`RunMacro`), mutate library documents, or change settings.
pub fn is_macro_allowed(kind: CommandKind) -> bool {
    matches!(
        kind,
        CommandKind::ClearOutputs
            | CommandKind::ClearLayer
            | CommandKind::SetLayerState
            | CommandKind::ShowLogo
            | CommandKind::PlayAudioLayer
            | CommandKind::StopAudioLayer
            | CommandKind::SetLook
            | CommandKind::ShowProp
            | CommandKind::HideProp
            | CommandKind::ShowMessage
            | CommandKind::HideMessage
            | CommandKind::StartTimer
            | CommandKind::PauseTimer
            | CommandKind::ResetTimer
            | CommandKind::TriggerClearGroup
            | CommandKind::SetStageLayout
    )
}

/// Commands that a role-locked Simple-mode volunteer may execute (M4.5, SEC-14, SAFE-01, M4-A12).
/// Producer commands (Looks, Props, audience Messages, Macros, library/template edits, output
/// configuration, policies) are refused when role lock is active.
pub fn is_volunteer_allowed(kind: CommandKind) -> bool {
    matches!(
        kind,
        CommandKind::LoadLiveService
            | CommandKind::TriggerItem
            | CommandKind::TriggerSlide
            | CommandKind::NextSlide
            | CommandKind::PreviousSlide
            | CommandKind::NextItem
            | CommandKind::PreviousItem
            | CommandKind::TriggerSectionHotkey
            | CommandKind::ClearOutputs
            | CommandKind::ClearLayer
            | CommandKind::ShowLogo
            | CommandKind::LiveUndo
            | CommandKind::LiveRedo
            | CommandKind::SendBoothMessage
            // Unlocking needs the PIN, which core checks.
            | CommandKind::SetRoleLock
            | CommandKind::RecordFeedback
            | CommandKind::StagePreviewScene
            | CommandKind::PreloadItems
    )
}

/// Commands allowed while Booth lock (`Ctrl/Cmd+L`, SAFE-06, M4-A12) is active:
/// leaves only Next, Previous, Clear, and unlocking Booth lock working.
pub fn is_booth_lock_allowed(kind: CommandKind) -> bool {
    matches!(
        kind,
        CommandKind::NextSlide
            | CommandKind::PreviousSlide
            | CommandKind::NextItem
            | CommandKind::PreviousItem
            | CommandKind::ClearOutputs
            | CommandKind::ClearLayer
            | CommandKind::ShowLogo
            | CommandKind::SetBoothLock
    )
}

/// Who is asking, once the actor is bound to the verified peer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Principal {
    /// The person at this computer, through the Selah UI (or a tour it runs).
    LocalOwner,
    /// A role-locked volunteer operating in Simple mode (M4.5, SEC-14, SAFE-01).
    Volunteer,
    /// An in-service macro action (M4.3).
    Macro,
    /// One of Selah's own processes.
    Internal,
    /// An origin whose feature isn't built yet (API, MIDI, timelines, sync).
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
        ActorOrigin::Macro => Principal::Macro,
        ActorOrigin::Core | ActorOrigin::Engine | ActorOrigin::Watchdog => Principal::Internal,
        ActorOrigin::Api
        | ActorOrigin::Midi
        | ActorOrigin::Timeline
        | ActorOrigin::Sync
        | ActorOrigin::Unspecified => Principal::NotYetSupported,
    }
}

/// Decide before the command is validated or applied. The peer is already verified as the
/// same OS user by the transport. `role_locked` is core's own state (SEC-14): while it is on,
/// the person at the UI is a volunteer, whatever the request says about itself.
pub fn authorize(actor: &Actor, kind: CommandKind, role_locked: bool) -> Result<(), Denied> {
    let origin = ActorOrigin::try_from(actor.origin).unwrap_or(ActorOrigin::Unspecified);
    let effective_principal = match principal(origin) {
        Principal::LocalOwner if role_locked => Principal::Volunteer,
        other => other,
    };
    let allowed = match (effective_principal, kind) {
        // Nobody outside core may send raw engine commands.
        (_, CommandKind::Render) => false,
        (Principal::LocalOwner, _) => true,
        (Principal::Volunteer, k) => is_volunteer_allowed(k),
        (Principal::Macro, k) => is_macro_allowed(k),
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

    /// The expected matrix for M1–M4. Changing a cell is a security decision: say why in the PR.
    fn expected(origin: ActorOrigin, kind: CommandKind, role_locked: bool) -> bool {
        match origin {
            ActorOrigin::Ui | ActorOrigin::Tour if role_locked => is_volunteer_allowed(kind),
            ActorOrigin::Ui | ActorOrigin::Tour => kind != CommandKind::Render,
            ActorOrigin::Macro => is_macro_allowed(kind),
            _ => false,
        }
    }

    #[test]
    fn permission_matrix_covers_every_origin_and_command() {
        // sec: SEC-14, SAFE-01
        let mut cells = 0;
        for role_locked in [false, true] {
            for origin in ORIGINS {
                for kind in ALL_COMMANDS {
                    // What the client calls itself never matters: core decides (SEC-14).
                    for user_id in ["local-owner", "volunteer", "admin"] {
                        let actor = Actor {
                            user_id: user_id.into(),
                            device_id: "test".into(),
                            origin: origin as i32,
                        };
                        assert_eq!(
                            authorize(&actor, kind, role_locked).is_ok(),
                            expected(origin, kind, role_locked),
                            "{origin:?} × {kind:?} × {user_id} × locked={role_locked}"
                        );
                    }
                    cells += 1;
                }
            }
        }
        assert_eq!(cells, 2 * ORIGINS.len() * ALL_COMMANDS.len());
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
                | CommandKind::DeleteBible
                | CommandKind::ImportMedia
                | CommandKind::ExportSelahBundle
                | CommandKind::ImportSelahBundle
                | CommandKind::CreateLibraryBackup
                | CommandKind::RestoreLibraryBackup
                | CommandKind::SetOutputConfigs
                | CommandKind::PresentScene
                | CommandKind::StagePreviewScene
                | CommandKind::PreloadItems
                | CommandKind::SaveService
                | CommandKind::DeleteService
                | CommandKind::SaveServiceType
                | CommandKind::DeleteServiceType
                | CommandKind::SaveTeam
                | CommandKind::DeleteTeam
                | CommandKind::CreateServiceFromPrevious
                | CommandKind::ExportServicePrint
                | CommandKind::LoadLiveService
                | CommandKind::TriggerItem
                | CommandKind::TriggerSlide
                | CommandKind::NextSlide
                | CommandKind::PreviousSlide
                | CommandKind::NextItem
                | CommandKind::PreviousItem
                | CommandKind::TriggerSectionHotkey
                | CommandKind::ClearLayer
                | CommandKind::SetLayerState
                | CommandKind::ShowLogo
                | CommandKind::PlayAudioLayer
                | CommandKind::StopAudioLayer
                | CommandKind::LiveUndo
                | CommandKind::LiveRedo
                | CommandKind::SaveLook
                | CommandKind::DeleteLook
                | CommandKind::SaveProp
                | CommandKind::DeleteProp
                | CommandKind::SaveMessage
                | CommandKind::DeleteMessage
                | CommandKind::SaveTimer
                | CommandKind::DeleteTimer
                | CommandKind::SaveMacro
                | CommandKind::DeleteMacro
                | CommandKind::SaveClearGroup
                | CommandKind::DeleteClearGroup
                | CommandKind::SaveStageLayout
                | CommandKind::DeleteStageLayout
                | CommandKind::SetLook
                | CommandKind::ShowProp
                | CommandKind::HideProp
                | CommandKind::ShowMessage
                | CommandKind::HideMessage
                | CommandKind::StartTimer
                | CommandKind::PauseTimer
                | CommandKind::ResetTimer
                | CommandKind::AdvanceLiveClock
                | CommandKind::RunMacro
                | CommandKind::TriggerClearGroup
                | CommandKind::SetStageLayout
                | CommandKind::SendBoothMessage
                | CommandKind::SetBoothLock
                | CommandKind::SetRoleLock => {}
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
        assert!(authorize(&actor, CommandKind::ClearOutputs, false).is_err());
    }
}
