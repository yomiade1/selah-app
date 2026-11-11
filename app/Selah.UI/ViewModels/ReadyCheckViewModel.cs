using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Selah.Contracts.V1;
using Selah.UI.Services;

namespace Selah.UI.ViewModels;

/// <summary>One line of the Ready for Sunday? list.</summary>
public sealed record ReadyCheckRow(ReadyCheck Check)
{
    public string KindName => Localizer.Instance[Check.Kind switch
    {
        ReadyCheckKind.Media => "ready-kind-media",
        ReadyCheckKind.Fonts => "ready-kind-fonts",
        ReadyCheckKind.TextFits => "ready-kind-text-fits",
        ReadyCheckKind.Copyright => "ready-kind-copyright",
        ReadyCheckKind.Outputs => "ready-kind-outputs",
        ReadyCheckKind.Backup => "ready-kind-backup",
        ReadyCheckKind.Flash => "ready-kind-flash",
        _ => "ready-kind-sound",
    }];

    public string StatusText => Localizer.Instance[Check.Status switch
    {
        ReadyCheckStatus.Pass => "ready-status-pass",
        ReadyCheckStatus.Warn => "ready-status-warn",
        ReadyCheckStatus.Fail => "ready-status-fail",
        _ => "ready-status-info",
    }];

    public string Message => Localizer.Instance.Get(Check.MessageKey, Check.Args.Select(a => (a.Key, (object)a.Value)).ToArray());

    public bool IsPass => Check.Status == ReadyCheckStatus.Pass;

    public bool IsWarn => Check.Status is ReadyCheckStatus.Warn or ReadyCheckStatus.Info;

    public bool IsFail => Check.Status == ReadyCheckStatus.Fail;

    public bool HasFix => !IsPass && Check.FixArticleId.Length > 0;
}

/// <summary>Ready for Sunday? (M4.7): core runs the checks; each problem links to its fix.</summary>
public sealed partial class ReadyCheckViewModel(ILibraryService? library, Func<Service?> selectedService) : ObservableObject
{
    public ObservableCollection<ReadyCheckRow> Rows { get; } = [];

    [ObservableProperty]
    private string summary = Localizer.Instance["ready-none-yet"];

    [ObservableProperty]
    private bool isRunning;

    /// <summary>Raised with an article id when someone selects How to fix.</summary>
    public event EventHandler<string>? FixRequested;

    [RelayCommand]
    private async Task RunAsync()
    {
        if (library is null || selectedService()?.Id is not { } id)
        {
            Summary = Localizer.Instance["ready-none-yet"];
            return;
        }
        IsRunning = true;
        try
        {
            var report = await library.RunReadyCheckAsync(id);
            Rows.Clear();
            // Problems first, so nobody misses them.
            foreach (var check in report.Checks.OrderByDescending(c => c.Status == ReadyCheckStatus.Fail).ThenByDescending(c => c.Status == ReadyCheckStatus.Warn))
            {
                Rows.Add(new ReadyCheckRow(check));
            }
            Summary = Localizer.Instance[!report.Ready ? "ready-has-problems"
                : Rows.Any(r => r.Check.Status == ReadyCheckStatus.Warn) ? "ready-has-warnings" : "ready-all-good"];
        }
        catch (CoreException error)
        {
            Summary = Localizer.Instance.Get(error.MessageKey);
        }
        finally
        {
            IsRunning = false;
        }
    }

    [RelayCommand]
    private void OpenFix(ReadyCheckRow row) => FixRequested?.Invoke(this, row.Check.FixArticleId);
}
