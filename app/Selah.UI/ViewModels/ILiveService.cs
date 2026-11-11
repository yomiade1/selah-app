using Selah.Contracts.V1;

namespace Selah.UI.ViewModels;

/// <summary>
/// Live control (M4.4, M4.5): every call is a command that core authorises (SEC-14) and answers
/// with the new <see cref="LiveStatus"/>. Clear everything uses the priority lane instead (SAFE-03).
/// </summary>
public interface ILiveService
{
    /// <summary>Raised on any thread when core reports a new live state.</summary>
    event EventHandler<LiveStatus>? LiveStatusChanged;

    Task<LiveStatus> GetLiveStateAsync();

    Task<LiveStatus> LoadLiveServiceAsync(Id serviceId, bool triggerFirstItem = false);

    Task<LiveStatus> TriggerItemAsync(uint itemIndex);

    Task<LiveStatus> TriggerSlideAsync(uint itemIndex, uint slideIndex);

    Task<LiveStatus> NextSlideAsync();

    Task<LiveStatus> PreviousSlideAsync();

    Task<LiveStatus> NextItemAsync();

    Task<LiveStatus> PreviousItemAsync();

    Task<LiveStatus> TriggerSectionHotkeyAsync(string hotkey);

    Task<LiveStatus> ClearLayerAsync(LayerKind kind);

    Task<LiveStatus> ShowLogoAsync(bool showLogo);

    Task<LiveStatus> LiveUndoAsync();

    Task<LiveStatus> LiveRedoAsync();

    Task<LiveStatus> SetLookAsync(Id lookId);

    Task<LiveStatus> RunMacroAsync(Id macroId);

    Task<LiveStatus> StartTimerAsync(Id timerId);

    Task<LiveStatus> PauseTimerAsync(Id timerId);

    Task<LiveStatus> ResetTimerAsync(Id timerId);

    Task<LiveStatus> HideMessageAsync();

    Task<LiveStatus> HidePropAsync();

    Task<LiveStatus> SetBoothLockAsync(bool locked);

    Task<LiveStatus> SendBoothMessageAsync(string text);

    /// <summary>SEC-14: lock to Simple mode, or unlock with the PIN. Core keeps the state.</summary>
    Task<LiveStatus> SetRoleLockAsync(bool locked, string pin);

    Task<IReadOnlyList<Look>> ListLooksAsync();

    Task<IReadOnlyList<Macro>> ListMacrosAsync();

    Task<IReadOnlyList<Selah.Contracts.V1.Timer>> ListTimersAsync();

    /// <summary>Saved stage display layouts (M4.6).</summary>
    Task<IReadOnlyList<StageLayout>> ListStageLayoutsAsync();

    /// <summary>Save a stage layout; core checks every rect and size (SEC-03).</summary>
    Task SaveStageLayoutAsync(StageLayout layout);

    /// <summary>Show this layout on every stage screen; an empty id goes back to the built-in one.</summary>
    Task<LiveStatus> SetStageLayoutAsync(Id layoutId);

    /// <summary>The emergency lane (SAFE-03): works even if core is frozen.</summary>
    Task<bool> ClearEverythingAsync();
}
