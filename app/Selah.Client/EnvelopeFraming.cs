using System.Buffers.Binary;
using Google.Protobuf;
using Selah.Contracts.V1;

namespace Selah.Client;

/// <summary>
/// Framing only. The transport must authenticate its peer before accepting messages.
/// </summary>
public static class EnvelopeFraming
{
    public const int MaximumFrameBytes = 16 * 1024 * 1024;

    public static async Task<Envelope> ReadAsync(Stream stream, CancellationToken cancellationToken)
    {
        var header = new byte[sizeof(uint)];
        await stream.ReadExactlyAsync(header, cancellationToken).ConfigureAwait(false);
        var length = BinaryPrimitives.ReadUInt32LittleEndian(header);
        if (length is 0 or > MaximumFrameBytes)
        {
            throw new InvalidDataException("Frame length is outside the permitted range.");
        }

        var body = new byte[(int)length];
        await stream.ReadExactlyAsync(body, cancellationToken).ConfigureAwait(false);
        Envelope message;
        try
        {
            message = Envelope.Parser.ParseFrom(body);
        }
        catch (InvalidProtocolBufferException exception)
        {
            throw new InvalidDataException("Malformed protobuf envelope.", exception);
        }

        Validate(message);
        return message;
    }

    public static async Task WriteAsync(Stream stream, Envelope message, CancellationToken cancellationToken)
    {
        Validate(message);
        var length = message.CalculateSize();
        if (length is 0 or > MaximumFrameBytes)
        {
            throw new InvalidDataException("Frame length is outside the permitted range.");
        }

        var header = new byte[sizeof(uint)];
        BinaryPrimitives.WriteUInt32LittleEndian(header, (uint)length);
        await stream.WriteAsync(header, cancellationToken).ConfigureAwait(false);
        await stream.WriteAsync(message.ToByteArray(), cancellationToken).ConfigureAwait(false);
        await stream.FlushAsync(cancellationToken).ConfigureAwait(false);
    }

    private static void Validate(Envelope message)
    {
        var actor = message.Actor;
        if (actor is null || string.IsNullOrEmpty(actor.UserId) || string.IsNullOrEmpty(actor.DeviceId)
            || actor.Origin == ActorOrigin.Unspecified || !Enum.IsDefined(actor.Origin))
        {
            throw new InvalidDataException("An envelope must have valid actor attribution.");
        }

        var known = message.BodyCase switch
        {
            Envelope.BodyOneofCase.Command => message.Command.ActionCase != Command.ActionOneofCase.None,
            Envelope.BodyOneofCase.Query => message.Query.RequestCase != Query.RequestOneofCase.None,
            Envelope.BodyOneofCase.Event => message.Event.NotificationCase != Event.NotificationOneofCase.None,
            Envelope.BodyOneofCase.Reply => message.Reply.ResultCase != Reply.ResultOneofCase.None,
            Envelope.BodyOneofCase.Error => !string.IsNullOrEmpty(message.Error.Code),
            _ => false,
        };
        if (!known)
        {
            throw new InvalidDataException("An envelope must have a known message type.");
        }
    }
}
