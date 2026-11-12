using Avalonia;
using Avalonia.Styling;
using CommunityToolkit.Mvvm.ComponentModel;
using Selah.Contracts.V1;
using Selah.UI.Services;

namespace Selah.UI.ViewModels;

/// <summary>Settings ▸ Appearance (DESIGN-SYSTEM §4.1, prototype screen 8).</summary>
public sealed partial class SettingsViewModel : ObservableObject
{
    private readonly Func<AppearanceSettings, Task>? save;
    private bool applyingFromCore;

    public SettingsViewModel(Func<AppearanceSettings, Task>? save = null, UpdatesViewModel? updates = null, ChurchSettingsViewModel? church = null)
    {
        this.save = save;
        Updates = updates ?? new UpdatesViewModel();
        Church = church ?? new ChurchSettingsViewModel(null);
        TextScale.Instance.PropertyChanged += (_, args) =>
        {
            if (args.PropertyName == nameof(TextScale.Percent))
            {
                OnPropertyChanged(nameof(TextSizeIndex));
                OnPropertyChanged(nameof(TextSizeName));
                OnPropertyChanged(nameof(TextSizeValue));
            }
        };
        Localizer.Instance.PropertyChanged += (_, _) =>
        {
            OnPropertyChanged(nameof(TextSizeName));
            OnPropertyChanged(nameof(TextSizeValue));
        };
    }

    /// <summary>Settings ▸ Updates.</summary>
    public UpdatesViewModel Updates { get; }

    /// <summary>Settings ▸ Church: CCLI licence number and the first Bible.</summary>
    public ChurchSettingsViewModel Church { get; }

    /// <summary>0 Light, 1 Booth, 2 Automatic (the order of the segmented control).</summary>
    [ObservableProperty]
    private int appearanceIndex;

    [ObservableProperty]
    private bool useComputerTextSize;

    public int TextSizeMaximum => TextScale.Steps.Count - 1;

    /// <summary>The slider position: an index into the 7 steps.</summary>
    public int TextSizeIndex
    {
        get => TextScale.Instance.StepIndex;
        set
        {
            var clamped = Math.Clamp(value, 0, TextSizeMaximum);
            if (clamped == TextScale.Instance.StepIndex)
            {
                return;
            }
            TextScale.Instance.Percent = TextScale.Steps[clamped];
            Persist();
        }
    }

    public string TextSizeName => Localizer.Instance[TextScale.StepNameKeys[TextScale.Instance.StepIndex]];

    public string TextSizeValue => Localizer.Instance.Get("text-size-value", ("percent", TextScale.Instance.Percent));

    public bool CanChooseTextSize => !UseComputerTextSize;

    partial void OnAppearanceIndexChanged(int value)
    {
        ApplyTheme();
        Persist();
    }

    partial void OnUseComputerTextSizeChanged(bool value)
    {
        OnPropertyChanged(nameof(CanChooseTextSize));
        if (value)
        {
            TextScale.Instance.Percent = SystemTextScale.Step();
        }
        Persist();
    }

    public void StepUp() => ChangeBy(TextScale.Instance.StepUp);

    public void StepDown() => ChangeBy(TextScale.Instance.StepDown);

    public void Reset() => ChangeBy(TextScale.Instance.Reset);

    private void ChangeBy(Action change)
    {
        UseComputerTextSize = false;
        change();
        Persist();
    }

    /// <summary>Core sent the stored settings (on each connection).</summary>
    public void ApplyFromCore(AppearanceSettings settings)
    {
        applyingFromCore = true;
        try
        {
            AppearanceIndex = settings.Appearance switch
            {
                Appearance.Booth => 1,
                Appearance.Automatic => 2,
                _ => 0,
            };
            UseComputerTextSize = settings.UseComputerTextSize;
            var percent = settings.UseComputerTextSize ? SystemTextScale.Step() : (int)settings.TextScalePercent;
            TextScale.Instance.Percent = TextScale.IsStep(percent) ? percent : TextScale.DefaultPercent;
        }
        finally
        {
            applyingFromCore = false;
        }
    }

    public AppearanceSettings ToSettings() => new()
    {
        Appearance = AppearanceIndex switch
        {
            1 => Appearance.Booth,
            2 => Appearance.Automatic,
            _ => Appearance.Light,
        },
        TextScalePercent = (uint)TextScale.Instance.Percent,
        UseComputerTextSize = UseComputerTextSize,
    };

    public void ApplyTheme()
    {
        if (Application.Current is { } app)
        {
            app.RequestedThemeVariant = AppearanceIndex switch
            {
                1 => ThemeVariant.Dark,
                2 => ThemeVariant.Default,
                _ => ThemeVariant.Light,
            };
        }
    }

    private void Persist()
    {
        if (!applyingFromCore && save is not null)
        {
            _ = save(ToSettings());
        }
    }
}
