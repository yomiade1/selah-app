using Avalonia;
using Avalonia.Controls;
using Avalonia.Headless;
using Avalonia.Headless.XUnit;
using Avalonia.Input;
using Avalonia.Styling;
using Avalonia.Threading;
using Avalonia.VisualTree;
using Selah.UI.Services;
using Selah.UI.ViewModels;
using Selah.UI.Views;

namespace Selah.UI.Tests;

/// <summary>
/// M1-A9: the shell at 100/150/200% text, Light and Booth, en and en-XA, with no clipped
/// critical label (DESIGN-SYSTEM §4.1). Each case saves a snapshot to target/ui-snapshots/.
/// </summary>
public sealed class LayoutTests : IDisposable
{
    private readonly Reset reset = new();

    public void Dispose() => reset.Dispose();

    public static TheoryData<int, string, string> Matrix()
    {
        var data = new TheoryData<int, string, string>();
        foreach (var scale in new[] { 100, 150, 200 })
        {
            foreach (var appearance in new[] { "light", "booth" })
            {
                foreach (var language in new[] { "en", Localizer.PseudoLocale })
                {
                    data.Add(scale, appearance, language);
                }
            }
        }
        return data;
    }

    private static void Configure(int scale, string appearance, string language)
    {
        TextScale.Instance.Percent = scale;
        Localizer.Instance.SetLanguage(language);
        Application.Current!.RequestedThemeVariant = appearance == "booth" ? ThemeVariant.Dark : ThemeVariant.Light;
    }

    private static MainViewModel LiveModel()
    {
        var model = new MainViewModel(null);
        // Everything critical showing: connected, live, and in safe mode.
        model.Apply(new ConnectionSnapshot(Connected: true, EngineConnected: true, SafeMode: true, Live: true, Generation: 1));
        return model;
    }

    private static Window Open(Window window, double width, double height)
    {
        window.Width = width;
        window.Height = height;
        window.Show();
        Dispatcher.UIThread.RunJobs();
        return window;
    }

    private static void Snapshot(Window window, string name)
    {
        var folder = Repo.PathOf("target", "ui-snapshots");
        Directory.CreateDirectory(folder);
        using var frame = window.CaptureRenderedFrame();
        using var file = File.Create(Path.Combine(folder, name + ".png"));
        frame?.Save(file, Avalonia.Media.Imaging.PngBitmapEncoderOptions.Default);
    }

    private static Rect InWindow(Visual visual, Window window)
    {
        var origin = visual.TranslatePoint(default, window) ?? default;
        return new Rect(origin, visual.Bounds.Size);
    }

    /// <summary>Critical controls that are hidden, outside the window or a clipping parent, or whose text doesn't fit.</summary>
    public static List<string> ClippedCriticalLabels(Window window)
    {
        var problems = new List<string>();
        var critical = window.GetVisualDescendants().OfType<Control>().Where(Critical.GetIsCritical).ToList();
        if (critical.Count == 0)
        {
            problems.Add("no critical labels found");
        }
        var client = new Rect(window.ClientSize).Inflate(0.5);
        foreach (var control in critical)
        {
            var name = control is TextBlock text ? $"\"{text.Text}\"" : control.GetType().Name;
            if (!control.IsEffectivelyVisible)
            {
                problems.Add($"{name} is hidden");
                continue;
            }
            var rect = InWindow(control, window);
            if (!client.Contains(rect))
            {
                problems.Add($"{name} at {rect} is outside the window {window.ClientSize}");
            }
            foreach (var ancestor in control.GetVisualAncestors().OfType<Visual>().Where(a => a.ClipToBounds && a != window))
            {
                if (!InWindow(ancestor, window).Inflate(0.5).Contains(rect))
                {
                    problems.Add($"{name} is clipped by {ancestor.GetType().Name}");
                    break;
                }
            }
            if (control is TextBlock block)
            {
                var layout = block.TextLayout;
                if (layout.WidthIncludingTrailingWhitespace > block.Bounds.Width + 0.5 || layout.Height > block.Bounds.Height + 0.5)
                {
                    problems.Add($"{name} needs {layout.WidthIncludingTrailingWhitespace:F0}×{layout.Height:F0} but has {block.Bounds.Size}");
                }
                if (layout.TextLines.Any(line => line.HasCollapsed))
                {
                    problems.Add($"{name} is trimmed");
                }
            }
        }
        return problems;
    }

