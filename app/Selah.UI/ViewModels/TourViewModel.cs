using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Selah.UI.Services;

namespace Selah.UI.ViewModels;

public sealed record TourStep(string Anchor, string TextKey)
{
    public string Text => Localizer.Instance[TextKey];
}

/// <summary>A guided tour from <c>content/help/tours/*.yaml</c> (KNOWLEDGE-BASE §6.5).</summary>
public sealed record TourDefinition(string Id, string TitleKey, string Mode, bool RequiresPracticeMode, IReadOnlyList<TourStep> Steps)
{
    public string Title => Localizer.Instance[TitleKey];

    /// <summary>Reads the small, fixed YAML shape tours use. Anything else is ignored.</summary>
    public static TourDefinition? Parse(string text)
    {
        string id = string.Empty, titleKey = string.Empty, mode = "producer";
        var practice = false;
        var steps = new List<TourStep>();
        string? anchor = null;
        foreach (var raw in text.Split('\n'))
        {
            var line = raw.TrimEnd('\r');
            var trimmed = line.Trim();
            if (trimmed.Length == 0 || trimmed.StartsWith('#'))
            {
                continue;
            }
            var stepStart = trimmed.StartsWith("- ", StringComparison.Ordinal);
            var body = stepStart ? trimmed[2..].Trim() : trimmed;
            var colon = body.IndexOf(':', StringComparison.Ordinal);
            if (colon <= 0)
            {
                continue;
            }
            var key = body[..colon].Trim();
            var value = body[(colon + 1)..].Trim().Trim('"');
            switch (key)
            {
                case "id" when !line.StartsWith(' '):
                    id = value;
                    break;
                case "title_key":
                    titleKey = value;
                    break;
                case "mode":
                    mode = value;
                    break;
                case "requires_practice_mode":
                    practice = value == "true";
                    break;
                case "anchor":
                    anchor = value;
                    break;
                case "text_key" when anchor is not null:
                    steps.Add(new TourStep(anchor, value));
                    anchor = null;
                    break;
            }
        }
        return id.Length == 0 || steps.Count == 0 ? null : new TourDefinition(id, titleKey, mode, practice, steps);
    }

    public static IReadOnlyList<TourDefinition> LoadAll(string? folder = null)
    {
        folder ??= Path.Combine(AppContext.BaseDirectory, "tours");
        if (!Directory.Exists(folder))
        {
            return [];
        }
        return Directory.GetFiles(folder, "*.yaml")
            .Order(StringComparer.Ordinal)
            .Select(path =>
            {
                try
                {
                    return Parse(File.ReadAllText(path));
                }
                catch (IOException)
                {
                    return null;
                }
            })
            .OfType<TourDefinition>()
            .ToList();
    }
}

/// <summary>
/// Runs a tour one coach mark at a time (M4.7). Tours never start while the screens show
/// something, unless practice mode is on (KNOWLEDGE-BASE §5.4, M4-A9).
/// </summary>
public sealed partial class TourViewModel : ObservableObject
{
    private readonly Func<bool> screensLive;
    private readonly Func<bool> practising;
    private readonly Func<bool, Task> setPractice;
    private readonly Action<string> openMode;
    private bool startedPractice;

    public TourViewModel(Func<bool> screensLive, Func<bool> practising, Func<bool, Task> setPractice, Action<string> openMode, IReadOnlyList<TourDefinition>? tours = null)
    {
        this.screensLive = screensLive;
        this.practising = practising;
        this.setPractice = setPractice;
        this.openMode = openMode;
        foreach (var tour in tours ?? TourDefinition.LoadAll())
        {
            Tours.Add(tour);
        }
    }

    public ObservableCollection<TourDefinition> Tours { get; } = [];

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsOpen))]
    [NotifyPropertyChangedFor(nameof(IsVisible))]
    private TourDefinition? active;

    /// <summary>The tour picker (Help ▸ Start tour) is showing.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsVisible))]
    private bool isPicking;

    public bool IsVisible => IsOpen || IsPicking;

    /// <summary>Raised with each step's anchor so the shell can bring its pane forward.</summary>
    public event EventHandler<string>? AnchorShown;

    partial void OnStepIndexChanged(int value)
    {
        if (Step is { } step)
        {
            AnchorShown?.Invoke(this, step.Anchor);
        }
    }

    partial void OnActiveChanged(TourDefinition? value)
    {
        if (Step is { } step)
        {
            AnchorShown?.Invoke(this, step.Anchor);
        }
    }

    public string NextText => Localizer.Instance[IsLastStep ? "tour-done" : "tour-next"];

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(Step))]
    [NotifyPropertyChangedFor(nameof(StepText))]
    [NotifyPropertyChangedFor(nameof(StepCounter))]
    [NotifyPropertyChangedFor(nameof(IsLastStep))]
    [NotifyPropertyChangedFor(nameof(NextText))]
    [NotifyPropertyChangedFor(nameof(CanGoBack))]
    [NotifyPropertyChangedFor(nameof(ActiveAnchor))]
    private int stepIndex;

    /// <summary>Why the last tour didn't start, if it didn't.</summary>
    [ObservableProperty]
    private string? refusal;

    public bool IsOpen => Active is not null;

    public TourStep? Step => Active is { } tour && StepIndex < tour.Steps.Count ? tour.Steps[StepIndex] : null;

    public string StepText => Step?.Text ?? string.Empty;

    public string ActiveAnchor => Step?.Anchor ?? string.Empty;

    public string StepCounter => Active is null ? string.Empty
        : Localizer.Instance.Get("tour-step", ("step", (StepIndex + 1).ToString(System.Globalization.CultureInfo.CurrentCulture)), ("count", Active.Steps.Count.ToString(System.Globalization.CultureInfo.CurrentCulture)));

    public bool IsLastStep => Active is { } tour && StepIndex >= tour.Steps.Count - 1;

    public bool CanGoBack => StepIndex > 0;

    /// <summary>Start a tour. Returns false (and says why) while the screens are live.</summary>
    public async Task<bool> StartAsync(TourDefinition tour)
    {
        Refusal = null;
        if (screensLive() && !practising())
        {
            Refusal = Localizer.Instance["tour-refused-live"];
            return false;
        }
        startedPractice = false;
        if (tour.RequiresPracticeMode && !practising())
        {
            await setPractice(true);
            startedPractice = true;
        }
        openMode(tour.Mode);
        IsPicking = false;
        StepIndex = 0;
        Active = tour;
        OnPropertyChanged(nameof(StepIndex));
        OnPropertyChanged(nameof(NextText));
        return true;
    }

    [RelayCommand]
    private void ShowPicker()
    {
        Refusal = null;
        IsPicking = true;
    }

    [RelayCommand]
    private Task Start(TourDefinition tour) => StartAsync(tour);

    [RelayCommand]
    private async Task Next()
    {
        if (IsLastStep)
        {
            await StopAsync();
            return;
        }
        StepIndex++;
    }

    [RelayCommand]
    private void Back()
    {
        if (StepIndex > 0)
        {
            StepIndex--;
        }
    }

    [RelayCommand]
    private Task Skip() => StopAsync();

    private async Task StopAsync()
    {
        IsPicking = false;
        Active = null;
        StepIndex = 0;
        if (startedPractice)
        {
            startedPractice = false;
            await setPractice(false);
        }
    }
}
