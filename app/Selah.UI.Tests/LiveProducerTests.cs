using Avalonia;
using Avalonia.Controls;
using Avalonia.Headless;
using Avalonia.Headless.XUnit;
using Avalonia.Input;
using Avalonia.Styling;
using Avalonia.Threading;
using Avalonia.VisualTree;
using Dock.Avalonia.Controls;
using Selah.Contracts.V1;
using Selah.UI.Services;
using Selah.UI.ViewModels;
using Selah.UI.Views;

namespace Selah.UI.Tests;

/// <summary>Producer mode (M4.4, prototype 01) and the hold-Esc clear (SAFE-03, M4-A12).</summary>
public sealed class LiveProducerTests : IDisposable
{
    private readonly Reset reset = new();
    private readonly string home = Directory.CreateTempSubdirectory("selah-live-ui-").FullName;

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

    private string LayoutPath => Path.Combine(home, "ui", "producer-layout.json");

    private (MainWindow Window, MainViewModel Model, FakeLive Live) Open(int scale = 100, string appearance = "light", string language = "en")
    {
        TextScale.Instance.Percent = scale;
        Localizer.Instance.SetLanguage(language);
        Application.Current!.RequestedThemeVariant = appearance == "booth" ? ThemeVariant.Dark : ThemeVariant.Light;
        var live = new FakeLive();
        var model = new MainViewModel(null, null, null, null, live, LayoutPath);
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

    private static async Task GoLive(MainViewModel model, MainWindow window, FakeLive live)
    {
        model.SelectModeCommand.Execute("Live");
        Dispatcher.UIThread.RunJobs();
        await model.LiveProducer.LoadServiceAsync(live.Service, triggerFirst: true);
        Dispatcher.UIThread.RunJobs();
        Named<LiveProducerView>(window, "LiveProducerViewControl").Focus();
        Dispatcher.UIThread.RunJobs();
    }

    [AvaloniaTheory]
    [MemberData(nameof(ModesMatrix))]
    public async Task Producer_mode_runs_a_service_from_the_keyboard_with_every_live_control_visible(int scale, string appearance, string language)
    {
        var (window, model, live) = Open(scale, appearance, language);
        await GoLive(model, window, live);

        // The docking workspace has the five panes of prototype 01.
        var dock = Named<DockControl>(window, "LiveDock");
        Assert.True(dock.IsEffectivelyVisible);
        Assert.Equal(["service-order", "slides", "program", "layers", "tiles"], model.LiveProducer.Dock.PaneIds(model.LiveProducer.DockLayout));
        Assert.Equal(4, model.LiveProducer.Items.Count);
        Assert.True(model.LiveProducer.Items[0].IsLive);
        Assert.True(model.LiveProducer.Items[1].IsNext);
        Assert.Equal(3, model.LiveProducer.Slides.Count);
        Assert.True(model.LiveProducer.Slides[0].IsLive);

        // Keyboard-first: Space, arrows, Shift+arrow, F9–F11, Ctrl+Z, number keys, Shift+number.
        Press(window, Key.Space);
        Assert.True(model.LiveProducer.Slides[1].IsLive);
        Press(window, Key.Left);
        Assert.True(model.LiveProducer.Slides[0].IsLive);
        Press(window, Key.D3);
        Assert.Contains("hotkey:3", live.Calls);
        Assert.True(model.LiveProducer.Slides[2].IsLive);
        Press(window, Key.F10);
        Press(window, Key.F11);
        Press(window, Key.F9);
        Assert.Contains("clear:Slide", live.Calls);
        Assert.Contains("clear:Media", live.Calls);
        Assert.Contains("logo:True", live.Calls);
        Press(window, Key.Z, RawInputModifiers.Control);
        Assert.Contains("undo", live.Calls);
        Press(window, Key.D1, RawInputModifiers.Shift);
        Assert.Contains("look:Worship", live.Calls);
        Assert.True(model.LiveProducer.Tiles.Single(t => t.Value == "Worship").IsOn);
        Press(window, Key.Right, RawInputModifiers.Shift);
        Assert.Contains("next-item", live.Calls);
        Assert.True(model.LiveProducer.Items[1].IsLive);
        Assert.True(model.LiveProducer.Items[0].IsDone);

        // F12 clears from Live through the priority lane (SAFE-03).
        Press(window, Key.F12);
        Assert.Equal(1, model.ClearEverythingRequests);
        Assert.Equal(1, live.ClearEverythingCount);

        // Nothing covers the program preview or the Next and Clear controls (DESIGN-SYSTEM §10).
        foreach (var name in new[] { "LiveNextButton", "LivePreviousButton", "LiveClearAllButton", "LiveClearTextButton" })
        {
            Assert.True(Named<Button>(window, name).IsEffectivelyVisible, name);
        }
        Assert.True(Named<Button>(window, "ClearEverythingButton").IsEffectivelyVisible);
        Assert.True(Named<Image>(window, "LiveProgramImage").IsEffectivelyVisible);
        // Shell labels that only show in safe mode or while the live guard is open are hidden here by design.
        Assert.DoesNotContain(LayoutTests.ClippedCriticalLabels(window), p => !p.EndsWith("is hidden", StringComparison.Ordinal));
        Assert.Equal(4, window.GetVisualDescendants().OfType<TextBlock>().Count(t => Critical.GetIsCritical(t) && t.FindAncestorOfType<LiveProducerView>() is not null && t.IsEffectivelyVisible));
        Assert.False(Named<Border>(window, "Sidebar").IsVisible);
        Assert.False(string.IsNullOrWhiteSpace(model.LiveProducer.Announcement));
        window.Close();
    }

    [AvaloniaFact]
    public async Task Typing_in_a_text_box_never_triggers_live_keys()
    {
        var (window, model, live) = Open();
        await GoLive(model, window, live);
        var box = new TextBox();
        Assert.False(LiveKeys.Handle(model.LiveProducer, Key.Space, KeyModifiers.None, box));
        Assert.False(LiveKeys.Handle(model.LiveProducer, Key.D2, KeyModifiers.None, box));
        Assert.DoesNotContain("next", live.Calls);
        window.Close();
    }

    [AvaloniaFact]
    public async Task Booth_lock_shows_the_banner_and_a_refused_command_says_why()
    {
        // safe: SAFE-06
        var (window, model, live) = Open();
        await GoLive(model, window, live);
        Press(window, Key.L, RawInputModifiers.Control);
        Assert.True(model.LiveProducer.IsBoothLocked);
        Assert.True(Named<Border>(window, "LiveBoothLockedBanner").IsEffectivelyVisible);
        Press(window, Key.D1, RawInputModifiers.Shift);
        Assert.Equal(Localizer.Instance["live-error-booth-locked"], model.LiveProducer.ErrorText);
        Press(window, Key.Space);
        Assert.Null(model.LiveProducer.ErrorText);
        window.Close();
    }

    [AvaloniaFact]
    public async Task A_message_from_the_booth_reaches_the_producer_only()
    {
        // safe: SAFE-01
        var (window, model, live) = Open();
        await GoLive(model, window, live);
        live.Raise(s => s.Automation[0].BoothMessage = "Mic 2 is out");
        Dispatcher.UIThread.RunJobs();
        Assert.True(Named<Border>(window, "LiveBoothMessageAlert").IsEffectivelyVisible);
        Assert.Equal("Mic 2 is out", model.LiveProducer.BoothMessage);
        window.Close();
    }

    [AvaloniaFact]
    public void A_saved_layout_comes_back_and_a_bad_layout_file_is_ignored()
    {
        var first = new LiveProducerViewModel(null, null, LayoutPath);
        first.ApplyLayoutPresetCommand.Execute(ProducerLayoutState.ProgramFocus);
        first.SaveLayoutCommand.Execute(null);
        Assert.True(File.Exists(LayoutPath));

        var second = new LiveProducerViewModel(null, null, LayoutPath);
        Assert.Equal("tiles", second.Layout.InspectorTab);
        Assert.Equal(ProducerLayoutState.ProgramFocus.Normalised().Inspector, second.Layout.Inspector, 3);

        File.WriteAllText(LayoutPath, "{ not json");
        Assert.Equal(ProducerLayoutState.Standard, new LiveProducerViewModel(null, null, LayoutPath).Layout);
        File.WriteAllText(LayoutPath, "{\"Name\":\"x\",\"ServiceOrder\":99,\"Slides\":-5,\"Inspector\":0,\"Program\":3,\"InspectorTab\":\"../../etc\"}");
        var clamped = new LiveProducerViewModel(null, null, LayoutPath).Layout;
        Assert.Equal(1.0, clamped.ServiceOrder + clamped.Slides + clamped.Inspector, 6);
        Assert.InRange(clamped.Program, 0.10, 0.80);
        Assert.Equal("layers", clamped.InspectorTab);
    }

    [AvaloniaFact]
    public void Holding_escape_for_one_second_clears_once_and_a_tap_does_not()
    {
        // safe: SAFE-03
        var (window, model, _) = Open();
        model.OnEscapeDown(TimeSpan.FromSeconds(10));
        model.OnEscapeUp();
        Assert.Equal(0, model.ClearEverythingRequests);

        model.OnEscapeDown(TimeSpan.FromSeconds(20));
        model.OnEscapeDown(TimeSpan.FromSeconds(20.5));
        Assert.Equal(0, model.ClearEverythingRequests);
        model.OnEscapeDown(TimeSpan.FromSeconds(21));
        model.OnEscapeDown(TimeSpan.FromSeconds(22));
        Assert.Equal(1, model.ClearEverythingRequests);
        model.OnEscapeUp();
        window.Close();
    }

    /// <summary>A small stand-in for core's live state machine (crates/selah-core/src/live.rs).</summary>
    internal sealed class FakeLive : ILiveService
    {
        private static readonly string[][] SlideText =
        [
            ["Be Thou my Vision", "Thou my best Thought", "Be Thou my Wisdom"],
            ["Romans 8:28", "Romans 8:29"],
            ["Sermon"],
            ["Amazing grace", "'Twas grace"],
        ];

        private readonly Id lookId = LibraryIds.NewId();
        private uint item;
        private uint slide;
        private bool cleared;
        private bool logo;
        private bool locked;
        private string? rolePin;
        private string boothMessage = string.Empty;
        private Id? activeLook;
        private readonly Stack<(uint, uint)> undo = new();

        public FakeLive()
        {
            Service = new Service { Id = LibraryIds.NewId(), Name = "Sunday Worship" };
            string[] titles = ["Be Thou My Vision", "Romans 8:28–39", "Sermon", "Amazing Grace"];
            ServiceItemKind[] kinds = [ServiceItemKind.Song, ServiceItemKind.Scripture, ServiceItemKind.Note, ServiceItemKind.Song];
            for (var i = 0; i < titles.Length; i++)
            {
                Service.Items.Add(new ServiceItem { Id = LibraryIds.NewId(), Title = titles[i], Kind = kinds[i] });
            }
        }

        public Service Service { get; }

        private static readonly HashSet<string> ProducerOnly = ["look", "macro", "timer-start", "timer-pause", "timer-reset", "booth-lock"];

        public List<string> Calls { get; } = [];

        public int ClearEverythingCount { get; private set; }

        public event EventHandler<LiveStatus>? LiveStatusChanged;

        public void Raise(Action<LiveStatus> edit)
        {
            var status = Build();
            edit(status);
            LiveStatusChanged?.Invoke(this, status);
        }

        private LiveStatus Build()
        {
            var slides = SlideText[item];
            var status = new LiveStatus
            {
                ServiceId = Service.Id,
                ServiceName = Service.Name,
                HasActiveItem = true,
                CurrentItemIndex = item,
                CurrentSlideIndex = slide,
                CurrentItemTitle = Service.Items[(int)item].Title,
                CurrentSlideLabel = $"Verse {slide + 1}",
                Cleared = cleared,
                ShowLogo = logo,
                CanLiveUndo = undo.Count > 0,
            };
            var last = slide + 1 >= slides.Length;
            status.HasNext = !(last && item + 1 >= SlideText.Length);
            status.NextItemIndex = last ? item + 1 : item;
            status.NextSlideIndex = last ? 0 : slide + 1;
            status.NextItemTitle = Service.Items[(int)status.NextItemIndex % Service.Items.Count].Title;
            status.NextSlideLabel = $"Verse {status.NextSlideIndex + 1}";
            // Like core: the words on the current and next slides.
            status.CurrentSlideText = slides[slide];
            status.NextSlideText = status.HasNext ? SlideText[status.NextItemIndex][status.NextSlideIndex] : string.Empty;
            for (var i = 0; i < slides.Length; i++)
            {
                status.CurrentItemSlides.Add(new LiveSlide { Index = (uint)i, Label = $"Verse {i + 1}", Text = slides[i], Hotkey = (i + 1).ToString(System.Globalization.CultureInfo.InvariantCulture) });
            }
            status.Layers.Add(new LiveLayerStatus { Kind = LayerKind.Slide, Active = !cleared, Title = status.CurrentItemTitle });
            status.Automation.Add(new LiveAutomationState { RoleLocked = rolePin is not null, BoothLocked = locked, BoothMessage = boothMessage, ActiveLookId = activeLook, ActiveLookName = activeLook is null ? string.Empty : "Worship" });
            return status;
        }

        private Task<LiveStatus> Do(string call, Action change, bool allowedWhenLocked = false)
        {
            Calls.Add(call);
            if ((locked && !allowedWhenLocked) || (rolePin is not null && ProducerOnly.Contains(call.Split(':')[0])))
            {
                throw new CoreException("SEL-SEC-0001", "error-not-permitted");
            }
            change();
            return Task.FromResult(Build());
        }

        private void Move(uint newItem, uint newSlide)
        {
            undo.Push((item, slide));
            item = newItem;
            slide = newSlide;
            cleared = false;
        }

        public Task<LiveStatus> GetLiveStateAsync() => Task.FromResult(Build());

        public Task<LiveStatus> LoadLiveServiceAsync(Id serviceId, bool triggerFirstItem = false) => Do("load", () => Move(0, 0));

        public Task<LiveStatus> TriggerItemAsync(uint itemIndex) => Do($"item:{itemIndex}", () => Move(itemIndex, 0));

        public Task<LiveStatus> TriggerSlideAsync(uint itemIndex, uint slideIndex) => Do($"slide:{itemIndex}:{slideIndex}", () => Move(itemIndex, slideIndex));

        public Task<LiveStatus> NextSlideAsync() => Do("next", () =>
        {
            if (slide + 1 < SlideText[item].Length)
            {
                Move(item, slide + 1);
            }
            else if (item + 1 < SlideText.Length)
            {
                Move(item + 1, 0);
            }
        }, allowedWhenLocked: true);

        public Task<LiveStatus> PreviousSlideAsync() => Do("previous", () =>
        {
            if (slide > 0)
            {
                Move(item, slide - 1);
            }
        }, allowedWhenLocked: true);

        public Task<LiveStatus> NextItemAsync() => Do("next-item", () => Move(Math.Min(item + 1, (uint)SlideText.Length - 1), 0), allowedWhenLocked: true);

        public Task<LiveStatus> PreviousItemAsync() => Do("previous-item", () => Move(item == 0 ? 0 : item - 1, 0), allowedWhenLocked: true);

        public Task<LiveStatus> TriggerSectionHotkeyAsync(string hotkey) => Do($"hotkey:{hotkey}", () => Move(item, Math.Min(uint.Parse(hotkey, System.Globalization.CultureInfo.InvariantCulture) - 1, (uint)SlideText[item].Length - 1)));

        public Task<LiveStatus> ClearLayerAsync(LayerKind kind) => Do($"clear:{kind}", () => cleared |= kind == LayerKind.Slide, allowedWhenLocked: true);

        public Task<LiveStatus> ShowLogoAsync(bool showLogo) => Do($"logo:{showLogo}", () => logo = showLogo);

        public Task<LiveStatus> LiveUndoAsync() => Do("undo", () =>
        {
            if (undo.Count > 0)
            {
                (item, slide) = undo.Pop();
            }
        });

        public Task<LiveStatus> LiveRedoAsync() => Do("redo", () => { });

        public Task<LiveStatus> SetLookAsync(Id lookId) => Do("look:Worship", () => activeLook = lookId);

        public Task<LiveStatus> RunMacroAsync(Id macroId) => Do("macro", () => { });

        public Task<LiveStatus> StartTimerAsync(Id timerId) => Do("timer-start", () => { });

        public Task<LiveStatus> PauseTimerAsync(Id timerId) => Do("timer-pause", () => { });

        public Task<LiveStatus> ResetTimerAsync(Id timerId) => Do("timer-reset", () => { });

        public Task<LiveStatus> HideMessageAsync() => Do("hide-message", () => { }, allowedWhenLocked: true);

        public Task<LiveStatus> HidePropAsync() => Do("hide-prop", () => { }, allowedWhenLocked: true);

        public Task<LiveStatus> SetBoothLockAsync(bool locked) => Do($"booth-lock:{locked}", () => this.locked = locked, allowedWhenLocked: true);

        public string LastBoothMessage => boothMessage;

        public Task<LiveStatus> SetRoleLockAsync(bool locked, string pin)
        {
            Calls.Add($"role-lock:{locked}");
            if (locked)
            {
                rolePin ??= pin;
            }
            else if (rolePin is not null && rolePin != pin)
            {
                throw new CoreException("SEL-SEC-0002", "error-role-lock-wrong-pin");
            }
            else
            {
                rolePin = null;
            }
            return Task.FromResult(Build());
        }

        public Task<LiveStatus> SendBoothMessageAsync(string text) => Do("booth-message", () => boothMessage = text, allowedWhenLocked: true);

        public Task<IReadOnlyList<Look>> ListLooksAsync() =>
            Task.FromResult<IReadOnlyList<Look>>([new Look { Id = lookId, Name = "Worship", Hotkey = "⇧1" }]);

        public Task<IReadOnlyList<Macro>> ListMacrosAsync() =>
            Task.FromResult<IReadOnlyList<Macro>>([new Macro { Id = LibraryIds.NewId(), Name = "Go to sermon", Hotkey = "M" }]);

        public Task<IReadOnlyList<Selah.Contracts.V1.Timer>> ListTimersAsync() =>
            Task.FromResult<IReadOnlyList<Selah.Contracts.V1.Timer>>([new Selah.Contracts.V1.Timer { Id = LibraryIds.NewId(), Name = "Sermon", DurationMs = 35 * 60 * 1000 }]);

        public Task<bool> ClearEverythingAsync()
        {
            ClearEverythingCount++;
            cleared = true;
            return Task.FromResult(true);
        }
    }
}
