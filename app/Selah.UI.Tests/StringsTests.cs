using System.Text.RegularExpressions;
using Avalonia.Headless.XUnit;
using Selah.UI.Services;

namespace Selah.UI.Tests;

/// <summary>Strings live in .ftl files (ADR-010); every key the UI uses exists; the pseudo-locale works.</summary>
public sealed partial class StringsTests : IDisposable
{
    private readonly Reset reset = new();

    public void Dispose() => reset.Dispose();

    [GeneratedRegex(@"\[(?<key>[a-z][a-z0-9-]+)\]")]
    private static partial Regex XamlKey();

    [GeneratedRegex(@"Localizer\.Instance(?:\[|\.Get\()""(?<key>[^""]+)""")]
    private static partial Regex CodeKey();

    private static IEnumerable<string> UsedKeys()
    {
        var ui = Repo.PathOf("app", "Selah.UI");
        foreach (var file in Directory.EnumerateFiles(ui, "*.axaml", SearchOption.AllDirectories))
        {
            foreach (Match match in XamlKey().Matches(File.ReadAllText(file)))
            {
                yield return match.Groups["key"].Value;
            }
        }
        foreach (var file in Directory.EnumerateFiles(ui, "*.cs", SearchOption.AllDirectories).Where(f => !f.Contains($"{Path.DirectorySeparatorChar}obj{Path.DirectorySeparatorChar}")))
        {
            foreach (Match match in CodeKey().Matches(File.ReadAllText(file)))
            {
                yield return match.Groups["key"].Value;
            }
        }
    }

    [AvaloniaFact]
    public void Every_key_the_ui_uses_is_in_english()
    {
        var used = UsedKeys().Distinct().ToList();
        Assert.Contains("command-clear-everything", used);
        used.AddRange(TextScale.StepNameKeys);
        used.AddRange(["status-connected", "status-connecting", "status-engine-not-connected"]);
        var missing = used.Where(key => !Localizer.Instance.Has(key)).ToList();
        Assert.True(missing.Count == 0, "missing strings: " + string.Join(", ", missing));
    }

    [AvaloniaFact]
    public void Pseudo_locale_is_accented_and_at_least_forty_percent_longer()
    {
        var english = Localizer.Instance["command-show-test-pattern"];
        Localizer.Instance.SetLanguage(Localizer.PseudoLocale);
        var pseudo = Localizer.Instance["command-show-test-pattern"];
        Assert.NotEqual(english, pseudo);
        Assert.StartsWith("[", pseudo);
        Assert.Contains("Šĥöŵ", pseudo);
        Assert.True(pseudo.Length >= english.Length * 1.4, $"{pseudo} is too short");
    }

    [AvaloniaFact]
    public void Placeables_survive_the_pseudo_locale()
    {
        Localizer.Instance.SetLanguage(Localizer.PseudoLocale);
        Assert.Contains("150", Localizer.Instance.Get("text-size-value", ("percent", 150)));
    }

    [AvaloniaFact]
    public void A_missing_key_shows_the_key_not_a_crash() =>
        Assert.Equal("no-such-string", Localizer.Instance["no-such-string"]);

    [AvaloniaFact]
    public void Changing_language_notifies_bindings()
    {
        var raised = new List<string?>();
        Localizer.Instance.PropertyChanged += (_, e) => raised.Add(e.PropertyName);
        Localizer.Instance.SetLanguage(Localizer.PseudoLocale);
        Assert.Contains("Item[]", raised);
    }
}
