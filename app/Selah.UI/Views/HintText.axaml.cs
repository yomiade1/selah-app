using Avalonia;
using Avalonia.Controls;

namespace Selah.UI.Views;

/// <summary>L0 contextual help (KNOWLEDGE-BASE §5.2): plain hint text below a setting or field.</summary>
public sealed partial class HintText : UserControl
{
    public static readonly StyledProperty<string?> TextProperty =
        AvaloniaProperty.Register<HintText, string?>(nameof(Text));

    public HintText()
    {
        InitializeComponent();
        UpdateText();
    }

    public string? Text
    {
        get => GetValue(TextProperty);
        set => SetValue(TextProperty, value);
    }

    protected override void OnPropertyChanged(AvaloniaPropertyChangedEventArgs change)
    {
        base.OnPropertyChanged(change);
        if (change.Property == TextProperty)
        {
            UpdateText();
        }
    }

    private void UpdateText()
    {
        var value = Text;
        HintBlock.Text = value;
        IsVisible = !string.IsNullOrWhiteSpace(value);
    }
}
