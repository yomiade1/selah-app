using System.Text.RegularExpressions;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Headless;
using Avalonia.Headless.XUnit;
using Avalonia.Input;
using Avalonia.Media.Imaging;
using Avalonia.Platform;
using Avalonia.Styling;
using Avalonia.Threading;
using Avalonia.VisualTree;
using Google.Protobuf;
using Selah.Contracts.V1;
using Selah.UI.Services;
using Selah.UI.ViewModels;
using Selah.UI.Views;

namespace Selah.UI.Tests;

/// <summary>
/// M2-A7: Headless UI flows pass at 100% and 200% in both appearances (Light and Booth):
/// paste a song, edit a section, add scripture, import media, and Ctrl/Cmd+K finds a song,
/// a command, and a help article.
/// </summary>
public sealed partial class LibraryAndEditorTests : IDisposable
{
    private readonly Reset reset = new();

    public void Dispose() => reset.Dispose();

    public static TheoryData<int, string, string> ModesMatrix() =>
        new()
        {
            { 100, "light", "en" },
            { 100, "booth", "en" },
            { 200, "light", "en" },
            { 200, "booth", "en" },
            { 200, "light", Localizer.PseudoLocale },
        };

    private static void Configure(int scale, string appearance, string language = "en")
    {
        TextScale.Instance.Percent = scale;
        Localizer.Instance.SetLanguage(language);
        Application.Current!.RequestedThemeVariant = appearance == "booth" ? ThemeVariant.Dark : ThemeVariant.Light;
    }

    private static MainWindow OpenWindow(MainViewModel model)
    {
        var window = new MainWindow { DataContext = model, Width = 1280, Height = 800 };
        window.Show();
        Dispatcher.UIThread.RunJobs();
        return window;
    }

    private static void Snapshot(Window window, string name)
    {
        var folder = Repo.PathOf("target", "ui-snapshots");
        Directory.CreateDirectory(folder);
        using var frame = window.CaptureRenderedFrame();
        using var file = File.Create(Path.Combine(folder, name + ".png"));
        frame?.Save(file, PngBitmapEncoderOptions.Default);
    }

    private static byte[] CreateValidPngBytes(int width = 32, int height = 18)
    {
        using var wb = new WriteableBitmap(
            new PixelSize(width, height),
            new Vector(96, 96),
            PixelFormat.Bgra8888,
            AlphaFormat.Premul);
        using (var fb = wb.Lock())
        {
            var row = new byte[width * 4];
            for (var x = 0; x < width; x++)
            {
                row[x * 4 + 0] = 0xD6;
                row[x * 4 + 1] = 0x76;
                row[x * 4 + 2] = 0x00;
                row[x * 4 + 3] = 0xFF;
            }
            for (var y = 0; y < height; y++)
            {
                System.Runtime.InteropServices.Marshal.Copy(row, 0, fb.Address + y * fb.RowBytes, row.Length);
            }
        }
        using var ms = new MemoryStream();
        wb.Save(ms, PngBitmapEncoderOptions.Default);
        return ms.ToArray();
    }

    private static T ControlByName<T>(Control root, string name) where T : Control =>
        root.GetVisualDescendants().OfType<T>().Single(c => c.Name == name);

    private static List<string> ActiveCriticalLabelProblems(Window window, MainViewModel model)
    {
        var problems = LayoutTests.ClippedCriticalLabels(window);
        if (!model.IsSafeMode)
        {
            var safeModeMsg = $"\"{Localizer.Instance["safe-mode-message"]}\" is hidden";
            problems.Remove(safeModeMsg);
        }
        if (!model.IsLiveGuardOpen)
        {
            problems.Remove($"\"{Localizer.Instance["live-guard-title"]}\" is hidden");
            problems.Remove($"\"{Localizer.Instance["live-guard-keep"]}\" is hidden");
        }
        if (!model.IsLive)
        {
            problems.Remove($"\"{Localizer.Instance["preview-live"]}\" is hidden");
        }
        return problems;
    }

    [AvaloniaTheory]
    [MemberData(nameof(ModesMatrix))]
    public void Paste_a_song_detects_sections_and_generates_slides(int scale, string appearance, string language)
    {
        Configure(scale, appearance, language);
        var library = new FakeLibrary();
        var model = new MainViewModel(null, null, library, new FakeHelp());
        var window = OpenWindow(model);

        model.SelectModeCommand.Execute("SongEditor");
        model.SongEditor.StartNewSong(startOnPaste: true);
        model.SongEditor.Title = "How Great Thou Art";
        model.SongEditor.PasteRawText = """
            Verse 1:
            [G]O Lord my [C]God
            When I in awesome [G]wonder
            Consider [D]all the worlds
            Thy hands have [G]made

            Chorus:
            Then sings my [C]soul
            My Saviour God to [G]Thee
            """;
        model.SongEditor.DetectSectionsCommand.ExecuteAsync(null).GetAwaiter().GetResult();
        Dispatcher.UIThread.RunJobs();

        Assert.True(model.SongEditor.HasDetectionBanner);
        Assert.Equal(2, model.SongEditor.DetectedSectionCount);
        Assert.Equal(SongEditorStep.Sections, model.SongEditor.CurrentStep);
        Assert.True(model.SongEditor.GeneratedSlides.Count >= 2);
        Assert.Contains(model.SongEditor.GeneratedSlides, s => s.ChordsLine.Contains("G", StringComparison.Ordinal));
        Assert.All(model.SongEditor.GeneratedSlides, s =>
        {
            Assert.True(s.HasThumbnail);
            Assert.NotNull(s.Thumbnail);
        });

        var alert = ControlByName<Border>(window, "DetectionAlert");
        Assert.True(alert.IsEffectivelyVisible);
        Assert.Empty(ActiveCriticalLabelProblems(window, model));

        Snapshot(window, $"editor-paste-{scale}-{appearance}-{language}");
        window.Close();
    }

    [AvaloniaTheory]
    [InlineData(100, "light")]
    [InlineData(200, "booth")]
    public void Edit_section_arrange_transpose_save_and_undo(int scale, string appearance)
    {
        Configure(scale, appearance);
        var library = new FakeLibrary();
        var model = new MainViewModel(null, null, library, new FakeHelp());
        var window = OpenWindow(model);

        model.SelectModeCommand.Execute("Library");
        model.Library.LoadCommand.ExecuteAsync(null).GetAwaiter().GetResult();
        Dispatcher.UIThread.RunJobs();

        var existing = Assert.Single(model.Library.Songs);
        model.Library.SelectedSong = existing;
        model.Library.EditSelectedSongCommand.Execute(null);
        Dispatcher.UIThread.RunJobs();

        Assert.Equal(WorkspaceMode.SongEditor, model.ActiveMode);
        Assert.Equal("Amazing Grace", model.SongEditor.Title);

        // Edit first section lyrics/chords and adjust split line limit.
        var verse = model.SongEditor.Sections[0];
        verse.Label = "Verse 1";
        verse.LyricsWithChords = "[G]Amazing grace!\nHow sweet the sound\nThat saved a wretch\nNow I see";
        model.SongEditor.MaxLinesPerSlide = 2;
        model.SongEditor.RefreshSlidesAsync().GetAwaiter().GetResult();
        Dispatcher.UIThread.RunJobs();

        var countBeforeArrange = model.SongEditor.GeneratedSlides.Count;
        Assert.True(countBeforeArrange >= 2);

        // Switch to Arrange and add another section token.
        model.SongEditor.SelectStepCommand.Execute("Arrange");
        model.SongEditor.AppendSectionToArrangementCommand.Execute(model.SongEditor.Sections.Last());
        model.SongEditor.RefreshSlidesAsync().GetAwaiter().GetResult();
        Dispatcher.UIThread.RunJobs();
        Assert.True(model.SongEditor.GeneratedSlides.Count > countBeforeArrange);

        // Transpose +2 semitones and verify key updates.
        var initialKey = model.SongEditor.SongKey;
        model.SongEditor.TransposeByAsync(2).GetAwaiter().GetResult();
        Dispatcher.UIThread.RunJobs();
        Assert.NotEqual(initialKey, model.SongEditor.SongKey);

        // Save back to library and verify undo/redo tracking.
        model.SongEditor.Title = "Amazing Grace (Edited)";
        model.SongEditor.SaveSongCommand.ExecuteAsync(null).GetAwaiter().GetResult();
        Dispatcher.UIThread.RunJobs();
        Assert.Equal("Amazing Grace (Edited)", library.Songs.Single().Title);
        Assert.True(model.SongEditor.CanUndo);

        model.SongEditor.UndoCommand.ExecuteAsync(null).GetAwaiter().GetResult();
        Dispatcher.UIThread.RunJobs();
        Assert.Equal("Amazing Grace", model.SongEditor.Title);
        Assert.True(model.SongEditor.CanRedo);

        Assert.Empty(ActiveCriticalLabelProblems(window, model));
        window.Close();
    }

