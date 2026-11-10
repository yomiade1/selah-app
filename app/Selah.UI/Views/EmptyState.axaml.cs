using System.Windows.Input;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Interactivity;
using Selah.UI.Services;

namespace Selah.UI.Views;

/// <summary>
/// Contextual empty state (KNOWLEDGE-BASE §5.2–5.3): explains what goes here, offers the primary
/// action button, and links "Learn how" to a guide in HelpPanel.
/// </summary>
public sealed partial class EmptyState : UserControl
{
    public static readonly StyledProperty<string?> TitleProperty =
        AvaloniaProperty.Register<EmptyState, string?>(nameof(Title));

    public static readonly StyledProperty<string?> DescriptionProperty =
        AvaloniaProperty.Register<EmptyState, string?>(nameof(Description));

    public static readonly StyledProperty<string?> ActionTextProperty =
        AvaloniaProperty.Register<EmptyState, string?>(nameof(ActionText));

    public static readonly StyledProperty<ICommand?> ActionCommandProperty =
        AvaloniaProperty.Register<EmptyState, ICommand?>(nameof(ActionCommand));

    public static readonly StyledProperty<object?> ActionCommandParameterProperty =
        AvaloniaProperty.Register<EmptyState, object?>(nameof(ActionCommandParameter));

    public static readonly StyledProperty<string?> HelpArticleIdProperty =
        AvaloniaProperty.Register<EmptyState, string?>(nameof(HelpArticleId));

    public static readonly StyledProperty<string?> HelpAnchorProperty =
        AvaloniaProperty.Register<EmptyState, string?>(nameof(HelpAnchor));

    public EmptyState()
    {
        InitializeComponent();
        Localizer.Instance.PropertyChanged += (_, _) => UpdateState();
        UpdateState();
    }

    public string? Title
    {
        get => GetValue(TitleProperty);
        set => SetValue(TitleProperty, value);
    }

    public string? Description
    {
        get => GetValue(DescriptionProperty);
        set => SetValue(DescriptionProperty, value);
    }

    public string? ActionText
    {
        get => GetValue(ActionTextProperty);
        set => SetValue(ActionTextProperty, value);
    }

    public ICommand? ActionCommand
    {
        get => GetValue(ActionCommandProperty);
        set => SetValue(ActionCommandProperty, value);
    }

    public object? ActionCommandParameter
    {
        get => GetValue(ActionCommandParameterProperty);
        set => SetValue(ActionCommandParameterProperty, value);
    }

    public string? HelpArticleId
    {
        get => GetValue(HelpArticleIdProperty);
        set => SetValue(HelpArticleIdProperty, value);
    }

    public string? HelpAnchor
    {
        get => GetValue(HelpAnchorProperty);
        set => SetValue(HelpAnchorProperty, value);
    }

    public event EventHandler? ActionClicked;

    public event EventHandler? LearnHowClicked;

    protected override void OnPropertyChanged(AvaloniaPropertyChangedEventArgs change)
    {
        base.OnPropertyChanged(change);
        if (change.Property == TitleProperty
            || change.Property == DescriptionProperty
            || change.Property == ActionTextProperty
            || change.Property == ActionCommandProperty
            || change.Property == HelpArticleIdProperty
            || change.Property == HelpAnchorProperty)
        {
            UpdateState();
        }
    }

    private void UpdateState()
    {
        TitleBlock.Text = Title;
        TitleBlock.IsVisible = !string.IsNullOrWhiteSpace(Title);
        DescriptionBlock.Text = Description;
        DescriptionBlock.IsVisible = !string.IsNullOrWhiteSpace(Description);

        var hasAction = !string.IsNullOrWhiteSpace(ActionText);
        ActionButtonText.Text = ActionText;
        ActionButton.IsVisible = hasAction;

        LearnHowTextBlock.Text = Localizer.Instance["help-learn-how"];
        LearnHowButton.IsVisible = !string.IsNullOrWhiteSpace(HelpArticleId) || !string.IsNullOrWhiteSpace(HelpAnchor);
    }

    private void OnActionClick(object? sender, RoutedEventArgs e)
    {
        ActionClicked?.Invoke(this, EventArgs.Empty);
        var command = ActionCommand;
        var param = ActionCommandParameter;
        if (command is not null && command.CanExecute(param))
        {
            command.Execute(param);
        }
    }

    private void OnLearnHowClick(object? sender, RoutedEventArgs e)
    {
        LearnHowClicked?.Invoke(this, EventArgs.Empty);
        var host = Help.FindHost(this);
        if (host is null)
        {
            return;
        }
        if (!string.IsNullOrWhiteSpace(HelpArticleId))
        {
            _ = host.OpenHelpArticleAsync(HelpArticleId);
        }
        else if (!string.IsNullOrWhiteSpace(HelpAnchor))
        {
            _ = host.OpenHelpForAnchorsAsync([HelpAnchor]);
        }
    }
}
