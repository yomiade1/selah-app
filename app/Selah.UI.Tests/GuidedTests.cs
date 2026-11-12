using System.Reflection;
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

/// <summary>M4.7: practice mode, tours (M4-A9), first-run setup and Ready for Sunday in the UI.</summary>
public sealed class GuidedTests : IDisposable
{
    private readonly Reset reset = new();
    private readonly string home = Directory.CreateTempSubdirectory("selah-guided-ui-").FullName;

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

    private static string RepoFile(params string[] parts)
    {
        var dir = new DirectoryInfo(AppContext.BaseDirectory);
        while (dir is not null && !Directory.Exists(Path.Combine(dir.FullName, "content", "help", "tours")))
        {
            dir = dir.Parent;
        }
        return Path.Combine([dir!.FullName, .. parts]);
    }

    private static IReadOnlyList<TourDefinition> Tours() => TourDefinition.LoadAll(RepoFile("content", "help", "tours"));

    private (MainWindow Window, MainViewModel Model, LiveProducerTests.FakeLive Live) Open(int scale = 100, string appearance = "light", string language = "en", ILibraryService? library = null)
    {
        TextScale.Instance.Percent = scale;
        Localizer.Instance.SetLanguage(language);
        Application.Current!.RequestedThemeVariant = appearance == "booth" ? ThemeVariant.Dark : ThemeVariant.Light;
        var live = new LiveProducerTests.FakeLive();
        var model = new MainViewModel(null, null, library, null, live, Path.Combine(home, "producer-layout.json"));
        model.Tour.Tours.Clear();
        foreach (var tour in Tours())
        {
            model.Tour.Tours.Add(tour);
        }
        model.Apply(new ConnectionSnapshot(true, true, SafeMode: false, Live: false, 1));
        var window = new MainWindow { DataContext = model, Width = 1280, Height = 800 };
        window.Show();
        Dispatcher.UIThread.RunJobs();
        return (window, model, live);
    }

    private static T Named<T>(Visual root, string name) where T : Control =>
        root.GetVisualDescendants().OfType<T>().First(c => c.Name == name);

    private static bool AnchorIsShowing(Window window, string anchor) =>
        window.GetVisualDescendants().OfType<Control>().Any(c => Help.GetAnchor(c) == anchor && c.IsEffectivelyVisible);

    [AvaloniaTheory]
    [MemberData(nameof(ModesMatrix))]
    public async Task Tours_refuse_to_start_while_the_screens_are_live_unless_practice_mode_is_on(int scale, string appearance, string language)
    {
        // M4-A9, KNOWLEDGE-BASE §5.4
        var (window, model, live) = Open(scale, appearance, language);
        var simple = model.Tour.Tours.Single(t => t.Id == "tour.simple-mode-basics");
        model.SelectModeCommand.Execute("Live");
        await model.LiveProducer.LoadServiceAsync(live.Service, triggerFirst: true);
        Dispatcher.UIThread.RunJobs();

        model.Tour.ShowPickerCommand.Execute(null);
        Dispatcher.UIThread.RunJobs();
        Assert.True(Named<StackPanel>(window, "TourPicker").IsEffectivelyVisible);
        Assert.False(await model.Tour.StartAsync(simple));
        Assert.Equal(Localizer.Instance["tour-refused-live"], model.Tour.Refusal);
        Assert.False(model.Tour.IsOpen);
        Dispatcher.UIThread.RunJobs();
        Assert.True(Named<TextBlock>(window, "TourRefusal").IsEffectivelyVisible);

        await model.SetPracticeAsync(true);
        Dispatcher.UIThread.RunJobs();
        Assert.True(model.IsPracticeMode);
        Assert.True(Named<Border>(window, "PracticeBanner").IsEffectivelyVisible);
        Assert.True(await model.Tour.StartAsync(simple));
        Dispatcher.UIThread.RunJobs();
        Assert.True(model.IsSimpleView);

        // Every step points at something on the screen, and the live controls stay uncovered.
        for (var i = 0; i < simple.Steps.Count; i++)
        {
            Dispatcher.UIThread.RunJobs();
            Assert.True(AnchorIsShowing(window, model.Tour.ActiveAnchor), model.Tour.ActiveAnchor);
            Assert.True(Named<Grid>(window, "TourCoachMark").IsEffectivelyVisible);
            Assert.True(Named<Button>(window, "ClearEverythingButton").IsEffectivelyVisible);
            await model.Tour.NextCommand.ExecuteAsync(null);
        }
        Assert.False(model.Tour.IsOpen);
        Assert.True(model.IsPracticeMode, "the tour leaves practice as it found it");
        var clipped = LayoutTests.ClippedCriticalLabels(window).Where(p => !p.EndsWith("is hidden", StringComparison.Ordinal)).ToList();
        Assert.True(clipped.Count == 0, string.Join("; ", clipped));
        window.Close();
    }

