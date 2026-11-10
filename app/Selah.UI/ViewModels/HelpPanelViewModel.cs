using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Selah.Contracts.V1;
using Selah.UI.Services;

namespace Selah.UI.ViewModels;

/// <summary>
/// Contextual Help panel (KNOWLEDGE-BASE §5.2–5.4, M2.8): non-blocking side panel displaying
/// "For this screen" articles for visible anchors, live-safe search, native article rendering,
/// and local "Did this help?" feedback.
/// </summary>
public sealed partial class HelpPanelViewModel : ObservableObject
{
    private readonly IHelpSource? source;

    public HelpPanelViewModel(IHelpSource? source)
    {
        this.source = source;
        Localizer.Instance.PropertyChanged += (_, _) =>
        {
            if (FeedbackSubmitted)
            {
                FeedbackMessage = Localizer.Instance["help-feedback-thanks"];
            }
        };
    }

    public IHelpSource? Source => source;

    public ObservableCollection<Article> ContextArticles { get; } = [];

    public ObservableCollection<Article> SearchResults { get; } = [];

    public List<string> VisibleAnchors { get; } = [];

    [ObservableProperty]
    private bool isOpen;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsShowingSearch))]
    private string query = string.Empty;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsShowingArticle))]
    [NotifyPropertyChangedFor(nameof(IsShowingList))]
    private Article? currentArticle;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasStatusMessage))]
    private string? statusMessage;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(ShowFeedbackButtons))]
    [NotifyPropertyChangedFor(nameof(HasFeedbackMessage))]
    private bool feedbackSubmitted;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasFeedbackMessage))]
    private string? feedbackMessage;

    [ObservableProperty]
    private Article? selectedContextArticle;

    [ObservableProperty]
    private Article? selectedSearchArticle;

    public bool IsShowingArticle => CurrentArticle is not null;

    public bool IsShowingList => CurrentArticle is null;

    public bool IsShowingSearch => !string.IsNullOrWhiteSpace(Query);

    public bool HasContextArticles => ContextArticles.Count > 0;

    public bool HasSearchResults => SearchResults.Count > 0;

    public bool HasStatusMessage => !string.IsNullOrWhiteSpace(StatusMessage);

    public bool ShowFeedbackButtons => IsShowingArticle && !FeedbackSubmitted;

    public bool HasFeedbackMessage => !string.IsNullOrWhiteSpace(FeedbackMessage);

    partial void OnQueryChanged(string value)
    {
        if (string.IsNullOrWhiteSpace(value))
        {
            SearchResults.Clear();
            OnPropertyChanged(nameof(HasSearchResults));
            StatusMessage = null;
        }
    }

    partial void OnSelectedContextArticleChanged(Article? value)
    {
        if (value is not null)
        {
            SelectedContextArticle = null;
            _ = OpenArticleAsync(value.Id);
        }
    }

    partial void OnSelectedSearchArticleChanged(Article? value)
    {
        if (value is not null)
        {
            SelectedSearchArticle = null;
            _ = OpenArticleAsync(value.Id);
        }
    }

    public async Task ToggleAsync(IEnumerable<string> visibleAnchors)
    {
        if (IsOpen)
        {
            IsOpen = false;
            return;
        }
        await OpenForAnchorsAsync(visibleAnchors);
    }

    public async Task OpenForAnchorsAsync(IEnumerable<string> anchors)
    {
        IsOpen = true;
        CurrentArticle = null;
        Query = string.Empty;
        StatusMessage = null;
        ResetFeedback();
        await RefreshContextAsync(anchors);
    }

    public async Task RefreshContextAsync(IEnumerable<string> anchors)
    {
        VisibleAnchors.Clear();
        VisibleAnchors.AddRange(anchors.Where(a => !string.IsNullOrWhiteSpace(a)).Distinct(StringComparer.Ordinal));

        if (source is null)
        {
            ContextArticles.Clear();
            OnPropertyChanged(nameof(HasContextArticles));
            return;
        }

        try
        {
            var found = await source.HelpForContextAsync(VisibleAnchors);
            ContextArticles.Clear();
            foreach (var article in found)
            {
                ContextArticles.Add(article);
            }
            OnPropertyChanged(nameof(HasContextArticles));
        }
        catch (Exception error) when (error is TimeoutException or IOException or InvalidOperationException)
        {
            ContextArticles.Clear();
            OnPropertyChanged(nameof(HasContextArticles));
            StatusMessage = Localizer.Instance["help-not-available"];
        }
    }

    [RelayCommand]
    public async Task SearchAsync()
    {
        if (source is null || string.IsNullOrWhiteSpace(Query))
        {
            SearchResults.Clear();
            OnPropertyChanged(nameof(HasSearchResults));
            return;
        }

        try
        {
            CurrentArticle = null;
            ResetFeedback();
            var found = await source.SearchHelpAsync(Query.Trim(), VisibleAnchors);
            SearchResults.Clear();
            foreach (var article in found)
            {
                SearchResults.Add(article);
            }
            OnPropertyChanged(nameof(HasSearchResults));
            StatusMessage = found.Count == 0 ? Localizer.Instance["help-no-results"] : null;
        }
        catch (Exception error) when (error is TimeoutException or IOException or InvalidOperationException)
        {
            StatusMessage = Localizer.Instance["help-not-available"];
        }
    }

    public async Task OpenArticleAsync(string id)
    {
        IsOpen = true;
        ResetFeedback();
        if (source is null)
        {
            StatusMessage = Localizer.Instance["help-not-available"];
            return;
        }

        try
        {
            var loaded = await source.GetArticleAsync(id);
            CurrentArticle = loaded;
            StatusMessage = loaded is null ? Localizer.Instance["help-not-available"] : null;
        }
        catch (Exception error) when (error is TimeoutException or IOException or InvalidOperationException)
        {
            StatusMessage = Localizer.Instance["help-not-available"];
        }
    }

    [RelayCommand]
    private void BackToList()
    {
        CurrentArticle = null;
        ResetFeedback();
        StatusMessage = null;
    }

    [RelayCommand]
    private void Close() => IsOpen = false;

    [RelayCommand]
    private async Task MarkHelpfulAsync() => await SubmitFeedbackAsync(true);

    [RelayCommand]
    private async Task MarkNotHelpfulAsync() => await SubmitFeedbackAsync(false);

    public async Task SubmitFeedbackAsync(bool helpful)
    {
        if (CurrentArticle is not { } article)
        {
            return;
        }
        if (source is not null)
        {
            try
            {
                await source.RecordFeedbackAsync(article.Id, helpful);
            }
            catch (Exception error) when (error is TimeoutException or IOException or InvalidOperationException)
            {
                StatusMessage = Localizer.Instance["help-not-available"];
                return;
            }
        }
        FeedbackSubmitted = true;
        FeedbackMessage = Localizer.Instance["help-feedback-thanks"];
    }

    private void ResetFeedback()
    {
        FeedbackSubmitted = false;
        FeedbackMessage = null;
    }
}
