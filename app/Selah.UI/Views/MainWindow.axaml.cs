using Avalonia;
using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Media.Imaging;
using Avalonia.Platform;
using Avalonia.Platform.Storage;
using Avalonia.Threading;
using Avalonia.VisualTree;
using Selah.Client;
using Selah.UI.Services;
using Selah.UI.ViewModels;

namespace Selah.UI.Views;

public sealed partial class MainWindow : Window, IHelpHost
{
    /// <summary>DESIGN-SYSTEM §4.1: below these effective widths the inspector, then the sidebar, hide.</summary>
    public const double InspectorBreakpoint = 1100;
    public const double SidebarBreakpoint = 900;
    public const double DualPreviewBreakpoint = 820;

    private readonly DispatcherTimer previewTimer = new() { Interval = TimeSpan.FromMilliseconds(100) };
    private PreviewRingReader? preview;
    private ulong lastPreviewFrame;
    private WriteableBitmap? previewBitmap;
    private PreviewRingReader? stagePreview;
    private ulong lastStagePreviewFrame;
    private WriteableBitmap? stagePreviewBitmap;
    private MainViewModel? subscribedModel;

    public MainWindow()
    {
        InitializeComponent();
        AddTextSizeShortcuts();
        AddHandler(KeyDownEvent, OnTunnelKeyDown, RoutingStrategies.Tunnel);
        AddHandler(PointerPressedEvent, OnTunnelPointerPressed, RoutingStrategies.Tunnel);
        TextScale.Instance.PropertyChanged += (_, _) => UpdateLayoutForWidth();
        previewTimer.Tick += (_, _) => RefreshPreview();
        Opened += (_, _) => previewTimer.Start();
        Closed += (_, _) =>
        {
            previewTimer.Stop();
            preview?.Dispose();
            stagePreview?.Dispose();
            previewBitmap?.Dispose();
            stagePreviewBitmap?.Dispose();
        };
    }

    private MainViewModel? Model => DataContext as MainViewModel;

    public IHelpSource? HelpSource => Model?.HelpSource;

    public Task OpenHelpForAnchorsAsync(IEnumerable<string> anchors) =>
        Model is null ? Task.CompletedTask : Model.OpenHelpForAnchorsAsync(anchors);

    public Task OpenHelpArticleAsync(string articleId) =>
        Model is null ? Task.CompletedTask : Model.OpenHelpArticleAsync(articleId);

    public Task ToggleHelpPanelAsync() =>
        Model is null ? Task.CompletedTask : Model.HelpPanel.ToggleAsync(Help.CollectVisibleAnchors(this));

    /// <summary>Window width divided by the text scale (DESIGN-SYSTEM §4.1).</summary>
    public double EffectiveWidth => Bounds.Width / TextScale.Instance.Factor;

    protected override void OnSizeChanged(SizeChangedEventArgs e)
    {
        base.OnSizeChanged(e);
        UpdateLayoutForWidth();
    }

    public void UpdateLayoutForWidth()
    {
        var width = EffectiveWidth;
        var showShellInspector = (Model?.IsPreviewMode ?? true) && !(Model?.IsHelpPanelOpen ?? false);
        Inspector.IsVisible = showShellInspector && width >= InspectorBreakpoint;
        Sidebar.IsVisible = width >= SidebarBreakpoint;
        if (width >= DualPreviewBreakpoint)
        {
            PreviewDualGrid.ColumnDefinitions = new ColumnDefinitions("*,*");
            PreviewDualGrid.RowDefinitions = new RowDefinitions("*");
            Grid.SetColumn(StagePreviewCard, 1);
            Grid.SetRow(StagePreviewCard, 0);
        }
        else
        {
            PreviewDualGrid.ColumnDefinitions = new ColumnDefinitions("*");
            PreviewDualGrid.RowDefinitions = new RowDefinitions("*,*");
            Grid.SetColumn(StagePreviewCard, 0);
            Grid.SetRow(StagePreviewCard, 1);
        }
    }

