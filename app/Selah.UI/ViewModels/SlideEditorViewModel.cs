using System.Collections.ObjectModel;
using System.Text.Json;
using Avalonia.Media.Imaging;
using Avalonia.Threading;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Selah.Contracts.V1;
using Selah.UI.Services;

namespace Selah.UI.ViewModels;

/// <summary>Editable wrapper around one <see cref="SlideElementModel"/> in the custom slide canvas.</summary>
public sealed partial class SlideElementItemViewModel : ObservableObject
{
    private readonly Action onChanged;

    public SlideElementItemViewModel(SlideElementModel model, Action onChanged)
    {
        Model = model;
        this.onChanged = onChanged;
        id = model.Id;
        name = model.Name;
        kind = model.Kind;
        groupId = model.GroupId;
        x = Math.Round(model.X, 4);
        y = Math.Round(model.Y, 4);
        width = Math.Round(model.Width, 4);
        height = Math.Round(model.Height, 4);
        opacity = Math.Round(model.Opacity, 2);
        visible = model.Visible;
        locked = model.Locked;
        zIndex = model.ZIndex;
        text = model.Text;
        fontFamily = BundledFonts.Canonical(model.FontFamily);
        fontSize = Math.Round(model.FontSize, 1);
        weight = model.Weight;
        colorHex = model.Color;
        alignH = model.AlignH;
        alignV = model.AlignV;
        clockShowSeconds = model.ClockShowSeconds;
        clock24h = model.Clock24h;
        clockOffsetMinutes = model.ClockOffsetMinutes;
        timerDurationSecs = (int)(model.TimerDurationSecs ?? 300);
        timerCountUp = model.TimerCountUp;
        shapeKind = model.ShapeKind;
        fillColorHex = model.FillColor;
        strokeColorHex = model.StrokeColor ?? string.Empty;
        strokeWidth = Math.Round(model.StrokeWidth, 1);
        cornerRadius = Math.Round(model.CornerRadius, 1);
        contentHash = model.ContentHash;
        mediaExtension = model.MediaExtension;
        fit = model.Fit;
        blurRadius = Math.Round(model.BlurRadius, 1);
    }

    public SlideElementModel Model { get; }

    public string Id => id;
    private string id;

    [ObservableProperty]
    private bool isSelected;

    [ObservableProperty]
    private string name;

    public string Kind => kind;
    private readonly string kind;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasGroup))]
    private string? groupId;

    public bool HasGroup => !string.IsNullOrWhiteSpace(GroupId);

    public bool IsTextKind => Kind == SlideElementKinds.Text;

    public bool IsShapeKind => Kind == SlideElementKinds.Shape;

    public bool IsMediaKind => Kind is SlideElementKinds.Image or SlideElementKinds.Video;

    public bool IsClockToken => IsTextKind && Text.Contains("{clock}", StringComparison.Ordinal);

    public bool IsTimerToken => IsTextKind && Text.Contains("{timer}", StringComparison.Ordinal);

    [ObservableProperty]
    private double x;

    [ObservableProperty]
    private double y;

    [ObservableProperty]
    private double width;

    [ObservableProperty]
    private double height;

    [ObservableProperty]
    private double opacity;

    [ObservableProperty]
    private bool visible;

    [ObservableProperty]
    private bool locked;

    [ObservableProperty]
    private int zIndex;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsClockToken))]
    [NotifyPropertyChangedFor(nameof(IsTimerToken))]
    private string text;

    [ObservableProperty]
    private string fontFamily;

    [ObservableProperty]
    private double fontSize;

    [ObservableProperty]
    private int weight;

    [ObservableProperty]
    private string colorHex;

    [ObservableProperty]
    private string alignH;

    [ObservableProperty]
    private string alignV;

    [ObservableProperty]
    private bool clockShowSeconds;

    [ObservableProperty]
    private bool clock24h;

    [ObservableProperty]
    private int clockOffsetMinutes;

    [ObservableProperty]
    private int timerDurationSecs;

    [ObservableProperty]
    private bool timerCountUp;

    [ObservableProperty]
    private string shapeKind;

    [ObservableProperty]
    private string fillColorHex;

    [ObservableProperty]
    private string strokeColorHex;

    [ObservableProperty]
    private double strokeWidth;

    [ObservableProperty]
    private double cornerRadius;

    [ObservableProperty]
    private string contentHash;

    [ObservableProperty]
    private string mediaExtension;

    [ObservableProperty]
    private string fit;

    [ObservableProperty]
    private double blurRadius;

    private bool suppressing;

    public void SyncFromModel()
    {
        suppressing = true;
        try
        {
            Name = Model.Name;
            GroupId = Model.GroupId;
            X = Math.Round(Model.X, 4);
            Y = Math.Round(Model.Y, 4);
            Width = Math.Round(Model.Width, 4);
            Height = Math.Round(Model.Height, 4);
            Opacity = Math.Round(Model.Opacity, 2);
            Visible = Model.Visible;
            Locked = Model.Locked;
            ZIndex = Model.ZIndex;
        }
        finally
        {
            suppressing = false;
        }
    }

    public void PushToModel()
    {
        Model.Name = Name;
        Model.GroupId = string.IsNullOrWhiteSpace(GroupId) ? null : GroupId;
        Model.X = (float)X;
        Model.Y = (float)Y;
        Model.Width = (float)Width;
        Model.Height = (float)Height;
        Model.Opacity = (float)Opacity;
        Model.Visible = Visible;
        Model.Locked = Locked;
        Model.ZIndex = ZIndex;
        Model.Text = Text;
        Model.FontFamily = BundledFonts.Canonical(FontFamily);
        Model.FontSize = (float)FontSize;
        Model.Weight = Math.Clamp(Weight, 100, 900);
        Model.Color = TemplateColorHelper.NormalizeHexOrDefault(ColorHex, "#FFFFFF");
        Model.AlignH = AlignH;
        Model.AlignV = AlignV;
        Model.ClockShowSeconds = ClockShowSeconds;
        Model.Clock24h = Clock24h;
        Model.ClockOffsetMinutes = ClockOffsetMinutes;
        Model.TimerDurationSecs = (uint)Math.Max(1, TimerDurationSecs);
        Model.TimerCountUp = TimerCountUp;
        Model.ShapeKind = ShapeKind;
        Model.FillColor = TemplateColorHelper.NormalizeHexOrDefault(FillColorHex, "#2378C3E0");
        Model.StrokeColor = string.IsNullOrWhiteSpace(StrokeColorHex)
            ? null
            : TemplateColorHelper.NormalizeHexOrDefault(StrokeColorHex, "#FFFFFF");
        Model.StrokeWidth = (float)StrokeWidth;
        Model.CornerRadius = (float)CornerRadius;
        Model.ContentHash = ContentHash;
        Model.MediaExtension = MediaExtension;
        Model.Fit = Fit;
        Model.BlurRadius = (float)BlurRadius;
        Model.Clamp();
    }

    private void NotifyMutated()
    {
        if (!suppressing)
        {
            PushToModel();
            onChanged();
        }
    }

    partial void OnNameChanged(string value) => NotifyMutated();
    partial void OnGroupIdChanged(string? value) => NotifyMutated();
    partial void OnXChanged(double value) => NotifyMutated();
    partial void OnYChanged(double value) => NotifyMutated();
    partial void OnWidthChanged(double value) => NotifyMutated();
    partial void OnHeightChanged(double value) => NotifyMutated();
    partial void OnOpacityChanged(double value) => NotifyMutated();
    partial void OnVisibleChanged(bool value) => NotifyMutated();
    partial void OnLockedChanged(bool value) => NotifyMutated();
    partial void OnZIndexChanged(int value) => NotifyMutated();
    partial void OnTextChanged(string value) => NotifyMutated();
    partial void OnFontFamilyChanged(string value) => NotifyMutated();
    partial void OnFontSizeChanged(double value) => NotifyMutated();
    partial void OnWeightChanged(int value) => NotifyMutated();
    partial void OnColorHexChanged(string value) => NotifyMutated();
    partial void OnAlignHChanged(string value) => NotifyMutated();
    partial void OnAlignVChanged(string value) => NotifyMutated();
    partial void OnClockShowSecondsChanged(bool value) => NotifyMutated();
    partial void OnClock24hChanged(bool value) => NotifyMutated();
    partial void OnClockOffsetMinutesChanged(int value) => NotifyMutated();
    partial void OnTimerDurationSecsChanged(int value) => NotifyMutated();
    partial void OnTimerCountUpChanged(bool value) => NotifyMutated();
    partial void OnShapeKindChanged(string value) => NotifyMutated();
    partial void OnFillColorHexChanged(string value) => NotifyMutated();
    partial void OnStrokeColorHexChanged(string value) => NotifyMutated();
    partial void OnStrokeWidthChanged(double value) => NotifyMutated();
    partial void OnCornerRadiusChanged(double value) => NotifyMutated();
    partial void OnContentHashChanged(string value) => NotifyMutated();
    partial void OnMediaExtensionChanged(string value) => NotifyMutated();
    partial void OnFitChanged(string value) => NotifyMutated();
    partial void OnBlurRadiusChanged(double value) => NotifyMutated();
}

