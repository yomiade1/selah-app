namespace Selah.UI.Services;

/// <summary>
/// The computer's own text size, for <b>Use my computer's text size</b> (DESIGN-SYSTEM §4.1).
/// Windows stores it as <c>TextScaleFactor</c> (100–225). macOS has no single value an app can
/// read, so it reports 100% until the accessibility API is wired up (M10.4).
/// </summary>
public static class SystemTextScale
{
    public static double ReadPercent()
    {
        if (!OperatingSystem.IsWindows())
        {
            return 100;
        }
        try
        {
            using var key = Microsoft.Win32.Registry.CurrentUser.OpenSubKey(@"Software\Microsoft\Accessibility");
            return key?.GetValue("TextScaleFactor") is int value and >= 100 and <= 225 ? value : 100;
        }
        catch (System.Security.SecurityException)
        {
            return 100;
        }
        catch (UnauthorizedAccessException)
        {
            return 100;
        }
    }

    /// <summary>The text-size step for the computer's setting.</summary>
    public static int Step() => TextScale.Nearest(ReadPercent());
}
