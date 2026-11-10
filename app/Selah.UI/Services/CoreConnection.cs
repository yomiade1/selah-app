using System.Diagnostics;
using Google.Protobuf;
using Selah.Client;
using Selah.Contracts.V1;
using Selah.UI.ViewModels;

namespace Selah.UI.Services;

/// <summary>Core refused a request: its catalogue code, string key, and optional fix help article ID.</summary>
public sealed class CoreException(string code, string messageKey, string fixArticleId = "") : InvalidOperationException(code)
{
    public string Code { get; } = code;

    public string MessageKey { get; } = messageKey;

    public string FixArticleId { get; } = fixArticleId;
}

/// <summary>A snapshot of what the UI knows about core and the screens.</summary>
public sealed record ConnectionSnapshot(bool Connected, bool EngineConnected, bool SafeMode, bool Live, ulong Generation);

/// <summary>
/// The UI's links to the other processes (ADR-001, ADR-004):
/// <list type="bullet">
/// <item>core: status, settings and commands;</item>
/// <item>the watchdog: a heartbeat every 250 ms, so a hung UI is restarted;</item>
/// <item>the engine's emergency lane: <b>Clear everything</b> and <b>Show logo</b> go straight to the
/// engine, so they work even if core is frozen (SAFE-03, ADR-026).</item>
/// </list>
/// Events arrive on thread-pool threads; view models marshal them to the UI thread.
/// </summary>
public sealed class CoreConnection : IAsyncDisposable, Selah.UI.ViewModels.IHelpSource, Selah.UI.ViewModels.IUpdateService, Selah.UI.ViewModels.IDiagnosticsService, Selah.UI.ViewModels.ILibraryService
{
    public static readonly TimeSpan HeartbeatInterval = TimeSpan.FromMilliseconds(250);
    private static readonly TimeSpan RequestTimeout = TimeSpan.FromSeconds(2);
    private static readonly TimeSpan SlowRequestTimeout = TimeSpan.FromSeconds(60);

    private readonly IpcClient core;
    private readonly IpcClient watchdog;
    private readonly IpcClient emergency;
    private readonly CancellationTokenSource stop = new();
    private readonly object gate = new();
    private readonly Stopwatch clock = Stopwatch.StartNew();
    private Task? heartbeat;
    private Status status = new();
    private ConnectionSnapshot snapshot = new(false, false, false, false, 0);

    public CoreConnection(string? ns = null)
    {
        var actor = new Actor { UserId = "local-owner", DeviceId = "ui", Origin = ActorOrigin.Ui };
        var coreEndpoint = ns is null ? IpcEndpoint.ForCurrentUser(IpcService.Core) : IpcEndpoint.Create(ns, IpcService.Core);
        core = new IpcClient(coreEndpoint, actor);
        watchdog = new IpcClient(coreEndpoint.Sibling(IpcService.Watchdog), actor);
        emergency = new IpcClient(coreEndpoint.Sibling(IpcService.Emergency), actor);
        core.StateChanged += OnCoreStateChanged;
        core.EnvelopeReceived += OnCoreEnvelope;
    }

    /// <summary>Raised (on a thread-pool thread) whenever the snapshot changes.</summary>
    public event EventHandler<ConnectionSnapshot>? Changed;

    /// <summary>Raised when core sends the settings (after each connection).</summary>
    public event EventHandler<Settings>? SettingsReceived;

    /// <summary>Raised when a library document is saved, deleted, undone, or redone.</summary>
    public event EventHandler<LibraryChanged>? LibraryChanged;

    /// <summary>Raised when undo/redo availability changes in core.</summary>
    public event EventHandler<UndoStateChanged>? UndoStateChanged;

    public ConnectionSnapshot Snapshot
    {
        get
        {
            lock (gate)
            {
                return snapshot;
            }
        }
    }

    /// <summary>The last status core sent.</summary>
    public Status CurrentStatus
    {
        get
        {
            lock (gate)
            {
                return status;
            }
        }
    }

    public void Start()
    {
        core.Start();
        watchdog.Start();
        emergency.Start();
        heartbeat ??= Task.Run(() => HeartbeatLoopAsync(stop.Token));
    }

    public async Task ShowTestPatternAsync()
    {
        await RequestAsync(new Envelope
        {
            Command = new Command { ShowTestPattern = new ShowTestPattern { Pattern = TestPattern.SmpteBars } },
        }).ConfigureAwait(false);
    }

