using System.Collections.ObjectModel;
using System.Globalization;
using System.Text;
using Avalonia.Threading;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Google.Protobuf;
using Selah.Contracts.V1;
using Selah.UI.Services;

namespace Selah.UI.ViewModels;

/// <summary>One editable section card in the Song Editor (verse, chorus, bridge, tag, etc.).</summary>
public sealed partial class SongSectionItemViewModel : ObservableObject
{
    private readonly Action onChanged;

    public SongSectionItemViewModel(Id id, string label, SectionKind kind, string hotkey, string lyricsWithChords, Action onChanged)
    {
        Id = id;
        this.label = label;
        this.kindIndex = KindToIndex(kind);
        this.hotkey = hotkey;
        this.lyricsWithChords = lyricsWithChords;
        this.onChanged = onChanged;
    }

    public Id Id { get; }

    [ObservableProperty]
    private string label;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(ColorHex))]
    private int kindIndex;

    [ObservableProperty]
    private string hotkey;

    [ObservableProperty]
    private string lyricsWithChords;

    [ObservableProperty]
    private bool isAutoDetected;

    public SectionKind Kind => IndexToKind(KindIndex);

    public string ColorHex => LibraryIds.SectionColorHex(Kind);

    partial void OnLabelChanged(string value) => onChanged();

    partial void OnKindIndexChanged(int value) => onChanged();

    partial void OnHotkeyChanged(string value) => onChanged();

    partial void OnLyricsWithChordsChanged(string value) => onChanged();

    public Section ToProto()
    {
        var section = new Section
        {
            Id = Id,
            Label = Label.Trim(),
            Kind = Kind,
            Color = LibraryIds.SectionColor(Kind),
            Hotkey = Hotkey.Trim(),
        };
        foreach (var rawLine in (LyricsWithChords ?? string.Empty).Replace("\r\n", "\n").Split('\n'))
        {
            section.Lines.Add(ParseInlineChordLine(rawLine));
        }
        return section;
    }

    public static SongSectionItemViewModel FromProto(Section section, Action onChanged, bool autoDetected = false)
    {
        var id = section.Id is { Value.IsEmpty: false } ? section.Id : LibraryIds.NewId();
        var text = string.Join("\n", section.Lines.Select(FormatInlineChordLine));
        return new SongSectionItemViewModel(
            id,
            section.Label,
            section.Kind == SectionKind.Unspecified ? SectionKind.Verse : section.Kind,
            section.Hotkey,
            text,
            onChanged)
        {
            IsAutoDetected = autoDetected,
        };
    }

    public static LyricLine ParseInlineChordLine(string rawLine)
    {
        var line = new LyricLine();
        var plain = new StringBuilder();
        var i = 0;
        var runeOffset = 0;
        while (i < rawLine.Length)
        {
            if (rawLine[i] == '[')
            {
                var close = rawLine.IndexOf(']', i + 1);
                if (close > i + 1)
                {
                    var chordText = rawLine.Substring(i + 1, close - i - 1).Trim();
                    if (!string.IsNullOrEmpty(chordText))
                    {
                        line.Chords.Add(new Chord
                        {
                            Position = (uint)runeOffset,
                            Text = chordText,
                        });
                        i = close + 1;
                        continue;
                    }
                }
            }

            if (Rune.DecodeFromUtf16(rawLine.AsSpan(i), out var rune, out var consumed) == System.Buffers.OperationStatus.Done)
            {
                plain.Append(rune.ToString());
                runeOffset++;
                i += consumed;
            }
            else
            {
                plain.Append(rawLine[i]);
                runeOffset++;
                i++;
            }
        }
        line.Text = plain.ToString();
        return line;
    }

    public static string FormatInlineChordLine(LyricLine line)
    {
        if (line.Chords.Count == 0)
        {
            return line.Text;
        }
        var chordsByRune = line.Chords
            .OrderBy(c => c.Position)
            .GroupBy(c => (int)c.Position)
            .ToDictionary(g => g.Key, g => g.Select(c => c.Text).ToList());
        var sb = new StringBuilder();
        var runeIndex = 0;
        foreach (var rune in line.Text.EnumerateRunes())
        {
            if (chordsByRune.TryGetValue(runeIndex, out var list))
            {
                foreach (var chord in list)
                {
                    sb.Append('[').Append(chord).Append(']');
                }
            }
            sb.Append(rune.ToString());
            runeIndex++;
        }
        foreach (var pair in chordsByRune.Where(kv => kv.Key >= runeIndex).OrderBy(kv => kv.Key))
        {
            foreach (var chord in pair.Value)
            {
                sb.Append('[').Append(chord).Append(']');
            }
        }
        return sb.ToString();
    }

    private static int KindToIndex(SectionKind kind) => kind switch
    {
        SectionKind.Chorus => 1,
        SectionKind.Bridge => 2,
        SectionKind.Tag => 3,
        SectionKind.Intro => 4,
        SectionKind.Outro => 5,
        SectionKind.Other => 6,
        _ => 0,
    };

    private static SectionKind IndexToKind(int index) => index switch
    {
        1 => SectionKind.Chorus,
        2 => SectionKind.Bridge,
        3 => SectionKind.Tag,
        4 => SectionKind.Intro,
        5 => SectionKind.Outro,
        6 => SectionKind.Other,
        _ => SectionKind.Verse,
    };
}

