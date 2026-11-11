using System.Collections.ObjectModel;
using System.Text.Json;
using Avalonia.Media;
using Avalonia.Threading;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Selah.Contracts.V1;
using Selah.UI.Services;

namespace Selah.UI.ViewModels;

/// <summary>One row in the Live service order (prototype 01, left).</summary>
public sealed partial class LiveItemRow : ObservableObject
{
    public required uint Index { get; init; }

    public required string Title { get; init; }

    public required string Subtitle { get; init; }

    public required ServiceItemKind Kind { get; init; }

    public bool IsHeader => Kind == ServiceItemKind.Header;

    public string KindGlyph => Kind switch
    {
        ServiceItemKind.Song => "♫",
        ServiceItemKind.Scripture => "¶",
        ServiceItemKind.Media => "▶",
        ServiceItemKind.Presentation => "▤",
        ServiceItemKind.Countdown => "◷",
        ServiceItemKind.Audio => "♪",
        _ => "•",
    };

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(StateText))]
    private bool isLive;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(StateText))]
    private bool isNext;

    [ObservableProperty]
    private bool isDone;

    /// <summary>LIVE and NEXT are words, never colour alone (DESIGN-SYSTEM §9).</summary>
    public string StateText => IsLive ? Localizer.Instance["live-tag-live"] : IsNext ? Localizer.Instance["live-tag-next"] : string.Empty;
}

/// <summary>One slide card in the Live slide grid.</summary>
public sealed partial class LiveSlideCard : ObservableObject
{
    public required uint ItemIndex { get; init; }

    public required uint SlideIndex { get; init; }

    public required string Label { get; init; }

    public required string Text { get; init; }

    public required string SectionColor { get; init; }

    public required string Hotkey { get; init; }

    public required string ActionBadge { get; init; }

    public bool HasHotkey => Hotkey.Length > 0;

    public bool HasActionBadge => ActionBadge.Length > 0;

    public string Number => (SlideIndex + 1).ToString(System.Globalization.CultureInfo.CurrentCulture);

    public string AccessibleName => Localizer.Instance.Get("live-slide-accessible", ("label", Label), ("number", Number), ("text", Text));

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(StateText))]
    private bool isLive;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(StateText))]
    private bool isNext;

    public string StateText => IsLive ? Localizer.Instance["live-tag-live"] : IsNext ? Localizer.Instance["live-tag-next"] : string.Empty;
}

/// <summary>One layer row: indicator, name, detail and its Clear button (prototype 01, right).</summary>
public sealed partial class LiveLayerRow : ObservableObject
{
    public required LayerKind Kind { get; init; }

    public required string Name { get; init; }

    public required string ClearText { get; init; }

    [ObservableProperty]
    private bool isActive;

    [ObservableProperty]
    private string detail = string.Empty;
}

public enum LiveTileKind
{
    Look,
    Timer,
    Macro,
}

/// <summary>A tile for a look, timer or macro.</summary>
public sealed partial class LiveTile : ObservableObject
{
    public required LiveTileKind Kind { get; init; }

    public required Id Id { get; init; }

    public required string Caption { get; init; }

    public required string Hotkey { get; init; }

    [ObservableProperty]
    private string value = string.Empty;

    [ObservableProperty]
    private bool isOn;

    public bool HasHotkey => Hotkey.Length > 0;
}

/// <summary>One output in the health bar: a dot plus words (DESIGN-SYSTEM §6).</summary>
public sealed record LiveHealthItem(string Name, bool Ok, string StateText);

/// <summary>A saved Producer layout: pane sizes and the selected tab in each dock (M4.4).</summary>
public sealed record ProducerLayoutState(string Name, double ServiceOrder, double Slides, double Inspector, double Program, string InspectorTab)
{
    public static ProducerLayoutState Standard { get; } = new("standard", 0.22, 0.48, 0.30, 0.55, "layers");

    public static ProducerLayoutState WideSlides { get; } = new("wide-slides", 0.16, 0.60, 0.24, 0.50, "layers");

    public static ProducerLayoutState ProgramFocus { get; } = new("program-focus", 0.18, 0.37, 0.45, 0.70, "tiles");

    public static IReadOnlyList<ProducerLayoutState> Presets { get; } = [Standard, WideSlides, ProgramFocus];