/// <summary>One editable slide inside a custom <see cref="Presentation"/>.</summary>
public sealed partial class SlideDeckEntryViewModel : ObservableObject
{
    public SlideDeckEntryViewModel(Id id, int number, SlideElementsDocumentModel document)
    {
        Id = id;
        this.number = number;
        Document = document;
    }

    public Id Id { get; }

    [ObservableProperty]
    private int number;

    public SlideElementsDocumentModel Document { get; set; }

    public string SummaryLabel => $"{Number}. {Document.Elements.Count} ({Document.Transition?.Kind ?? "fade"})";

    public void RefreshSummary() => OnPropertyChanged(nameof(SummaryLabel));

    public Slide ToProto()
    {
        var summaryText = string.Join(
            " ",
            Document.Elements
                .Where(e => e.Kind == SlideElementKinds.Text && !string.IsNullOrWhiteSpace(e.Text))
                .Select(e => e.Text.Trim()));
        return new Slide
        {
            Id = Id,
            Text = summaryText,
            ElementsJson = Document.ToJson(),
        };
    }
}

/// <summary>One built-in effect entry on a slide (closed to the 6 safe M3.5 effects; SEC-04).</summary>
public sealed partial class SlideEffectItemViewModel : ObservableObject
{
    private readonly Action onChanged;

    public SlideEffectItemViewModel(SlideEffectEntryModel model, Action onChanged)
    {
        Model = model;
        this.onChanged = onChanged;
        kind = SlideEffectKinds.IsAllowed(model.Kind) ? model.Kind : "vignette";
        enabled = model.Enabled;
        intensity = Math.Round(model.Intensity, 2);
    }

    public SlideEffectEntryModel Model { get; }

    [ObservableProperty]
    private string kind;

    [ObservableProperty]
    private bool enabled;

    [ObservableProperty]
    private double intensity;

    partial void OnKindChanged(string value)
    {
        Model.Kind = SlideEffectKinds.IsAllowed(value) ? value : "vignette";
        onChanged();
    }

    partial void OnEnabledChanged(bool value)
    {
        Model.Enabled = value;
        onChanged();
    }

    partial void OnIntensityChanged(double value)
    {
        Model.Intensity = (float)Math.Clamp(value, 0, 1);
        onChanged();
    }
}

/// <summary>
/// Custom Slide Editor ViewModel (M3.7 / M3-A5):
/// Presentation &amp; slide strip, positioned text/clock/timer/shape/image/video elements,
/// multi-select alignment, grouping, locking, Z-order, backgrounds, transitions, closed 6 effects,
/// WCAG 2.3.1 SAFE-04 flash guard on Stage, live offscreen canvas preview, and local + Core Undo/Redo.
/// </summary>
public sealed partial class SlideEditorViewModel : ObservableObject
{
    private readonly ILibraryService? library;
    private readonly Stack<string> localUndo = new();
    private readonly Stack<string> localRedo = new();
    private bool coreCanUndo;
    private bool coreCanRedo;
    private bool suppressCallbacks;
    private int previewGeneration;

    public SlideEditorViewModel(ILibraryService? library)
    {
        this.library = library;
        PresentationId = LibraryIds.NewId();
        StartNewPresentation();
        if (library is not null)
        {
            library.UndoStateChanged += (_, state) =>
            {
                if (state.DocId is null || LibraryIds.Same(state.DocId, PresentationId))
                {
                    Dispatcher.UIThread.Post(() =>
                    {
                        var undo = state.Document ?? state.Global;
                        if (undo is not null)
                        {
                            coreCanUndo = undo.CanUndo;
                            coreCanRedo = undo.CanRedo;
                            RefreshUndoProperties();
                        }
                    });
                }
            };
        }
    }

    public Id PresentationId { get; private set; }

