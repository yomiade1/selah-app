using System.Collections.ObjectModel;
using System.Text.Json;
using Avalonia.Media.Imaging;
using Avalonia.Threading;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Selah.Contracts.V1;
using Selah.UI.Services;

namespace Selah.UI.ViewModels;

/// <summary>Selectable option in ComboBoxes representing an aspect ratio or named template.</summary>
public sealed record NamedOption(string Value, string Label)
{
    public override string ToString() => Label;
}

/// <summary>
/// Full M3 Template Editor ViewModel: role slots (body, reference, title, footer, chords, translation),
/// 7 bundled OFL fonts, weight 100..900, outline/shadow/background box, aspect variants (16:9, 32:9, 4:3, 9:16),
/// safe-area margins, 4 starter looks, song/scripture default policy assignment, output assignments,
/// live offscreen scene preview, and per-document undo/redo.
/// </summary>
public sealed partial class TemplateEditorViewModel : ObservableObject
{
    private readonly ILibraryService? library;
    private TemplateStyleModel style = TemplateStyleModel.StarterSanctuary();
    private bool suppressSync;
    private int previewGeneration;

    public TemplateEditorViewModel(ILibraryService? library)
    {
        this.library = library;
        TemplateId = LibraryIds.NewId();
        LoadFromStyleModel(TemplateStyleModel.StarterSanctuary());
        if (library is not null)
        {
            library.UndoStateChanged += (_, state) =>
            {
                if (state.DocId is null || LibraryIds.Same(state.DocId, TemplateId))
                {
                    Dispatcher.UIThread.Post(() =>
                    {
                        var undo = state.Document ?? state.Global;
                        if (undo is not null)
                        {
                            CanUndo = undo.CanUndo;
                            CanRedo = undo.CanRedo;
                        }
                    });
                }
            };
        }
    }

    public Id TemplateId { get; private set; }

    public IReadOnlyList<string> FontFamilies => BundledFonts.Families;

    public IReadOnlyList<string> RoleKeys { get; } = ["body", "reference", "title", "footer", "chords", "translation"];

    public IReadOnlyList<string> AlignHOptions { get; } = ["left", "center", "right", "justify"];

    public IReadOnlyList<string> AlignVOptions { get; } = ["top", "middle", "bottom"];

    public IReadOnlyList<string> AspectOptions { get; } = ["16:9", "32:9", "4:3", "9:16"];

    public ObservableCollection<Template> Templates { get; } = [];

    public ObservableCollection<AspectVariantStyleModel> Variants { get; } = [];

    public ObservableCollection<OutputConfig> OutputConfigs { get; } = [];

    [ObservableProperty]
    private Template? selectedTemplate;

    [ObservableProperty]
    private string templateName = "Sanctuary Lyrics";

    [ObservableProperty]
    private string selectedRoleKey = "body";

    [ObservableProperty]
    private AspectVariantStyleModel? selectedVariant;

    [ObservableProperty]
    private OutputConfig? selectedOutputConfig;

    [ObservableProperty]
    private string previewAspect = "16:9";

    // ----- Active Role Style Fields -----
    [ObservableProperty]
    private string roleFontFamily = "Manrope";

    [ObservableProperty]
    private double roleSize = 74;

    [ObservableProperty]
    private double roleMinSize = 32;

    [ObservableProperty]
    private int roleWeight = 700;

    [ObservableProperty]
    private string roleColorHex = "#FFFFFF";

    [ObservableProperty]
    private string roleAlignH = "center";

    [ObservableProperty]
    private string roleAlignV = "middle";

    [ObservableProperty]
    private double roleLineHeight = 1.25;

    [ObservableProperty]
    private double roleLetterSpacing = 0.5;

    [ObservableProperty]
    private int roleMaxLines = 4;

    [ObservableProperty]
    private bool roleVisible = true;

    [ObservableProperty]
    private bool hasOutline;

    [ObservableProperty]
    private string outlineColorHex = "#0A0E18D2";

    [ObservableProperty]
    private double outlineWidth = 2.5;

    [ObservableProperty]
    private bool hasShadow;

    [ObservableProperty]
    private string shadowColorHex = "#000000B9";

