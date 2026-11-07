using Avalonia.Controls;
using Avalonia.Controls.Documents;
using Avalonia.Headless;
using Avalonia.Headless.XUnit;
using Avalonia.Threading;
using Avalonia.VisualTree;
using Selah.Contracts.V1;
using Selah.UI.Services;
using Selah.UI.ViewModels;
using Selah.UI.Views;

namespace Selah.UI.Tests;

/// <summary>M1-A10: an article renders in the UI with native controls (KNOWLEDGE-BASE Â§6.4).</summary>
public sealed class HelpTests : IDisposable
{
    private readonly Reset reset = new();

    public void Dispose() => reset.Dispose();

    private static readonly string Ast = File.ReadAllText(Repo.PathOf("tests", "fixtures", "kb", "show-a-song.ast.json"));

    private sealed class FakeHelp : IHelpSource
    {
        public List<string> Opened { get; } = [];

        public Task<IReadOnlyList<Article>> SearchHelpAsync(string query, IEnumerable<string> anchors) =>
            Task.FromResult<IReadOnlyList<Article>>(query.Contains("song", StringComparison.Ordinal)
                ? [new Article { Id = "live.show-a-song", Title = "Show a song on the screens", Summary = "Pick the song.", AstJson = Ast }]
                : []);

        public Task<Article?> GetArticleAsync(string id)
        {
            Opened.Add(id);
            return Task.FromResult<Article?>(new Article { Id = id, Title = "Show a song on the screens", AstJson = Ast });
        }
    }

    private static IEnumerable<string> Runs(Control root) =>
        root.GetVisualDescendants().OfType<TextBlock>().SelectMany(t => t.Inlines is { } inlines ? Flatten(inlines) : [t.Text ?? string.Empty]);

    private static IEnumerable<string> Flatten(InlineCollection inlines)
    {
        foreach (var inline in inlines)
        {
            switch (inline)
            {
                case Run run:
                    yield return run.Text ?? string.Empty;
                    break;
                case Span span:
                    foreach (var text in Flatten(span.Inlines))
                    {
                        yield return text;
                    }
                    break;
            }
        }
    }

    [AvaloniaTheory]
    [InlineData(100, "light")]
    [InlineData(200, "booth")]
    public void An_article_renders_with_steps_labels_keys_and_a_tip(int scale, string appearance)
    {
        TextScale.Instance.Percent = scale;
        Avalonia.Application.Current!.RequestedThemeVariant = appearance == "booth" ? Avalonia.Styling.ThemeVariant.Dark : Avalonia.Styling.ThemeVariant.Light;
        var help = new FakeHelp();
        var model = new HelpViewModel(help);
        var window = new HelpWindow { DataContext = model, Width = 980, Height = 720 };
        window.Show();
        model.OpenAsync("live.show-a-song").GetAwaiter().GetResult();
        Dispatcher.UIThread.RunJobs();

        var host = window.GetVisualDescendants().OfType<ContentControl>().Single(c => c.Name == "ArticleHost");
        var article = Assert.IsAssignableFrom<Control>(host.Content);
        var texts = Runs(article).ToList();
        Assert.Contains("Show a song on the screens", texts);
        Assert.Contains("Next slide", texts);
        var bold = article.GetVisualDescendants().OfType<TextBlock>()
            .SelectMany(t => t.Inlines ?? []).OfType<Bold>().SelectMany(b => b.Inlines).OfType<Run>().Select(r => r.Text);
        Assert.Contains("Next slide", bold);
        var keys = article.GetVisualDescendants().OfType<Border>().Where(b => b.Classes.Contains("key")).ToList();
        Assert.Contains(keys, k => (k.Child as TextBlock)?.Text == "Space");
        Assert.Single(article.GetVisualDescendants().OfType<Border>(), b => b.Classes.Contains("callout-info"));
        var markers = article.GetVisualDescendants().OfType<TextBlock>().Select(t => t.Text).Where(t => t is "1." or "5.").ToList();
        Assert.Equal(["1.", "5."], markers);

        // A kb: link opens the other article.
        var link = article.GetVisualDescendants().OfType<Button>().First(b => (b.Tag as string) == "kb:live.clear-the-screens");
        link.Command?.Execute(null);
        link.RaiseEvent(new Avalonia.Interactivity.RoutedEventArgs(Button.ClickEvent));
        Dispatcher.UIThread.RunJobs();
        Assert.Contains("live.clear-the-screens", help.Opened);

        using (var frame = window.CaptureRenderedFrame())
        {
            Directory.CreateDirectory(Repo.PathOf("target", "ui-snapshots"));
            using var file = File.Create(Repo.PathOf("target", "ui-snapshots", $"help-{scale}-{appearance}.png"));
            frame?.Save(file, Avalonia.Media.Imaging.PngBitmapEncoderOptions.Default);
        }
        window.Close();
    }

    [AvaloniaFact]
    public void Search_lists_results_and_says_when_nothing_matches()
    {
        var model = new HelpViewModel(new FakeHelp()) { Query = "song" };
        model.SearchCommand.ExecuteAsync(null).GetAwaiter().GetResult();
        Assert.Single(model.Results);
        Assert.Null(model.Message);
        model.Query = "nothing here";
        model.SearchCommand.ExecuteAsync(null).GetAwaiter().GetResult();
        Assert.Empty(model.Results);
        Assert.Equal(Localizer.Instance["help-no-results"], model.Message);
    }
}
