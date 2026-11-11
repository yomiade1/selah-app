using System.Collections.ObjectModel;
using System.ComponentModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Selah.Contracts.V1;
using Selah.UI.Services;

namespace Selah.UI.ViewModels;

/// <summary>One step in the Simple-mode service progress (prototype 02).</summary>
public sealed partial class SimpleStep : ObservableObject
{
    public required uint ItemIndex { get; init; }

    public required int Number { get; init; }

    public required string Title { get; init; }

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(Marker))]
    private bool isDone;

    [ObservableProperty]
    private bool isCurrent;

    /// <summary>A check mark for done steps, else the step number (never colour alone).</summary>
    public string Marker => IsDone ? "✓" : Number.ToString(System.Globalization.CultureInfo.CurrentCulture);
}

/// <summary>
/// Simple mode (M4.5, prototype 02): Now and Next, big buttons and plain clear buttons, for
/// volunteers. It shares live state and commands with <see cref="LiveProducerViewModel"/>, so
/// both views always agree with core. When the role lock is on, core enforces it (SEC-14).
/// </summary>
public sealed partial class SimpleModeViewModel : ObservableObject
{
    private static readonly string[] TipKeys = ["simple-tip-next", "simple-tip-clear", "simple-tip-wrong-slide"];
    private readonly ILiveService? live;
    private int tipIndex;

    public SimpleModeViewModel(LiveProducerViewModel producer, ILiveService? live)
    {
        Producer = producer;
        this.live = live;
        producer.PropertyChanged += OnProducerChanged;
        producer.Items.CollectionChanged += (_, _) => RebuildSteps();
        Tip = Localizer.Instance[TipKeys[0]];
    }

    public LiveProducerViewModel Producer { get; }

    public ObservableCollection<SimpleStep> Steps { get; } = [];

    public ObservableCollection<LiveSlideCard> Slides => Producer.Slides;

    [ObservableProperty]
    private string nowText = string.Empty;

    [ObservableProperty]
    private string nowCaption = string.Empty;

    [ObservableProperty]
    private string nextText = string.Empty;

    [ObservableProperty]
    private string nextCaption = string.Empty;

    [ObservableProperty]
    private string tip;

    [ObservableProperty]
    private bool isComposingBoothMessage;

    [ObservableProperty]
    [NotifyCanExecuteChangedFor(nameof(SendBoothMessageCommand))]
    private string boothMessageDraft = string.Empty;

    [ObservableProperty]
    private string? boothMessageStatus;

    /// <summary>Set by the main window from core's role lock (SEC-14).</summary>
    [ObservableProperty]
    private bool isRoleLocked;

    private void OnProducerChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(LiveProducerViewModel.Status))
        {
            Refresh(Producer.Status);
        }
    }

    private void RebuildSteps()
    {
        Steps.Clear();
        var number = 0;
        foreach (var item in Producer.Items.Where(i => !i.IsHeader))
        {
            Steps.Add(new SimpleStep { ItemIndex = item.Index, Number = ++number, Title = item.Title });
        }
        Refresh(Producer.Status);
    }

    private void Refresh(LiveStatus status)
    {
        var count = status.CurrentItemSlides.Count;
        var number = (status.CurrentSlideIndex + 1).ToString(System.Globalization.CultureInfo.CurrentCulture);
        NowText = status.HasActiveItem && !status.Cleared ? status.CurrentSlideText : string.Empty;
        NowCaption = status.HasActiveItem
            ? Localizer.Instance.Get("simple-now-caption", ("label", status.CurrentSlideLabel), ("number", number), ("count", count))
            : Localizer.Instance["simple-nothing-on-screen"];
        NextText = status.HasNext ? status.NextSlideText : string.Empty;
        NextCaption = status.HasNext
            ? status.NextItemIndex == status.CurrentItemIndex
                ? Localizer.Instance.Get("simple-next-caption", ("label", status.NextSlideLabel), ("number", (status.NextSlideIndex + 1).ToString(System.Globalization.CultureInfo.CurrentCulture)))
                : Localizer.Instance.Get("simple-next-song-caption", ("title", status.NextItemTitle))
            : Localizer.Instance["live-next-none"];
        foreach (var step in Steps)
        {
            step.IsDone = status.HasActiveItem && step.ItemIndex < status.CurrentItemIndex;
            step.IsCurrent = status.HasActiveItem && step.ItemIndex == status.CurrentItemIndex;
        }
    }

    [ObservableProperty]
    private bool isUnlocking;

    [ObservableProperty]
    private string unlockPin = string.Empty;

    [ObservableProperty]
    private string? unlockError;

    /// <summary>Raised after core turned the role lock off.</summary>
    public event EventHandler? Unlocked;

    [RelayCommand]
    private void StartUnlock()
    {
        IsUnlocking = true;
        UnlockError = null;
    }

    /// <summary>SEC-14: core checks the PIN and rate-limits wrong ones.</summary>
    [RelayCommand]
    private async Task UnlockAsync()
    {
        if (live is null)
        {
            return;
        }
        try
        {
            var status = await live.SetRoleLockAsync(false, UnlockPin);
            UnlockPin = string.Empty;
            IsUnlocking = false;
            UnlockError = null;
            Producer.Apply(status);
            Unlocked?.Invoke(this, EventArgs.Empty);
        }
        catch (CoreException error)
        {
            UnlockPin = string.Empty;
            UnlockError = Localizer.Instance[error.Code == "SEL-SEC-0003" ? "role-lock-too-many-tries" : "role-lock-wrong-pin"];
        }
    }

    /// <summary>A short tap on Esc hides the words; holding it clears everything (in the window).</summary>
    public void OnEscapeTapped() => Producer.ClearTextCommand.Execute(null);

    [RelayCommand]
    private void NextTip()
    {
        tipIndex = (tipIndex + 1) % TipKeys.Length;
        Tip = Localizer.Instance[TipKeys[tipIndex]];
    }

    [RelayCommand]
    private void OpenBoothMessage()
    {
        IsComposingBoothMessage = true;
        BoothMessageStatus = null;
    }

    [RelayCommand]
    private void CancelBoothMessage()
    {
        IsComposingBoothMessage = false;
        BoothMessageDraft = string.Empty;
    }

    private bool CanSendBoothMessage() => !string.IsNullOrWhiteSpace(BoothMessageDraft);

    /// <summary>SAFE-01: goes to the producer's screen and the stage alert only, never the audience.</summary>
    [RelayCommand(CanExecute = nameof(CanSendBoothMessage))]
    private async Task SendBoothMessageAsync()
    {
        if (live is null)
        {
            return;
        }
        var text = BoothMessageDraft.Trim();
        // Outside input is parsed with limits: a booth message is one short line.
        if (text.Length > 200)
        {
            text = text[..200];
        }
        try
        {
            await live.SendBoothMessageAsync(text);
            BoothMessageStatus = Localizer.Instance["simple-booth-message-sent"];
            BoothMessageDraft = string.Empty;
            IsComposingBoothMessage = false;
        }
        catch (CoreException)
        {
            BoothMessageStatus = Localizer.Instance["simple-booth-message-failed"];
        }
    }
}
