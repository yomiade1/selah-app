using System.Collections.ObjectModel;
using Avalonia;
using Rect = Selah.Contracts.V1.Rect;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Selah.Contracts.V1;
using Selah.UI.Services;

namespace Selah.UI.ViewModels;

/// <summary>Where the stage elements go. Each arrangement also sets sensible tick boxes.</summary>
public enum StageArrangement
{
    Standard,
    LyricsOnly,
    Speaker,
}

public sealed record StageArrangementOption(StageArrangement Value, string Name);

/// <summary>One box in the editor's stage preview, in preview pixels (320 × 180).</summary>
public sealed record StagePreviewBox(string Label, Thickness Margin, double Width, double Height, bool IsWords);

/// <summary>
/// The stage layout editor (M4.6, prototype screen 06): what singers and speakers see on the
/// stage screens. Core draws the stage outputs from the saved layout; this only edits it.
/// </summary>
public sealed partial class StageEditorViewModel : ObservableObject
{
    /// <summary>The preview canvas size, in the same 16:9 as the stage output.</summary>
    public const double PreviewWidth = 320;
    public const double PreviewHeight = 180;
    private const int MaxNameLength = 80;

    private readonly ILiveService? live;
    private readonly Action<CoreException> showError;
    private bool loading;

    public StageEditorViewModel(ILiveService? live, Action<CoreException> showError)
    {
        this.live = live;
        this.showError = showError;
        Arrangements =
        [
            new(StageArrangement.Standard, Localizer.Instance["stage-arrangement-standard"]),
            new(StageArrangement.LyricsOnly, Localizer.Instance["stage-arrangement-lyrics"]),
            new(StageArrangement.Speaker, Localizer.Instance["stage-arrangement-speaker"]),
        ];
        selectedArrangement = Arrangements[0];
        name = Localizer.Instance["stage-default-layout-name"];
        UpdatePreview();
    }

    public IReadOnlyList<StageArrangementOption> Arrangements { get; }

    public ObservableCollection<StageLayout> Layouts { get; } = [];

    public ObservableCollection<StagePreviewBox> Preview { get; } = [];

    [ObservableProperty]
    private StageLayout? selectedLayout;

    [ObservableProperty]
    private StageArrangementOption selectedArrangement;

    [ObservableProperty]
    private string name;

    [ObservableProperty]
    private bool showChords = true;

    [ObservableProperty]
    private bool showKey = true;

    [ObservableProperty]
    private bool showSections = true;

    [ObservableProperty]
    private bool showClock = true;

    [ObservableProperty]
    private bool showNext = true;

    [ObservableProperty]
    private bool showTimer = true;

    [ObservableProperty]
    private bool showNotes;

    [ObservableProperty]
    private bool showBooth = true;

    /// <summary>Text size on the stage, 50–200 %.</summary>
    [ObservableProperty]
    private double textSize = 100;

    /// <summary>The name of the layout on the stage screens now (from core).</summary>
    [ObservableProperty]
    private string activeLayoutName = string.Empty;

    [ObservableProperty]
    private string? statusText;

    public bool IsInUse => SelectedLayout?.Id is { } id && !id.Value.IsEmpty && ActiveLayoutId?.Value == id.Value;

    private Id? ActiveLayoutId { get; set; }

    public string InUseText => Localizer.Instance["stage-editor-in-use"];

    /// <summary>Load the saved layouts.</summary>
    public async Task RefreshAsync()
    {
        if (live is null)
        {
            return;
        }
        try
        {
            var layouts = await live.ListStageLayoutsAsync();
            var keep = SelectedLayout?.Id?.Value;
            Layouts.Clear();
            foreach (var layout in layouts)
            {
                Layouts.Add(layout);
            }
            SelectedLayout = Layouts.FirstOrDefault(l => keep is not null && l.Id?.Value == keep)
                ?? Layouts.FirstOrDefault(l => ActiveLayoutId is not null && l.Id?.Value == ActiveLayoutId.Value);
        }
        catch (CoreException error)
        {
            showError(error);
        }
    }

    /// <summary>Follow core: which layout is on the stage screens.</summary>
    public void Apply(LiveStatus status)
    {
        var automation = status.Automation.FirstOrDefault();
        ActiveLayoutId = automation?.ActiveStageLayoutId;
        ActiveLayoutName = automation?.ActiveStageLayoutName ?? string.Empty;
        OnPropertyChanged(nameof(IsInUse));
    }

