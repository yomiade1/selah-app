using System.ComponentModel;
using System.Globalization;
using System.Text;
using Linguini.Bundle;
using Linguini.Bundle.Builder;
using Linguini.Shared.Types.Bundle;

namespace Selah.UI.Services;

/// <summary>
/// UI strings from <c>content/i18n/&lt;lang&gt;/*.ftl</c> (ADR-010), copied next to the app as
/// <c>i18n/</c>. Views bind to the indexer (<c>{Binding [key], Source={x:Static ...}}</c>), so a
/// language change updates every label. <c>en-XA</c> is the pseudo-locale used by UI tests.
/// </summary>
public sealed class Localizer : INotifyPropertyChanged
{
    public const string PseudoLocale = "en-XA";

    private FluentBundle bundle;

    private Localizer()
    {
        bundle = Load("en");
    }

    public static Localizer Instance { get; } = new();

    /// <summary>The folder that holds one sub-folder of <c>.ftl</c> files per language.</summary>
    public static string StringsDirectory => Path.Combine(AppContext.BaseDirectory, "i18n");

    public string Language { get; private set; } = "en";

    public event PropertyChangedEventHandler? PropertyChanged;

    public string this[string key] => Get(key);

    public void SetLanguage(string language)
    {
        if (language == Language)
        {
            return;
        }
        bundle = Load(language);
        Language = language;
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs("Item[]"));
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(nameof(Language)));
    }

    public bool Has(string key) => bundle.HasMessage(key);

    /// <summary>The message for <paramref name="key"/>; a missing key shows as the key itself.</summary>
    public string Get(string key, params (string Name, object Value)[] args)
    {
        var arguments = new Dictionary<string, IFluentType>();
        foreach (var (name, value) in args)
        {
            arguments[name] = value switch
            {
                int number => FluentNumber.FromString(number.ToString(CultureInfo.InvariantCulture)),
                double number => FluentNumber.FromString(number.ToString(CultureInfo.InvariantCulture)),
                _ => new FluentString(value.ToString() ?? string.Empty),
            };
        }
        return bundle.TryGetMessage(key, arguments, out _, out var message) && message is not null ? message : key;
    }

    /// <summary>Every message key in the language (for tests and <c>tools/lint-strings</c>).</summary>
    public IEnumerable<string> Keys => bundle.GetMessageEnumerable();

    private static FluentBundle Load(string language)
    {
        var source = language == PseudoLocale ? "en" : language;
        var folder = Path.Combine(StringsDirectory, source);
        var files = Directory.Exists(folder) ? Directory.GetFiles(folder, "*.ftl").Order(StringComparer.Ordinal).ToArray() : [];
        var ready = LinguiniBuilder.Builder()
            .CultureInfo(new CultureInfo(source))
            .AddFiles(files)
            .SetUseIsolating(false);
        if (language == PseudoLocale)
        {
            ready = ready.SetTransformFunc(Pseudo.Transform);
        }
        return ready.UncheckedBuild();
    }
}

/// <summary>
/// The pseudo-locale (ADR-010): accented letters and text about 40% longer, in brackets, so
/// tests catch hard-coded strings, clipping and layouts that can't grow.
/// </summary>
public static class Pseudo
{
    private static readonly Dictionary<char, char> Accents = new()
    {
        ['a'] = 'á',
        ['b'] = 'ƀ',
        ['c'] = 'ç',
        ['d'] = 'ð',
        ['e'] = 'é',
        ['f'] = 'ƒ',
        ['g'] = 'ĝ',
        ['h'] = 'ĥ',
        ['i'] = 'í',
        ['j'] = 'ĵ',
        ['k'] = 'ķ',
        ['l'] = 'ĺ',
        ['m'] = 'ɱ',
        ['n'] = 'ñ',
        ['o'] = 'ö',
        ['p'] = 'þ',
        ['q'] = 'ǫ',
        ['r'] = 'ŕ',
        ['s'] = 'š',
        ['t'] = 'ţ',
        ['u'] = 'û',
        ['v'] = 'ṽ',
        ['w'] = 'ŵ',
        ['x'] = 'ẋ',
        ['y'] = 'ý',
        ['z'] = 'ž',
        ['A'] = 'Å',
        ['B'] = 'Ɓ',
        ['C'] = 'Ç',
        ['D'] = 'Ð',
        ['E'] = 'É',
        ['F'] = 'Ƒ',
        ['G'] = 'Ĝ',
        ['H'] = 'Ĥ',
        ['I'] = 'Í',
        ['J'] = 'Ĵ',
        ['K'] = 'Ķ',
        ['L'] = 'Ĺ',
        ['M'] = 'Ṁ',
        ['N'] = 'Ñ',
        ['O'] = 'Ö',
        ['P'] = 'Þ',
        ['Q'] = 'Ǫ',
        ['R'] = 'Ŕ',
        ['S'] = 'Š',
        ['T'] = 'Ţ',
        ['U'] = 'Û',
        ['V'] = 'Ṽ',
        ['W'] = 'Ŵ',
        ['X'] = 'Ẋ',
        ['Y'] = 'Ý',
        ['Z'] = 'Ž',
    };

    public static string Transform(string text)
    {
        if (string.IsNullOrWhiteSpace(text))
        {
            return text;
        }
        var builder = new StringBuilder(text.Length * 2);
        builder.Append('[');
        foreach (var c in text)
        {
            builder.Append(Accents.TryGetValue(c, out var accented) ? accented : c);
        }
        var padding = (int)Math.Ceiling(text.Length * 0.4);
        for (var i = 0; i < padding; i++)
        {
            builder.Append(i % 4 == 3 ? ' ' : '~');
        }
        builder.Append(']');
        return builder.ToString();
    }
}