    public IReadOnlyList<string> FontFamilies => BundledFonts.Families;
    public IReadOnlyList<string> ShapeKinds => SlideShapeKinds.All;
    public IReadOnlyList<string> AlignHOptions { get; } = ["left", "center", "right", "justify"];
    public IReadOnlyList<string> AlignVOptions { get; } = ["top", "middle", "bottom"];
    public IReadOnlyList<string> FitOptions { get; } = ["fit", "fill", "stretch"];
    public IReadOnlyList<string> BackgroundModes { get; } = ["none", "solid", "image", "video"];
    public IReadOnlyList<string> TransitionKinds => SlideTransitionKinds.AllKinds;
    public IReadOnlyList<string> TransitionDirections => SlideTransitionKinds.AllDirections;
    public IReadOnlyList<string> TransitionEasings => SlideTransitionKinds.AllEasings;
    public IReadOnlyList<string> EffectKinds => SlideEffectKinds.BuiltinKinds;

    public ObservableCollection<Presentation> Presentations { get; } = [];
    public ObservableCollection<SlideDeckEntryViewModel> Slides { get; } = [];
    public ObservableCollection<SlideElementItemViewModel> Elements { get; } = [];
    public ObservableCollection<SlideEffectItemViewModel> Effects { get; } = [];
    public ObservableCollection<MediaItem> AvailableMedia { get; } = [];
    public ObservableCollection<Template> AvailableTemplates { get; } = [];

    [ObservableProperty]
    private Presentation? selectedPresentation;

    [ObservableProperty]
    private string presentationName = "Custom slides";

    [ObservableProperty]
    private Template? selectedTemplate;