/// <summary>A section token inside an arrangement sequence.</summary>
public sealed class ArrangementTokenViewModel
{
    public required Id SectionId { get; init; }

    public required string Label { get; init; }

    public required string ColorHex { get; init; }
}

/// <summary>An arrangement preset (e.g., Sunday) holding an ordered sequence of sections.</summary>
public sealed partial class ArrangementItemViewModel : ObservableObject
{
    public ArrangementItemViewModel(Id id, string name)
    {
        Id = id;
        this.name = name;
    }

    public Id Id { get; }

    [ObservableProperty]
    private string name;

    public ObservableCollection<ArrangementTokenViewModel> Tokens { get; } = [];
}

/// <summary>Preview screen target matching prototype screen 03 (16:9, LED 32:9, Stage, Stream).</summary>
public enum SongPreviewTarget
{
    Main16x9 = 0,
    Led32x9 = 1,
    Stage = 2,
    Stream = 3,
}

/// <summary>
/// Full 4-step USWDS + Apple song editor view model (Paste, Sections, Arrange, Review)
/// with chords, slide splitting preview, transpose, arrangements, autosave buffer, and undo/redo (M2.7).
/// </summary>
public sealed partial class SongEditorViewModel : ObservableObject
{
    private readonly ILibraryService? library;
    private bool suppressCallbacks;

    public SongEditorViewModel(ILibraryService? library)
    {
        this.library = library;
        SongId = LibraryIds.NewId();
        Localizer.Instance.PropertyChanged += (_, _) =>
        {
            OnPropertyChanged(nameof(DetectedBannerTitle));
            OnPropertyChanged(nameof(SlidesSummaryText));
            OnPropertyChanged(nameof(ContinueButtonText));
        };
        if (library is not null)
        {
            library.UndoStateChanged += (_, state) =>
            {
                if (state.DocId is null || LibraryIds.Same(state.DocId, SongId))
                {
                    Dispatcher.UIThread.Post(() =>
                    {
                        var undo = state.Document ?? state.Global;
                        if (undo is not null)
                        {
                            CanUndo = undo.CanUndo;
                            CanRedo = undo.CanRedo;
                        }
                    });
                }
            };
        }
        StartNewSong();
    }

    public Id SongId { get; private set; }