    [AvaloniaTheory]
    [InlineData(100, "light")]
    [InlineData(200, "booth")]
    public void Add_scripture_lookup_search_and_split_slides(int scale, string appearance)
    {
        Configure(scale, appearance);
        var library = new FakeLibrary();
        var model = new MainViewModel(null, null, library, new FakeHelp());
        var window = OpenWindow(model);

        model.OpenLibraryTabCommand.Execute("Bibles");
        model.Library.LoadCommand.ExecuteAsync(null).GetAwaiter().GetResult();
        Dispatcher.UIThread.RunJobs();

        Assert.Equal(2, model.Library.Bibles.Count);
        model.Library.ScriptureReferenceQuery = "John 3:16-18";
        model.Library.LookupPassageCommand.ExecuteAsync(null).GetAwaiter().GetResult();
        Dispatcher.UIThread.RunJobs();

        Assert.Equal(3, model.Library.Verses.Count);
        model.Library.ScriptureMaxLines = 1;
        model.Library.ScriptureTitleSlide = true;
        model.Library.SplitScriptureSlidesAsync().GetAwaiter().GetResult();
        Dispatcher.UIThread.RunJobs();
        Assert.Equal(4, model.Library.ScripturePreviewSlides.Count);
        Assert.All(model.Library.ScripturePreviewSlides, s =>
        {
            Assert.True(s.HasThumbnail);
            Assert.NotNull(s.Thumbnail);
        });

        model.Library.ScriptureSearchQuery = "loved";
        model.Library.SearchScriptureCommand.ExecuteAsync(null).GetAwaiter().GetResult();
        Dispatcher.UIThread.RunJobs();
        Assert.NotEmpty(model.Library.SearchHits);

        Assert.Empty(ActiveCriticalLabelProblems(window, model));
        Snapshot(window, $"library-bibles-{scale}-{appearance}");
        window.Close();
    }

    [AvaloniaTheory]
    [InlineData(100, "light")]
    [InlineData(200, "booth")]
    public void Import_media_loads_thumbnail_bitmap_in_media_bin(int scale, string appearance)
    {
        Configure(scale, appearance);
        var library = new FakeLibrary();
        var model = new MainViewModel(null, null, library, new FakeHelp());
        var window = OpenWindow(model);

        var tempFile = Path.Combine(Path.GetTempPath(), $"selah-ui-test-{Guid.NewGuid():N}.png");
        try
        {
            File.WriteAllBytes(tempFile, CreateValidPngBytes());
            model.OpenLibraryTabCommand.Execute("Media");
            model.Library.LoadCommand.ExecuteAsync(null).GetAwaiter().GetResult();
            model.Library.ImportMediaPath = tempFile;
            model.Library.ImportMediaCommand.ExecuteAsync(null).GetAwaiter().GetResult();
            Dispatcher.UIThread.RunJobs();

            var card = Assert.Single(model.Library.MediaItems);
            Assert.True(card.HasThumbnail);
            Assert.NotNull(card.Thumbnail);
            Assert.Equal(32, card.Thumbnail.PixelSize.Width);
            Assert.Empty(ActiveCriticalLabelProblems(window, model));

            Snapshot(window, $"library-media-{scale}-{appearance}");
        }
        finally
        {
            if (File.Exists(tempFile))
            {
                File.Delete(tempFile);
            }
            window.Close();
        }
    }

    [AvaloniaTheory]
    [InlineData(100, "light")]
    [InlineData(200, "booth")]
    public void Ctrl_K_finds_song_command_and_help_article(int scale, string appearance)
    {
        Configure(scale, appearance);
        var library = new FakeLibrary();
        var help = new FakeHelp();
        var model = new MainViewModel(null, null, library, help);
        var openedArticles = new List<string>();
        model.ArticleRequested += (_, id) => openedArticles.Add(id);

        var window = OpenWindow(model);
        var commandModifier = OperatingSystem.IsMacOS() ? RawInputModifiers.Meta : RawInputModifiers.Control;

        // Press Ctrl/Cmd+K and verify palette opens below Toolbar (Clear everything remains uncovered).
        window.KeyPress(Key.K, commandModifier, PhysicalKey.K, "k");
        Dispatcher.UIThread.RunJobs();
        Assert.True(model.CommandPalette.IsOpen);

        var clearBtn = ControlByName<Button>(window, "ClearEverythingButton");
        var paletteDialog = ControlByName<Border>(window, "PaletteDialog");
        var clearRect = new Rect(clearBtn.TranslatePoint(default, window) ?? default, clearBtn.Bounds.Size);
        var paletteRect = new Rect(paletteDialog.TranslatePoint(default, window) ?? default, paletteDialog.Bounds.Size);
        Assert.False(clearRect.Intersects(paletteRect), "Ctrl+K palette must not cover Clear everything");

        // 1) Find a song and activate it -> enters Song Editor with that song loaded.
        model.CommandPalette.Query = "Amazing";
        model.CommandPalette.SearchAsync("Amazing").GetAwaiter().GetResult();
        Dispatcher.UIThread.RunJobs();
        var songHit = Assert.Single(model.CommandPalette.Results, r => r.Kind == PaletteItemKind.Song);
        Snapshot(window, $"palette-{scale}-{appearance}");
        model.CommandPalette.ActivateCommand.Execute(songHit);
        Dispatcher.UIThread.RunJobs();
        Assert.False(model.CommandPalette.IsOpen);
        Assert.Equal(WorkspaceMode.SongEditor, model.ActiveMode);
        Assert.Equal("Amazing Grace", model.SongEditor.Title);

        // 2) Ctrl/Cmd+K finds a command and executes it -> switches back to Library.
        window.KeyPress(Key.K, commandModifier, PhysicalKey.K, "k");
        Dispatcher.UIThread.RunJobs();
        Assert.True(model.CommandPalette.IsOpen);
        model.CommandPalette.Query = "Library";
        model.CommandPalette.SearchAsync("Library").GetAwaiter().GetResult();
        Dispatcher.UIThread.RunJobs();
        var cmdHit = model.CommandPalette.Results.First(r => r.Kind == PaletteItemKind.Command);
        model.CommandPalette.ActivateCommand.Execute(cmdHit);
        Dispatcher.UIThread.RunJobs();
        Assert.Equal(WorkspaceMode.Library, model.ActiveMode);

        // 3) Ctrl/Cmd+K finds a help article and opens it.
        window.KeyPress(Key.K, commandModifier, PhysicalKey.K, "k");
        Dispatcher.UIThread.RunJobs();
        model.CommandPalette.Query = "Show a song";
        model.CommandPalette.SearchAsync("Show a song").GetAwaiter().GetResult();
        Dispatcher.UIThread.RunJobs();
        var helpHit = Assert.Single(model.CommandPalette.Results, r => r.Kind == PaletteItemKind.Help);
        model.CommandPalette.ActivateCommand.Execute(helpHit);
        Dispatcher.UIThread.RunJobs();
        Assert.Contains("live.show-a-song", openedArticles);

        Assert.Empty(ActiveCriticalLabelProblems(window, model));
        window.Close();
    }

    [AvaloniaTheory]
    [MemberData(nameof(ModesMatrix))]
    public void Workspace_modes_and_palette_preserve_all_critical_labels_while_live(int scale, string appearance, string language)
    {
        Configure(scale, appearance, language);
        var model = new MainViewModel(null, null, new FakeLibrary(), new FakeHelp());
        model.Apply(new ConnectionSnapshot(true, true, SafeMode: true, Live: true, 1));
        _ = model.ConfirmIfLiveAsync(LiveGuardKind.Quit);
        var window = OpenWindow(model);

        model.SelectModeCommand.Execute("Library");
        Dispatcher.UIThread.RunJobs();
        Assert.Empty(LayoutTests.ClippedCriticalLabels(window));

        model.SelectModeCommand.Execute("SongEditor");
        Dispatcher.UIThread.RunJobs();
        Assert.Empty(LayoutTests.ClippedCriticalLabels(window));

        model.SelectModeCommand.Execute("SlideEditor");
        Dispatcher.UIThread.RunJobs();
        Assert.Empty(LayoutTests.ClippedCriticalLabels(window));

        model.SelectModeCommand.Execute("Templates");
        Dispatcher.UIThread.RunJobs();
        Assert.Empty(LayoutTests.ClippedCriticalLabels(window));

        model.OpenCommandPaletteCommand.Execute(null);
        Dispatcher.UIThread.RunJobs();
        Assert.Empty(LayoutTests.ClippedCriticalLabels(window));

        window.Close();
    }