    public async Task SetAppearanceAsync(AppearanceSettings appearance)
    {
        await RequestAsync(new Envelope { Command = new Command { SetAppearance = new SetAppearance { Appearance = appearance } } })
            .ConfigureAwait(false);
    }

    /// <summary>
    /// <b>Restart normally</b> (SAFE-07): ask the watchdog to restart core and the UI without
    /// safe mode. This UI process is closed and started again.
    /// </summary>
    public async Task RestartNormallyAsync()
    {
        var reply = await watchdog.RequestAsync(
            new Envelope { Command = new Command { RestartNormally = new RestartNormally() } }, RequestTimeout).ConfigureAwait(false);
        if (reply.Error is { } error)
        {
            throw new CoreException(error.Code, error.MessageKey, error.FixArticleId);
        }
    }

    /// <summary>Help ▸ Diagnostics: this computer, the graphics and the screens (M1.13).</summary>
    public async Task<Diagnostics> GetDiagnosticsAsync()
    {
        var reply = await RequestAsync(new Envelope { Query = new Query { GetDiagnostics = new GetDiagnostics() } }, TimeSpan.FromSeconds(5))
            .ConfigureAwait(false);
        return reply.Reply?.Diagnostics ?? new Diagnostics();
    }

    /// <summary>Help ▸ Diagnostics ▸ Run benchmark. It takes about a minute.</summary>
    public async Task<DiagnosticsReport> RunDiagnosticsAsync(bool quick)
    {
        var reply = await RequestAsync(new Envelope { Command = new Command { RunDiagnostics = new RunDiagnostics { Quick = quick } } }, TimeSpan.FromMinutes(5))
            .ConfigureAwait(false);
        return reply.Reply?.DiagnosticsReport ?? new DiagnosticsReport();
    }

    /// <summary>Help ▸ Report a problem: core saves a redacted support bundle (SEC-06).</summary>
    public async Task<SupportBundle> CreateSupportBundleAsync()
    {
        var reply = await RequestAsync(new Envelope { Command = new Command { CreateSupportBundle = new CreateSupportBundle() } }, TimeSpan.FromSeconds(30))
            .ConfigureAwait(false);
        return reply.Reply?.SupportBundle ?? new SupportBundle();
    }

    /// <summary>Settings ▸ Updates (M1.11).</summary>
    public async Task SetUpdateSettingsAsync(string feedUrl, bool allow)
    {
        await RequestAsync(new Envelope
        {
            Command = new Command { SetUpdateSettings = new SetUpdateSettings { UpdateFeedUrl = feedUrl, AllowUpdateCheck = allow } },
        }).ConfigureAwait(false);
    }

    /// <summary>Only when the person asks (SEC-13). The feed may take a while to answer.</summary>
    public async Task<UpdateCheck> CheckForUpdateAsync()
    {
        var reply = await RequestAsync(new Envelope { Command = new Command { CheckForUpdate = new CheckForUpdate() } }, SlowRequestTimeout)
            .ConfigureAwait(false);
        return reply.Reply?.UpdateCheck ?? new UpdateCheck();
    }

    /// <summary>Core downloads and verifies the package, then the installer restarts Selah.</summary>
    public async Task InstallUpdateAsync(string version)
    {
        await RequestAsync(new Envelope { Command = new Command { InstallUpdate = new InstallUpdate { Version = version } } }, TimeSpan.FromMinutes(15))
            .ConfigureAwait(false);
    }

    /// <summary>The emergency lane (SAFE-03): straight to the engine, never through core.</summary>
    public async Task<bool> ClearEverythingAsync()
    {
        try
        {
            var reply = await emergency.RequestAsync(
                new Envelope { Command = new Command { Render = new RenderCommand { ClearEverything = new ClearEverything() } } },
                RequestTimeout).ConfigureAwait(false);
            return reply.Reply?.Acknowledged == true;
        }
        catch (Exception error) when (error is TimeoutException or IOException or InvalidOperationException or OperationCanceledException)
        {
            // Core may still be able to clear through the normal path.
            try
            {
                await RequestAsync(new Envelope { Command = new Command { ClearOutputs = new ClearOutputs() } }).ConfigureAwait(false);
                return true;
            }
            catch (Exception fallback) when (fallback is TimeoutException or IOException or InvalidOperationException or OperationCanceledException)
            {
                return false;
            }
        }
    }

