using Avalonia.Controls;
using Selah.UI.Services;
using Selah.UI.ViewModels;

namespace Selah.UI.Views;

public sealed partial class SettingsWindow : Window, IHelpHost
{
    public SettingsWindow()
    {
        InitializeComponent();
    }

    public IHelpSource? HelpSource => (Owner as IHelpHost)?.HelpSource;

    public async Task OpenHelpForAnchorsAsync(IEnumerable<string> anchors)
    {
        if (Owner is IHelpHost host)
        {
            await host.OpenHelpForAnchorsAsync(anchors);
            return;
        }
        OpenStandaloneHelp(null);
    }

    public async Task OpenHelpArticleAsync(string articleId)
    {
        if (Owner is IHelpHost host)
        {
            await host.OpenHelpArticleAsync(articleId);
            return;
        }
        OpenStandaloneHelp(articleId);
    }

    private void OpenStandaloneHelp(string? articleId)
    {
        var help = new HelpViewModel(HelpSource);
        var window = new HelpWindow { DataContext = help };
        window.Show(this);
        if (!string.IsNullOrWhiteSpace(articleId))
        {
            _ = help.OpenAsync(articleId);
        }
    }
}
