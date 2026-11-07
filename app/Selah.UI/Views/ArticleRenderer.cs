using System.Text.Json;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.Documents;
using Avalonia.Layout;
using Avalonia.Media;

namespace Selah.UI.Views;

/// <summary>
/// Renders a help article's AST (built by <c>selah-kb</c>, KNOWLEDGE-BASE §6.4) with native
/// controls, never a web view, so text size, appearances and screen readers all work.
/// Bold is a UI label, <c>[[Key]]</c> a key cap, and <c>kb:</c> links open another article.
/// </summary>
public static class ArticleRenderer
{
    public static Control Render(string astJson, Action<string>? openArticle = null)
    {
        using var document = JsonDocument.Parse(astJson);
        var panel = new StackPanel();
        panel.Bind(StackPanel.SpacingProperty, panel.GetResourceObservable("Space.4"));
        foreach (var block in document.RootElement.GetProperty("blocks").EnumerateArray())
        {
            panel.Children.Add(RenderBlock(block, openArticle));
        }
        return panel;
    }

    private static string Text(JsonElement element, string name) =>
        element.TryGetProperty(name, out var value) ? value.GetString() ?? string.Empty : string.Empty;

    private static Control RenderBlock(JsonElement block, Action<string>? openArticle)
    {
        switch (Text(block, "type"))
        {
            case "heading":
                var level = block.GetProperty("level").GetInt32();
                var heading = Paragraph(block.GetProperty("inlines"), openArticle);
                heading.Classes.Add(level <= 1 ? "title-1" : level == 2 ? "title-3" : "label");
                AutomationPropertiesHeading(heading, level);
                return heading;
            case "paragraph":
                return Paragraph(block.GetProperty("inlines"), openArticle);
            case "list":
                return List(block, openArticle);
            case "callout":
                return Callout(block, openArticle);
            case "quote":
                return Blocks(block.GetProperty("blocks"), openArticle);
            case "table":
                return Table(block, openArticle);
            case "code":
                var code = new SelectableTextBlock { Text = Text(block, "text") };
                code.Bind(TextBlock.FontFamilyProperty, code.GetResourceObservable("Font.Mono"));
                return code;
            default:
                return new Border();
        }
    }

    private static void AutomationPropertiesHeading(Control control, int level) =>
        Avalonia.Automation.AutomationProperties.SetHeadingLevel(control, Math.Clamp(level, 1, 9));

    private static StackPanel Blocks(JsonElement blocks, Action<string>? openArticle)
    {
        var panel = new StackPanel();
        panel.Bind(StackPanel.SpacingProperty, panel.GetResourceObservable("Space.3"));
        foreach (var child in blocks.EnumerateArray())
        {
            panel.Children.Add(RenderBlock(child, openArticle));
        }
        return panel;
    }

    private static Control List(JsonElement block, Action<string>? openArticle)
    {
        var ordered = block.GetProperty("ordered").GetBoolean();
        var grid = new Grid { ColumnDefinitions = new ColumnDefinitions("Auto,*") };
        var row = 0;
        foreach (var item in block.GetProperty("items").EnumerateArray())
        {
            grid.RowDefinitions.Add(new RowDefinition(GridLength.Auto));
            var marker = new TextBlock
            {
                Text = ordered ? $"{row + 1}." : "•",
                FontWeight = ordered ? FontWeight.Bold : FontWeight.Normal,
                HorizontalAlignment = HorizontalAlignment.Right,
            };
            marker.Bind(Layoutable.MarginProperty, marker.GetResourceObservable("Space.Text.Small"));
            Grid.SetRow(marker, row);
            var content = Blocks(item, openArticle);
            Grid.SetRow(content, row);
            Grid.SetColumn(content, 1);
            content.Bind(Layoutable.MarginProperty, content.GetResourceObservable("Space.Item"));
            grid.Children.Add(marker);
            grid.Children.Add(content);
            row++;
        }
        return grid;
    }

    private static Control Callout(JsonElement block, Action<string>? openArticle)
    {
        var warning = Text(block, "kind") is "warning" or "caution" or "important";
        var border = new Border { Child = Blocks(block.GetProperty("blocks"), openArticle) };
        border.Classes.Add(warning ? "callout-warning" : "callout-info");
        border.Bind(Border.BackgroundProperty, border.GetResourceObservable(warning ? "Brush.WarningBg" : "Brush.InfoBg"));
        border.Bind(Border.BorderBrushProperty, border.GetResourceObservable(warning ? "Brush.Warning" : "Brush.Info"));
        border.Bind(Border.BorderThicknessProperty, border.GetResourceObservable("Line.AlertBar"));
        border.Bind(Decorator.PaddingProperty, border.GetResourceObservable("Space.Text.Large"));
        return border;
    }