    /// <summary>Help search (KNOWLEDGE-BASE §6.4); anchors on screen lift their articles.</summary>
    public async Task<IReadOnlyList<Article>> SearchHelpAsync(string query, IEnumerable<string> anchors)
    {
        var search = new SearchHelp { Query = query, Language = CurrentHelpLanguage() };
        search.Anchors.AddRange(anchors);
        var reply = await RequestAsync(new Envelope { Query = new Query { SearchHelp = search } }).ConfigureAwait(false);
        return reply.Reply?.HelpResults?.Articles.ToList() ?? [];
    }

    /// <summary>Articles for the anchors visible on screen (KNOWLEDGE-BASE §5.2, §6.4).</summary>
    public async Task<IReadOnlyList<Article>> HelpForContextAsync(IEnumerable<string> anchors)
    {
        var request = new HelpForContext { Language = CurrentHelpLanguage() };
        request.Anchors.AddRange(anchors);
        if (request.Anchors.Count == 0)
        {
            return [];
        }
        var reply = await RequestAsync(new Envelope { Query = new Query { HelpForContext = request } }).ConfigureAwait(false);
        return reply.Reply?.HelpResults?.Articles.ToList() ?? [];
    }

    public async Task<Article?> GetArticleAsync(string id)
    {
        var reply = await RequestAsync(new Envelope { Query = new Query { GetArticle = new GetArticle { Id = id, Language = CurrentHelpLanguage() } } }).ConfigureAwait(false);
        return reply.Reply?.Article;
    }

    /// <summary>Persist "Did this help?" feedback locally in core's library database (KNOWLEDGE-BASE §6.4, M2.8).</summary>
    public async Task RecordFeedbackAsync(string articleId, bool helpful)
    {
        if (string.IsNullOrWhiteSpace(articleId))
        {
            return;
        }
        await RequestAsync(new Envelope
        {
            Command = new Command
            {
                RecordFeedback = new RecordFeedback
                {
                    ArticleId = articleId,
                    Helpful = helpful,
                },
            },
        }).ConfigureAwait(false);
    }

    private static string CurrentHelpLanguage() =>
        Localizer.Instance.Language == Localizer.PseudoLocale ? "en" : Localizer.Instance.Language;

    public async Task<IReadOnlyList<Song>> ListSongsAsync(uint limit = 200, uint offset = 0)
    {
        var reply = await RequestAsync(new Envelope { Query = new Query { ListSongs = new ListSongs { Limit = limit, Offset = offset } } })
            .ConfigureAwait(false);
        return reply.Reply?.SongList?.Songs.ToList() ?? [];
    }

    public async Task<SongSearchResults> SearchSongsAsync(string query, uint limit = 50, uint offset = 0)
    {
        var reply = await RequestAsync(new Envelope { Query = new Query { SearchSongs = new SearchSongs { Query = query, Limit = limit, Offset = offset } } })
            .ConfigureAwait(false);
        return reply.Reply?.SongSearchResults ?? new SongSearchResults();
    }

    public async Task<Song?> GetSongAsync(Id id)
    {
        var reply = await RequestAsync(new Envelope { Query = new Query { GetSong = new GetSong { Id = id } } })
            .ConfigureAwait(false);
        return reply.Reply?.Song;
    }

    public async Task SaveSongAsync(Song song, Id? undoGroup = null)
    {
        if (song.Id is null || song.Id.Value.IsEmpty)
        {
            song.Id = LibraryIds.NewId();
        }
        foreach (var section in song.Sections)
        {
            if (section.Id is null || section.Id.Value.IsEmpty)
            {
                section.Id = LibraryIds.NewId();
            }
        }
        foreach (var arrangement in song.Arrangements)
        {
            if (arrangement.Id is null || arrangement.Id.Value.IsEmpty)
            {
                arrangement.Id = LibraryIds.NewId();
            }
        }
        var command = new Command { SaveSong = new SaveSong { Song = song } };
        if (undoGroup is not null)
        {
            command.UndoGroup = undoGroup;
        }
        await RequestAsync(new Envelope { Command = command }).ConfigureAwait(false);
    }

    public async Task DeleteSongAsync(Id id, Id? undoGroup = null)
    {
        var command = new Command { DeleteSong = new DeleteSong { Id = id } };
        if (undoGroup is not null)
        {
            command.UndoGroup = undoGroup;
        }
        await RequestAsync(new Envelope { Command = command }).ConfigureAwait(false);
    }