    /// <summary>Ctrl (Cmd on macOS) shortcuts: + − 0 text size, K command palette, S save song, F12 clear everything.</summary>
    private void AddTextSizeShortcuts()
    {
        var command = OperatingSystem.IsMacOS() ? KeyModifiers.Meta : KeyModifiers.Control;
        void Bind(Key key, KeyModifiers modifiers, Action action) =>
            KeyBindings.Add(new KeyBinding { Gesture = new KeyGesture(key, modifiers), Command = new Relay(action) });
        foreach (var key in new[] { Key.OemPlus, Key.Add })
        {
            Bind(key, command, () => Model?.TextSizeUpCommand.Execute(null));
            Bind(key, command | KeyModifiers.Shift, () => Model?.TextSizeUpCommand.Execute(null));
        }
        foreach (var key in new[] { Key.OemMinus, Key.Subtract })
        {
            Bind(key, command, () => Model?.TextSizeDownCommand.Execute(null));
        }
        foreach (var key in new[] { Key.D0, Key.NumPad0 })
        {
            Bind(key, command, () => Model?.TextSizeResetCommand.Execute(null));
        }
        Bind(Key.K, command, () => Model?.OpenCommandPaletteCommand.Execute(null));
        Bind(Key.S, command, () =>
        {
            if (Model?.IsSongEditorMode == true)
            {
                Model.SongEditor.SaveSongCommand.Execute(null);
            }
            else if (Model?.IsSlideEditorMode == true)
            {
                Model.SlideEditor.SavePresentationCommand.Execute(null);
            }
            else if (Model?.IsTemplatesMode == true)
            {
                Model.TemplateEditor.SaveTemplateCommand.Execute(null);
            }
        });
    }

    private void OnTunnelKeyDown(object? sender, KeyEventArgs e)
    {
        // §5.4: Clear everything (F12) bypasses text input / help panel focus at all times.
        if (e.Key == Key.F12)
        {
            Model?.ClearEverythingCommand.Execute(null);
            e.Handled = true;
            return;
        }
        if (e.Key == Key.F1)
        {
            if ((e.KeyModifiers & KeyModifiers.Shift) != 0)
            {
                Model?.ToggleWhatsThisCommand.Execute(null);
                e.Handled = true;
            }
            else if (e.KeyModifiers == KeyModifiers.None)
            {
                _ = ToggleHelpPanelAsync();
                e.Handled = true;
            }
            return;
        }
        if (e.Key == Key.Escape && Model?.IsWhatsThisActive == true)
        {
            Model.DismissWhatsThisCommand.Execute(null);
            e.Handled = true;
        }
    }

    private void OnTunnelPointerPressed(object? sender, PointerPressedEventArgs e)
    {
        if (Model is not { IsWhatsThisActive: true })
        {
            return;
        }
        var target = e.Source as Visual;
        if (IsDescendantOf(target, ClearEverythingButton) || IsDescendantOf(target, WhatsThisControl))
        {
            return;
        }
        e.Handled = true;
        _ = WhatsThisControl.InspectVisualAsync(target);
    }

    private static bool IsDescendantOf(Visual? source, Visual container)
    {
        for (var current = source; current is not null; current = current.GetVisualParent())
        {
            if (ReferenceEquals(current, container))
            {
                return true;
            }
        }
        return false;
    }

    private void OnHelpForScreen(object? sender, RoutedEventArgs e) => _ = ToggleHelpPanelAsync();

    private void OnToggleWhatsThis(object? sender, RoutedEventArgs e) => Model?.ToggleWhatsThisCommand.Execute(null);

    private void OnWhatsThisClosed(object? sender, EventArgs e)
    {
        if (Model is not null)
        {
            Model.IsWhatsThisActive = false;
        }
    }

    private void ScheduleHelpContextRefresh()
    {
        if (Model is not { IsHelpPanelOpen: true } model)
        {
            return;
        }
        Dispatcher.UIThread.Post(() =>
        {
            if (Model is { IsHelpPanelOpen: true })
            {
                _ = model.HelpPanel.RefreshContextAsync(Help.CollectVisibleAnchors(this));
            }
        });
    }

    private void OnOpenSettings(object? sender, RoutedEventArgs e)
    {
        if (Model is null)
        {
            return;
        }
        var window = new SettingsWindow { DataContext = Model.Settings };
        window.Show(this);
    }

    private HelpWindow? helpWindow;

    private void OnOpenHelp(object? sender, RoutedEventArgs e) => ShowHelpWindow(null);

    /// <summary>Opens a requested article inside the docked contextual Help panel (M2.8).</summary>
    public void ShowHelp(string? articleId)
    {
        if (Model is null)
        {
            return;
        }
        if (articleId is not null)
        {
            _ = OpenHelpArticleAsync(articleId);
            return;
        }
        _ = OpenHelpForAnchorsAsync(Help.CollectVisibleAnchors(this));
    }

    /// <summary>Open the standalone help window (one at a time), optionally on an article.</summary>
    public void ShowHelpWindow(string? articleId)
    {
        if (Model is null)
        {
            return;
        }
        if (helpWindow is null)
        {
            helpWindow = new HelpWindow { DataContext = Model.Help };
            helpWindow.Closed += (_, _) => helpWindow = null;
            helpWindow.Show(this);
        }
        else
        {
            helpWindow.Activate();
        }
        if (articleId is not null)
        {
            _ = Model.Help.OpenAsync(articleId);
        }
    }