    [ObservableProperty]
    private double shadowOffsetX;

    [ObservableProperty]
    private double shadowOffsetY = 4;

    [ObservableProperty]
    private double shadowBlur = 10;

    [ObservableProperty]
    private bool hasBackgroundBox;

    [ObservableProperty]
    private string boxColorHex = "#162E51E6";

    [ObservableProperty]
    private double boxPaddingX = 24;

    [ObservableProperty]
    private double boxPaddingY = 14;

    [ObservableProperty]
    private double boxRadius = 10;

    [ObservableProperty]
    private bool boxFullWidth;

    [ObservableProperty]
    private string boxAccentColorHex = string.Empty;

    [ObservableProperty]
    private double boxAccentWidth;

    // ----- Safe Area & Variant overrides -----
    [ObservableProperty]
    private double safeLeft = 0.08;

    [ObservableProperty]
    private double safeTop = 0.08;

    [ObservableProperty]
    private double safeRight = 0.08;

    [ObservableProperty]
    private double safeBottom = 0.08;

    [ObservableProperty]
    private int variantMaxLines = 4;

    // ----- Assignment & Policy Defaults -----
    [ObservableProperty]
    private string defaultSongTemplateIdHex = string.Empty;

    [ObservableProperty]
    private string defaultScriptureTemplateIdHex = string.Empty;

    [ObservableProperty]
    private string defaultSongTemplateName = string.Empty;

    [ObservableProperty]
    private string defaultScriptureTemplateName = string.Empty;

