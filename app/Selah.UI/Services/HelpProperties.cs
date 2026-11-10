using Avalonia;
using Avalonia.Controls;
using Avalonia.VisualTree;

namespace Selah.UI.Services;

/// <summary>Implemented by windows that host contextual help (<see cref="Views.HelpPanelView"/> or <see cref="Views.HelpWindow"/>).</summary>
public interface IHelpHost
{
    ViewModels.IHelpSource? HelpSource { get; }

    Task OpenHelpForAnchorsAsync(IEnumerable<string> anchors);

    Task OpenHelpArticleAsync(string articleId);
}

/// <summary>
/// Attached help anchor property (<c>help:Help.Anchor="library.songs"</c>, KNOWLEDGE-BASE §5.3).
/// Every interactive region declares an anchor from <c>content/help/anchors.yaml</c> so F1,
/// What's this? (Shift+F1), and InfoButtons can show the right articles for the visible screen.
/// </summary>
public static class Help
{
    public static readonly AttachedProperty<string?> AnchorProperty =
        AvaloniaProperty.RegisterAttached<Control, string?>("Anchor", typeof(Help));

    public static string? GetAnchor(Control control) => control.GetValue(AnchorProperty);

    public static void SetAnchor(Control control, string? value) => control.SetValue(AnchorProperty, value);

    /// <summary>
    /// Collects distinct non-empty anchors attached to currently visible controls inside
    /// <paramref name="root"/>, placing workspace/content anchors ahead of toolbar <c>shell.*</c>
    /// anchors so screen-specific guides lead "For this screen".
    /// </summary>
    public static IReadOnlyList<string> CollectVisibleAnchors(Visual root)
    {
        var primary = new List<string>();
        var secondary = new List<string>();
        var seen = new HashSet<string>(StringComparer.Ordinal);

        foreach (var control in root.GetVisualDescendants().OfType<Control>())
        {
            if (!control.IsEffectivelyVisible)
            {
                continue;
            }
            var anchor = GetAnchor(control);
            if (string.IsNullOrWhiteSpace(anchor) || anchor == HelpAnchors.ShellHelpPanel)
            {
                continue;
            }
            if (!seen.Add(anchor))
            {
                continue;
            }
            var isWorkspaceContent = !anchor.StartsWith("shell.", StringComparison.Ordinal)
                && !anchor.StartsWith("help.", StringComparison.Ordinal)
                || anchor == HelpAnchors.ShellPreview
                || anchor == HelpAnchors.ShellCommandPalette;
            if (isWorkspaceContent)
            {
                primary.Add(anchor);
            }
            else
            {
                secondary.Add(anchor);
            }
        }

        primary.AddRange(secondary);
        return primary;
    }

    /// <summary>Walks up the visual tree from <paramref name="visual"/> to find the nearest attached help anchor.</summary>
    public static string? FindNearestAnchor(Visual? visual)
    {
        for (var current = visual; current is not null; current = current.GetVisualParent())
        {
            if (current is Control control)
            {
                var anchor = GetAnchor(control);
                if (!string.IsNullOrWhiteSpace(anchor))
                {
                    return anchor;
                }
            }
        }
        return null;
    }

    /// <summary>Finds the closest <see cref="IHelpHost"/> hosting <paramref name="visual"/>.</summary>
    public static IHelpHost? FindHost(Visual? visual)
    {
        for (var current = visual; current is not null; current = current.GetVisualParent())
        {
            if (current is IHelpHost host)
            {
                return host;
            }
        }
        return visual is Control c && TopLevel.GetTopLevel(c) is IHelpHost topHost ? topHost : null;
    }
}
