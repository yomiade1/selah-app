using System.ComponentModel;
using Avalonia.Controls;
using Avalonia.Input;
using Selah.UI.ViewModels;

namespace Selah.UI.Views;

/// <summary>Contextual help panel side view (KNOWLEDGE-BASE §5.2–5.4, M2.8).</summary>
public sealed partial class HelpPanelView : UserControl
{
    private HelpPanelViewModel? currentModel;

    public HelpPanelView()
    {
        InitializeComponent();
    }

    protected override void OnDataContextChanged(EventArgs e)
    {
        base.OnDataContextChanged(e);
        if (currentModel is not null)
        {
            currentModel.PropertyChanged -= OnModelPropertyChanged;
        }
        currentModel = DataContext as HelpPanelViewModel;
        if (currentModel is not null)
        {
            currentModel.PropertyChanged += OnModelPropertyChanged;
            RenderArticle(currentModel);
        }
        else
        {
            HelpArticleHost.Content = null;
        }
    }

    private void OnModelPropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(HelpPanelViewModel.CurrentArticle) && sender is HelpPanelViewModel model)
        {
            RenderArticle(model);
        }
    }

    private void RenderArticle(HelpPanelViewModel model)
    {
        HelpArticleHost.Content = model.CurrentArticle is { AstJson.Length: > 0 } article
            ? ArticleRenderer.Render(article.AstJson, id => _ = model.OpenArticleAsync(id))
            : null;
        HelpArticleScroller.Offset = default;
    }

    private void OnSearchKeyDown(object? sender, KeyEventArgs e)
    {
        // §5.4: live control function keys (F9-F12) never get eaten by help search.
        if (e.Key is Key.F9 or Key.F10 or Key.F11 or Key.F12)
        {
            return;
        }
        if (e.Key == Key.Enter && DataContext is HelpPanelViewModel model)
        {
            e.Handled = true;
            _ = model.SearchCommand.ExecuteAsync(null);
        }
    }
}