    [ObservableProperty]
    [NotifyCanExecuteChangedFor(nameof(StageSlideCommand))]
    private SlideDeckEntryViewModel? selectedSlide;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasSelectedElement))]
    private SlideElementItemViewModel? selectedElement;

    public bool HasSelectedElement => SelectedElement is not null;

    // ----- Slide Background / Transition / New Effect -----
    [ObservableProperty]
    private string backgroundMode = "solid";

    [ObservableProperty]
    private string backgroundColorHex = "#0F1724";

    [ObservableProperty]
    private MediaItem? selectedBackgroundMedia;

    [ObservableProperty]
    private string backgroundFit = "fill";

    [ObservableProperty]
    private double backgroundBlurRadius;

    [ObservableProperty]
    private string transitionKind = "fade";

    [ObservableProperty]
    private string transitionDirection = "left";

    [ObservableProperty]
    private string transitionEasing = "ease_in_out";

    [ObservableProperty]
    private int transitionDurationMs = 350;

    [ObservableProperty]
    private int repeatCadenceMs;

    [ObservableProperty]
    private string newEffectKind = "vignette";

    [ObservableProperty]
    private MediaItem? selectedElementMedia;

    // ----- Preview & Flash Risk Guard (SAFE-04) -----
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasPreviewBitmap))]
    private Bitmap? previewBitmap;

    public bool HasPreviewBitmap => PreviewBitmap is not null;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(CanStageSlide))]
    [NotifyPropertyChangedFor(nameof(IsStageBlockedByFlashGuard))]
    [NotifyCanExecuteChangedFor(nameof(StageSlideCommand))]
    private bool hasFlashWarning;

    [ObservableProperty]
    private string flashWarningReason = string.Empty;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(CanStageSlide))]
    [NotifyPropertyChangedFor(nameof(IsStageBlockedByFlashGuard))]
    [NotifyCanExecuteChangedFor(nameof(StageSlideCommand))]
    private bool flashWarningAcknowledged;

    public bool IsStageBlockedByFlashGuard => HasFlashWarning && !FlashWarningAcknowledged;

    public bool CanStageSlide => SelectedSlide is not null && (!HasFlashWarning || FlashWarningAcknowledged);

    [ObservableProperty]
    private bool canUndo;

    [ObservableProperty]
    private bool canRedo;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasStatusMessage))]
    private string? statusMessage;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasErrorMessage))]
    private string? errorMessage;

    [ObservableProperty]
    private string? errorFixArticleId;

    public bool HasStatusMessage => !string.IsNullOrWhiteSpace(StatusMessage);

    public bool HasErrorMessage => !string.IsNullOrWhiteSpace(ErrorMessage);

    public event EventHandler? SceneStaged;

    partial void OnSelectedPresentationChanged(Presentation? value)
    {
        if (suppressCallbacks || value is null)
        {
            return;
        }
        LoadPresentationIntoEditor(value);
        _ = RefreshUndoStateAsync();
        _ = RefreshPreviewAndRiskAsync();
    }

    partial void OnSelectedSlideChanged(SlideDeckEntryViewModel? value)
    {
        if (suppressCallbacks || value is null)
        {
            return;
        }
        localUndo.Clear();
        localRedo.Clear();
        RefreshUndoProperties();
        LoadSlideDocument(value.Document);
        OnPropertyChanged(nameof(CanStageSlide));
        _ = RefreshPreviewAndRiskAsync();
    }

    partial void OnSelectedTemplateChanged(Template? value)
    {
        if (!suppressCallbacks)
        {
            _ = RefreshPreviewAndRiskAsync();
        }
    }

    partial void OnBackgroundModeChanged(string value) => SyncBackgroundToDocument();
    partial void OnBackgroundColorHexChanged(string value) => SyncBackgroundToDocument();
    partial void OnSelectedBackgroundMediaChanged(MediaItem? value) => SyncBackgroundToDocument();
    partial void OnBackgroundFitChanged(string value) => SyncBackgroundToDocument();
    partial void OnBackgroundBlurRadiusChanged(double value) => SyncBackgroundToDocument();

    partial void OnTransitionKindChanged(string value) => SyncTransitionToDocument();
    partial void OnTransitionDirectionChanged(string value) => SyncTransitionToDocument();
    partial void OnTransitionEasingChanged(string value) => SyncTransitionToDocument();
    partial void OnTransitionDurationMsChanged(int value) => SyncTransitionToDocument();
    partial void OnRepeatCadenceMsChanged(int value)
    {
        if (!suppressCallbacks)
        {
            _ = RefreshPreviewAndRiskAsync();
        }
    }

    public void StartNewPresentation()
    {
        suppressCallbacks = true;
        try
        {
            PresentationId = LibraryIds.NewId();
            SelectedPresentation = null;
            PresentationName = "Custom slides";
            Slides.Clear();
            var doc = new SlideElementsDocumentModel
            {
                Background = new SlideBackgroundSpecModel { Mode = "solid" },
                Transition = new SlideTransitionSpecModel { Kind = "fade", DurationMs = 350 },
            };
            var slide = new SlideDeckEntryViewModel(LibraryIds.NewId(), 1, doc);
            Slides.Add(slide);
            SelectedSlide = slide;
            localUndo.Clear();
            localRedo.Clear();
            LoadSlideDocument(doc);
            RefreshUndoProperties();
            HasFlashWarning = false;
            FlashWarningAcknowledged = false;
            FlashWarningReason = string.Empty;
        }
        finally
        {
            suppressCallbacks = false;
        }
        _ = RefreshPreviewAndRiskAsync();
    }

    public async Task LoadAsync()
    {
        if (library is null)
        {
            return;
        }
        ClearError();
        try
        {
            var mediaList = await library.ListMediaItemsAsync();
            var tmplList = await library.ListTemplatesAsync();
            var presList = await library.ListPresentationsAsync();

            suppressCallbacks = true;
            try
            {
                AvailableMedia.Clear();
                foreach (var m in mediaList)
                {
                    AvailableMedia.Add(m);
                }
                SelectedElementMedia ??= AvailableMedia.FirstOrDefault(m => m.Kind == MediaKind.Image) ?? AvailableMedia.FirstOrDefault();

                AvailableTemplates.Clear();
                foreach (var t in tmplList)
                {
                    AvailableTemplates.Add(t);
                }

                var currentHex = LibraryIds.Hex(PresentationId);
                Presentations.Clear();
                foreach (var p in presList)
                {
                    Presentations.Add(p);
                }
                var match = Presentations.FirstOrDefault(p => LibraryIds.Same(p.Id, PresentationId));
                if (match is not null)
                {
                    SelectedPresentation = match;
                }
            }
            finally
            {
                suppressCallbacks = false;
            }

            if (SelectedPresentation is not null)
            {
                LoadPresentationIntoEditor(SelectedPresentation);
            }
            await RefreshUndoStateAsync();
            await RefreshPreviewAndRiskAsync();
        }
        catch (CoreException error)
        {
            SetCoreError(error);
        }
        catch (Exception error) when (error is TimeoutException or IOException or InvalidOperationException)
        {
            ErrorMessage = Localizer.Instance["error-not-connected"];
        }
    }

    [RelayCommand]
    public void NewPresentation() => StartNewPresentation();

    [RelayCommand]
    public void AddSlide()
    {
        var doc = new SlideElementsDocumentModel
        {
            Background = new SlideBackgroundSpecModel { Mode = "solid" },
            Transition = new SlideTransitionSpecModel { Kind = "fade", DurationMs = 350 },
        };
        var slide = new SlideDeckEntryViewModel(LibraryIds.NewId(), Slides.Count + 1, doc);
        Slides.Add(slide);
        RenumberSlides();
        SelectedSlide = slide;
    }

    [RelayCommand]
    public void DuplicateSelectedSlide()
    {
        if (SelectedSlide is null)
        {
            return;
        }
        SyncDocumentFromViewModels();
        var clonedDoc = SelectedSlide.Document.Clone();
        foreach (var el in clonedDoc.Elements)
        {
            el.Id = Guid.NewGuid().ToString("N");
        }
        var slide = new SlideDeckEntryViewModel(LibraryIds.NewId(), Slides.Count + 1, clonedDoc);
        var idx = Slides.IndexOf(SelectedSlide);
        Slides.Insert(idx + 1, slide);
        RenumberSlides();
        SelectedSlide = slide;
    }

    [RelayCommand]
    public void RemoveSelectedSlide()
    {
        if (SelectedSlide is null || Slides.Count <= 1)
        {
            return;
        }
        var idx = Slides.IndexOf(SelectedSlide);
        Slides.Remove(SelectedSlide);
        RenumberSlides();
        SelectedSlide = Slides[Math.Clamp(idx, 0, Slides.Count - 1)];
    }

    [RelayCommand]
    public void MoveSlideUp()
    {
        if (SelectedSlide is null)
        {
            return;
        }
        var idx = Slides.IndexOf(SelectedSlide);
        if (idx > 0)
        {
            Slides.Move(idx, idx - 1);
            RenumberSlides();
        }
    }

    [RelayCommand]
    public void MoveSlideDown()
    {
        if (SelectedSlide is null)
        {
            return;
        }
        var idx = Slides.IndexOf(SelectedSlide);
        if (idx >= 0 && idx < Slides.Count - 1)
        {
            Slides.Move(idx, idx + 1);
            RenumberSlides();
        }
    }

    [RelayCommand]
    public void AddTextElement()
    {
        AddElementInternal(new SlideElementModel
        {
            Id = Guid.NewGuid().ToString("N"),
            Name = $"Text {Elements.Count + 1}",
            Kind = SlideElementKinds.Text,
            X = 0.15f,
            Y = 0.28f,
            Width = 0.70f,
            Height = 0.22f,
            ZIndex = NextZIndex(),
            Text = "Slide text",
            FontFamily = "Manrope",
            FontSize = 64f,
            Weight = 700,
            Color = "#FFFFFF",
        });
    }

    [RelayCommand]
    public void AddClockElement()
    {
        AddElementInternal(new SlideElementModel
        {
            Id = Guid.NewGuid().ToString("N"),
            Name = "Clock",
            Kind = SlideElementKinds.Text,
            X = 0.72f,
            Y = 0.06f,
            Width = 0.22f,
            Height = 0.12f,
            ZIndex = NextZIndex(),
            Text = "{clock}",
            FontFamily = "JetBrains Mono",
            FontSize = 44f,
            Weight = 700,
            Color = "#FFFFFF",
            ClockShowSeconds = true,
            Clock24h = false,
        });
    }

    [RelayCommand]
    public void AddTimerElement()
    {
        AddElementInternal(new SlideElementModel
        {
            Id = Guid.NewGuid().ToString("N"),
            Name = "Timer",
            Kind = SlideElementKinds.Text,
            X = 0.35f,
            Y = 0.42f,
            Width = 0.30f,
            Height = 0.16f,
            ZIndex = NextZIndex(),
            Text = "{timer}",
            FontFamily = "JetBrains Mono",
            FontSize = 64f,
            Weight = 700,
            Color = "#FFBE2E",
            TimerDurationSecs = 300,
        });
    }

    [RelayCommand]
    public void AddShapeElement(string shapeKind = SlideShapeKinds.RoundedRect)
    {
        var validKind = SlideShapeKinds.All.Contains(shapeKind) ? shapeKind : SlideShapeKinds.RoundedRect;
        AddElementInternal(new SlideElementModel
        {
            Id = Guid.NewGuid().ToString("N"),
            Name = $"Shape {Elements.Count + 1}",
            Kind = SlideElementKinds.Shape,
            ShapeKind = validKind,
            X = 0.10f,
            Y = 0.65f,
            Width = 0.35f,
            Height = 0.20f,
            ZIndex = NextZIndex(),
            FillColor = "#2378C3E0",
            StrokeColor = "#FFFFFF",
            StrokeWidth = 2f,
            CornerRadius = 14f,
        });
    }

    [RelayCommand]
    public void AddImageElement(MediaItem? media = null)
    {
        var picked = media
            ?? SelectedElementMedia
            ?? AvailableMedia.FirstOrDefault(m => m.Kind == MediaKind.Image)
            ?? AvailableMedia.FirstOrDefault();
        var hash = !string.IsNullOrWhiteSpace(picked?.ContentHash)
            ? picked!.ContentHash
            : "0000000000000000000000000000000000000000000000000000000000000000";
        var ext = !string.IsNullOrWhiteSpace(picked?.Extension) ? picked!.Extension : "png";
        AddElementInternal(new SlideElementModel
        {
            Id = Guid.NewGuid().ToString("N"),
            Name = string.IsNullOrWhiteSpace(picked?.Name) ? $"Image {Elements.Count + 1}" : picked!.Name,
            Kind = SlideElementKinds.Image,
            X = 0.60f,
            Y = 0.15f,
            Width = 0.30f,
            Height = 0.30f,
            ZIndex = NextZIndex(),
            ContentHash = hash,
            MediaExtension = ext,
            Fit = "fit",
        });
    }

    [RelayCommand]
    public void AddVideoElement(MediaItem? media = null)
    {
        var picked = media
            ?? SelectedElementMedia
            ?? AvailableMedia.FirstOrDefault(m => m.Kind == MediaKind.Video)
            ?? AvailableMedia.FirstOrDefault();
        var hash = !string.IsNullOrWhiteSpace(picked?.ContentHash)
            ? picked!.ContentHash
            : "0000000000000000000000000000000000000000000000000000000000000000";
        var ext = !string.IsNullOrWhiteSpace(picked?.Extension) ? picked!.Extension : "mp4";
        AddElementInternal(new SlideElementModel
        {
            Id = Guid.NewGuid().ToString("N"),
            Name = string.IsNullOrWhiteSpace(picked?.Name) ? $"Video {Elements.Count + 1}" : picked!.Name,
            Kind = SlideElementKinds.Video,
            X = 0.55f,
            Y = 0.12f,
            Width = 0.38f,
            Height = 0.34f,
            ZIndex = NextZIndex(),
            ContentHash = hash,
            MediaExtension = ext,
            Fit = "fill",
        });
    }

    private void AddElementInternal(SlideElementModel model)
    {
        if (Elements.Count >= SlideElementsDocumentModel.MaxElements)
        {
            ErrorMessage = Localizer.Instance["slides-max-elements-reached"];
            return;
        }
        PushLocalUndoSnapshot();
        var item = new SlideElementItemViewModel(model, OnElementEdited) { IsSelected = true };
        Elements.Add(item);
        SelectedElement = item;
        SyncDocumentFromViewModels();
        _ = RefreshPreviewAndRiskAsync();
    }

    [RelayCommand]
    public void RemoveSelectedElement()
    {
        if (SelectedElement is null)
        {
            return;
        }
        PushLocalUndoSnapshot();
        var idx = Elements.IndexOf(SelectedElement);
        Elements.Remove(SelectedElement);
        SelectedElement = Elements.Count > 0 ? Elements[Math.Clamp(idx, 0, Elements.Count - 1)] : null;
        SyncDocumentFromViewModels();
        _ = RefreshPreviewAndRiskAsync();
    }

    [RelayCommand]
    public void SelectAllElements()
    {
        foreach (var el in Elements)
        {
            el.IsSelected = true;
        }
    }

    [RelayCommand]
    public void GroupSelected()
    {
        var targets = SelectedIndicesOrCurrent();
        if (targets.Count < 2)
        {
            return;
        }
        PushLocalUndoSnapshot();
        var groupId = "group-" + Guid.NewGuid().ToString("N")[..8];
        foreach (var idx in targets)
        {
            Elements[idx].GroupId = groupId;
        }
        SyncDocumentFromViewModels();
    }

    [RelayCommand]
    public void UngroupSelected()
    {
        var targets = SelectedIndicesOrCurrent();
        if (targets.Count == 0)
        {
            return;
        }
        PushLocalUndoSnapshot();
        foreach (var idx in targets)
        {
            Elements[idx].GroupId = null;
        }
        SyncDocumentFromViewModels();
    }

    [RelayCommand]
    public void AlignLeft() => AlignSelection(SlideAlignMode.Left);

    [RelayCommand]
    public void AlignCenter() => AlignSelection(SlideAlignMode.Center);

    [RelayCommand]
    public void AlignRight() => AlignSelection(SlideAlignMode.Right);

    [RelayCommand]
    public void AlignTop() => AlignSelection(SlideAlignMode.Top);

    [RelayCommand]
    public void AlignMiddle() => AlignSelection(SlideAlignMode.Middle);

    [RelayCommand]
    public void AlignBottom() => AlignSelection(SlideAlignMode.Bottom);

    public void AlignSelection(SlideAlignMode mode)
    {
        var indices = SelectedIndicesOrCurrent();
        if (indices.Count == 0)
        {
            return;
        }
        PushLocalUndoSnapshot();
        var models = Elements.Select(e =>
        {
            e.PushToModel();
            return e.Model;
        }).ToList();
        SlideElementsDocumentModel.AlignElements(models, indices, mode);
        foreach (var el in Elements)
        {
            el.SyncFromModel();
        }
        SyncDocumentFromViewModels();
        _ = RefreshPreviewAndRiskAsync();
    }

    [RelayCommand]
    public void BringForward()
    {
        if (SelectedElement is null)
        {
            return;
        }
        PushLocalUndoSnapshot();
        SelectedElement.ZIndex++;
        SyncDocumentFromViewModels();
        _ = RefreshPreviewAndRiskAsync();
    }

    [RelayCommand]
    public void SendBackward()
    {
        if (SelectedElement is null)
        {
            return;
        }
        PushLocalUndoSnapshot();
        SelectedElement.ZIndex--;
        SyncDocumentFromViewModels();
        _ = RefreshPreviewAndRiskAsync();
    }

    [RelayCommand]
    public void AddEffect()
    {
        if (!SlideEffectKinds.IsAllowed(NewEffectKind))
        {
            return;
        }
        if (Effects.Count >= SlideElementsDocumentModel.MaxEffectsPerChain)
        {
            ErrorMessage = Localizer.Instance["slides-max-effects-reached"];
            return;
        }
        PushLocalUndoSnapshot();
        Effects.Add(new SlideEffectItemViewModel(
            new SlideEffectEntryModel { Kind = NewEffectKind, Enabled = true, Intensity = 0.5f },
            OnElementEdited));
        SyncDocumentFromViewModels();
        _ = RefreshPreviewAndRiskAsync();
    }

    [RelayCommand]
    public void RemoveEffect(SlideEffectItemViewModel? item)
    {
        if (item is null)
        {
            return;
        }
        PushLocalUndoSnapshot();
        Effects.Remove(item);
        SyncDocumentFromViewModels();
        _ = RefreshPreviewAndRiskAsync();
    }

    [RelayCommand]
    public async Task SavePresentationAsync()
    {
        if (library is null)
        {
            return;
        }
        ClearError();
        try
        {
            var proto = BuildPresentationProto();
            await library.SavePresentationAsync(proto);
            PresentationId = proto.Id;
            await LoadAsync();
            StatusMessage = Localizer.Instance["slides-saved-status"];
        }
        catch (CoreException error)
        {
            SetCoreError(error);
        }
        catch (InvalidOperationException error)
        {
            ErrorMessage = error.Message;
        }
        catch (Exception error) when (error is TimeoutException or IOException)
        {
            ErrorMessage = Localizer.Instance["error-not-connected"];
        }
    }

    [RelayCommand]
    public async Task DeletePresentationAsync()
    {
        if (library is null || SelectedPresentation?.Id is not { Value.IsEmpty: false } id)
        {
            return;
        }
        ClearError();
        try
        {
            await library.DeletePresentationAsync(id);
            StartNewPresentation();
            await LoadAsync();
        }
        catch (CoreException error)
        {
            SetCoreError(error);
        }
    }

    [RelayCommand]
    public async Task UndoAsync()
    {
        if (localUndo.Count > 0 && SelectedSlide is not null)
        {
            localRedo.Push(CaptureCurrentSlideSnapshot());
            var previous = localUndo.Pop();
            RestoreSlideSnapshot(previous);
            RefreshUndoProperties();
            await RefreshPreviewAndRiskAsync();
            return;
        }
        if (library is null)
        {
            return;
        }
        ClearError();
        try
        {
            await library.UndoAsync(PresentationId);
            var reloaded = await library.GetPresentationAsync(PresentationId);
            if (reloaded is not null)
            {
                LoadPresentationIntoEditor(reloaded);
            }
            await RefreshUndoStateAsync();
            await RefreshPreviewAndRiskAsync();
        }
        catch (CoreException error)
        {
            SetCoreError(error);
        }
    }

    [RelayCommand]
    public async Task RedoAsync()
    {
        if (localRedo.Count > 0 && SelectedSlide is not null)
        {
            localUndo.Push(CaptureCurrentSlideSnapshot());
            var next = localRedo.Pop();
            RestoreSlideSnapshot(next);
            RefreshUndoProperties();
            await RefreshPreviewAndRiskAsync();
            return;
        }
        if (library is null)
        {
            return;
        }
        ClearError();
        try
        {
            await library.RedoAsync(PresentationId);
            var reloaded = await library.GetPresentationAsync(PresentationId);
            if (reloaded is not null)
            {
                LoadPresentationIntoEditor(reloaded);
            }
            await RefreshUndoStateAsync();
            await RefreshPreviewAndRiskAsync();
        }
        catch (CoreException error)
        {
            SetCoreError(error);
        }
    }

    [RelayCommand(CanExecute = nameof(CanStageSlide))]
    public async Task StageSlideAsync()
    {
        if (library is null || !CanStageSlide)
        {
            return;
        }
        ClearError();
        try
        {
            var scene = BuildCurrentScene();
            await library.StagePreviewSceneAsync(scene);
            StatusMessage = Localizer.Instance["editor-staged-status"];
            SceneStaged?.Invoke(this, EventArgs.Empty);
        }
        catch (CoreException error)
        {
            SetCoreError(error);
        }
        catch (Exception error) when (error is TimeoutException or IOException or InvalidOperationException)
        {
            ErrorMessage = Localizer.Instance["error-not-connected"];
        }
    }

    private List<int> SelectedIndicesOrCurrent()
    {
        var checkedIndices = new List<int>();
        for (var i = 0; i < Elements.Count; i++)
        {
            if (Elements[i].IsSelected)
            {
                checkedIndices.Add(i);
            }
        }
        if (checkedIndices.Count > 0)
        {
            return checkedIndices;
        }
        if (SelectedElement is not null)
        {
            var idx = Elements.IndexOf(SelectedElement);
            if (idx >= 0)
            {
                // If element belongs to a group, align all unlocked group members together.
                if (!string.IsNullOrWhiteSpace(SelectedElement.GroupId))
                {
                    for (var i = 0; i < Elements.Count; i++)
                    {
                        if (Elements[i].GroupId == SelectedElement.GroupId)
                        {
                            checkedIndices.Add(i);
                        }
                    }
                    return checkedIndices;
                }
                return [idx];
            }
        }
        return [];
    }

    private int NextZIndex() => Elements.Count == 0 ? 0 : Elements.Max(e => e.ZIndex) + 1;

    private void PushLocalUndoSnapshot()
    {
        localUndo.Push(CaptureCurrentSlideSnapshot());
        localRedo.Clear();
        RefreshUndoProperties();
    }

    private string CaptureCurrentSlideSnapshot()
    {
        SyncDocumentFromViewModels();
        return SelectedSlide?.Document.ToJson() ?? "{}";
    }

    private void RestoreSlideSnapshot(string json)
    {
        if (SelectedSlide is null)
        {
            return;
        }
        var doc = SlideElementsDocumentModel.FromJson(json);
        SelectedSlide.Document = doc;
        LoadSlideDocument(doc);
        SelectedSlide.RefreshSummary();
    }

    private void RefreshUndoProperties()
    {
        CanUndo = localUndo.Count > 0 || coreCanUndo;
        CanRedo = localRedo.Count > 0 || coreCanRedo;
    }

    private async Task RefreshUndoStateAsync()
    {
        if (library is null)
        {
            return;
        }
        try
        {
            var state = await library.GetUndoStateAsync(PresentationId);
            coreCanUndo = state.CanUndo;
            coreCanRedo = state.CanRedo;
            RefreshUndoProperties();
        }
        catch (Exception)
        {
        }
    }

    private void RenumberSlides()
    {
        for (var i = 0; i < Slides.Count; i++)
        {
            Slides[i].Number = i + 1;
            Slides[i].RefreshSummary();
        }
    }

    private void LoadPresentationIntoEditor(Presentation pres)
    {
        suppressCallbacks = true;
        try
        {
            PresentationId = pres.Id is { Value.IsEmpty: false } ? pres.Id : LibraryIds.NewId();
            PresentationName = string.IsNullOrWhiteSpace(pres.Name) ? "Custom slides" : pres.Name;
            SelectedTemplate = pres.TemplateId is { Value.IsEmpty: false } tid
                ? AvailableTemplates.FirstOrDefault(t => LibraryIds.Same(t.Id, tid))
                : null;
            Slides.Clear();
            var idx = 1;
            foreach (var s in pres.Slides)
            {
                var doc = SlideElementsDocumentModel.FromJson(s.ElementsJson);
                if (doc.Elements.Count == 0 && !string.IsNullOrWhiteSpace(s.Text))
                {
                    doc.Elements.Add(new SlideElementModel
                    {
                        Id = Guid.NewGuid().ToString("N"),
                        Name = "Text 1",
                        Kind = SlideElementKinds.Text,
                        Text = s.Text,
                    });
                }
                Slides.Add(new SlideDeckEntryViewModel(
                    s.Id is { Value.IsEmpty: false } ? s.Id : LibraryIds.NewId(),
                    idx++,
                    doc));
            }
            if (Slides.Count == 0)
            {
                Slides.Add(new SlideDeckEntryViewModel(LibraryIds.NewId(), 1, new SlideElementsDocumentModel()));
            }
            SelectedSlide = Slides[0];
            localUndo.Clear();
            localRedo.Clear();
            LoadSlideDocument(SelectedSlide.Document);
            RefreshUndoProperties();
        }
        finally
        {
            suppressCallbacks = false;
        }
    }

    private void LoadSlideDocument(SlideElementsDocumentModel doc)
    {
        suppressCallbacks = true;
        try
        {
            Elements.Clear();
            foreach (var el in doc.Elements)
            {
                Elements.Add(new SlideElementItemViewModel(el.Clone(), OnElementEdited));
            }
            SelectedElement = Elements.FirstOrDefault();

            Effects.Clear();
            foreach (var ef in doc.Effects)
            {
                Effects.Add(new SlideEffectItemViewModel(ef.Clone(), OnElementEdited));
            }

            var bg = doc.Background ?? new SlideBackgroundSpecModel { Mode = "solid" };
            BackgroundMode = BackgroundModes.Contains(bg.Mode) ? bg.Mode : "solid";
            BackgroundFit = FitOptions.Contains(bg.Fit) ? bg.Fit : "fill";
            BackgroundBlurRadius = Math.Round(bg.BlurRadius, 1);
            if (!string.IsNullOrWhiteSpace(bg.MediaId))
            {
                SelectedBackgroundMedia = AvailableMedia.FirstOrDefault(m =>
                    string.Equals(LibraryIds.Hex(m.Id), bg.MediaId, StringComparison.OrdinalIgnoreCase)
                    || string.Equals(m.ContentHash, bg.ContentHash, StringComparison.OrdinalIgnoreCase));
            }
            else if (!string.IsNullOrWhiteSpace(bg.ContentHash))
            {
                SelectedBackgroundMedia = AvailableMedia.FirstOrDefault(m =>
                    string.Equals(m.ContentHash, bg.ContentHash, StringComparison.OrdinalIgnoreCase));
            }
            else
            {
                SelectedBackgroundMedia = null;
            }
            if (bg.SolidColor is { Length: >= 3 } rgba)
            {
                BackgroundColorHex = $"#{ToByte(rgba[0]):X2}{ToByte(rgba[1]):X2}{ToByte(rgba[2]):X2}";
            }

            var tr = doc.Transition ?? new SlideTransitionSpecModel { Kind = "fade", DurationMs = 350 };
            TransitionKind = TransitionKinds.Contains(tr.Kind) ? tr.Kind : "fade";
            TransitionDirection = TransitionDirections.Contains(tr.Direction) ? tr.Direction : "left";
            TransitionEasing = TransitionEasings.Contains(tr.Easing) ? tr.Easing : "ease_in_out";
            TransitionDurationMs = (int)tr.DurationMs;
        }
        finally
        {
            suppressCallbacks = false;
        }
    }

    private void SyncBackgroundToDocument()
    {
        if (suppressCallbacks)
        {
            return;
        }
        SyncDocumentFromViewModels();
        _ = RefreshPreviewAndRiskAsync();
    }

    private void SyncTransitionToDocument()
    {
        if (suppressCallbacks)
        {
            return;
        }
        SyncDocumentFromViewModels();
        _ = RefreshPreviewAndRiskAsync();
    }

    private void OnElementEdited()
    {
        if (suppressCallbacks)
        {
            return;
        }
        SyncDocumentFromViewModels();
        _ = RefreshPreviewAndRiskAsync();
    }

    private void SyncDocumentFromViewModels()
    {
        if (SelectedSlide is null)
        {
            return;
        }
        var doc = SelectedSlide.Document;
        doc.Elements.Clear();
        foreach (var item in Elements)
        {
            item.PushToModel();
            doc.Elements.Add(item.Model.Clone());
        }
        doc.Effects.Clear();
        foreach (var ef in Effects)
        {
            doc.Effects.Add(ef.Model.Clone());
        }
        doc.Background = BuildBackgroundModel();
        doc.Transition = new SlideTransitionSpecModel
        {
            Kind = TransitionKind,
            Direction = TransitionDirection,
            Easing = TransitionEasing,
            DurationMs = (uint)Math.Clamp(TransitionDurationMs, 0, 10_000),
        };
        SelectedSlide.RefreshSummary();
    }

    private SlideBackgroundSpecModel BuildBackgroundModel()
    {
        var rgb = ParseHexRgb(BackgroundColorHex);
        var model = new SlideBackgroundSpecModel
        {
            Mode = BackgroundMode,
            Fit = BackgroundFit,
            BlurRadius = (float)Math.Clamp(BackgroundBlurRadius, 0, 64),
            Opacity = 1.0f,
            SolidColor = [rgb.r, rgb.g, rgb.b, 1.0f],
        };
        if (BackgroundMode is "image" or "video" && SelectedBackgroundMedia is { } media)
        {
            model.MediaId = LibraryIds.Hex(media.Id);
            model.ContentHash = media.ContentHash;
            model.MediaExtension = media.Extension;
            model.MediaKind = media.Kind == MediaKind.Video ? "video" : "image";
        }
        return model;
    }

    public Presentation BuildPresentationProto()
    {
        SyncDocumentFromViewModels();
        var pres = new Presentation
        {
            Id = PresentationId,
            Name = string.IsNullOrWhiteSpace(PresentationName) ? "Custom slides" : PresentationName.Trim(),
        };
        if (SelectedTemplate?.Id is { Value.IsEmpty: false } tid)
        {
            pres.TemplateId = tid;
        }
        foreach (var slide in Slides)
        {
            pres.Slides.Add(slide.ToProto());
        }
        return pres;
    }

    public Scene BuildCurrentScene(uint width = 1920, uint height = 1080)
    {
        SyncDocumentFromViewModels();
        var res = new Resolution { Width = width, Height = height };
        var output = new OutputScene
        {
            OutputIndex = 0,
            Role = OutputRole.Main,
            Resolution = res,
            Transition = new SlideTransitionSpecModel
            {
                Kind = TransitionKind,
                Direction = TransitionDirection,
                Easing = TransitionEasing,
                DurationMs = (uint)Math.Clamp(TransitionDurationMs, 0, 10_000),
            }.ToProto(),
        };

        var bg = BuildBackgroundModel();
        if (bg.Mode != "none")
        {
            var mediaLayer = new MediaLayerSpec { Fit = bg.Fit == "fill" ? MediaFitMode.Fill : bg.Fit == "stretch" ? MediaFitMode.Stretch : MediaFitMode.Fit, BlurRadius = bg.BlurRadius };
            if (bg.Mode is "image" or "video" && !string.IsNullOrWhiteSpace(bg.ContentHash))
            {
                mediaLayer.ContentHash = bg.ContentHash;
                mediaLayer.MediaExtension = bg.MediaExtension;
                mediaLayer.MediaKind = bg.MediaKind;
            }
            else
            {
                var (r, g, b) = ParseHexRgb(BackgroundColorHex);
                mediaLayer.SolidColor = new Color { Red = r, Green = g, Blue = b, Alpha = 1f };
            }
            output.Layers.Add(new LayerSpec
            {
                Kind = LayerKind.Media,
                Enabled = true,
                Media = mediaLayer,
            });
        }

        var slideLayer = new SlideLayerSpec
        {
            ElementsJson = SelectedSlide?.Document.ToJson() ?? string.Empty,
            StyleJson = SelectedTemplate is not null ? TemplateStyleModel.FromProto(SelectedTemplate).ToJson() : string.Empty,
        };
        foreach (var ef in Effects.Where(e => e.Enabled && SlideEffectKinds.IsAllowed(e.Kind)))
        {
            slideLayer.Effects.Add(new EffectSpec
            {
                Kind = SlideEffectKinds.ToProtoKind(ef.Kind),
                Enabled = ef.Enabled,
                Intensity = (float)Math.Clamp(ef.Intensity, 0, 1),
            });
        }

        output.Layers.Add(new LayerSpec
        {
            Kind = LayerKind.Slide,
            Enabled = true,
            Slide = slideLayer,
        });

        return new Scene
        {
            Resolution = res,
            Outputs = { output },
        };
    }

    public async Task RefreshPreviewAndRiskAsync()
    {
        if (library is null || SelectedSlide is null)
        {
            return;
        }
        var generation = Interlocked.Increment(ref previewGeneration);
        try
        {
            var scene = BuildCurrentScene();
            var thumb = await library.RenderSceneThumbnailAsync(scene, 0, 480, 270);
            if (Volatile.Read(ref previewGeneration) != generation)
            {
                return;
            }
            PreviewBitmap = MediaCardViewModel.DecodePng(thumb.PngBytes.ToByteArray());

            var req = new AnalyzeFlashRisk
            {
                Transition = new SlideTransitionSpecModel
                {
                    Kind = TransitionKind,
                    Direction = TransitionDirection,
                    Easing = TransitionEasing,
                    DurationMs = (uint)Math.Clamp(TransitionDurationMs, 0, 10_000),
                }.ToProto(),
                RepeatCadenceMs = (uint)Math.Max(0, RepeatCadenceMs),
                ToScene = scene,
            };
            if (BackgroundMode is "image" or "video" && SelectedBackgroundMedia?.Id is { Value.IsEmpty: false } bgMediaId)
            {
                req.MediaId = bgMediaId;
            }
            var risk = await library.AnalyzeFlashRiskAsync(req);
            if (Volatile.Read(ref previewGeneration) != generation)
            {
                return;
            }
            var anyFlash = thumb.FlashWarning || risk.FlashWarning;
            if (anyFlash)
            {
                if (!HasFlashWarning)
                {
                    FlashWarningAcknowledged = false;
                }
                HasFlashWarning = true;
                FlashWarningReason = !string.IsNullOrWhiteSpace(risk.Reason)
                    ? risk.Reason
                    : Localizer.Instance["preview-flash-warning"];
            }
            else
            {
                HasFlashWarning = false;
                FlashWarningAcknowledged = false;
                FlashWarningReason = string.Empty;
            }
            StageSlideCommand.NotifyCanExecuteChanged();
        }
        catch (Exception)
        {
        }
    }

    private static (float r, float g, float b) ParseHexRgb(string hex)
    {
        var norm = TemplateColorHelper.NormalizeHexOrDefault(hex, "#0F1724").TrimStart('#');
        if (norm.Length >= 6
            && byte.TryParse(norm.AsSpan(0, 2), System.Globalization.NumberStyles.HexNumber, null, out var r)
            && byte.TryParse(norm.AsSpan(2, 2), System.Globalization.NumberStyles.HexNumber, null, out var g)
            && byte.TryParse(norm.AsSpan(4, 2), System.Globalization.NumberStyles.HexNumber, null, out var b))
        {
            return (r / 255f, g / 255f, b / 255f);
        }
        return (0.06f, 0.09f, 0.14f);
    }

    private static byte ToByte(float f) => (byte)Math.Clamp((int)Math.Round(f * 255f), 0, 255);

    private void ClearError()
    {
        ErrorMessage = null;
        ErrorFixArticleId = null;
    }

    private void SetCoreError(CoreException error)
    {
        ErrorMessage = Localizer.Instance[error.MessageKey];
        ErrorFixArticleId = string.IsNullOrWhiteSpace(error.FixArticleId) ? null : error.FixArticleId;
    }
}
