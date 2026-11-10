using System.Collections.ObjectModel;
using Avalonia.Media.Imaging;
using Avalonia.Threading;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Selah.Contracts.V1;
using Selah.UI.Services;

namespace Selah.UI.ViewModels;

/// <summary>Slide preview card shared by the library song preview, scripture preview, and song editor panes.</summary>
public sealed partial class SlidePreviewCard : ObservableObject
{
    public required int Number { get; init; }

    public required string Header { get; init; }

    public required string ColorHex { get; init; }

    public required string Body { get; init; }

    public string ChordsLine { get; init; } = string.Empty;

    public string Footer { get; init; } = string.Empty;

    public Scene? PreviewScene { get; init; }

    public bool HasChords => !string.IsNullOrWhiteSpace(ChordsLine);

    public bool HasFooter => !string.IsNullOrWhiteSpace(Footer);

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasThumbnail))]
    private Bitmap? thumbnail;

    [ObservableProperty]
    private bool hasFlashWarning;

    public bool HasThumbnail => Thumbnail is not null;

    public static Scene BuildSlideScene(
        string body,
        string footer = "",
        string referenceText = "",
        string titleText = "",
        string chordsJson = "",
        string translationText = "",
        uint width = 1920,
        uint height = 1080,
        string styleJson = "")
    {
        var res = new Resolution { Width = width, Height = height };
        var output = new OutputScene
        {
            OutputIndex = 0,
            Role = OutputRole.Main,
            Resolution = res,
        };
        output.Layers.Add(new LayerSpec
        {
            Kind = LayerKind.Media,
            Enabled = true,
            Media = new MediaLayerSpec
            {
                SolidColor = new Color { Red = 0.06f, Green = 0.09f, Blue = 0.14f, Alpha = 1f },
            },
        });
        output.Layers.Add(new LayerSpec
        {
            Kind = LayerKind.Slide,
            Enabled = true,
            Slide = new SlideLayerSpec
            {
                Text = body,
                FooterText = footer,
                ReferenceText = referenceText,
                TitleText = titleText,
                ChordsJson = chordsJson,
                TranslationText = translationText,
                StyleJson = styleJson,
            },
        });
        return new Scene
        {
            Resolution = res,
            Outputs = { output },
        };
    }
}

/// <summary>Media card in the Library ▸ Media bin, including decoded PNG thumbnail preview.</summary>
public sealed partial class MediaCardViewModel : ObservableObject
{
    public MediaCardViewModel(MediaItem item, Bitmap? thumbnail)
    {
        Item = item;
        this.thumbnail = thumbnail;
    }

    public MediaItem Item { get; }

    public Id Id => Item.Id;

    public string Name => Item.Name;

    public MediaKind Kind => Item.Kind;

    public string KindLabel => Item.Kind switch
    {
        MediaKind.Image => Localizer.Instance["media-kind-image"],
        MediaKind.Video => Localizer.Instance["media-kind-video"],
        MediaKind.Audio => Localizer.Instance["media-kind-audio"],
        _ => Item.Extension,
    };

    public string MetaLabel
    {
        get
        {
            var resolution = Item.Resolution is { Width: > 0, Height: > 0 }
                ? $"{Item.Resolution.Width}×{Item.Resolution.Height}"
                : Item.Extension.TrimStart('.').ToUpperInvariant();
            var kb = Math.Max(1, (Item.SizeBytes + 1023) / 1024);
            return $"{KindLabel} · {resolution} · {kb} KB";
        }
    }

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasThumbnail))]
    private Bitmap? thumbnail;

    public bool HasThumbnail => Thumbnail is not null;

    public static Bitmap? DecodePng(byte[]? pngBytes)
    {
        if (pngBytes is null || pngBytes.Length == 0)
        {
            return null;
        }
        try
        {
            using var stream = new MemoryStream(pngBytes);
            return new Bitmap(stream);
        }
        catch (Exception)
        {
            return null;
        }
    }
}

