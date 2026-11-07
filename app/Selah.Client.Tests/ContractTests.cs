using Google.Protobuf;
using Selah.Contracts.V1;
using Xunit;

namespace Selah.Client.Tests;

public sealed class ContractTests
{
    [Fact]
    public void EnvelopeRoundTripPreservesActorAndCorrelation()
    {
        var original = new Envelope
        {
            Id = 42,
            CorrelationId = 7,
            Actor = new Actor
            {
                UserId = "local-owner",
                DeviceId = "test-device",
                Origin = ActorOrigin.Ui,
            },
            Query = new Query { GetStatus = new GetStatus() },
        };

        var decoded = Envelope.Parser.ParseFrom(original.ToByteArray());
        Assert.Equal(original, decoded);
    }

    [Fact]
    public void TruncatedEnvelopeIsRejected()
    {
        Assert.Throws<InvalidProtocolBufferException>(
            () => Envelope.Parser.ParseFrom(new byte[] { 0x1a, 0x7f, 0x01 }));
    }

    [Fact]
    public void WireFixtureIsSharedWithRustAndTypeScript()
    {
        var path = Path.Combine(AppContext.BaseDirectory, "fixtures", "status-query.hex");
        var bytes = Convert.FromHexString(File.ReadAllText(path).Trim());
        var message = Envelope.Parser.ParseFrom(bytes);
        Assert.Equal(42UL, message.Id);
        Assert.Equal(7UL, message.CorrelationId);
        Assert.Equal(ActorOrigin.Ui, message.Actor.Origin);
        Assert.Equal(bytes, message.ToByteArray());
    }
}
