using System.Collections.ObjectModel;
using System.Globalization;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Selah.Contracts.V1;
using Selah.UI.Services;

namespace Selah.UI.ViewModels;

public sealed record ShowControlKindOption(ShowControlKind Kind, string Name);

public sealed record TimerKindOption(TimerKind Kind, string Name);

public sealed record OverrunOption(TimerOverrunMode Mode, string Name);

public sealed record MacroStepOption(MacroActionKind Kind, string Name, bool ShowLogo = false);

/// <summary>Something a macro step can use: a look, prop, message, timer, clear group, stage layout or layer.</summary>
public sealed record StepTarget(string Name, Id? Id = null, LayerKind Layer = LayerKind.Unspecified);

/// <summary>A tick box for one layer.</summary>
public sealed partial class LayerChoice : ObservableObject
{
    public required LayerKind Kind { get; init; }

    public required string Name { get; init; }

    [ObservableProperty]
    private bool isChecked;
}

/// <summary>One screen in a look: which layers it shows.</summary>
public sealed class LookScreenRow
{
    public required uint OutputIndex { get; init; }

    public required string Name { get; init; }

    public required ObservableCollection<LayerChoice> Layers { get; init; }
}

/// <summary>One step of a macro, with the words shown in its list.</summary>
public sealed record MacroStepRow(MacroAction Action, string Text);

/// <summary>
/// Set up tiles (M4.9): make and change looks, props, messages, timers, macros and clear groups.
/// Each saved one becomes a tile in Producer mode. Core checks and keeps them (M4.3, SEC-03).
/// </summary>
public sealed partial class TilesEditorViewModel : ObservableObject
{
    public const int MaxNameLength = 80;
    public const int MaxWordsLength = 200;
    public const int MaxSteps = 32;
    public const int MaxSeconds = 3600;
    public const int MaxMinutes = 600;

    private static readonly LayerKind[] AudienceLayers =
        [LayerKind.Messages, LayerKind.Props, LayerKind.Slide, LayerKind.Media, LayerKind.VideoInput];

    private readonly ILiveService? live;
    private readonly ILibraryService? library;
    private readonly Action<CoreException> showError;
    private readonly Func<Task> tilesChanged;
    private List<ShowControl> all = [];
    private List<(uint Index, string Name)> screens = [];
    private List<StageLayout> stageLayouts = [];
    private bool loading;

    public TilesEditorViewModel(ILiveService? live, ILibraryService? library, Action<CoreException> showError, Func<Task> tilesChanged)
    {
        this.live = live;
        this.library = library;
        this.showError = showError;
        this.tilesChanged = tilesChanged;
        var l = Localizer.Instance;
        Kinds =
        [
            new(ShowControlKind.Look, l["tiles-kind-look"]),
            new(ShowControlKind.Prop, l["tiles-kind-prop"]),
            new(ShowControlKind.Message, l["tiles-kind-message"]),
            new(ShowControlKind.Timer, l["tiles-kind-timer"]),
            new(ShowControlKind.Macro, l["tiles-kind-macro"]),
            new(ShowControlKind.ClearGroup, l["tiles-kind-clear-group"]),
        ];
        TimerKinds =
        [
            new(TimerKind.Countdown, l["tiles-timer-countdown"]),
            new(TimerKind.CountUp, l["tiles-timer-count-up"]),
            new(TimerKind.CountToTime, l["tiles-timer-count-to-time"]),
        ];
        OverrunOptions =
        [
            new(TimerOverrunMode.StopAtZero, l["tiles-timer-stop"]),
            new(TimerOverrunMode.CountUpNegative, l["tiles-timer-keep-counting"]),
            new(TimerOverrunMode.TriggerMacro, l["tiles-timer-run-macro"]),
        ];
        StepKinds =
        [
            new(MacroActionKind.SetLook, l["tiles-step-set-look"]),
            new(MacroActionKind.ShowProp, l["tiles-step-show-prop"]),
            new(MacroActionKind.HideProp, l["tiles-step-hide-prop"]),
            new(MacroActionKind.ShowMessage, l["tiles-step-show-message"]),
            new(MacroActionKind.HideMessage, l["tiles-step-hide-message"]),
            new(MacroActionKind.StartTimer, l["tiles-step-start-timer"]),
            new(MacroActionKind.PauseTimer, l["tiles-step-pause-timer"]),
            new(MacroActionKind.ResetTimer, l["tiles-step-reset-timer"]),
            new(MacroActionKind.ClearLayer, l["tiles-step-clear-layer"]),
            new(MacroActionKind.TriggerClearGroup, l["tiles-step-clear-group"]),
            new(MacroActionKind.ClearAll, l["tiles-step-clear-all"]),
            new(MacroActionKind.ShowLogo, l["tiles-step-show-logo"], ShowLogo: true),
            new(MacroActionKind.ShowLogo, l["tiles-step-hide-logo"]),
            new(MacroActionKind.SetStageLayout, l["tiles-step-stage-layout"]),
        ];
        selectedKind = Kinds[0];
        selectedTimerKind = TimerKinds[0];
        selectedOverrun = OverrunOptions[0];
        selectedStepKind = StepKinds[0];
        ClearLayers = NewLayerChoices(isChecked: false);
        SetScreens([]);
        UpdateStepTargets();
    }

