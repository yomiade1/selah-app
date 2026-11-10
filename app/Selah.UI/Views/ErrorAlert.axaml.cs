using Avalonia;
using Avalonia.Controls;
using Avalonia.Interactivity;
using Selah.UI.Services;

namespace Selah.UI.Views;

/// <summary>
/// Plain-language error banner (KNOWLEDGE-BASE §5.2–5.3): explains what went wrong and offers
/// a "How to fix" link straight to core's mapped troubleshooting article.
/// </summary>
public sealed partial class ErrorAlert : UserControl
{
    public static readonly StyledProperty<string?> MessageProperty =
        AvaloniaProperty.Register<ErrorAlert, string?>(nameof(Message));

    public static readonly StyledProperty<string?> FixArticleIdProperty =
        AvaloniaProperty.Register<ErrorAlert, string?>(nameof(FixArticleId));

    public ErrorAlert()
    {
        InitializeComponent();
        Localizer.Instance.PropertyChanged += (_, _) => UpdateState();
        UpdateState();
    }

    public string? Message
    {
        get => GetValue(MessageProperty);
        set => SetValue(MessageProperty, value);
    }

    public string? FixArticleId
    {
        get => GetValue(FixArticleIdProperty);
        set => SetValue(FixArticleIdProperty, value);
    }

    public event EventHandler<string>? HowToFixClicked;

    protected override void OnPropertyChanged(AvaloniaPropertyChangedEventArgs change)
    {
        base.OnPropertyChanged(change);
        if (change.Property == MessageProperty || change.Property == FixArticleIdProperty)
        {
            UpdateState();
        }
    }

    private void UpdateState()
    {
        MessageBlock.Text = Message;
        IsVisible = !string.IsNullOrWhiteSpace(Message);
        HowToFixText.Text = Localizer.Instance["help-how-to-fix"];
        HowToFixButton.IsVisible = !string.IsNullOrWhiteSpace(FixArticleId);
    }

    private void OnHowToFixClick(object? sender, RoutedEventArgs e)
    {
        if (string.IsNullOrWhiteSpace(FixArticleId))
        {
            return;
        }
        HowToFixClicked?.Invoke(this, FixArticleId);
        var host = Help.FindHost(this);
        if (host is not null)
        {
            _ = host.OpenHelpArticleAsync(FixArticleId);
        }
    }
}