    protected override void OnDataContextChanged(EventArgs e)
    {
        base.OnDataContextChanged(e);
        if (subscribedModel is not null)
        {
            subscribedModel.PropertyChanged -= OnModelPropertyChanged;
            subscribedModel.Library.PropertyChanged -= OnLibraryPropertyChanged;
            subscribedModel.SongEditor.PropertyChanged -= OnEditorPropertyChanged;
            subscribedModel.SlideEditor.PropertyChanged -= OnSlideEditorPropertyChanged;
            subscribedModel.TemplateEditor.PropertyChanged -= OnTemplateEditorPropertyChanged;
            subscribedModel.ArticleRequested -= OnArticleRequested;
            subscribedModel.OpenUrlRequested -= OnOpenUrlRequested;
            subscribedModel.ShowFileRequested -= OnShowFileRequested;
        }
        subscribedModel = Model;
        if (subscribedModel is { } model)
        {
            model.PropertyChanged += OnModelPropertyChanged;
            model.Library.PropertyChanged += OnLibraryPropertyChanged;
            model.SongEditor.PropertyChanged += OnEditorPropertyChanged;
            model.SlideEditor.PropertyChanged += OnSlideEditorPropertyChanged;
            model.TemplateEditor.PropertyChanged += OnTemplateEditorPropertyChanged;
            model.ArticleRequested += OnArticleRequested;
            model.OpenUrlRequested += OnOpenUrlRequested;
            model.ShowFileRequested += OnShowFileRequested;
        }
        UpdateLayoutForWidth();
    }

    private void OnModelPropertyChanged(object? sender, System.ComponentModel.PropertyChangedEventArgs args)
    {
        if (args.PropertyName is nameof(MainViewModel.ActiveMode)
            or nameof(MainViewModel.IsPreviewMode)
            or nameof(MainViewModel.IsHelpPanelOpen)
            or nameof(MainViewModel.IsSafeMode))
        {
            UpdateLayoutForWidth();
            ScheduleHelpContextRefresh();
        }
    }

    private void OnLibraryPropertyChanged(object? sender, System.ComponentModel.PropertyChangedEventArgs args)
    {
        if (args.PropertyName == nameof(LibraryViewModel.ActiveTab))
        {
            ScheduleHelpContextRefresh();
        }
    }

    private void OnEditorPropertyChanged(object? sender, System.ComponentModel.PropertyChangedEventArgs args)
    {
        if (args.PropertyName is nameof(SongEditorViewModel.CurrentStep) or nameof(SongEditorViewModel.InspectorTab))
        {
            ScheduleHelpContextRefresh();
        }
    }

    private void OnSlideEditorPropertyChanged(object? sender, System.ComponentModel.PropertyChangedEventArgs args)
    {
        if (args.PropertyName is nameof(SlideEditorViewModel.SelectedSlide) or nameof(SlideEditorViewModel.SelectedElement))
        {
            ScheduleHelpContextRefresh();
        }
    }

    private void OnTemplateEditorPropertyChanged(object? sender, System.ComponentModel.PropertyChangedEventArgs args)
    {
        if (args.PropertyName is nameof(TemplateEditorViewModel.SelectedTemplate) or nameof(TemplateEditorViewModel.SelectedRoleKey))
        {
            ScheduleHelpContextRefresh();
        }
    }

    private void OnArticleRequested(object? sender, string id) => ShowHelp(id);

    private void OnOpenUrlRequested(object? sender, string url) => _ = OpenUrlAsync(url);

    private void OnShowFileRequested(object? sender, string path) => _ = ShowFileAsync(path);

    private DiagnosticsWindow? diagnosticsWindow;

    private void OnOpenDiagnostics(object? sender, RoutedEventArgs e)
    {
        if (Model is null)
        {
            return;
        }
        if (diagnosticsWindow is not null)
        {
            diagnosticsWindow.Activate();
            return;
        }
        var diagnostics = Model.CreateDiagnostics();
        diagnostics.ShowFileRequested += (_, path) => _ = ShowFileAsync(path);
        diagnosticsWindow = new DiagnosticsWindow { DataContext = diagnostics };
        diagnosticsWindow.Closed += (_, _) => diagnosticsWindow = null;
        diagnosticsWindow.Show(this);
        _ = diagnostics.LoadAsync();
    }