    [AvaloniaTheory]
    [MemberData(nameof(ModesMatrix))]
    public void Slide_editor_create_align_undo_and_redo_flow(int scale, string appearance, string language)
    {
        Configure(scale, appearance, language);
        var library = new FakeLibrary();
        var model = new MainViewModel(null, null, library, new FakeHelp());
        var window = OpenWindow(model);

        model.SelectModeCommand.Execute("SlideEditor");
        model.SlideEditor.LoadAsync().GetAwaiter().GetResult();
        Dispatcher.UIThread.RunJobs();

        model.SlideEditor.PresentationName = "Sunday Announcements";
        model.SlideEditor.AddTextElementCommand.Execute(null);
        model.SlideEditor.AddImageElementCommand.Execute(null);
        model.SlideEditor.AddShapeElementCommand.Execute("rounded_rect");
        Dispatcher.UIThread.RunJobs();

        Assert.Equal(3, model.SlideEditor.Elements.Count);
        var textEl = model.SlideEditor.Elements[0];
        var imageEl = model.SlideEditor.Elements[1];
        var shapeEl = model.SlideEditor.Elements[2];

        textEl.X = 0.05;
        textEl.Width = 0.20;
        imageEl.X = 0.70;
        imageEl.Width = 0.20;
        shapeEl.X = 0.42;
        shapeEl.Width = 0.12;
        shapeEl.Locked = true;
        var lockedOriginalX = shapeEl.X;
        var imageOriginalX = imageEl.X;

        model.SlideEditor.SelectAllElementsCommand.Execute(null);
        model.SlideEditor.AlignCenterCommand.Execute(null);
        Dispatcher.UIThread.RunJobs();

        var expectedTextCenterX = textEl.X + textEl.Width / 2.0;
        var expectedImageCenterX = imageEl.X + imageEl.Width / 2.0;
        Assert.Equal(expectedTextCenterX, expectedImageCenterX, precision: 3);
        Assert.NotEqual(imageOriginalX, imageEl.X, precision: 3);
        Assert.Equal(lockedOriginalX, shapeEl.X, precision: 3);
        Assert.True(model.SlideEditor.CanUndo);

        model.SlideEditor.UndoCommand.ExecuteAsync(null).GetAwaiter().GetResult();
        Dispatcher.UIThread.RunJobs();
        Assert.Equal(imageOriginalX, model.SlideEditor.Elements[1].X, precision: 3);
        Assert.True(model.SlideEditor.CanRedo);

        model.SlideEditor.RedoCommand.ExecuteAsync(null).GetAwaiter().GetResult();
        Dispatcher.UIThread.RunJobs();
        Assert.NotEqual(imageOriginalX, model.SlideEditor.Elements[1].X, precision: 3);

        model.SlideEditor.SavePresentationCommand.ExecuteAsync(null).GetAwaiter().GetResult();
        Dispatcher.UIThread.RunJobs();

        var saved = Assert.Single(library.Presentations);
        Assert.Equal("Sunday Announcements", saved.Name);
        Assert.Single(saved.Slides);
        Assert.True(model.SlideEditor.HasPreviewBitmap);
        Assert.Empty(ActiveCriticalLabelProblems(window, model));

        Snapshot(window, $"slide-editor-{scale}-{appearance}-{language}");
        window.Close();
    }

    [AvaloniaTheory]
    [MemberData(nameof(ModesMatrix))]
    public void Template_editor_looks_assignment_and_song_fit_flow(int scale, string appearance, string language)
    {
        Configure(scale, appearance, language);
        var library = new FakeLibrary();
        var model = new MainViewModel(null, null, library, new FakeHelp());
        var window = OpenWindow(model);

        model.SelectModeCommand.Execute("Templates");
        model.TemplateEditor.LoadAsync().GetAwaiter().GetResult();
        Dispatcher.UIThread.RunJobs();

        var initialCount = library.Templates.Count;
        model.TemplateEditor.NewTemplateCommand.Execute(null);
        model.TemplateEditor.ApplyLookCommand.Execute("warm-serif");
        model.TemplateEditor.TemplateName = "Sunday Warm Serif";
        model.TemplateEditor.RoleSize = 68;
        model.TemplateEditor.HasBackgroundBox = true;
        model.TemplateEditor.SaveTemplateCommand.ExecuteAsync(null).GetAwaiter().GetResult();
        Dispatcher.UIThread.RunJobs();

        Assert.Equal(initialCount + 1, library.Templates.Count);
        Assert.Contains(library.Templates, t => t.Name == "Sunday Warm Serif");
        Assert.True(model.TemplateEditor.HasPreviewBitmap);

        model.TemplateEditor.SetAsDefaultSongTemplateCommand.ExecuteAsync(null).GetAwaiter().GetResult();
        model.TemplateEditor.AssignToSelectedOutputCommand.ExecuteAsync(null).GetAwaiter().GetResult();
        Dispatcher.UIThread.RunJobs();

        Assert.Equal("Sunday Warm Serif", model.TemplateEditor.DefaultSongTemplateName);
        Assert.True(LibraryIds.Same(library.Outputs[0].TemplateId, model.TemplateEditor.TemplateId));

        model.SelectModeCommand.Execute("SongEditor");
        model.SongEditor.LoadTemplatesAsync().GetAwaiter().GetResult();
        model.SongEditor.LoadSongAsync(library.Songs.Single()).GetAwaiter().GetResult();
        Dispatcher.UIThread.RunJobs();

        Assert.Equal("Sunday Warm Serif", model.SongEditor.SelectedTemplate?.Name);
        Assert.True(model.SongEditor.HasFitBadge);
        Assert.Contains("68", model.SongEditor.FitSummaryText, StringComparison.Ordinal);
        Assert.All(model.SongEditor.GeneratedSlides, card => Assert.True(card.HasThumbnail));
        Assert.Empty(ActiveCriticalLabelProblems(window, model));

        Snapshot(window, $"template-editor-{scale}-{appearance}-{language}");
        window.Close();
    }

    [AvaloniaTheory]
    [MemberData(nameof(ModesMatrix))]
    public void Slide_editor_flash_warning_guard_blocks_unconfirmed_stage(int scale, string appearance, string language)
    {
        Configure(scale, appearance, language);
        var library = new FakeLibrary();
        var model = new MainViewModel(null, null, library, new FakeHelp());
        var window = OpenWindow(model);

        model.SelectModeCommand.Execute("SlideEditor");
        model.SlideEditor.LoadAsync().GetAwaiter().GetResult();
        model.SlideEditor.AddTextElementCommand.Execute(null);
        model.SlideEditor.TransitionKind = "fade_through_black";
        model.SlideEditor.TransitionDurationMs = 120;
        model.SlideEditor.RepeatCadenceMs = 150;
        model.SlideEditor.RefreshPreviewAndRiskAsync().GetAwaiter().GetResult();
        Dispatcher.UIThread.RunJobs();

        Assert.True(model.SlideEditor.HasFlashWarning);
        Assert.True(model.SlideEditor.IsStageBlockedByFlashGuard);
        Assert.False(model.SlideEditor.CanStageSlide);

        var stageBtn = ControlByName<Button>(window, "StageCustomSlideButton");
        var flashBanner = ControlByName<Border>(window, "FlashGuardBanner");
        var ackCheck = ControlByName<CheckBox>(window, "AcknowledgeFlashWarningCheckBox");
        Assert.False(stageBtn.IsEnabled);
        Assert.True(flashBanner.IsEffectivelyVisible);

        ackCheck.IsChecked = true;
        Dispatcher.UIThread.RunJobs();

        Assert.True(model.SlideEditor.CanStageSlide);
        Assert.True(stageBtn.IsEnabled);

        model.SlideEditor.StageSlideCommand.ExecuteAsync(null).GetAwaiter().GetResult();
        Dispatcher.UIThread.RunJobs();

        Assert.Single(library.StagedScenes);
        Assert.True(model.HasStagedPreview);
        Assert.Empty(ActiveCriticalLabelProblems(window, model));

        window.Close();
    }

