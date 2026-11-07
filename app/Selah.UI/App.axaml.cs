using Avalonia;
using Avalonia.Controls.ApplicationLifetimes;
using Avalonia.Markup.Xaml;
using Selah.UI.Services;
using Selah.UI.ViewModels;
using Selah.UI.Views;

namespace Selah.UI;

public sealed partial class App : Application
{
    private CoreConnection? connection;

    public bool Headless { get; init; }

    public override void Initialize()
    {
        AvaloniaXamlLoader.Load(this);
        // Sizes start at 100% and follow the Text size setting (ADR-009).
        TextScale.Instance.Apply(Resources);
        var language = Environment.GetEnvironmentVariable("SELAH_LANGUAGE");
        if (!string.IsNullOrEmpty(language))
        {
            Localizer.Instance.SetLanguage(language);
        }
    }

    public override void OnFrameworkInitializationCompleted()
    {
        if (ApplicationLifetime is IClassicDesktopStyleApplicationLifetime desktop)
        {
            connection = new CoreConnection();
            var model = new MainViewModel(connection);
            desktop.MainWindow = new MainWindow { DataContext = model };
            desktop.ShutdownRequested += async (_, _) =>
            {
                if (connection is not null)
                {
                    await connection.DisposeAsync();
                }
            };
            connection.Start();
        }
        base.OnFrameworkInitializationCompleted();
    }
}
