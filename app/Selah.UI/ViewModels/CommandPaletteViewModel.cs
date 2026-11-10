using System.Collections.ObjectModel;
using Avalonia.Threading;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Selah.Contracts.V1;
using Selah.UI.Services;

namespace Selah.UI.ViewModels;

public enum PaletteItemKind
{
    Command,
    Song,
    Scripture,
    Media,
    Help,
}

/// <summary>One selectable entry in the Ctrl/Cmd+K command palette.</summary>
public sealed class PaletteItem
{
    public required PaletteItemKind Kind { get; init; }

    public required string Title { get; init; }

    public string Subtitle { get; init; } = string.Empty;

    public Action? ExecuteCommand { get; init; }

    public Song? Song { get; init; }

    public ScriptureVerseHit? Verse { get; init; }

    public MediaItem? Media { get; init; }

    public Article? Article { get; init; }

    public string CategoryLabel => Kind switch
    {
        PaletteItemKind.Command => Localizer.Instance["palette-category-command"],
        PaletteItemKind.Song => Localizer.Instance["palette-category-song"],
        PaletteItemKind.Scripture => Localizer.Instance["palette-category-scripture"],
        PaletteItemKind.Media => Localizer.Instance["palette-category-media"],
        PaletteItemKind.Help => Localizer.Instance["palette-category-help"],
        _ => string.Empty,
    };
}

/// <summary>
/// Unified Ctrl/Cmd+K command palette across app commands, library content (songs, scripture, media),
/// and KB help articles (M2.7, M2-A7).
/// </summary>
public sealed partial class CommandPaletteViewModel : ObservableObject
{
    private sealed record RegisteredCommand(string TitleKey, string SubtitleKey, Action Execute);

    private readonly ILibraryService? library;
    private readonly IHelpSource? help;
    private readonly List<RegisteredCommand> commands = [];
    private int searchGeneration;

    public CommandPaletteViewModel(ILibraryService? library, IHelpSource? help)
    {
        this.library = library;
        this.help = help;
        Localizer.Instance.PropertyChanged += (_, _) => OnPropertyChanged(nameof(EmptyPrompt));
    }

    public ObservableCollection<PaletteItem> Results { get; } = [];