    public async Task<ParsePastedSongReply> ParsePastedSongAsync(
        string rawText,
        SongPasteFormat format = SongPasteFormat.Unspecified,
        string defaultTitle = "")
    {
        var reply = await RequestAsync(new Envelope
        {
            Query = new Query
            {
                ParsePastedSong = new ParsePastedSong
                {
                    RawText = rawText,
                    Format = format,
                    DefaultTitle = defaultTitle,
                },
            },
        }).ConfigureAwait(false);
        return reply.Reply?.ParsedSong ?? new ParsePastedSongReply();
    }

    public async Task<Song> TransposeSongAsync(
        Song song,
        int semitones,
        AccidentalSpelling spelling = AccidentalSpelling.Unspecified)
    {
        var reply = await RequestAsync(new Envelope
        {
            Query = new Query
            {
                TransposeSong = new TransposeSong
                {
                    Song = song,
                    Semitones = semitones,
                    AccidentalSpelling = spelling,
                },
            },
        }).ConfigureAwait(false);
        return reply.Reply?.Song ?? song;
    }

    public async Task<GeneratedSongSlides> SplitSongSlidesAsync(SplitSongSlides request)
    {
        var reply = await RequestAsync(new Envelope { Query = new Query { SplitSongSlides = request } })
            .ConfigureAwait(false);
        return reply.Reply?.GeneratedSongSlides ?? new GeneratedSongSlides();
    }

    public async Task SaveEditorBufferAsync(Id docId, DocumentType docType, string content)
    {
        await RequestAsync(new Envelope
        {
            Command = new Command
            {
                SaveEditorBuffer = new SaveEditorBuffer
                {
                    DocId = docId,
                    DocType = docType,
                    Content = content,
                },
            },
        }).ConfigureAwait(false);
    }

    public async Task DiscardEditorBufferAsync(Id docId)
    {
        await RequestAsync(new Envelope
        {
            Command = new Command { DiscardEditorBuffer = new DiscardEditorBuffer { DocId = docId } },
        }).ConfigureAwait(false);
    }

    public async Task<EditorBufferReply> GetEditorBufferAsync(Id docId)
    {
        var reply = await RequestAsync(new Envelope { Query = new Query { GetEditorBuffer = new GetEditorBuffer { DocId = docId } } })
            .ConfigureAwait(false);
        return reply.Reply?.EditorBuffer ?? new EditorBufferReply();
    }

    public async Task<UndoState> GetUndoStateAsync(Id? docId = null)
    {
        var query = new GetUndoState();
        if (docId is not null)
        {
            query.DocId = docId;
        }
        var reply = await RequestAsync(new Envelope { Query = new Query { GetUndoState = query } })
            .ConfigureAwait(false);
        return reply.Reply?.UndoState ?? new UndoState();
    }

    public async Task UndoAsync(Id? docId = null)
    {
        var undo = new Undo();
        if (docId is not null)
        {
            undo.DocId = docId;
        }
        await RequestAsync(new Envelope { Command = new Command { Undo = undo } }).ConfigureAwait(false);
    }

    public async Task RedoAsync(Id? docId = null)
    {
        var redo = new Redo();
        if (docId is not null)
        {
            redo.DocId = docId;
        }
        await RequestAsync(new Envelope { Command = new Command { Redo = redo } }).ConfigureAwait(false);
    }

    public async Task<IReadOnlyList<BibleTranslation>> ListBiblesAsync(bool includeBooks = false)
    {
        var reply = await RequestAsync(new Envelope { Query = new Query { ListBibles = new ListBibles { IncludeBooks = includeBooks } } })
            .ConfigureAwait(false);
        return reply.Reply?.BibleList?.Bibles.ToList() ?? [];
    }

    public async Task<BibleTranslation> ImportBibleAsync(ImportBible request)
    {
        var reply = await RequestAsync(new Envelope { Command = new Command { ImportBible = request } }, TimeSpan.FromSeconds(30))
            .ConfigureAwait(false);
        return reply.Reply?.BibleTranslation ?? new BibleTranslation();
    }

    public async Task DeleteBibleAsync(string code)
    {
        await RequestAsync(new Envelope { Command = new Command { DeleteBible = new DeleteBible { Code = code } } })
            .ConfigureAwait(false);
    }

