using Avalonia;
using Avalonia.Controls;

namespace Selah.UI.Views;

/// <summary>
/// Marks a label that must never be clipped or truncated: Next, Clear, LIVE, connection and
/// output health (DESIGN-SYSTEM §4.1). UI tests fail if a critical element doesn't fit at any
/// text size, appearance or language.
/// </summary>
public static class Critical
{
    public static readonly AttachedProperty<bool> IsCriticalProperty =
        AvaloniaProperty.RegisterAttached<Control, bool>("IsCritical", typeof(Critical));

    public static bool GetIsCritical(Control control) => control.GetValue(IsCriticalProperty);

    public static void SetIsCritical(Control control, bool value) => control.SetValue(IsCriticalProperty, value);
}
