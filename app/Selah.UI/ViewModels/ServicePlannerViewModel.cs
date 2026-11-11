using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Selah.Contracts.V1;
using Selah.UI.Services;

namespace Selah.UI.ViewModels;

/// <summary>One row in the Service Planner table (prototype screen 04).</summary>
public sealed partial class ServiceOrderRowViewModel : ObservableObject
{
    public ServiceOrderRowViewModel(ServiceItem item, int index, string formattedTime = "09:00", string formattedDuration = "4:00")
    {
        Item = item.Clone();
        Index = index;
        this.formattedTime = formattedTime;
        this.formattedDuration = formattedDuration;
    }

    public ServiceItem Item { get; }

    [ObservableProperty]
    private int index;

    [ObservableProperty]
    private string formattedTime;

    [ObservableProperty]
    private string formattedDuration;

    public Id Id => Item.Id ??= LibraryIds.NewId();

    public ServiceItemKind Kind
    {
        get => Item.Kind;
        set
        {
            if (Item.Kind != value)
            {
                Item.Kind = value;
                OnPropertyChanged();
                OnPropertyChanged(nameof(IsHeader));
                OnPropertyChanged(nameof(KindBadge));
            }
        }
    }

    public bool IsHeader => Item.Kind == ServiceItemKind.Header;

    public string KindBadge => Item.Kind switch
    {
        ServiceItemKind.Song => "SONG",
        ServiceItemKind.Scripture => "BIBLE",
        ServiceItemKind.Media => "MEDIA",
        ServiceItemKind.Presentation => "SLIDES",
        ServiceItemKind.Note => "NOTE",
        ServiceItemKind.Countdown => "TIMER",
        ServiceItemKind.Header => "SECTION",
        ServiceItemKind.Audio => "AUDIO",
        _ => "ITEM",
    };

    public string Title
    {
        get => Item.Title;
        set
        {
            if (Item.Title != value)
            {
                Item.Title = value ?? string.Empty;
                OnPropertyChanged();
            }
        }
    }

    public string Subtitle
    {
        get => Item.Subtitle;
        set
        {
            if (Item.Subtitle != value)
            {
                Item.Subtitle = value ?? string.Empty;
                OnPropertyChanged();
                OnPropertyChanged(nameof(HasSubtitle));
            }
        }
    }

    public bool HasSubtitle => !string.IsNullOrWhiteSpace(Item.Subtitle);

    public string LookName
    {
        get => string.IsNullOrWhiteSpace(Item.LookName) ? "—" : Item.LookName;
        set
        {
            var clean = value == "—" ? string.Empty : (value ?? string.Empty);
            if (Item.LookName != clean)
            {
                Item.LookName = clean;
                OnPropertyChanged();
            }
        }
    }

    public string Owner
    {
        get => string.IsNullOrWhiteSpace(Item.Owner) ? "—" : Item.Owner;
        set
        {
            var clean = value == "—" ? string.Empty : (value ?? string.Empty);
            if (Item.Owner != clean)
            {
                Item.Owner = clean;
                OnPropertyChanged();
            }
        }
    }

    public uint DurationSeconds
    {
        get => Item.DurationSeconds;
        set
        {
            if (Item.DurationSeconds != value)
            {
                Item.DurationSeconds = value;
                OnPropertyChanged();
            }
        }
    }

    public string StatusTag
    {
        get => string.IsNullOrWhiteSpace(Item.StatusTag) ? (IsHeader ? string.Empty : "Ready") : Item.StatusTag;
        set
        {
            if (Item.StatusTag != value)
            {
                Item.StatusTag = value ?? string.Empty;
                OnPropertyChanged();
            }
        }
    }

    public string SongKey
    {
        get => Item.SongKey;
        set
        {
            if (Item.SongKey != value)
            {
                Item.SongKey = value ?? string.Empty;
                OnPropertyChanged();
            }
        }
    }

    public string ScriptureReference
    {
        get => Item.ScriptureReference;
        set
        {
            if (Item.ScriptureReference != value)
            {
                Item.ScriptureReference = value ?? string.Empty;
                OnPropertyChanged();
            }
        }
    }

    public string BibleTranslation
    {
        get => Item.BibleTranslation;
        set
        {
            if (Item.BibleTranslation != value)
            {
                Item.BibleTranslation = value ?? string.Empty;
                OnPropertyChanged();
            }
        }
    }

    public uint CountdownSeconds
    {
        get => Item.CountdownSeconds;
        set
        {
            if (Item.CountdownSeconds != value)
            {
                Item.CountdownSeconds = value;
                OnPropertyChanged();
            }
        }
    }

    public string Notes
    {
        get => Item.Notes;
        set
        {
            if (Item.Notes != value)
            {
                Item.Notes = value ?? string.Empty;
                OnPropertyChanged();
            }
        }
    }
}