    /// <summary>Keep sizes sane whatever the file says: each pane gets at least 10%.</summary>
    public ProducerLayoutState Normalised()
    {
        static double Clamp(double v) => double.IsFinite(v) ? Math.Clamp(v, 0.10, 0.80) : 0.33;
        var a = Clamp(ServiceOrder);
        var b = Clamp(Slides);
        var c = Clamp(Inspector);
        var sum = a + b + c;
        var tab = InspectorTab is "layers" or "tiles" or "stage" ? InspectorTab : "layers";
        return this with { ServiceOrder = a / sum, Slides = b / sum, Inspector = c / sum, Program = Clamp(Program), InspectorTab = tab };
    }
}

/// <summary>
/// Producer mode (M4.4, prototype 01): service order, slide grid, program, layers, and tiles for
/// looks, timers and macros, all keyboard-first. Every action is a core command (SEC-14); core's
/// reply is the truth the view shows.
/// </summary>
public sealed partial class LiveProducerViewModel : ObservableObject
{
    private static readonly JsonSerializerOptions LayoutJson = new() { WriteIndented = true };
    private readonly ILiveService? live;
    private readonly ILibraryService? library;
    private readonly string? layoutPath;
    private Service? loadedService;

    public LiveProducerViewModel(ILiveService? live, ILibraryService? library, string? layoutPath = null)
    {
        this.live = live;
        this.library = library;
        this.layoutPath = layoutPath;
        Layers =
        [
            NewLayer(LayerKind.Messages, "live-layer-messages", "live-clear"),
            NewLayer(LayerKind.Props, "live-layer-props", "live-clear"),
            NewLayer(LayerKind.Slide, "live-layer-slide", "live-clear-text-key"),
            NewLayer(LayerKind.Media, "live-layer-media", "live-clear-background-key"),
            NewLayer(LayerKind.VideoInput, "live-layer-video-input", "live-clear"),
        ];
        Stage = new StageEditorViewModel(live, ShowError);
        Layout = LoadLayout();
        Dock = new ProducerDockFactory(this);
        DockLayout = Dock.Build(Layout);
        if (live is not null)
        {
            live.LiveStatusChanged += (_, status) => Dispatcher.UIThread.Post(() => Apply(status));
        }
    }

    private static LiveLayerRow NewLayer(LayerKind kind, string nameKey, string clearKey) =>
        new() { Kind = kind, Name = Localizer.Instance[nameKey], ClearText = Localizer.Instance[clearKey] };

    public ObservableCollection<LiveItemRow> Items { get; } = [];

    public ObservableCollection<LiveSlideCard> Slides { get; } = [];

    public ObservableCollection<LiveLayerRow> Layers { get; }

    public ObservableCollection<LiveTile> Tiles { get; } = [];

    public ObservableCollection<LiveHealthItem> Health { get; } = [];

    public ObservableCollection<Service> Services { get; } = [];

    public IReadOnlyList<ProducerLayoutState> LayoutPresets => ProducerLayoutState.Presets;

    public ProducerDockFactory Dock { get; }

    /// <summary>Bring forward the inspector tab that holds a help anchor (tours, M4.7).</summary>
    public void ShowPaneFor(string anchor)
    {
        var id = anchor switch
        {
            "live.tiles" => "tiles",
            "live.layers" => "layers",
            "live.stage-editor" or "live.stage-preview" => "stage",
            _ => null,
        };
        if (id is null)
        {
            return;
        }
        Dock.Model.Core.IDockable? Find(Dock.Model.Core.IDockable node) =>
            node is ProducerPane pane && pane.Id == id ? pane
            : node is Dock.Model.Core.IDock { VisibleDockables: { } children } ? children.Select(Find).FirstOrDefault(f => f is not null)
            : null;
        if (Find(DockLayout) is { } found)
        {
            this.Dock.SetActiveDockable(found);
        }
    }

    /// <summary>The stage display editor (M4.6), the Stage tab of the inspector.</summary>
    public StageEditorViewModel Stage { get; }

    [ObservableProperty]
    private Dock.Model.Controls.IRootDock dockLayout;

    [ObservableProperty]
    private ProducerLayoutState layout;

    [ObservableProperty]
    private LiveStatus status = new();

    [ObservableProperty]
    private string serviceName = string.Empty;

    [ObservableProperty]
    private string currentItemTitle = string.Empty;

    [ObservableProperty]
    private string currentItemEyebrow = string.Empty;

