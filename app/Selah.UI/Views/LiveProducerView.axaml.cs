using System.Globalization;
using Avalonia.Controls;
using Avalonia.Data.Converters;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Markup.Xaml;
using Selah.UI.Services;
using Selah.UI.ViewModels;

namespace Selah.UI.Views;

/// <summary>Producer mode (M4.4). Keyboard-first: see <see cref="LiveKeys"/>.</summary>
public partial class LiveProducerView : UserControl
{
    public LiveProducerView()
    {
        InitializeComponent();
        AddHandler(KeyDownEvent, OnKeyDown, RoutingStrategies.Tunnel);
    }

    private LiveProducerViewModel? Model => DataContext as LiveProducerViewModel;

    private void InitializeComponent() => AvaloniaXamlLoader.Load(this);

    private void OnKeyDown(object? sender, KeyEventArgs e)
    {
        if (Model is { } model && LiveKeys.Handle(model, e.Key, e.KeyModifiers, e.Source))
        {
            e.Handled = true;
        }
    }

    private void OnLayoutPresetChosen(object? sender, SelectionChangedEventArgs e)
    {
        if (sender is ComboBox { SelectedItem: ProducerLayoutState preset } box && Model is { } model)
        {
            model.ApplyLayoutPresetCommand.Execute(preset);
            box.SelectedItem = null;
        }
    }
}

/// <summary>
/// The Live keyboard map (M4.4, prototype 01). It never takes keys from a text box, so typing in
/// search or help still works; F12 is handled by the window for every view (SAFE-03).
/// </summary>
public static class LiveKeys
{
    public static bool Handle(LiveProducerViewModel model, Key key, KeyModifiers modifiers, object? source)
    {
        if (source is TextBox)
        {
            return false;
        }
        var ctrl = (modifiers & (KeyModifiers.Control | KeyModifiers.Meta)) != 0;
        var shift = (modifiers & KeyModifiers.Shift) != 0;
        switch (key)
        {
            case Key.Space or Key.Right or Key.Down or Key.PageDown when !ctrl && !shift:
                model.NextCommand.Execute(null);
                return true;
            case Key.Left or Key.Up or Key.PageUp when !ctrl && !shift:
                model.PreviousCommand.Execute(null);
                return true;
            case Key.Right when shift:
                model.NextItemCommand.Execute(null);
                return true;
            case Key.Left when shift:
                model.PreviousItemCommand.Execute(null);
                return true;
            case Key.F9:
                model.ShowLogoCommand.Execute(null);
                return true;
            case Key.F10:
                model.ClearTextCommand.Execute(null);
                return true;
            case Key.F11:
                model.ClearBackgroundCommand.Execute(null);
                return true;
            case Key.Z when ctrl && shift:
            case Key.Y when ctrl:
                model.RedoCommand.Execute(null);
                return true;
            case Key.Z when ctrl:
                model.UndoCommand.Execute(null);
                return true;
            case Key.L when ctrl:
                model.ToggleBoothLockCommand.Execute(null);
                return true;
        }
        if (ctrl)
        {
            return false;
        }
        var name = HotkeyName(key);
        if (name is null)
        {
            return false;
        }
        _ = model.HotkeyAsync(name, shift);
        return true;
    }

    /// <summary>1–9 and A–Z become section, macro or (with Shift) look hotkeys.</summary>
    public static string? HotkeyName(Key key) => key switch
    {
        >= Key.D1 and <= Key.D9 => ((int)(key - Key.D1) + 1).ToString(CultureInfo.InvariantCulture),
        >= Key.NumPad1 and <= Key.NumPad9 => ((int)(key - Key.NumPad1) + 1).ToString(CultureInfo.InvariantCulture),
        >= Key.A and <= Key.Z => ((char)('A' + (key - Key.A))).ToString(),
        _ => null,
    };
}

/// <summary>Shows a layout preset's name in the person's language.</summary>
public sealed class LayoutNameConverter : IValueConverter
{
    public static LayoutNameConverter Instance { get; } = new();

    public object? Convert(object? value, Type targetType, object? parameter, CultureInfo culture) =>
        Localizer.Instance[$"live-layout-{value as string ?? "standard"}"];

    public object? ConvertBack(object? value, Type targetType, object? parameter, CultureInfo culture) =>
        throw new NotSupportedException();
}