    [AvaloniaFact]
    public async Task A_tour_that_needs_practice_mode_turns_it_on_and_off_again()
    {
        var (window, model, live) = Open();
        var simple = model.Tour.Tours.Single(t => t.Id == "tour.simple-mode-basics");
        Assert.True(simple.RequiresPracticeMode);
        Assert.True(await model.Tour.StartAsync(simple));
        Assert.True(model.IsPracticeMode);
        await model.Tour.SkipCommand.ExecuteAsync(null);
        Assert.False(model.IsPracticeMode);
        Assert.Equal(["practice:True", "practice:False"], live.Calls.Where(c => c.StartsWith("practice", StringComparison.Ordinal)));

        // The Producer overview points at real panes too.
        var producer = model.Tour.Tours.Single(t => t.Id == "tour.producer-overview");
        Assert.False(await model.Tour.StartAsync(producer), "the fake has a song on the screens");
        await model.SetPracticeAsync(true);
        Assert.True(await model.Tour.StartAsync(producer));
        Dispatcher.UIThread.RunJobs();
        Assert.True(model.IsProducerView);
        foreach (var step in producer.Steps)
        {
            Dispatcher.UIThread.RunJobs();
            Assert.True(AnchorIsShowing(window, model.Tour.ActiveAnchor), model.Tour.ActiveAnchor);
            await model.Tour.NextCommand.ExecuteAsync(null);
        }
        window.Close();
    }

    [AvaloniaFact]
    public async Task Practice_mode_shows_the_banner_and_is_refused_under_booth_lock()
    {
        // safe: SAFE-01, SAFE-06
        var (window, model, live) = Open();
        model.SelectModeCommand.Execute("Live");
        Dispatcher.UIThread.RunJobs();
        Assert.Equal(Localizer.Instance["live-practice-start"], model.PracticeButtonText);
        await model.TogglePracticeCommand.ExecuteAsync(null);
        Dispatcher.UIThread.RunJobs();
        Assert.True(Named<Border>(window, "PracticeBanner").IsEffectivelyVisible);
        Assert.Equal(Localizer.Instance["live-practice-stop"], model.PracticeButtonText);
        await model.TogglePracticeCommand.ExecuteAsync(null);
        Dispatcher.UIThread.RunJobs();
        Assert.False(Named<Border>(window, "PracticeBanner").IsVisible);

        await live.SetBoothLockAsync(true);
        await model.SetPracticeAsync(true);
        Assert.False(model.IsPracticeMode);
        window.Close();
    }

    [AvaloniaFact]
    public async Task First_run_setup_saves_the_answers_and_adds_the_sample_service()
    {
        var library = FakeLibrary.Create();
        var setup = new SetupViewModel(library.Service);
        await setup.OpenIfFirstRunAsync();
        Assert.True(setup.IsOpen);
        Assert.True(setup.IsWelcome);
        Assert.Equal("KJV", setup.SelectedBible?.Code);

        setup.NextCommand.Execute(null);
        setup.NextCommand.Execute(null);
        setup.NextCommand.Execute(null);
        Assert.True(setup.IsCcli);
        setup.CcliNumber = "CCLI-1";
        setup.NextCommand.Execute(null);
        Assert.True(setup.IsCcli, "letters are refused");
        Assert.Equal(Localizer.Instance["setup-ccli-invalid"], setup.StatusText);
        setup.CcliNumber = "1234567";
        setup.NextCommand.Execute(null);
        Assert.True(setup.IsSample);

        var service = await SampleService.AddAsync(library.Service, RepoFile("content", "samples", "sample-service.json"));
        Assert.Equal(3, service.Items.Count);
        Assert.Equal(3, library.Calls.Count(c => c == nameof(ILibraryService.SaveSongAsync)));
        Assert.All(library.Songs, s => Assert.Equal("Public Domain", s.Copyright));

        await setup.FinishCommand.ExecuteAsync(null);
        Assert.False(setup.IsOpen);
        Assert.Equal("true", library.Policies[SetupViewModel.CompletedKey]);
        Assert.Equal("\"1234567\"", library.Policies[SetupViewModel.CcliKey]);
        Assert.Equal("\"KJV\"", library.Policies[SetupViewModel.BibleKey]);

        var again = new SetupViewModel(library.Service);
        await again.OpenIfFirstRunAsync();
        Assert.False(again.IsOpen, "setup runs once");
    }

