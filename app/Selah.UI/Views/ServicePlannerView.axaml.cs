using Avalonia;
using Avalonia.Controls;
using Selah.UI.Services;

namespace Selah.UI.Views;

public sealed partial class ServicePlannerView : UserControl
{
    public const double ThreeColumnBreakpoint = 960;

    public ServicePlannerView()
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
            ServiceMainGrid.ColumnDefinitions = new ColumnDefinitions("240,*,310");
            Grid.SetColumn(ServiceLeftPane, 0);
            Grid.SetRow(ServiceLeftPane, 0);
            Grid.SetColumn(ServiceCenterPane, 1);
            Grid.SetRow(ServiceCenterPane, 0);
            Grid.SetColumn(ServiceRightPane, 2);
            Grid.SetRow(ServiceRightPane, 0);
        }
        else
        {
            ServiceMainGrid.ColumnDefinitions = new ColumnDefinitions("*");
            Grid.SetColumn(ServiceLeftPane, 0);
            Grid.SetRow(ServiceLeftPane, 0);
            Grid.SetColumn(ServiceCenterPane, 0);
            Grid.SetRow(ServiceCenterPane, 1);
            Grid.SetColumn(ServiceRightPane, 0);
            Grid.SetRow(ServiceRightPane, 2);
        }
    }
}