    [ObservableProperty]
    private bool isOpen;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasQuery))]
    [NotifyPropertyChangedFor(nameof(HasNoResults))]
    private string query = string.Empty;

    [ObservableProperty]
    private PaletteItem? selectedResult;

    [ObservableProperty]
    private bool isSearching;

    public bool HasQuery => !string.IsNullOrWhiteSpace(Query);

    public bool HasNoResults => Results.Count == 0 && HasQuery && !IsSearching;

    public string EmptyPrompt => Localizer.Instance[HasQuery ? "palette-no-results" : "palette-empty"];

    public Func<IEnumerable<string>>? VisibleAnchorsProvider { get; set; }

    public event EventHandler<Song>? SongChosen;

    public event EventHandler<ScriptureVerseHit>? ScriptureChosen;

    public event EventHandler<MediaItem>? MediaChosen;

    public event EventHandler<Article>? HelpChosen;

    public void RegisterCommand(string titleKey, string subtitleKey, Action execute) =>
        commands.Add(new RegisteredCommand(titleKey, subtitleKey, execute));

    partial void OnQueryChanged(string value)
    {
        if (IsOpen)
        {
            _ = SearchAsync(value);
        }
    }

    [RelayCommand]
    public void Open()
    {
        IsOpen = true;
        Query = string.Empty;
        _ = SearchAsync(string.Empty);
    }

    [RelayCommand]
    public void Close()
    {
        IsOpen = false;
        searchGeneration++;
    }

    [RelayCommand]
    public void ActivateSelected() => Activate(SelectedResult ?? Results.FirstOrDefault());

    [RelayCommand]
    public void Activate(PaletteItem? item)
    {
        if (item is null)
        {
            return;
        }
        Close();
        switch (item.Kind)
        {
            case PaletteItemKind.Command:
                item.ExecuteCommand?.Invoke();
                break;
            case PaletteItemKind.Song when item.Song is { } song:
                SongChosen?.Invoke(this, song);
                break;
            case PaletteItemKind.Scripture when item.Verse is { } verse:
                ScriptureChosen?.Invoke(this, verse);
                break;
            case PaletteItemKind.Media when item.Media is { } media:
                MediaChosen?.Invoke(this, media);
                break;
            case PaletteItemKind.Help when item.Article is { } article:
                HelpChosen?.Invoke(this, article);
                break;
        }
    }

    public void MoveSelection(int delta)
    {
        if (Results.Count == 0)
        {
            return;
        }
        var index = SelectedResult is null ? -1 : Results.IndexOf(SelectedResult);
        var next = Math.Clamp(index + delta, 0, Results.Count - 1);
        SelectedResult = Results[next];
    }

    public async Task SearchAsync(string? queryOverride = null)
    {
        var text = (queryOverride ?? Query).Trim();
        var generation = ++searchGeneration;
        IsSearching = true;
        try
        {
            var items = new List<PaletteItem>();
            foreach (var command in commands)
            {
                var title = Localizer.Instance[command.TitleKey];
                var subtitle = Localizer.Instance[command.SubtitleKey];
                if (string.IsNullOrEmpty(text)
                    || title.Contains(text, StringComparison.OrdinalIgnoreCase)
                    || subtitle.Contains(text, StringComparison.OrdinalIgnoreCase))
                {
                    items.Add(new PaletteItem
                    {
                        Kind = PaletteItemKind.Command,
                        Title = title,
                        Subtitle = subtitle,
                        ExecuteCommand = command.Execute,
                    });
                }
            }

            if (!string.IsNullOrEmpty(text))
            {
                if (library is not null)
                {
                    try
                    {
                        var found = await library.SearchLibraryAsync(text, SearchScope.All, string.Empty, 12);
                        if (generation != searchGeneration)
                        {
                            return;
                        }
                        foreach (var hit in found.Songs)
                        {
                            items.Add(new PaletteItem
                            {
                                Kind = PaletteItemKind.Song,
                                Title = hit.Song.Title,
                                Subtitle = string.IsNullOrWhiteSpace(hit.Snippet)
                                    ? string.Join(", ", hit.Song.Authors)
                                    : hit.Snippet,
                                Song = hit.Song,
                            });
                        }
                        foreach (var hit in found.Scripture)
                        {
                            items.Add(new PaletteItem
                            {
                                Kind = PaletteItemKind.Scripture,
                                Title = $"{hit.BookName} {hit.Chapter}:{hit.Verse} ({hit.Translation})",
                                Subtitle = hit.Text,
                                Verse = hit,
                            });
                        }
                        foreach (var hit in found.Media)
                        {
                            items.Add(new PaletteItem
                            {
                                Kind = PaletteItemKind.Media,
                                Title = hit.MediaItem.Name,
                                Subtitle = hit.MediaItem.Extension.TrimStart('.').ToUpperInvariant(),
                                Media = hit.MediaItem,
                            });
                        }
                    }
                    catch (Exception error) when (error is TimeoutException or IOException or InvalidOperationException)
                    {
                    }
                }

                if (help is not null)
                {
                    try
                    {
                        var anchors = VisibleAnchorsProvider?.Invoke() ?? [];
                        var articles = await help.SearchHelpAsync(text, anchors);
                        if (generation != searchGeneration)
                        {
                            return;
                        }
                        foreach (var article in articles)
                        {
                            items.Add(new PaletteItem
                            {
                                Kind = PaletteItemKind.Help,
                                Title = article.Title,
                                Subtitle = article.Summary,
                                Article = article,
                            });
                        }
                    }
                    catch (Exception error) when (error is TimeoutException or IOException or InvalidOperationException)
                    {
                    }
                }
            }

            if (generation != searchGeneration)
            {
                return;
            }

            void ApplyResults()
            {
                Results.Clear();
                foreach (var item in items)
                {
                    Results.Add(item);
                }
                SelectedResult = Results.FirstOrDefault();
                OnPropertyChanged(nameof(HasNoResults));
                OnPropertyChanged(nameof(EmptyPrompt));
            }

            if (Dispatcher.UIThread.CheckAccess())
            {
                ApplyResults();
            }
            else
            {
                Dispatcher.UIThread.Post(ApplyResults);
            }
        }
        finally
        {
            if (generation == searchGeneration)
            {
                IsSearching = false;
                OnPropertyChanged(nameof(HasNoResults));
            }
        }
    }
}