    [ObservableProperty]
    private string nextText = string.Empty;

    [ObservableProperty]
    private string activeLookName = string.Empty;

    [ObservableProperty]
    private bool canUndo;

    [ObservableProperty]
    private bool isBoothLocked;

    [ObservableProperty]
    private bool isShowingLogo;

    /// <summary>Core's Simple-mode role lock (SEC-14). The UI follows it; core enforces it.</summary>
    [ObservableProperty]
    private bool isRoleLocked;

    [ObservableProperty]
    private bool isLocking;

    [ObservableProperty]
    private string lockPin = string.Empty;

    [ObservableProperty]
    private string boothMessage = string.Empty;

    [ObservableProperty]
    private string? errorText;

    [ObservableProperty]
    private string? errorFixArticleId;

    /// <summary>The newest program frame from the engine's preview ring (set by the window).</summary>
    [ObservableProperty]
    private IImage? programFrame;

    [ObservableProperty]
    private Service? selectedService;

    /// <summary>Screen readers hear slide changes through a polite live region (DESIGN-SYSTEM §9).</summary>
    [ObservableProperty]
    private string announcement = string.Empty;

    public bool HasBoothMessage => BoothMessage.Length > 0;

    public bool HasService => loadedService is not null || Status.ServiceId is { Value.IsEmpty: false };

    partial void OnBoothMessageChanged(string value) => OnPropertyChanged(nameof(HasBoothMessage));

    /// <summary>Load the services list and the current live state.</summary>
    public async Task RefreshAsync()
    {
        if (library is not null)
        {
            try
            {
                var services = await library.ListServicesAsync();
                Services.Clear();
                foreach (var service in services)
                {
                    Services.Add(service);
                }
                SelectedService ??= Services.FirstOrDefault();
            }
            catch (CoreException error)
            {
                ShowError(error);
            }
        }
        await LoadTilesAsync();
        if (live is not null)
        {
            await RunAsync(live.GetLiveStateAsync);
        }
        await Stage.RefreshAsync();
    }

    private async Task LoadTilesAsync()
    {
        if (live is null)
        {
            return;
        }
        try
        {
            var looks = await live.ListLooksAsync();
            var timers = await live.ListTimersAsync();
            var macros = await live.ListMacrosAsync();
            Tiles.Clear();
            foreach (var look in looks)
            {
                Tiles.Add(new LiveTile { Kind = LiveTileKind.Look, Id = look.Id, Caption = Localizer.Instance["live-tile-look"], Hotkey = look.Hotkey, Value = look.Name });
            }
            foreach (var timer in timers)
            {
                Tiles.Add(new LiveTile { Kind = LiveTileKind.Timer, Id = timer.Id, Caption = timer.Name, Hotkey = string.Empty, Value = LiveFormat.Duration(timer.DurationMs) });
            }
            foreach (var macro in macros)
            {
                Tiles.Add(new LiveTile { Kind = LiveTileKind.Macro, Id = macro.Id, Caption = Localizer.Instance["live-tile-macro"], Hotkey = macro.Hotkey, Value = macro.Name });
            }
        }
        catch (CoreException error)
        {
            ShowError(error);
        }
    }