    public async Task<ParseScriptureReferenceReply> ParseScriptureReferenceAsync(string reference, string languageHint = "")
    {
        var reply = await RequestAsync(new Envelope
        {
            Query = new Query
            {
                ParseScriptureReference = new ParseScriptureReference
                {
                    Reference = reference,
                    LanguageHint = languageHint,
                },
            },
        }).ConfigureAwait(false);
        return reply.Reply?.ParsedScriptureReference ?? new ParseScriptureReferenceReply();
    }

    public async Task<ScripturePassages> GetScripturePassagesAsync(string translation, string reference, string languageHint = "")
    {
        var reply = await RequestAsync(new Envelope
        {
            Query = new Query
            {
                GetScripturePassages = new GetScripturePassages
                {
                    Translation = translation,
                    Reference = reference,
                    LanguageHint = languageHint,
                },
            },
        }).ConfigureAwait(false);
        return reply.Reply?.ScripturePassages ?? new ScripturePassages();
    }

    public async Task<ScriptureSearchResults> SearchScriptureAsync(string query, string translation = "", uint limit = 50, uint offset = 0)
    {
        var reply = await RequestAsync(new Envelope
        {
            Query = new Query
            {
                SearchScripture = new SearchScripture
                {
                    Query = query,
                    Translation = translation,
                    Limit = limit,
                    Offset = offset,
                },
            },
        }).ConfigureAwait(false);
        return reply.Reply?.ScriptureSearchResults ?? new ScriptureSearchResults();
    }

    public async Task<GeneratedScriptureSlides> SplitScriptureSlidesAsync(SplitScriptureSlides request)
    {
        var reply = await RequestAsync(new Envelope { Query = new Query { SplitScriptureSlides = request } })
            .ConfigureAwait(false);
        return reply.Reply?.GeneratedScriptureSlides ?? new GeneratedScriptureSlides();
    }

    public async Task<IReadOnlyList<MediaItem>> ListMediaItemsAsync(uint limit = 200, uint offset = 0)
    {
        var reply = await RequestAsync(new Envelope { Query = new Query { ListMediaItems = new ListMediaItems { Limit = limit, Offset = offset } } })
            .ConfigureAwait(false);
        return reply.Reply?.MediaItemList?.MediaItems.ToList() ?? [];
    }

    public async Task<MediaSearchResults> SearchMediaAsync(string query, MediaKind kind = MediaKind.Unspecified, uint limit = 50, uint offset = 0)
    {
        var reply = await RequestAsync(new Envelope
        {
            Query = new Query
            {
                SearchMedia = new SearchMedia
                {
                    Query = query,
                    Kind = kind,
                    Limit = limit,
                    Offset = offset,
                },
            },
        }).ConfigureAwait(false);
        return reply.Reply?.MediaSearchResults ?? new MediaSearchResults();
    }

    public async Task<MediaItem> ImportMediaAsync(string sourcePath, string name = "", IEnumerable<string>? tags = null)
    {
        var import = new ImportMedia { SourcePath = sourcePath, Name = name };
        if (tags is not null)
        {
            import.Tags.AddRange(tags);
        }
        var reply = await RequestAsync(new Envelope { Command = new Command { ImportMedia = import } }, TimeSpan.FromSeconds(30))
            .ConfigureAwait(false);
        return reply.Reply?.MediaItem ?? new MediaItem();
    }

    public async Task DeleteMediaItemAsync(Id id)
    {
        await RequestAsync(new Envelope { Command = new Command { DeleteMediaItem = new DeleteMediaItem { Id = id } } })
            .ConfigureAwait(false);
    }

    public async Task<MediaThumbnail> GetMediaThumbnailAsync(Id mediaId, uint maxWidth = 320, uint maxHeight = 180)
    {
        var reply = await RequestAsync(new Envelope
        {
            Query = new Query
            {
                GetMediaThumbnail = new GetMediaThumbnail
                {
                    MediaId = mediaId,
                    MaxWidth = maxWidth,
                    MaxHeight = maxHeight,
                },
            },
        }, TimeSpan.FromSeconds(10)).ConfigureAwait(false);
        return reply.Reply?.MediaThumbnail ?? new MediaThumbnail();
    }

