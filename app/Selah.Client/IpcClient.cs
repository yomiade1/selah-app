using System.Collections.Concurrent;
using System.IO.Pipes;
using System.Net.Sockets;
using Selah.Contracts.V1;

namespace Selah.Client;

/// <summary>Connection state; <see cref="Generation"/> increases on every new connection.</summary>
public readonly record struct IpcConnectionState(bool Connected, ulong Generation);

/// <summary>
/// A reconnecting IPC client (the C# twin of <c>selah_ipc::Client</c>): request/reply with
/// correlation, unsolicited events, and reconnect with backoff (25 ms doubling to 500 ms).
/// </summary>
public sealed class IpcClient : IAsyncDisposable
{
    private static long nextInstance;
    private readonly IpcEndpoint endpoint;
    private readonly Actor actor;
    private readonly ConcurrentDictionary<ulong, TaskCompletionSource<Envelope>> pending = new();
    private readonly CancellationTokenSource stop = new();
    private readonly SemaphoreSlim writeLock = new(1, 1);
    private readonly object stateLock = new();
    private Stream? stream;
    private Task? supervisor;
    private long nextId = InitialEnvelopeId();
    private IpcConnectionState state;
    private TaskCompletionSource connectedSignal = NewSignal();

    public IpcClient(IpcEndpoint endpoint, Actor actor)
    {
        this.endpoint = endpoint;
        this.actor = actor.Clone();
    }

    /// <summary>Raised on a thread-pool thread for every event or unsolicited envelope.</summary>
    public event EventHandler<Envelope>? EnvelopeReceived;

    /// <summary>Raised when the connection opens or closes.</summary>
    public event EventHandler<IpcConnectionState>? StateChanged;

    public IpcConnectionState State
    {
        get
        {
            lock (stateLock)
            {
                return state;
            }
        }
    }

    public void Start() => supervisor ??= Task.Run(() => SuperviseAsync(stop.Token));

    public async Task WaitConnectedAsync(TimeSpan timeout, CancellationToken cancellationToken = default)
    {
        Task signal;
        lock (stateLock)
        {
            signal = connectedSignal.Task;
        }

        await signal.WaitAsync(timeout, cancellationToken).ConfigureAwait(false);
    }

    /// <summary>Send <paramref name="message"/> (body already set) and wait for its correlated reply.</summary>
    public async Task<Envelope> RequestAsync(Envelope message, TimeSpan timeout, CancellationToken cancellationToken = default)
    {
        Prepare(message);
        var reply = new TaskCompletionSource<Envelope>(TaskCreationOptions.RunContinuationsAsynchronously);
        pending[message.Id] = reply;
        try
        {
            await WriteAsync(message, cancellationToken).ConfigureAwait(false);
            return await reply.Task.WaitAsync(timeout, cancellationToken).ConfigureAwait(false);
        }
        finally
        {
            pending.TryRemove(message.Id, out _);
        }
    }

    /// <summary>Send without waiting for a reply.</summary>
    public Task SendAsync(Envelope message, CancellationToken cancellationToken = default)
    {
        Prepare(message);
        return WriteAsync(message, cancellationToken);
    }

    public async ValueTask DisposeAsync()
    {
        await stop.CancelAsync().ConfigureAwait(false);
        CloseStream();
        if (supervisor is not null)
        {
            try
            {
                await supervisor.ConfigureAwait(false);
            }
            catch (OperationCanceledException)
            {
            }
        }

        stop.Dispose();
        writeLock.Dispose();
    }

    /// <summary>Same schedule as Rust's <c>reconnect_delay</c>.</summary>
    public static TimeSpan ReconnectDelay(int attempt) =>
        TimeSpan.FromMilliseconds(Math.Min(500, 25 << Math.Clamp(attempt, 0, 5)));

    private static TaskCompletionSource NewSignal() => new(TaskCreationOptions.RunContinuationsAsynchronously);