    [AvaloniaTheory]
    [MemberData(nameof(ModesMatrix))]
    public void Dual_preview_and_slide_staging_render_thumbnails_and_preserve_critical_labels(int scale, string appearance, string language)
    {
        Configure(scale, appearance, language);
        var library = new FakeLibrary();
        var model = new MainViewModel(null, null, library, new FakeHelp());
        model.Apply(new ConnectionSnapshot(true, true, SafeMode: false, Live: true, Generation: 1, PreviewSceneRevision: 0, FlashWarning: true));
        var window = OpenWindow(model);

        var previewImage = ControlByName<Image>(window, "PreviewImage");
        var stageImage = ControlByName<Image>(window, "StagePreviewImage");
        var programSurface = ControlByName<Grid>(window, "ProgramPreviewSurface");
        var stageSurface = ControlByName<Grid>(window, "StagePreviewSurface");
        var flashBanner = ControlByName<TextBlock>(window, "PreviewFlashWarningText");
        Assert.True(previewImage.IsEffectivelyVisible);
        Assert.True(stageImage.IsEffectivelyVisible);
        Assert.True(programSurface.Bounds.Width > 0 && programSurface.Bounds.Height > 0);
        Assert.True(stageSurface.Bounds.Width > 0 && stageSurface.Bounds.Height > 0);
        var programOrigin = programSurface.TranslatePoint(default, window) ?? default;
        var stageOrigin = stageSurface.TranslatePoint(default, window) ?? default;
        if (scale == 100)
        {
            Assert.True(stageOrigin.X > programOrigin.X, "At 100%, Staged preview sits beside Program preview.");
        }
        else
        {
            Assert.True(stageOrigin.Y > programOrigin.Y, "At 200%, Staged preview stacks below Program preview.");
        }
        Assert.True(flashBanner.IsEffectivelyVisible);
        Assert.Empty(ActiveCriticalLabelProblems(window, model));

        // Load library song previews and verify offscreen thumbnails are populated.
        model.SelectModeCommand.Execute("Library");
        model.Library.LoadCommand.ExecuteAsync(null).GetAwaiter().GetResult();
        Dispatcher.UIThread.RunJobs();
        var selectedSong = Assert.Single(model.Library.Songs);
        model.Library.SelectedSong = selectedSong;
        Dispatcher.UIThread.RunJobs();
        Assert.NotEmpty(model.Library.SongPreviewSlides);
        Assert.All(model.Library.SongPreviewSlides, card => Assert.True(card.HasThumbnail));

        // Edit song, switch to 32:9 target (wider thumbnail), then stage first slide to Next preview.
        model.Library.EditSelectedSongCommand.Execute(null);
        Dispatcher.UIThread.RunJobs();
        model.SongEditor.SelectPreviewTargetCommand.Execute("Led");
        model.SongEditor.RefreshSlidesAsync().GetAwaiter().GetResult();
        Dispatcher.UIThread.RunJobs();

        var firstSlide = model.SongEditor.GeneratedSlides[0];
        Assert.True(firstSlide.HasThumbnail);
        Assert.Equal(3840u, firstSlide.PreviewScene?.Resolution?.Width);
        Assert.Equal(320, firstSlide.Thumbnail!.PixelSize.Width);
        Assert.Equal(90, firstSlide.Thumbnail!.PixelSize.Height);

        model.SongEditor.StageSlideCommand.ExecuteAsync(firstSlide).GetAwaiter().GetResult();
        Dispatcher.UIThread.RunJobs();

        Assert.Single(library.StagedScenes);
        Assert.True(model.HasStagedPreview);
        Assert.Equal(Localizer.Instance["editor-staged-status"], model.SongEditor.StatusMessage);
        Assert.Empty(ActiveCriticalLabelProblems(window, model));

        window.Close();
    }

    [AvaloniaTheory]
    [MemberData(nameof(ModesMatrix))]
    public void Service_planner_builds_service_inherits_last_week_projects_runtime_and_exports_print(int scale, string appearance, string language)
    {
        Configure(scale, appearance, language);
        var library = new FakeLibrary();
        var model = new MainViewModel(null, null, library, new FakeHelp());
        var window = OpenWindow(model);

        model.SelectModeCommand.Execute("ServicePlanner");
        model.ServicePlanner.LoadCommand.ExecuteAsync(null).GetAwaiter().GetResult();
        Dispatcher.UIThread.RunJobs();

        Assert.Equal(WorkspaceMode.ServicePlanner, model.ActiveMode);
        Assert.NotEmpty(model.ServicePlanner.Services);
        Assert.NotEmpty(model.ServicePlanner.ServiceTypes);
        Assert.NotEmpty(model.ServicePlanner.TeamMembers);
        Assert.True(model.ServicePlanner.Items.Count >= 6);

        // Add an Audio item and a Note item so all item kinds are represented, then reorder and save.
        model.ServicePlanner.AddAudioItemCommand.ExecuteAsync(null).GetAwaiter().GetResult();
        model.ServicePlanner.AddNoteItemCommand.ExecuteAsync(null).GetAwaiter().GetResult();
        model.ServicePlanner.MoveItemUpCommand.ExecuteAsync(null).GetAwaiter().GetResult();
        model.ServicePlanner.SaveServiceCommand.ExecuteAsync(null).GetAwaiter().GetResult();
        Dispatcher.UIThread.RunJobs();

        Assert.Contains("09:00", model.ServicePlanner.RuntimeSummary);
        Assert.Equal(Localizer.Instance["services-saved-status"], model.ServicePlanner.StatusMessage);

        // Add a volunteer to the team roster.
        model.ServicePlanner.NewMemberName = "Alex Rivera";
        model.ServicePlanner.NewMemberRole = "Camera 1";
        model.ServicePlanner.AddTeamMemberCommand.ExecuteAsync(null).GetAwaiter().GetResult();
        Dispatcher.UIThread.RunJobs();
        Assert.Contains(model.ServicePlanner.TeamMembers, m => m.Name == "Alex Rivera" && m.Initials == "AR");

        // Export print sheet & transposed chord charts.
        model.ServicePlanner.ExportChordChartsCommand.ExecuteAsync(null).GetAwaiter().GetResult();
        Dispatcher.UIThread.RunJobs();
        Assert.Contains("Amazing Grace", model.ServicePlanner.LastExportedHtml);
        Assert.True(model.ServicePlanner.LastExportedPdfBytes.Length > 100);
        Assert.True(model.ServicePlanner.LastExportedPageCount >= 1);

        // Inherit from last week.
        var beforeCount = model.ServicePlanner.Services.Count;
        model.ServicePlanner.InheritFromLastWeekCommand.ExecuteAsync(null).GetAwaiter().GetResult();
        Dispatcher.UIThread.RunJobs();
        Assert.Equal(beforeCount + 1, model.ServicePlanner.Services.Count);
        Assert.Equal(Localizer.Instance["services-inherited-status"], model.ServicePlanner.StatusMessage);

        Assert.Empty(ActiveCriticalLabelProblems(window, model));
        Snapshot(window, $"service-planner-{scale}-{appearance}-{language}");
        window.Close();
    }

    private sealed class FakeHelp : IHelpSource
    {
        private static readonly string Ast = File.ReadAllText(Repo.PathOf("tests", "fixtures", "kb", "show-a-song.ast.json"));

        public Task<IReadOnlyList<Article>> SearchHelpAsync(string query, IEnumerable<string> anchors) =>
            Task.FromResult<IReadOnlyList<Article>>(
                query.Contains("song", StringComparison.OrdinalIgnoreCase)
                    ? [new Article { Id = "live.show-a-song", Title = "Show a song on the screens", Summary = "Pick the song and move through slides.", AstJson = Ast }]
                    : []);

        public Task<IReadOnlyList<Article>> HelpForContextAsync(IEnumerable<string> anchors) =>
            Task.FromResult<IReadOnlyList<Article>>([]);

        public Task<Article?> GetArticleAsync(string id) =>
            Task.FromResult<Article?>(new Article { Id = id, Title = "Show a song on the screens", AstJson = Ast });

        public Task RecordFeedbackAsync(string articleId, bool helpful) => Task.CompletedTask;
    }

