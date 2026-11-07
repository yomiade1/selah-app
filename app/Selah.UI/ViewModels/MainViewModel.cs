using Avalonia.Threading;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Selah.UI.Services;

namespace Selah.UI.ViewModels;

/// <summary>Why the "Something is on the screens" question is showing (SAFE-06).</summary>
public enum LiveGuardKind
{
    None,
    Quit,
    Update,
}

/// <summary>The main window: connection status, the test-pattern command and the panes (M1.8).</summary>
public sealed partial class MainViewModel : ObservableObject
{
    private readonly CoreConnection? connection;
    private readonly IDiagnosticsService? diagnostics;

    public MainViewModel(CoreConnection? connection, IDiagnosticsService? diagnostics = null)
    {
        this.connection = connection;
        this.diagnostics = diagnostics ?? connection;
        Settings = new SettingsViewModel(
            connection is null ? null : connection.SetAppearanceAsync,
            new UpdatesViewModel(connection, () => ConfirmIfLiveAsync(LiveGuardKind.Update)));
        Help = new HelpViewModel(connection);
        Localizer.Instance.PropertyChanged += (_, _) =>
        {
            OnPropertyChanged(nameof(StatusText));
            OnPropertyChanged(nameof(LiveGuardQuestion));
            OnPropertyChanged(nameof(LiveGuardConfirmText));
        };
        if (connection is not null)
        {
            connection.Changed += (_, snapshot) => Dispatcher.UIThread.Post(() => Apply(snapshot));
            connection.SettingsReceived += (_, settings) => Dispatcher.UIThread.Post(() =>
            {
                Settings.ApplyFromCore(settings.Appearance ?? new());
                Settings.Updates.ApplyFromCore(settings);
            });
            Apply(connection.Snapshot);
        }
    }

    public SettingsViewModel Settings { get; }