    /// <summary>Apply core's answer. Everything on this screen follows from <see cref="LiveStatus"/>.</summary>
    public void Apply(LiveStatus next)
    {
        var itemChanged = next.CurrentItemIndex != Status.CurrentItemIndex || next.ServiceId?.Value != Status.ServiceId?.Value
            || next.CurrentItemSlides.Count != Slides.Count;
        var slideChanged = itemChanged || next.CurrentSlideIndex != Status.CurrentSlideIndex;
        Status = next;
        Stage.Apply(next);
        ServiceName = next.ServiceName;
        CurrentItemTitle = next.CurrentItemTitle;
        CanUndo = next.CanLiveUndo;
        IsShowingLogo = next.ShowLogo;
        NextText = next.HasNext ? Localizer.Instance.Get("live-next-up", ("title", next.NextItemTitle), ("label", next.NextSlideLabel)) : Localizer.Instance["live-next-none"];
        var automation = next.Automation.FirstOrDefault();
        IsBoothLocked = automation?.BoothLocked ?? false;
        IsRoleLocked = automation?.RoleLocked ?? false;
        ActiveLookName = automation?.ActiveLookName ?? string.Empty;
        BoothMessage = automation?.BoothMessage ?? string.Empty;

        if (itemChanged)
        {
            Slides.Clear();
            foreach (var slide in next.CurrentItemSlides)
            {
                Slides.Add(new LiveSlideCard
                {
                    ItemIndex = next.CurrentItemIndex,
                    SlideIndex = slide.Index,
                    Label = slide.Label,
                    Text = slide.IsBlank ? Localizer.Instance["live-slide-blank"] : slide.Text,
                    SectionColor = string.IsNullOrEmpty(slide.SectionColor) ? "#71767A" : slide.SectionColor,
                    Hotkey = slide.Hotkey,
                    ActionBadge = slide.ActionBadge,
                });
            }
        }
        foreach (var card in Slides)
        {
            card.IsLive = next.HasActiveItem && !next.Cleared && card.SlideIndex == next.CurrentSlideIndex;
            card.IsNext = next.HasNext && next.NextItemIndex == next.CurrentItemIndex && card.SlideIndex == next.NextSlideIndex;
        }
        // The service order marks the next item, even while the next slide is still in this one.
        var nextItem = Items.FirstOrDefault(i => !i.IsHeader && i.Index > next.CurrentItemIndex);
        foreach (var row in Items)
        {
            row.IsLive = next.HasActiveItem && row.Index == next.CurrentItemIndex;
            row.IsNext = next.HasActiveItem && ReferenceEquals(row, nextItem);
            row.IsDone = next.HasActiveItem && row.Index < next.CurrentItemIndex;
        }
        var kind = Items.FirstOrDefault(i => i.Index == next.CurrentItemIndex)?.Kind ?? ServiceItemKind.Unspecified;
        CurrentItemEyebrow = next.HasActiveItem ? Localizer.Instance.Get("live-now-eyebrow", ("kind", LiveFormat.KindName(kind))) : Localizer.Instance["live-nothing-live"];

        foreach (var layer in Layers)
        {
            var source = next.Layers.FirstOrDefault(l => l.Kind == layer.Kind);
            layer.IsActive = source?.Active ?? false;
            layer.Detail = source is { Active: true } ? Join(source.Title, source.Subtitle) : Localizer.Instance["live-layer-empty"];
        }
        foreach (var tile in Tiles)
        {
            switch (tile.Kind)
            {
                case LiveTileKind.Look:
                    tile.IsOn = automation?.ActiveLookId is { } look && LibraryIds.Same(look, tile.Id);
                    break;
                case LiveTileKind.Timer when automation?.Timers.FirstOrDefault(t => LibraryIds.Same(t.TimerId, tile.Id)) is { } timer:
                    tile.Value = timer.FormattedTime;
                    tile.IsOn = timer.Running;
                    break;
            }
        }
        if (slideChanged && next.HasActiveItem && !next.Cleared)
        {
            Announcement = Localizer.Instance.Get("live-announce-slide", ("label", next.CurrentSlideLabel), ("number", (next.CurrentSlideIndex + 1).ToString(System.Globalization.CultureInfo.CurrentCulture)));
        }
        else if (next.Cleared)
        {
            Announcement = Localizer.Instance["live-announce-cleared"];
        }
        OnPropertyChanged(nameof(HasService));
    }

    private static string Join(string a, string b) => string.IsNullOrEmpty(b) ? a : string.IsNullOrEmpty(a) ? b : $"{a} · {b}";

    /// <summary>Health bar: one entry per output, with words (from the engine's output statuses).</summary>
    public void ApplyHealth(IEnumerable<(string Name, bool Ok)> outputs)
    {
        Health.Clear();
        foreach (var (name, ok) in outputs)
        {
            Health.Add(new LiveHealthItem(name, ok, Localizer.Instance[ok ? "live-health-ok" : "live-health-problem"]));
        }
    }

    [RelayCommand]
    private async Task GoLiveWithServiceAsync()
    {
        if (live is null || SelectedService is not { } service)
        {
            return;
        }
        await LoadServiceAsync(service);
    }

    /// <summary>Load a service into Live (from the planner's <b>Go live</b> or the picker).</summary>
    public async Task LoadServiceAsync(Service service, bool triggerFirst = false)
    {
        loadedService = service;
        Items.Clear();
        for (var i = 0; i < service.Items.Count; i++)
        {
            var item = service.Items[i];
            Items.Add(new LiveItemRow { Index = (uint)i, Title = item.Title, Subtitle = item.Subtitle, Kind = item.Kind });
        }
        if (live is not null)
        {
            await RunAsync(() => live.LoadLiveServiceAsync(service.Id, triggerFirst));
        }
    }

