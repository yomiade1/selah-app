using System.IO.Pipes;
using System.Net.Sockets;
using Selah.Contracts.V1;
using Xunit;

namespace Selah.Client.Tests;

/// <summary>A minimal same-user server so the client logic is tested over the real transport.</summary>
internal sealed class TestServer : IAsyncDisposable
{
    private readonly CancellationTokenSource stop = new();
    private readonly Task loop;
    private readonly List<Stream> connections = [];

    public TestServer(IpcEndpoint endpoint, Func<Envelope, Envelope?> handler)
    {
        Endpoint = endpoint;
        Socket? listener = null;
        if (!OperatingSystem.IsWindows())
        {
            Directory.CreateDirectory(Path.GetDirectoryName(endpoint.SocketPath)!);
            File.Delete(endpoint.SocketPath);
            listener = new Socket(AddressFamily.Unix, SocketType.Stream, ProtocolType.Unspecified);
            listener.Bind(new UnixDomainSocketEndPoint(endpoint.SocketPath));
            listener.Listen(8);
        }

        loop = Task.Run(async () =>
        {
            using var owned = listener;
            while (!stop.IsCancellationRequested)
            {
                Stream stream;
                if (OperatingSystem.IsWindows())
                {
                    var pipe = new NamedPipeServerStream(endpoint.PipeName, PipeDirection.InOut,
                        NamedPipeServerStream.MaxAllowedServerInstances, PipeTransmissionMode.Byte, PipeOptions.Asynchronous);
                    try
                    {
                        await pipe.WaitForConnectionAsync(stop.Token);
                    }
                    catch
                    {
                        // A cancelled instance would otherwise keep listening and swallow clients.
                        await pipe.DisposeAsync();
                        throw;
                    }

                    stream = pipe;
                }
                else
                {
                    var socket = await listener!.AcceptAsync(stop.Token);
                    stream = new NetworkStream(socket, ownsSocket: true);
                }

                lock (connections)
                {
                    connections.Add(stream);
                }

                _ = Task.Run(async () =>
                {
                    try
                    {
                        while (true)
                        {
                            var request = await EnvelopeFraming.ReadAsync(stream, stop.Token);
                            var reply = handler(request);
                            if (reply is not null)
                            {
                                await EnvelopeFraming.WriteAsync(stream, reply, stop.Token);
                            }
                        }
                    }
                    catch (Exception)
                    {
                        await stream.DisposeAsync();
                    }
                });
            }
        });
    }

    public IpcEndpoint Endpoint { get; }

    public async Task PushAsync(Envelope message)
    {
        Stream[] all;
        lock (connections)
        {
            all = [.. connections];
        }

        foreach (var stream in all)
        {
            await EnvelopeFraming.WriteAsync(stream, message, CancellationToken.None);
        }
    }

    public async ValueTask DisposeAsync()
    {
        await stop.CancelAsync();
        lock (connections)
        {
            foreach (var stream in connections)
            {
                stream.Dispose();
            }
        }

        try
        {
            await loop;
        }
        catch (OperationCanceledException)
        {
        }

        if (!OperatingSystem.IsWindows())
        {
            File.Delete(Endpoint.SocketPath);
        }
    }
}

public sealed class IpcClientTests
{
    private static readonly Actor UiActor = new() { UserId = "local-owner", DeviceId = "test", Origin = ActorOrigin.Ui };
    private static readonly Actor CoreActor = new() { UserId = "local-owner", DeviceId = "core", Origin = ActorOrigin.Core };

    static IpcClientTests()
    {
        if (!OperatingSystem.IsWindows())
        {
            // /tmp, not the temp folder: on macOS that is a long /var/folders/… path, and a Unix
            // socket path must fit in 104 bytes there (Selah itself uses /tmp/selah-<uid>).
            var dir = Path.Combine("/tmp", $"selah-cs-{Environment.ProcessId}");
            Directory.CreateDirectory(dir);
            Environment.SetEnvironmentVariable("SELAH_RUNTIME_DIR", dir);
        }
    }

    private static IpcEndpoint UniqueEndpoint() =>
        IpcEndpoint.Create($"cs{Environment.ProcessId}x{Guid.NewGuid():N}"[..30], IpcService.Core);

    private static Envelope? StatusReply(Envelope request) => new()
    {
        Id = request.Id,
        CorrelationId = request.Id,
        Actor = CoreActor,
        Reply = new Reply { Status = new Status { Connected = true, SceneRevision = request.Id } },
    };

    private static Envelope StatusQuery() => new() { Query = new Query { GetStatus = new GetStatus() } };

    [Fact]
    public async Task ThousandsOfRoundTripsKeepCorrelation()
    {
        await using var server = new TestServer(UniqueEndpoint(), StatusReply);
        await using var client = new IpcClient(server.Endpoint, UiActor);
        client.Start();
        await client.WaitConnectedAsync(TimeSpan.FromSeconds(5));
        for (var i = 0; i < 2_000; i++)
        {
            var reply = await client.RequestAsync(StatusQuery(), TimeSpan.FromSeconds(5));
            Assert.Equal(reply.CorrelationId, reply.Reply.Status.SceneRevision);
        }
    }

