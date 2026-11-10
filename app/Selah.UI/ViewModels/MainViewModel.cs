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

/// <summary>The main window: connection status, the test-pattern command and the panes (M1.8, M2.7).</summary>
public sealed partial class MainViewModel : ObservableObject
{
    private readonly CoreConnection? connection;
    private readonly IDiagnosticsService? diagnostics;
    private bool wasConnected;
    private bool libraryLoadedOnce;

    public MainViewModel(
        CoreConnection? connection,
        IDiagnosticsService? diagnostics = null,
        ILibraryService? library = null,
        IHelpSource? helpSource = null)
    {
        this.connection = connection;
        this.diagnostics = diagnostics ?? connection;
        var resolvedLibrary = library ?? connection;
        var resolvedHelp = helpSource ?? connection;
        Settings = new SettingsViewModel(
            connection is null ? null : connection.SetAppearanceAsync,
            new UpdatesViewModel(connection, () => ConfirmIfLiveAsync(LiveGuardKind.Update)));
        HelpSource = resolvedHelp;
        Help = new HelpViewModel(resolvedHelp);
        HelpPanel = new HelpPanelViewModel(resolvedHelp);
        HelpPanel.PropertyChanged += (_, args) =>
        {
            if (args.PropertyName == nameof(HelpPanelViewModel.IsOpen))
            {
                OnPropertyChanged(nameof(IsHelpPanelOpen));
            }
        };
        Library = new LibraryViewModel(resolvedLibrary);
        SongEditor = new SongEditorViewModel(resolvedLibrary);
        SlideEditor = new SlideEditorViewModel(resolvedLibrary);
        TemplateEditor = new TemplateEditorViewModel(resolvedLibrary);
        CommandPalette = new CommandPaletteViewModel(resolvedLibrary, resolvedHelp);

        Library.EditSongRequested += (_, song) => _ = EditSongInEditorAsync(song);
        Library.SceneStaged += (_, _) => HasStagedPreview = true;
        SongEditor.SongSaved += (_, song) => _ = OnSongSavedAsync(song);
        SongEditor.SceneStaged += (_, _) => HasStagedPreview = true;
        SlideEditor.SceneStaged += (_, _) => HasStagedPreview = true;
        TemplateEditor.TemplatesChanged += (_, _) => _ = RefreshTemplatesEverywhereAsync();

        RegisterPaletteCommands();
        CommandPalette.SongChosen += (_, song) => _ = EditSongInEditorAsync(song);
        CommandPalette.ScriptureChosen += (_, verse) =>
        {
            ActiveMode = WorkspaceMode.Library;
            _ = Library.OpenVerseHitAsync(verse);
        };
        CommandPalette.MediaChosen += (_, media) =>
        {
            ActiveMode = WorkspaceMode.Library;
            Library.ActiveTab = LibraryTab.Media;
            _ = Library.SearchMediaAsync();
        };
        CommandPalette.HelpChosen += (_, article) => ArticleRequested?.Invoke(this, article.Id);

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

    public IHelpSource? HelpSource { get; }

    public HelpViewModel Help { get; }

    public HelpPanelViewModel HelpPanel { get; }

    public LibraryViewModel Library { get; }

    public SongEditorViewModel SongEditor { get; }

    public SlideEditorViewModel SlideEditor { get; }

    public TemplateEditorViewModel TemplateEditor { get; }

    public CommandPaletteViewModel CommandPalette { get; }

    [ObservableProperty]
    private bool isWhatsThisActive;

    [ObservableProperty]
    private string? lastErrorFixArticleId;

    public bool IsHelpPanelOpen => HelpPanel.IsOpen;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsPreviewMode))]
    [NotifyPropertyChangedFor(nameof(IsLibraryMode))]
    [NotifyPropertyChangedFor(nameof(IsSongEditorMode))]
    [NotifyPropertyChangedFor(nameof(IsSlideEditorMode))]
    [NotifyPropertyChangedFor(nameof(IsTemplatesMode))]
    private WorkspaceMode activeMode = WorkspaceMode.Preview;

    public bool IsPreviewMode => ActiveMode == WorkspaceMode.Preview;

    public bool IsLibraryMode => ActiveMode == WorkspaceMode.Library;

    public bool IsSongEditorMode => ActiveMode == WorkspaceMode.SongEditor;

    public bool IsSlideEditorMode => ActiveMode == WorkspaceMode.SlideEditor;

    public bool IsTemplatesMode => ActiveMode == WorkspaceMode.Templates;

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
    private bool hasStagedPreview;

    [ObservableProperty]
    private bool hasPreviewFlashWarning;

    [ObservableProperty]
    private bool isSettingsOpen;

    internal string? IpcNamespace => connection?.Namespace;

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

    partial void OnLastErrorChanged(string? value)
    {
        if (string.IsNullOrWhiteSpace(value))
        {
            LastErrorFixArticleId = null;
        }
    }

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
        HasStagedPreview = snapshot.PreviewSceneRevision > 0;
        HasPreviewFlashWarning = snapshot.FlashWarning;
        OnPropertyChanged(nameof(StatusKey));
        if (snapshot.Connected && !wasConnected)
        {
            wasConnected = true;
            _ = Library.LoadAsync();
            _ = SongEditor.LoadTemplatesAsync();
            libraryLoadedOnce = true;
        }
        else if (!snapshot.Connected)
        {
            wasConnected = false;
        }
    }

    partial void OnActiveModeChanged(WorkspaceMode value)
    {
        if (value == WorkspaceMode.Library && !libraryLoadedOnce)
        {
            libraryLoadedOnce = true;
            _ = Library.LoadAsync();
        }
        else if (value == WorkspaceMode.SongEditor)
        {
            _ = SongEditor.LoadTemplatesAsync();
        }
        else if (value == WorkspaceMode.SlideEditor)
        {
            _ = SlideEditor.LoadAsync();
        }
        else if (value == WorkspaceMode.Templates)
        {
            _ = TemplateEditor.LoadAsync();
        }
    }

    [RelayCommand]
    public void SelectMode(string mode)
    {
        ActiveMode = mode switch
        {
            "Library" => WorkspaceMode.Library,
            "SongEditor" => WorkspaceMode.SongEditor,
            "SlideEditor" => WorkspaceMode.SlideEditor,
            "Templates" => WorkspaceMode.Templates,
            _ => WorkspaceMode.Preview,
        };
    }

    private async Task RefreshTemplatesEverywhereAsync()
    {
        await Task.WhenAll(
            Library.LoadTemplatesAsync(),
            SongEditor.LoadTemplatesAsync(),
            SlideEditor.LoadAsync());
        if (Library.SelectedSong is not null)
        {
            _ = Library.SearchSongsAsync();
        }
        _ = SongEditor.RefreshSlidesAsync();
    }

    [RelayCommand]
    public void OpenLibraryTab(string tabName)
    {
        ActiveMode = WorkspaceMode.Library;
        Library.SelectTab(tabName);
    }

    [RelayCommand]
    public void OpenNewSong()
    {
        SongEditor.StartNewSong(startOnPaste: true);
        ActiveMode = WorkspaceMode.SongEditor;
    }

    [RelayCommand]
    public void OpenCommandPalette() => CommandPalette.Open();

    private async Task EditSongInEditorAsync(Selah.Contracts.V1.Song? song)
    {
        if (song is null)
        {
            SongEditor.StartNewSong(startOnPaste: true);
        }
        else
        {
            await SongEditor.LoadSongAsync(song);
        }
        ActiveMode = WorkspaceMode.SongEditor;
    }

    private async Task OnSongSavedAsync(Selah.Contracts.V1.Song song)
    {
        await Library.LoadAsync();
        Library.SelectedSong = Library.Songs.FirstOrDefault(s => LibraryIds.Same(s.Id, song.Id)) ?? Library.SelectedSong;
    }

    private void RegisterPaletteCommands()
    {
        CommandPalette.RegisterCommand("mode-preview", "preview-on-screen", () => SelectMode("Preview"));
        CommandPalette.RegisterCommand("mode-library", "library-title", () => SelectMode("Library"));
        CommandPalette.RegisterCommand("library-tab-songs", "library-title", () => OpenLibraryTab("Songs"));
        CommandPalette.RegisterCommand("library-tab-media", "library-title", () => OpenLibraryTab("Media"));
        CommandPalette.RegisterCommand("library-tab-bibles", "bible-translation-label", () => OpenLibraryTab("Bibles"));
        CommandPalette.RegisterCommand("mode-song-editor", "editor-title", () => SelectMode("SongEditor"));
        CommandPalette.RegisterCommand("mode-slide-editor", "slides-title", () => SelectMode("SlideEditor"));
        CommandPalette.RegisterCommand("mode-templates", "templates-title", () => SelectMode("Templates"));
        CommandPalette.RegisterCommand("library-new-song", "editor-paste-heading", OpenNewSong);
        CommandPalette.RegisterCommand("command-show-test-pattern", "command-show-test-pattern-hint", () => ShowTestPatternCommand.Execute(null));
        CommandPalette.RegisterCommand("command-clear-everything", "command-clear-everything-hint", () => ClearEverythingCommand.Execute(null));
        CommandPalette.RegisterCommand("library-undo", "library-title", () =>
        {
            if (ActiveMode == WorkspaceMode.SongEditor)
            {
                SongEditor.UndoCommand.Execute(null);
            }
            else if (ActiveMode == WorkspaceMode.SlideEditor)
            {
                SlideEditor.UndoCommand.Execute(null);
            }
            else if (ActiveMode == WorkspaceMode.Templates)
            {
                TemplateEditor.UndoCommand.Execute(null);
            }
            else
            {
                Library.UndoCommand.Execute(null);
            }
        });
        CommandPalette.RegisterCommand("library-redo", "library-title", () =>
        {
            if (ActiveMode == WorkspaceMode.SongEditor)
            {
                SongEditor.RedoCommand.Execute(null);
            }
            else if (ActiveMode == WorkspaceMode.SlideEditor)
            {
                SlideEditor.RedoCommand.Execute(null);
            }
            else if (ActiveMode == WorkspaceMode.Templates)
            {
                TemplateEditor.RedoCommand.Execute(null);
            }
            else
            {
                Library.RedoCommand.Execute(null);
            }
        });
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
        catch (CoreException error)
        {
            LastError = Localizer.Instance[error.MessageKey];
            LastErrorFixArticleId = string.IsNullOrWhiteSpace(error.FixArticleId) ? null : error.FixArticleId;
        }
        catch (Exception error) when (error is TimeoutException or IOException or InvalidOperationException)
        {
            LastError = Localizer.Instance["error-not-connected"];
            LastErrorFixArticleId = null;
        }
    }

    public event EventHandler? ClearEverythingTriggered;

    /// <summary>Always available, even when core isn't connected (SAFE-03).</summary>
    [RelayCommand]
    private async Task ClearEverythingAsync()
    {
        ClearEverythingTriggered?.Invoke(this, EventArgs.Empty);
        if (connection is not null && !await connection.ClearEverythingAsync())
        {
            LastError = Localizer.Instance["error-not-connected"];
            LastErrorFixArticleId = null;
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

    /// <summary>Raised when something asks for a help article (the help panel opens it).</summary>
    public event EventHandler<string>? ArticleRequested;

    [RelayCommand]
    private void OpenArticle(string id) => ArticleRequested?.Invoke(this, id);

    public Task OpenHelpForAnchorsAsync(IEnumerable<string> anchors) => HelpPanel.OpenForAnchorsAsync(anchors);

    public Task OpenHelpArticleAsync(string articleId) => HelpPanel.OpenArticleAsync(articleId);

    [RelayCommand]
    private void ToggleWhatsThis() => IsWhatsThisActive = !IsWhatsThisActive;

    [RelayCommand]
    private void DismissWhatsThis() => IsWhatsThisActive = false;

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
