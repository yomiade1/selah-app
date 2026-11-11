using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Selah.Contracts.V1;
using Selah.UI.Services;

namespace Selah.UI.ViewModels;

/// <summary>What Help ▸ Diagnostics and Help ▸ Report a problem need from core (M1.13).</summary>
public interface IDiagnosticsService
{
    Task<Diagnostics> GetDiagnosticsAsync();

    Task<DiagnosticsReport> RunDiagnosticsAsync(bool quick);

    Task<SupportBundle> CreateSupportBundleAsync();
}

/// <summary>One line in the Diagnostics window: a label and its value.</summary>
public sealed record DiagnosticsRow(string Label, string Value);

/// <summary>
/// Help ▸ Diagnostics (M1.13): system, graphics and screen details, and <b>Run benchmark</b>, which
/// saves a report to attach to a PR or a problem report (ADR-023).
/// </summary>
public sealed partial class DiagnosticsViewModel : ObservableObject
{
    private readonly IDiagnosticsService? service;

    public DiagnosticsViewModel(IDiagnosticsService? service = null)
    {
        this.service = service;
    }

    public System.Collections.ObjectModel.ObservableCollection<DiagnosticsRow> Rows { get; } = [];

    public System.Collections.ObjectModel.ObservableCollection<DiagnosticsRow> Results { get; } = [];

    [ObservableProperty]
    [NotifyCanExecuteChangedFor(nameof(RunBenchmarkCommand))]
    private bool isRunning;

    [ObservableProperty]
    private string? message;

    /// <summary>The saved report, for <b>Show the report</b>.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasReport))]
    private string? reportPath;

    public bool HasReport => ReportPath is not null;

    public bool CanRun => !IsRunning;

    /// <summary>Raised to show a saved file in the file manager.</summary>
    public event EventHandler<string>? ShowFileRequested;

    public async Task LoadAsync()
    {
        if (service is null)
        {
            return;
        }
        try
        {
            Show(await service.GetDiagnosticsAsync());
        }
        catch (CoreException error)
        {
            Message = Localizer.Instance[error.MessageKey];
        }
        catch (Exception error) when (error is TimeoutException or IOException or InvalidOperationException or OperationCanceledException)
        {
            Message = Localizer.Instance["error-not-connected"];
        }
    }

    public void Show(Diagnostics diagnostics)
    {
        var l = Localizer.Instance;
        Rows.Clear();
        Rows.Add(new(l["diagnostics-version"], diagnostics.Version));
        Rows.Add(new(l["diagnostics-system"], $"{diagnostics.Os} {diagnostics.Arch}"));
        Rows.Add(new(l["diagnostics-safe-mode"], l[diagnostics.SafeMode ? "diagnostics-on" : "diagnostics-off"]));
        Rows.Add(new(l["diagnostics-graphics"], diagnostics.EngineConnected ? diagnostics.Renderer : l["status-engine-not-connected"]));
        var resolution = diagnostics.OutputResolution;
        Rows.Add(new(l["diagnostics-screens"], resolution is null
            ? l["status-engine-not-connected"]
            : l.Get("diagnostics-screen-size", ("width", (int)resolution.Width), ("height", (int)resolution.Height))));
    }

    [RelayCommand(CanExecute = nameof(CanRun))]
    private async Task RunBenchmarkAsync()
    {
        if (service is null)
        {
            return;
        }
        IsRunning = true;
        Message = Localizer.Instance["diagnostics-running"];
        try
        {
            var report = await service.RunDiagnosticsAsync(quick: false);
            ShowResults(report);
            Message = Localizer.Instance["diagnostics-saved"];
        }
        catch (CoreException error)
        {
            Message = Localizer.Instance[error.MessageKey];
        }
        catch (Exception error) when (error is TimeoutException or IOException or InvalidOperationException or OperationCanceledException)
        {
            Message = Localizer.Instance["error-not-connected"];
        }
        finally
        {
            IsRunning = false;
        }
    }

    public void ShowResults(DiagnosticsReport report)
    {
        var l = Localizer.Instance;
        Results.Clear();
        Results.Add(new(l["diagnostics-ipc"], l.Get("diagnostics-ipc-result",
            ("ms", report.IpcP99Ms.ToString("0.000", System.Globalization.CultureInfo.CurrentCulture)),
            ("verdict", l[report.IpcPass ? "diagnostics-pass" : "diagnostics-fail"]))));
        Results.Add(new(l["diagnostics-video"], l.Get("diagnostics-video-result",
            ("clip", report.VideoClip),
            ("dropped", (int)report.VideoDropped),
            ("frames", (int)report.VideoFrames),
            ("ms", report.VideoCompositeP99Ms.ToString("0.0", System.Globalization.CultureInfo.CurrentCulture)),
            ("verdict", l[report.VideoPass ? "diagnostics-pass" : "diagnostics-fail"]))));
        Results.Add(new(l["diagnostics-m3"], l.Get("diagnostics-m3-result",
            ("ms", report.M3P99Ms.ToString("0.0", System.Globalization.CultureInfo.CurrentCulture)),
            ("verdict", l[report.M3Pass ? "diagnostics-pass" : "diagnostics-fail"]))));
        ReportPath = report.Path;
    }

    [RelayCommand]
    private void ShowReport()
    {
        if (ReportPath is { } path)
        {
            ShowFileRequested?.Invoke(this, path);
        }
    }
}
