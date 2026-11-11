using Avalonia;
using Avalonia.Controls;
using CommunityToolkit.Mvvm.ComponentModel;
using Selah.UI.Styles;

namespace Selah.UI.Services;

/// <summary>
/// The Text size setting (ADR-009, DESIGN-SYSTEM §4.1). Every <c>type.*</c> token, control
/// height and icon size next to text is a resource recalculated here when the size changes:
/// <c>Type.Body.Size</c>, <c>Type.Body.LineHeight</c>, <c>Size.ControlHeight</c> and so on.
/// Slides and screens are never scaled by it.
/// </summary>
public sealed partial class TextScale : ObservableObject
{
    public static IReadOnlyList<int> Steps => Tokens.TextScaleSteps;

    public const int DefaultPercent = 100;

    /// <summary>String keys of the step names, in step order.</summary>
    public static readonly string[] StepNameKeys =
    [
        "text-size-smaller", "text-size-default", "text-size-large", "text-size-larger",
        "text-size-extra-large", "text-size-huge", "text-size-largest",
    ];

    [ObservableProperty]
    private int percent = DefaultPercent;

    public static TextScale Instance { get; } = new();

    public double Factor => Percent / 100.0;

    public int StepIndex => Math.Max(0, Steps.ToList().IndexOf(Percent));

    /// <summary>The step nearest an OS text scale (Windows text scaling, macOS text size).</summary>
    public static int Nearest(double percent) => Steps.MinBy(step => Math.Abs(step - percent));

    public static bool IsStep(int percent) => Steps.Contains(percent);

    public void StepUp() => Percent = Steps[Math.Min(StepIndex + 1, Steps.Count - 1)];

    public void StepDown() => Percent = Steps[Math.Max(StepIndex - 1, 0)];

    public void Reset() => Percent = DefaultPercent;

    partial void OnPercentChanging(int value)
    {
        if (!IsStep(value))
        {
            throw new ArgumentOutOfRangeException(nameof(value), value, "Text size must be one of the 7 steps.");
        }
    }

    partial void OnPercentChanged(int value)
    {
        OnPropertyChanged(nameof(Factor));
        OnPropertyChanged(nameof(StepIndex));
        Apply(Application.Current?.Resources);
    }

    /// <summary>Write the scaled sizes into <paramref name="resources"/>.</summary>
    public void Apply(IResourceDictionary? resources)
    {
        if (resources is null)
        {
            return;
        }
        var f = Factor;
        foreach (var token in Tokens.TypeScale)
        {
            var name = ResourceName(token.Name);
            resources[$"Type.{name}.Size"] = token.Size * f;
            resources[$"Type.{name}.LineHeight"] = Math.Round(token.Size * token.LineHeight * f, 2);
            resources[$"Type.{name}.LetterSpacing"] = token.LetterSpacing * f;
        }
        resources["Size.ControlHeight"] = Tokens.SizeControlHeight * f;
        resources["Size.TouchTarget"] = Math.Max(Tokens.SizeTouchTarget, Tokens.SizeTouchTarget * f);
        // Simple mode's big buttons are at least 64 px high (DESIGN-SYSTEM §5).
        resources["Size.SimpleButtonHeight"] = Math.Max(Tokens.SizeSimpleButtonHeight, Tokens.SizeSimpleButtonHeight * f);
        resources["Size.Icon"] = Tokens.SizeIcon * f;
        resources["Size.ToolbarHeight"] = Tokens.SizeToolbarHeight * f;
        // Side panels grow more slowly than text, so the preview keeps its space (§4.1).
        resources["Size.SidebarWidth"] = Tokens.SizeSidebarWidth * Math.Sqrt(f);
        resources["Size.InspectorWidth"] = Tokens.SizeInspectorWidth * Math.Sqrt(f);
        resources["Space.Text.Small"] = new Thickness(Tokens.Space3 * f, Tokens.Space2 * f);
        resources["Space.Text.Medium"] = new Thickness(Tokens.Space5 * f, Tokens.Space3 * f);
        resources["Space.Text.Large"] = new Thickness(Tokens.Space7 * f, Tokens.Space5 * f);
        resources["Space.Item"] = new Thickness(0, 0, 0, Tokens.Space3 * f);
        resources["Space.Cell"] = new Thickness(0, 0, Tokens.Space4 * f, Tokens.Space3 * f);
        resources["Space.Key"] = new Thickness(Tokens.Space2 * f, 0);
        double[] spaces = [Tokens.Space1, Tokens.Space2, Tokens.Space3, Tokens.Space4, Tokens.Space5, Tokens.Space6, Tokens.Space7, Tokens.Space8, Tokens.Space9, Tokens.Space10];
        for (var i = 0; i < spaces.Length; i++)
        {
            resources[$"Space.{i + 1}"] = spaces[i] * f;
        }
        ApplyFixed(resources);
    }

    /// <summary>Tokens that don't follow the text size: radii and line widths (DESIGN-SYSTEM §5).</summary>
    public static void ApplyFixed(IResourceDictionary resources)
    {
        resources["Radius.Tag"] = new CornerRadius(Tokens.RadiusTag);
        resources["Radius.Input"] = new CornerRadius(Tokens.RadiusInput);
        resources["Radius.Button"] = new CornerRadius(Tokens.RadiusButton);
        resources["Radius.Control"] = new CornerRadius(Tokens.RadiusControl);
        resources["Radius.Card"] = new CornerRadius(Tokens.RadiusCard);
        resources["Radius.Pill"] = new CornerRadius(Tokens.RadiusPill);
        resources["Radius.SimpleButton"] = new CornerRadius(Tokens.RadiusSimpleButton);
        resources["Focus.Thickness"] = new Thickness(Tokens.FocusWidth);
        resources["Focus.Margin"] = new Thickness(-(Tokens.FocusWidth + Tokens.FocusOffset));
        // USWDS hairlines and the alert bar (the alert's left border is the focus width).
        resources["Line.Hairline"] = new Thickness(Tokens.Space1 / 2);
        resources["Line.Emphasis"] = new Thickness(Tokens.Space1);
        resources["Line.Bottom"] = new Thickness(0, 0, 0, Tokens.Space1 / 2);
        resources["Line.Right"] = new Thickness(0, 0, Tokens.Space1 / 2, 0);
        resources["Line.Left"] = new Thickness(Tokens.Space1 / 2, 0, 0, 0);
        resources["Line.AlertBar"] = new Thickness(Tokens.FocusWidth + Tokens.Space1, 0, 0, 0);
    }

    /// <summary><c>body-sm</c> → <c>BodySm</c>.</summary>
    public static string ResourceName(string token) =>
        string.Concat(token.Split('-').Select(part => char.ToUpperInvariant(part[0]) + part[1..]));
}
