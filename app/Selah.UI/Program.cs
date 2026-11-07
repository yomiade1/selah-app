using Avalonia;
using Avalonia.Headless;

namespace Selah.UI;

/// <summary>
/// The Selah desktop app. Started by <c>selah-watchdog</c> (ADR-001); it can also run alone for
/// development, and reconnects to core whenever core restarts.
/// <c>--headless</c> runs with no display (watchdog chaos tests on CI, the installer check).
/// </summary>
internal static class Program
{
    [STAThread]
    public static int Main(string[] args)
    {
        var headless = args.Contains("--headless") || Environment.GetEnvironmentVariable("SELAH_HEADLESS") == "1";
        return BuildAvaloniaApp(headless).StartWithClassicDesktopLifetime(args);
    }

    /// <summary>Used by the XAML previewer.</summary>
    public static AppBuilder BuildAvaloniaApp() => BuildAvaloniaApp(headless: false);

    private static AppBuilder BuildAvaloniaApp(bool headless)
    {
        var builder = AppBuilder.Configure(() => new App { Headless = headless });
        builder = headless
            ? builder.UseSkia().UseHeadless(new AvaloniaHeadlessPlatformOptions { UseHeadlessDrawing = false })
            : builder.UsePlatformDetect();
        return builder.LogToTrace();
    }
}
