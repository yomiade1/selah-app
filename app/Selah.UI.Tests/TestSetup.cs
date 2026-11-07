using Avalonia;
using Avalonia.Headless;
using Selah.UI;
using Selah.UI.Services;

[assembly: AvaloniaTestApplication(typeof(Selah.UI.Tests.TestAppBuilder))]
// The text size and language are app-wide, so tests run one at a time.
[assembly: CollectionBehavior(DisableTestParallelization = true)]

namespace Selah.UI.Tests;

public static class TestAppBuilder
{
    /// <summary>The real App (styles, tokens, fonts), rendered by Skia with no display.</summary>
    public static AppBuilder BuildAvaloniaApp() =>
        AppBuilder.Configure<App>().UseSkia().UseHeadless(new AvaloniaHeadlessPlatformOptions { UseHeadlessDrawing = false });
}

/// <summary>Where the repository is, for tests that read its source files.</summary>
public static class Repo
{
    public static string Root { get; } = Find();

    private static string Find()
    {
        var directory = new DirectoryInfo(AppContext.BaseDirectory);
        while (directory is not null && !File.Exists(Path.Combine(directory.FullName, "design", "tokens", "tokens.json")))
        {
            directory = directory.Parent;
        }
        return directory?.FullName ?? throw new DirectoryNotFoundException("repository root");
    }

    public static string PathOf(params string[] parts) => Path.Combine([Root, .. parts]);
}

/// <summary>Puts the app-wide settings back after each test.</summary>
public sealed class Reset : IDisposable
{
    public void Dispose()
    {
        TextScale.Instance.Percent = TextScale.DefaultPercent;
        Localizer.Instance.SetLanguage("en");
        if (Application.Current is { } app)
        {
            app.RequestedThemeVariant = Avalonia.Styling.ThemeVariant.Light;
        }
    }
}