    private sealed partial class FakeLibrary : ILibraryService
    {
        public List<Song> Songs { get; } = [];
        public List<MediaItem> Media { get; } = [];
        public List<Template> Templates { get; } = [];
        public List<Presentation> Presentations { get; } = [];
        public List<Service> Services { get; } = [];
        public List<ServiceType> ServiceTypes { get; } = [];
        public List<Team> Teams { get; } = [];
        public List<OutputConfig> Outputs { get; } =
        [
            new OutputConfig { OutputIndex = 0, Name = "Main 16:9", Role = OutputRole.Main, Resolution = new Resolution { Width = 1920, Height = 1080 } },
            new OutputConfig { OutputIndex = 1, Name = "LED 32:9", Role = OutputRole.Main, Resolution = new Resolution { Width = 3840, Height = 1080 } },
        ];
        public Dictionary<string, PolicySetting> Policies { get; } = new(StringComparer.Ordinal);
        public List<Scene> StagedScenes { get; } = [];
        public Dictionary<string, byte[]> Thumbnails { get; } = new();
        public List<BibleTranslation> Bibles { get; } =
        [
            new BibleTranslation { Code = "KJV", Name = "King James Version", Language = "en", BookCount = 66, VerseCount = 31102, IsBundled = true },
            new BibleTranslation { Code = "WEB", Name = "World English Bible", Language = "en", BookCount = 66, VerseCount = 31100, IsBundled = true },
        ];
        private readonly Dictionary<string, EditorBuffer> buffers = new();
        private readonly Stack<List<Song>> undoStack = new();
        private readonly Stack<List<Song>> redoStack = new();

        public event EventHandler<LibraryChanged>? LibraryChanged;
        public event EventHandler<UndoStateChanged>? UndoStateChanged;

        public FakeLibrary()
        {
            Templates.Add(TemplateStyleModel.StarterSanctuary().ToProto(LibraryIds.NewId()));
            Templates.Add(TemplateStyleModel.StarterWarmSerif().ToProto(LibraryIds.NewId()));
            Templates.Add(TemplateStyleModel.StarterLowerThird().ToProto(LibraryIds.NewId()));
            Templates.Add(TemplateStyleModel.StarterStageMonitor().ToProto(LibraryIds.NewId()));

            var secId1 = LibraryIds.NewId();
            var secId2 = LibraryIds.NewId();
            var arrId = LibraryIds.NewId();
            var song = new Song
            {
                Id = LibraryIds.NewId(),
                Title = "Amazing Grace",
                Key = "G",
                PublicDomain = true,
                Copyright = "Public Domain",
            };
            song.Authors.Add("John Newton");
            var v1 = new Section { Id = secId1, Label = "Verse 1", Kind = SectionKind.Verse, Hotkey = "1" };
            v1.Lines.Add(SongSectionItemViewModel.ParseInlineChordLine("[G]Amazing [C]grace! How [G]sweet the sound"));
            v1.Lines.Add(SongSectionItemViewModel.ParseInlineChordLine("That saved a [D]wretch like [G]me"));
            var c1 = new Section { Id = secId2, Label = "Chorus", Kind = SectionKind.Chorus, Hotkey = "C" };
            c1.Lines.Add(SongSectionItemViewModel.ParseInlineChordLine("Was blind but now I see"));
            song.Sections.Add(v1);
            song.Sections.Add(c1);
            var arr = new Arrangement { Id = arrId, Name = "Sunday" };
            arr.SectionIds.Add(secId1);
            arr.SectionIds.Add(secId2);
            song.Arrangements.Add(arr);
            Songs.Add(song);
        }

        private List<Song> CloneSongs() => Songs.Select(s => s.Clone()).ToList();

        public Task<IReadOnlyList<Song>> ListSongsAsync(uint limit = 200, uint offset = 0) =>
            Task.FromResult<IReadOnlyList<Song>>(Songs.Select(s => s.Clone()).ToList());

        public Task<SongSearchResults> SearchSongsAsync(string query, uint limit = 50, uint offset = 0)
        {
            var results = new SongSearchResults();
            foreach (var s in Songs.Where(s => s.Title.Contains(query, StringComparison.OrdinalIgnoreCase)))
            {
                results.Hits.Add(new SongSearchHit { Song = s.Clone(), Score = 1.0, Snippet = s.Title });
            }
            results.Total = (uint)results.Hits.Count;
            return Task.FromResult(results);
        }

        public Task<Song?> GetSongAsync(Id id) =>
            Task.FromResult(Songs.FirstOrDefault(s => LibraryIds.Same(s.Id, id))?.Clone());

        public Task SaveSongAsync(Song song, Id? undoGroup = null)
        {
            undoStack.Push(CloneSongs());
            redoStack.Clear();
            var idx = Songs.FindIndex(s => LibraryIds.Same(s.Id, song.Id));
            if (idx >= 0)
            {
                Songs[idx] = song.Clone();
            }
            else
            {
                Songs.Add(song.Clone());
            }
            RaiseChanged(DocumentType.Song, song.Id);
            return Task.CompletedTask;
        }

        public Task DeleteSongAsync(Id id, Id? undoGroup = null)
        {
            undoStack.Push(CloneSongs());
            redoStack.Clear();
            Songs.RemoveAll(s => LibraryIds.Same(s.Id, id));
            RaiseChanged(DocumentType.Song, id);
            return Task.CompletedTask;
        }