    // ----- Live Preview -----
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasPreviewBitmap))]
    private Bitmap? previewBitmap;

    [ObservableProperty]
    private bool hasFlashWarning;

    public bool HasPreviewBitmap => PreviewBitmap is not null;

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

    public event EventHandler? TemplatesChanged;

    partial void OnSelectedTemplateChanged(Template? value)
    {
        if (suppressSync || value is null)
        {
            return;
        }
        TemplateId = value.Id is { Value.IsEmpty: false } ? value.Id : LibraryIds.NewId();
        LoadFromStyleModel(TemplateStyleModel.FromProto(value));
        _ = RefreshUndoStateAsync();
        _ = RefreshPreviewAsync();
    }

    partial void OnSelectedRoleKeyChanged(string value)
    {
        if (suppressSync)
        {
            return;
        }
        LoadRoleFields(style.Roles.GetRole(value));
    }

    partial void OnSelectedVariantChanged(AspectVariantStyleModel? value)
    {
        if (suppressSync || value is null)
        {
            return;
        }
        suppressSync = true;
        try
        {
            VariantMaxLines = (int)(value.MaxLines ?? style.Roles.Body.MaxLines ?? 4);
            var sa = value.SafeArea ?? style.SafeArea;
            SafeLeft = Math.Round(sa.Left, 3);
            SafeTop = Math.Round(sa.Top, 3);
            SafeRight = Math.Round(sa.Right, 3);
            SafeBottom = Math.Round(sa.Bottom, 3);
            PreviewAspect = AspectOptions.Contains(value.AspectRatio) ? value.AspectRatio : "16:9";
        }
        finally
        {
            suppressSync = false;
        }
        _ = RefreshPreviewAsync();
    }

    partial void OnPreviewAspectChanged(string value)
    {
        if (!suppressSync)
        {
            _ = RefreshPreviewAsync();
        }
    }

    partial void OnTemplateNameChanged(string value)
    {
        if (!suppressSync)
        {
            style.Name = value;
            _ = RefreshPreviewAsync();
        }
    }

    partial void OnRoleFontFamilyChanged(string value) => PushRoleChange();
    partial void OnRoleSizeChanged(double value) => PushRoleChange();
    partial void OnRoleMinSizeChanged(double value) => PushRoleChange();
    partial void OnRoleWeightChanged(int value) => PushRoleChange();
    partial void OnRoleColorHexChanged(string value) => PushRoleChange();
    partial void OnRoleAlignHChanged(string value) => PushRoleChange();
    partial void OnRoleAlignVChanged(string value) => PushRoleChange();
    partial void OnRoleLineHeightChanged(double value) => PushRoleChange();
    partial void OnRoleLetterSpacingChanged(double value) => PushRoleChange();
    partial void OnRoleMaxLinesChanged(int value) => PushRoleChange();
    partial void OnRoleVisibleChanged(bool value) => PushRoleChange();
    partial void OnHasOutlineChanged(bool value) => PushRoleChange();
    partial void OnOutlineColorHexChanged(string value) => PushRoleChange();
    partial void OnOutlineWidthChanged(double value) => PushRoleChange();
    partial void OnHasShadowChanged(bool value) => PushRoleChange();
    partial void OnShadowColorHexChanged(string value) => PushRoleChange();
    partial void OnShadowOffsetXChanged(double value) => PushRoleChange();
    partial void OnShadowOffsetYChanged(double value) => PushRoleChange();
    partial void OnShadowBlurChanged(double value) => PushRoleChange();
    partial void OnHasBackgroundBoxChanged(bool value) => PushRoleChange();
    partial void OnBoxColorHexChanged(string value) => PushRoleChange();
    partial void OnBoxPaddingXChanged(double value) => PushRoleChange();
    partial void OnBoxPaddingYChanged(double value) => PushRoleChange();
    partial void OnBoxRadiusChanged(double value) => PushRoleChange();
    partial void OnBoxFullWidthChanged(bool value) => PushRoleChange();
    partial void OnBoxAccentColorHexChanged(string value) => PushRoleChange();
    partial void OnBoxAccentWidthChanged(double value) => PushRoleChange();

    partial void OnSafeLeftChanged(double value) => PushSafeAreaChange();
    partial void OnSafeTopChanged(double value) => PushSafeAreaChange();
    partial void OnSafeRightChanged(double value) => PushSafeAreaChange();
    partial void OnSafeBottomChanged(double value) => PushSafeAreaChange();

    partial void OnVariantMaxLinesChanged(int value)
    {
        if (suppressSync || SelectedVariant is null)
        {
            return;
        }
        SelectedVariant.MaxLines = (uint)Math.Clamp(value, 1, 16);
        _ = RefreshPreviewAsync();
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
            var currentHex = LibraryIds.Hex(TemplateId);
            var list = await library.ListTemplatesAsync();
            suppressSync = true;
            try
            {
                Templates.Clear();
                foreach (var t in list)
                {
                    Templates.Add(t);
                }
                var match = !string.IsNullOrEmpty(currentHex)
                    ? Templates.FirstOrDefault(t => string.Equals(LibraryIds.Hex(t.Id), currentHex, StringComparison.OrdinalIgnoreCase))
                    : null;
                SelectedTemplate = match ?? Templates.FirstOrDefault();
            }
            finally
            {
                suppressSync = false;
            }
            if (SelectedTemplate is not null)
            {
                TemplateId = SelectedTemplate.Id;
                LoadFromStyleModel(TemplateStyleModel.FromProto(SelectedTemplate));
            }
            await RefreshDefaultsAndOutputsAsync();
            await RefreshUndoStateAsync();
            await RefreshPreviewAsync();
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

    private async Task RefreshDefaultsAndOutputsAsync()
    {
        if (library is null)
        {
            return;
        }
        DefaultSongTemplateIdHex = ParsePolicyHex(await library.GetPolicySettingAsync("templates.default.song"));
        DefaultScriptureTemplateIdHex = ParsePolicyHex(await library.GetPolicySettingAsync("templates.default.scripture"));
        UpdateDefaultNames();

        var cfgs = await library.GetOutputConfigsAsync();
        OutputConfigs.Clear();
        foreach (var cfg in cfgs)
        {
            OutputConfigs.Add(cfg);
        }
        SelectedOutputConfig = OutputConfigs.FirstOrDefault();
    }

    private void UpdateDefaultNames()
    {
        string ResolveName(string hex)
        {
            if (string.IsNullOrEmpty(hex))
            {
                return Localizer.Instance["templates-default-none"];
            }
            var found = Templates.FirstOrDefault(t => string.Equals(LibraryIds.Hex(t.Id), hex, StringComparison.OrdinalIgnoreCase));
            return found?.Name ?? Localizer.Instance["templates-default-none"];
        }
        DefaultSongTemplateName = ResolveName(DefaultSongTemplateIdHex);
        DefaultScriptureTemplateName = ResolveName(DefaultScriptureTemplateIdHex);
    }

    private static string ParsePolicyHex(GetPolicySettingReply reply)
    {
        if (!reply.Found || string.IsNullOrWhiteSpace(reply.Setting?.ValueJson))
        {
            return string.Empty;
        }
        try
        {
            using var doc = JsonDocument.Parse(reply.Setting.ValueJson);
            if (doc.RootElement.ValueKind == JsonValueKind.String)
            {
                return doc.RootElement.GetString() ?? string.Empty;
            }
        }
        catch (JsonException)
        {
        }
        return reply.Setting.ValueJson.Trim().Trim('"');
    }

    [RelayCommand]
    public void NewTemplate()
    {
        TemplateId = LibraryIds.NewId();
        suppressSync = true;
        try
        {
            SelectedTemplate = null;
        }
        finally
        {
            suppressSync = false;
        }
        var fresh = TemplateStyleModel.StarterSanctuary();
        fresh.Name = Localizer.Instance["templates-new-name"];
        LoadFromStyleModel(fresh);
        CanUndo = false;
        CanRedo = false;
        StatusMessage = null;
        _ = RefreshPreviewAsync();
    }

    [RelayCommand]
    public void DuplicateTemplate()
    {
        var copy = BuildStyleModel();
        TemplateId = LibraryIds.NewId();
        suppressSync = true;
        try
        {
            SelectedTemplate = null;
        }
        finally
        {
            suppressSync = false;
        }
        copy.Name = $"{copy.Name} Copy";
        LoadFromStyleModel(copy);
        _ = RefreshPreviewAsync();
    }

    [RelayCommand]
    public void ApplyLook(string lookKey)
    {
        var look = TemplateStyleModel.StarterByKey(lookKey);
        if (SelectedTemplate is not null)
        {
            // Keep editing current saved template slot unless blank
            if (!string.IsNullOrWhiteSpace(TemplateName) && TemplateName != Localizer.Instance["templates-new-name"])
            {
                look.Name = TemplateName;
            }
        }
        LoadFromStyleModel(look);
        StatusMessage = Localizer.Instance["templates-look-applied"];
        _ = RefreshPreviewAsync();
    }

    [RelayCommand]
    public void AddVariant(string aspect)
    {
        var key = AspectOptions.Contains(aspect) ? aspect : "16:9";
        var existing = Variants.FirstOrDefault(v => string.Equals(v.AspectRatio, key, StringComparison.OrdinalIgnoreCase));
        if (existing is not null)
        {
            SelectedVariant = existing;
            return;
        }
        var added = AspectVariantStyleModel.CreateForAspect(key);
        Variants.Add(added);
        style.Variants = Variants.ToList();
        SelectedVariant = added;
        _ = RefreshPreviewAsync();
    }

    [RelayCommand]
    public void RemoveSelectedVariant()
    {
        if (SelectedVariant is null || Variants.Count <= 1)
        {
            return;
        }
        var idx = Variants.IndexOf(SelectedVariant);
        Variants.Remove(SelectedVariant);
        style.Variants = Variants.ToList();
        SelectedVariant = Variants[Math.Clamp(idx, 0, Variants.Count - 1)];
        _ = RefreshPreviewAsync();
    }

    [RelayCommand]
    public async Task SaveTemplateAsync()
    {
        if (library is null)
        {
            return;
        }
        ClearError();
        try
        {
            var proto = BuildTemplateProto();
            await library.SaveTemplateAsync(proto);
            TemplateId = proto.Id;
            await LoadAsync();
            StatusMessage = Localizer.Instance["templates-saved-status"];
            TemplatesChanged?.Invoke(this, EventArgs.Empty);
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
    public async Task DeleteSelectedTemplateAsync()
    {
        if (library is null || SelectedTemplate?.Id is not { Value.IsEmpty: false } id)
        {
            return;
        }
        ClearError();
        try
        {
            await library.DeleteTemplateAsync(id);
            TemplateId = LibraryIds.NewId();
            await LoadAsync();
            TemplatesChanged?.Invoke(this, EventArgs.Empty);
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
    public async Task SetAsDefaultSongTemplateAsync()
    {
        await AssignDefaultPolicyAsync("templates.default.song", isSong: true);
    }

    [RelayCommand]
    public async Task SetAsDefaultScriptureTemplateAsync()
    {
        await AssignDefaultPolicyAsync("templates.default.scripture", isSong: false);
    }

    private async Task AssignDefaultPolicyAsync(string key, bool isSong)
    {
        if (library is null)
        {
            return;
        }
        ClearError();
        try
        {
            // Ensure current template is persisted before assigning its ID to policy.
            var proto = BuildTemplateProto();
            var exists = Templates.Any(t => LibraryIds.Same(t.Id, proto.Id));
            if (!exists)
            {
                await library.SaveTemplateAsync(proto);
                TemplateId = proto.Id;
                await LoadAsync();
            }
            var hex = LibraryIds.Hex(TemplateId);
            await library.SetPolicySettingAsync(key, JsonSerializer.Serialize(hex), PolicyLevel.User, locked: false);
            if (isSong)
            {
                DefaultSongTemplateIdHex = hex;
            }
            else
            {
                DefaultScriptureTemplateIdHex = hex;
            }
            UpdateDefaultNames();
            StatusMessage = Localizer.Instance["templates-default-assigned"];
            TemplatesChanged?.Invoke(this, EventArgs.Empty);
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
    public async Task AssignToSelectedOutputAsync()
    {
        if (library is null || SelectedOutputConfig is null)
        {
            return;
        }
        ClearError();
        try
        {
            var proto = BuildTemplateProto();
            if (!Templates.Any(t => LibraryIds.Same(t.Id, proto.Id)))
            {
                await library.SaveTemplateAsync(proto);
                TemplateId = proto.Id;
                await LoadAsync();
            }
            var targetIndex = SelectedOutputConfig.OutputIndex;
            var updated = new List<OutputConfig>(OutputConfigs.Count);
            foreach (var cfg in OutputConfigs)
            {
                var copy = cfg.Clone();
                if (copy.OutputIndex == targetIndex)
                {
                    copy.TemplateId = TemplateId.Clone();
                }
                updated.Add(copy);
            }
            await library.SetOutputConfigsAsync(updated);
            await RefreshDefaultsAndOutputsAsync();
            StatusMessage = Localizer.Instance["templates-output-assigned"];
            TemplatesChanged?.Invoke(this, EventArgs.Empty);
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
    public async Task UndoAsync()
    {
        if (library is null)
        {
            return;
        }
        ClearError();
        try
        {
            await library.UndoAsync(TemplateId);
            var reloaded = await library.GetTemplateAsync(TemplateId);
            await LoadAsync();
            if (reloaded is not null)
            {
                TemplateId = reloaded.Id;
                LoadFromStyleModel(TemplateStyleModel.FromProto(reloaded));
            }
            await RefreshUndoStateAsync();
            await RefreshPreviewAsync();
            TemplatesChanged?.Invoke(this, EventArgs.Empty);
        }
        catch (CoreException error)
        {
            SetCoreError(error);
        }
    }

    [RelayCommand]
    public async Task RedoAsync()
    {
        if (library is null)
        {
            return;
        }
        ClearError();
        try
        {
            await library.RedoAsync(TemplateId);
            var reloaded = await library.GetTemplateAsync(TemplateId);
            await LoadAsync();
            if (reloaded is not null)
            {
                TemplateId = reloaded.Id;
                LoadFromStyleModel(TemplateStyleModel.FromProto(reloaded));
            }
            await RefreshUndoStateAsync();
            await RefreshPreviewAsync();
            TemplatesChanged?.Invoke(this, EventArgs.Empty);
        }
        catch (CoreException error)
        {
            SetCoreError(error);
        }
    }

    private async Task RefreshUndoStateAsync()
    {
        if (library is null)
        {
            return;
        }
        try
        {
            var undo = await library.GetUndoStateAsync(TemplateId);
            CanUndo = undo.CanUndo;
            CanRedo = undo.CanRedo;
        }
        catch (Exception)
        {
        }
    }

    public TemplateStyleModel BuildStyleModel()
    {
        style.Name = string.IsNullOrWhiteSpace(TemplateName) ? "Untitled" : TemplateName.Trim();
        style.Variants = Variants.Select(v => v.Clone()).ToList();
        style.Normalize();
        return style;
    }

    public Template BuildTemplateProto() => BuildStyleModel().ToProto(TemplateId);

    private void LoadFromStyleModel(TemplateStyleModel model)
    {
        style = model;
        style.Normalize();
        suppressSync = true;
        try
        {
            TemplateName = style.Name;
            Variants.Clear();
            foreach (var v in style.Variants)
            {
                Variants.Add(v.Clone());
            }
            if (Variants.Count == 0)
            {
                Variants.Add(AspectVariantStyleModel.CreateForAspect("16:9"));
            }
            SelectedVariant = Variants.FirstOrDefault();
            VariantMaxLines = (int)(SelectedVariant?.MaxLines ?? style.Roles.Body.MaxLines ?? 4);
            var sa = SelectedVariant?.SafeArea ?? style.SafeArea;
            SafeLeft = Math.Round(sa.Left, 3);
            SafeTop = Math.Round(sa.Top, 3);
            SafeRight = Math.Round(sa.Right, 3);
            SafeBottom = Math.Round(sa.Bottom, 3);
            LoadRoleFields(style.Roles.GetRole(SelectedRoleKey));
        }
        finally
        {
            suppressSync = false;
        }
    }

    private void LoadRoleFields(SlotTextStyleModel role)
    {
        suppressSync = true;
        try
        {
            RoleFontFamily = BundledFonts.Canonical(role.FontFamily);
            RoleSize = Math.Round(role.Size, 1);
            RoleMinSize = Math.Round(role.MinSize, 1);
            RoleWeight = role.Weight;
            RoleColorHex = role.Color;
            RoleAlignH = role.AlignH;
            RoleAlignV = role.AlignV;
            RoleLineHeight = Math.Round(role.LineHeight, 2);
            RoleLetterSpacing = Math.Round(role.LetterSpacing, 1);
            RoleMaxLines = (int)(role.MaxLines ?? 4);
            RoleVisible = role.Visible;

            HasOutline = role.Outline is not null;
            OutlineColorHex = role.Outline?.Color ?? "#0A0E18D2";
            OutlineWidth = Math.Round(role.Outline?.Width ?? 2.5, 1);

            HasShadow = role.Shadow is not null;
            ShadowColorHex = role.Shadow?.Color ?? "#000000B9";
            ShadowOffsetX = Math.Round(role.Shadow?.OffsetX ?? 0, 1);
            ShadowOffsetY = Math.Round(role.Shadow?.OffsetY ?? 4, 1);
            ShadowBlur = Math.Round(role.Shadow?.Blur ?? 8, 1);

            HasBackgroundBox = role.BackgroundBox is not null;
            BoxColorHex = role.BackgroundBox?.Color ?? "#162E51E6";
            BoxPaddingX = Math.Round(role.BackgroundBox?.PaddingX ?? 24, 1);
            BoxPaddingY = Math.Round(role.BackgroundBox?.PaddingY ?? 14, 1);
            BoxRadius = Math.Round(role.BackgroundBox?.Radius ?? 10, 1);
            BoxFullWidth = role.BackgroundBox?.FullWidth ?? false;
            BoxAccentColorHex = role.BackgroundBox?.AccentBarColor ?? string.Empty;
            BoxAccentWidth = Math.Round(role.BackgroundBox?.AccentBarWidth ?? 0, 1);
        }
        finally
        {
            suppressSync = false;
        }
    }

    private void PushRoleChange()
    {
        if (suppressSync)
        {
            return;
        }
        var role = style.Roles.GetRole(SelectedRoleKey);
        role.FontFamily = BundledFonts.Canonical(RoleFontFamily);
        role.Size = (float)Math.Clamp(RoleSize, 8, 400);
        role.MinSize = (float)Math.Clamp(RoleMinSize, 8, role.Size);
        role.Weight = Math.Clamp(RoleWeight, 100, 900);
        role.Color = TemplateColorHelper.NormalizeHexOrDefault(RoleColorHex, role.Color);
        role.AlignH = RoleAlignH;
        role.AlignV = RoleAlignV;
        role.LineHeight = (float)Math.Clamp(RoleLineHeight, 0.8, 4.0);
        role.LetterSpacing = (float)Math.Clamp(RoleLetterSpacing, -24, 64);
        role.MaxLines = (uint)Math.Clamp(RoleMaxLines, 1, 16);
        role.Visible = RoleVisible;

        role.Outline = HasOutline
            ? new OutlineStyleModel
            {
                Color = TemplateColorHelper.NormalizeHexOrDefault(OutlineColorHex, "#0A0E18D2"),
                Width = (float)Math.Clamp(OutlineWidth, 0, 64),
            }
            : null;
        role.Shadow = HasShadow
            ? new ShadowStyleModel
            {
                Color = TemplateColorHelper.NormalizeHexOrDefault(ShadowColorHex, "#000000B9"),
                OffsetX = (float)Math.Clamp(ShadowOffsetX, -128, 128),
                OffsetY = (float)Math.Clamp(ShadowOffsetY, -128, 128),
                Blur = (float)Math.Clamp(ShadowBlur, 0, 64),
            }
            : null;
        role.BackgroundBox = HasBackgroundBox
            ? new BackgroundBoxStyleModel
            {
                Color = TemplateColorHelper.NormalizeHexOrDefault(BoxColorHex, "#162E51E6"),
                PaddingX = (float)Math.Clamp(BoxPaddingX, 0, 240),
                PaddingY = (float)Math.Clamp(BoxPaddingY, 0, 240),
                Radius = (float)Math.Clamp(BoxRadius, 0, 128),
                FullWidth = BoxFullWidth,
                AccentBarColor = string.IsNullOrWhiteSpace(BoxAccentColorHex)
                    ? null
                    : TemplateColorHelper.NormalizeHexOrDefault(BoxAccentColorHex, "#00BDE3"),
                AccentBarWidth = (float)Math.Clamp(BoxAccentWidth, 0, 48),
            }
            : null;

        _ = RefreshPreviewAsync();
    }

    private void PushSafeAreaChange()
    {
        if (suppressSync)
        {
            return;
        }
        var clamped = new SafeAreaModel
        {
            Left = (float)SafeLeft,
            Top = (float)SafeTop,
            Right = (float)SafeRight,
            Bottom = (float)SafeBottom,
        };
        clamped.Clamp();
        if (SelectedVariant is not null)
        {
            SelectedVariant.SafeArea = clamped;
        }
        else
        {
            style.SafeArea = clamped;
        }
        if (PreviewAspect == "16:9")
        {
            style.SafeArea = clamped.Clone();
        }
        _ = RefreshPreviewAsync();
    }

    public async Task RefreshPreviewAsync()
    {
        if (library is null)
        {
            return;
        }
        var generation = Interlocked.Increment(ref previewGeneration);
        try
        {
            var (w, h) = PreviewAspect switch
            {
                "32:9" => (3840u, 1080u),
                "4:3" => (1440u, 1080u),
                "9:16" => (1080u, 1920u),
                _ => (1920u, 1080u),
            };
            var styleJson = BuildStyleModel().ToJson();
            var chordsJson = JsonSerializer.Serialize(new[]
            {
                new { line = 0, position = 0, text = "G" },
                new { line = 0, position = 8, text = "C" },
            });
            var scene = SlidePreviewCard.BuildSlideScene(
                "Amazing grace, how sweet the sound\nThat saved a wretch like me",
                footer: $"{TemplateName} · Public Domain",
                referenceText: "John 3:16 (KJV)",
                titleText: "Verse 1",
                chordsJson: chordsJson,
                translationText: "Oore-ọ̀fẹ́ ti o dùn!",
                width: w,
                height: h,
                styleJson: styleJson);
            var reply = await library.RenderSceneThumbnailAsync(scene, 0, 480, 270);
            if (Volatile.Read(ref previewGeneration) != generation)
            {
                return;
            }
            PreviewBitmap = MediaCardViewModel.DecodePng(reply.PngBytes.ToByteArray());
            HasFlashWarning = reply.FlashWarning;
        }
        catch (Exception)
        {
        }
    }

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
