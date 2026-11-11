using Avalonia;
using Avalonia.Controls;
using Avalonia.Headless.XUnit;
using Avalonia.Styling;
using Avalonia.Threading;
using Avalonia.VisualTree;
using Selah.Contracts.V1;
using Selah.UI.Services;
using Selah.UI.ViewModels;
using Selah.UI.Views;

namespace Selah.UI.Tests;

/// <summary>The stage layout editor (M4.6, prototype 06): the Stage tab in Producer mode.</summary>
public sealed class StageEditorTests : IDisposable
{
    private readonly Reset reset = new();
    private readonly string home = Directory.CreateTempSubdirectory("selah-stage-ui-").FullName;

    public void Dispose()
    {
        reset.Dispose();
        try
        {
            Directory.Delete(home, recursive: true);
        }
        catch (IOException)
        {
        }
    }

    public static TheoryData<int, string, string> ModesMatrix() => LibraryAndEditorTests.ModesMatrix();

    private (MainWindow Window, MainViewModel Model, LiveProducerTests.FakeLive Live) Open(int scale, string appearance, string language)
    {
        TextScale.Instance.Percent = scale;
        Localizer.Instance.SetLanguage(language);
        Application.Current!.RequestedThemeVariant = appearance == "booth" ? ThemeVariant.Dark : ThemeVariant.Light;
        var live = new LiveProducerTests.FakeLive();
        var model = new MainViewModel(null, null, null, null, live, Path.Combine(home, "producer-layout.json"));
        model.Apply(new ConnectionSnapshot(true, true, SafeMode: false, Live: false, 1));
        var window = new MainWindow { DataContext = model, Width = 1280, Height = 800 };
        window.Show();
        Dispatcher.UIThread.RunJobs();
        return (window, model, live);
    }

    private static T Named<T>(Visual root, string name) where T : Control =>
        root.GetVisualDescendants().OfType<T>().First(c => c.Name == name);

    private static async Task OpenStageTab(MainViewModel model, MainWindow window, LiveProducerTests.FakeLive live)
    {
        model.SelectModeCommand.Execute("Live");
        Dispatcher.UIThread.RunJobs();
        await model.LiveProducer.LoadServiceAsync(live.Service, triggerFirst: true);
        await model.LiveProducer.RefreshAsync();
        var pane = FindPane<StagePane>(model.LiveProducer.DockLayout);
        model.LiveProducer.Dock.SetActiveDockable(pane);
        Dispatcher.UIThread.RunJobs();
    }

    private static T FindPane<T>(Dock.Model.Core.IDockable root) where T : class
    {
        if (root is T found)
        {
            return found;
        }
        if (root is Dock.Model.Core.IDock { VisibleDockables: { } children })
        {
            foreach (var child in children)
            {
                if (child is T match)
                {
                    return match;
                }
                if (child is Dock.Model.Core.IDock)
                {
                    try
                    {
                        return FindPane<T>(child);
                    }
                    catch (InvalidOperationException)
                    {
                    }
                }
            }
        }
        throw new InvalidOperationException($"no {typeof(T).Name}");
    }

    [AvaloniaTheory]
    [MemberData(nameof(ModesMatrix))]
    public async Task The_stage_tab_saves_a_layout_and_puts_it_on_the_stage_screens(int scale, string appearance, string language)
    {
        var (window, model, live) = Open(scale, appearance, language);
        await OpenStageTab(model, window, live);
        var stage = model.LiveProducer.Stage;

        Assert.True(Named<StackPanel>(window, "StageEditor").IsEffectivelyVisible);
        Assert.True(Named<Border>(window, "StagePreview").IsEffectivelyVisible);
        // The standard arrangement previews prototype 06: header, words, next, timer, booth message.
        Assert.Equal(5, stage.Preview.Count);
        Assert.DoesNotContain(stage.Preview, b => b.Label == Localizer.Instance["stage-preview-notes"]);

        // A speaker's arrangement: notes on, chords and next off; the preview follows.
        stage.SelectedArrangement = stage.Arrangements.Single(a => a.Value == StageArrangement.Speaker);
        Dispatcher.UIThread.RunJobs();
        Assert.True(stage.ShowNotes);
        Assert.False(stage.ShowChords);
        Assert.False(stage.ShowNext);
        Assert.Contains(stage.Preview, b => b.Label == Localizer.Instance["stage-preview-notes"]);
        Assert.True(Named<CheckBox>(window, "StageShowNotes").IsChecked);
        stage.TextSize = 125;
        stage.Name = "Pastor Sam";

        await stage.SaveCommand.ExecuteAsync(null);
        Dispatcher.UIThread.RunJobs();
        var saved = Assert.Single(live.StageLayouts);
        Assert.Equal("Pastor Sam", saved.Name);
        Assert.True(saved.ShowSpeakerNotes);
        Assert.False(saved.ShowNextSlide);
        var words = saved.Elements.Single(e => e.Kind == StageElementKind.CurrentSlide);
        Assert.Equal(1.25f, words.FontScale, 3);
        Assert.InRange(words.Rect.Width, 0.5f, 0.6f);
        Assert.Equal(Localizer.Instance["stage-editor-saved"], stage.StatusText);
        Assert.False(stage.IsInUse);

        await stage.UseCommand.ExecuteAsync(null);
        Dispatcher.UIThread.RunJobs();
        Assert.Contains("stage-use", live.Calls);
        Assert.True(stage.IsInUse);
        Assert.Equal("Pastor Sam", stage.ActiveLayoutName);

        foreach (var name in new[] { "StageSaveButton", "StageUseButton" })
        {
            Assert.True(Named<Button>(window, name).IsEffectivelyVisible, name);
        }
        // The live controls stay put while the Stage tab is open (DESIGN-SYSTEM §10).
        Assert.True(Named<Button>(window, "LiveNextButton").IsEffectivelyVisible);
        Assert.True(Named<Button>(window, "LiveClearAllButton").IsEffectivelyVisible);
        Assert.DoesNotContain(LayoutTests.ClippedCriticalLabels(window), p => !p.EndsWith("is hidden", StringComparison.Ordinal));
        window.Close();
    }

    [AvaloniaFact]
    public async Task Changing_the_layout_in_use_sends_it_to_the_stage_again()
    {
        var (window, model, live) = Open(100, "light", "en");
        await OpenStageTab(model, window, live);
        var stage = model.LiveProducer.Stage;
        await stage.UseCommand.ExecuteAsync(null);
        Assert.True(stage.IsInUse);
        var uses = live.Calls.Count(c => c == "stage-use");

        stage.ShowClock = false;
        await stage.SaveCommand.ExecuteAsync(null);
        Assert.Equal(uses + 1, live.Calls.Count(c => c == "stage-use"));
        Assert.False(Assert.Single(live.StageLayouts).ShowClock);
        Assert.True(stage.IsInUse);
        window.Close();
    }
}
