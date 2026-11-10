using Avalonia;
using Avalonia.Controls;
using Avalonia.Interactivity;
using Selah.Contracts.V1;
using Selah.UI.Services;

namespace Selah.UI.Views;

/// <summary>
/// What's this? (Shift+F1) non-blocking mode (KNOWLEDGE-BASE §5.2–5.4, M2.8): inspecting any
/// control displays its anchor's L1 summary popover with a "See guide: &lt;title&gt;" link.
/// </summary>
public sealed partial class WhatsThisOverlay : UserControl
{
    public static readonly StyledProperty<bool> IsActiveProperty =
        AvaloniaProperty.Register<WhatsThisOverlay, bool>(nameof(IsActive));

    private string? currentArticleId;
    private string? currentAnchor;

    public WhatsThisOverlay()
    {
        InitializeComponent();
        UpdateVisibility();
    }

    public bool IsActive
    {
        get => GetValue(IsActiveProperty);
        set => SetValue(IsActiveProperty, value);
    }

    public string? InspectedAnchor => currentAnchor;

    public Article? LastInspectedArticle { get; private set; }

    public event EventHandler? Closed;

    protected override void OnPropertyChanged(AvaloniaPropertyChangedEventArgs change)
    {
        base.OnPropertyChanged(change);
        if (change.Property == IsActiveProperty)
        {
            UpdateVisibility();
            if (!IsActive)
            {
                ClearPopover();
            }
        }
    }

    private void UpdateVisibility() => IsVisible = IsActive;

    public void ClearPopover()
    {
        currentArticleId = null;
        currentAnchor = null;
        LastInspectedArticle = null;
        WhatsThisPopoverCard.IsVisible = false;
        WhatsThisPopoverTitle.Text = string.Empty;
        WhatsThisPopoverSummary.Text = string.Empty;
        WhatsThisSeeGuideButton.IsVisible = false;
    }

    /// <summary>Inspects <paramref name="target"/> and displays the popover for its nearest <see cref="Help.AnchorProperty"/>.</summary>
    public async Task<bool> InspectVisualAsync(Visual? target)
    {
        var anchor = Help.FindNearestAnchor(target);
        currentAnchor = anchor;
        currentArticleId = null;
        LastInspectedArticle = null;

        if (string.IsNullOrWhiteSpace(anchor))
        {
            WhatsThisPopoverTitle.Text = Localizer.Instance["help-title"];
            WhatsThisPopoverSummary.Text = Localizer.Instance["whats-this-no-anchor"];
            WhatsThisSeeGuideButton.IsVisible = false;
            WhatsThisPopoverCard.IsVisible = true;
            return false;
        }

        var host = Help.FindHost(this);
        Article? article = null;
        if (host?.HelpSource is { } source)
        {
            try
            {
                var found = await source.HelpForContextAsync([anchor]);
                article = found.FirstOrDefault();
            }
            catch (Exception error) when (error is TimeoutException or IOException or InvalidOperationException)
            {
                article = null;
            }
        }

        LastInspectedArticle = article;
        if (article is not null)
        {
            currentArticleId = article.Id;
            WhatsThisPopoverTitle.Text = article.Title;
            WhatsThisPopoverSummary.Text = string.IsNullOrWhiteSpace(article.Summary)
                ? article.Title
                : article.Summary;
            WhatsThisSeeGuideText.Text = Localizer.Instance.Get("help-see-guide", ("title", article.Title));
            WhatsThisSeeGuideButton.IsVisible = true;
        }
        else
        {
            WhatsThisPopoverTitle.Text = anchor;
            WhatsThisPopoverSummary.Text = Localizer.Instance["help-not-available"];
            WhatsThisSeeGuideButton.IsVisible = false;
        }
        WhatsThisPopoverCard.IsVisible = true;
        return article is not null;
    }

    private void OnCloseClick(object? sender, RoutedEventArgs e)
    {
        ClearPopover();
        Closed?.Invoke(this, EventArgs.Empty);
    }

    private async void OnSeeGuideClick(object? sender, RoutedEventArgs e)
    {
        var host = Help.FindHost(this);
        var articleId = currentArticleId;
        var anchor = currentAnchor;
        ClearPopover();
        Closed?.Invoke(this, EventArgs.Empty);
        if (host is null)
        {
            return;
        }
        if (!string.IsNullOrWhiteSpace(articleId))
        {
            await host.OpenHelpArticleAsync(articleId);
        }
        else if (!string.IsNullOrWhiteSpace(anchor))
        {
            await host.OpenHelpForAnchorsAsync([anchor]);
        }
    }
}