    [AvaloniaTheory]
    [MemberData(nameof(Matrix))]
    public void Main_window_keeps_critical_labels_whole(int scale, string appearance, string language)
    {
        Configure(scale, appearance, language);
        var window = Open(new MainWindow { DataContext = LiveModel() }, 1280, 800);
        Snapshot(window, $"main-{scale}-{appearance}-{language}");
        var problems = ClippedCriticalLabels(window);
        window.Close();
        Assert.True(problems.Count == 0, string.Join("\n", problems));
    }

    [AvaloniaTheory]
    [MemberData(nameof(Matrix))]
    public void Settings_render_at_every_size(int scale, string appearance, string language)
    {
        Configure(scale, appearance, language);
        var settings = new SettingsViewModel();
        var window = Open(new SettingsWindow { DataContext = settings }, 820, 720);
        Snapshot(window, $"settings-{scale}-{appearance}-{language}");
        var name = window.GetVisualDescendants().OfType<TextBlock>().Single(t => t.Name == "TextSizeName");
        var expected = Localizer.Instance[TextScale.StepNameKeys[TextScale.Instance.StepIndex]];
        window.Close();
        Assert.Equal(expected, name.Text);
    }

    [AvaloniaFact]
    public void Shortcuts_change_the_text_size()
    {
        var window = Open(new MainWindow { DataContext = LiveModel() }, 1280, 800);
        var command = OperatingSystem.IsMacOS() ? RawInputModifiers.Meta : RawInputModifiers.Control;
        window.KeyPress(Key.OemPlus, command, PhysicalKey.Equal, "=");
        Assert.Equal(115, TextScale.Instance.Percent);
        window.KeyPress(Key.OemPlus, command, PhysicalKey.Equal, "=");
        Assert.Equal(130, TextScale.Instance.Percent);
        window.KeyPress(Key.OemMinus, command, PhysicalKey.Minus, "-");
        Assert.Equal(115, TextScale.Instance.Percent);
        window.KeyPress(Key.D0, command, PhysicalKey.Digit0, "0");
        Assert.Equal(100, TextScale.Instance.Percent);
        window.Close();
    }

    [AvaloniaFact]
    public void Text_size_scales_the_type_tokens()
    {
        var resources = Application.Current!.Resources;
        Assert.Equal(15.0, (double)resources["Type.Body.Size"]!);
        TextScale.Instance.Percent = 200;
        Assert.Equal(30.0, (double)resources["Type.Body.Size"]!);
        Assert.Equal(72.0, (double)resources["Size.ControlHeight"]!);
        Assert.Throws<ArgumentOutOfRangeException>(() => TextScale.Instance.Percent = 120);
    }

    [AvaloniaFact]
    public void Side_panels_hide_below_their_effective_widths()
    {
        var window = Open(new MainWindow { DataContext = LiveModel() }, 1280, 800);
        var sidebar = window.GetVisualDescendants().OfType<Border>().Single(b => b.Name == "Sidebar");
        var inspector = window.GetVisualDescendants().OfType<Border>().Single(b => b.Name == "Inspector");
        Assert.True(sidebar.IsVisible && inspector.IsVisible);
        TextScale.Instance.Percent = 130; // 1280 / 1.3 = 985 effective px
        Dispatcher.UIThread.RunJobs();
        Assert.True(sidebar.IsVisible);
        Assert.False(inspector.IsVisible);
        TextScale.Instance.Percent = 150; // 853 effective px
        Dispatcher.UIThread.RunJobs();
        Assert.False(sidebar.IsVisible);
        window.Close();
    }

    [AvaloniaFact]
    public void Status_says_connected_only_when_core_and_screens_are()
    {
        var model = new MainViewModel(null);
        Assert.Equal("Connecting…", model.StatusText);
        model.Apply(new ConnectionSnapshot(true, false, false, false, 1));
        Assert.Equal("Screens not connected", model.StatusText);
        model.Apply(new ConnectionSnapshot(true, true, false, false, 1));
        Assert.Equal("Connected", model.StatusText);
        Assert.True(model.ShowTestPatternCommand.CanExecute(null));
    }
}