/// <summary>
/// Library browser view model: Songs, Media bin, Bibles & Scripture lookup/search/splitting,
/// bundles, backups, and global undo/redo (M2.7, M2-A7).
/// </summary>
public sealed partial class LibraryViewModel : ObservableObject
{
    private readonly ILibraryService? library;
    private int songThumbGeneration;
    private int scriptureThumbGeneration;

    public LibraryViewModel(ILibraryService? library)
    {
        this.library = library;
        Localizer.Instance.PropertyChanged += (_, _) =>
        {
            OnPropertyChanged(nameof(SelectedSongSlidesLabel));
            OnPropertyChanged(nameof(HasNoSongs));
            OnPropertyChanged(nameof(HasNoMedia));
            OnPropertyChanged(nameof(HasNoVerses));
        };
        if (library is not null)
        {
            library.LibraryChanged += (_, _) => Dispatcher.UIThread.Post(() => _ = LoadAsync());
            library.UndoStateChanged += (_, state) => Dispatcher.UIThread.Post(() => ApplyUndoState(state.Global));
        }
    }

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsSongsTab))]
    [NotifyPropertyChangedFor(nameof(IsMediaTab))]
    [NotifyPropertyChangedFor(nameof(IsBiblesTab))]
    private LibraryTab activeTab = LibraryTab.Songs;

    public bool IsSongsTab => ActiveTab == LibraryTab.Songs;

    public bool IsMediaTab => ActiveTab == LibraryTab.Media;

    public bool IsBiblesTab => ActiveTab == LibraryTab.Bibles;

    [ObservableProperty]
    private bool canUndo;

    [ObservableProperty]
    private bool canRedo;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasStatusMessage))]
    private string? statusMessage;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasErrorMessage))]
    private string? errorMessage;

    [ObservableProperty]
    private string? errorFixArticleId;

    partial void OnErrorMessageChanged(string? value)
    {
        if (string.IsNullOrWhiteSpace(value))
        {
            ErrorFixArticleId = null;
        }
    }

    public bool HasStatusMessage => !string.IsNullOrWhiteSpace(StatusMessage);

    public bool HasErrorMessage => !string.IsNullOrWhiteSpace(ErrorMessage);

    // ----- Songs -----
    public ObservableCollection<Song> Songs { get; } = [];

    public ObservableCollection<SlidePreviewCard> SongPreviewSlides { get; } = [];

    [ObservableProperty]
    private string songSearchQuery = string.Empty;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasSelectedSong))]
    [NotifyPropertyChangedFor(nameof(SelectedSongAuthorText))]
    [NotifyPropertyChangedFor(nameof(SelectedSongSlidesLabel))]
    private Song? selectedSong;

    public bool HasSelectedSong => SelectedSong is not null;

    public bool HasNoSongs => Songs.Count == 0;

    public string SelectedSongAuthorText =>
        SelectedSong is null ? string.Empty : string.Join(", ", SelectedSong.Authors);

    public string SelectedSongSlidesLabel =>
        Localizer.Instance.Get("library-slides-count", ("count", SongPreviewSlides.Count));

    // ----- Media -----
    public ObservableCollection<MediaCardViewModel> MediaItems { get; } = [];

    [ObservableProperty]
    private string mediaSearchQuery = string.Empty;

    [ObservableProperty]
    private int mediaKindFilterIndex;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasSelectedMedia))]
    private MediaCardViewModel? selectedMedia;

    [ObservableProperty]
    private string importMediaPath = string.Empty;

    public bool HasSelectedMedia => SelectedMedia is not null;

    public bool HasNoMedia => MediaItems.Count == 0;

    // ----- Bibles & Scripture -----
    public ObservableCollection<BibleTranslation> Bibles { get; } = [];

    public ObservableCollection<ScriptureVerse> Verses { get; } = [];

    public ObservableCollection<ScriptureVerseHit> SearchHits { get; } = [];

    public ObservableCollection<SlidePreviewCard> ScripturePreviewSlides { get; } = [];

    public bool HasNoVerses => Verses.Count == 0;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(CanDeleteSelectedBible))]
    private BibleTranslation? selectedBible;

    public bool CanDeleteSelectedBible => SelectedBible is { IsBundled: false };

    [ObservableProperty]
    private string scriptureReferenceQuery = "John 3:16-18";

    [ObservableProperty]
    private string scriptureSearchQuery = string.Empty;

    [ObservableProperty]
    private int scriptureMaxLines = 4;

    [ObservableProperty]
    private bool scriptureShowVerseNumbers = true;

    [ObservableProperty]
    private int scriptureReferencePlacementIndex = 2; // 0: None, 1: Header, 2: Footer

    [ObservableProperty]
    private bool scriptureTitleSlide;

    [ObservableProperty]
    private bool scriptureBlankSlideAtEnd;

    [ObservableProperty]
    private string scriptureNormalizedReference = string.Empty;

    [ObservableProperty]
    private string importBiblePath = string.Empty;

    [ObservableProperty]
    private string importBibleCode = string.Empty;

    [ObservableProperty]
    private string importBibleName = string.Empty;

    [ObservableProperty]
    private int importBibleFormatIndex;

    // ----- Bundles & Backups -----
    [ObservableProperty]
    private string bundlePath = string.Empty;

    [ObservableProperty]
    private string backupPath = string.Empty;

    public event EventHandler<Song?>? EditSongRequested;

    public event EventHandler? SceneStaged;

    [RelayCommand]
    public void SelectTab(string tabName)
    {
        ActiveTab = tabName switch
        {
            "Media" => LibraryTab.Media,
            "Bibles" => LibraryTab.Bibles,
            _ => LibraryTab.Songs,
        };
    }

    partial void OnSelectedSongChanged(Song? value)
    {
        if (value is not null)
        {
            _ = RefreshSelectedSongSlidesAsync(value);
        }
        else
        {
            SongPreviewSlides.Clear();
            OnPropertyChanged(nameof(SelectedSongSlidesLabel));
        }
    }

    partial void OnMediaKindFilterIndexChanged(int value) => _ = SearchMediaAsync();

    partial void OnScriptureMaxLinesChanged(int value) => _ = SplitScriptureSlidesAsync();

    partial void OnScriptureShowVerseNumbersChanged(bool value) => _ = SplitScriptureSlidesAsync();

    partial void OnScriptureReferencePlacementIndexChanged(int value) => _ = SplitScriptureSlidesAsync();

    partial void OnScriptureTitleSlideChanged(bool value) => _ = SplitScriptureSlidesAsync();

    partial void OnScriptureBlankSlideAtEndChanged(bool value) => _ = SplitScriptureSlidesAsync();

    [RelayCommand]
    public async Task LoadAsync()
    {
        if (library is null)
        {
            return;
        }
        ClearError();
        await Task.WhenAll(
            SearchSongsAsync(),
            SearchMediaAsync(),
            LoadBiblesAsync(),
            RefreshUndoStateAsync());
    }

    public async Task RefreshUndoStateAsync()
    {
        if (library is null)
        {
            return;
        }
        try
        {
            var state = await library.GetUndoStateAsync();
            ApplyUndoState(state);
        }
        catch (Exception)
        {
        }
    }

    private void ApplyUndoState(UndoState? state)
    {
        if (state is null)
        {
            return;
        }
        CanUndo = state.CanUndo;
        CanRedo = state.CanRedo;
    }

    [RelayCommand]
    public async Task UndoAsync()
    {
        if (library is null)
        {
            return;
        }
        await RunSafeAsync(async () =>
        {
            await library.UndoAsync();
            await LoadAsync();
        });
    }

    [RelayCommand]
    public async Task RedoAsync()
    {
        if (library is null)
        {
            return;
        }
        await RunSafeAsync(async () =>
        {
            await library.RedoAsync();
            await LoadAsync();
        });
    }

    [RelayCommand]
    public async Task SearchSongsAsync()
    {
        if (library is null)
        {
            return;
        }
        await RunSafeAsync(async () =>
        {
            var currentId = SelectedSong?.Id;
            IReadOnlyList<Song> loaded;
            if (string.IsNullOrWhiteSpace(SongSearchQuery))
            {
                loaded = await library.ListSongsAsync();
            }
            else
            {
                var found = await library.SearchSongsAsync(SongSearchQuery.Trim());
                loaded = found.Hits.Select(h => h.Song).ToList();
            }
            Songs.Clear();
            foreach (var song in loaded)
            {
                Songs.Add(song);
            }
            OnPropertyChanged(nameof(HasNoSongs));
            SelectedSong = currentId is not null
                ? Songs.FirstOrDefault(s => LibraryIds.Same(s.Id, currentId)) ?? Songs.FirstOrDefault()
                : Songs.FirstOrDefault();
        });
    }

    private async Task RefreshSelectedSongSlidesAsync(Song song)
    {
        if (library is null)
        {
            return;
        }
        var generation = Interlocked.Increment(ref songThumbGeneration);
        try
        {
            var generated = await library.SplitSongSlidesAsync(new SplitSongSlides
            {
                Song = song,
                MaxLines = 4,
                CopyrightPlacement = CopyrightPlacement.FirstAndLast,
            });
            if (Volatile.Read(ref songThumbGeneration) != generation)
            {
                return;
            }
            SongPreviewSlides.Clear();
            var cards = new List<SlidePreviewCard>(generated.Slides.Count);
            var idx = 1;
            foreach (var slide in generated.Slides)
            {
                var body = string.Join("\n", slide.Lines.Select(l => l.Text));
                var titleText = slide.IsTitle ? slide.SectionLabel : string.Empty;
                var card = new SlidePreviewCard
                {
                    Number = idx++,
                    Header = slide.SectionLabel,
                    ColorHex = LibraryIds.SectionColorHex(slide.SectionKind),
                    Body = body,
                    Footer = slide.FooterCopyright,
                    PreviewScene = SlidePreviewCard.BuildSlideScene(
                        body,
                        footer: slide.FooterCopyright,
                        titleText: titleText),
                };
                SongPreviewSlides.Add(card);
                cards.Add(card);
            }
            OnPropertyChanged(nameof(SelectedSongSlidesLabel));
            await PopulateThumbnailsAsync(cards, 320, 180, () => Volatile.Read(ref songThumbGeneration) == generation);
        }
        catch (Exception)
        {
        }
    }

    internal async Task PopulateThumbnailsAsync(
        IReadOnlyList<SlidePreviewCard> cards,
        uint maxWidth,
        uint maxHeight,
        Func<bool> isCurrent)
    {
        if (library is null)
        {
            return;
        }
        foreach (var card in cards)
        {
            if (!isCurrent())
            {
                return;
            }
            if (card.PreviewScene is not { } scene)
            {
                continue;
            }
            try
            {
                var reply = await library.RenderSceneThumbnailAsync(scene, 0, maxWidth, maxHeight);
                if (!isCurrent())
                {
                    return;
                }
                card.Thumbnail = MediaCardViewModel.DecodePng(reply.PngBytes.ToByteArray());
                card.HasFlashWarning = reply.FlashWarning;
            }
            catch (Exception)
            {
            }
        }
    }

    [RelayCommand]
    public async Task StageSlideAsync(SlidePreviewCard? slide)
    {
        if (library is null || slide?.PreviewScene is not { } scene)
        {
            return;
        }
        await RunSafeAsync(async () =>
        {
            await library.StagePreviewSceneAsync(scene);
            StatusMessage = Localizer.Instance["editor-staged-status"];
            SceneStaged?.Invoke(this, EventArgs.Empty);
        });
    }

    [RelayCommand]
    private void NewSong() => EditSongRequested?.Invoke(this, null);

    [RelayCommand]
    private void EditSelectedSong()
    {
        if (SelectedSong is { } song)
        {
            EditSongRequested?.Invoke(this, song);
        }
    }

    [RelayCommand]
    public async Task DeleteSelectedSongAsync()
    {
        if (library is null || SelectedSong?.Id is not { } id)
        {
            return;
        }
        await RunSafeAsync(async () =>
        {
            await library.DeleteSongAsync(id);
            SelectedSong = null;
            await SearchSongsAsync();
            await RefreshUndoStateAsync();
        });
    }

    public async Task SearchMediaAsync()
    {
        if (library is null)
        {
            return;
        }
        await RunSafeAsync(async () =>
        {
            var currentId = SelectedMedia?.Id;
            var filterKind = MediaKindFilterIndex switch
            {
                1 => MediaKind.Image,
                2 => MediaKind.Video,
                3 => MediaKind.Audio,
                _ => MediaKind.Unspecified,
            };
            IReadOnlyList<MediaItem> loaded;
            if (string.IsNullOrWhiteSpace(MediaSearchQuery) && filterKind == MediaKind.Unspecified)
            {
                loaded = await library.ListMediaItemsAsync();
            }
            else
            {
                var found = await library.SearchMediaAsync(MediaSearchQuery.Trim(), filterKind);
                loaded = found.Hits.Select(h => h.MediaItem).ToList();
            }

            MediaItems.Clear();
            foreach (var item in loaded)
            {
                Bitmap? thumb = null;
                if (item.Kind != MediaKind.Audio)
                {
                    try
                    {
                        var reply = await library.GetMediaThumbnailAsync(item.Id, 320, 180);
                        thumb = MediaCardViewModel.DecodePng(reply.PngBytes.ToByteArray());
                    }
                    catch (Exception)
                    {
                    }
                }
                MediaItems.Add(new MediaCardViewModel(item, thumb));
            }
            OnPropertyChanged(nameof(HasNoMedia));
            SelectedMedia = currentId is not null
                ? MediaItems.FirstOrDefault(m => LibraryIds.Same(m.Id, currentId)) ?? MediaItems.FirstOrDefault()
                : MediaItems.FirstOrDefault();
        });
    }

    [RelayCommand]
    public async Task RunMediaSearchAsync() => await SearchMediaAsync();

    [RelayCommand]
    public async Task ImportMediaAsync()
    {
        if (string.IsNullOrWhiteSpace(ImportMediaPath))
        {
            return;
        }
        await ImportMediaFileAsync(ImportMediaPath.Trim());
    }

    public async Task ImportMediaFileAsync(string sourcePath, string name = "")
    {
        if (library is null)
        {
            return;
        }
        await RunSafeAsync(async () =>
        {
            var imported = await library.ImportMediaAsync(sourcePath, name);
            ImportMediaPath = string.Empty;
            await SearchMediaAsync();
            await RefreshUndoStateAsync();
            SelectedMedia = MediaItems.FirstOrDefault(m => LibraryIds.Same(m.Id, imported.Id)) ?? SelectedMedia;
        });
    }

    [RelayCommand]
    public async Task DeleteSelectedMediaAsync()
    {
        if (library is null || SelectedMedia is not { } media)
        {
            return;
        }
        await RunSafeAsync(async () =>
        {
            await library.DeleteMediaItemAsync(media.Id);
            SelectedMedia = null;
            await SearchMediaAsync();
            await RefreshUndoStateAsync();
        });
    }

    public async Task LoadBiblesAsync()
    {
        if (library is null)
        {
            return;
        }
        await RunSafeAsync(async () =>
        {
            var selectedCode = SelectedBible?.Code;
            var list = await library.ListBiblesAsync(includeBooks: false);
            Bibles.Clear();
            foreach (var bible in list)
            {
                Bibles.Add(bible);
            }
            SelectedBible = Bibles.FirstOrDefault(b => b.Code == selectedCode)
                ?? Bibles.FirstOrDefault(b => string.Equals(b.Code, "KJV", StringComparison.OrdinalIgnoreCase))
                ?? Bibles.FirstOrDefault();
        });
    }

    [RelayCommand]
    public async Task LookupPassageAsync()
    {
        if (library is null || string.IsNullOrWhiteSpace(ScriptureReferenceQuery))
        {
            return;
        }
        await RunSafeAsync(async () =>
        {
            var translation = SelectedBible?.Code ?? string.Empty;
            var result = await library.GetScripturePassagesAsync(translation, ScriptureReferenceQuery.Trim());
            ScriptureNormalizedReference = result.NormalizedReference;
            Verses.Clear();
            foreach (var verse in result.Verses)
            {
                Verses.Add(verse);
            }
            OnPropertyChanged(nameof(HasNoVerses));
            SearchHits.Clear();
            await SplitScriptureSlidesAsync();
        });
    }

    [RelayCommand]
    public async Task SearchScriptureAsync()
    {
        if (library is null || string.IsNullOrWhiteSpace(ScriptureSearchQuery))
        {
            return;
        }
        await RunSafeAsync(async () =>
        {
            var translation = SelectedBible?.Code ?? string.Empty;
            var found = await library.SearchScriptureAsync(ScriptureSearchQuery.Trim(), translation, 30);
            SearchHits.Clear();
            Verses.Clear();
            foreach (var hit in found.Hits)
            {
                SearchHits.Add(hit);
                Verses.Add(new ScriptureVerse
                {
                    Translation = hit.Translation,
                    BookCode = hit.Book,
                    BookName = hit.BookName,
                    Chapter = hit.Chapter,
                    Verse = hit.Verse,
                    Text = hit.Text,
                });
            }
            OnPropertyChanged(nameof(HasNoVerses));
            ScriptureNormalizedReference = ScriptureSearchQuery.Trim();
            await SplitScriptureSlidesAsync();
        });
    }

    public async Task OpenVerseHitAsync(ScriptureVerseHit hit)
    {
        ActiveTab = LibraryTab.Bibles;
        SelectedBible = Bibles.FirstOrDefault(b => string.Equals(b.Code, hit.Translation, StringComparison.OrdinalIgnoreCase))
            ?? SelectedBible;
        ScriptureReferenceQuery = $"{hit.BookName} {hit.Chapter}:{hit.Verse}";
        await LookupPassageAsync();
    }

    public async Task SplitScriptureSlidesAsync()
    {
        if (library is null || Verses.Count == 0)
        {
            ScripturePreviewSlides.Clear();
            return;
        }
        var generation = Interlocked.Increment(ref scriptureThumbGeneration);
        await RunSafeAsync(async () =>
        {
            var placement = ScriptureReferencePlacementIndex switch
            {
                0 => ReferencePlacement.None,
                1 => ReferencePlacement.SlideHeader,
                _ => ReferencePlacement.SlideFooter,
            };
            var request = new SplitScriptureSlides
            {
                Translation = SelectedBible?.Code ?? Verses[0].Translation,
                TranslationName = SelectedBible?.Name ?? Verses[0].Translation,
                Reference = string.IsNullOrWhiteSpace(ScriptureNormalizedReference)
                    ? ScriptureReferenceQuery
                    : ScriptureNormalizedReference,
                MaxLines = (uint)Math.Clamp(ScriptureMaxLines, 1, 20),
                ShowVerseNumbers = ScriptureShowVerseNumbers,
                ReferencePlacement = placement,
                TitleSlide = ScriptureTitleSlide,
                BlankSlideAtEnd = ScriptureBlankSlideAtEnd,
            };
            request.Verses.AddRange(Verses);
            var generated = await library.SplitScriptureSlidesAsync(request);
            if (Volatile.Read(ref scriptureThumbGeneration) != generation)
            {
                return;
            }
            ScripturePreviewSlides.Clear();
            var cards = new List<SlidePreviewCard>(generated.Slides.Count);
            var idx = 1;
            foreach (var slide in generated.Slides)
            {
                var header = !string.IsNullOrWhiteSpace(slide.HeaderReference)
                    ? slide.HeaderReference
                    : slide.Reference;
                var body = string.Join("\n", slide.Lines);
                var card = new SlidePreviewCard
                {
                    Number = idx++,
                    Header = header,
                    ColorHex = "#005EA2",
                    Body = body,
                    Footer = slide.FooterReference,
                    PreviewScene = SlidePreviewCard.BuildSlideScene(
                        slide.IsTitle ? string.Empty : body,
                        footer: slide.FooterReference,
                        referenceText: slide.HeaderReference,
                        titleText: slide.IsTitle ? header : string.Empty),
                };
                ScripturePreviewSlides.Add(card);
                cards.Add(card);
            }
            await PopulateThumbnailsAsync(cards, 320, 180, () => Volatile.Read(ref scriptureThumbGeneration) == generation);
        });
    }

    [RelayCommand]
    public async Task ImportBibleAsync()
    {
        if (library is null || string.IsNullOrWhiteSpace(ImportBiblePath))
        {
            return;
        }
        await RunSafeAsync(async () =>
        {
            var format = ImportBibleFormatIndex switch
            {
                1 => BibleFormat.Osis,
                2 => BibleFormat.Usfm,
                3 => BibleFormat.Zefania,
                _ => BibleFormat.Unspecified,
            };
            var imported = await library.ImportBibleAsync(new ImportBible
            {
                SourcePath = ImportBiblePath.Trim(),
                Format = format,
                Code = ImportBibleCode.Trim(),
                Name = ImportBibleName.Trim(),
                Language = "en",
                ReplaceExisting = true,
            });
            ImportBiblePath = string.Empty;
            ImportBibleCode = string.Empty;
            ImportBibleName = string.Empty;
            await LoadBiblesAsync();
            SelectedBible = Bibles.FirstOrDefault(b => b.Code == imported.Code) ?? SelectedBible;
        });
    }

    [RelayCommand]
    public async Task DeleteSelectedBibleAsync()
    {
        if (library is null || SelectedBible is not { IsBundled: false } bible)
        {
            return;
        }
        await RunSafeAsync(async () =>
        {
            await library.DeleteBibleAsync(bible.Code);
            await LoadBiblesAsync();
        });
    }

    [RelayCommand]
    public async Task ExportBundleAsync()
    {
        if (library is null || string.IsNullOrWhiteSpace(BundlePath))
        {
            return;
        }
        await RunSafeAsync(async () =>
        {
            var request = new ExportSelahBundle { OutputPath = BundlePath.Trim(), IncludeMedia = true };
            var exported = await library.ExportSelahBundleAsync(request);
            StatusMessage = Localizer.Instance.Get(
                "library-exported-status",
                ("songs", (int)exported.SongCount),
                ("media", (int)exported.MediaCount));
        });
    }

    [RelayCommand]
    public async Task ImportBundleAsync()
    {
        if (library is null || string.IsNullOrWhiteSpace(BundlePath))
        {
            return;
        }
        await RunSafeAsync(async () =>
        {
            var imported = await library.ImportSelahBundleAsync(new ImportSelahBundle
            {
                SourcePath = BundlePath.Trim(),
                OverwriteExisting = true,
            });
            StatusMessage = Localizer.Instance.Get(
                "library-imported-status",
                ("songs", (int)imported.SongCount),
                ("media", (int)imported.MediaCount));
            await LoadAsync();
        });
    }

    [RelayCommand]
    public async Task CreateBackupAsync()
    {
        if (library is null)
        {
            return;
        }
        await RunSafeAsync(async () =>
        {
            var info = await library.CreateLibraryBackupAsync(BackupPath.Trim());
            BackupPath = info.BackupPath;
            StatusMessage = Localizer.Instance.Get("library-backup-created", ("path", info.BackupPath));
        });
    }

    [RelayCommand]
    public async Task RestoreBackupAsync()
    {
        if (library is null || string.IsNullOrWhiteSpace(BackupPath))
        {
            return;
        }
        await RunSafeAsync(async () =>
        {
            var result = await library.RestoreLibraryBackupAsync(BackupPath.Trim());
            StatusMessage = Localizer.Instance.Get(
                "library-backup-restored",
                ("songs", (int)result.SongCount),
                ("bibles", (int)result.BibleCount));
            await LoadAsync();
        });
    }

    private void ClearError()
    {
        ErrorMessage = null;
        ErrorFixArticleId = null;
    }

    private async Task RunSafeAsync(Func<Task> action)
    {
        ClearError();
        try
        {
            await action();
        }
        catch (CoreException error)
        {
            ErrorMessage = Localizer.Instance[error.MessageKey];
            ErrorFixArticleId = string.IsNullOrWhiteSpace(error.FixArticleId) ? null : error.FixArticleId;
        }
        catch (Exception error) when (error is TimeoutException or IOException or InvalidOperationException)
        {
            ErrorMessage = Localizer.Instance["error-not-connected"];
            ErrorFixArticleId = null;
        }
    }
}
