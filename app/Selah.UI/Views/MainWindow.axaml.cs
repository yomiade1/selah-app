using Avalonia;
using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Media.Imaging;
using Avalonia.Platform;
using Avalonia.Threading;
using Selah.Client;
using Selah.UI.Services;
using Selah.UI.ViewModels;

namespace Selah.UI.Views;

public sealed partial class MainWindow : Window
{
    /// <summary>DESIGN-SYSTEM §4.1: below these effective widths the inspector, then the sidebar, hide.</summary>
    public const double InspectorBreakpoint = 1100;
    public const double SidebarBreakpoint = 900;

    private readonly DispatcherTimer previewTimer = new() { Interval = TimeSpan.FromMilliseconds(100) };
    private PreviewRingReader? preview;
    private ulong lastPreviewFrame;
    private WriteableBitmap? previewBitmap;

    public MainWindow()
    {
        InitializeComponent();
        AddTextSizeShortcuts();
        TextScale.Instance.PropertyChanged += (_, _) => UpdateLayoutForWidth();
        previewTimer.Tick += (_, _) => RefreshPreview();
        Opened += (_, _) => previewTimer.Start();
        Closed += (_, _) =>
        {
            previewTimer.Stop();
            preview?.Dispose();
        };
    }

    private MainViewModel? Model => DataContext as MainViewModel;

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
        Inspector.IsVisible = width >= InspectorBreakpoint;
        Sidebar.IsVisible = width >= SidebarBreakpoint;
    }

    /// <summary>Ctrl (Cmd on macOS) with + − 0 changes the text size; F12 clears everything.</summary>
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
        Bind(Key.F12, KeyModifiers.None, () => Model?.ClearEverythingCommand.Execute(null));
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

    private void OnOpenHelp(object? sender, RoutedEventArgs e) => ShowHelp(null);

    /// <summary>Open the help window (one at a time), optionally on an article.</summary>
    public void ShowHelp(string? articleId)
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
        if (Model is { } model)
        {
            model.ArticleRequested += (_, id) => ShowHelp(id);
        }
    }

    private void OnSafeModeLearnWhy(object? sender, RoutedEventArgs e) =>
        Model?.OpenArticleCommand.Execute("troubleshooting.selah-started-in-safe-mode");

    /// <summary>Show the newest frame from the engine's preview ring (ADR-004), about 10 times a second.</summary>
    private void RefreshPreview()
    {
        try
        {
            if (preview is null)
            {
                var ns = IpcEndpoint.ForCurrentUser(IpcService.Engine).Namespace;
                var path = Path.Combine(RuntimeDirectory.Path, $"{ns}-preview-0.ring");
                if (!File.Exists(path))
                {
                    return;
                }
                preview = PreviewRingReader.Open(path);
            }
            if (preview.Published == lastPreviewFrame || preview.Latest() is not { } frame)
            {
                return;
            }
            lastPreviewFrame = frame.Frame;
            if (previewBitmap is null || previewBitmap.PixelSize.Width != frame.Width || previewBitmap.PixelSize.Height != frame.Height)
            {
                previewBitmap = new WriteableBitmap(new PixelSize(frame.Width, frame.Height), new Vector(96, 96), PixelFormat.Bgra8888, AlphaFormat.Premul);
            }
            using (var buffer = previewBitmap.Lock())
            {
                for (var row = 0; row < frame.Height; row++)
                {
                    System.Runtime.InteropServices.Marshal.Copy(frame.Pixels, row * frame.Stride, buffer.Address + (row * buffer.RowBytes), frame.Width * 4);
                }
            }
            PreviewImage.Source = null;
            PreviewImage.Source = previewBitmap;
        }
        catch (Exception error) when (error is IOException or InvalidDataException or UnauthorizedAccessException)
        {
            // The engine restarted and the ring is being recreated; try again on the next tick.
            preview?.Dispose();
            preview = null;
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
