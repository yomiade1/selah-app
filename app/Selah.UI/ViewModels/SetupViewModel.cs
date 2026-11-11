using System.Collections.ObjectModel;
using System.Text.Json;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Selah.Contracts.V1;
using Selah.UI.Services;

namespace Selah.UI.ViewModels;

/// <summary>
/// First-run setup (M4.7): welcome, screens, Bible, CCLI licence and a sample service of
/// public-domain hymns. Answers are policy settings in core; everything can change later.
/// </summary>
public sealed partial class SetupViewModel(ILibraryService? library) : ObservableObject
{
    public const string CompletedKey = "setup.completed";
    public const string CcliKey = "church.ccli-licence";
    public const string BibleKey = "scripture.default-translation";
    public const int StepCount = 5;

    public ObservableCollection<OutputConfig> Screens { get; } = [];

    public ObservableCollection<BibleTranslation> Bibles { get; } = [];

    [ObservableProperty]
    private bool isOpen;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(StepText))]
    [NotifyPropertyChangedFor(nameof(IsWelcome))]
    [NotifyPropertyChangedFor(nameof(IsScreens))]
    [NotifyPropertyChangedFor(nameof(IsBible))]
    [NotifyPropertyChangedFor(nameof(IsCcli))]
    [NotifyPropertyChangedFor(nameof(IsSample))]
    [NotifyPropertyChangedFor(nameof(CanGoBack))]
    [NotifyPropertyChangedFor(nameof(IsLastStep))]
    [NotifyPropertyChangedFor(nameof(IsNotLastStep))]
    private int step;

    [ObservableProperty]
    private BibleTranslation? selectedBible;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsCcliValid))]
    private string ccliNumber = string.Empty;

    [ObservableProperty]
    private bool sampleAdded;

    [ObservableProperty]
    private string? statusText;

    public bool IsWelcome => Step == 0;
    public bool IsScreens => Step == 1;
    public bool IsBible => Step == 2;
    public bool IsCcli => Step == 3;
    public bool IsSample => Step == 4;
    public bool CanGoBack => Step > 0;
    public bool IsLastStep => Step == StepCount - 1;
    public bool IsNotLastStep => !IsLastStep;
    public bool HasScreens => Screens.Count > 0;
    public bool IsCcliValid => CcliNumber.Trim().Length == 0 || CcliNumber.Trim().All(char.IsAsciiDigit);

    public string StepText => Localizer.Instance.Get("setup-step",
        ("step", (Step + 1).ToString(System.Globalization.CultureInfo.CurrentCulture)),
        ("count", StepCount.ToString(System.Globalization.CultureInfo.CurrentCulture)));

    /// <summary>Open setup if this library has never finished it.</summary>
    public async Task OpenIfFirstRunAsync()
    {
        if (library is null)
        {
            return;
        }
        try
        {
            var done = await library.GetPolicySettingAsync(CompletedKey);
            if (done.Found)
            {
                return;
            }
            await LoadAsync();
            Step = 0;
            IsOpen = true;
        }
        catch (CoreException)
        {
            // Core will answer later; setup can open from Help then.
        }
    }

    private async Task LoadAsync()
    {
        if (library is null)
        {
            return;
        }
        Screens.Clear();
        foreach (var screen in await library.GetOutputConfigsAsync())
        {
            Screens.Add(screen);
        }
        OnPropertyChanged(nameof(HasScreens));
        Bibles.Clear();
        foreach (var bible in await library.ListBiblesAsync())
        {
            Bibles.Add(bible);
        }
        SelectedBible ??= Bibles.FirstOrDefault(b => b.Code == "KJV") ?? Bibles.FirstOrDefault();
    }

    [RelayCommand]
    private void Next()
    {
        if (IsCcli && !IsCcliValid)
        {
            StatusText = Localizer.Instance["setup-ccli-invalid"];
            return;
        }
        StatusText = null;
        if (Step < StepCount - 1)
        {
            Step++;
        }
    }

    [RelayCommand]
    private void Back()
    {
        if (Step > 0)
        {
            Step--;
        }
    }

    [RelayCommand]
    private async Task AddSampleAsync()
    {
        if (library is null || SampleAdded)
        {
            return;
        }
        try
        {
            await SampleService.AddAsync(library);
            SampleAdded = true;
            StatusText = Localizer.Instance["setup-sample-added"];
        }
        catch (CoreException error)
        {
            StatusText = Localizer.Instance.Get(error.MessageKey);
        }
    }

    [RelayCommand]
    private async Task FinishAsync()
    {
        if (!IsCcliValid)
        {
            Step = 3;
            StatusText = Localizer.Instance["setup-ccli-invalid"];
            return;
        }
        if (library is not null)
        {
            try
            {
                var number = CcliNumber.Trim();
                if (number.Length > 0)
                {
                    await library.SetPolicySettingAsync(CcliKey, JsonSerializer.Serialize(number), PolicyLevel.Machine);
                }
                if (SelectedBible is { } bible)
                {
                    await library.SetPolicySettingAsync(BibleKey, JsonSerializer.Serialize(bible.Code), PolicyLevel.Machine);
                }
                await library.SetPolicySettingAsync(CompletedKey, "true", PolicyLevel.Machine);
            }
            catch (CoreException error)
            {
                StatusText = Localizer.Instance.Get(error.MessageKey);
                return;
            }
        }
        IsOpen = false;
    }

    [RelayCommand]
    private async Task SkipAsync()
    {
        if (library is not null)
        {
            try
            {
                await library.SetPolicySettingAsync(CompletedKey, "true", PolicyLevel.Machine);
            }
            catch (CoreException)
            {
            }
        }
        IsOpen = false;
    }
}

/// <summary>The first-run sample service: three public-domain hymns (content/samples).</summary>
public static class SampleService
{
    private sealed record SampleSection(string Label, List<string> Lines);

    private sealed record SampleSong(string Title, List<string> Authors, string Copyright, string Key, List<SampleSection> Sections);

    private sealed record SampleFile(string Service_name, List<SampleSong> Songs);

    public static string FilePath => Path.Combine(AppContext.BaseDirectory, "samples", "sample-service.json");

    public static async Task<Service> AddAsync(ILibraryService library, string? path = null)
    {
        var json = await File.ReadAllTextAsync(path ?? FilePath);
        var sample = JsonSerializer.Deserialize<SampleFile>(json, new JsonSerializerOptions { PropertyNameCaseInsensitive = true })
            ?? throw new InvalidDataException("sample service is empty");
        var service = new Service { Id = LibraryIds.NewId(), Name = sample.Service_name };
        foreach (var entry in sample.Songs)
        {
            var song = new Song { Id = LibraryIds.NewId(), Title = entry.Title, Copyright = entry.Copyright, Key = entry.Key };
            song.Authors.AddRange(entry.Authors);
            foreach (var part in entry.Sections)
            {
                var section = new Section
                {
                    Id = LibraryIds.NewId(),
                    Kind = SectionKind.Verse,
                    Label = part.Label,
                };
                section.Lines.AddRange(part.Lines.Select(l => new LyricLine { Text = l }));
                song.Sections.Add(section);
            }
            await library.SaveSongAsync(song);
            service.Items.Add(new ServiceItem
            {
                Id = LibraryIds.NewId(),
                Kind = ServiceItemKind.Song,
                ContentId = song.Id,
                Title = song.Title,
                SongKey = song.Key,
            });
        }
        await library.SaveServiceAsync(service);
        return service;
    }
}
