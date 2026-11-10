using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Threading;
using Selah.UI.ViewModels;

namespace Selah.UI.Views;

public sealed partial class CommandPaletteView : UserControl
{
    public CommandPaletteView()
    {
        InitializeComponent();
        PropertyChanged += (_, args) =>
        {
            if (args.Property == IsVisibleProperty && IsVisible)
            {
                Dispatcher.UIThread.Post(() => PaletteSearchBox.Focus());
            }
        };
    }

    private CommandPaletteViewModel? Model => DataContext as CommandPaletteViewModel;

    private void OnBackdropPressed(object? sender, PointerPressedEventArgs e)
    {
        if (Equals(e.Source, PaletteBackdrop))
        {
            Model?.CloseCommand.Execute(null);
        }
    }

    private void OnSearchKeyDown(object? sender, KeyEventArgs e)
    {
        if (Model is null)
        {
            return;
        }
        switch (e.Key)
        {
            case Key.Escape:
                Model.CloseCommand.Execute(null);
                e.Handled = true;
                break;
            case Key.Down:
                Model.MoveSelection(1);
                e.Handled = true;
                break;
            case Key.Up:
                Model.MoveSelection(-1);
                e.Handled = true;
                break;
            case Key.Enter:
                Model.ActivateSelectedCommand.Execute(null);
                e.Handled = true;
                break;
        }
    }

    private void OnListDoubleTapped(object? sender, RoutedEventArgs e) =>
        Model?.ActivateSelectedCommand.Execute(null);

    private void OnPaletteItemClick(object? sender, RoutedEventArgs e)
    {
        if (sender is Control { DataContext: PaletteItem item })
        {
            Model?.ActivateCommand.Execute(item);
        }
    }
}