    partial void OnSelectedLayoutChanged(StageLayout? value)
    {
        OnPropertyChanged(nameof(IsInUse));
        if (value is null)
        {
            return;
        }
        loading = true;
        Name = value.Name;
        ShowChords = value.ShowChords;
        ShowKey = value.ShowKeyBadge;
        ShowSections = value.ShowArrangementStrip;
        ShowClock = value.ShowClock;
        ShowNext = value.ShowNextSlide;
        ShowTimer = value.ShowTimer;
        ShowNotes = value.ShowSpeakerNotes;
        ShowBooth = value.ShowBoothMessage;
        var words = value.Elements.FirstOrDefault(e => e.Kind == StageElementKind.CurrentSlide);
        TextSize = words is { FontScale: > 0 } ? Math.Round(words.FontScale * 100) : 100;
        var notes = value.Elements.FirstOrDefault(e => e.Kind == StageElementKind.SpeakerNotes);
        SelectedArrangement = Arrangements[notes is { Rect.Width: > 0 } && words is { Rect.Width: < 0.9f } ? 2 : value.ShowChords ? 0 : 1];
        loading = false;
        UpdatePreview();
    }

    partial void OnSelectedArrangementChanged(StageArrangementOption value)
    {
        if (!loading)
        {
            // Each arrangement starts from sensible tick boxes; the person can change them after.
            (ShowChords, ShowKey, ShowSections, ShowNext, ShowTimer, ShowNotes) = value.Value switch
            {
                StageArrangement.LyricsOnly => (false, false, true, true, false, false),
                StageArrangement.Speaker => (false, false, false, false, true, true),
                _ => (true, true, true, true, true, false),
            };
        }
        UpdatePreview();
    }

    partial void OnShowChordsChanged(bool value) => UpdatePreview();

    partial void OnShowKeyChanged(bool value) => UpdatePreview();

    partial void OnShowSectionsChanged(bool value) => UpdatePreview();

    partial void OnShowClockChanged(bool value) => UpdatePreview();

    partial void OnShowNextChanged(bool value) => UpdatePreview();

    partial void OnShowTimerChanged(bool value) => UpdatePreview();

    partial void OnShowNotesChanged(bool value) => UpdatePreview();

    partial void OnShowBoothChanged(bool value) => UpdatePreview();

    partial void OnTextSizeChanged(double value) => UpdatePreview();

    /// <summary>The layout as it would be saved now.</summary>
    public StageLayout BuildLayout()
    {
        var scale = (float)Math.Clamp(TextSize, 50, 200) / 100f;
        var name = Name.Trim();
        if (name.Length == 0)
        {
            name = Localizer.Instance["stage-default-layout-name"];
        }
        if (name.Length > MaxNameLength)
        {
            name = name[..MaxNameLength];
        }
        var layout = new StageLayout
        {
            Id = SelectedLayout?.Id ?? LibraryIds.NewId(),
            Name = name,
            ShowChords = ShowChords,
            ShowKeyBadge = ShowKey,
            ShowArrangementStrip = ShowSections,
            ShowClock = ShowClock,
            ShowNextSlide = ShowNext,
            ShowTimer = ShowTimer,
            ShowSpeakerNotes = ShowNotes,
            ShowBoothMessage = ShowBooth,
        };
        foreach (var kind in new[] { StageElementKind.CurrentSlide, StageElementKind.NextSlide, StageElementKind.SpeakerNotes, StageElementKind.Timer })
        {
            layout.Elements.Add(new StageElement
            {
                Id = LibraryIds.NewId(),
                Kind = kind,
                Rect = StageGeometry.RectFor(SelectedArrangement.Value, kind),
                Visible = true,
                FontScale = scale,
                ShowChords = kind == StageElementKind.CurrentSlide,
            });
        }
        return layout;
    }