    public IReadOnlyList<ShowControlKindOption> Kinds { get; }

    public IReadOnlyList<TimerKindOption> TimerKinds { get; }

    public IReadOnlyList<OverrunOption> OverrunOptions { get; }

    public IReadOnlyList<MacroStepOption> StepKinds { get; }

    /// <summary>The saved ones of the kind being made.</summary>
    public ObservableCollection<ShowControl> Saved { get; } = [];

    public ObservableCollection<LookScreenRow> ScreenRules { get; } = [];

    public ObservableCollection<LayerChoice> ClearLayers { get; }

    public ObservableCollection<MacroStepRow> Steps { get; } = [];

    public ObservableCollection<StepTarget> StepTargets { get; } = [];

    /// <summary>Macros a timer can run at zero.</summary>
    public ObservableCollection<Macro> Macros { get; } = [];

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsLook), nameof(IsProp), nameof(IsMessage), nameof(IsTimer), nameof(IsMacro), nameof(IsClearGroup), nameof(HasKey), nameof(HasWords))]
    private ShowControlKindOption selectedKind;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(CanDelete))]
    private ShowControl? selectedSaved;

    [ObservableProperty]
    private string name = string.Empty;

    [ObservableProperty]
    private string key = string.Empty;

    [ObservableProperty]
    private string words = string.Empty;

    [ObservableProperty]
    private decimal hideAfterSeconds;

    [ObservableProperty]
    private bool stageOnly;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(UsesMinutes), nameof(UsesTimeOfDay))]
    private TimerKindOption selectedTimerKind;

    [ObservableProperty]
    private decimal minutes = 5;

    [ObservableProperty]
    private string timeOfDay = "10:30";

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(RunsMacroAtZero))]
    private OverrunOption selectedOverrun;

    [ObservableProperty]
    private Macro? overrunMacro;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(StepNeedsTarget))]
    private MacroStepOption selectedStepKind;

    [ObservableProperty]
    private StepTarget? selectedStepTarget;

    [ObservableProperty]
    private bool stopAudio;

    [ObservableProperty]
    private bool stopTimers;

    [ObservableProperty]
    private bool hideLogo = true;

    [ObservableProperty]
    private string? statusText;

    [ObservableProperty]
    private bool statusIsError;

    public bool IsLook => SelectedKind.Kind == ShowControlKind.Look;

    public bool IsProp => SelectedKind.Kind == ShowControlKind.Prop;

    public bool IsMessage => SelectedKind.Kind == ShowControlKind.Message;

    public bool IsTimer => SelectedKind.Kind == ShowControlKind.Timer;

    public bool IsMacro => SelectedKind.Kind == ShowControlKind.Macro;

    public bool IsClearGroup => SelectedKind.Kind == ShowControlKind.ClearGroup;

    /// <summary>Props and messages have words and a time to stay up.</summary>
    public bool HasWords => IsProp || IsMessage;

    /// <summary>Looks, macros and clear groups can have a key.</summary>
    public bool HasKey => IsLook || IsMacro || IsClearGroup;

    public bool UsesMinutes => SelectedTimerKind.Kind == TimerKind.Countdown;

    public bool UsesTimeOfDay => SelectedTimerKind.Kind == TimerKind.CountToTime;

    public bool RunsMacroAtZero => SelectedOverrun.Mode == TimerOverrunMode.TriggerMacro;

    public bool StepNeedsTarget => NeedsTarget(SelectedStepKind.Kind);

    public bool HasSteps => Steps.Count > 0;

    public bool HasNoSaved => Saved.Count == 0;

    public bool CanDelete => SelectedSaved is not null;

    /// <summary>Load everything saved, the screens (for looks) and the stage layouts (for macros).</summary>
    public async Task RefreshAsync()
    {
        if (live is null)
        {
            return;
        }
        try
        {
            var found = new List<ShowControl>();
            found.AddRange((await live.ListLooksAsync()).Select(ShowControl.Of));
            found.AddRange((await live.ListPropsAsync()).Select(ShowControl.Of));
            found.AddRange((await live.ListMessagesAsync()).Select(ShowControl.Of));
            found.AddRange((await live.ListTimersAsync()).Select(ShowControl.Of));
            found.AddRange((await live.ListMacrosAsync()).Select(ShowControl.Of));
            found.AddRange((await live.ListClearGroupsAsync()).Select(ShowControl.Of));
            all = found;
            stageLayouts = [.. await live.ListStageLayoutsAsync()];
            var outputs = library is null ? [] : await library.GetOutputConfigsAsync();
            screens = [.. outputs.Where(o => o.Enabled && o.Role != OutputRole.Stage).Select(o => (o.OutputIndex, o.Name))];
        }
        catch (CoreException error)
        {
            showError(error);
            return;
        }
        Macros.Clear();
        foreach (var macro in all.Where(c => c.Kind == ShowControlKind.Macro).Select(c => c.Macro!))
        {
            Macros.Add(macro);
        }
        var keep = SelectedSaved?.Id.Value;
        ShowSaved();
        SelectedSaved = Saved.FirstOrDefault(c => keep is not null && c.Id.Value == keep);
        if (SelectedSaved is null)
        {
            New();
        }
        UpdateStepTargets();
    }

    partial void OnSelectedKindChanged(ShowControlKindOption value)
    {
        StatusText = null;
        ShowSaved();
        SelectedSaved = null;
        New();
    }

    partial void OnSelectedSavedChanged(ShowControl? value)
    {
        if (value is null || loading)
        {
            return;
        }
        loading = true;
        StatusText = null;
        Name = value.Name;
        Key = string.Empty;
        switch (value.Kind)
        {
            case ShowControlKind.Look:
                Key = value.Look!.Hotkey.TrimStart('⇧');
                SetScreens(value.Look.Rules);
                break;
            case ShowControlKind.Prop:
                Words = value.Prop!.Label;
                HideAfterSeconds = value.Prop.AutoClearSeconds;
                break;
            case ShowControlKind.Message:
                Words = value.Message!.TemplateText;
                HideAfterSeconds = value.Message.DurationSeconds;
                StageOnly = value.Message.StageOnly;
                break;
            case ShowControlKind.Timer:
                var timer = value.Timer!;
                SelectedTimerKind = TimerKinds.FirstOrDefault(k => k.Kind == timer.Kind) ?? TimerKinds[0];
                Minutes = Math.Max(1, (decimal)Math.Round(timer.DurationMs / 60000.0));
                TimeOfDay = TimeSpan.FromSeconds(timer.TargetTimeOfDaySeconds % 86400).ToString(@"hh\:mm", CultureInfo.InvariantCulture);
                SelectedOverrun = OverrunOptions.FirstOrDefault(o => o.Mode == timer.OverrunMode) ?? OverrunOptions[0];
                OverrunMacro = Macros.FirstOrDefault(m => LibraryIds.Same(m.Id, timer.OverrunMacroId));
                break;
            case ShowControlKind.Macro:
                Key = value.Macro!.Hotkey;
                Steps.Clear();
                foreach (var action in value.Macro.Actions)
                {
                    Steps.Add(new MacroStepRow(action.Clone(), Describe(action)));
                }
                OnPropertyChanged(nameof(HasSteps));
                break;
            case ShowControlKind.ClearGroup:
                var group = value.ClearGroup!;
                Key = group.Hotkey;
                foreach (var choice in ClearLayers)
                {
                    choice.IsChecked = group.LayersToClear.Contains(choice.Kind);
                }
                StopAudio = group.StopAudio;
                StopTimers = group.StopTimers;
                HideLogo = group.ClearShowLogo;
                break;
        }
        loading = false;
    }

    partial void OnSelectedStepKindChanged(MacroStepOption value) => UpdateStepTargets();

    /// <summary>Start a new one of the kind being made, with sensible starting values.</summary>
    [RelayCommand]
    private void New()
    {
        loading = true;
        SelectedSaved = null;
        Name = string.Empty;
        Key = string.Empty;
        Words = string.Empty;
        HideAfterSeconds = SelectedKind.Kind == ShowControlKind.Message ? 10 : 0;
        StageOnly = false;
        SelectedTimerKind = TimerKinds[0];
        Minutes = 5;
        TimeOfDay = "10:30";
        SelectedOverrun = OverrunOptions[0];
        OverrunMacro = null;
        Steps.Clear();
        OnPropertyChanged(nameof(HasSteps));
        foreach (var choice in ClearLayers)
        {
            choice.IsChecked = choice.Kind is LayerKind.Slide or LayerKind.Messages or LayerKind.Props;
        }
        StopAudio = false;
        StopTimers = false;
        HideLogo = true;
        SetScreens([]);
        loading = false;
    }

    [RelayCommand]
    private void AddStep()
    {
        StatusText = null;
        if (Steps.Count >= MaxSteps)
        {
            return;
        }
        var option = SelectedStepKind;
        var target = SelectedStepTarget;
        if (StepNeedsTarget && target is null)
        {
            Fail("tiles-error-target");
            return;
        }
        var action = new MacroAction { Id = LibraryIds.NewId(), Kind = option.Kind, BoolParam = option.ShowLogo };
        if (target is not null && StepNeedsTarget)
        {
            if (target.Id is { } id)
            {
                action.TargetId = id;
            }
            action.LayerKind = target.Layer;
        }
        Steps.Add(new MacroStepRow(action, Describe(action)));
        OnPropertyChanged(nameof(HasSteps));
    }

    [RelayCommand]
    private void RemoveStep(MacroStepRow? row)
    {
        if (row is not null && Steps.Remove(row))
        {
            OnPropertyChanged(nameof(HasSteps));
        }
    }

    [RelayCommand]
    private void MoveStepUp(MacroStepRow? row)
    {
        var index = row is null ? -1 : Steps.IndexOf(row);
        if (index > 0)
        {
            Steps.Move(index, index - 1);
        }
    }

    /// <summary>What would be saved now, or the message key that says what to fix.</summary>
    public (ShowControl? Control, string? Problem) Build()
    {
        var name = Name.Trim();
        if (name.Length == 0)
        {
            return (null, "tiles-error-name");
        }
        if (name.Length > MaxNameLength)
        {
            name = name[..MaxNameLength];
        }
        var key = Key.Trim().ToUpperInvariant();
        if (HasKey && key.Length > 0 && (key.Length != 1 || !char.IsAsciiLetterOrDigit(key[0])))
        {
            return (null, "tiles-error-key");
        }
        var id = SelectedSaved?.Id ?? LibraryIds.NewId();
        var seconds = (uint)Math.Clamp(HideAfterSeconds, 0, MaxSeconds);
        var words = Words.Trim();
        if (words.Length > MaxWordsLength)
        {
            words = words[..MaxWordsLength];
        }
        switch (SelectedKind.Kind)
        {
            case ShowControlKind.Look:
                var look = new Look { Id = id, Name = name, Hotkey = key.Length > 0 ? "⇧" + key : string.Empty };
                foreach (var screen in ScreenRules)
                {
                    var rule = new LookRule { Id = LibraryIds.NewId(), OutputIndex = screen.OutputIndex };
                    rule.EnabledLayers.AddRange(screen.Layers.Where(l => l.IsChecked).Select(l => l.Kind));
                    look.Rules.Add(rule);
                }
                return (ShowControl.Of(look), null);
            case ShowControlKind.Prop:
                return words.Length == 0
                    ? (null, "tiles-error-words")
                    : (ShowControl.Of(new Prop { Id = id, Name = name, Label = words, AutoClearSeconds = seconds }), null);
            case ShowControlKind.Message:
                return words.Length == 0
                    ? (null, "tiles-error-words")
                    : (ShowControl.Of(new LiveMessage { Id = id, Name = name, TemplateText = words, DurationSeconds = seconds, StageOnly = StageOnly }), null);
            case ShowControlKind.Timer:
                var timer = new Selah.Contracts.V1.Timer { Id = id, Name = name, Kind = SelectedTimerKind.Kind, OverrunMode = SelectedOverrun.Mode };
                if (UsesMinutes)
                {
                    if (Minutes < 1 || Minutes > MaxMinutes)
                    {
                        return (null, "tiles-error-minutes");
                    }
                    timer.DurationMs = (ulong)(Minutes * 60000);
                }
                if (UsesTimeOfDay)
                {
                    if (!TimeSpan.TryParseExact(TimeOfDay.Trim(), [@"h\:mm", @"hh\:mm"], CultureInfo.InvariantCulture, out var at) || at.TotalHours >= 24)
                    {
                        return (null, "tiles-error-time");
                    }
                    timer.TargetTimeOfDaySeconds = (uint)at.TotalSeconds;
                }
                if (RunsMacroAtZero)
                {
                    if (OverrunMacro?.Id is not { } macroId)
                    {
                        return (null, "tiles-error-target");
                    }
                    timer.OverrunMacroId = macroId;
                }
                return (ShowControl.Of(timer), null);
            case ShowControlKind.Macro:
                if (Steps.Count == 0)
                {
                    return (null, "tiles-error-steps");
                }
                var macro = new Macro { Id = id, Name = name, Hotkey = key };
                macro.Actions.AddRange(Steps.Select(s => s.Action.Clone()));
                return (ShowControl.Of(macro), null);
            default:
                if (!ClearLayers.Any(l => l.IsChecked) && !StopAudio && !StopTimers && !HideLogo)
                {
                    return (null, "tiles-error-layers");
                }
                var group = new ClearGroup { Id = id, Name = name, Hotkey = key, StopAudio = StopAudio, StopTimers = StopTimers, ClearShowLogo = HideLogo };
                group.LayersToClear.AddRange(ClearLayers.Where(l => l.IsChecked).Select(l => l.Kind));
                return (ShowControl.Of(group), null);
        }
    }

    [RelayCommand]
    private async Task SaveAsync()
    {
        if (live is null)
        {
            return;
        }
        var (control, problem) = Build();
        if (control is null)
        {
            Fail(problem!);
            return;
        }
        try
        {
            await live.SaveControlAsync(control);
            SelectedSaved = null;
            await RefreshAsync();
            loading = true;
            SelectedSaved = Saved.FirstOrDefault(c => c.Id.Value == control.Id.Value);
            loading = false;
            await tilesChanged();
            Done("tiles-saved");
        }
        catch (CoreException error)
        {
            showError(error);
        }
    }

    [RelayCommand]
    private async Task DeleteAsync()
    {
        if (live is null || SelectedSaved is not { } saved)
        {
            return;
        }
        try
        {
            await live.DeleteControlAsync(saved.Kind, saved.Id);
            SelectedSaved = null;
            await RefreshAsync();
            await tilesChanged();
            Done("tiles-deleted");
        }
        catch (CoreException error)
        {
            showError(error);
        }
    }

    private void Fail(string key)
    {
        StatusIsError = true;
        StatusText = Localizer.Instance[key];
    }

    private void Done(string key)
    {
        StatusIsError = false;
        StatusText = Localizer.Instance[key];
    }

    private void ShowSaved()
    {
        Saved.Clear();
        foreach (var control in all.Where(c => c.Kind == SelectedKind.Kind).OrderBy(c => c.Name, StringComparer.CurrentCultureIgnoreCase))
        {
            Saved.Add(control);
        }
        OnPropertyChanged(nameof(HasNoSaved));
    }

    private static ObservableCollection<LayerChoice> NewLayerChoices(bool isChecked) =>
    [
        .. AudienceLayers.Select(kind => new LayerChoice { Kind = kind, Name = LayerName(kind), IsChecked = isChecked }),
    ];

    private static string LayerName(LayerKind kind) => Localizer.Instance[kind switch
    {
        LayerKind.Messages => "live-layer-messages",
        LayerKind.Props => "live-layer-props",
        LayerKind.Slide => "live-layer-slide",
        LayerKind.Media => "live-layer-media",
        _ => "live-layer-video-input",
    }];

    /// <summary>One row per audience screen; a screen with no rule shows every layer, as in core.</summary>
    private void SetScreens(IEnumerable<LookRule> rules)
    {
        var known = screens.Count > 0 ? screens : [(0u, Localizer.Instance["tiles-look-screen-default"])];
        var byScreen = rules.GroupBy(r => r.OutputIndex).ToDictionary(g => g.Key, g => g.First());
        ScreenRules.Clear();
        foreach (var (index, screenName) in known)
        {
            var layers = NewLayerChoices(isChecked: true);
            if (byScreen.TryGetValue(index, out var rule))
            {
                foreach (var choice in layers)
                {
                    choice.IsChecked = rule.EnabledLayers.Contains(choice.Kind);
                }
            }
            ScreenRules.Add(new LookScreenRow
            {
                OutputIndex = index,
                Name = string.IsNullOrWhiteSpace(screenName) ? Localizer.Instance["tiles-look-screen-default"] : screenName,
                Layers = layers,
            });
        }
    }

    private static ShowControlKind? TargetKindFor(MacroActionKind kind) => kind switch
    {
        MacroActionKind.SetLook => ShowControlKind.Look,
        MacroActionKind.ShowProp => ShowControlKind.Prop,
        MacroActionKind.ShowMessage => ShowControlKind.Message,
        MacroActionKind.StartTimer or MacroActionKind.PauseTimer or MacroActionKind.ResetTimer => ShowControlKind.Timer,
        MacroActionKind.TriggerClearGroup => ShowControlKind.ClearGroup,
        _ => null,
    };

    /// <summary>Steps that use something: a saved show control, a layer or a stage layout.</summary>
    private static bool NeedsTarget(MacroActionKind kind) =>
        kind is MacroActionKind.ClearLayer or MacroActionKind.SetStageLayout || TargetKindFor(kind) is not null;

    private void UpdateStepTargets()
    {
        StepTargets.Clear();
        var kind = SelectedStepKind.Kind;
        if (kind == MacroActionKind.ClearLayer)
        {
            foreach (var layer in AudienceLayers)
            {
                StepTargets.Add(new StepTarget(LayerName(layer), Layer: layer));
            }
        }
        else if (kind == MacroActionKind.SetStageLayout)
        {
            foreach (var layout in stageLayouts)
            {
                StepTargets.Add(new StepTarget(layout.Name, layout.Id));
            }
        }
        else if (TargetKindFor(kind) is { } targetKind)
        {
            // A macro can't run another macro (core refuses it), so macros are never a target.
            foreach (var control in all.Where(c => c.Kind == targetKind).OrderBy(c => c.Name, StringComparer.CurrentCultureIgnoreCase))
            {
                StepTargets.Add(new StepTarget(control.Name, control.Id));
            }
        }
        SelectedStepTarget = StepTargets.FirstOrDefault();
        OnPropertyChanged(nameof(StepNeedsTarget));
    }

    private string Describe(MacroAction action)
    {
        var option = StepKinds.FirstOrDefault(o => o.Kind == action.Kind && (o.Kind != MacroActionKind.ShowLogo || o.ShowLogo == action.BoolParam));
        var step = option?.Name ?? action.Kind.ToString();
        string? target = action.Kind switch
        {
            MacroActionKind.ClearLayer => LayerName(action.LayerKind),
            MacroActionKind.SetStageLayout => stageLayouts.FirstOrDefault(s => LibraryIds.Same(s.Id, action.TargetId))?.Name,
            _ when NeedsTarget(action.Kind) => all.FirstOrDefault(c => LibraryIds.Same(c.Id, action.TargetId))?.Name,
            _ => null,
        };
        return target is null ? step : Localizer.Instance.Get("tiles-step-summary", ("step", step), ("target", target));
    }
}