    [AvaloniaFact]
    public async Task Ready_for_sunday_lists_problems_first_and_links_each_to_its_fix()
    {
        var library = FakeLibrary.Create();
        library.Report = new ReadyReport
        {
            Ready = false,
            Checks =
            {
                new ReadyCheck { Kind = ReadyCheckKind.Media, Status = ReadyCheckStatus.Pass, MessageKey = "ready-media-ok", ItemIndex = -1 },
                new ReadyCheck { Kind = ReadyCheckKind.Outputs, Status = ReadyCheckStatus.Fail, Code = "SEL-RDY-0006", MessageKey = "error-ready-no-screens", FixArticleId = "troubleshooting.screens-are-black", ItemIndex = -1 },
                new ReadyCheck { Kind = ReadyCheckKind.Copyright, Status = ReadyCheckStatus.Warn, Code = "SEL-RDY-0004", MessageKey = "error-ready-no-copyright", FixArticleId = "services.add-copyright-and-licence-details", Args = { ["item"] = "New Song" } },
            },
        };
        var service = new Service { Id = LibraryIds.NewId(), Name = "Sunday" };
        var ready = new ReadyCheckViewModel(library.Service, () => service);
        string? opened = null;
        ready.FixRequested += (_, id) => opened = id;
        await ready.RunCommand.ExecuteAsync(null);

        Assert.Equal(Localizer.Instance["ready-has-problems"], ready.Summary);
        Assert.True(ready.Rows[0].IsFail);
        Assert.True(ready.Rows[1].IsWarn);
        Assert.True(ready.Rows[2].IsPass);
        Assert.False(ready.Rows[2].HasFix);
        Assert.Contains("New Song", ready.Rows[1].Message, StringComparison.Ordinal);
        ready.OpenFixCommand.Execute(ready.Rows[0]);
        Assert.Equal("troubleshooting.screens-are-black", opened);
    }

    [AvaloniaTheory]
    [MemberData(nameof(ModesMatrix))]
    public void The_ready_for_sunday_card_and_setup_fit_at_every_text_size(int scale, string appearance, string language)
    {
        var library = FakeLibrary.Create();
        var (window, model, _) = Open(scale, appearance, language, library.Service);
        model.SelectModeCommand.Execute("ServicePlanner");
        model.Setup.IsOpen = true;
        Dispatcher.UIThread.RunJobs();
        Assert.True(Named<Button>(window, "RunReadyCheckButton").IsEffectivelyVisible);
        Assert.True(Named<Border>(window, "SetupPanel").IsEffectivelyVisible);
        Assert.True(Named<Button>(window, "SetupNextButton").IsEffectivelyVisible);
        Assert.True(Named<Button>(window, "ClearEverythingButton").IsEffectivelyVisible);
        Assert.DoesNotContain(LayoutTests.ClippedCriticalLabels(window), p => !p.EndsWith("is hidden", StringComparison.Ordinal));
        window.Close();
    }

