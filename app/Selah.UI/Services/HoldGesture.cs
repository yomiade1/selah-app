namespace Selah.UI.Services;

/// <summary>
/// "Hold a key for N seconds" (SAFE-03: hold Esc for 1 second to clear the screens). Key repeat
/// calls <see cref="Down"/> again while the key is held; a timer may also call it. It fires once
/// per press. Time comes from the caller, so tests drive it without waiting.
/// </summary>
public sealed class HoldGesture(TimeSpan threshold)
{
    private TimeSpan? pressedAt;
    private bool fired;

    public TimeSpan Threshold { get; } = threshold;

    public bool IsHeld => pressedAt is not null;

    /// <summary>Returns true exactly once, when the key has been held for the threshold.</summary>
    public bool Down(TimeSpan now)
    {
        pressedAt ??= now;
        if (fired || now - pressedAt.Value < Threshold)
        {
            return false;
        }
        fired = true;
        return true;
    }

    /// <summary>The key came up. Returns true if this press already fired (a hold, not a tap).</summary>
    public bool Up()
    {
        var wasHold = fired;
        pressedAt = null;
        fired = false;
        return wasHold;
    }
}

/// <summary>Paths for files the UI itself keeps (not the library, which core owns).</summary>
public static class UiPaths
{
    /// <summary>Under <c>SELAH_HOME</c> for tests and portable runs, else the per-user app data folder.</summary>
    public static string Folder
    {
        get
        {
            var home = Environment.GetEnvironmentVariable("SELAH_HOME");
            var root = string.IsNullOrEmpty(home)
                ? Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Selah")
                : home;
            return Path.Combine(root, "ui");
        }
    }

    public static string ProducerLayout => Path.Combine(Folder, "producer-layout.json");
}
