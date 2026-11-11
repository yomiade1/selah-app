using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.Documents;
using Avalonia.Headless;
using Avalonia.Headless.XUnit;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Styling;
using Avalonia.Threading;
using Avalonia.VisualTree;
using Selah.Contracts.V1;
using Selah.UI.Services;
using Selah.UI.ViewModels;
using Selah.UI.Views;

namespace Selah.UI.Tests;

/// <summary>
/// M2-A8: Contextual help UI tests (KNOWLEDGE-BASE §5.2–5.4): F1 context panel across three screens,
/// help search and native article rendering, What's this? (Shift+F1) without firing live actions,
/// local "Did this help?" feedback, InfoButton / EmptyState / ErrorAlert guides, and live safety
/// (F12 bypasses search focus; help never covers Clear everything or preview).
/// </summary>
public sealed class ContextualHelpTests : IDisposable
{
    private readonly Reset reset = new();

    public void Dispose() => reset.Dispose();

    public static TheoryData<int, string, string> AppearanceMatrix() =>
        new()
        {
            { 100, "light", "en" },
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

    private static T ControlByName<T>(Control root, string name) where T : Control =>
        root.GetVisualDescendants().OfType<T>().Single(c => c.Name == name);

    private static Rect BoundsInWindow(Visual visual, Window window)
    {
        var topLeft = visual.TranslatePoint(default, window) ?? default;
        var bottomRight = visual.TranslatePoint(new Point(visual.Bounds.Width, visual.Bounds.Height), window) ?? topLeft;
        return new Rect(topLeft, bottomRight);
    }

    private static bool RectsOverlap(Rect a, Rect b) =>
        a.Width > 0 && a.Height > 0 && b.Width > 0 && b.Height > 0 && a.Intersects(b);

    private static IEnumerable<string> Runs(Control root) =>
        root.GetVisualDescendants().OfType<TextBlock>().SelectMany(t => t.Inlines is { } inlines ? Flatten(inlines) : [t.Text ?? string.Empty]);

    private static IEnumerable<string> Flatten(InlineCollection inlines)
    {
        foreach (var inline in inlines)
        {
            switch (inline)
            {
                case Run run:
                    yield return run.Text ?? string.Empty;
                    break;
                case Span span:
                    foreach (var text in Flatten(span.Inlines))
                    {
                        yield return text;
                    }
                    break;
            }
        }
    }

    private sealed class FakeContextHelp : IHelpSource
    {
        private static readonly string Ast = File.ReadAllText(Repo.PathOf("tests", "fixtures", "kb", "show-a-song.ast.json"));

        private readonly Dictionary<string, Article> articles;
        private readonly Dictionary<string, string[]> anchorMap;

        public List<(string ArticleId, bool Helpful)> Feedback { get; } = [];
        public List<IReadOnlyList<string>> ContextRequests { get; } = [];

        public FakeContextHelp()
        {
            articles = new Dictionary<string, Article>(StringComparer.Ordinal)
            {
                ["getting-started.show-test-pattern-and-clear-screens"] = new Article
                {
                    Id = "getting-started.show-test-pattern-and-clear-screens",
                    Title = "Show a test pattern and clear screens",
                    Summary = "Put colour bars on every screen to check cables, and clear every screen right away with Clear everything.",
                    AstJson = Ast,
                },
                ["getting-started.connection-status"] = new Article
                {
                    Id = "getting-started.connection-status",
                    Title = "Check the connection status",
                    Summary = "Read the status pill in the toolbar to verify core and screen connections.",
                    AstJson = Ast,
                },
                ["songs.add-a-song-by-pasting"] = new Article
                {
                    Id = "songs.add-a-song-by-pasting",
                    Title = "Add a song by pasting",
                    Summary = "Open the Song editor, paste your lyrics or ChordPro words, and let Selah split them into sections and slides.",
                    AstJson = Ast,
                },
                ["songs.edit-sections"] = new Article
                {
                    Id = "songs.edit-sections",
                    Title = "Edit song sections and slides",
                    Summary = "Change section labels, split long verses into multiple slides, and inspect slide cards.",
                    AstJson = Ast,
                },
                ["songs.chords-and-chord-charts"] = new Article
                {
                    Id = "songs.chords-and-chord-charts",
                    Title = "Chords and chord charts",
                    Summary = "Transpose a song key, toggle flats or sharps, and toggle chords on slide previews.",
                    AstJson = Ast,
                },
                ["media.add-pictures-and-videos"] = new Article
                {
                    Id = "media.add-pictures-and-videos",
                    Title = "Add pictures and videos",
                    Summary = "Import backgrounds into Library -> Media, filter by type, and delete unused media.",
                    AstJson = Ast,
                },
                ["scripture.add-scripture-to-a-service"] = new Article
                {
                    Id = "scripture.add-scripture-to-a-service",
                    Title = "Add scripture to a service",
                    Summary = "Look up Bible references, choose verse splitting options, and build scripture slides.",
                    AstJson = Ast,
                },
                ["library.bundles-and-backups"] = new Article
                {
                    Id = "library.bundles-and-backups",
                    Title = "Share songs and back up your library",
                    Summary = "Export or import .selah bundles and create or restore full library database backups.",
                    AstJson = Ast,
                },
                ["getting-started.appearance-and-text-size"] = new Article
                {
                    Id = "getting-started.appearance-and-text-size",
                    Title = "Change appearance and text size",
                    Summary = "Switch between Light and Booth dark appearance and scale interface text size.",
                    AstJson = Ast,
                },
                ["getting-started.run-diagnostics"] = new Article
                {
                    Id = "getting-started.run-diagnostics",
                    Title = "Run diagnostics",
                    Summary = "Inspect hardware, timing, and diagnostic logs.",
                    AstJson = Ast,
                },
                ["troubleshooting.cant-save-library-item"] = new Article
                {
                    Id = "troubleshooting.cant-save-library-item",
                    Title = "Selah can't save a library item",
                    Summary = "Check required fields, inline chord syntax, and free disk space.",
                    AstJson = Ast,
                },
            };

            anchorMap = new Dictionary<string, string[]>(StringComparer.Ordinal)
            {
                [HelpAnchors.ShellPreview] = ["getting-started.show-test-pattern-and-clear-screens"],
                [HelpAnchors.ShellShowTestPattern] = ["getting-started.show-test-pattern-and-clear-screens"],
                [HelpAnchors.ShellClearEverything] = ["getting-started.show-test-pattern-and-clear-screens"],
                [HelpAnchors.ShellStatus] = ["getting-started.connection-status"],
                [HelpAnchors.LibrarySongs] = ["songs.add-a-song-by-pasting"],
                [HelpAnchors.LibraryExchange] = ["library.bundles-and-backups"],
                [HelpAnchors.LibraryMedia] = ["media.add-pictures-and-videos"],
                [HelpAnchors.LibraryBibles] = ["scripture.add-scripture-to-a-service"],
                [HelpAnchors.EditorPaste] = ["songs.add-a-song-by-pasting"],
                [HelpAnchors.EditorSections] = ["songs.edit-sections"],
                [HelpAnchors.EditorSlides] = ["songs.edit-sections"],
                [HelpAnchors.EditorInspector] = ["songs.chords-and-chord-charts"],
                [HelpAnchors.SettingsAppearance] = ["getting-started.appearance-and-text-size"],
                [HelpAnchors.HelpDiagnostics] = ["getting-started.run-diagnostics"],
            };
        }

        public Task<IReadOnlyList<Article>> HelpForContextAsync(IEnumerable<string> anchors)
        {
            var anchorList = anchors.ToList();
            ContextRequests.Add(anchorList);
            var seen = new HashSet<string>(StringComparer.Ordinal);
            var outList = new List<Article>();
            foreach (var anchor in anchorList)
            {
                if (!anchorMap.TryGetValue(anchor, out var ids))
                {
                    continue;
                }
                foreach (var id in ids)
                {
                    if (seen.Add(id) && articles.TryGetValue(id, out var article))
                    {
                        outList.Add(article.Clone());
                    }
                }
            }
            return Task.FromResult<IReadOnlyList<Article>>(outList);
        }

        public Task<IReadOnlyList<Article>> SearchHelpAsync(string query, IEnumerable<string> anchors)
        {
            var matched = articles.Values
                .Where(a =>
                    a.Title.Contains(query, StringComparison.OrdinalIgnoreCase)
                    || a.Summary.Contains(query, StringComparison.OrdinalIgnoreCase)
                    || a.Id.Contains(query, StringComparison.OrdinalIgnoreCase))
                .Select(a => a.Clone())
                .ToList();
            return Task.FromResult<IReadOnlyList<Article>>(matched);
        }

        public Task<Article?> GetArticleAsync(string id) =>
            Task.FromResult(articles.TryGetValue(id, out var article) ? article.Clone() : null);

        public Task RecordFeedbackAsync(string articleId, bool helpful)
        {
            Feedback.Add((articleId, helpful));
            return Task.CompletedTask;
        }
    }

    private sealed class EmptyFakeLibrary : ILibraryService
    {
        public event EventHandler<LibraryChanged>? LibraryChanged { add { } remove { } }
        public event EventHandler<UndoStateChanged>? UndoStateChanged { add { } remove { } }

        public Task<IReadOnlyList<Song>> ListSongsAsync(uint limit = 200, uint offset = 0) =>
            Task.FromResult<IReadOnlyList<Song>>([]);

        public Task<SongSearchResults> SearchSongsAsync(string query, uint limit = 50, uint offset = 0) =>
            Task.FromResult(new SongSearchResults());

        public Task<Song?> GetSongAsync(Id id) => Task.FromResult<Song?>(null);
        public Task SaveSongAsync(Song song, Id? undoGroup = null) => Task.CompletedTask;
        public Task DeleteSongAsync(Id id, Id? undoGroup = null) => Task.CompletedTask;
        public Task<ParsePastedSongReply> ParsePastedSongAsync(string rawText, SongPasteFormat format = SongPasteFormat.Unspecified, string defaultTitle = "") =>
            Task.FromResult(new ParsePastedSongReply());
        public Task<Song> TransposeSongAsync(Song song, int semitones, AccidentalSpelling spelling = AccidentalSpelling.Unspecified) =>
            Task.FromResult(song);
        public Task<GeneratedSongSlides> SplitSongSlidesAsync(SplitSongSlides request) =>
            Task.FromResult(new GeneratedSongSlides());
        public Task SaveEditorBufferAsync(Id docId, DocumentType docType, string content) => Task.CompletedTask;
        public Task DiscardEditorBufferAsync(Id docId) => Task.CompletedTask;
        public Task<EditorBufferReply> GetEditorBufferAsync(Id docId) => Task.FromResult(new EditorBufferReply());
        public Task<UndoState> GetUndoStateAsync(Id? docId = null) => Task.FromResult(new UndoState());
        public Task UndoAsync(Id? docId = null) => Task.CompletedTask;
        public Task RedoAsync(Id? docId = null) => Task.CompletedTask;
        public Task<IReadOnlyList<BibleTranslation>> ListBiblesAsync(bool includeBooks = false) =>
            Task.FromResult<IReadOnlyList<BibleTranslation>>([]);
        public Task<BibleTranslation> ImportBibleAsync(ImportBible request) =>
            Task.FromResult(new BibleTranslation());
        public Task DeleteBibleAsync(string code) => Task.CompletedTask;
        public Task<ParseScriptureReferenceReply> ParseScriptureReferenceAsync(string reference, string languageHint = "") =>
            Task.FromResult(new ParseScriptureReferenceReply());
        public Task<ScripturePassages> GetScripturePassagesAsync(string translation, string reference, string languageHint = "") =>
            Task.FromResult(new ScripturePassages());
        public Task<ScriptureSearchResults> SearchScriptureAsync(string query, string translation = "", uint limit = 50, uint offset = 0) =>
            Task.FromResult(new ScriptureSearchResults());
        public Task<GeneratedScriptureSlides> SplitScriptureSlidesAsync(SplitScriptureSlides request) =>
            Task.FromResult(new GeneratedScriptureSlides());
        public Task<IReadOnlyList<MediaItem>> ListMediaItemsAsync(uint limit = 200, uint offset = 0) =>
            Task.FromResult<IReadOnlyList<MediaItem>>([]);
        public Task<MediaSearchResults> SearchMediaAsync(string query, MediaKind kind = MediaKind.Unspecified, uint limit = 50, uint offset = 0) =>
            Task.FromResult(new MediaSearchResults());
        public Task<MediaItem> ImportMediaAsync(string sourcePath, string name = "", IEnumerable<string>? tags = null) =>
            Task.FromResult(new MediaItem());
        public Task DeleteMediaItemAsync(Id id) => Task.CompletedTask;
        public Task<MediaThumbnail> GetMediaThumbnailAsync(Id mediaId, uint maxWidth = 320, uint maxHeight = 180) =>
            Task.FromResult(new MediaThumbnail());
        public Task<SceneThumbnail> RenderSceneThumbnailAsync(Scene scene, uint outputIndex = 0, uint maxWidth = 320, uint maxHeight = 180) =>
            Task.FromResult(new SceneThumbnail());
        public Task StagePreviewSceneAsync(Scene scene) => Task.CompletedTask;
        public Task<ExportedSelahBundle> ExportSelahBundleAsync(ExportSelahBundle request) =>
            Task.FromResult(new ExportedSelahBundle());
        public Task<ImportedSelahBundle> ImportSelahBundleAsync(ImportSelahBundle request) =>
            Task.FromResult(new ImportedSelahBundle());
        public Task<LibraryBackupInfo> CreateLibraryBackupAsync(string outputPath = "") =>
            Task.FromResult(new LibraryBackupInfo());
        public Task<RestoreLibraryBackupResult> RestoreLibraryBackupAsync(string backupPath) =>
            Task.FromResult(new RestoreLibraryBackupResult());
        public Task<IReadOnlyList<Tag>> ListTagsAsync(uint limit = 200, uint offset = 0) =>
            Task.FromResult<IReadOnlyList<Tag>>([]);
        public Task SaveTagAsync(Tag tag) => Task.CompletedTask;
        public Task<LibrarySearchResults> SearchLibraryAsync(string query, SearchScope scope = SearchScope.All, string translation = "", uint limit = 20) =>
            Task.FromResult(new LibrarySearchResults());
        public Task<IReadOnlyList<Template>> ListTemplatesAsync(uint limit = 200, uint offset = 0) =>
            Task.FromResult<IReadOnlyList<Template>>([]);
        public Task<Template?> GetTemplateAsync(Id id) => Task.FromResult<Template?>(null);
        public Task SaveTemplateAsync(Template template, Id? undoGroup = null) => Task.CompletedTask;
        public Task DeleteTemplateAsync(Id id, Id? undoGroup = null) => Task.CompletedTask;
        public Task<IReadOnlyList<Presentation>> ListPresentationsAsync(uint limit = 200, uint offset = 0) =>
            Task.FromResult<IReadOnlyList<Presentation>>([]);
        public Task<Presentation?> GetPresentationAsync(Id id) => Task.FromResult<Presentation?>(null);
        public Task SavePresentationAsync(Presentation presentation, Id? undoGroup = null) => Task.CompletedTask;
        public Task DeletePresentationAsync(Id id, Id? undoGroup = null) => Task.CompletedTask;
        public Task<IReadOnlyList<OutputConfig>> GetOutputConfigsAsync() =>
            Task.FromResult<IReadOnlyList<OutputConfig>>([]);
        public Task SetOutputConfigsAsync(IEnumerable<OutputConfig> outputs) => Task.CompletedTask;
        public Task<GetPolicySettingReply> GetPolicySettingAsync(string key) =>
            Task.FromResult(new GetPolicySettingReply());
        public Task SetPolicySettingAsync(string key, string valueJson, PolicyLevel level = PolicyLevel.User, bool locked = false) =>
            Task.CompletedTask;
        public Task<FlashRiskReport> AnalyzeFlashRiskAsync(AnalyzeFlashRisk request) =>
            Task.FromResult(new FlashRiskReport());
        public Task PresentSceneAsync(Scene scene) => Task.CompletedTask;
    }

    [AvaloniaTheory]
    [InlineData(100, "light")]
    [InlineData(200, "booth")]
    public void F1_opens_help_panel_with_context_across_three_screens(int scale, string appearance)
    {
        Configure(scale, appearance);
        var help = new FakeContextHelp();
        var model = new MainViewModel(null, null, new EmptyFakeLibrary(), help);
        var window = OpenWindow(model);

        var inspector = ControlByName<Border>(window, "Inspector");
        var helpPanel = ControlByName<HelpPanelView>(window, "HelpPanelControl");
        Assert.False(helpPanel.IsVisible);

        // Screen 1: Preview
        window.KeyPress(Key.F1, RawInputModifiers.None, PhysicalKey.F1, string.Empty);
        Dispatcher.UIThread.RunJobs();
        Assert.True(model.IsHelpPanelOpen);
        Assert.True(helpPanel.IsVisible);
        Assert.False(inspector.IsVisible);
        var previewIds = model.HelpPanel.ContextArticles.Select(a => a.Id).ToList();
        Assert.Contains("getting-started.show-test-pattern-and-clear-screens", previewIds);
        Assert.Contains("getting-started.connection-status", previewIds);
        Assert.DoesNotContain("media.add-pictures-and-videos", previewIds);

        // Screen 2: Library (Songs -> Media -> Bibles)
        model.SelectModeCommand.Execute("Library");
        Dispatcher.UIThread.RunJobs();
        var librarySongsIds = model.HelpPanel.ContextArticles.Select(a => a.Id).ToList();
        Assert.Contains("songs.add-a-song-by-pasting", librarySongsIds);
        Assert.Contains("library.bundles-and-backups", librarySongsIds);

        model.Library.SelectTab("Media");
        Dispatcher.UIThread.RunJobs();
        var libraryMediaIds = model.HelpPanel.ContextArticles.Select(a => a.Id).ToList();
        Assert.Contains("media.add-pictures-and-videos", libraryMediaIds);
        Assert.DoesNotContain("library.bundles-and-backups", libraryMediaIds);

        model.Library.SelectTab("Bibles");
        Dispatcher.UIThread.RunJobs();
        var libraryBibleIds = model.HelpPanel.ContextArticles.Select(a => a.Id).ToList();
        Assert.Contains("scripture.add-scripture-to-a-service", libraryBibleIds);

        // Screen 3: Song Editor (Paste step + Slides + Inspector)
        model.SelectModeCommand.Execute("SongEditor");
        Dispatcher.UIThread.RunJobs();
        var editorIds = model.HelpPanel.ContextArticles.Select(a => a.Id).ToList();
        Assert.Contains("songs.add-a-song-by-pasting", editorIds);
        Assert.Contains("songs.edit-sections", editorIds);
        Assert.Contains("songs.chords-and-chord-charts", editorIds);
        Assert.DoesNotContain("scripture.add-scripture-to-a-service", editorIds);

        // Pressing F1 again closes HelpPanel cleanly.
        window.KeyPress(Key.F1, RawInputModifiers.None, PhysicalKey.F1, string.Empty);
        Dispatcher.UIThread.RunJobs();
        Assert.False(model.IsHelpPanelOpen);
        Assert.False(helpPanel.IsVisible);

        window.Close();
    }

    [AvaloniaTheory]
    [InlineData(100, "light")]
    [InlineData(200, "booth")]
    public void Help_panel_searches_renders_native_article_and_records_feedback(int scale, string appearance)
    {
        Configure(scale, appearance);
        var help = new FakeContextHelp();
        var model = new MainViewModel(null, null, new EmptyFakeLibrary(), help);
        var window = OpenWindow(model);

        window.KeyPress(Key.F1, RawInputModifiers.None, PhysicalKey.F1, string.Empty);
        Dispatcher.UIThread.RunJobs();

        var searchBox = ControlByName<TextBox>(window, "HelpSearchBox");
        searchBox.Focus();
        model.HelpPanel.Query = "ChordPro";
        searchBox.RaiseEvent(new KeyEventArgs
        {
            RoutedEvent = InputElement.KeyDownEvent,
            Key = Key.Enter,
            PhysicalKey = PhysicalKey.Enter,
        });
        Dispatcher.UIThread.RunJobs();

        Assert.True(model.HelpPanel.HasSearchResults);
        Assert.Contains(model.HelpPanel.SearchResults, a => a.Id == "songs.add-a-song-by-pasting");

        model.HelpPanel.SelectedSearchArticle = model.HelpPanel.SearchResults.First(a => a.Id == "songs.add-a-song-by-pasting");
        Dispatcher.UIThread.RunJobs();

        var articleHost = ControlByName<ContentControl>(window, "HelpArticleHost");
        Assert.NotNull(articleHost.Content);
        var renderedTexts = Runs((Control)articleHost.Content!).ToList();
        Assert.Contains("Show a song on the screens", renderedTexts);
        Assert.Contains("Next slide", renderedTexts);

        var yesButton = ControlByName<Button>(window, "HelpFeedbackYesButton");
        var thanksText = ControlByName<TextBlock>(window, "HelpFeedbackThanksText");
        Assert.True(model.HelpPanel.ShowFeedbackButtons);
        yesButton.Command!.Execute(null);
        Dispatcher.UIThread.RunJobs();

        Assert.Single(help.Feedback);
        Assert.Equal(("songs.add-a-song-by-pasting", true), help.Feedback[0]);
        Assert.False(model.HelpPanel.ShowFeedbackButtons);
        Assert.True(thanksText.IsVisible);
        Assert.Equal(Localizer.Instance["help-feedback-thanks"], thanksText.Text);

        // Open a second article and vote "No".
        model.HelpPanel.OpenArticleAsync("songs.edit-sections").GetAwaiter().GetResult();
        Dispatcher.UIThread.RunJobs();
        var noButton = ControlByName<Button>(window, "HelpFeedbackNoButton");
        noButton.Command!.Execute(null);
        Dispatcher.UIThread.RunJobs();

        Assert.Equal(2, help.Feedback.Count);
        Assert.Equal(("songs.edit-sections", false), help.Feedback[1]);

        window.Close();
    }

    [AvaloniaTheory]
    [InlineData(100, "light")]
    [InlineData(200, "booth")]
    public void Whats_this_inspects_without_firing_live_actions_and_esc_dismisses(int scale, string appearance)
    {
        Configure(scale, appearance);
        var help = new FakeContextHelp();
        var model = new MainViewModel(null, null, new EmptyFakeLibrary(), help);
        model.Apply(new ConnectionSnapshot(Connected: true, EngineConnected: true, SafeMode: false, Live: true, Generation: 1));
        var window = OpenWindow(model);

        var showTestPatternClicks = 0;
        var showTestPatternButton = ControlByName<Button>(window, "ShowTestPatternButton");
        showTestPatternButton.Click += (_, _) => showTestPatternClicks++;

        // Activate What's this? via Shift+F1.
        window.KeyPress(Key.F1, RawInputModifiers.Shift, PhysicalKey.F1, string.Empty);
        Dispatcher.UIThread.RunJobs();
        Assert.True(model.IsWhatsThisActive);

        var whatsThis = ControlByName<WhatsThisOverlay>(window, "WhatsThisControl");
        Assert.True(whatsThis.IsVisible);

        // Mouse click over ShowTestPatternButton inspects its anchor instead of firing the live action.
        var stpCenter = BoundsInWindow(showTestPatternButton, window).Center;
        window.MouseDown(stpCenter, MouseButton.Left);
        window.MouseUp(stpCenter, MouseButton.Left);
        Dispatcher.UIThread.RunJobs();

        Assert.Equal(0, showTestPatternClicks);
        Assert.Equal(HelpAnchors.ShellShowTestPattern, whatsThis.InspectedAnchor);
        Assert.True(ControlByName<Border>(whatsThis, "WhatsThisPopoverCard").IsVisible);
        Assert.Equal("Show a test pattern and clear screens", ControlByName<TextBlock>(whatsThis, "WhatsThisPopoverTitle").Text);

        // Clear everything stays directly clickable (not swallowed by What's this?).
        var clearTriggered = 0;
        model.ClearEverythingTriggered += (_, _) => clearTriggered++;
        var clearButton = ControlByName<Button>(window, "ClearEverythingButton");
        var clearCenter = BoundsInWindow(clearButton, window).Center;
        window.MouseDown(clearCenter, MouseButton.Left);
        window.MouseUp(clearCenter, MouseButton.Left);
        Dispatcher.UIThread.RunJobs();
        Assert.Equal(HelpAnchors.ShellShowTestPattern, whatsThis.InspectedAnchor);
        Assert.Equal(1, clearTriggered);

        // Clicking "See guide" opens the article in HelpPanel and closes What's this?.
        var seeGuide = ControlByName<Button>(whatsThis, "WhatsThisSeeGuideButton");
        seeGuide.RaiseEvent(new RoutedEventArgs(Button.ClickEvent));
        Dispatcher.UIThread.RunJobs();

        Assert.False(model.IsWhatsThisActive);
        Assert.True(model.IsHelpPanelOpen);
        Assert.Equal("getting-started.show-test-pattern-and-clear-screens", model.HelpPanel.CurrentArticle?.Id);

        // Escape dismisses What's this?.
        window.KeyPress(Key.F1, RawInputModifiers.Shift, PhysicalKey.F1, string.Empty);
        Dispatcher.UIThread.RunJobs();
        Assert.True(model.IsWhatsThisActive);
        window.KeyPress(Key.Escape, RawInputModifiers.None, PhysicalKey.Escape, string.Empty);
        Dispatcher.UIThread.RunJobs();
        Assert.False(model.IsWhatsThisActive);

        window.Close();
    }

    [AvaloniaTheory]
    [MemberData(nameof(AppearanceMatrix))]
    public void Live_safety_f12_bypasses_help_search_and_help_never_covers_live_controls(int scale, string appearance, string language)
    {
        Configure(scale, appearance, language);
        var help = new FakeContextHelp();
        var model = new MainViewModel(null, null, new EmptyFakeLibrary(), help);
        model.Apply(new ConnectionSnapshot(Connected: true, EngineConnected: true, SafeMode: true, Live: true, Generation: 1));
        var window = OpenWindow(model);

        var clearCount = 0;
        model.ClearEverythingTriggered += (_, _) => clearCount++;

        // Open Help panel AND What's this? simultaneously while live & in safe mode.
        window.KeyPress(Key.F1, RawInputModifiers.None, PhysicalKey.F1, string.Empty);
        window.KeyPress(Key.F1, RawInputModifiers.Shift, PhysicalKey.F1, string.Empty);
        Dispatcher.UIThread.RunJobs();

        var clearButton = ControlByName<Button>(window, "ClearEverythingButton");
        var previewImage = ControlByName<Image>(window, "PreviewImage");
        var helpPanel = ControlByName<HelpPanelView>(window, "HelpPanelControl");
        var whatsThis = ControlByName<WhatsThisOverlay>(window, "WhatsThisControl");
        var searchBox = ControlByName<TextBox>(window, "HelpSearchBox");

        var clearRect = BoundsInWindow(clearButton, window);
        var previewRect = BoundsInWindow(previewImage, window);
        var helpRect = BoundsInWindow(helpPanel, window);
        var whatsThisRect = BoundsInWindow(whatsThis, window);

        Assert.False(RectsOverlap(helpRect, clearRect), $"HelpPanel {helpRect} overlapped ClearEverything {clearRect}");
        Assert.False(RectsOverlap(helpRect, previewRect), $"HelpPanel {helpRect} overlapped PreviewImage {previewRect}");
        Assert.False(RectsOverlap(whatsThisRect, clearRect), $"WhatsThis {whatsThisRect} overlapped ClearEverything {clearRect}");
        Assert.False(RectsOverlap(whatsThisRect, previewRect), $"WhatsThis {whatsThisRect} overlapped PreviewImage {previewRect}");

        // Every critical label stays whole and unclipped when Help panel is open at 100%, 200%, and en-XA.
        var problems = LayoutTests.ClippedCriticalLabels(window);
        problems.Remove($"\"{Localizer.Instance["live-guard-title"]}\" is hidden");
        problems.Remove($"\"{Localizer.Instance["live-guard-keep"]}\" is hidden");
        Assert.True(problems.Count == 0, string.Join("\n", problems));

        // Focusing HelpSearchBox and pressing F12 ALWAYS runs ClearEverything.
        searchBox.Focus();
        Dispatcher.UIThread.RunJobs();
        window.KeyPress(Key.F12, RawInputModifiers.None, PhysicalKey.F12, string.Empty);
        Dispatcher.UIThread.RunJobs();
        Assert.Equal(1, clearCount);

        window.Close();
    }

    [AvaloniaFact]
    public void InfoButton_EmptyState_and_ErrorAlert_open_contextual_guides()
    {
        Configure(100, "light");
        var help = new FakeContextHelp();
        var model = new MainViewModel(null, null, new EmptyFakeLibrary(), help);
        var window = OpenWindow(model);

        // 1. InfoButton loads L1 summary and links into HelpPanel.
        var infoButton = ControlByName<InfoButton>(window, "PreviewInfoButton");
        var loaded = infoButton.ShowPopoverAsync().GetAwaiter().GetResult();
        Dispatcher.UIThread.RunJobs();
        Assert.NotNull(loaded);
        Assert.True(infoButton.IsPopoverOpen);
        infoButton.OpenGuideAsync().GetAwaiter().GetResult();
        Dispatcher.UIThread.RunJobs();
        Assert.False(infoButton.IsPopoverOpen);
        Assert.True(model.IsHelpPanelOpen);
        Assert.Equal("getting-started.show-test-pattern-and-clear-screens", model.HelpPanel.CurrentArticle?.Id);

        // 2. EmptyState across Songs, Media, Bibles shows guidance + Learn how opens HelpPanel.
        model.OpenLibraryTab("Songs");
        Dispatcher.UIThread.RunJobs();
        var songsEmpty = ControlByName<EmptyState>(window, "SongsEmptyState");
        Assert.True(songsEmpty.IsVisible);
        Assert.False(string.IsNullOrWhiteSpace(ControlByName<TextBlock>(songsEmpty, "DescriptionBlock").Text));
        ControlByName<Button>(songsEmpty, "LearnHowButton").RaiseEvent(new RoutedEventArgs(Button.ClickEvent));
        Dispatcher.UIThread.RunJobs();
        Assert.Equal("songs.add-a-song-by-pasting", model.HelpPanel.CurrentArticle?.Id);

        model.OpenLibraryTab("Media");
        Dispatcher.UIThread.RunJobs();
        var mediaEmpty = ControlByName<EmptyState>(window, "MediaEmptyState");
        Assert.True(mediaEmpty.IsVisible);
        ControlByName<Button>(mediaEmpty, "LearnHowButton").RaiseEvent(new RoutedEventArgs(Button.ClickEvent));
        Dispatcher.UIThread.RunJobs();
        Assert.Equal("media.add-pictures-and-videos", model.HelpPanel.CurrentArticle?.Id);

        model.OpenLibraryTab("Bibles");
        Dispatcher.UIThread.RunJobs();
        var biblesEmpty = ControlByName<EmptyState>(window, "BiblesEmptyState");
        Assert.True(biblesEmpty.IsVisible);
        ControlByName<Button>(biblesEmpty, "LearnHowButton").RaiseEvent(new RoutedEventArgs(Button.ClickEvent));
        Dispatcher.UIThread.RunJobs();
        Assert.Equal("scripture.add-scripture-to-a-service", model.HelpPanel.CurrentArticle?.Id);

        // 3. ErrorAlert displays core's mapped fix article and clears when ErrorMessage clears.
        model.Library.ErrorMessage = "Selah couldn't save that item.";
        model.Library.ErrorFixArticleId = "troubleshooting.cant-save-library-item";
        Dispatcher.UIThread.RunJobs();
        var libraryErrorAlert = ControlByName<ErrorAlert>(window, "LibraryErrorAlert");
        Assert.True(libraryErrorAlert.IsVisible);
        ControlByName<Button>(libraryErrorAlert, "HowToFixButton").RaiseEvent(new RoutedEventArgs(Button.ClickEvent));
        Dispatcher.UIThread.RunJobs();
        Assert.Equal("troubleshooting.cant-save-library-item", model.HelpPanel.CurrentArticle?.Id);

        model.Library.ErrorMessage = null;
        Dispatcher.UIThread.RunJobs();
        Assert.False(libraryErrorAlert.IsVisible);
        Assert.Null(model.Library.ErrorFixArticleId);

        // 4. SettingsWindow & DiagnosticsWindow inherit IHelpHost from Owner and expose InfoButtons.
        var settingsWindow = new SettingsWindow { DataContext = model.Settings, Width = 820, Height = 720 };
        settingsWindow.Show(window);
        Dispatcher.UIThread.RunJobs();
        var settingsInfo = settingsWindow.GetVisualDescendants().OfType<InfoButton>().First();
        var settingsArticle = settingsInfo.ShowPopoverAsync().GetAwaiter().GetResult();
        Assert.NotNull(settingsArticle);
        settingsWindow.Close();

        var diagWindow = new DiagnosticsWindow { DataContext = model.CreateDiagnostics(), Width = 780, Height = 640 };
        diagWindow.Show(window);
        Dispatcher.UIThread.RunJobs();
        var diagInfo = diagWindow.GetVisualDescendants().OfType<InfoButton>().First();
        var diagArticle = diagInfo.ShowPopoverAsync().GetAwaiter().GetResult();
        Assert.NotNull(diagArticle);
        diagWindow.Close();

        window.Close();
    }
}
