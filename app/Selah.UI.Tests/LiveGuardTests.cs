using Avalonia.Controls;
using Avalonia.Headless.XUnit;
using Avalonia.Threading;
using Avalonia.VisualTree;
using Selah.UI.Services;
using Selah.UI.ViewModels;
using Selah.UI.Views;

namespace Selah.UI.Tests;

/// <summary>SAFE-06: quitting while something is on the screens asks first (M1.12).</summary>
public sealed class LiveGuardTests : IDisposable
{
    private readonly Reset reset = new();

    public void Dispose() => reset.Dispose();

    private static (MainWindow Window, MainViewModel Model) Open(bool live)
    {
        var model = new MainViewModel(null);
        // Safe mode too, so every critical label is showing for the clipping check.
        model.Apply(new ConnectionSnapshot(true, true, SafeMode: true, live, 1));
        var window = new MainWindow { DataContext = model, Width = 1280, Height = 800 };
        window.Show();
        Dispatcher.UIThread.RunJobs();
        return (window, model);
    }

    [AvaloniaFact]
    public async Task Quitting_while_live_asks_first_and_keep_showing_stays_open()
    {
        // safe: SAFE-06
        var (window, model) = Open(live: true);
        Assert.True(window.RequestClose(programmatic: false, osShutdown: false));
        Dispatcher.UIThread.RunJobs();
        var bar = window.GetVisualDescendants().OfType<Border>().Single(b => b.Name == "LiveGuardBar");
        Assert.True(bar.IsEffectivelyVisible);
        Assert.Equal("Do you want to stop showing it and quit?", model.LiveGuardQuestion);
        Assert.Equal("Clear and quit", model.LiveGuardConfirmText);

        // The program preview and Clear everything stay visible below and above it.
        var clear = window.GetVisualDescendants().OfType<Button>().Single(b => b.Name == "ClearEverythingButton");
        Assert.True(clear.IsEffectivelyVisible);
        Assert.Empty(LayoutTests.ClippedCriticalLabels(window));

        model.CancelLiveGuardCommand.Execute(null);
        Dispatcher.UIThread.RunJobs();
        Assert.True(window.IsVisible);
        Assert.False(model.IsLiveGuardOpen);

        Assert.True(window.RequestClose(programmatic: false, osShutdown: false));
        await model.ConfirmLiveGuardCommand.ExecuteAsync(null);
        Dispatcher.UIThread.RunJobs();
        Assert.False(window.IsVisible);
    }

    [AvaloniaFact]
    public void Quitting_with_nothing_on_the_screens_does_not_ask()
    {
        var (window, model) = Open(live: false);
        Assert.False(window.RequestClose(programmatic: false, osShutdown: false));
        Assert.False(model.IsLiveGuardOpen);
        window.Close();
    }

    [AvaloniaFact]
    public void The_computer_shutting_down_is_never_held()
    {
        var (window, model) = Open(live: true);
        Assert.False(window.RequestClose(programmatic: false, osShutdown: true));
        Assert.False(model.IsLiveGuardOpen);
        window.Close();
    }

    [AvaloniaFact]
    public void Safe_mode_banner_offers_restart_normally()
    {
        // safe: SAFE-07
        var model = new MainViewModel(null);
        model.Apply(new ConnectionSnapshot(true, true, SafeMode: true, Live: false, 1));
        var window = new MainWindow { DataContext = model, Width = 1280, Height = 800 };
        window.Show();
        Dispatcher.UIThread.RunJobs();
        var restart = window.GetVisualDescendants().OfType<Button>().Single(b => b.Name == "SafeModeRestart");
        Assert.True(restart.IsEffectivelyVisible);
        Assert.Equal("Restart normally", restart.Content);
        Assert.True(model.RestartNormallyCommand.CanExecute(null));
        window.Close();
    }
}
