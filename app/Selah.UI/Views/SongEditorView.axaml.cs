using Avalonia;
using Avalonia.Controls;
using Avalonia.Interactivity;
using Selah.UI.Services;
using Selah.UI.ViewModels;

namespace Selah.UI.Views;

public sealed partial class SongEditorView : UserControl
{
    public const double ThreeColumnBreakpoint = 960;

    public SongEditorView()
    {
        InitializeComponent();
        TextScale.Instance.PropertyChanged += (_, _) => UpdateResponsiveLayout();
    }

    private SongEditorViewModel? Model => DataContext as SongEditorViewModel;

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
            EditorMainGrid.ColumnDefinitions = new ColumnDefinitions("340,*,310");
            Grid.SetColumn(EditorLeftPane, 0);
            Grid.SetRow(EditorLeftPane, 0);
            Grid.SetColumn(EditorCenterPane, 1);
            Grid.SetRow(EditorCenterPane, 0);
            Grid.SetColumn(EditorRightPane, 2);
            Grid.SetRow(EditorRightPane, 0);
        }
        else
        {
            EditorMainGrid.ColumnDefinitions = new ColumnDefinitions("*");
            Grid.SetColumn(EditorLeftPane, 0);
            Grid.SetRow(EditorLeftPane, 0);
            Grid.SetColumn(EditorCenterPane, 0);
            Grid.SetRow(EditorCenterPane, 1);
            Grid.SetColumn(EditorRightPane, 0);
            Grid.SetRow(EditorRightPane, 2);
        }
    }

    private void OnMoveSectionUpClick(object? sender, RoutedEventArgs e)
    {
        if (sender is Control { DataContext: SongSectionItemViewModel section })
        {
            Model?.MoveSectionUpCommand.Execute(section);
        }
    }

    private void OnMoveSectionDownClick(object? sender, RoutedEventArgs e)
    {
        if (sender is Control { DataContext: SongSectionItemViewModel section })
        {
            Model?.MoveSectionDownCommand.Execute(section);
        }
    }

    private void OnRemoveSectionClick(object? sender, RoutedEventArgs e)
    {
        if (sender is Control { DataContext: SongSectionItemViewModel section })
        {
            Model?.RemoveSectionCommand.Execute(section);
        }
    }

    private void OnAppendSectionClick(object? sender, RoutedEventArgs e)
    {
        if (sender is Control { DataContext: SongSectionItemViewModel section })
        {
            Model?.AppendSectionToArrangementCommand.Execute(section);
        }
    }

    private void OnRemoveArrangementTokenClick(object? sender, RoutedEventArgs e)
    {
        if (sender is Control { DataContext: ArrangementTokenViewModel token })
        {
            Model?.RemoveArrangementTokenCommand.Execute(token);
        }
    }

    private void OnStageSlideClick(object? sender, RoutedEventArgs e)
    {
        if (sender is Control { DataContext: SlidePreviewCard slide })
        {
            Model?.StageSlideCommand.Execute(slide);
        }
    }
}
