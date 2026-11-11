using Avalonia;
using Avalonia.Controls;
using Avalonia.Headless;
using Avalonia.Headless.XUnit;
using Avalonia.Input;
using Avalonia.Styling;
using Avalonia.Threading;
using Avalonia.VisualTree;
using Selah.UI.Services;
using Selah.UI.ViewModels;
using Selah.UI.Views;

namespace Selah.UI.Tests;

/// <summary>Simple mode (M4.5, prototype 02), M4-A6 and the role lock in the UI (SEC-14).</summary>
public sealed class SimpleModeTests : IDisposable
{
    private readonly Reset reset = new();
    private readonly string home = Directory.CreateTempSubdirectory("selah-simple-ui-").FullName;

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

    private (MainWindow Window, MainViewModel Model, LiveProducerTests.FakeLive Live) Open(int scale = 100, string appearance = "light", string language = "en")
    {
        TextScale.Instance.Percent = scale;
        Localizer.Instance.SetLanguage(language);
        Application.Current!.RequestedThemeVariant = appearance == "booth" ? ThemeVariant.Dark : ThemeVariant.Light;
        var live = new LiveProducerTests.FakeLive();
        var model = new MainViewModel(null, null, null, null, live, Path.Combine(home, "layout.json"));
        model.Apply(new ConnectionSnapshot(true, true, SafeMode: false, Live: false, 1));
        var window = new MainWindow { DataContext = model, Width = 1280, Height = 800 };
        window.Show();
        Dispatcher.UIThread.RunJobs();
        return (window, model, live);
    }

    private static T Named<T>(Visual root, string name) where T : Control =>
        root.GetVisualDescendants().OfType<T>().First(c => c.Name == name);

    private static void Press(Window window, Key key, RawInputModifiers modifiers = RawInputModifiers.None)
    {
        window.KeyPress(key, modifiers, PhysicalKey.None, string.Empty);
        window.KeyRelease(key, modifiers, PhysicalKey.None, string.Empty);
        Dispatcher.UIThread.RunJobs();
    }

    private static async Task OpenSimple(MainViewModel model, MainWindow window, LiveProducerTests.FakeLive live)
    {
        model.SelectModeCommand.Execute("Live");
        model.SelectLiveViewCommand.Execute("Simple");
        await model.LiveProducer.LoadServiceAsync(live.Service, triggerFirst: true);
        Dispatcher.UIThread.RunJobs();
        Named<SimpleModeView>(window, "SimpleModeViewControl").Focus();
        Dispatcher.UIThread.RunJobs();
    }

    [AvaloniaTheory]
    [MemberData(nameof(ModesMatrix))]
    public async Task A_keyboard_only_simple_mode_flow_runs_a_song(int scale, string appearance, string language)
    {
        // M4-A6
        var (window, model, live) = Open(scale, appearance, language);
        await OpenSimple(model, window, live);
        Assert.True(Named<SimpleModeView>(window, "SimpleModeViewControl").IsEffectivelyVisible);
        Assert.False(Named<LiveProducerView>(window, "LiveProducerViewControl").IsEffectivelyVisible);

        // The step indicator lists the service; step 1 is current.
        Assert.Equal(4, model.Simple.Steps.Count);
        Assert.True(model.Simple.Steps[0].IsCurrent);
        Assert.Equal("Be Thou my Vision", model.Simple.NowText);
        Assert.Equal("Thou my best Thought", model.Simple.NextText);

        // Space through the whole song, then into the next song: keyboard only.
        Press(window, Key.Space);
        Assert.Equal("Thou my best Thought", model.Simple.NowText);
        Press(window, Key.Space);
        Assert.Equal("Be Thou my Wisdom", model.Simple.NowText);
        Press(window, Key.Left);
        Assert.Equal("Thou my best Thought", model.Simple.NowText);
        Press(window, Key.Space);
        Press(window, Key.Space);
        Assert.True(model.Simple.Steps[0].IsDone);
        Assert.True(model.Simple.Steps[1].IsCurrent);
        Assert.Equal("Romans 8:28", model.Simple.NowText);

        // A tap on Esc hides the words (holding it clears everything; see LiveProducerTests).
        window.KeyPress(Key.Escape, RawInputModifiers.None, PhysicalKey.Escape, string.Empty);
        window.KeyRelease(Key.Escape, RawInputModifiers.None, PhysicalKey.Escape, string.Empty);
        Dispatcher.UIThread.RunJobs();
        Assert.Contains("clear:Slide", live.Calls);
        Assert.Equal(0, model.ClearEverythingRequests);

        // Big buttons are at least 64 px high, and the critical ones are never clipped.
        foreach (var name in new[] { "SimpleBackButton", "SimpleNextSlideButton", "SimpleClearEverythingButton" })
        {
            var button = Named<Button>(window, name);
            Assert.True(button.IsEffectivelyVisible, name);
            Assert.True(button.Bounds.Height >= 64, $"{name} is {button.Bounds.Height} px high");
        }
        Assert.DoesNotContain(LayoutTests.ClippedCriticalLabels(window), p => !p.EndsWith("is hidden", StringComparison.Ordinal));
        Assert.True(Named<Button>(window, "ClearEverythingButton").IsEffectivelyVisible);
        window.Close();
    }

