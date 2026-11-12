using System.Collections.ObjectModel;
using System.Text.Json;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Selah.Contracts.V1;
using Selah.UI.Services;

namespace Selah.UI.ViewModels;

/// <summary>
/// Settings ▸ Church: the CCLI licence number and the Bible Selah opens first. First-run setup
/// sets them too (<see cref="SetupViewModel"/>); this is where to change them later.
/// </summary>
public sealed partial class ChurchSettingsViewModel(ILibraryService? library) : ObservableObject
{
    /// <summary>CCLI licence numbers are short runs of digits.</summary>
    public const int MaxCcliLength = 12;

    public ObservableCollection<BibleTranslation> Bibles { get; } = [];

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsCcliValid))]
    private string ccliNumber = string.Empty;

    [ObservableProperty]
    private BibleTranslation? selectedBible;

    [ObservableProperty]
    private string? statusText;

    public bool IsCcliValid
    {
        get
        {
            var number = CcliNumber.Trim();
            return number.Length <= MaxCcliLength && number.All(char.IsAsciiDigit);
        }
    }

    /// <summary>Read what is saved now.</summary>
    public async Task LoadAsync()
    {
        if (library is null)
        {
            return;
        }
        try
        {
            var bibles = await library.ListBiblesAsync();
            Bibles.Clear();
            foreach (var bible in bibles)
            {
                Bibles.Add(bible);
            }
            CcliNumber = ReadString(await library.GetPolicySettingAsync(SetupViewModel.CcliKey)) ?? string.Empty;
            var code = ReadString(await library.GetPolicySettingAsync(SetupViewModel.BibleKey));
            SelectedBible = Bibles.FirstOrDefault(b => b.Code == code) ?? Bibles.FirstOrDefault();
            StatusText = null;
        }
        catch (CoreException error)
        {
            StatusText = Localizer.Instance.Get(error.MessageKey);
        }
    }

    [RelayCommand]
    private async Task SaveAsync()
    {
        if (!IsCcliValid)
        {
            StatusText = Localizer.Instance["setup-ccli-invalid"];
            return;
        }
        if (library is null)
        {
            return;
        }
        try
        {
            // An empty number is saved as empty: the Ready for Sunday check then says it's missing.
            await library.SetPolicySettingAsync(SetupViewModel.CcliKey, JsonSerializer.Serialize(CcliNumber.Trim()), PolicyLevel.Machine);
            if (SelectedBible is { } bible)
            {
                await library.SetPolicySettingAsync(SetupViewModel.BibleKey, JsonSerializer.Serialize(bible.Code), PolicyLevel.Machine);
            }
            StatusText = Localizer.Instance["church-saved"];
        }
        catch (CoreException error)
        {
            StatusText = Localizer.Instance.Get(error.MessageKey);
        }
    }

    private static string? ReadString(GetPolicySettingReply reply)
    {
        if (!reply.Found || reply.Setting is not { ValueJson: { Length: > 0 } json })
        {
            return null;
        }
        try
        {
            return JsonSerializer.Deserialize<string>(json);
        }
        catch (JsonException)
        {
            return null;
        }
    }
}
