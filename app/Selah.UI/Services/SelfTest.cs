using Selah.Client;
using Selah.Contracts.V1;

namespace Selah.UI.Services;

/// <summary>
/// The installed-build check (M1.11, <c>tools/test-installer.ps1</c>). With <c>SELAH_SELF_TEST=1</c>
/// and <c>SELAH_HOME</c> set, the UI waits until it shows "Connected", asks for the test pattern
/// through the normal path, waits for the engine to show that scene, and checks that the preview
/// ring holds SMPTE bars (the first bar is 75% grey). It writes <c>$SELAH_HOME/self-test.txt</c>:
/// <c>pass &lt;frame checksum&gt;</c> or <c>fail &lt;reason&gt;</c>. It runs once per UI process.
/// </summary>
public static class SelfTest
{
    private static int started;

    public static bool Requested =>
        Environment.GetEnvironmentVariable("SELAH_SELF_TEST") == "1"
        && !string.IsNullOrEmpty(Environment.GetEnvironmentVariable("SELAH_HOME"));

    /// <summary>Start the check when the snapshot first says "Connected".</summary>
    public static void OnSnapshot(CoreConnection connection, ConnectionSnapshot snapshot)
    {
        if (!Requested || !snapshot.Connected || !snapshot.EngineConnected || Interlocked.Exchange(ref started, 1) == 1)
        {
            return;
        }
        _ = Task.Run(() => RunAsync(connection));
    }

    private static async Task RunAsync(CoreConnection connection)
    {
        string result;
        try
        {
            result = await CheckAsync(connection).ConfigureAwait(false);
        }
        catch (Exception error) when (error is TimeoutException or IOException or InvalidOperationException or OperationCanceledException)
        {
            result = $"fail {error.GetType().Name}";
        }
        Write(result);
    }

    private static async Task<string> CheckAsync(CoreConnection connection)
    {
        await connection.ShowTestPatternAsync().ConfigureAwait(false);
        var deadline = DateTime.UtcNow + TimeSpan.FromSeconds(20);
        Status? shown = null;
        while (DateTime.UtcNow < deadline)
        {
            var status = connection.CurrentStatus;
            if (status.Live && status.FrameChecksum.Length == 32 && status.EngineSceneRevision == status.SceneRevision)
            {
                shown = status;
                break;
            }
            await Task.Delay(100).ConfigureAwait(false);
        }
        if (shown is null)
        {
            return "fail the engine did not show the test pattern";
        }
        var ns = IpcEndpoint.ForCurrentUser(IpcService.Engine).Namespace;
        var ring = Path.Combine(RuntimeDirectory.Path, $"{ns}-preview-0.ring");
        while (DateTime.UtcNow < deadline)
        {
            if (File.Exists(ring))
            {
                using var reader = PreviewRingReader.Open(ring);
                // BGRA: the first SMPTE bar is 75% grey (191, 191, 191).
                if (reader.Latest() is { } frame && frame.Pixels[0] == 191 && frame.Pixels[1] == 191 && frame.Pixels[2] == 191)
                {
                    return $"pass {Convert.ToHexString(shown.FrameChecksum.ToByteArray()).ToLowerInvariant()}";
                }
            }
            await Task.Delay(100).ConfigureAwait(false);
        }
        return "fail the preview did not show the test pattern";
    }

    private static void Write(string result)
    {
        var home = Environment.GetEnvironmentVariable("SELAH_HOME")!;
        var target = Path.Combine(home, "self-test.txt");
        File.WriteAllText(target + ".tmp", result);
        File.Move(target + ".tmp", target, overwrite: true);
    }
}
