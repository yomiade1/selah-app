using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Selah.Contracts.V1;
using Selah.UI.Services;

namespace Selah.UI.ViewModels;

/// <summary>Where help articles come from (core over IPC; a fake in tests).</summary>
public interface IHelpSource
{
    Task<IReadOnlyList<Article>> SearchHelpAsync(string query, IEnumerable<string> anchors);

    Task<Article?> GetArticleAsync(string id);
}

/// <summary>The help window: search, results and the open article (KNOWLEDGE-BASE §5–6).</summary>
public sealed partial class HelpViewModel : ObservableObject
{
    private readonly IHelpSource? source;

    public HelpViewModel(IHelpSource? source)
    {
        this.source = source;
    }

    public ObservableCollection<Article> Results { get; } = [];

    [ObservableProperty]
    private string query = string.Empty;

    [ObservableProperty]
    private Article? article;

    [ObservableProperty]
    private Article? selectedResult;

    /// <summary>Shown when a search finds nothing or help can't be reached.</summary>
    [ObservableProperty]
    private string? message;

    partial void OnSelectedResultChanged(Article? value)
    {
        if (value is not null)
        {
            _ = OpenAsync(value.Id);
        }
    }

    [RelayCommand]
    private async Task SearchAsync()
    {
        if (source is null || string.IsNullOrWhiteSpace(Query))
        {
            return;
        }
        try
        {
            var found = await source.SearchHelpAsync(Query, []);
            Results.Clear();
            foreach (var result in found)
            {
                Results.Add(result);
            }
            Message = found.Count == 0 ? Localizer.Instance["help-no-results"] : null;
        }
        catch (Exception error) when (error is TimeoutException or IOException or InvalidOperationException)
        {
            Message = Localizer.Instance["help-not-available"];
        }
    }

    public async Task OpenAsync(string id)
    {
        if (source is null)
        {
            return;
        }
        try
        {
            Article = await source.GetArticleAsync(id);
            Message = Article is null ? Localizer.Instance["help-not-available"] : null;
        }
        catch (Exception error) when (error is TimeoutException or IOException or InvalidOperationException)
        {
            Message = Localizer.Instance["help-not-available"];
        }
    }
}
