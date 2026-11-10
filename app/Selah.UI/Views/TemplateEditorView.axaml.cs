using Avalonia;
using Avalonia.Controls;
using Selah.UI.Services;

namespace Selah.UI.Views;

public sealed partial class TemplateEditorView : UserControl
{
    public const double ThreeColumnBreakpoint = 960;

    public TemplateEditorView()
    {
        InitializeComponent();
        TextScale.Instance.PropertyChanged += (_, _) => UpdateResponsiveLayout();
    }

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
            TemplateMainGrid.ColumnDefinitions = new ColumnDefinitions("260,*,310");
            Grid.SetColumn(TemplateLeftPane, 0);
            Grid.SetRow(TemplateLeftPane, 0);
            Grid.SetColumn(TemplateCenterPane, 1);
            Grid.SetRow(TemplateCenterPane, 0);
            Grid.SetColumn(TemplateRightPane, 2);
            Grid.SetRow(TemplateRightPane, 0);
        }
        else
        {
            TemplateMainGrid.ColumnDefinitions = new ColumnDefinitions("*");
            Grid.SetColumn(TemplateLeftPane, 0);
            Grid.SetRow(TemplateLeftPane, 0);
            Grid.SetColumn(TemplateCenterPane, 0);
            Grid.SetRow(TemplateCenterPane, 1);
            Grid.SetColumn(TemplateRightPane, 0);
            Grid.SetRow(TemplateRightPane, 2);
        }
    }
}
