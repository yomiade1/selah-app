using Avalonia;
using Avalonia.Controls;
using Avalonia.Interactivity;
using Selah.UI.Services;
using Selah.UI.ViewModels;

namespace Selah.UI.Views;

public sealed partial class SlideEditorView : UserControl
{
    public const double ThreeColumnBreakpoint = 960;

    public SlideEditorView()
    {
        InitializeComponent();
        TextScale.Instance.PropertyChanged += (_, _) => UpdateResponsiveLayout();
    }

    private SlideEditorViewModel? Model => DataContext as SlideEditorViewModel;

    protected override void OnSizeChanged(SizeChangedEventArgs e)
    {
        base.OnSizeChanged(e);
        UpdateResponsiveLayout();
    }

    private void UpdateResponsiveLayout()
    {
        var effectiveWidth = Bounds.Width / TextScale.Instance.Factor;
        if (effectiveWidth <= 0)
        {
            return;
        }
        if (effectiveWidth >= ThreeColumnBreakpoint)
        {
            SlideMainGrid.ColumnDefinitions = new ColumnDefinitions("310,*,320");
            Grid.SetColumn(SlideLeftPane, 0);
            Grid.SetRow(SlideLeftPane, 0);
            Grid.SetColumn(SlideCenterPane, 1);
            Grid.SetRow(SlideCenterPane, 0);
            Grid.SetColumn(SlideRightPane, 2);
            Grid.SetRow(SlideRightPane, 0);
        }
        else
        {
            SlideMainGrid.ColumnDefinitions = new ColumnDefinitions("*");
            Grid.SetColumn(SlideLeftPane, 0);
            Grid.SetRow(SlideLeftPane, 0);
            Grid.SetColumn(SlideCenterPane, 0);
            Grid.SetRow(SlideCenterPane, 1);
            Grid.SetColumn(SlideRightPane, 0);
            Grid.SetRow(SlideRightPane, 2);
        }
    }

    private void OnRemoveEffectClick(object? sender, RoutedEventArgs e)
    {
        if (sender is Control { DataContext: SlideEffectItemViewModel effect })
        {
            Model?.RemoveEffectCommand.Execute(effect);
        }
    }
}