        public Task<ParsePastedSongReply> ParsePastedSongAsync(string rawText, SongPasteFormat format = SongPasteFormat.Unspecified, string defaultTitle = "")
        {
            var song = new Song { Id = LibraryIds.NewId(), Title = string.IsNullOrWhiteSpace(defaultTitle) ? "Untitled" : defaultTitle, Key = "G" };
            var blocks = rawText.Replace("\r\n", "\n").Split("\n\n", StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
            foreach (var block in blocks)
            {
                var lines = block.Split('\n');
                var label = "Verse 1";
                var kind = SectionKind.Verse;
                var start = 0;
                if (lines[0].TrimEnd().EndsWith(':'))
                {
                    label = lines[0].TrimEnd().TrimEnd(':');
                    kind = label.StartsWith("Chorus", StringComparison.OrdinalIgnoreCase) ? SectionKind.Chorus : SectionKind.Verse;
                    start = 1;
                }
                var sec = new Section { Id = LibraryIds.NewId(), Label = label, Kind = kind, Color = LibraryIds.SectionColor(kind) };
                for (var i = start; i < lines.Length; i++)
                {
                    sec.Lines.Add(SongSectionItemViewModel.ParseInlineChordLine(lines[i]));
                }
                song.Sections.Add(sec);
            }
            var arr = new Arrangement { Id = LibraryIds.NewId(), Name = "Default" };
            arr.SectionIds.AddRange(song.Sections.Select(s => s.Id));
            song.Arrangements.Add(arr);
            return Task.FromResult(new ParsePastedSongReply { Song = song, DetectedFormat = SongPasteFormat.ChordPro });
        }

        public Task<Song> TransposeSongAsync(Song song, int semitones, AccidentalSpelling spelling = AccidentalSpelling.Unspecified)
        {
            var copy = song.Clone();
            string[] notes = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
            var currentIdx = Math.Max(0, Array.IndexOf(notes, copy.Key));
            copy.Key = notes[((currentIdx + semitones) % 12 + 12) % 12];
            return Task.FromResult(copy);
        }

        public Task<GeneratedSongSlides> SplitSongSlidesAsync(SplitSongSlides request)
        {
            var outSlides = new GeneratedSongSlides();
            var song = request.Song;
            var max = (int)(request.MaxLines == 0 ? 4 : request.MaxLines);
            var byId = song.Sections.ToDictionary(s => LibraryIds.Hex(s.Id), s => s);
            IEnumerable<Section> seq = song.Arrangements.FirstOrDefault(a => LibraryIds.Same(a.Id, request.ArrangementId)) is { SectionIds.Count: > 0 } arr
                ? arr.SectionIds.Where(id => byId.ContainsKey(LibraryIds.Hex(id))).Select(id => byId[LibraryIds.Hex(id)])
                : song.Sections;
            foreach (var sec in seq)
            {
                var chunks = Math.Max(1, (int)Math.Ceiling(sec.Lines.Count / (double)max));
                for (var c = 0; c < chunks; c++)
                {
                    var slide = new GeneratedSlide
                    {
                        SectionId = sec.Id,
                        SectionLabel = sec.Label,
                        SectionKind = sec.Kind,
                        PartIndex = (uint)(c + 1),
                        PartCount = (uint)chunks,
                    };
                    slide.Lines.AddRange(sec.Lines.Skip(c * max).Take(max));
                    outSlides.Slides.Add(slide);
                }
            }
            PopulateSongFitReports(request, outSlides);
            return Task.FromResult(outSlides);
        }

        private void PopulateSongFitReports(SplitSongSlides request, GeneratedSongSlides outSlides)
        {
            if (!request.ShrinkToFit && request.TargetOutputs.Count == 0 && request.TemplateId is null && string.IsNullOrEmpty(request.StyleJson))
            {
                return;
            }
            var style = ResolveTemplateStyle(request.TemplateId, "templates.default.song");
            var body = style.Roles.Body;
            var targets = request.TargetOutputs.Count > 0
                ? request.TargetOutputs.ToList()
                : [new FitTargetOutput { OutputIndex = 0, Name = "Main", Role = OutputRole.Main, Resolution = new Resolution { Width = 1920, Height = 1080 } }];
            foreach (var target in targets)
            {
                var applied = body.Size > 0 ? body.Size : 74f;
                var minSize = body.MinSize > 0 ? body.MinSize : 32f;
                outSlides.FitReports.Add(new OutputFitReport
                {
                    OutputIndex = target.OutputIndex,
                    OutputName = target.Name,
                    AppliedSize = applied,
                    MinSize = minSize,
                    MatchedVariant = "16:9",
                    Fits = true,
                    Summary = $"Fits at {applied:0} pt",
                });
                var fitted = new OutputFittedSongSlides { OutputIndex = target.OutputIndex };
                foreach (var s in outSlides.Slides)
                {
                    fitted.Slides.Add(s.Clone());
                }
                outSlides.PerOutputSlides.Add(fitted);
            }
        }

        private TemplateStyleModel ResolveTemplateStyle(Id? templateId, string defaultPolicyKey)
        {
            Template? match = null;
            if (templateId is { Value.IsEmpty: false })
            {
                match = Templates.FirstOrDefault(t => LibraryIds.Same(t.Id, templateId));
            }
            if (match is null && Policies.TryGetValue(defaultPolicyKey, out var setting) && !string.IsNullOrWhiteSpace(setting.ValueJson))
            {
                var hex = setting.ValueJson.Trim().Trim('"');
                match = Templates.FirstOrDefault(t => string.Equals(LibraryIds.Hex(t.Id), hex, StringComparison.OrdinalIgnoreCase));
            }
            return match is not null ? TemplateStyleModel.FromProto(match) : TemplateStyleModel.StarterSanctuary();
        }

        public Task SaveEditorBufferAsync(Id docId, DocumentType docType, string content)
        {
            buffers[LibraryIds.Hex(docId)] = new EditorBuffer { DocId = docId, DocType = docType, Content = content };
            return Task.CompletedTask;
        }

        public Task DiscardEditorBufferAsync(Id docId)
        {
            buffers.Remove(LibraryIds.Hex(docId));
            return Task.CompletedTask;
        }

        public Task<EditorBufferReply> GetEditorBufferAsync(Id docId) =>
            Task.FromResult(buffers.TryGetValue(LibraryIds.Hex(docId), out var buf)
                ? new EditorBufferReply { Found = true, Buffer = buf }
                : new EditorBufferReply { Found = false });

        public Task<UndoState> GetUndoStateAsync(Id? docId = null) =>
            Task.FromResult(new UndoState
            {
                CanUndo = undoStack.Count > 0,
                CanRedo = redoStack.Count > 0,
                UndoDepth = (uint)undoStack.Count,
                RedoDepth = (uint)redoStack.Count,
            });

        public Task UndoAsync(Id? docId = null)
        {
            if (undoStack.Count == 0)
            {
                throw new CoreException("SEL-LIB-0004", "error-nothing-to-undo");
            }
            redoStack.Push(CloneSongs());
            Songs.Clear();
            Songs.AddRange(undoStack.Pop());
            RaiseChanged(DocumentType.Song, docId ?? LibraryIds.NewId());
            return Task.CompletedTask;
        }

        public Task RedoAsync(Id? docId = null)
        {
            if (redoStack.Count == 0)
            {
                throw new CoreException("SEL-LIB-0004", "error-nothing-to-undo");
            }
            undoStack.Push(CloneSongs());
            Songs.Clear();
            Songs.AddRange(redoStack.Pop());
            RaiseChanged(DocumentType.Song, docId ?? LibraryIds.NewId());
            return Task.CompletedTask;
        }

        public Task<IReadOnlyList<BibleTranslation>> ListBiblesAsync(bool includeBooks = false) =>
            Task.FromResult<IReadOnlyList<BibleTranslation>>(Bibles);

        public Task<BibleTranslation> ImportBibleAsync(ImportBible request) =>
            Task.FromResult(new BibleTranslation { Code = request.Code, Name = request.Name, Language = "en" });

        public Task DeleteBibleAsync(string code) => Task.CompletedTask;

        public Task<ParseScriptureReferenceReply> ParseScriptureReferenceAsync(string reference, string languageHint = "") =>
            Task.FromResult(new ParseScriptureReferenceReply { Normalized = "John 3:16-18" });

        public Task<ScripturePassages> GetScripturePassagesAsync(string translation, string reference, string languageHint = "")
        {
            var code = string.IsNullOrWhiteSpace(translation) ? "KJV" : translation;
            var passages = new ScripturePassages { Translation = Bibles.First(b => b.Code == code), NormalizedReference = reference };
            passages.Verses.Add(new ScriptureVerse { Translation = code, BookCode = "JHN", BookName = "John", Chapter = 3, Verse = 16, Text = "For God so loved the world, that he gave his only begotten Son." });
            passages.Verses.Add(new ScriptureVerse { Translation = code, BookCode = "JHN", BookName = "John", Chapter = 3, Verse = 17, Text = "For God sent not his Son into the world to condemn the world." });
            passages.Verses.Add(new ScriptureVerse { Translation = code, BookCode = "JHN", BookName = "John", Chapter = 3, Verse = 18, Text = "He that believeth on him is not condemned." });
            return Task.FromResult(passages);
        }

        public Task<ScriptureSearchResults> SearchScriptureAsync(string query, string translation = "", uint limit = 50, uint offset = 0)
        {
            var results = new ScriptureSearchResults();
            if (query.Contains("loved", StringComparison.OrdinalIgnoreCase))
            {
                results.Hits.Add(new ScriptureVerseHit { Translation = "KJV", Book = "JHN", BookName = "John", Chapter = 3, Verse = 16, Text = "For God so loved the world...", Score = 1.0 });
            }
            results.Total = (uint)results.Hits.Count;
            return Task.FromResult(results);
        }

        public Task<GeneratedScriptureSlides> SplitScriptureSlidesAsync(SplitScriptureSlides request)
        {
            var reply = new GeneratedScriptureSlides { Translation = request.Translation, ReferenceLabel = request.Reference };
            if (request.TitleSlide)
            {
                reply.Slides.Add(new GeneratedScriptureSlide { Reference = request.Reference, IsTitle = true, Lines = { request.Reference } });
            }
            var max = (int)(request.MaxLines == 0 ? 4 : request.MaxLines);
            for (var i = 0; i < request.Verses.Count; i += max)
            {
                var slice = request.Verses.Skip(i).Take(max).ToList();
                var slide = new GeneratedScriptureSlide { Reference = request.Reference };
                foreach (var v in slice)
                {
                    slide.Lines.Add(request.ShowVerseNumbers ? $"{v.Verse} {v.Text}" : v.Text);
                }
                if (request.ReferencePlacement == ReferencePlacement.SlideHeader)
                {
                    slide.HeaderReference = request.Reference;
                }
                else if (request.ReferencePlacement == ReferencePlacement.SlideFooter)
                {
                    slide.FooterReference = request.Reference;
                }
                reply.Slides.Add(slide);
            }
            if (request.ShrinkToFit || request.TargetOutputs.Count > 0 || request.TemplateId is not null || !string.IsNullOrEmpty(request.StyleJson))
            {
                var style = ResolveTemplateStyle(request.TemplateId, "templates.default.scripture");
                var body = style.Roles.Body;
                var applied = body.Size > 0 ? body.Size : 64f;
                var minSize = body.MinSize > 0 ? body.MinSize : 28f;
                reply.FitReports.Add(new OutputFitReport
                {
                    OutputIndex = 0,
                    OutputName = "Main",
                    AppliedSize = applied,
                    MinSize = minSize,
                    MatchedVariant = "16:9",
                    Fits = true,
                    Summary = $"Fits at {applied:0} pt",
                });
                var fitted = new OutputFittedScriptureSlides { OutputIndex = 0 };
                foreach (var s in reply.Slides)
                {
                    fitted.Slides.Add(s.Clone());
                }
                reply.PerOutputSlides.Add(fitted);
            }
            return Task.FromResult(reply);
        }

        public Task<IReadOnlyList<MediaItem>> ListMediaItemsAsync(uint limit = 200, uint offset = 0) =>
            Task.FromResult<IReadOnlyList<MediaItem>>(Media);

        public Task<MediaSearchResults> SearchMediaAsync(string query, MediaKind kind = MediaKind.Unspecified, uint limit = 50, uint offset = 0)
        {
            var results = new MediaSearchResults();
            foreach (var m in Media.Where(m =>
                         (kind == MediaKind.Unspecified || m.Kind == kind)
                         && (string.IsNullOrEmpty(query) || m.Name.Contains(query, StringComparison.OrdinalIgnoreCase))))
            {
                results.Hits.Add(new MediaSearchHit { MediaItem = m, Score = 1.0 });
            }
            results.Total = (uint)results.Hits.Count;
            return Task.FromResult(results);
        }

        public Task<MediaItem> ImportMediaAsync(string sourcePath, string name = "", IEnumerable<string>? tags = null)
        {
            var bytes = File.ReadAllBytes(sourcePath);
            var id = LibraryIds.NewId();
            var item = new MediaItem
            {
                Id = id,
                Name = string.IsNullOrWhiteSpace(name) ? Path.GetFileNameWithoutExtension(sourcePath) : name,
                Kind = MediaKind.Image,
                Extension = Path.GetExtension(sourcePath),
                SizeBytes = (ulong)bytes.Length,
                Resolution = new Resolution { Width = 32, Height = 18 },
            };
            Media.Add(item);
            Thumbnails[LibraryIds.Hex(id)] = bytes;
            RaiseChanged(DocumentType.Media, id);
            return Task.FromResult(item);
        }

        public Task DeleteMediaItemAsync(Id id)
        {
            Media.RemoveAll(m => LibraryIds.Same(m.Id, id));
            Thumbnails.Remove(LibraryIds.Hex(id));
            return Task.CompletedTask;
        }

        public Task<MediaThumbnail> GetMediaThumbnailAsync(Id mediaId, uint maxWidth = 320, uint maxHeight = 180)
        {
            Thumbnails.TryGetValue(LibraryIds.Hex(mediaId), out var bytes);
            return Task.FromResult(new MediaThumbnail
            {
                MediaId = mediaId,
                Width = 32,
                Height = 18,
                PngBytes = ByteString.CopyFrom(bytes ?? []),
            });
        }

        public Task<SceneThumbnail> RenderSceneThumbnailAsync(Scene scene, uint outputIndex = 0, uint maxWidth = 320, uint maxHeight = 180)
        {
            var res = scene.Resolution ?? new Resolution { Width = 1920, Height = 1080 };
            var scale = Math.Min((double)maxWidth / Math.Max(1u, res.Width), (double)maxHeight / Math.Max(1u, res.Height));
            var thumbW = Math.Clamp((int)Math.Round(res.Width * scale), 16, 1024);
            var thumbH = Math.Clamp((int)Math.Round(res.Height * scale), 16, 1024);
            return Task.FromResult(new SceneThumbnail
            {
                PngBytes = ByteString.CopyFrom(CreateValidPngBytes(thumbW, thumbH)),
                Width = (uint)thumbW,
                Height = (uint)thumbH,
                CacheKey = $"fake-{scene.Resolution?.Width}x{scene.Resolution?.Height}",
            });
        }

        public Task StagePreviewSceneAsync(Scene scene)
        {
            StagedScenes.Add(scene.Clone());
            return Task.CompletedTask;
        }

        public Task<ExportedSelahBundle> ExportSelahBundleAsync(ExportSelahBundle request) =>
            Task.FromResult(new ExportedSelahBundle { OutputPath = request.OutputPath, SongCount = (uint)Songs.Count });

        public Task<ImportedSelahBundle> ImportSelahBundleAsync(ImportSelahBundle request) =>
            Task.FromResult(new ImportedSelahBundle { SongCount = (uint)Songs.Count });

        public Task<LibraryBackupInfo> CreateLibraryBackupAsync(string outputPath = "") =>
            Task.FromResult(new LibraryBackupInfo { BackupPath = outputPath, SchemaVersion = 1 });

        public Task<RestoreLibraryBackupResult> RestoreLibraryBackupAsync(string backupPath) =>
            Task.FromResult(new RestoreLibraryBackupResult { SchemaVersion = 1, SongCount = (uint)Songs.Count });

        public Task<IReadOnlyList<Tag>> ListTagsAsync(uint limit = 200, uint offset = 0) =>
            Task.FromResult<IReadOnlyList<Tag>>([]);

        public Task SaveTagAsync(Tag tag) => Task.CompletedTask;

        public Task<LibrarySearchResults> SearchLibraryAsync(string query, SearchScope scope = SearchScope.All, string translation = "", uint limit = 20)
        {
            var results = new LibrarySearchResults();
            foreach (var s in Songs.Where(s => s.Title.Contains(query, StringComparison.OrdinalIgnoreCase)))
            {
                results.Songs.Add(new SongSearchHit { Song = s.Clone(), Score = 1.0, Snippet = s.Key });
            }
            return Task.FromResult(results);
        }

        public Task<IReadOnlyList<Template>> ListTemplatesAsync(uint limit = 200, uint offset = 0) =>
            Task.FromResult<IReadOnlyList<Template>>(Templates.Select(t => t.Clone()).ToList());

        public Task<Template?> GetTemplateAsync(Id id) =>
            Task.FromResult(Templates.FirstOrDefault(t => LibraryIds.Same(t.Id, id))?.Clone());

        public Task SaveTemplateAsync(Template template, Id? undoGroup = null)
        {
            var idx = Templates.FindIndex(t => LibraryIds.Same(t.Id, template.Id));
            if (idx >= 0)
            {
                Templates[idx] = template.Clone();
            }
            else
            {
                Templates.Add(template.Clone());
            }
            RaiseChanged(DocumentType.Template, template.Id);
            return Task.CompletedTask;
        }

        public Task DeleteTemplateAsync(Id id, Id? undoGroup = null)
        {
            Templates.RemoveAll(t => LibraryIds.Same(t.Id, id));
            RaiseChanged(DocumentType.Template, id);
            return Task.CompletedTask;
        }

        public Task<IReadOnlyList<Presentation>> ListPresentationsAsync(uint limit = 200, uint offset = 0) =>
            Task.FromResult<IReadOnlyList<Presentation>>(Presentations.Select(p => p.Clone()).ToList());

        public Task<Presentation?> GetPresentationAsync(Id id) =>
            Task.FromResult(Presentations.FirstOrDefault(p => LibraryIds.Same(p.Id, id))?.Clone());

        public Task SavePresentationAsync(Presentation presentation, Id? undoGroup = null)
        {
            var idx = Presentations.FindIndex(p => LibraryIds.Same(p.Id, presentation.Id));
            if (idx >= 0)
            {
                Presentations[idx] = presentation.Clone();
            }
            else
            {
                Presentations.Add(presentation.Clone());
            }
            RaiseChanged(DocumentType.Presentation, presentation.Id);
            return Task.CompletedTask;
        }

        public Task DeletePresentationAsync(Id id, Id? undoGroup = null)
        {
            Presentations.RemoveAll(p => LibraryIds.Same(p.Id, id));
            RaiseChanged(DocumentType.Presentation, id);
            return Task.CompletedTask;
        }

        public Task<IReadOnlyList<OutputConfig>> GetOutputConfigsAsync() =>
            Task.FromResult<IReadOnlyList<OutputConfig>>(Outputs.Select(o => o.Clone()).ToList());

        public Task SetOutputConfigsAsync(IEnumerable<OutputConfig> outputs)
        {
            Outputs.Clear();
            foreach (var o in outputs)
            {
                Outputs.Add(o.Clone());
            }
            return Task.CompletedTask;
        }

        public Task<GetPolicySettingReply> GetPolicySettingAsync(string key) =>
            Task.FromResult(Policies.TryGetValue(key, out var s)
                ? new GetPolicySettingReply { Found = true, Setting = s.Clone() }
                : new GetPolicySettingReply { Found = false });

        public Task SetPolicySettingAsync(string key, string valueJson, PolicyLevel level = PolicyLevel.User, bool locked = false)
        {
            Policies[key] = new PolicySetting { Key = key, ValueJson = valueJson, Level = level, Locked = locked };
            return Task.CompletedTask;
        }

        public Task<FlashRiskReport> AnalyzeFlashRiskAsync(AnalyzeFlashRisk request)
        {
            var fps = request.RepeatCadenceMs > 0 ? 1000f / request.RepeatCadenceMs : request.FlashesPerSecondHint;
            var flash = fps > 3.0f || request.RepeatCadenceMs is > 0 and <= 333;
            return Task.FromResult(new FlashRiskReport
            {
                FlashWarning = flash,
                FlashesPerSecond = fps,
                Reason = flash ? "Rapid flash rate exceeds 3 flashes per second (WCAG 2.3.1)." : string.Empty,
                WarningCodes = { flash ? "SAFE-04-FLASH" : string.Empty },
            });
        }

        public Task PresentSceneAsync(Scene scene)
        {
            StagedScenes.Add(scene.Clone());
            return Task.CompletedTask;
        }

        public Task<IReadOnlyList<Service>> ListServicesAsync(uint limit = 200, uint offset = 0) =>
            Task.FromResult<IReadOnlyList<Service>>(Services.Select(s => s.Clone()).ToList());

        public Task<Service?> GetServiceAsync(Id id) =>
            Task.FromResult(Services.FirstOrDefault(s => LibraryIds.Same(s.Id, id))?.Clone());

        public Task SaveServiceAsync(Service service, Id? undoGroup = null)
        {
            var idx = Services.FindIndex(s => LibraryIds.Same(s.Id, service.Id));
            if (idx >= 0)
            {
                Services[idx] = service.Clone();
            }
            else
            {
                Services.Add(service.Clone());
            }
            RaiseChanged(DocumentType.Service, service.Id);
            return Task.CompletedTask;
        }

        public Task DeleteServiceAsync(Id id, Id? undoGroup = null)
        {
            Services.RemoveAll(s => LibraryIds.Same(s.Id, id));
            RaiseChanged(DocumentType.Service, id);
            return Task.CompletedTask;
        }

        public Task<Service> CreateServiceFromPreviousAsync(CreateServiceFromPrevious request, Id? undoGroup = null)
        {
            var source = Services.FirstOrDefault(s => LibraryIds.Same(s.Id, request.SourceServiceId)) ?? Services.FirstOrDefault();
            var created = source is not null ? source.Clone() : new Service();
            created.Id = LibraryIds.NewId();
            created.Name = string.IsNullOrWhiteSpace(request.Name) ? "Sunday Service (Inherited)" : request.Name;
            created.SourceServiceId = source?.Id;
            foreach (var item in created.Items)
            {
                item.Id = LibraryIds.NewId();
            }
            Services.Add(created.Clone());
            RaiseChanged(DocumentType.Service, created.Id);
            return Task.FromResult(created);
        }

        public Task<IReadOnlyList<ServiceType>> ListServiceTypesAsync(uint limit = 200, uint offset = 0) =>
            Task.FromResult<IReadOnlyList<ServiceType>>(ServiceTypes.Select(t => t.Clone()).ToList());

        public Task<ServiceType?> GetServiceTypeAsync(Id id) =>
            Task.FromResult(ServiceTypes.FirstOrDefault(t => LibraryIds.Same(t.Id, id))?.Clone());

        public Task SaveServiceTypeAsync(ServiceType serviceType, Id? undoGroup = null)
        {
            var idx = ServiceTypes.FindIndex(t => LibraryIds.Same(t.Id, serviceType.Id));
            if (idx >= 0)
            {
                ServiceTypes[idx] = serviceType.Clone();
            }
            else
            {
                ServiceTypes.Add(serviceType.Clone());
            }
            RaiseChanged(DocumentType.ServiceType, serviceType.Id);
            return Task.CompletedTask;
        }

        public Task DeleteServiceTypeAsync(Id id, Id? undoGroup = null)
        {
            ServiceTypes.RemoveAll(t => LibraryIds.Same(t.Id, id));
            RaiseChanged(DocumentType.ServiceType, id);
            return Task.CompletedTask;
        }

        public Task<IReadOnlyList<Team>> ListTeamsAsync(uint limit = 200, uint offset = 0) =>
            Task.FromResult<IReadOnlyList<Team>>(Teams.Select(t => t.Clone()).ToList());

        public Task<Team?> GetTeamAsync(Id id) =>
            Task.FromResult(Teams.FirstOrDefault(t => LibraryIds.Same(t.Id, id))?.Clone());

        public Task SaveTeamAsync(Team team, Id? undoGroup = null)
        {
            var idx = Teams.FindIndex(t => LibraryIds.Same(t.Id, team.Id));
            if (idx >= 0)
            {
                Teams[idx] = team.Clone();
            }
            else
            {
                Teams.Add(team.Clone());
            }
            RaiseChanged(DocumentType.Team, team.Id);
            return Task.CompletedTask;
        }

        public Task DeleteTeamAsync(Id id, Id? undoGroup = null)
        {
            Teams.RemoveAll(t => LibraryIds.Same(t.Id, id));
            RaiseChanged(DocumentType.Team, id);
            return Task.CompletedTask;
        }

        public Task<ServiceRuntimeProjection> ProjectServiceRuntimeAsync(ProjectServiceRuntime request)
        {
            var svc = request.Service ?? Services.FirstOrDefault(s => LibraryIds.Same(s.Id, request.ServiceId)) ?? new Service();
            var baseSecs = svc.StartsAt is { UnixMilliseconds: > 0 } ts ? (int)((ts.UnixMilliseconds / 1000) % 86400) : 9 * 3600;
            var offset = 0u;
            var proj = new ServiceRuntimeProjection();
            for (var i = 0; i < svc.Items.Count; i++)
            {
                var item = svc.Items[i];
                var isHeader = item.Kind == ServiceItemKind.Header;
                var cur = (int)((baseSecs + offset) % 86400);
                var dur = isHeader ? 0u : (item.DurationSeconds > 0 ? item.DurationSeconds : 240u);
                proj.Items.Add(new ProjectedServiceItem
                {
                    ItemId = item.Id,
                    Index = (uint)i,
                    OffsetSeconds = offset,
                    DurationSeconds = dur,
                    FormattedTime = isHeader ? "—" : $"{cur / 3600:D2}:{(cur % 3600) / 60:D2}",
                    FormattedDuration = isHeader ? "—" : $"{dur / 60}:{dur % 60:D2}",
                    IsHeader = isHeader,
                });
                offset += dur;
            }
            var endSecs = (int)((baseSecs + offset) % 86400);
            proj.TotalDurationSeconds = offset;
            proj.FormattedStartTime = $"{baseSecs / 3600:D2}:{(baseSecs % 3600) / 60:D2}";
            proj.FormattedEndTime = $"{endSecs / 3600:D2}:{(endSecs % 3600) / 60:D2}";
            var mins = (offset + 59) / 60;
            proj.FormattedTotalDuration = mins >= 60 ? $"{mins / 60} hr {mins % 60} min" : $"{mins} min";
            return Task.FromResult(proj);
        }

        public Task<ServicePrintExport> ExportServicePrintAsync(ExportServicePrint request)
        {
            var svc = Services.FirstOrDefault(s => LibraryIds.Same(s.Id, request.ServiceId)) ?? new Service { Name = "Sunday Service" };
            var html = $"<!DOCTYPE html><html><body><h1>{svc.Name}</h1><p>Amazing Grace (Key: G)</p><pre>G   C   G\nAmazing grace! How sweet the sound</pre></body></html>";
            var pdf = System.Text.Encoding.ASCII.GetBytes("%PDF-1.4\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n%%EOF\n" + new string(' ', 120));
            return Task.FromResult(new ServicePrintExport
            {
                Path = request.OutputPath,
                Html = html,
                PdfBytes = ByteString.CopyFrom(pdf),
                PageCount = 2,
            });
        }

        private void RaiseChanged(DocumentType docType, Id id)
        {
            LibraryChanged?.Invoke(this, new LibraryChanged { DocType = docType, DocId = id, Change = LibraryChangeKind.Saved });
            UndoStateChanged?.Invoke(this, new UndoStateChanged
            {
                DocId = id,
                Global = new UndoState { CanUndo = undoStack.Count > 0, CanRedo = redoStack.Count > 0 },
                Document = new UndoState { CanUndo = undoStack.Count > 0, CanRedo = redoStack.Count > 0 },
            });
        }
    }
}
