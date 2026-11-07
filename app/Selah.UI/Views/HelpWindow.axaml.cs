using Avalonia.Controls;
using Selah.UI.ViewModels;

namespace Selah.UI.Views;

public sealed partial class HelpWindow : Window
{
    public HelpWindow()
    {
        InitializeComponent();
        DataContextChanged += (_, _) =>
        {
            if (DataContext is HelpViewModel model)
            {
                model.PropertyChanged += (_, e) =>
                {
                    if (e.PropertyName == nameof(HelpViewModel.Article))
                    {
                        ShowArticle(model);
                    }
                };
                ShowArticle(model);
            }
        };
    }

    private void ShowArticle(HelpViewModel model)
    {
        ArticleHost.Content = model.Article is { } article
            ? ArticleRenderer.Render(article.AstJson, id => _ = model.OpenAsync(id))
            : null;
        ArticleScroller.Offset = default;
    }
}