    /// <summary>Opens the problem-report form in the browser. Only https links from core.</summary>
    private async Task OpenUrlAsync(string url)
    {
        if (Uri.TryCreate(url, UriKind.Absolute, out var uri) && uri.Scheme == Uri.UriSchemeHttps)
        {
            await Launcher.LaunchUriAsync(uri);
        }
    }

    /// <summary>Shows a saved file's folder in Explorer or the Finder.</summary>
    private async Task ShowFileAsync(string path)
    {
        if (Path.GetDirectoryName(path) is { } folder && Directory.Exists(folder))
        {
            await Launcher.LaunchDirectoryInfoAsync(new DirectoryInfo(folder));
        }
    }

    private bool closeConfirmed;

    protected override void OnClosing(WindowClosingEventArgs e)
    {
        if (RequestClose(e.IsProgrammatic, e.CloseReason == WindowCloseReason.OSShutdown))
        {
            e.Cancel = true;
        }
        base.OnClosing(e);
    }

    /// <summary>
    /// SAFE-06: when the person closes Selah while something is on the screens, ask first
    /// ("Something is on the screens." with <b>Keep showing</b> and <b>Clear and quit</b>). Returns
    /// true when the close must wait for their answer. Selah's own closes and the computer shutting
    /// down aren't held.
    /// </summary>
    public bool RequestClose(bool programmatic, bool osShutdown)
    {
        if (closeConfirmed || programmatic || osShutdown || Model is not { IsLive: true } model)
        {
            return false;
        }
        _ = AskThenCloseAsync(model);
        return true;
    }

    private async Task AskThenCloseAsync(MainViewModel model)
    {
        if (await model.ConfirmIfLiveAsync(LiveGuardKind.Quit))
        {
            closeConfirmed = true;
            Close();
        }
    }

    private void OnSafeModeLearnWhy(object? sender, RoutedEventArgs e) =>
        Model?.OpenArticleCommand.Execute("troubleshooting.selah-started-in-safe-mode");

    /// <summary>Show the newest frames from the engine's program and staged preview rings (ADR-004, M3.6).</summary>
    private void RefreshPreview()
    {
        var ns = Model?.IpcNamespace ?? IpcEndpoint.ForCurrentUser(IpcService.Engine).Namespace;
        RefreshRing(
            Path.Combine(RuntimeDirectory.Path, $"{ns}-preview-0.ring"),
            ref preview,
            ref lastPreviewFrame,
            ref previewBitmap,
            PreviewImage);
        if (RefreshRing(
                Path.Combine(RuntimeDirectory.Path, $"{ns}-stage-0.ring"),
                ref stagePreview,
                ref lastStagePreviewFrame,
                ref stagePreviewBitmap,
                StagePreviewImage)
            && Model is { HasStagedPreview: false })
        {
            Model.HasStagedPreview = true;
        }
    }

    private static bool RefreshRing(
        string path,
        ref PreviewRingReader? reader,
        ref ulong lastFrame,
        ref WriteableBitmap? bitmap,
        Image target)
    {
        try
        {
            if (reader is null)
            {
                if (!File.Exists(path))
                {
                    return false;
                }
                reader = PreviewRingReader.Open(path);
            }
            if (reader.Published == lastFrame || reader.Latest() is not { } frame)
            {
                return false;
            }
            lastFrame = frame.Frame;
            if (bitmap is null || bitmap.PixelSize.Width != frame.Width || bitmap.PixelSize.Height != frame.Height)
            {
                bitmap?.Dispose();
                bitmap = new WriteableBitmap(new PixelSize(frame.Width, frame.Height), new Vector(96, 96), PixelFormat.Bgra8888, AlphaFormat.Premul);
            }
            using (var buffer = bitmap.Lock())
            {
                for (var row = 0; row < frame.Height; row++)
                {
                    System.Runtime.InteropServices.Marshal.Copy(frame.Pixels, row * frame.Stride, buffer.Address + (row * buffer.RowBytes), frame.Width * 4);
                }
            }
            target.Source = null;
            target.Source = bitmap;
            return frame.Frame > 0;
        }
        catch (Exception error) when (error is IOException or InvalidDataException or UnauthorizedAccessException)
        {
            // The engine restarted and the ring is being recreated; try again on the next tick.
            reader?.Dispose();
            reader = null;
            return false;
        }
    }

    private sealed class Relay(Action action) : System.Windows.Input.ICommand
    {
        public event EventHandler? CanExecuteChanged
        {
            add { }
            remove { }
        }

        public bool CanExecute(object? parameter) => true;

        public void Execute(object? parameter) => action();
    }
}
