using System.Globalization;
using System.Text.Json;
using Selah.UI.Services;
using Selah.UI.Styles;

namespace Selah.UI.Tests;

/// <summary>The token contrast test (ADR-022, DESIGN-SYSTEM §3) and the text-size steps.</summary>
public sealed class TokenTests
{
    private static double Channel(int value)
    {
        var c = value / 255.0;
        return c <= 0.03928 ? c / 12.92 : Math.Pow((c + 0.055) / 1.055, 2.4);
    }

    private static double Luminance(string hex)
    {
        var rgb = int.Parse(hex.TrimStart('#'), NumberStyles.HexNumber, CultureInfo.InvariantCulture);
        return (0.2126 * Channel((rgb >> 16) & 0xFF)) + (0.7152 * Channel((rgb >> 8) & 0xFF)) + (0.0722 * Channel(rgb & 0xFF));
    }

    public static double Contrast(string a, string b)
    {
        var (la, lb) = (Luminance(a), Luminance(b));
        return (Math.Max(la, lb) + 0.05) / (Math.Min(la, lb) + 0.05);
    }

    [Fact]
    public void Every_declared_contrast_pair_passes()
    {
        using var document = JsonDocument.Parse(File.ReadAllText(Repo.PathOf("design", "tokens", "tokens.json")));
        var colors = document.RootElement.GetProperty("color");
        string Hex(string path)
        {
            var node = colors;
            foreach (var part in path.Split('.'))
            {
                node = node.GetProperty(part);
            }
            return node.GetProperty("$value").GetString()!;
        }
        var pairs = document.RootElement.GetProperty("contrastPairs").GetProperty("$value").EnumerateArray().ToList();
        Assert.NotEmpty(pairs);
        var failures = new List<string>();
        foreach (var pair in pairs)
        {
            var (foreground, background, minimum) = (pair[0].GetString()!, pair[1].GetString()!, pair[2].GetDouble());
            var ratio = Contrast(Hex(foreground), Hex(background));
            if (ratio < minimum)
            {
                failures.Add($"{foreground} on {background}: {ratio:F2} < {minimum}");
            }
        }
        Assert.True(failures.Count == 0, string.Join("\n", failures));
    }

    [Fact]
    public void Contrast_maths_matches_wcag()
    {
        Assert.Equal(21, Contrast("#000000", "#FFFFFF"), 1);
        Assert.Equal(1, Contrast("#777777", "#777777"), 3);
    }

    [Fact]
    public void Text_size_has_the_seven_documented_steps()
    {
        Assert.Equal([90, 100, 115, 130, 150, 175, 200], Tokens.TextScaleSteps);
        Assert.Equal(Tokens.TextScaleSteps.Length, TextScale.StepNameKeys.Length);
    }

    [Theory]
    [InlineData(100, 100)]
    [InlineData(125, 130)]
    [InlineData(160, 150)]
    [InlineData(225, 200)]
    [InlineData(80, 90)]
    public void Computer_text_size_maps_to_the_nearest_step(double computer, int step) =>
        Assert.Equal(step, TextScale.Nearest(computer));
}