    private static Control Table(JsonElement block, Action<string>? openArticle)
    {
        var header = block.GetProperty("header").EnumerateArray().ToList();
        var rows = block.GetProperty("rows").EnumerateArray().ToList();
        var grid = new Grid();
        for (var c = 0; c < Math.Max(header.Count, 1); c++)
        {
            grid.ColumnDefinitions.Add(new ColumnDefinition(GridLength.Star));
        }
        var all = new List<(List<JsonElement> Cells, bool Head)> { (header, true) };
        all.AddRange(rows.Select(r => (r.EnumerateArray().ToList(), false)));
        for (var r = 0; r < all.Count; r++)
        {
            grid.RowDefinitions.Add(new RowDefinition(GridLength.Auto));
            for (var c = 0; c < all[r].Cells.Count && c < grid.ColumnDefinitions.Count; c++)
            {
                var cell = Paragraph(all[r].Cells[c], openArticle);
                if (all[r].Head)
                {
                    cell.FontWeight = FontWeight.Bold;
                }
                cell.Bind(Layoutable.MarginProperty, cell.GetResourceObservable("Space.Cell"));
                Grid.SetRow(cell, r);
                Grid.SetColumn(cell, c);
                grid.Children.Add(cell);
            }
        }
        return grid;
    }

    private static TextBlock Paragraph(JsonElement inlines, Action<string>? openArticle)
    {
        var block = new TextBlock { TextWrapping = TextWrapping.Wrap };
        var collection = new InlineCollection();
        foreach (var inline in inlines.EnumerateArray())
        {
            collection.Add(RenderInline(inline, openArticle));
        }
        block.Inlines = collection;
        return block;
    }

    private static Inline RenderInline(JsonElement inline, Action<string>? openArticle)
    {
        switch (Text(inline, "type"))
        {
            case "label":
                return new Bold { Inlines = { new Run(Text(inline, "text")) } };
            case "key":
                var key = new Border
                {
                    Child = new TextBlock { Text = KeyName(Text(inline, "text")), FontWeight = FontWeight.SemiBold },
                };
                key.Bind(Decorator.PaddingProperty, key.GetResourceObservable("Space.Key"));
                key.Classes.Add("key");
                key.Bind(Border.BackgroundProperty, key.GetResourceObservable("Brush.Surface2"));
                key.Bind(Border.BorderBrushProperty, key.GetResourceObservable("Brush.Line2"));
                key.Bind(Border.BorderThicknessProperty, key.GetResourceObservable("Line.Hairline"));
                key.Bind(Border.CornerRadiusProperty, key.GetResourceObservable("Radius.Input"));
                return new InlineUIContainer(key) { BaselineAlignment = BaselineAlignment.TextBottom };
            case "emphasis":
                return new Italic { Inlines = { new Run(Text(inline, "text")) } };
            case "code":
                var run = new Run(Text(inline, "text"));
                run.Bind(TextElement.FontFamilyProperty, run.GetResourceObservable("Font.Mono"));
                return run;
            case "link":
                var label = string.Concat(inline.GetProperty("inlines").EnumerateArray().Select(i => Text(i, "text")));
                var kind = Text(inline, "kind");
                var target = Text(inline, "target");
                var button = new Button { Content = new TextBlock { Text = label, TextDecorations = TextDecorations.Underline }, Tag = $"{kind}:{target}" };
                button.Classes.Add("link");
                if (kind == "kb" && openArticle is not null)
                {
                    button.Click += (_, _) => openArticle(target);
                }
                return new InlineUIContainer(button) { BaselineAlignment = BaselineAlignment.TextBottom };
            case "image":
                return new Run(Text(inline, "alt"));
            case "break":
                return new LineBreak();
            default:
                return new Run(Text(inline, "text"));
        }
    }

    /// Ctrl is Cmd on macOS (KNOWLEDGE-BASE §6.3).
    private static string KeyName(string key) => OperatingSystem.IsMacOS() && key == "Ctrl" ? "Cmd" : key;
}
