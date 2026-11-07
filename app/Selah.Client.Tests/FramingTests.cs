using Selah.Contracts.V1;
using Xunit;

namespace Selah.Client.Tests;

public sealed class FramingTests
{
    private static Envelope Message(ulong id) => new()
    {
        Id = id,
        CorrelationId = id - 1,
        Actor = new Actor { UserId = "local-owner", DeviceId = "test-device", Origin = ActorOrigin.Ui },
        Query = new Query { GetStatus = new GetStatus() },
    };

    [Fact]
    public async Task TenThousandFramingRoundTripsPreserveCorrelation()
    {
        using var stream = new MemoryStream();
        for (ulong id = 1; id <= 10_000; id++)
        {
            stream.SetLength(0);
            var original = Message(id);
            await EnvelopeFraming.WriteAsync(stream, original, CancellationToken.None);
            stream.Position = 0;
            Assert.Equal(original, await EnvelopeFraming.ReadAsync(stream, CancellationToken.None));
        }
    }

    [Fact]
    public async Task OversizedLengthIsRejectedBeforeBodyRead()
    {
        using var stream = new MemoryStream(new byte[] { 1, 0, 0, 1 });
        await Assert.ThrowsAsync<InvalidDataException>(
            () => EnvelopeFraming.ReadAsync(stream, CancellationToken.None));
    }

    [Theory]
    [InlineData(new byte[] { 0, 0, 0, 0 })]
    [InlineData(new byte[] { 2, 0, 0, 0, 8, 1 })]
    public async Task EmptyOrUnknownBodyIsRejected(byte[] bytes)
    {
        using var stream = new MemoryStream(bytes);
        await Assert.ThrowsAsync<InvalidDataException>(
            () => EnvelopeFraming.ReadAsync(stream, CancellationToken.None));
    }

    [Fact]
    public async Task TruncatedBodyIsRejected()
    {
        using var stream = new MemoryStream(new byte[] { 4, 0, 0, 0, 1 });
        await Assert.ThrowsAsync<EndOfStreamException>(
            () => EnvelopeFraming.ReadAsync(stream, CancellationToken.None));
    }

    [Fact]
    public async Task ActorlessOutgoingMessageIsRejected()
    {
        using var stream = new MemoryStream();
        var message = Message(1);
        message.Actor = null;
        await Assert.ThrowsAsync<InvalidDataException>(
            () => EnvelopeFraming.WriteAsync(stream, message, CancellationToken.None));
        Assert.Equal(0, stream.Length);
    }

    [Fact]
    public async Task UnknownNestedMessageIsRejected()
    {
        using var stream = new MemoryStream();
        var message = Message(1);
        message.Query = new Query();
        await Assert.ThrowsAsync<InvalidDataException>(
            () => EnvelopeFraming.WriteAsync(stream, message, CancellationToken.None));
        Assert.Equal(0, stream.Length);
    }

    [Theory]
    [InlineData(0)]
    [InlineData(9999)]
    public async Task UnknownOriginIsRejected(int origin)
    {
        using var stream = new MemoryStream();
        var message = Message(1);
        message.Actor.Origin = (ActorOrigin)origin;
        await Assert.ThrowsAsync<InvalidDataException>(
            () => EnvelopeFraming.WriteAsync(stream, message, CancellationToken.None));
        Assert.Equal(0, stream.Length);
    }
}