    public async Task<ExportedSelahBundle> ExportSelahBundleAsync(ExportSelahBundle request)
    {
        var reply = await RequestAsync(new Envelope { Command = new Command { ExportSelahBundle = request } }, SlowRequestTimeout)
            .ConfigureAwait(false);
        return reply.Reply?.ExportedSelahBundle ?? new ExportedSelahBundle();
    }

    public async Task<ImportedSelahBundle> ImportSelahBundleAsync(ImportSelahBundle request)
    {
        var reply = await RequestAsync(new Envelope { Command = new Command { ImportSelahBundle = request } }, SlowRequestTimeout)
            .ConfigureAwait(false);
        return reply.Reply?.ImportedSelahBundle ?? new ImportedSelahBundle();
    }

    public async Task<LibraryBackupInfo> CreateLibraryBackupAsync(string outputPath = "")
    {
        var reply = await RequestAsync(new Envelope
        {
            Command = new Command { CreateLibraryBackup = new CreateLibraryBackup { OutputPath = outputPath } },
        }, SlowRequestTimeout).ConfigureAwait(false);
        return reply.Reply?.LibraryBackupInfo ?? new LibraryBackupInfo();
    }

    public async Task<RestoreLibraryBackupResult> RestoreLibraryBackupAsync(string backupPath)
    {
        var reply = await RequestAsync(new Envelope
        {
            Command = new Command { RestoreLibraryBackup = new RestoreLibraryBackup { BackupPath = backupPath } },
        }, SlowRequestTimeout).ConfigureAwait(false);
        return reply.Reply?.RestoreLibraryBackupResult ?? new RestoreLibraryBackupResult();
    }

    public async Task<IReadOnlyList<Tag>> ListTagsAsync(uint limit = 200, uint offset = 0)
    {
        var reply = await RequestAsync(new Envelope { Query = new Query { ListTags = new ListTags { Limit = limit, Offset = offset } } })
            .ConfigureAwait(false);
        return reply.Reply?.TagList?.Tags.ToList() ?? [];
    }

    public async Task SaveTagAsync(Tag tag)
    {
        if (tag.Id is null || tag.Id.Value.IsEmpty)
        {
            tag.Id = LibraryIds.NewId();
        }
        await RequestAsync(new Envelope { Command = new Command { SaveTag = new SaveTag { Tag = tag } } })
            .ConfigureAwait(false);
    }

    public async Task<LibrarySearchResults> SearchLibraryAsync(
        string query,
        SearchScope scope = SearchScope.All,
        string translation = "",
        uint limit = 20)
    {
        var reply = await RequestAsync(new Envelope
        {
            Query = new Query
            {
                SearchLibrary = new SearchLibrary
                {
                    Query = query,
                    Scope = scope,
                    Translation = translation,
                    Limit = limit,
                },
            },
        }).ConfigureAwait(false);
        return reply.Reply?.LibrarySearchResults ?? new LibrarySearchResults();
    }

    private Task<Envelope> RequestAsync(Envelope message) => RequestAsync(message, RequestTimeout);

    private async Task<Envelope> RequestAsync(Envelope message, TimeSpan timeout)
    {
        var reply = await core.RequestAsync(message, timeout).ConfigureAwait(false);
        if (reply.Error is { } error)
        {
            throw new CoreException(error.Code, error.MessageKey, error.FixArticleId);
        }
        return reply;
    }

    private void OnCoreStateChanged(object? sender, IpcConnectionState state)
    {
        if (!state.Connected)
        {
            Update(status, connected: false, state.Generation);
            return;
        }
        _ = Task.Run(() => OnConnectedAsync(state.Generation));
    }

    private async Task OnConnectedAsync(ulong generation)
    {
        try
        {
            await core.RequestAsync(new Envelope { Query = new Query { Subscribe = new Subscribe { EventNames = { "status", "library", "undo" } } } }, RequestTimeout)
                .ConfigureAwait(false);
            var reply = await core.RequestAsync(new Envelope { Query = new Query { GetStatus = new GetStatus() } }, RequestTimeout)
                .ConfigureAwait(false);
            if (reply.Reply?.Status is { } current)
            {
                Update(current, connected: true, generation);
            }
            var settings = await core.RequestAsync(new Envelope { Query = new Query { GetSettings = new GetSettings() } }, RequestTimeout)
                .ConfigureAwait(false);
            if (settings.Reply?.Settings is { } received)
            {
                SettingsReceived?.Invoke(this, received);
            }
            WriteTestMarker(generation);
        }
        catch (Exception error) when (error is TimeoutException or IOException or InvalidOperationException or OperationCanceledException)
        {
            // The connection dropped while starting; the next connection tries again.
        }
    }