    [AvaloniaTheory]
    [MemberData(nameof(ModesMatrix))]
    public async Task Settings_change_the_ccli_licence_and_the_first_bible_after_setup(int scale, string appearance, string language)
    {
        var library = FakeLibrary.Create();
        library.Policies[SetupViewModel.CcliKey] = "\"12345\"";
        var (window, model, _) = Open(scale, appearance, language, library.Service);
        var settings = new SettingsWindow { DataContext = model.Settings, Width = 820, Height = 720 };
        settings.Show();
        var church = model.Settings.Church;
        await church.LoadAsync();
        Dispatcher.UIThread.RunJobs();
        Assert.Equal("12345", church.CcliNumber);
        Assert.Equal("WEB", church.SelectedBible?.Code);
        foreach (var name in new[] { "CcliNumber", "DefaultBible", "SaveChurch" })
        {
            Assert.True(settings.GetVisualDescendants().OfType<Control>().First(c => c.Name == name).IsEffectivelyVisible, name);
        }

        // Letters are refused and nothing is saved.
        church.CcliNumber = "12A";
        await church.SaveCommand.ExecuteAsync(null);
        Assert.Equal(Localizer.Instance["setup-ccli-invalid"], church.StatusText);
        Assert.Equal("\"12345\"", library.Policies[SetupViewModel.CcliKey]);

        church.CcliNumber = " 7654321 ";
        church.SelectedBible = church.Bibles.Single(b => b.Code == "KJV");
        await church.SaveCommand.ExecuteAsync(null);
        Assert.Equal("\"7654321\"", library.Policies[SetupViewModel.CcliKey]);
        Assert.Equal("\"KJV\"", library.Policies[SetupViewModel.BibleKey]);
        Assert.Equal(Localizer.Instance["church-saved"], church.StatusText);
        Assert.DoesNotContain(LayoutTests.ClippedCriticalLabels(settings), p => !p.EndsWith("is hidden", StringComparison.Ordinal));
        settings.Close();
        window.Close();
    }

    /// <summary>A library stand-in that records calls; unhandled calls return empty results.</summary>
    public class FakeLibrary : DispatchProxy
    {
        public ILibraryService Service { get; private set; } = null!;

        public List<string> Calls { get; } = [];

        public Dictionary<string, string> Policies { get; } = [];

        public List<Song> Songs { get; } = [];

        public ReadyReport Report { get; set; } = new();

        public static FakeLibrary Create()
        {
            var proxy = DispatchProxy.Create<ILibraryService, FakeLibrary>();
            var fake = (FakeLibrary)(object)proxy;
            fake.Service = proxy;
            return fake;
        }

        protected override object? Invoke(MethodInfo? method, object?[]? args)
        {
            var name = method!.Name;
            Calls.Add(name);
            switch (name)
            {
                case nameof(ILibraryService.GetPolicySettingAsync):
                    var key = (string)args![0]!;
                    return Task.FromResult(Policies.TryGetValue(key, out var value)
                        ? new GetPolicySettingReply { Found = true, Setting = new PolicySetting { Key = key, ValueJson = value } }
                        : new GetPolicySettingReply());
                case nameof(ILibraryService.SetPolicySettingAsync):
                    Policies[(string)args![0]!] = (string)args[1]!;
                    return Task.CompletedTask;
                case nameof(ILibraryService.ListBiblesAsync):
                    return Task.FromResult<IReadOnlyList<BibleTranslation>>([new BibleTranslation { Code = "WEB", Name = "World English Bible" }, new BibleTranslation { Code = "KJV", Name = "King James Version" }]);
                case nameof(ILibraryService.GetOutputConfigsAsync):
                    return Task.FromResult<IReadOnlyList<OutputConfig>>([new OutputConfig { OutputIndex = 0, Name = "Main", Enabled = true }]);
                case nameof(ILibraryService.SaveSongAsync):
                    Songs.Add((Song)args![0]!);
                    return Task.CompletedTask;
                case nameof(ILibraryService.RunReadyCheckAsync):
                    return Task.FromResult(Report);
            }
            var type = method.ReturnType;
            if (type == typeof(Task))
            {
                return Task.CompletedTask;
            }
            if (type.IsGenericType && type.GetGenericTypeDefinition() == typeof(Task<>))
            {
                var inner = type.GetGenericArguments()[0];
                object? result = inner.IsGenericType && inner.GetGenericTypeDefinition() == typeof(IReadOnlyList<>)
                    ? Array.CreateInstance(inner.GetGenericArguments()[0], 0)
                    : inner.IsValueType ? Activator.CreateInstance(inner)
                    : inner.GetConstructor(Type.EmptyTypes) is not null ? Activator.CreateInstance(inner) : null;
                return typeof(Task).GetMethod(nameof(Task.FromResult))!.MakeGenericMethod(inner).Invoke(null, [result]);
            }
            return type.IsValueType && type != typeof(void) ? Activator.CreateInstance(type) : null;
        }
    }
}