    [RelayCommand]
    private Task NextAsync() => RunAsync(() => live!.NextSlideAsync());

    [RelayCommand]
    private Task PreviousAsync() => RunAsync(() => live!.PreviousSlideAsync());

    [RelayCommand]
    private Task NextItemAsync() => RunAsync(() => live!.NextItemAsync());

    [RelayCommand]
    private Task PreviousItemAsync() => RunAsync(() => live!.PreviousItemAsync());

    [RelayCommand]
    private Task TriggerSlideAsync(LiveSlideCard? card) =>
        card is null ? Task.CompletedTask : RunAsync(() => live!.TriggerSlideAsync(card.ItemIndex, card.SlideIndex));

    [RelayCommand]
    private Task TriggerItemAsync(LiveItemRow? row) =>
        row is null || row.IsHeader ? Task.CompletedTask : RunAsync(() => live!.TriggerItemAsync(row.Index));

    [RelayCommand]
    private Task ClearLayerAsync(LiveLayerRow? row) =>
        row is null ? Task.CompletedTask : RunAsync(() => row.Kind == LayerKind.Messages ? live!.HideMessageAsync()
            : row.Kind == LayerKind.Props ? live!.HidePropAsync() : live!.ClearLayerAsync(row.Kind));

    /// <summary>F10.</summary>
    [RelayCommand]
    private Task ClearTextAsync() => RunAsync(() => live!.ClearLayerAsync(LayerKind.Slide));

    /// <summary>F11.</summary>
    [RelayCommand]
    private Task ClearBackgroundAsync() => RunAsync(() => live!.ClearLayerAsync(LayerKind.Media));

    /// <summary>F9 toggles the logo.</summary>
    [RelayCommand]
    private Task ShowLogoAsync() => RunAsync(() => live!.ShowLogoAsync(!IsShowingLogo));

    /// <summary>F12 and a held Esc: the priority lane, no confirmation (SAFE-03).</summary>
    [RelayCommand]
    private async Task ClearAllAsync()
    {
        if (live is not null)
        {
            await live.ClearEverythingAsync();
        }
    }

    [RelayCommand]
    private Task UndoAsync() => RunAsync(() => live!.LiveUndoAsync());

    [RelayCommand]
    private Task RedoAsync() => RunAsync(() => live!.LiveRedoAsync());

    /// <summary>Ctrl/Cmd+L (SAFE-06).</summary>
    [RelayCommand]
    private Task ToggleBoothLockAsync() => RunAsync(() => live!.SetBoothLockAsync(!IsBoothLocked));

    [RelayCommand]
    private void StartLock()
    {
        IsLocking = true;
        LockPin = string.Empty;
    }

    [RelayCommand]
    private void CancelLock()
    {
        IsLocking = false;
        LockPin = string.Empty;
    }

    /// <summary>Lock to Simple mode (SEC-14): core keeps the state and a salted hash of the PIN.</summary>
    [RelayCommand]
    private async Task LockAsync()
    {
        var pin = LockPin;
        LockPin = string.Empty;
        await RunAsync(() => live!.SetRoleLockAsync(true, pin));
        if (ErrorText is null)
        {
            IsLocking = false;
        }
    }

    [RelayCommand]
    private Task DismissBoothMessageAsync()
    {
        BoothMessage = string.Empty;
        return Task.CompletedTask;
    }

    [RelayCommand]
    private Task ActivateTileAsync(LiveTile? tile) => tile switch
    {
        null => Task.CompletedTask,
        { Kind: LiveTileKind.Look } => RunAsync(() => live!.SetLookAsync(tile.Id)),
        { Kind: LiveTileKind.Macro } => RunAsync(() => live!.RunMacroAsync(tile.Id)),
        { Kind: LiveTileKind.Timer, IsOn: true } => RunAsync(() => live!.PauseTimerAsync(tile.Id)),
        _ => RunAsync(() => live!.StartTimerAsync(tile.Id)),
    };