    [RelayCommand]
    private async Task SaveAsync()
    {
        if (live is null)
        {
            return;
        }
        try
        {
            StatusText = null;
            var layout = BuildLayout();
            var wasInUse = IsInUse;
            await live.SaveStageLayoutAsync(layout);
            SelectedLayout = null;
            await RefreshAsync();
            SelectedLayout = Layouts.FirstOrDefault(l => l.Id?.Value == layout.Id.Value);
            if (wasInUse)
            {
                // Core keeps its own copy of the layout in use: send the new one.
                Apply(await live.SetStageLayoutAsync(layout.Id));
            }
            StatusText = Localizer.Instance["stage-editor-saved"];
        }
        catch (CoreException error)
        {
            showError(error);
        }
    }

    [RelayCommand]
    private async Task UseAsync()
    {
        if (live is null)
        {
            return;
        }
        if (SelectedLayout is null)
        {
            await SaveAsync();
        }
        if (SelectedLayout?.Id is not { } id)
        {
            return;
        }
        try
        {
            Apply(await live.SetStageLayoutAsync(id));
        }
        catch (CoreException error)
        {
            showError(error);
        }
    }

    private void UpdatePreview()
    {
        Preview.Clear();
        var arrangement = SelectedArrangement?.Value ?? StageArrangement.Standard;
        void Add(string key, Rect rect, bool words = false)
        {
            var x = rect.X * PreviewWidth;
            var y = rect.Y * PreviewHeight;
            Preview.Add(new StagePreviewBox(Localizer.Instance[key], new Thickness(x, y, 0, 0), Math.Max(rect.Width * PreviewWidth, 1), Math.Max(rect.Height * PreviewHeight, 1), words));
        }
        Add("stage-preview-header", StageGeometry.RectFor(arrangement, StageElementKind.HeaderBar));
        Add("stage-preview-current", StageGeometry.RectFor(arrangement, StageElementKind.CurrentSlide), words: true);
        if (ShowNext)
        {
            Add("stage-preview-next", StageGeometry.RectFor(arrangement, StageElementKind.NextSlide));
        }
        if (ShowNotes)
        {
            Add("stage-preview-notes", StageGeometry.RectFor(arrangement, StageElementKind.SpeakerNotes));
        }
        if (ShowTimer)
        {
            Add("stage-preview-timer", StageGeometry.RectFor(arrangement, StageElementKind.Timer));
        }
        if (ShowBooth)
        {
            Add("stage-preview-booth", StageGeometry.RectFor(arrangement, StageElementKind.BoothMessage));
        }
    }
}

/// <summary>
/// Where stage elements sit, as fractions of the screen. The Standard positions match core's
/// defaults (<c>crates/selah-core/src/stage.rs</c>, prototype screen 06).
/// </summary>
public static class StageGeometry
{
    private static Rect R(double x, double y, double width, double height) =>
        new() { X = (float)x, Y = (float)y, Width = (float)width, Height = (float)height };

    public static Rect RectFor(StageArrangement arrangement, StageElementKind kind) => (arrangement, kind) switch
    {
        (StageArrangement.Speaker, StageElementKind.CurrentSlide) => R(0, 72.0 / 1080, 0.55, 0.65),
        (StageArrangement.Speaker, StageElementKind.SpeakerNotes) => R(0.56, 0.09, 0.42, 0.6),
        (StageArrangement.Speaker, StageElementKind.Timer) => R(0, 0.72, 0.5, 0.22),
        (StageArrangement.LyricsOnly, StageElementKind.CurrentSlide) => R(0, 72.0 / 1080, 1, 728.0 / 1080),
        (_, StageElementKind.HeaderBar) => R(0, 0, 1, 72.0 / 1080),
        (_, StageElementKind.CurrentSlide) => R(0, 72.0 / 1080, 1, 728.0 / 1080),
        (_, StageElementKind.NextSlide) => R(0, 800.0 / 1080, 1490.0 / 1920, 220.0 / 1080),
        (_, StageElementKind.SpeakerNotes) => R(1100.0 / 1920, 100.0 / 1080, 790.0 / 1920, 680.0 / 1080),
        (_, StageElementKind.Timer) => R(1490.0 / 1920, 800.0 / 1080, 430.0 / 1920, 220.0 / 1080),
        (_, StageElementKind.Clock) => R(1500.0 / 1920, 0, 400.0 / 1920, 72.0 / 1080),
        (_, StageElementKind.BoothMessage) => R(0, 1020.0 / 1080, 1, 60.0 / 1080),
        _ => R(0, 0, 1, 1),
    };
}