    public HelpViewModel Help { get; }

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(StatusText))]
    [NotifyCanExecuteChangedFor(nameof(ShowTestPatternCommand))]
    private bool isConnected;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(StatusText))]
    private bool isEngineConnected;

    [ObservableProperty]
    private bool isSafeMode;

    [ObservableProperty]
    private bool isLive;

    [ObservableProperty]
    private bool isSettingsOpen;

    /// <summary>The question shown under the toolbar while something is on the screens (SAFE-06).</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsLiveGuardOpen))]
    [NotifyPropertyChangedFor(nameof(LiveGuardQuestion))]
    [NotifyPropertyChangedFor(nameof(LiveGuardConfirmText))]
    private LiveGuardKind liveGuard;

    private TaskCompletionSource<bool>? liveGuardDecision;

    public bool IsLiveGuardOpen => LiveGuard != LiveGuardKind.None;

    public string LiveGuardQuestion => Localizer.Instance[LiveGuard == LiveGuardKind.Update ? "live-guard-update" : "live-guard-quit"];

    public string LiveGuardConfirmText => Localizer.Instance[LiveGuard == LiveGuardKind.Update ? "live-guard-clear-and-update" : "live-guard-clear-and-quit"];

    /// <summary>
    /// SAFE-06: if something is on the screens, ask first. Returns true straight away when nothing
    /// is live; otherwise true once the person chose to clear the screens (and they were cleared).
    /// </summary>
    public Task<bool> ConfirmIfLiveAsync(LiveGuardKind kind)
    {
        if (!IsLive)
        {
            return Task.FromResult(true);
        }
        liveGuardDecision?.TrySetResult(false);
        liveGuardDecision = new TaskCompletionSource<bool>();
        LiveGuard = kind;
        return liveGuardDecision.Task;
    }

    /// <summary><b>Clear and quit</b> / <b>Clear and update</b>: clear over the emergency lane, then go on.</summary>
    [RelayCommand]
    private async Task ConfirmLiveGuardAsync()
    {
        var decision = liveGuardDecision;
        liveGuardDecision = null;
        LiveGuard = LiveGuardKind.None;
        var cleared = connection is null || await connection.ClearEverythingAsync();
        if (!cleared)
        {
            LastError = Localizer.Instance["error-not-connected"];
        }
        decision?.TrySetResult(cleared);
    }

    /// <summary><b>Keep showing</b>: nothing changes.</summary>
    [RelayCommand]
    private void CancelLiveGuard()
    {
        var decision = liveGuardDecision;
        liveGuardDecision = null;
        LiveGuard = LiveGuardKind.None;
        decision?.TrySetResult(false);
    }

    /// <summary>Set when the last command failed, for the status line.</summary>
    [ObservableProperty]
    private string? lastError;

    public string StatusText => Localizer.Instance[StatusKey];

    public string StatusKey => (IsConnected, IsEngineConnected) switch
    {
        (false, _) => "status-connecting",
        (true, false) => "status-engine-not-connected",
        _ => "status-connected",
    };

    public void Apply(ConnectionSnapshot snapshot)
    {
        IsConnected = snapshot.Connected;
        IsEngineConnected = snapshot.EngineConnected;
        IsSafeMode = snapshot.SafeMode;
        IsLive = snapshot.Live;
        OnPropertyChanged(nameof(StatusKey));
    }

    [RelayCommand(CanExecute = nameof(IsConnected))]
    private async Task ShowTestPatternAsync()
    {
        if (connection is null)
        {
            return;
        }
        try
        {
            await connection.ShowTestPatternAsync();
            LastError = null;
        }
        catch (Exception error) when (error is TimeoutException or IOException or InvalidOperationException)
        {
            LastError = Localizer.Instance["error-not-connected"];
        }
    }

    /// <summary>Always available, even when core isn't connected (SAFE-03).</summary>
    [RelayCommand]
    private async Task ClearEverythingAsync()
    {
        if (connection is not null && !await connection.ClearEverythingAsync())
        {
            LastError = Localizer.Instance["error-not-connected"];
        }
    }

    /// <summary>The Report a problem bar under the toolbar (M1.13).</summary>
    [ObservableProperty]
    private bool isReportOpen;

    [ObservableProperty]
    private string? reportMessage;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasReportFile))]
    private string? reportPath;

    private string? reportUrl;

    public bool HasReportFile => ReportPath is not null;

    /// <summary>Raised to open a web page (the problem-report form) in the browser.</summary>
    public event EventHandler<string>? OpenUrlRequested;

    /// <summary>Raised to show a saved file in the file manager.</summary>
    public event EventHandler<string>? ShowFileRequested;

    /// <summary>
    /// Help ▸ Report a problem (SEC-06): core saves a redacted support bundle, then the form opens
    /// with the version filled in. The person attaches the file themselves; nothing is sent.
    /// </summary>
    [RelayCommand]
    private async Task ReportProblemAsync()
    {
        IsReportOpen = true;
        ReportPath = null;
        ReportMessage = Localizer.Instance["report-saving"];
        if (diagnostics is null)
        {
            ReportMessage = Localizer.Instance["error-not-connected"];
            return;
        }
        try
        {
            var bundle = await diagnostics.CreateSupportBundleAsync();
            ReportPath = bundle.Path;
            reportUrl = bundle.ReportUrl;
            ReportMessage = Localizer.Instance["report-saved"];
            OpenUrlRequested?.Invoke(this, bundle.ReportUrl);
        }
        catch (CoreException error)
        {
            ReportMessage = Localizer.Instance[error.MessageKey];
        }
        catch (Exception error) when (error is TimeoutException or IOException or InvalidOperationException or OperationCanceledException)
        {
            ReportMessage = Localizer.Instance["error-not-connected"];
        }
    }

    [RelayCommand]
    private void ShowReportFile()
    {
        if (ReportPath is { } path)
        {
            ShowFileRequested?.Invoke(this, path);
        }
    }

    [RelayCommand]
    private void OpenReportForm()
    {
        if (reportUrl is { } url)
        {
            OpenUrlRequested?.Invoke(this, url);
        }
    }

    [RelayCommand]
    private void CloseReport() => IsReportOpen = false;

    /// <summary>A view model for a new Help ▸ Diagnostics window.</summary>
    public DiagnosticsViewModel CreateDiagnostics() => new(diagnostics);

    /// <summary><b>Restart normally</b> on the safe-mode banner (SAFE-07).</summary>
    [RelayCommand]
    private async Task RestartNormallyAsync()
    {
        if (connection is null)
        {
            return;
        }
        try
        {
            await connection.RestartNormallyAsync();
        }
        catch (Exception error) when (error is TimeoutException or IOException or InvalidOperationException or OperationCanceledException)
        {
            LastError = Localizer.Instance["error-not-connected"];
        }
    }

    /// <summary>Raised when something asks for a help article (the article view opens it).</summary>
    public event EventHandler<string>? ArticleRequested;

    [RelayCommand]
    private void OpenArticle(string id) => ArticleRequested?.Invoke(this, id);

    [RelayCommand]
    private void OpenSettings() => IsSettingsOpen = true;

    [RelayCommand]
    private void CloseSettings() => IsSettingsOpen = false;

    [RelayCommand]
    private void TextSizeUp() => Settings.StepUp();

    [RelayCommand]
    private void TextSizeDown() => Settings.StepDown();

    [RelayCommand]
    private void TextSizeReset() => Settings.Reset();
}
