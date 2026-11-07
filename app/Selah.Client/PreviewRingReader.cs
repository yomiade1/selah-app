using System.IO.MemoryMappedFiles;

namespace Selah.Client;

/// <summary>One preview frame copied out of the ring (BGRA, premultiplied).</summary>
public sealed record PreviewFrame(ulong Frame, int Width, int Height, int Stride, ulong TimestampNs, byte[] Pixels);

/// <summary>
/// Reads the engine's shared-memory preview ring (the twin of <c>selah_ipc::shm::RingReader</c>).
/// Each slot is guarded by a seqlock: a frame is accepted only if its sequence word was even and
/// unchanged across the copy, so a torn frame is never returned.
/// </summary>
public sealed class PreviewRingReader : IDisposable
{
    private const int HeaderBytes = 64;
    private const int SlotHeaderBytes = 32;
    private const int LatestOffset = 24;
    private const uint MaxSlots = 8;
    private const ulong MaxSlotCapacity = 3840UL * 2160 * 4;
    private static readonly byte[] Magic = "SELAHRNG"u8.ToArray();

    private readonly MemoryMappedFile file;
    private readonly MemoryMappedViewAccessor view;
    private readonly uint slots;
    private readonly ulong capacity;

    private PreviewRingReader(MemoryMappedFile file, MemoryMappedViewAccessor view, uint slots, ulong capacity)
    {
        this.file = file;
        this.view = view;
        this.slots = slots;
        this.capacity = capacity;
    }

    public static PreviewRingReader Open(string path)
    {
        using var stream = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.ReadWrite);
        var length = stream.Length;
        if (length < HeaderBytes)
        {
            throw new InvalidDataException("Not a Selah preview ring.");
        }

        var file = MemoryMappedFile.CreateFromFile(stream, null, 0, MemoryMappedFileAccess.Read,
            HandleInheritability.None, leaveOpen: false);
        var view = file.CreateViewAccessor(0, length, MemoryMappedFileAccess.Read);
        try
        {
            var magic = new byte[8];
            view.ReadArray(0, magic, 0, 8);
            var version = view.ReadUInt32(8);
            var slots = view.ReadUInt32(12);
            var capacity = view.ReadUInt64(16);
            if (!magic.AsSpan().SequenceEqual(Magic) || version != 1 || slots is 0 or > MaxSlots
                || capacity is 0 or > MaxSlotCapacity || capacity % 8 != 0
                || HeaderBytes + (long)slots * (SlotHeaderBytes + (long)capacity) > length)
            {
                throw new InvalidDataException("The preview ring header is invalid.");
            }

            return new PreviewRingReader(file, view, slots, capacity);
        }
        catch
        {
            view.Dispose();
            file.Dispose();
            throw;
        }
    }

    public ulong Published
    {
        get
        {
            var value = view.ReadUInt64(LatestOffset);
            Interlocked.MemoryBarrier();
            return value;
        }
    }

    public PreviewFrame? Latest()
    {
        for (var attempt = 0; attempt < 8; attempt++)
        {
            var published = Published;
            if (published == 0)
            {
                return null;
            }

            var frame = Read(published - 1);
            if (frame is not null)
            {
                return frame;
            }

            Thread.SpinWait(16);
        }

        return null;
    }

    public PreviewFrame? Read(ulong frame)
    {
        var offset = HeaderBytes + (long)(frame % slots) * (SlotHeaderBytes + (long)capacity);
        var expected = (frame * 2) + 2;
        var before = view.ReadUInt64(offset);
        Interlocked.MemoryBarrier();
        if (before != expected)
        {
            return null;
        }

        var width = view.ReadInt32(offset + 8);
        var height = view.ReadInt32(offset + 12);
        var stride = view.ReadInt32(offset + 16);
        var timestamp = view.ReadUInt64(offset + 24);
        var length = (long)stride * height;
        if (width <= 0 || height <= 0 || stride < width * 4 || length > (long)capacity)
        {
            return null;
        }

        var pixels = new byte[length];
        view.ReadArray(offset + SlotHeaderBytes, pixels, 0, (int)length);
        Interlocked.MemoryBarrier();
        return view.ReadUInt64(offset) == expected
            ? new PreviewFrame(frame, width, height, stride, timestamp, pixels)
            : null;
    }

    public void Dispose()
    {
        view.Dispose();
        file.Dispose();
    }
}
