using System.Globalization;
using Avalonia.Controls;
using Avalonia.Data.Converters;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Markup.Xaml;
using Avalonia.Media;
using Selah.UI.ViewModels;

namespace Selah.UI.Views;

/// <summary>
/// Simple mode (M4.5, prototype 02). The same keyboard map as Producer mode (<see cref="LiveKeys"/>);
/// core refuses anything a role-locked volunteer may not do (SEC-14).
/// </summary>
public partial class SimpleModeView : UserControl
{
    public SimpleModeView()
    {
        InitializeComponent();
        AddHandler(KeyDownEvent, OnKeyDown, RoutingStrategies.Tunnel);
    }

    private SimpleModeViewModel? Model => DataContext as SimpleModeViewModel;

    private void InitializeComponent() => AvaloniaXamlLoader.Load(this);

    private void OnKeyDown(object? sender, KeyEventArgs e)
    {
        if (Model is { } model && LiveKeys.Handle(model.Producer, e.Key, e.KeyModifiers, e.Source))
        {
            e.Handled = true;
        }
    }
}

/// <summary>Bold for the current step, normal otherwise (the step also has its own marker).</summary>
public sealed class BoldWhenTrue : IValueConverter
{
    public static BoldWhenTrue Instance { get; } = new();

    public object? Convert(object? value, Type targetType, object? parameter, CultureInfo culture) =>
        value is true ? FontWeight.Bold : FontWeight.Normal;

    public object? ConvertBack(object? value, Type targetType, object? parameter, CultureInfo culture) =>
        throw new NotSupportedException();
}
