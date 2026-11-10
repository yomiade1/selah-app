using System.Collections.Concurrent;
using System.Globalization;
using Avalonia.Data.Converters;
using Avalonia.Media;

namespace Selah.UI.Views;

/// <summary>Value converters shared across Selah views (compiled-binding compatible).</summary>
public static class UiConverters
{
    public static readonly IValueConverter HexBrush = new HexBrushConverter();

    private sealed class HexBrushConverter : IValueConverter
    {
        private static readonly ConcurrentDictionary<string, SolidColorBrush> Cache = new(StringComparer.OrdinalIgnoreCase);
        private static readonly SolidColorBrush Fallback = new(Color.Parse("#71767A"));

        public object Convert(object? value, Type targetType, object? parameter, CultureInfo culture)
        {
            if (value is not string hex || string.IsNullOrWhiteSpace(hex))
            {
                return Fallback;
            }
            return Cache.GetOrAdd(hex.Trim(), key =>
            {
                try
                {
                    return new SolidColorBrush(Color.Parse(key));
                }
                catch (FormatException)
                {
                    return Fallback;
                }
            });
        }

        public object ConvertBack(object? value, Type targetType, object? parameter, CultureInfo culture) =>
            Avalonia.Data.BindingOperations.DoNothing;
    }
}