    [Fact]
    public async Task UnsolicitedEnvelopesAreRaisedAsEvents()
    {
        await using var server = new TestServer(UniqueEndpoint(), StatusReply);
        await using var client = new IpcClient(server.Endpoint, UiActor);
        var received = new TaskCompletionSource<Envelope>(TaskCreationOptions.RunContinuationsAsynchronously);
        client.EnvelopeReceived += (_, envelope) => received.TrySetResult(envelope);
        client.Start();
        await client.WaitConnectedAsync(TimeSpan.FromSeconds(5));
        await client.RequestAsync(StatusQuery(), TimeSpan.FromSeconds(5));
        await server.PushAsync(new Envelope
        {
            Id = 99,
            Actor = CoreActor,
            Event = new Event { Heartbeat = new Heartbeat { MonotonicNanoseconds = 7 } },
        });
        var envelope = await received.Task.WaitAsync(TimeSpan.FromSeconds(5));
        Assert.Equal(7UL, envelope.Event.Heartbeat.MonotonicNanoseconds);
    }

    [Fact]
    public async Task ReconnectsAfterTheServerRestarts()
    {
        var endpoint = UniqueEndpoint();
        var server = new TestServer(endpoint, StatusReply);
        await using var client = new IpcClient(endpoint, UiActor);
        client.Start();
        await client.WaitConnectedAsync(TimeSpan.FromSeconds(5));
        var firstGeneration = client.State.Generation;
        await server.DisposeAsync().AsTask().WaitAsync(TimeSpan.FromSeconds(5));
        await Assert.ThrowsAnyAsync<Exception>(() => client.RequestAsync(StatusQuery(), TimeSpan.FromSeconds(2)));

        await using var restarted = new TestServer(endpoint, StatusReply);
        var started = DateTime.UtcNow;
        while (client.State.Generation == firstGeneration || !client.State.Connected)
        {
            Assert.True(DateTime.UtcNow - started < TimeSpan.FromSeconds(2), "no reconnect within 2 s");
            await Task.Delay(10);
        }

        await client.RequestAsync(StatusQuery(), TimeSpan.FromSeconds(5));
    }

    [Fact]
    public async Task RequestWhileDisconnectedFailsFast()
    {
        await using var client = new IpcClient(UniqueEndpoint(), UiActor);
        client.Start();
        await Assert.ThrowsAsync<IOException>(() => client.RequestAsync(StatusQuery(), TimeSpan.FromSeconds(5)));
    }

    [Fact]
    public void ReconnectScheduleMatchesRust()
    {
        var delays = Enumerable.Range(0, 7).Select(n => (int)IpcClient.ReconnectDelay(n).TotalMilliseconds);
        Assert.Equal([25, 50, 100, 200, 400, 500, 500], delays);
        Assert.Equal(500, IpcClient.ReconnectDelay(int.MaxValue).TotalMilliseconds);
    }

    [Fact]
    public void EndpointNamesMatchRust()
    {
        var endpoint = IpcEndpoint.Create(@"..\..\evil/../x y", IpcService.Core);
        Assert.Equal("evilxy", endpoint.Namespace);
        Assert.Equal("selah-evilxy-core", endpoint.PipeName);
        Assert.Equal("user", IpcEndpoint.Create("", IpcService.Engine).Namespace);
        Assert.Equal(40, IpcEndpoint.Create(new string('a', 100), IpcService.Engine).Namespace.Length);
    }
}

public sealed class PreviewRingReaderTests
{
    private static string Fixture => Path.Combine(AppContext.BaseDirectory, "fixtures", "preview-ring.bin");

    [Fact]
    public void ReadsTheRingWrittenByRust()
    {
        // tests/fixtures/ipc/preview-ring.bin is written by selah-ipc's ring fixture test:
        // 3 slots, 2 × 2 BGRA frames, frames 0..=3 published with every byte equal to the frame number.
        using var reader = PreviewRingReader.Open(Fixture);
        Assert.Equal(4UL, reader.Published);
        var latest = reader.Latest();
        Assert.NotNull(latest);
        Assert.Equal(3UL, latest.Frame);
        Assert.Equal((2, 2, 8), (latest.Width, latest.Height, latest.Stride));
        Assert.Equal(3000UL, latest.TimestampNs);
        Assert.All(latest.Pixels, b => Assert.Equal(3, b));
        Assert.Null(reader.Read(0)); // overwritten by frame 3
        Assert.NotNull(reader.Read(1));
    }

    [Fact]
    public void RejectsFilesThatAreNotRings()
    {
        var path = Path.GetTempFileName();
        try
        {
            File.WriteAllBytes(path, new byte[128]);
            Assert.Throws<InvalidDataException>(() => PreviewRingReader.Open(path));
        }
        finally
        {
            File.Delete(path);
        }
    }
}
