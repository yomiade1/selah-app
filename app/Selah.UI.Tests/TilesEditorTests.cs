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

/// <summary>Set up tiles (M4.9): make looks, props, messages, timers, macros and clear groups.</summary>
public sealed class TilesEditorTests : IDisposable
{
    private readonly Reset reset = new();
    private readonly string home = Directory.CreateTempSubdirectory("selah-tiles-ui-").FullName;

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

    private async Task<(MainWindow Window, MainViewModel Model, LiveProducerTests.FakeLive Live)> OpenAsync(int scale, string appearance, string language)
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
        model.SelectModeCommand.Execute("Live");
        Dispatcher.UIThread.RunJobs();
        await model.LiveProducer.LoadServiceAsync(live.Service, triggerFirst: true);
        await model.LiveProducer.RefreshAsync();
        var pane = FindPane<TilesSetupPane>(model.LiveProducer.DockLayout);
        model.LiveProducer.Dock.SetActiveDockable(pane);
        Dispatcher.UIThread.RunJobs();
        return (window, model, live);
    }

    private static T Named<T>(Visual root, string name) where T : Control =>
        root.GetVisualDescendants().OfType<T>().First(c => c.Name == name);

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

    private static void Pick(TilesEditorViewModel editor, ShowControlKind kind)
    {
        editor.SelectedKind = editor.Kinds.Single(k => k.Kind == kind);
        Dispatcher.UIThread.RunJobs();
    }

    [AvaloniaTheory]
    [MemberData(nameof(ModesMatrix))]
    public async Task Set_up_tiles_makes_each_kind_and_each_becomes_a_tile(int scale, string appearance, string language)
    {
        var (window, model, live) = await OpenAsync(scale, appearance, language);
        var editor = model.LiveProducer.TilesSetup;
        Assert.True(Named<StackPanel>(window, "TilesEditor").IsEffectivelyVisible);

        // A look that hides the words on the main screen.
        Pick(editor, ShowControlKind.Look);
        Assert.True(Named<StackPanel>(window, "TilesLookFields").IsEffectivelyVisible);
        editor.Name = "Lobby";
        editor.Key = "2";
        var screen = Assert.Single(editor.ScreenRules);
        screen.Layers.Single(l => l.Kind == LayerKind.Slide).IsChecked = false;
        await editor.SaveCommand.ExecuteAsync(null);
        var look = live.Controls.Single(c => c.Kind == ShowControlKind.Look).Look!;
        Assert.Equal("⇧2", look.Hotkey);
        Assert.DoesNotContain(LayerKind.Slide, Assert.Single(look.Rules).EnabledLayers);
        Assert.Contains(LayerKind.Media, look.Rules[0].EnabledLayers);
        Assert.Equal(Localizer.Instance["tiles-saved"], editor.StatusText);

        // A prop and a message.
        Pick(editor, ShowControlKind.Prop);
        Assert.True(Named<StackPanel>(window, "TilesWordsFields").IsEffectivelyVisible);
        editor.Name = "Speaker name";
        editor.Words = "Pastor Ana Silva";
        await editor.SaveCommand.ExecuteAsync(null);
        Pick(editor, ShowControlKind.Message);
        Assert.True(Named<CheckBox>(window, "TilesStageOnly").IsEffectivelyVisible);
        editor.Name = "Parking";
        editor.Words = "A car's lights are on. Time left: {timer}";
        editor.HideAfterSeconds = 20;
        await editor.SaveCommand.ExecuteAsync(null);
        var message = live.Controls.Single(c => c.Kind == ShowControlKind.Message).Message!;
        Assert.Equal(20u, message.DurationSeconds);
        Assert.Contains("{timer}", message.TemplateText, StringComparison.Ordinal);

        // A 30-minute countdown that keeps counting past zero.
        Pick(editor, ShowControlKind.Timer);
        Assert.True(Named<StackPanel>(window, "TilesTimerFields").IsEffectivelyVisible);
        editor.Name = "Sermon timer";
        editor.Minutes = 30;
        editor.SelectedOverrun = editor.OverrunOptions.Single(o => o.Mode == TimerOverrunMode.CountUpNegative);
        await editor.SaveCommand.ExecuteAsync(null);
        var timer = live.Controls.Single(c => c.Kind == ShowControlKind.Timer).Timer!;
        Assert.Equal(30UL * 60 * 1000, timer.DurationMs);
        Assert.Equal(TimerOverrunMode.CountUpNegative, timer.OverrunMode);

        // A macro: change the look, then start the timer.
        Pick(editor, ShowControlKind.Macro);
        Assert.True(Named<StackPanel>(window, "TilesMacroFields").IsEffectivelyVisible);
        editor.Name = "Sermon";
        editor.Key = "s";
        editor.SelectedStepKind = editor.StepKinds.First(s => s.Kind == MacroActionKind.SetLook);
        editor.SelectedStepTarget = editor.StepTargets.Single(t => t.Name == "Lobby");
        editor.AddStepCommand.Execute(null);
        editor.SelectedStepKind = editor.StepKinds.First(s => s.Kind == MacroActionKind.StartTimer);
        editor.SelectedStepTarget = editor.StepTargets.Single(t => t.Name == "Sermon timer");
        editor.AddStepCommand.Execute(null);
        Dispatcher.UIThread.RunJobs();
        Assert.Equal(2, editor.Steps.Count);
        await editor.SaveCommand.ExecuteAsync(null);
        var macro = live.Controls.Single(c => c.Kind == ShowControlKind.Macro).Macro!;
        Assert.Equal("S", macro.Hotkey);
        Assert.Equal([MacroActionKind.SetLook, MacroActionKind.StartTimer], macro.Actions.Select(a => a.Kind));
        Assert.True(LibraryIds.Same(look.Id, macro.Actions[0].TargetId));

        // A clear group for the words and the prop.
        Pick(editor, ShowControlKind.ClearGroup);
        Assert.True(Named<StackPanel>(window, "TilesClearFields").IsEffectivelyVisible);
        editor.Name = "Words and props";
        await editor.SaveCommand.ExecuteAsync(null);
        var group = live.Controls.Single(c => c.Kind == ShowControlKind.ClearGroup).ClearGroup!;
        Assert.Contains(LayerKind.Slide, group.LayersToClear);
        Assert.Contains(LayerKind.Props, group.LayersToClear);
        Assert.DoesNotContain(LayerKind.Media, group.LayersToClear);

        // Every one became a tile.
        var tiles = model.LiveProducer.Tiles;
        foreach (var (kind, name) in new[] { (LiveTileKind.Look, "Lobby"), (LiveTileKind.Prop, "Speaker name"), (LiveTileKind.Message, "Parking"),
            (LiveTileKind.Macro, "Sermon"), (LiveTileKind.ClearGroup, "Words and props") })
        {
            Assert.Contains(tiles, t => t.Kind == kind && t.Value == name);
        }
        Assert.Contains(tiles, t => t.Kind == LiveTileKind.Timer && t.Caption == "Sermon timer");

        foreach (var name in new[] { "TilesSaveButton", "TilesDeleteButton", "TilesNewButton" })
        {
            Assert.True(Named<Button>(window, name).IsEffectivelyVisible, name);
        }
        // The live controls stay put while the tab is open (DESIGN-SYSTEM §10).
        Assert.True(Named<Button>(window, "LiveNextButton").IsEffectivelyVisible);
        Assert.True(Named<Button>(window, "LiveClearAllButton").IsEffectivelyVisible);
        Assert.DoesNotContain(LayoutTests.ClippedCriticalLabels(window), p => !p.EndsWith("is hidden", StringComparison.Ordinal));
        window.Close();
    }

    [AvaloniaFact]
    public async Task Nothing_is_saved_until_it_is_complete_and_each_problem_says_what_to_fix()
    {
        var (window, model, live) = await OpenAsync(100, "light", "en");
        var editor = model.LiveProducer.TilesSetup;
        var before = live.Calls.Count;

        await editor.SaveCommand.ExecuteAsync(null);
        Assert.Equal(Localizer.Instance["tiles-error-name"], editor.StatusText);
        Assert.True(editor.StatusIsError);

        Pick(editor, ShowControlKind.Macro);
        editor.Name = "Empty";
        await editor.SaveCommand.ExecuteAsync(null);
        Assert.Equal(Localizer.Instance["tiles-error-steps"], editor.StatusText);

        editor.Key = "!";
        await editor.SaveCommand.ExecuteAsync(null);
        Assert.Equal(Localizer.Instance["tiles-error-key"], editor.StatusText);

        Pick(editor, ShowControlKind.Timer);
        editor.Name = "Start";
        editor.SelectedTimerKind = editor.TimerKinds.Single(k => k.Kind == TimerKind.CountToTime);
        editor.TimeOfDay = "25:00";
        await editor.SaveCommand.ExecuteAsync(null);
        Assert.Equal(Localizer.Instance["tiles-error-time"], editor.StatusText);
        editor.TimeOfDay = "9:45";
        await editor.SaveCommand.ExecuteAsync(null);
        Assert.False(editor.StatusIsError);
        Assert.Equal((uint)((9 * 60) + 45) * 60, live.Controls.Single().Timer!.TargetTimeOfDaySeconds);

        Pick(editor, ShowControlKind.Prop);
        editor.Name = "No words";
        await editor.SaveCommand.ExecuteAsync(null);
        Assert.Equal(Localizer.Instance["tiles-error-words"], editor.StatusText);

        // Only the one complete timer reached core.
        Assert.Equal(1, live.Calls.Skip(before).Count(c => c.StartsWith("save:", StringComparison.Ordinal)));
        window.Close();
    }

    [AvaloniaFact]
    public async Task A_saved_one_can_be_changed_or_deleted_and_its_tile_follows()
    {
        var (window, model, live) = await OpenAsync(100, "light", "en");
        var editor = model.LiveProducer.TilesSetup;
        Pick(editor, ShowControlKind.Prop);
        editor.Name = "Welcome";
        editor.Words = "Welcome to church";
        await editor.SaveCommand.ExecuteAsync(null);
        var id = live.Controls.Single().Id;

        // Change it: the same prop is saved again, not a second one.
        editor.NewCommand.Execute(null);
        editor.SelectedSaved = editor.Saved.Single();
        Assert.Equal("Welcome to church", editor.Words);
        editor.Words = "Welcome home";
        await editor.SaveCommand.ExecuteAsync(null);
        var prop = Assert.Single(live.Controls).Prop!;
        Assert.True(LibraryIds.Same(id, prop.Id));
        Assert.Equal("Welcome home", prop.Label);

        // Its tile shows the prop, then hides it.
        var tile = model.LiveProducer.Tiles.Single(t => t.Kind == LiveTileKind.Prop);
        await model.LiveProducer.ActivateTileCommand.ExecuteAsync(tile);
        Assert.Contains("prop", live.Calls);
        Assert.True(tile.IsOn);
        await model.LiveProducer.ActivateTileCommand.ExecuteAsync(tile);
        Assert.Contains("hide-prop", live.Calls);
        Assert.False(tile.IsOn);

        editor.SelectedSaved = editor.Saved.Single();
        await editor.DeleteCommand.ExecuteAsync(null);
        Assert.Empty(live.Controls);
        Assert.DoesNotContain(model.LiveProducer.Tiles, t => t.Kind == LiveTileKind.Prop);
        Assert.Equal(Localizer.Instance["tiles-deleted"], editor.StatusText);
        Assert.True(editor.HasNoSaved);
        window.Close();
    }

    [AvaloniaFact]
    public async Task A_clear_group_tile_and_a_message_tile_reach_core()
    {
        var (window, model, live) = await OpenAsync(100, "light", "en");
        var editor = model.LiveProducer.TilesSetup;
        Pick(editor, ShowControlKind.ClearGroup);
        editor.Name = "Just the words";
        foreach (var layer in editor.ClearLayers)
        {
            layer.IsChecked = layer.Kind == LayerKind.Slide;
        }
        editor.HideLogo = false;
        await editor.SaveCommand.ExecuteAsync(null);
        Pick(editor, ShowControlKind.Message);
        editor.Name = "Nursery";
        editor.Words = "Parent of child 12, please come to the nursery";
        await editor.SaveCommand.ExecuteAsync(null);

        await model.LiveProducer.ActivateTileCommand.ExecuteAsync(model.LiveProducer.Tiles.Single(t => t.Kind == LiveTileKind.ClearGroup));
        Assert.Contains("clear-group", live.Calls);
        var message = model.LiveProducer.Tiles.Single(t => t.Kind == LiveTileKind.Message);
        await model.LiveProducer.ActivateTileCommand.ExecuteAsync(message);
        Assert.Contains("message", live.Calls);
        Assert.True(message.IsOn);
        window.Close();
    }
}
