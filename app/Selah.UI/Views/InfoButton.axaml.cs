using Avalonia;
using Avalonia.Controls;
using Avalonia.Interactivity;
using Avalonia.Media;
using Selah.Contracts.V1;
using Selah.UI.Services;
using Selah.UI.ViewModels;

namespace Selah.UI.Views;

/// <summary>
/// L1 contextual help button (KNOWLEDGE-BASE §5.2): displays a non-blocking popover for an
/// anchor or article with its summary and a "See guide: &lt;title&gt;" link to the Help panel.
/// </summary>
public sealed partial class InfoButton : UserControl
{
    public static readonly StyledProperty<string?> AnchorProperty =
        AvaloniaProperty.Register<InfoButton, string?>(nameof(Anchor));

    public static readonly StyledProperty<string?> ArticleIdProperty =
        AvaloniaProperty.Register<InfoButton, string?>(nameof(ArticleId));

    public static readonly StyledProperty<string?> FallbackTitleProperty =
        AvaloniaProperty.Register<InfoButton, string?>(nameof(FallbackTitle));

    public static readonly StyledProperty<string?> FallbackSummaryProperty =
        AvaloniaProperty.Register<InfoButton, string?>(nameof(FallbackSummary));

    private Flyout? activeFlyout;

    public InfoButton()
    {
        InitializeComponent();
    }

    public string? Anchor
    {
        get => GetValue(AnchorProperty);
        set => SetValue(AnchorProperty, value);
    }

    public string? ArticleId
    {
        get => GetValue(ArticleIdProperty);
        set => SetValue(ArticleIdProperty, value);
    }

    public string? FallbackTitle
    {
        get => GetValue(FallbackTitleProperty);
        set => SetValue(FallbackTitleProperty, value);
    }

    public string? FallbackSummary
    {
        get => GetValue(FallbackSummaryProperty);
        set => SetValue(FallbackSummaryProperty, value);
    }

    public Article? LastLoadedArticle { get; private set; }

    public bool IsPopoverOpen => activeFlyout?.IsOpen == true;

    public event EventHandler<Article?>? PopoverShown;

    private void OnTriggerClick(object? sender, RoutedEventArgs e) => _ = ShowPopoverAsync();

    public async Task<Article?> ShowPopoverAsync()
    {
        var host = Help.FindHost(this);
        var source = host?.HelpSource;
        Article? article = null;

        if (source is not null)
        {
            try
            {
                if (!string.IsNullOrWhiteSpace(ArticleId))
                {
                    article = await source.GetArticleAsync(ArticleId);
                }
                else if (!string.IsNullOrWhiteSpace(Anchor))
                {
                    var found = await source.HelpForContextAsync([Anchor]);
                    article = found.FirstOrDefault();
                }
            }
            catch (Exception error) when (error is TimeoutException or IOException or InvalidOperationException)
            {
                article = null;
            }
        }

        LastLoadedArticle = article;
        var title = article?.Title;
        if (string.IsNullOrWhiteSpace(title))
        {
            title = !string.IsNullOrWhiteSpace(FallbackTitle) ? FallbackTitle : Localizer.Instance["help-title"];
        }
        var summary = article?.Summary;
        if (string.IsNullOrWhiteSpace(summary))
        {
            summary = !string.IsNullOrWhiteSpace(FallbackSummary) ? FallbackSummary : Localizer.Instance["help-not-available"];
        }

        var seeGuideText = article is not null
            ? Localizer.Instance.Get("help-see-guide", ("title", title))
            : Localizer.Instance["help-see-guide-short"];

        var stack = new StackPanel { MaxWidth = 340, Spacing = 8 };
        stack.Children.Add(new TextBlock { Classes = { "label" }, TextWrapping = TextWrapping.Wrap, Text = title });
        stack.Children.Add(new TextBlock { TextWrapping = TextWrapping.Wrap, Text = summary });

        var guideButton = new Button
        {
            Name = "SeeGuideButton",
            Classes = { "link" },
            Content = new TextBlock { Text = seeGuideText, TextDecorations = TextDecorations.Underline, TextWrapping = TextWrapping.Wrap },
            IsVisible = article is not null || !string.IsNullOrWhiteSpace(ArticleId) || !string.IsNullOrWhiteSpace(Anchor),
        };
        guideButton.Click += (_, _) => _ = OpenGuideAsync();
        stack.Children.Add(guideButton);

        var flyout = new Flyout
        {
            Placement = PlacementMode.BottomEdgeAlignedLeft,
            Content = stack,
        };
        activeFlyout = flyout;
        flyout.ShowAt(TriggerButton);
        PopoverShown?.Invoke(this, article);
        return article;
    }

    public async Task OpenGuideAsync()
    {
        activeFlyout?.Hide();
        var host = Help.FindHost(this);
        if (host is null)
        {
            return;
        }
        var targetId = LastLoadedArticle?.Id ?? ArticleId;
        if (!string.IsNullOrWhiteSpace(targetId))
        {
            await host.OpenHelpArticleAsync(targetId);
        }
        else if (!string.IsNullOrWhiteSpace(Anchor))
        {
            await host.OpenHelpForAnchorsAsync([Anchor]);
        }
    }
}