    /// <summary>Section hotkeys (1–9, letters) and look hotkeys (Shift+1–9).</summary>
    public Task HotkeyAsync(string key, bool shift)
    {
        if (shift)
        {
            var look = Tiles.Where(t => t.Kind == LiveTileKind.Look).FirstOrDefault(t => string.Equals(t.Hotkey.TrimStart('⇧'), key, StringComparison.OrdinalIgnoreCase));
            return look is null ? Task.CompletedTask : ActivateTileAsync(look);
        }
        var macro = Tiles.FirstOrDefault(t => t.Kind == LiveTileKind.Macro && string.Equals(t.Hotkey, key, StringComparison.OrdinalIgnoreCase));
        if (macro is not null && !Slides.Any(s => string.Equals(s.Hotkey, key, StringComparison.OrdinalIgnoreCase)))
        {
            return ActivateTileAsync(macro);
        }
        return RunAsync(() => live!.TriggerSectionHotkeyAsync(key));
    }

    [RelayCommand]
    private void ApplyLayoutPreset(ProducerLayoutState? preset)
    {
        if (preset is null)
        {
            return;
        }
        Layout = preset.Normalised();
        DockLayout = Dock.Build(Layout);
    }

    /// <summary>Save the current pane sizes and tabs (read back from the dock) as the layout.</summary>
    [RelayCommand]
    private void SaveLayout()
    {
        Layout = Dock.Capture().Normalised();
        if (layoutPath is null)
        {
            return;
        }
        try
        {
            Directory.CreateDirectory(Path.GetDirectoryName(layoutPath)!);
            var temp = layoutPath + ".tmp";
            File.WriteAllText(temp, JsonSerializer.Serialize(Layout, LayoutJson));
            File.Move(temp, layoutPath, overwrite: true);
        }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException)
        {
            ErrorText = Localizer.Instance["live-layout-save-failed"];
        }
    }

    private ProducerLayoutState LoadLayout()
    {
        if (layoutPath is null || !File.Exists(layoutPath))
        {
            return ProducerLayoutState.Standard;
        }
        try
        {
            var info = new FileInfo(layoutPath);
            // Outside input is parsed with limits (ADR-026): a layout file is tiny.
            if (info.Length > 16 * 1024)
            {
                return ProducerLayoutState.Standard;
            }
            return JsonSerializer.Deserialize<ProducerLayoutState>(File.ReadAllText(layoutPath))?.Normalised() ?? ProducerLayoutState.Standard;
        }
        catch (Exception error) when (error is IOException or JsonException or UnauthorizedAccessException)
        {
            return ProducerLayoutState.Standard;
        }
    }

    private async Task RunAsync(Func<Task<LiveStatus>> action)
    {
        if (live is null)
        {
            return;
        }
        try
        {
            ErrorText = null;
            ErrorFixArticleId = null;
            Apply(await action());
        }
        catch (CoreException error)
        {
            ShowError(error);
        }
        catch (Exception error) when (error is TimeoutException or IOException or InvalidOperationException)
        {
            ErrorText = Localizer.Instance["live-error-core-busy"];
        }
    }

    private void ShowError(CoreException error)
    {
        // SEC-14: a refused command says so plainly, and the booth lock says why.
        ErrorText = error.Code == "SEL-SEC-0001"
            ? Localizer.Instance[IsBoothLocked ? "live-error-booth-locked" : "live-error-not-permitted"]
            : Localizer.Instance.Get(error.MessageKey);
        ErrorFixArticleId = error.FixArticleId;
    }
}

/// <summary>Formatting shared by the Live views.</summary>
public static class LiveFormat
{
    public static string Duration(ulong milliseconds)
    {
        var span = TimeSpan.FromMilliseconds(milliseconds);
        return span.TotalHours >= 1
            ? span.ToString(@"h\:mm\:ss", System.Globalization.CultureInfo.InvariantCulture)
            : span.ToString(@"mm\:ss", System.Globalization.CultureInfo.InvariantCulture);
    }

    public static string KindName(ServiceItemKind kind) => Localizer.Instance[kind switch
    {
        ServiceItemKind.Song => "live-kind-song",
        ServiceItemKind.Scripture => "live-kind-scripture",
        ServiceItemKind.Media => "live-kind-media",
        ServiceItemKind.Presentation => "live-kind-presentation",
        ServiceItemKind.Countdown => "live-kind-countdown",
        ServiceItemKind.Audio => "live-kind-audio",
        _ => "live-kind-item",
    }];
}