    // ----- 4-step workflow -----
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsStepPaste))]
    [NotifyPropertyChangedFor(nameof(IsStepSections))]
    [NotifyPropertyChangedFor(nameof(IsStepArrange))]
    [NotifyPropertyChangedFor(nameof(IsStepReview))]
    [NotifyPropertyChangedFor(nameof(IsStepPastPaste))]
    [NotifyPropertyChangedFor(nameof(IsStepPastSections))]
    [NotifyPropertyChangedFor(nameof(IsStepPastArrange))]
    [NotifyPropertyChangedFor(nameof(ContinueButtonText))]
    [NotifyPropertyChangedFor(nameof(CanContinueStep))]
    private SongEditorStep currentStep = SongEditorStep.Paste;

    public bool IsStepPaste => CurrentStep == SongEditorStep.Paste;

    public bool IsStepSections => CurrentStep == SongEditorStep.Sections;

    public bool IsStepArrange => CurrentStep == SongEditorStep.Arrange;

    public bool IsStepReview => CurrentStep == SongEditorStep.Review;

    public bool IsStepPastPaste => (int)CurrentStep > (int)SongEditorStep.Paste;

    public bool IsStepPastSections => (int)CurrentStep > (int)SongEditorStep.Sections;

    public bool IsStepPastArrange => (int)CurrentStep > (int)SongEditorStep.Arrange;

    public bool CanContinueStep => CurrentStep != SongEditorStep.Review;

    public string ContinueButtonText => CurrentStep switch
    {
        SongEditorStep.Paste => Localizer.Instance["editor-continue-sections"],
        SongEditorStep.Sections => Localizer.Instance["editor-continue-arrange"],
        _ => Localizer.Instance["editor-continue-review"],
    };

    // ----- Inspector tabs -----
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsInspectorSong))]
    [NotifyPropertyChangedFor(nameof(IsInspectorLayout))]
    [NotifyPropertyChangedFor(nameof(IsInspectorArrange))]
    [NotifyPropertyChangedFor(nameof(IsInspectorActions))]
    private SongInspectorTab inspectorTab = SongInspectorTab.Song;

    public bool IsInspectorSong => InspectorTab == SongInspectorTab.Song;

    public bool IsInspectorLayout => InspectorTab == SongInspectorTab.Layout;

    public bool IsInspectorArrange => InspectorTab == SongInspectorTab.Arrange;

    public bool IsInspectorActions => InspectorTab == SongInspectorTab.Actions;

    // ----- Step 1: Paste -----
    [ObservableProperty]
    private string pasteRawText = string.Empty;

    [ObservableProperty]
    private int pasteFormatIndex; // 0: Auto, 1: Plain, 2: Markers, 3: ChordsOverLyrics, 4: ChordPro

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(DetectedBannerTitle))]
    private bool hasDetectionBanner;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(DetectedBannerTitle))]
    private int detectedSectionCount;

    public string DetectedBannerTitle =>
        Localizer.Instance.Get("editor-detected-title", ("count", DetectedSectionCount));

    // ----- Metadata -----
    [ObservableProperty]
    private string title = string.Empty;

    [ObservableProperty]
    private string authorsText = string.Empty;

    [ObservableProperty]
    private string songKey = string.Empty;

    [ObservableProperty]
    private string tempoText = string.Empty;

    [ObservableProperty]
    private string ccliNumber = string.Empty;

    [ObservableProperty]
    private string copyright = string.Empty;

    [ObservableProperty]
    private string tagsText = string.Empty;

    [ObservableProperty]
    private bool publicDomain;

    // ----- Layout / split options -----
    [ObservableProperty]
    private int maxLinesPerSlide = 4;

    [ObservableProperty]
    private int copyrightPlacementIndex = 3; // First and last slide

    [ObservableProperty]
    private bool titleSlide;

    [ObservableProperty]
    private bool blankSlideAtEnd;

    [ObservableProperty]
    private string translationLanguage = string.Empty;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsPreviewMain))]
    [NotifyPropertyChangedFor(nameof(IsPreviewLed))]
    [NotifyPropertyChangedFor(nameof(IsPreviewStage))]
    [NotifyPropertyChangedFor(nameof(IsPreviewStream))]
    [NotifyPropertyChangedFor(nameof(ShowChordsInSlides))]
    private SongPreviewTarget previewTarget = SongPreviewTarget.Main16x9;

    public bool IsPreviewMain => PreviewTarget == SongPreviewTarget.Main16x9;

    public bool IsPreviewLed => PreviewTarget == SongPreviewTarget.Led32x9;

    public bool IsPreviewStage => PreviewTarget == SongPreviewTarget.Stage;

    public bool IsPreviewStream => PreviewTarget == SongPreviewTarget.Stream;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(ShowChordsInSlides))]
    private bool showChords = true;

    public bool ShowChordsInSlides => ShowChords || IsPreviewStage;

    // ----- Sections, Arrangements, Generated slides -----
    public ObservableCollection<SongSectionItemViewModel> Sections { get; } = [];

    public ObservableCollection<ArrangementItemViewModel> Arrangements { get; } = [];

    public ObservableCollection<SlidePreviewCard> GeneratedSlides { get; } = [];

    [ObservableProperty]
    private SongSectionItemViewModel? selectedSection;

    [ObservableProperty]
    private ArrangementItemViewModel? selectedArrangement;

    [ObservableProperty]
    private bool preferFlats;

    [ObservableProperty]
    private bool canUndo;

    [ObservableProperty]
    private bool canRedo;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasDraftRestored))]
    private bool draftRestored;

    public bool HasDraftRestored => DraftRestored;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasStatusMessage))]
    private string? statusMessage;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasErrorMessage))]
    private string? errorMessage;

    [ObservableProperty]
    private string? errorFixArticleId;

    public bool HasStatusMessage => !string.IsNullOrWhiteSpace(StatusMessage);

    public bool HasErrorMessage => !string.IsNullOrWhiteSpace(ErrorMessage);

    public string SlidesSummaryText =>
        Localizer.Instance.Get("editor-slides-summary", ("count", GeneratedSlides.Count));

    public event EventHandler<Song>? SongSaved;

    partial void OnTitleChanged(string value) => OnModelMutated();

    partial void OnAuthorsTextChanged(string value) => OnModelMutated();

    partial void OnSongKeyChanged(string value) => OnModelMutated();

    partial void OnCopyrightChanged(string value) => OnModelMutated();

    partial void OnCcliNumberChanged(string value) => OnModelMutated();

    partial void OnPublicDomainChanged(bool value) => OnModelMutated();

    partial void OnMaxLinesPerSlideChanged(int value) => OnModelMutated();

    partial void OnCopyrightPlacementIndexChanged(int value) => OnModelMutated();

    partial void OnTitleSlideChanged(bool value) => OnModelMutated();

    partial void OnBlankSlideAtEndChanged(bool value) => OnModelMutated();

    partial void OnTranslationLanguageChanged(string value) => OnModelMutated();

    partial void OnShowChordsChanged(bool value) => _ = RefreshSlidesAsync();

    partial void OnPreviewTargetChanged(SongPreviewTarget value) => _ = RefreshSlidesAsync();

    partial void OnSelectedArrangementChanged(ArrangementItemViewModel? value) => _ = RefreshSlidesAsync();

    public void StartNewSong(bool startOnPaste = true)
    {
        suppressCallbacks = true;
        try
        {
            SongId = LibraryIds.NewId();
            CurrentStep = startOnPaste ? SongEditorStep.Paste : SongEditorStep.Sections;
            InspectorTab = SongInspectorTab.Song;
            HasDetectionBanner = false;
            DetectedSectionCount = 0;
            DraftRestored = false;
            StatusMessage = null;
            ErrorMessage = null;
            PasteRawText = string.Empty;
            Title = string.Empty;
            AuthorsText = string.Empty;
            SongKey = string.Empty;
            TempoText = string.Empty;
            CcliNumber = string.Empty;
            Copyright = string.Empty;
            TagsText = string.Empty;
            PublicDomain = false;
            Sections.Clear();
            Arrangements.Clear();
            GeneratedSlides.Clear();
            var verse = new SongSectionItemViewModel(LibraryIds.NewId(), "Verse 1", SectionKind.Verse, "1", string.Empty, OnModelMutated);
            Sections.Add(verse);
            SelectedSection = verse;
            var arr = new ArrangementItemViewModel(LibraryIds.NewId(), "Default");
            arr.Tokens.Add(new ArrangementTokenViewModel { SectionId = verse.Id, Label = verse.Label, ColorHex = verse.ColorHex });
            Arrangements.Add(arr);
            SelectedArrangement = arr;
        }
        finally
        {
            suppressCallbacks = false;
        }
        _ = RefreshSlidesAsync();
    }

    public async Task LoadSongAsync(Song song)
    {
        suppressCallbacks = true;
        try
        {
            ApplySongProto(song, autoDetected: false);
            CurrentStep = SongEditorStep.Sections;
            InspectorTab = SongInspectorTab.Song;
            HasDetectionBanner = false;
            DraftRestored = false;
            StatusMessage = null;
            ErrorMessage = null;
        }
        finally
        {
            suppressCallbacks = false;
        }

        if (library is not null)
        {
            try
            {
                var buffer = await library.GetEditorBufferAsync(SongId);
                if (buffer.Found && buffer.Buffer is { Content.Length: > 0 })
                {
                    var recovered = Song.Parser.ParseJson(buffer.Buffer.Content);
                    suppressCallbacks = true;
                    try
                    {
                        ApplySongProto(recovered, autoDetected: false);
                        DraftRestored = true;
                    }
                    finally
                    {
                        suppressCallbacks = false;
                    }
                }
                var undo = await library.GetUndoStateAsync(SongId);
                CanUndo = undo.CanUndo;
                CanRedo = undo.CanRedo;
            }
            catch (Exception)
            {
            }
        }
        await RefreshSlidesAsync();
    }

    [RelayCommand]
    public void SelectStep(string stepName)
    {
        CurrentStep = stepName switch
        {
            "Paste" => SongEditorStep.Paste,
            "Sections" => SongEditorStep.Sections,
            "Arrange" => SongEditorStep.Arrange,
            _ => SongEditorStep.Review,
        };
        if (CurrentStep == SongEditorStep.Arrange)
        {
            InspectorTab = SongInspectorTab.Arrange;
        }
    }

    [RelayCommand]
    private void ContinueStep()
    {
        CurrentStep = CurrentStep switch
        {
            SongEditorStep.Paste => SongEditorStep.Sections,
            SongEditorStep.Sections => SongEditorStep.Arrange,
            SongEditorStep.Arrange => SongEditorStep.Review,
            _ => SongEditorStep.Review,
        };
        if (CurrentStep == SongEditorStep.Arrange)
        {
            InspectorTab = SongInspectorTab.Arrange;
        }
    }

    [RelayCommand]
    public void SelectInspectorTab(string tabName)
    {
        InspectorTab = tabName switch
        {
            "Layout" => SongInspectorTab.Layout,
            "Arrange" => SongInspectorTab.Arrange,
            "Actions" => SongInspectorTab.Actions,
            _ => SongInspectorTab.Song,
        };
    }

    [RelayCommand]
    public void SelectPreviewTarget(string targetName)
    {
        PreviewTarget = targetName switch
        {
            "Led" => SongPreviewTarget.Led32x9,
            "Stage" => SongPreviewTarget.Stage,
            "Stream" => SongPreviewTarget.Stream,
            _ => SongPreviewTarget.Main16x9,
        };
    }

    [RelayCommand]
    public async Task DetectSectionsAsync()
    {
        if (library is null || string.IsNullOrWhiteSpace(PasteRawText))
        {
            return;
        }
        ErrorMessage = null;
        try
        {
            var format = PasteFormatIndex switch
            {
                1 => SongPasteFormat.Plain,
                2 => SongPasteFormat.Markers,
                3 => SongPasteFormat.ChordsOverLyrics,
                4 => SongPasteFormat.ChordPro,
                _ => SongPasteFormat.Unspecified,
            };
            var parsed = await library.ParsePastedSongAsync(PasteRawText, format, Title.Trim());
            if (parsed.Song is { } song)
            {
                var preservedId = SongId;
                suppressCallbacks = true;
                try
                {
                    ApplySongProto(song, autoDetected: true);
                    SongId = preservedId;
                }
                finally
                {
                    suppressCallbacks = false;
                }
                DetectedSectionCount = Sections.Count;
                HasDetectionBanner = true;
                CurrentStep = SongEditorStep.Sections;
                await RefreshSlidesAsync();
                await AutosaveBufferAsync();
            }
        }
        catch (CoreException error)
        {
            ErrorMessage = Localizer.Instance[error.MessageKey];
            ErrorFixArticleId = string.IsNullOrWhiteSpace(error.FixArticleId) ? null : error.FixArticleId;
        }
        catch (Exception error) when (error is TimeoutException or IOException or InvalidOperationException)
        {
            ErrorMessage = Localizer.Instance["error-not-connected"];
        }
    }

    [RelayCommand]
    public void AddSection()
    {
        var number = Sections.Count(s => s.Kind == SectionKind.Verse) + 1;
        var section = new SongSectionItemViewModel(
            LibraryIds.NewId(),
            $"Verse {number}",
            SectionKind.Verse,
            number <= 9 ? number.ToString(CultureInfo.InvariantCulture) : string.Empty,
            string.Empty,
            OnModelMutated);
        Sections.Add(section);
        SelectedSection = section;
        SelectedArrangement?.Tokens.Add(new ArrangementTokenViewModel
        {
            SectionId = section.Id,
            Label = section.Label,
            ColorHex = section.ColorHex,
        });
        OnModelMutated();
    }

    [RelayCommand]
    public void RemoveSection(SongSectionItemViewModel? section)
    {
        section ??= SelectedSection;
        if (section is null || Sections.Count <= 1)
        {
            return;
        }
        Sections.Remove(section);
        foreach (var arr in Arrangements)
        {
            for (var i = arr.Tokens.Count - 1; i >= 0; i--)
            {
                if (LibraryIds.Same(arr.Tokens[i].SectionId, section.Id))
                {
                    arr.Tokens.RemoveAt(i);
                }
            }
        }
        SelectedSection = Sections.FirstOrDefault();
        OnModelMutated();
    }

    [RelayCommand]
    public void MoveSectionUp(SongSectionItemViewModel? section)
    {
        section ??= SelectedSection;
        if (section is null)
        {
            return;
        }
        var idx = Sections.IndexOf(section);
        if (idx > 0)
        {
            Sections.Move(idx, idx - 1);
            OnModelMutated();
        }
    }

    [RelayCommand]
    public void MoveSectionDown(SongSectionItemViewModel? section)
    {
        section ??= SelectedSection;
        if (section is null)
        {
            return;
        }
        var idx = Sections.IndexOf(section);
        if (idx >= 0 && idx < Sections.Count - 1)
        {
            Sections.Move(idx, idx + 1);
            OnModelMutated();
        }
    }

    [RelayCommand]
    public void AppendSectionToArrangement(SongSectionItemViewModel? section)
    {
        if (section is null)
        {
            return;
        }
        if (SelectedArrangement is null)
        {
            AddArrangement();
        }
        SelectedArrangement!.Tokens.Add(new ArrangementTokenViewModel
        {
            SectionId = section.Id,
            Label = section.Label,
            ColorHex = section.ColorHex,
        });
        OnModelMutated();
    }

    [RelayCommand]
    public void RemoveArrangementToken(ArrangementTokenViewModel? token)
    {
        if (token is null || SelectedArrangement is null)
        {
            return;
        }
        SelectedArrangement.Tokens.Remove(token);
        OnModelMutated();
    }

    [RelayCommand]
    public void ClearArrangementTokens()
    {
        if (SelectedArrangement is null)
        {
            return;
        }
        SelectedArrangement.Tokens.Clear();
        OnModelMutated();
    }

    [RelayCommand]
    public void AddArrangement()
    {
        var arr = new ArrangementItemViewModel(LibraryIds.NewId(), $"Arrangement {Arrangements.Count + 1}");
        Arrangements.Add(arr);
        SelectedArrangement = arr;
        OnModelMutated();
    }

    [RelayCommand]
    public void DeleteSelectedArrangement()
    {
        if (SelectedArrangement is null || Arrangements.Count <= 1)
        {
            return;
        }
        Arrangements.Remove(SelectedArrangement);
        SelectedArrangement = Arrangements.FirstOrDefault();
        OnModelMutated();
    }

    [RelayCommand]
    public Task TransposeUpAsync() => TransposeByAsync(1);

    [RelayCommand]
    public Task TransposeDownAsync() => TransposeByAsync(-1);

    public async Task TransposeByAsync(int semitones)
    {
        if (library is null)
        {
            return;
        }
        try
        {
            var spelling = PreferFlats ? AccidentalSpelling.Flats : AccidentalSpelling.Sharps;
            var updated = await library.TransposeSongAsync(BuildSongProto(), semitones, spelling);
            var preservedStep = CurrentStep;
            suppressCallbacks = true;
            try
            {
                ApplySongProto(updated, autoDetected: HasDetectionBanner);
                CurrentStep = preservedStep;
            }
            finally
            {
                suppressCallbacks = false;
            }
            await RefreshSlidesAsync();
            await AutosaveBufferAsync();
        }
        catch (CoreException error)
        {
            ErrorMessage = Localizer.Instance[error.MessageKey];
            ErrorFixArticleId = string.IsNullOrWhiteSpace(error.FixArticleId) ? null : error.FixArticleId;
        }
        catch (Exception error) when (error is TimeoutException or IOException or InvalidOperationException)
        {
            ErrorMessage = Localizer.Instance["error-not-connected"];
        }
    }

    [RelayCommand]
    public async Task SaveSongAsync()
    {
        if (library is null)
        {
            return;
        }
        ErrorMessage = null;
        try
        {
            var song = BuildSongProto();
            if (string.IsNullOrWhiteSpace(song.Title))
            {
                song.Title = Sections.FirstOrDefault()?.Label ?? "Untitled song";
                Title = song.Title;
            }
            await library.SaveSongAsync(song);
            await library.DiscardEditorBufferAsync(SongId);
            DraftRestored = false;
            StatusMessage = Localizer.Instance["editor-saved-status"];
            var undo = await library.GetUndoStateAsync(SongId);
            CanUndo = undo.CanUndo;
            CanRedo = undo.CanRedo;
            SongSaved?.Invoke(this, song);
        }
        catch (CoreException error)
        {
            ErrorMessage = Localizer.Instance[error.MessageKey];
            ErrorFixArticleId = string.IsNullOrWhiteSpace(error.FixArticleId) ? null : error.FixArticleId;
        }
        catch (Exception error) when (error is TimeoutException or IOException or InvalidOperationException)
        {
            ErrorMessage = Localizer.Instance["error-not-connected"];
        }
    }

    [RelayCommand]
    public async Task UndoAsync()
    {
        if (library is null)
        {
            return;
        }
        try
        {
            await library.UndoAsync(SongId);
            var reloaded = await library.GetSongAsync(SongId);
            if (reloaded is not null)
            {
                suppressCallbacks = true;
                try
                {
                    ApplySongProto(reloaded, autoDetected: false);
                }
                finally
                {
                    suppressCallbacks = false;
                }
                await RefreshSlidesAsync();
            }
            var undo = await library.GetUndoStateAsync(SongId);
            CanUndo = undo.CanUndo;
            CanRedo = undo.CanRedo;
        }
        catch (CoreException error)
        {
            ErrorMessage = Localizer.Instance[error.MessageKey];
        }
    }

    [RelayCommand]
    public async Task RedoAsync()
    {
        if (library is null)
        {
            return;
        }
        try
        {
            await library.RedoAsync(SongId);
            var reloaded = await library.GetSongAsync(SongId);
            if (reloaded is not null)
            {
                suppressCallbacks = true;
                try
                {
                    ApplySongProto(reloaded, autoDetected: false);
                }
                finally
                {
                    suppressCallbacks = false;
                }
                await RefreshSlidesAsync();
            }
            var undo = await library.GetUndoStateAsync(SongId);
            CanUndo = undo.CanUndo;
            CanRedo = undo.CanRedo;
        }
        catch (CoreException error)
        {
            ErrorMessage = Localizer.Instance[error.MessageKey];
        }
    }

    [RelayCommand]
    public async Task DiscardDraftAsync()
    {
        if (library is not null)
        {
            try
            {
                await library.DiscardEditorBufferAsync(SongId);
            }
            catch (Exception)
            {
            }
        }
        DraftRestored = false;
        StatusMessage = null;
    }

    public Song BuildSongProto()
    {
        double.TryParse(TempoText.Trim(), NumberStyles.Float, CultureInfo.InvariantCulture, out var tempo);
        var song = new Song
        {
            Id = SongId,
            Title = Title.Trim(),
            CcliNumber = CcliNumber.Trim(),
            Copyright = Copyright.Trim(),
            Key = SongKey.Trim(),
            Tempo = tempo,
            PublicDomain = PublicDomain,
        };
        foreach (var author in AuthorsText.Split([',', ';'], StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries))
        {
            song.Authors.Add(author);
        }
        foreach (var tag in TagsText.Split([',', ';'], StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries))
        {
            song.Tags.Add(tag);
        }
        foreach (var section in Sections)
        {
            song.Sections.Add(section.ToProto());
        }
        foreach (var arr in Arrangements)
        {
            var protoArr = new Arrangement
            {
                Id = arr.Id,
                Name = arr.Name.Trim(),
            };
            foreach (var token in arr.Tokens)
            {
                protoArr.SectionIds.Add(token.SectionId);
            }
            song.Arrangements.Add(protoArr);
        }
        return song;
    }

    public async Task RefreshSlidesAsync()
    {
        if (library is null)
        {
            return;
        }
        try
        {
            var placement = CopyrightPlacementIndex switch
            {
                0 => CopyrightPlacement.None,
                1 => CopyrightPlacement.First,
                2 => CopyrightPlacement.Last,
                4 => CopyrightPlacement.All,
                _ => CopyrightPlacement.FirstAndLast,
            };
            var request = new SplitSongSlides
            {
                Song = BuildSongProto(),
                MaxLines = (uint)Math.Clamp(MaxLinesPerSlide, 1, 16),
                CopyrightPlacement = placement,
                TitleSlide = TitleSlide,
                BlankSlideAtEnd = BlankSlideAtEnd,
                TranslationLanguage = TranslationLanguage.Trim(),
            };
            if (SelectedArrangement is { } arr)
            {
                request.ArrangementId = arr.Id;
            }
            var reply = await library.SplitSongSlidesAsync(request);
            GeneratedSlides.Clear();
            var idx = 1;
            foreach (var slide in reply.Slides)
            {
                var header = slide.PartCount > 1
                    ? $"{slide.SectionLabel} ({slide.PartIndex}/{slide.PartCount})"
                    : slide.SectionLabel;
                var chords = ShowChordsInSlides
                    ? string.Join(" · ", slide.Lines.SelectMany(l => l.Chords).Select(c => c.Text).Where(c => !string.IsNullOrEmpty(c)))
                    : string.Empty;
                GeneratedSlides.Add(new SlidePreviewCard
                {
                    Number = idx++,
                    Header = header,
                    ColorHex = LibraryIds.SectionColorHex(slide.SectionKind),
                    Body = string.Join("\n", slide.Lines.Select(l => l.Text)),
                    ChordsLine = chords,
                    Footer = slide.FooterCopyright,
                });
            }
            OnPropertyChanged(nameof(SlidesSummaryText));
        }
        catch (Exception)
        {
        }
    }

    private void OnModelMutated()
    {
        if (suppressCallbacks)
        {
            return;
        }
        SyncArrangementTokenLabels();
        _ = RefreshSlidesAsync();
        _ = AutosaveBufferAsync();
    }

    private void SyncArrangementTokenLabels()
    {
        var map = Sections.ToDictionary(s => LibraryIds.Hex(s.Id), s => s);
        foreach (var arr in Arrangements)
        {
            for (var i = 0; i < arr.Tokens.Count; i++)
            {
                var token = arr.Tokens[i];
                if (map.TryGetValue(LibraryIds.Hex(token.SectionId), out var sec)
                    && (token.Label != sec.Label || token.ColorHex != sec.ColorHex))
                {
                    arr.Tokens[i] = new ArrangementTokenViewModel
                    {
                        SectionId = sec.Id,
                        Label = sec.Label,
                        ColorHex = sec.ColorHex,
                    };
                }
            }
        }
    }

    private async Task AutosaveBufferAsync()
    {
        if (library is null)
        {
            return;
        }
        try
        {
            var json = JsonFormatter.Default.Format(BuildSongProto());
            await library.SaveEditorBufferAsync(SongId, DocumentType.Song, json);
        }
        catch (Exception)
        {
        }
    }

    private void ApplySongProto(Song song, bool autoDetected)
    {
        SongId = song.Id is { Value.IsEmpty: false } ? song.Id : LibraryIds.NewId();
        Title = song.Title;
        AuthorsText = string.Join(", ", song.Authors);
        SongKey = song.Key;
        TempoText = song.Tempo > 0 ? song.Tempo.ToString(CultureInfo.InvariantCulture) : string.Empty;
        CcliNumber = song.CcliNumber;
        Copyright = song.Copyright;
        TagsText = string.Join(", ", song.Tags);
        PublicDomain = song.PublicDomain;

        Sections.Clear();
        foreach (var protoSection in song.Sections)
        {
            Sections.Add(SongSectionItemViewModel.FromProto(protoSection, OnModelMutated, autoDetected));
        }
        if (Sections.Count == 0)
        {
            Sections.Add(new SongSectionItemViewModel(LibraryIds.NewId(), "Verse 1", SectionKind.Verse, "1", string.Empty, OnModelMutated));
        }
        SelectedSection = Sections.FirstOrDefault();

        var byHex = Sections.ToDictionary(s => LibraryIds.Hex(s.Id), s => s);
        Arrangements.Clear();
        foreach (var protoArr in song.Arrangements)
        {
            var arrId = protoArr.Id is { Value.IsEmpty: false } ? protoArr.Id : LibraryIds.NewId();
            var arr = new ArrangementItemViewModel(arrId, string.IsNullOrWhiteSpace(protoArr.Name) ? "Default" : protoArr.Name);
            foreach (var secId in protoArr.SectionIds)
            {
                if (byHex.TryGetValue(LibraryIds.Hex(secId), out var sec))
                {
                    arr.Tokens.Add(new ArrangementTokenViewModel
                    {
                        SectionId = sec.Id,
                        Label = sec.Label,
                        ColorHex = sec.ColorHex,
                    });
                }
            }
            Arrangements.Add(arr);
        }
        if (Arrangements.Count == 0)
        {
            var fallback = new ArrangementItemViewModel(LibraryIds.NewId(), "Default");
            foreach (var sec in Sections)
            {
                fallback.Tokens.Add(new ArrangementTokenViewModel
                {
                    SectionId = sec.Id,
                    Label = sec.Label,
                    ColorHex = sec.ColorHex,
                });
            }
            Arrangements.Add(fallback);
        }
        SelectedArrangement = Arrangements.FirstOrDefault();
    }
}
