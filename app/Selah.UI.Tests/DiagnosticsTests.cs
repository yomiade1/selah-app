using Avalonia.Controls;
using Avalonia.Headless;
using Avalonia.Headless.XUnit;
using Avalonia.Threading;
using Avalonia.VisualTree;
using Selah.Contracts.V1;
using Selah.UI.Services;
using Selah.UI.ViewModels;
using Selah.UI.Views;

namespace Selah.UI.Tests;

/// <summary>Help ▸ Report a problem and Help ▸ Diagnostics (M1.13, M1-A15).</summary>
public sealed class DiagnosticsTests : IDisposable
{
    private readonly Reset reset = new();

    public void Dispose() => reset.Dispose();

    private sealed class FakeDiagnostics : IDiagnosticsService
    {
        public Exception? Failure { get; set; }

        public Task<Diagnostics> GetDiagnosticsAsync() => Task.FromResult(new Diagnostics
        {
            Version = "0.1.0",
            Os = "windows",
            Arch = "x86_64",
            EngineConnected = true,
            Renderer = "wgpu Dx12 Test GPU",
            OutputResolution = new Resolution { Width = 1920, Height = 1080 },
        });

        public Task<DiagnosticsReport> RunDiagnosticsAsync(bool quick) => Failure is { } failure
            ? Task.FromException<DiagnosticsReport>(failure)
            : Task.FromResult(new DiagnosticsReport
            {
                Path = "C:/support/selah-diagnostics-x.json",
                IpcP99Ms = 0.096,
                IpcPass = true,
                VideoClip = "hevc-2160p60.mp4",
                VideoFrames = 180,
                VideoDropped = 9,
                VideoCompositeP99Ms = 16,
                VideoPass = false,
                M3P99Ms = 14.9,
                M3Pass = false,
            });

        public Task<SupportBundle> CreateSupportBundleAsync() => Failure is { } failure
            ? Task.FromException<SupportBundle>(failure)
            : Task.FromResult(new SupportBundle
            {
                Path = "C:/support/selah-support-x.zip",
                ReportUrl = "https://github.com/yomiade1/selah-app/issues/new?template=problem-report.yml&version=0.1.0",
            });
    }

    [AvaloniaFact]
    public async Task Report_a_problem_saves_a_file_and_opens_the_form_with_the_version()
    {
        // sec: SEC-06
        var model = new MainViewModel(null, new FakeDiagnostics());
        var opened = new List<string>();
        var shown = new List<string>();
        model.OpenUrlRequested += (_, url) => opened.Add(url);
        model.ShowFileRequested += (_, path) => shown.Add(path);

        await model.ReportProblemCommand.ExecuteAsync(null);
        Assert.True(model.IsReportOpen);
        Assert.Equal("C:/support/selah-support-x.zip", model.ReportPath);
        Assert.Equal(Localizer.Instance["report-saved"], model.ReportMessage);
        Assert.Single(opened);
        Assert.EndsWith("version=0.1.0", opened[0]);

        model.ShowReportFileCommand.Execute(null);
        Assert.Equal(["C:/support/selah-support-x.zip"], shown);
        model.CloseReportCommand.Execute(null);
        Assert.False(model.IsReportOpen);
    }

    [AvaloniaFact]
    public async Task A_bundle_that_cannot_be_saved_says_so_and_opens_nothing()
    {
        var model = new MainViewModel(null, new FakeDiagnostics { Failure = new CoreException("SEL-SUP-0001", "error-support-bundle") });
        var opened = 0;
        model.OpenUrlRequested += (_, _) => opened++;
        await model.ReportProblemCommand.ExecuteAsync(null);
        Assert.Equal(Localizer.Instance["error-support-bundle"], model.ReportMessage);
        Assert.False(model.HasReportFile);
        Assert.Equal(0, opened);
    }