    private static long InitialEnvelopeId()
    {
        var ms = (ulong)Math.Max(0L, DateTimeOffset.UtcNow.ToUnixTimeMilliseconds() - 1_700_000_000_000L) & 0x1FF_FFFF_FFFFUL;
        var tag = ((ulong)Environment.ProcessId ^ (ulong)Interlocked.Increment(ref nextInstance)) & 0xFFFUL;
        return (long)((ms << 22) | (tag << 10));
    }

    private void Prepare(Envelope message)
    {
        message.Id = (ulong)Interlocked.Increment(ref nextId);
        message.Actor = actor.Clone();
    }

    private async Task WriteAsync(Envelope message, CancellationToken cancellationToken)
    {
        await writeLock.WaitAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            var current = stream ?? throw new IOException("Not connected to Selah.");
            await EnvelopeFraming.WriteAsync(current, message, cancellationToken).ConfigureAwait(false);
        }
        finally
        {
            writeLock.Release();
        }
    }

    private async Task SuperviseAsync(CancellationToken token)
    {
        var attempt = 0;
        ulong generation = 0;
        while (!token.IsCancellationRequested)
        {
            Stream? connected = null;
            try
            {
                connected = await ConnectAsync(token).ConfigureAwait(false);
            }
            catch (Exception exception) when (exception is IOException or SocketException or TimeoutException
                or UnauthorizedAccessException)
            {
                connected = null;
            }

            if (connected is not null)
            {
                attempt = 0;
                generation++;
                stream = connected;
                SetState(new IpcConnectionState(true, generation));
                try
                {
                    while (!token.IsCancellationRequested)
                    {
                        Dispatch(await EnvelopeFraming.ReadAsync(connected, token).ConfigureAwait(false));
                    }
                }
                catch (Exception exception) when (exception is IOException or InvalidDataException
                    or EndOfStreamException or ObjectDisposedException)
                {
                }

                CloseStream();
                foreach (var waiter in pending.Values)
                {
                    waiter.TrySetException(new IOException("The connection to Selah closed."));
                }

                SetState(new IpcConnectionState(false, generation));
            }

            await Task.Delay(ReconnectDelay(attempt), token).ConfigureAwait(false);
            attempt++;
        }
    }

    private async Task<Stream> ConnectAsync(CancellationToken token)
    {
        if (OperatingSystem.IsWindows())
        {
            var pipe = new NamedPipeClientStream(".", endpoint.PipeName, PipeDirection.InOut, PipeOptions.Asynchronous);
            try
            {
                await pipe.ConnectAsync(100, token).ConfigureAwait(false);
                PeerVerification.VerifyPipeServer(pipe);
                return pipe;
            }
            catch
            {
                await pipe.DisposeAsync().ConfigureAwait(false);
                throw;
            }
        }

        var socket = new Socket(AddressFamily.Unix, SocketType.Stream, ProtocolType.Unspecified);
        try
        {
            await socket.ConnectAsync(new UnixDomainSocketEndPoint(endpoint.SocketPath), token).ConfigureAwait(false);
            PeerVerification.VerifySocketPeer(socket);
            return new NetworkStream(socket, ownsSocket: true);
        }
        catch
        {
            socket.Dispose();
            throw;
        }
    }

    private void Dispatch(Envelope message)
    {
        var isReply = message.BodyCase is Envelope.BodyOneofCase.Reply or Envelope.BodyOneofCase.Error;
        if (isReply && message.CorrelationId != 0 && pending.TryRemove(message.CorrelationId, out var waiter))
        {
            waiter.TrySetResult(message);
            return;
        }

        EnvelopeReceived?.Invoke(this, message);
    }

    private void CloseStream()
    {
        var current = Interlocked.Exchange(ref stream, null);
        current?.Dispose();
    }

    private void SetState(IpcConnectionState next)
    {
        lock (stateLock)
        {
            state = next;
            if (next.Connected)
            {
                connectedSignal.TrySetResult();
            }
            else if (connectedSignal.Task.IsCompleted)
            {
                connectedSignal = NewSignal();
            }
        }

        StateChanged?.Invoke(this, next);
    }
}
