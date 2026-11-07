using Avalonia.Headless.XUnit;
using Selah.Contracts.V1;
using Selah.UI.Services;
using Selah.UI.ViewModels;

namespace Selah.UI.Tests;

/// <summary>Settings ▸ Updates (M1.11): checks only when asked; never installs while live (SAFE-06, SEC-12).</summary>
public sealed class UpdateTests : IDisposable
{
    private readonly Reset reset = new();

    public void Dispose() => reset.Dispose();

    private sealed class FakeUpdates : IUpdateService
    {
        public List<(string Feed, bool Allow)> Saved { get; } = [];

        public List<string> Installed { get; } = [];

        public int Checks { get; private set; }

        public Exception? Failure { get; set; }

        public Task SetUpdateSettingsAsync(string feedUrl, bool allow)
        {
            Saved.Add((feedUrl, allow));
            return Task.CompletedTask;
        }

        public Task<UpdateCheck> CheckForUpdateAsync()
        {
            Checks++;
            return Failure is { } failure
                ? Task.FromException<UpdateCheck>(failure)
                : Task.FromResult(new UpdateCheck { Available = true, Version = "9.0.0", CurrentVersion = "0.0.1" });
        }

        public Task InstallUpdateAsync(string version)
        {
            Installed.Add(version);
            return Task.CompletedTask;
        }
    }

    private static MainViewModel Live()
    {
        var model = new MainViewModel(null);
        model.Apply(new ConnectionSnapshot(true, true, false, Live: true, 1));
        return model;
    }

    [AvaloniaFact]
    public async Task Checking_needs_the_switch_and_an_address()
    {
        // sec: SEC-13
        var fake = new FakeUpdates();
        var updates = new UpdatesViewModel(fake);
        Assert.False(updates.CheckCommand.CanExecute(null));
        updates.FeedUrl = "https://updates.example.invalid/releases.dev.json";
        Assert.False(updates.CheckCommand.CanExecute(null));
        updates.AllowUpdateCheck = true;
        Assert.True(updates.CheckCommand.CanExecute(null));
        Assert.Equal(("https://updates.example.invalid/releases.dev.json", true), fake.Saved.Last());

        await updates.CheckCommand.ExecuteAsync(null);
        Assert.Equal(1, fake.Checks);
        Assert.Equal("9.0.0", updates.AvailableVersion);
        Assert.Equal("Version 9.0.0 is ready to install.", updates.Message);
    }

    [AvaloniaFact]
    public void Settings_from_core_are_shown_without_saving_them_back()
    {
        var fake = new FakeUpdates();
        var updates = new UpdatesViewModel(fake);
        updates.ApplyFromCore(new Settings { UpdateFeedUrl = "https://x.example.invalid/releases.dev.json", AllowUpdateCheck = true });
        Assert.True(updates.AllowUpdateCheck);
        Assert.Empty(fake.Saved);
    }

    [AvaloniaFact]
    public async Task Core_errors_are_shown_in_plain_words()
    {
        var fake = new FakeUpdates { Failure = new CoreException("SEL-UPD-0002", "error-update-feed") };
        var updates = new UpdatesViewModel(fake) { FeedUrl = "https://x.example.invalid/releases.dev.json" };
        updates.AllowUpdateCheck = true;
        await updates.CheckCommand.ExecuteAsync(null);
        Assert.Equal(Localizer.Instance["error-update-feed"], updates.Message);
        Assert.Null(updates.AvailableVersion);
    }

    [AvaloniaFact]
    public async Task Installing_while_live_asks_first_and_keep_showing_installs_nothing()
    {
        // safe: SAFE-06
        var model = Live();
        var fake = new FakeUpdates();
        var updates = new UpdatesViewModel(fake, () => model.ConfirmIfLiveAsync(LiveGuardKind.Update))
        {
            FeedUrl = "https://x.example.invalid/releases.dev.json",
        };
        updates.AllowUpdateCheck = true;
        await updates.CheckCommand.ExecuteAsync(null);

        var install = updates.InstallCommand.ExecuteAsync(null);
        Assert.True(model.IsLiveGuardOpen);
        Assert.Equal(LiveGuardKind.Update, model.LiveGuard);
        Assert.Equal("Do you want to stop showing it and update?", model.LiveGuardQuestion);
        Assert.Equal("Clear and update", model.LiveGuardConfirmText);
        model.CancelLiveGuardCommand.Execute(null);
        await install;
        Assert.False(model.IsLiveGuardOpen);
        Assert.Empty(fake.Installed);

        install = updates.InstallCommand.ExecuteAsync(null);
        await model.ConfirmLiveGuardCommand.ExecuteAsync(null);
        await install;
        Assert.Equal(["9.0.0"], fake.Installed);
    }

    [AvaloniaFact]
    public async Task Installing_with_nothing_on_the_screens_does_not_ask()
    {
        var model = new MainViewModel(null);
        model.Apply(new ConnectionSnapshot(true, true, false, Live: false, 1));
        var fake = new FakeUpdates();
        var updates = new UpdatesViewModel(fake, () => model.ConfirmIfLiveAsync(LiveGuardKind.Update))
        {
            FeedUrl = "https://x.example.invalid/releases.dev.json",
        };
        updates.AllowUpdateCheck = true;
        await updates.CheckCommand.ExecuteAsync(null);
        await updates.InstallCommand.ExecuteAsync(null);
        Assert.False(model.IsLiveGuardOpen);
        Assert.Equal(["9.0.0"], fake.Installed);
    }
}