    [AvaloniaFact]
    public async Task A_role_locked_volunteer_cannot_open_producer_mode_or_the_editors()
    {
        // sec: SEC-14
        var (window, model, live) = Open();
        model.SelectModeCommand.Execute("Live");
        await model.LiveProducer.LoadServiceAsync(live.Service);
        model.LiveProducer.StartLockCommand.Execute(null);
        model.LiveProducer.LockPin = "2468";
        await model.LiveProducer.LockCommand.ExecuteAsync(null);
        Dispatcher.UIThread.RunJobs();

        Assert.True(model.IsRoleLocked);
        Assert.True(model.IsSimpleView);
        Assert.True(Named<TextBlock>(window, "SimpleRoleLockedText").IsEffectivelyVisible);
        Assert.False(Named<Button>(window, "LiveProducerViewButton").IsEnabled);
        Assert.False(Named<Button>(window, "ModeLibraryButton").IsEnabled);

        model.SelectLiveViewCommand.Execute("Producer");
        Assert.True(model.IsSimpleView);
        model.SelectModeCommand.Execute("Library");
        Assert.True(model.IsLiveMode);
        Assert.True(model.IsSimpleView);

        // Core refuses Producer commands anyway (the fake does as core does).
        await model.LiveProducer.HotkeyAsync("1", shift: true);
        Assert.Equal(Localizer.Instance["live-error-not-permitted"], model.LiveProducer.ErrorText);

        // A wrong PIN keeps it locked; the right one unlocks.
        model.Simple.StartUnlockCommand.Execute(null);
        model.Simple.UnlockPin = "0000";
        await model.Simple.UnlockCommand.ExecuteAsync(null);
        Assert.True(model.IsRoleLocked);
        Assert.Equal(Localizer.Instance["role-lock-wrong-pin"], model.Simple.UnlockError);
        model.Simple.UnlockPin = "2468";
        await model.Simple.UnlockCommand.ExecuteAsync(null);
        Dispatcher.UIThread.RunJobs();
        Assert.False(model.IsRoleLocked);
        model.SelectLiveViewCommand.Execute("Producer");
        Assert.True(model.IsProducerView);
        window.Close();
    }

    [AvaloniaFact]
    public async Task Message_the_booth_sends_a_short_note_to_the_producer_only()
    {
        // safe: SAFE-01
        var (window, model, live) = Open();
        await OpenSimple(model, window, live);
        model.Simple.OpenBoothMessageCommand.Execute(null);
        Dispatcher.UIThread.RunJobs();
        Assert.True(Named<TextBox>(window, "SimpleBoothMessageBox").IsEffectivelyVisible);
        Assert.False(model.Simple.SendBoothMessageCommand.CanExecute(null));
        model.Simple.BoothMessageDraft = "Mic 2 is out " + new string('!', 400);
        await model.Simple.SendBoothMessageCommand.ExecuteAsync(null);
        Assert.Contains("booth-message", live.Calls);
        Assert.Equal(200, live.LastBoothMessage.Length);
        Assert.Equal(Localizer.Instance["simple-booth-message-sent"], model.Simple.BoothMessageStatus);
        Assert.False(model.Simple.IsComposingBoothMessage);

        // Typing in the message box never moves the slides.
        model.Simple.OpenBoothMessageCommand.Execute(null);
        Dispatcher.UIThread.RunJobs();
        var box = Named<TextBox>(window, "SimpleBoothMessageBox");
        Assert.False(LiveKeys.Handle(model.LiveProducer, Key.Space, KeyModifiers.None, box));
        window.Close();
    }
}