/// <summary>M4.1 Service Planner view model (prototype 04-service-planner.html).</summary>
public sealed partial class ServicePlannerViewModel : ObservableObject
{
    private readonly ILibraryService? library;
    private bool suppressSelectionSync;

    public ServicePlannerViewModel(ILibraryService? library)
    {
        this.library = library;
        if (library is not null)
        {
            library.UndoStateChanged += (_, e) =>
            {
                CanUndo = e.Global?.CanUndo ?? false;
                CanRedo = e.Global?.CanRedo ?? false;
            };
        }
    }

    public ObservableCollection<Service> Services { get; } = [];

    public ObservableCollection<ServiceType> ServiceTypes { get; } = [];

    public ObservableCollection<Team> Teams { get; } = [];

    public ObservableCollection<ServiceOrderRowViewModel> Items { get; } = [];

    public ObservableCollection<TeamMember> TeamMembers { get; } = [];

    public event EventHandler<Service>? GoLiveRequested;

    public event EventHandler<Service>? RehearseRequested;

    [ObservableProperty]
    private Service? selectedService;

    [ObservableProperty]
    private ServiceType? selectedServiceType;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasSelectedItem))]
    [NotifyPropertyChangedFor(nameof(IsSongItemSelected))]
    [NotifyPropertyChangedFor(nameof(IsScriptureItemSelected))]
    [NotifyPropertyChangedFor(nameof(IsCountdownItemSelected))]
    private ServiceOrderRowViewModel? selectedItem;

    public bool HasSelectedItem => SelectedItem is not null;

    public bool IsSongItemSelected => SelectedItem?.Kind == ServiceItemKind.Song;

    public bool IsScriptureItemSelected => SelectedItem?.Kind == ServiceItemKind.Scripture;

    public bool IsCountdownItemSelected => SelectedItem?.Kind == ServiceItemKind.Countdown;

    [ObservableProperty]
    private string serviceName = "Sunday Nov 15 · 9:00 AM";

    [ObservableProperty]
    private string serviceSeries = "Romans: Unshakeable Hope (Wk 4)";

    [ObservableProperty]
    private string serviceLocation = "Sanctuary";

    [ObservableProperty]
    private string serviceNotes = string.Empty;

    [ObservableProperty]
    private string startTimeText = "09:00";

    [ObservableProperty]
    private string defaultLookName = "Worship";

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(RuntimeSummary))]
    private string projectedStartTime = "09:00";

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(RuntimeSummary))]
    private string projectedEndTime = "10:15";

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(RuntimeSummary))]
    private string projectedTotalDuration = "1 hr 15 min";

    public string RuntimeSummary => $"{ProjectedStartTime} – {ProjectedEndTime} ({ProjectedTotalDuration})";

    // Selected item inspector fields
    [ObservableProperty]
    private string selectedItemTitle = string.Empty;

    [ObservableProperty]
    private string selectedItemSubtitle = string.Empty;

    [ObservableProperty]
    private string selectedItemOwner = string.Empty;

    [ObservableProperty]
    private uint selectedItemDurationSeconds = 240;

    [ObservableProperty]
    private string selectedItemLookName = "Worship";

    [ObservableProperty]
    private string selectedItemSongKey = string.Empty;

    [ObservableProperty]
    private string selectedItemScriptureReference = string.Empty;

    [ObservableProperty]
    private string selectedItemBibleTranslation = "KJV";

    [ObservableProperty]
    private uint selectedItemCountdownSeconds = 300;

    [ObservableProperty]
    private string selectedItemNotes = string.Empty;

    [ObservableProperty]
    private string selectedItemStatusTag = "Ready";

    // Team roster form fields
    [ObservableProperty]
    private string newMemberName = string.Empty;

    [ObservableProperty]
    private string newMemberRole = "Slides (Simple mode)";

    [ObservableProperty]
    private bool newMemberSimpleLock = true;

    // Export & status feedback
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasStatusMessage))]
    private string? statusMessage;

    public bool HasStatusMessage => !string.IsNullOrWhiteSpace(StatusMessage);

    [ObservableProperty]
    private string? errorMessage;

    [ObservableProperty]
    private string? errorFixArticleId;

    [ObservableProperty]
    private string lastExportedHtml = string.Empty;

    [ObservableProperty]
    private byte[] lastExportedPdfBytes = [];

    [ObservableProperty]
    private uint lastExportedPageCount;

    [ObservableProperty]
    private bool canUndo;

    [ObservableProperty]
    private bool canRedo;

    partial void OnSelectedServiceChanged(Service? value)
    {
        if (suppressSelectionSync || value is null)
        {
            return;
        }
        ApplyServiceToEditor(value);
        _ = RefreshProjectionAsync();
    }

    partial void OnSelectedItemChanged(ServiceOrderRowViewModel? value)
    {
        if (value is null)
        {
            return;
        }
        suppressSelectionSync = true;
        SelectedItemTitle = value.Title;
        SelectedItemSubtitle = value.Subtitle;
        SelectedItemOwner = value.Item.Owner;
        SelectedItemDurationSeconds = value.DurationSeconds;
        SelectedItemLookName = value.Item.LookName;
        SelectedItemSongKey = value.SongKey;
        SelectedItemScriptureReference = value.ScriptureReference;
        SelectedItemBibleTranslation = string.IsNullOrWhiteSpace(value.BibleTranslation) ? "KJV" : value.BibleTranslation;
        SelectedItemCountdownSeconds = value.CountdownSeconds > 0 ? value.CountdownSeconds : 300;
        SelectedItemNotes = value.Notes;
        SelectedItemStatusTag = value.StatusTag;
        suppressSelectionSync = false;
    }

    partial void OnSelectedItemTitleChanged(string value)
    {
        if (!suppressSelectionSync && SelectedItem is not null)
        {
            SelectedItem.Title = value;
        }
    }

    partial void OnSelectedItemSubtitleChanged(string value)
    {
        if (!suppressSelectionSync && SelectedItem is not null)
        {
            SelectedItem.Subtitle = value;
        }
    }

    partial void OnSelectedItemOwnerChanged(string value)
    {
        if (!suppressSelectionSync && SelectedItem is not null)
        {
            SelectedItem.Owner = value;
        }
    }

    partial void OnSelectedItemDurationSecondsChanged(uint value)
    {
        if (!suppressSelectionSync && SelectedItem is not null)
        {
            SelectedItem.DurationSeconds = value;
            _ = RefreshProjectionAsync();
        }
    }

    partial void OnSelectedItemLookNameChanged(string value)
    {
        if (!suppressSelectionSync && SelectedItem is not null)
        {
            SelectedItem.LookName = value;
        }
    }

    partial void OnSelectedItemSongKeyChanged(string value)
    {
        if (!suppressSelectionSync && SelectedItem is not null)
        {
            SelectedItem.SongKey = value;
        }
    }

    partial void OnSelectedItemScriptureReferenceChanged(string value)
    {
        if (!suppressSelectionSync && SelectedItem is not null)
        {
            SelectedItem.ScriptureReference = value;
        }
    }

    partial void OnSelectedItemBibleTranslationChanged(string value)
    {
        if (!suppressSelectionSync && SelectedItem is not null)
        {
            SelectedItem.BibleTranslation = value;
        }
    }

    partial void OnSelectedItemCountdownSecondsChanged(uint value)
    {
        if (!suppressSelectionSync && SelectedItem is not null)
        {
            SelectedItem.CountdownSeconds = value;
            if (SelectedItem.Kind == ServiceItemKind.Countdown)
            {
                SelectedItem.DurationSeconds = value;
                _ = RefreshProjectionAsync();
            }
        }
    }

    partial void OnSelectedItemNotesChanged(string value)
    {
        if (!suppressSelectionSync && SelectedItem is not null)
        {
            SelectedItem.Notes = value;
        }
    }

    partial void OnSelectedItemStatusTagChanged(string value)
    {
        if (!suppressSelectionSync && SelectedItem is not null)
        {
            SelectedItem.StatusTag = value;
        }
    }

    partial void OnStartTimeTextChanged(string value)
    {
        if (!suppressSelectionSync)
        {
            _ = RefreshProjectionAsync();
        }
    }

    [RelayCommand]
    public async Task LoadAsync()
    {
        if (library is null)
        {
            return;
        }
        try
        {
            ErrorMessage = null;
            var types = await library.ListServiceTypesAsync();
            ServiceTypes.Clear();
            foreach (var st in types)
            {
                ServiceTypes.Add(st);
            }
            if (ServiceTypes.Count == 0)
            {
                await SeedDefaultServiceTypesAsync();
            }

            var teams = await library.ListTeamsAsync();
            Teams.Clear();
            foreach (var t in teams)
            {
                Teams.Add(t);
            }
            if (Teams.Count == 0)
            {
                await SeedDefaultTeamAsync();
            }

            var services = await library.ListServicesAsync();
            Services.Clear();
            foreach (var s in services)
            {
                Services.Add(s);
            }
            if (Services.Count == 0)
            {
                await SeedDefaultServiceAsync();
            }

            SelectedServiceType = ServiceTypes.FirstOrDefault();
            TeamMembers.Clear();
            if (Teams.FirstOrDefault() is { } primaryTeam)
            {
                foreach (var m in primaryTeam.Members)
                {
                    TeamMembers.Add(m.Clone());
                }
            }

            if (Services.FirstOrDefault() is { } first)
            {
                SelectedService = first;
            }
            var undo = await library.GetUndoStateAsync();
            CanUndo = undo.CanUndo;
            CanRedo = undo.CanRedo;
        }
        catch (CoreException ex)
        {
            ErrorMessage = Localizer.Instance[ex.MessageKey];
            ErrorFixArticleId = string.IsNullOrWhiteSpace(ex.FixArticleId) ? null : ex.FixArticleId;
        }
    }

    private async Task SeedDefaultServiceTypesAsync()
    {
        if (library is null)
        {
            return;
        }
        var sunday9 = new ServiceType
        {
            Id = LibraryIds.NewId(),
            Name = "Sunday 9:00 AM",
            DefaultStartTime = "09:00",
            DefaultDurationSeconds = 4500,
            DefaultLocation = "Sanctuary",
            DefaultLookName = "Worship",
        };
        var sunday11 = new ServiceType
        {
            Id = LibraryIds.NewId(),
            Name = "Sunday 11:00 AM",
            DefaultStartTime = "11:00",
            DefaultDurationSeconds = 4500,
            DefaultLocation = "Sanctuary",
            DefaultLookName = "Worship",
        };
        await library.SaveServiceTypeAsync(sunday9);
        await library.SaveServiceTypeAsync(sunday11);
        ServiceTypes.Add(sunday9);
        ServiceTypes.Add(sunday11);
    }

    private async Task SeedDefaultTeamAsync()
    {
        if (library is null)
        {
            return;
        }
        var team = new Team
        {
            Id = LibraryIds.NewId(),
            Name = "Sunday Worship & Tech",
            Members =
            {
                new TeamMember { Id = LibraryIds.NewId(), Name = "Jordan Lee", Initials = "JL", Role = "Worship Leader", Confirmed = true },
                new TeamMember { Id = LibraryIds.NewId(), Name = "Sam Patel", Initials = "SP", Role = "Slides (Simple mode)", Confirmed = true, ModeLock = "simple" },
                new TeamMember { Id = LibraryIds.NewId(), Name = "Pastor David", Initials = "PD", Role = "Preaching", Confirmed = true },
            },
        };
        await library.SaveTeamAsync(team);
        Teams.Add(team);
    }

    private async Task SeedDefaultServiceAsync()
    {
        if (library is null)
        {
            return;
        }
        var songs = await library.ListSongsAsync(10, 0);
        var firstSong = songs.FirstOrDefault();
        var secondSong = songs.Skip(1).FirstOrDefault() ?? firstSong;

        var service = new Service
        {
            Id = LibraryIds.NewId(),
            Name = "Sunday Nov 15 · 9:00 AM",
            StartsAt = new Timestamp { UnixMilliseconds = 9 * 3600 * 1000 },
            ServiceTypeId = ServiceTypes.FirstOrDefault()?.Id,
            Series = "Romans: Unshakeable Hope (Wk 4)",
            Location = "Sanctuary",
            PlannedDurationSeconds = 4500,
            DefaultLookName = "Worship",
            Items =
            {
                new ServiceItem
                {
                    Id = LibraryIds.NewId(),
                    Kind = ServiceItemKind.Countdown,
                    Title = "Pre-service Countdown",
                    Subtitle = "5:00 timer · Warm welcome loop",
                    Owner = "Auto",
                    DurationSeconds = 300,
                    CountdownSeconds = 300,
                    LookName = "Pre-service",
                    StatusTag = "Ready",
                },
                new ServiceItem
                {
                    Id = LibraryIds.NewId(),
                    Kind = ServiceItemKind.Header,
                    Title = "Praise & Worship",
                },
                new ServiceItem
                {
                    Id = LibraryIds.NewId(),
                    Kind = ServiceItemKind.Song,
                    ContentId = firstSong?.Id,
                    Title = firstSong?.Title ?? "Way Maker",
                    Subtitle = $"Key: {firstSong?.Key ?? "G"} · 4:30",
                    SongKey = string.IsNullOrWhiteSpace(firstSong?.Key) ? "G" : firstSong.Key,
                    Owner = "Jordan L.",
                    DurationSeconds = 270,
                    LookName = "Worship",
                    StatusTag = "Ready",
                },
                new ServiceItem
                {
                    Id = LibraryIds.NewId(),
                    Kind = ServiceItemKind.Song,
                    ContentId = secondSong?.Id,
                    Title = secondSong?.Title ?? "Great Are You Lord",
                    Subtitle = "Key: A · 5:00",
                    SongKey = "A",
                    Owner = "Jordan L.",
                    DurationSeconds = 300,
                    LookName = "Worship",
                    StatusTag = "Ready",
                },
                new ServiceItem
                {
                    Id = LibraryIds.NewId(),
                    Kind = ServiceItemKind.Header,
                    Title = "The Word",
                },
                new ServiceItem
                {
                    Id = LibraryIds.NewId(),
                    Kind = ServiceItemKind.Scripture,
                    Title = "Romans 8:28–39",
                    Subtitle = "KJV · 3 slides",
                    ScriptureReference = "Romans 8:28-39",
                    BibleTranslation = "KJV",
                    Owner = "Pastor David",
                    DurationSeconds = 180,
                    LookName = "Scripture",
                    StatusTag = "Ready",
                },
                new ServiceItem
                {
                    Id = LibraryIds.NewId(),
                    Kind = ServiceItemKind.Presentation,
                    Title = "Sermon: More Than Conquerors",
                    Subtitle = "14 slides · Notes attached",
                    Owner = "Pastor David",
                    DurationSeconds = 1920,
                    LookName = "Sermon",
                    StatusTag = "Ready",
                },
            },
        };
        if (Teams.FirstOrDefault()?.Id is { } tid)
        {
            service.TeamIds.Add(tid);
        }
        await library.SaveServiceAsync(service);
        Services.Add(service);
    }

    private void ApplyServiceToEditor(Service service)
    {
        suppressSelectionSync = true;
        ServiceName = service.Name;
        ServiceSeries = service.Series;
        ServiceLocation = string.IsNullOrWhiteSpace(service.Location) ? "Sanctuary" : service.Location;
        ServiceNotes = service.Notes;
        DefaultLookName = string.IsNullOrWhiteSpace(service.DefaultLookName) ? "Worship" : service.DefaultLookName;
        if (service.StartsAt is { UnixMilliseconds: > 0 } ts)
        {
            var secs = (int)((ts.UnixMilliseconds / 1000) % 86400);
            if (secs < 0)
            {
                secs += 86400;
            }
            StartTimeText = $"{secs / 3600:D2}:{(secs % 3600) / 60:D2}";
        }
        else
        {
            StartTimeText = "09:00";
        }

        Items.Clear();
        for (var i = 0; i < service.Items.Count; i++)
        {
            Items.Add(new ServiceOrderRowViewModel(service.Items[i], i));
        }
        SelectedItem = Items.FirstOrDefault(r => !r.IsHeader) ?? Items.FirstOrDefault();
        suppressSelectionSync = false;
    }

    public Service BuildCurrentServiceDocument()
    {
        var id = SelectedService?.Id ?? LibraryIds.NewId();
        var startsAtSeconds = ParseStartTimeSeconds(StartTimeText);
        var doc = new Service
        {
            Id = id,
            Name = string.IsNullOrWhiteSpace(ServiceName) ? "Sunday Morning" : ServiceName.Trim(),
            StartsAt = new Timestamp { UnixMilliseconds = startsAtSeconds * 1000L },
            ServiceTypeId = SelectedServiceType?.Id ?? SelectedService?.ServiceTypeId,
            Series = ServiceSeries?.Trim() ?? string.Empty,
            Location = ServiceLocation?.Trim() ?? "Sanctuary",
            Notes = ServiceNotes?.Trim() ?? string.Empty,
            SourceServiceId = SelectedService?.SourceServiceId,
            PlannedDurationSeconds = (uint)Items.Sum(i => i.IsHeader ? 0 : (int)i.DurationSeconds),
            DefaultLookName = DefaultLookName?.Trim() ?? "Worship",
        };
        if (Teams.FirstOrDefault()?.Id is { } tid)
        {
            doc.TeamIds.Add(tid);
        }
        foreach (var row in Items)
        {
            if (row.Item.Id is null || row.Item.Id.Value.IsEmpty)
            {
                row.Item.Id = LibraryIds.NewId();
            }
            doc.Items.Add(row.Item.Clone());
        }
        return doc;
    }

    private static int ParseStartTimeSeconds(string raw)
    {
        if (!string.IsNullOrWhiteSpace(raw))
        {
            var parts = raw.Trim().Split(':');
            if (parts.Length >= 2
                && int.TryParse(parts[0], out var h)
                && int.TryParse(parts[1].Length >= 2 ? parts[1][..2] : parts[1], out var m)
                && h is >= 0 and < 24
                && m is >= 0 and < 60)
            {
                return h * 3600 + m * 60;
            }
        }
        return 9 * 3600;
    }

    public async Task RefreshProjectionAsync()
    {
        var doc = BuildCurrentServiceDocument();
        if (library is not null)
        {
            try
            {
                var proj = await library.ProjectServiceRuntimeAsync(new ProjectServiceRuntime
                {
                    ServiceId = doc.Id,
                    Service = doc,
                });
                ProjectedStartTime = proj.FormattedStartTime;
                ProjectedEndTime = proj.FormattedEndTime;
                ProjectedTotalDuration = proj.FormattedTotalDuration;
                for (var i = 0; i < Math.Min(Items.Count, proj.Items.Count); i++)
                {
                    Items[i].Index = i;
                    Items[i].FormattedTime = proj.Items[i].FormattedTime;
                    Items[i].FormattedDuration = proj.Items[i].FormattedDuration;
                }
                return;
            }
            catch
            {
                // Fallback to local calculation if disconnected.
            }
        }

        var baseSecs = ParseStartTimeSeconds(StartTimeText);
        var offset = 0;
        for (var i = 0; i < Items.Count; i++)
        {
            var row = Items[i];
            row.Index = i;
            if (row.IsHeader)
            {
                row.FormattedTime = "—";
                row.FormattedDuration = "—";
            }
            else
            {
                var cur = (baseSecs + offset) % 86400;
                row.FormattedTime = $"{cur / 3600:D2}:{(cur % 3600) / 60:D2}";
                var dur = (int)row.DurationSeconds;
                row.FormattedDuration = $"{dur / 60}:{dur % 60:D2}";
                offset += dur;
            }
        }
        var endSecs = (baseSecs + offset) % 86400;
        ProjectedStartTime = $"{baseSecs / 3600:D2}:{(baseSecs % 3600) / 60:D2}";
        ProjectedEndTime = $"{endSecs / 3600:D2}:{(endSecs % 3600) / 60:D2}";
        var mins = (offset + 59) / 60;
        ProjectedTotalDuration = mins >= 60 ? $"{mins / 60} hr {mins % 60} min" : $"{mins} min";
    }

    [RelayCommand]
    public async Task SaveServiceAsync()
    {
        if (library is null)
        {
            return;
        }
        try
        {
            ErrorMessage = null;
            var doc = BuildCurrentServiceDocument();
            await library.SaveServiceAsync(doc);
            var idx = Services.ToList().FindIndex(s => LibraryIds.Same(s.Id, doc.Id));
            suppressSelectionSync = true;
            if (idx >= 0)
            {
                Services[idx] = doc;
            }
            else
            {
                Services.Add(doc);
            }
            SelectedService = doc;
            suppressSelectionSync = false;
            await RefreshProjectionAsync();
            StatusMessage = Localizer.Instance["services-saved-status"];
        }
        catch (CoreException ex)
        {
            ErrorMessage = Localizer.Instance[ex.MessageKey];
            ErrorFixArticleId = string.IsNullOrWhiteSpace(ex.FixArticleId) ? null : ex.FixArticleId;
        }
    }

    [RelayCommand]
    public async Task NewServiceAsync()
    {
        var fresh = new Service
        {
            Id = LibraryIds.NewId(),
            Name = "New Sunday Service",
            StartsAt = new Timestamp { UnixMilliseconds = 9 * 3600 * 1000 },
            ServiceTypeId = SelectedServiceType?.Id,
            Series = string.Empty,
            Location = "Sanctuary",
            DefaultLookName = "Worship",
            Items =
            {
                new ServiceItem
                {
                    Id = LibraryIds.NewId(),
                    Kind = ServiceItemKind.Countdown,
                    Title = "Pre-service Countdown",
                    Owner = "Auto",
                    DurationSeconds = 300,
                    CountdownSeconds = 300,
                    LookName = "Pre-service",
                    StatusTag = "Ready",
                },
                new ServiceItem
                {
                    Id = LibraryIds.NewId(),
                    Kind = ServiceItemKind.Header,
                    Title = "Worship",
                },
            },
        };
        if (library is not null)
        {
            await library.SaveServiceAsync(fresh);
        }
        Services.Add(fresh);
        SelectedService = fresh;
        StatusMessage = Localizer.Instance["services-created-status"];
    }

    [RelayCommand]
    public async Task InheritFromLastWeekAsync()
    {
        if (library is null)
        {
            return;
        }
        try
        {
            ErrorMessage = null;
            var inherited = await library.CreateServiceFromPreviousAsync(new CreateServiceFromPrevious
            {
                SourceServiceId = SelectedService?.Id,
                ServiceTypeId = SelectedServiceType?.Id,
                Name = $"{ServiceName} (Next Week)",
                Series = ServiceSeries,
                KeepSongs = true,
            });
            Services.Add(inherited);
            SelectedService = inherited;
            StatusMessage = Localizer.Instance["services-inherited-status"];
        }
        catch (CoreException ex)
        {
            ErrorMessage = Localizer.Instance[ex.MessageKey];
            ErrorFixArticleId = string.IsNullOrWhiteSpace(ex.FixArticleId) ? null : ex.FixArticleId;
        }
    }

    [RelayCommand]
    public async Task DeleteServiceAsync()
    {
        if (library is null || SelectedService?.Id is null)
        {
            return;
        }
        var toDelete = SelectedService;
        await library.DeleteServiceAsync(toDelete.Id);
        Services.Remove(toDelete);
        SelectedService = Services.FirstOrDefault();
    }

    private async Task AddItemInternalAsync(ServiceItem item)
    {
        var row = new ServiceOrderRowViewModel(item, Items.Count);
        Items.Add(row);
        SelectedItem = row;
        await RefreshProjectionAsync();
    }

    [RelayCommand]
    public Task AddSongItemAsync() => AddItemInternalAsync(new ServiceItem
    {
        Id = LibraryIds.NewId(),
        Kind = ServiceItemKind.Song,
        Title = "Amazing Grace",
        Subtitle = "Key: G · 4:00",
        SongKey = "G",
        Owner = "Worship Team",
        DurationSeconds = 240,
        LookName = "Worship",
        StatusTag = "Ready",
    });

    [RelayCommand]
    public Task AddScriptureItemAsync() => AddItemInternalAsync(new ServiceItem
    {
        Id = LibraryIds.NewId(),
        Kind = ServiceItemKind.Scripture,
        Title = "Psalm 23:1–6",
        Subtitle = "KJV",
        ScriptureReference = "Psalm 23:1-6",
        BibleTranslation = "KJV",
        Owner = "Reader",
        DurationSeconds = 180,
        LookName = "Scripture",
        StatusTag = "Ready",
    });

    [RelayCommand]
    public Task AddMediaItemAsync() => AddItemInternalAsync(new ServiceItem
    {
        Id = LibraryIds.NewId(),
        Kind = ServiceItemKind.Media,
        Title = "Announcements Video",
        Subtitle = "1080p60 MP4",
        Owner = "Booth",
        DurationSeconds = 120,
        LookName = "Video",
        StatusTag = "Ready",
    });

    [RelayCommand]
    public Task AddPresentationItemAsync() => AddItemInternalAsync(new ServiceItem
    {
        Id = LibraryIds.NewId(),
        Kind = ServiceItemKind.Presentation,
        Title = "Sermon Slides",
        Subtitle = "Custom slide deck",
        Owner = "Pastor",
        DurationSeconds = 1800,
        LookName = "Sermon",
        StatusTag = "Ready",
    });

    [RelayCommand]
    public Task AddCountdownItemAsync() => AddItemInternalAsync(new ServiceItem
    {
        Id = LibraryIds.NewId(),
        Kind = ServiceItemKind.Countdown,
        Title = "5-Minute Countdown",
        Subtitle = "5:00 timer",
        Owner = "Auto",
        DurationSeconds = 300,
        CountdownSeconds = 300,
        LookName = "Pre-service",
        StatusTag = "Ready",
    });

    [RelayCommand]
    public Task AddHeaderItemAsync() => AddItemInternalAsync(new ServiceItem
    {
        Id = LibraryIds.NewId(),
        Kind = ServiceItemKind.Header,
        Title = "New Section",
    });

    [RelayCommand]
    public Task AddNoteItemAsync() => AddItemInternalAsync(new ServiceItem
    {
        Id = LibraryIds.NewId(),
        Kind = ServiceItemKind.Note,
        Title = "Pastoral Prayer & Welcome",
        Notes = "Stage lights warm",
        Owner = "Pastor",
        DurationSeconds = 180,
        LookName = "Stage Only",
        StatusTag = "Ready",
    });

    [RelayCommand]
    public Task AddAudioItemAsync() => AddItemInternalAsync(new ServiceItem
    {
        Id = LibraryIds.NewId(),
        Kind = ServiceItemKind.Audio,
        Title = "Warm Pad in G",
        Subtitle = "Audio bed",
        Owner = "Auto",
        DurationSeconds = 240,
        LookName = "Worship",
        StatusTag = "Ready",
    });

    [RelayCommand]
    public async Task MoveItemUpAsync()
    {
        if (SelectedItem is null)
        {
            return;
        }
        var idx = Items.IndexOf(SelectedItem);
        if (idx > 0)
        {
            var item = SelectedItem;
            Items.Move(idx, idx - 1);
            SelectedItem = item;
            await RefreshProjectionAsync();
        }
    }

    [RelayCommand]
    public async Task MoveItemDownAsync()
    {
        if (SelectedItem is null)
        {
            return;
        }
        var idx = Items.IndexOf(SelectedItem);
        if (idx >= 0 && idx < Items.Count - 1)
        {
            var item = SelectedItem;
            Items.Move(idx, idx + 1);
            SelectedItem = item;
            await RefreshProjectionAsync();
        }
    }

    [RelayCommand]
    public async Task RemoveItemAsync()
    {
        if (SelectedItem is null)
        {
            return;
        }
        var idx = Items.IndexOf(SelectedItem);
        Items.Remove(SelectedItem);
        SelectedItem = Items.Count > 0 ? Items[Math.Clamp(idx, 0, Items.Count - 1)] : null;
        await RefreshProjectionAsync();
    }

    [RelayCommand]
    public async Task AddTeamMemberAsync()
    {
        if (string.IsNullOrWhiteSpace(NewMemberName))
        {
            return;
        }
        var trimmed = NewMemberName.Trim();
        var parts = trimmed.Split(' ', StringSplitOptions.RemoveEmptyEntries);
        var initials = parts.Length >= 2
            ? $"{char.ToUpperInvariant(parts[0][0])}{char.ToUpperInvariant(parts[^1][0])}"
            : trimmed[..Math.Min(2, trimmed.Length)].ToUpperInvariant();
        var member = new TeamMember
        {
            Id = LibraryIds.NewId(),
            Name = trimmed,
            Initials = initials,
            Role = string.IsNullOrWhiteSpace(NewMemberRole) ? "Volunteer" : NewMemberRole.Trim(),
            Confirmed = true,
            ModeLock = NewMemberSimpleLock ? "simple" : string.Empty,
        };
        TeamMembers.Add(member);
        NewMemberName = string.Empty;

        if (library is not null && Teams.FirstOrDefault() is { } team)
        {
            team.Members.Clear();
            foreach (var m in TeamMembers)
            {
                team.Members.Add(m.Clone());
            }
            await library.SaveTeamAsync(team);
        }
    }

    private async Task ExportPrintInternalAsync(ServicePrintKind kind)
    {
        if (library is null)
        {
            return;
        }
        await SaveServiceAsync();
        if (SelectedService?.Id is null)
        {
            return;
        }
        try
        {
            var exported = await library.ExportServicePrintAsync(new ExportServicePrint
            {
                ServiceId = SelectedService.Id,
                Kind = kind,
            });
            LastExportedHtml = exported.Html;
            LastExportedPdfBytes = exported.PdfBytes.ToByteArray();
            LastExportedPageCount = exported.PageCount;
            StatusMessage = Localizer.Instance["services-exported-status"];
        }
        catch (CoreException ex)
        {
            ErrorMessage = Localizer.Instance[ex.MessageKey];
            ErrorFixArticleId = string.IsNullOrWhiteSpace(ex.FixArticleId) ? null : ex.FixArticleId;
        }
    }

    [RelayCommand]
    public Task ExportServiceOrderAsync() => ExportPrintInternalAsync(ServicePrintKind.ServiceOrder);

    [RelayCommand]
    public Task ExportChordChartsAsync() => ExportPrintInternalAsync(ServicePrintKind.ChordCharts);

    [RelayCommand]
    public Task ExportPacketAsync() => ExportPrintInternalAsync(ServicePrintKind.Both);

    [RelayCommand]
    public async Task RehearseAsync()
    {
        await SaveServiceAsync();
        if (SelectedService is not null)
        {
            if (library is not null)
            {
                var firstItem = Items.FirstOrDefault(i => !i.IsHeader);
                var previewScene = new Scene
                {
                    Revision = 1,
                    IsPreview = true,
                    Outputs =
                    {
                        new OutputScene
                        {
                            OutputIndex = 0,
                            Layers =
                            {
                                new LayerSpec
                                {
                                    Kind = LayerKind.Slide,
                                    Enabled = true,
                                    Slide = new SlideLayerSpec { Text = firstItem?.Title ?? ServiceName },
                                },
                            },
                        },
                    },
                };
                await library.StagePreviewSceneAsync(previewScene);
            }
            StatusMessage = Localizer.Instance["services-rehearse-status"];
            RehearseRequested?.Invoke(this, SelectedService);
        }
    }

    [RelayCommand]
    public async Task GoLiveAsync()
    {
        await SaveServiceAsync();
        if (SelectedService is not null)
        {
            if (library is not null)
            {
                var firstItem = Items.FirstOrDefault(i => !i.IsHeader);
                var liveScene = new Scene
                {
                    Revision = 1,
                    Outputs =
                    {
                        new OutputScene
                        {
                            OutputIndex = 0,
                            Layers =
                            {
                                new LayerSpec
                                {
                                    Kind = LayerKind.Slide,
                                    Enabled = true,
                                    Slide = new SlideLayerSpec { Text = firstItem?.Title ?? ServiceName },
                                },
                            },
                        },
                    },
                };
                await library.PresentSceneAsync(liveScene);
            }
            GoLiveRequested?.Invoke(this, SelectedService);
        }
    }

    [RelayCommand]
    public async Task UndoAsync()
    {
        if (library is null)
        {
            return;
        }
        await library.UndoAsync(SelectedService?.Id);
        await LoadAsync();
    }

    [RelayCommand]
    public async Task RedoAsync()
    {
        if (library is null)
        {
            return;
        }
        await library.RedoAsync(SelectedService?.Id);
        await LoadAsync();
    }
}
