using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Selah.Contracts.V1;
using Selah.UI.Services;

namespace Selah.UI.ViewModels;

/// <summary>What Settings ▸ Updates needs from core (M1.11).</summary>
public interface IUpdateService
{
    Task SetUpdateSettingsAsync(string feedUrl, bool allow);

    Task<UpdateCheck> CheckForUpdateAsync();

    Task InstallUpdateAsync(string version);
}

/// <summary>
/// Settings ▸ Updates (M1.11, SEC-12, SEC-13, SAFE-06). Selah looks for a new version only when
/// the person selects <b>Check for updates</b>, and only if they turned checks on and gave an
/// address. <b>Install update</b> asks first if something is on the screens.
/// </summary>
public sealed partial class UpdatesViewModel : ObservableObject
{
    private readonly IUpdateService? service;
    private readonly Func<Task<bool>>? confirmIfLive;
    private bool applyingFromCore;

    /// <param name="service">Core, or null in previews and layout tests.</param>
    /// <param name="confirmIfLive">Asks the person to clear the screens first when something is on them (SAFE-06); true to go on.</param>
    public UpdatesViewModel(IUpdateService? service = null, Func<Task<bool>>? confirmIfLive = null)
    {
        this.service = service;
        this.confirmIfLive = confirmIfLive;
    }

    [ObservableProperty]
    [NotifyCanExecuteChangedFor(nameof(CheckCommand))]
    private bool allowUpdateCheck;

    [ObservableProperty]
    [NotifyCanExecuteChangedFor(nameof(CheckCommand))]
    private string feedUrl = string.Empty;

    /// <summary>The result of the last action, in plain words.</summary>
    [ObservableProperty]
    private string? message;

    [ObservableProperty]
    [NotifyCanExecuteChangedFor(nameof(InstallCommand))]
    [NotifyPropertyChangedFor(nameof(CanInstall))]
    private string? availableVersion;

    [ObservableProperty]
    [NotifyCanExecuteChangedFor(nameof(CheckCommand))]
    [NotifyCanExecuteChangedFor(nameof(InstallCommand))]
    [NotifyCanExecuteChangedFor(nameof(SaveAddressCommand))]
    private bool isBusy;

    public bool CanCheck => AllowUpdateCheck && !string.IsNullOrWhiteSpace(FeedUrl) && !IsBusy;

    public bool CanInstall => AvailableVersion is not null && !IsBusy;

    public bool CanSave => !IsBusy;

    /// <summary>Core sent the stored settings.</summary>
    public void ApplyFromCore(Settings settings)
    {
        applyingFromCore = true;
        try
        {
            FeedUrl = settings.UpdateFeedUrl;
            AllowUpdateCheck = settings.AllowUpdateCheck;
        }
        finally
        {
            applyingFromCore = false;
        }
    }

    partial void OnAllowUpdateCheckChanged(bool value)
    {
        if (!applyingFromCore)
        {
            _ = SaveAsync();
        }
    }

    [RelayCommand(CanExecute = nameof(CanSave))]
    private Task SaveAddressAsync() => SaveAsync();

    private async Task SaveAsync()
    {
        if (service is null)
        {
            return;
        }
        await RunAsync(async () =>
        {
            await service.SetUpdateSettingsAsync(FeedUrl.Trim(), AllowUpdateCheck);
            Message = Localizer.Instance["updates-saved"];
        });
    }

    [RelayCommand(CanExecute = nameof(CanCheck))]
    private async Task CheckAsync()
    {
        if (service is null)
        {
            return;
        }
        Message = Localizer.Instance["updates-checking"];
        await RunAsync(async () =>
        {
            var result = await service.CheckForUpdateAsync();
            AvailableVersion = result.Available ? result.Version : null;
            Message = result.Available
                ? Localizer.Instance.Get("updates-available", ("version", result.Version))
                : Localizer.Instance.Get("updates-none", ("version", result.CurrentVersion));
        });
    }

    [RelayCommand(CanExecute = nameof(CanInstall))]
    private async Task InstallAsync()
    {
        if (service is null || AvailableVersion is not { } version)
        {
            return;
        }
        // SAFE-06: never while something is on the screens, unless the person clears them first.
        if (confirmIfLive is not null && !await confirmIfLive())
        {
            return;
        }
        Message = Localizer.Instance["updates-installing"];
        await RunAsync(() => service.InstallUpdateAsync(version));
    }

    private async Task RunAsync(Func<Task> action)
    {
        IsBusy = true;
        try
        {
            await action();
        }
        catch (CoreException error)
        {
            Message = Localizer.Instance[error.MessageKey];
        }
        catch (Exception error) when (error is TimeoutException or IOException or InvalidOperationException or OperationCanceledException)
        {
            Message = Localizer.Instance["error-not-connected"];
        }
        finally
        {
            IsBusy = false;
        }
    }
}
