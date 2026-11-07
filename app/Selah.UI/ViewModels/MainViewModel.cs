using Avalonia.Threading;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Selah.UI.Services;

namespace Selah.UI.ViewModels;

/// <summary>The main window: connection status, the test-pattern command and the panes (M1.8).</summary>
public sealed partial class MainViewModel : ObservableObject
{
    private readonly CoreConnection? connection;

    public MainViewModel(CoreConnection? connection)
    {
        this.connection = connection;
        Settings = new SettingsViewModel(connection is null ? null : connection.SetAppearanceAsync);
        Localizer.Instance.PropertyChanged += (_, _) => OnPropertyChanged(nameof(StatusText));
        if (connection is not null)
        {
            connection.Changed += (_, snapshot) => Dispatcher.UIThread.Post(() => Apply(snapshot));
            connection.SettingsReceived += (_, settings) =>
                Dispatcher.UIThread.Post(() => Settings.ApplyFromCore(settings.Appearance ?? new()));
            Apply(connection.Snapshot);
        }
    }

    public SettingsViewModel Settings { get; }

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
