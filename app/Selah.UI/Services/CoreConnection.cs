using System.Diagnostics;
using Selah.Client;
using Selah.Contracts.V1;

namespace Selah.UI.Services;

/// <summary>Core refused a request: its catalogue code and the string key of its plain message.</summary>
public sealed class CoreException(string code, string messageKey) : InvalidOperationException(code)
{
    public string Code { get; } = code;

    public string MessageKey { get; } = messageKey;
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
public sealed class CoreConnection : IAsyncDisposable, Selah.UI.ViewModels.IHelpSource, Selah.UI.ViewModels.IUpdateService, Selah.UI.ViewModels.IDiagnosticsService
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
            throw new CoreException(error.Code, error.MessageKey);
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
        var search = new SearchHelp { Query = query, Language = Localizer.Instance.Language == Localizer.PseudoLocale ? "en" : Localizer.Instance.Language };
        search.Anchors.AddRange(anchors);
        var reply = await RequestAsync(new Envelope { Query = new Query { SearchHelp = search } }).ConfigureAwait(false);
        return reply.Reply?.HelpResults?.Articles.ToList() ?? [];
    }

    public async Task<Article?> GetArticleAsync(string id)
    {
        var reply = await RequestAsync(new Envelope { Query = new Query { GetArticle = new GetArticle { Id = id, Language = "en" } } }).ConfigureAwait(false);
        return reply.Reply?.Article;
    }

    private Task<Envelope> RequestAsync(Envelope message) => RequestAsync(message, RequestTimeout);

    private async Task<Envelope> RequestAsync(Envelope message, TimeSpan timeout)
    {
        var reply = await core.RequestAsync(message, timeout).ConfigureAwait(false);
        if (reply.Error is { } error)
        {
            throw new CoreException(error.Code, error.MessageKey);
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
            await core.RequestAsync(new Envelope { Query = new Query { Subscribe = new Subscribe { EventNames = { "status" } } } }, RequestTimeout)
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
        var wasConnected = false;
        using var timer = new PeriodicTimer(HeartbeatInterval);
        while (await timer.WaitForNextTickAsync(cancellationToken).ConfigureAwait(false))
        {
            var connected = watchdog.State.Connected;
            if (exitWithWatchdog && wasConnected && !connected)
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