    private void OnCoreEnvelope(object? sender, Envelope envelope)
    {
        if (envelope.Event?.Status is { } current)
        {
            Update(current, connected: true, core.State.Generation);
        }
        if (envelope.Event?.LibraryChanged is { } libraryChanged)
        {
            LibraryChanged?.Invoke(this, libraryChanged);
        }
        if (envelope.Event?.UndoStateChanged is { } undoStateChanged)
        {
            UndoStateChanged?.Invoke(this, undoStateChanged);
        }
    }

    private void Update(Status current, bool connected, ulong generation)
    {
        ConnectionSnapshot next;
        lock (gate)
        {
            status = current;
            next = new ConnectionSnapshot(connected, connected && current.EngineConnected, current.SafeMode, connected && current.Live, generation);
            if (next == snapshot)
            {
                return;
            }
            snapshot = next;
        }
        SelfTest.OnSnapshot(this, next);
        Changed?.Invoke(this, next);
    }

    private async Task HeartbeatLoopAsync(CancellationToken cancellationToken)
    {
        // Tests only (SELAH_EXIT_WITH_WATCHDOG=1): leave when the watchdog goes, like the Rust
        // processes, so no test leaves a UI behind.
        var exitWithWatchdog = Environment.GetEnvironmentVariable("SELAH_EXIT_WITH_WATCHDOG") == "1";
        int? watchdogPid = int.TryParse(Environment.GetEnvironmentVariable("SELAH_WATCHDOG_PID"), out var parsedPid) && parsedPid > 0
            ? parsedPid
            : null;
        var wasConnected = false;
        using var timer = new PeriodicTimer(HeartbeatInterval);
        while (await timer.WaitForNextTickAsync(cancellationToken).ConfigureAwait(false))
        {
            if (exitWithWatchdog && watchdogPid is int parentPid && !IsProcessAlive(parentPid))
            {
                Environment.Exit(0);
            }
            var connected = watchdog.State.Connected;
            if (exitWithWatchdog && ((wasConnected && !connected) || (!wasConnected && clock.Elapsed > TimeSpan.FromSeconds(10))))
            {
                Environment.Exit(0);
            }
            wasConnected |= connected;
            if (!connected)
            {
                continue;
            }
            try
            {
                var nanos = (ulong)(clock.Elapsed.Ticks * 100);
                await watchdog.SendAsync(new Envelope { Event = new Event { Heartbeat = new Heartbeat { MonotonicNanoseconds = nanos } } }, cancellationToken)
                    .ConfigureAwait(false);
            }
            catch (Exception error) when (error is IOException or InvalidOperationException)
            {
                // The watchdog went away; the client reconnects.
            }
        }
    }

    private static bool IsProcessAlive(int pid)
    {
        try
        {
            using var process = Process.GetProcessById(pid);
            return !process.HasExited;
        }
        catch (Exception error) when (error is ArgumentException or InvalidOperationException)
        {
            return false;
        }
    }

    /// <summary>
    /// Chaos tests read <c>$SELAH_HOME/ui.txt</c> (<c>&lt;pid&gt; &lt;generation&gt;</c>) to see that the UI
    /// reconnected. Written only when <c>SELAH_HOME</c> is set (tests and portable runs).
    /// </summary>
    private static void WriteTestMarker(ulong generation)
    {
        var home = Environment.GetEnvironmentVariable("SELAH_HOME");
        if (string.IsNullOrEmpty(home) || !Directory.Exists(home))
        {
            return;
        }
        var target = Path.Combine(home, "ui.txt");
        var temporary = target + ".tmp";
        File.WriteAllText(temporary, $"{Environment.ProcessId} {generation}");
        File.Move(temporary, target, overwrite: true);
    }

    public async ValueTask DisposeAsync()
    {
        await stop.CancelAsync().ConfigureAwait(false);
        if (heartbeat is not null)
        {
            try
            {
                await heartbeat.ConfigureAwait(false);
            }
            catch (OperationCanceledException)
            {
            }
        }
        await core.DisposeAsync().ConfigureAwait(false);
        await watchdog.DisposeAsync().ConfigureAwait(false);
        await emergency.DisposeAsync().ConfigureAwait(false);
        stop.Dispose();
    }
}