    [AvaloniaFact]
    public async Task Diagnostics_shows_details_and_benchmark_results()
    {
        var diagnostics = new DiagnosticsViewModel(new FakeDiagnostics());
        await diagnostics.LoadAsync();
        Assert.Contains(diagnostics.Rows, row => row.Value == "wgpu Dx12 Test GPU");
        Assert.Contains(diagnostics.Rows, row => row.Value == "1920 × 1080");

        await diagnostics.RunBenchmarkCommand.ExecuteAsync(null);
        Assert.True(diagnostics.HasReport);
        Assert.Equal(Localizer.Instance["diagnostics-saved"], diagnostics.Message);
        Assert.Contains(diagnostics.Results, row => row.Value.Contains("9 of 180 frames dropped", StringComparison.Ordinal));
        Assert.Contains(diagnostics.Results, row =>
            row.Label == Localizer.Instance["diagnostics-m3"]
            && row.Value.Contains(14.9.ToString("0.0", System.Globalization.CultureInfo.CurrentCulture), StringComparison.Ordinal));
        var shown = new List<string>();
        diagnostics.ShowFileRequested += (_, path) => shown.Add(path);
        diagnostics.ShowReportCommand.Execute(null);
        Assert.Equal(["C:/support/selah-diagnostics-x.json"], shown);
    }

    [AvaloniaFact]
    public async Task The_benchmark_refused_while_live_says_why()
    {
        // safe: SAFE-06
        var diagnostics = new DiagnosticsViewModel(new FakeDiagnostics { Failure = new CoreException("SEL-DIAG-0001", "error-diagnostics-live") });
        await diagnostics.RunBenchmarkCommand.ExecuteAsync(null);
        Assert.Equal("Something is on the screens. Clear them, then run the benchmark.", diagnostics.Message);
        Assert.False(diagnostics.HasReport);
        Assert.False(diagnostics.IsRunning);
    }

    [AvaloniaFact]
    public void The_help_menu_has_report_a_problem_and_diagnostics()
    {
        var window = new MainWindow { DataContext = new MainViewModel(null), Width = 1280, Height = 800 };
        window.Show();
        Dispatcher.UIThread.RunJobs();
        var help = window.GetVisualDescendants().OfType<Button>().Single(b => b.Name == "HelpMenuButton");
        var items = ((MenuFlyout)help.Flyout!).Items.OfType<MenuItem>().Select(i => i.Header).ToList();
        Assert.Equal(
            ["Help for this screen (F1)", "What's this? (Shift+F1)", "Help articles", "Report a problem", "Diagnostics"],
            items);
        window.Close();
    }

    [AvaloniaTheory]
    [MemberData(nameof(LayoutTests.Matrix), MemberType = typeof(LayoutTests))]
    public async Task Diagnostics_render_at_every_size(int scale, string appearance, string language)
    {
        TextScale.Instance.Percent = scale;
        Localizer.Instance.SetLanguage(language);
        Avalonia.Application.Current!.RequestedThemeVariant = appearance == "booth"
            ? Avalonia.Styling.ThemeVariant.Dark
            : Avalonia.Styling.ThemeVariant.Light;
        var diagnostics = new DiagnosticsViewModel(new FakeDiagnostics());
        await diagnostics.LoadAsync();
        await diagnostics.RunBenchmarkCommand.ExecuteAsync(null);
        var window = new DiagnosticsWindow { DataContext = diagnostics, Width = 720, Height = 640 };
        window.Show();
        Dispatcher.UIThread.RunJobs();
        var folder = Repo.PathOf("target", "ui-snapshots");
        Directory.CreateDirectory(folder);
        using (var frame = window.CaptureRenderedFrame())
        using (var file = File.Create(Path.Combine(folder, $"diagnostics-{scale}-{appearance}-{language}.png")))
        {
            frame?.Save(file, Avalonia.Media.Imaging.PngBitmapEncoderOptions.Default);
        }
        var results = window.GetVisualDescendants().OfType<Border>().Single(b => b.Name == "DiagnosticsResults");
        window.Close();
        Assert.True(results.IsVisible);
    }
}
