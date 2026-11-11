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
    public void Booth_has_no_shadows_and_light_uses_the_documented_elevation()
    {
        // DESIGN-SYSTEM §3: "Booth uses no shadows; separation comes from surface colour."
        using var document = JsonDocument.Parse(File.ReadAllText(Repo.PathOf("design", "tokens", "tokens.json")));
        var shadow = document.RootElement.GetProperty("shadow");
        var booth = shadow.GetProperty("booth").GetProperty("elevation").GetProperty("$value").GetString();
        Assert.Equal("none", booth);
        var light = shadow.GetProperty("light").GetProperty("elevation").GetProperty("$value").GetString()!;
        Assert.Contains("#0F000000", light);
        Assert.DoesNotContain("#40000000", light);
    }

    [Fact]
    public void Slide_preview_and_scrim_tokens_exist_in_both_appearances()
    {
        // Added so no view has to hard-code a colour for slide text or an overlay scrim.
        using var document = JsonDocument.Parse(File.ReadAllText(Repo.PathOf("design", "tokens", "tokens.json")));
        var colors = document.RootElement.GetProperty("color");
        foreach (var appearance in new[] { "light", "booth" })
        {
            var mode = colors.GetProperty(appearance);
            foreach (var name in new[] { "slide-text", "slide-text-muted", "scrim" })
            {
                Assert.True(mode.TryGetProperty(name, out var token), $"{appearance}.{name} is missing");
                Assert.False(string.IsNullOrWhiteSpace(token.GetProperty("$value").GetString()));
            }
        }
    }

    [Fact]
    public void No_view_hard_codes_a_colour()
    {
        // The tokens-only rule (CLAUDE.md, DESIGN-SYSTEM §3). Tokens.g.axaml is generated and
        // is the one place literal colours belong.
        var views = Directory.GetFiles(Repo.PathOf("app", "Selah.UI"), "*.axaml", SearchOption.AllDirectories)
            .Where(f => !Path.GetFileName(f).Contains(".g.", StringComparison.Ordinal))
            .ToList();
        Assert.NotEmpty(views);
        var offenders = new List<string>();
        foreach (var view in views)
        {
            var lines = File.ReadAllLines(view);
            for (var i = 0; i < lines.Length; i++)
            {
                if (System.Text.RegularExpressions.Regex.IsMatch(lines[i], "#[0-9A-Fa-f]{6,8}\\b"))
                {
                    offenders.Add($"{Path.GetFileName(view)}:{i + 1}: {lines[i].Trim()}");
                }
            }
        }
        Assert.Empty(offenders);
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
