using System.Globalization;
using System.Text.Json;
using System.Text.Json.Serialization;
using Selah.Contracts.V1;
using Selah.UI.ViewModels;

namespace Selah.UI.Services;

/// <summary>
/// C# model mirroring Rust <c>selah_render::template::TemplateStyle</c> (M3.2 / M3.7).
/// Serializes cleanly to and from protobuf <see cref="Template"/> and <see cref="SlideLayerSpec.StyleJson"/>.
/// </summary>
public static class BundledFonts
{
    public static readonly IReadOnlyList<string> Families =
    [
        "Noto Sans",
        "Inter",
        "Manrope",
        "JetBrains Mono",
        "Noto Serif",
        "Noto Sans KR",
        "Noto Naskh Arabic",
    ];

    public static string Canonical(string? family)
    {
        if (string.IsNullOrWhiteSpace(family))
        {
            return "Noto Sans";
        }
        var trimmed = family.Trim();
        var match = Families.FirstOrDefault(f => string.Equals(f, trimmed, StringComparison.OrdinalIgnoreCase));
        if (match is not null)
        {
            return match;
        }
        return trimmed.ToLowerInvariant() switch
        {
            "sans" or "sans-serif" or "arial" or "helvetica" or "system-ui" => "Noto Sans",
            "serif" or "georgia" or "times" or "times new roman" => "Noto Serif",
            "mono" or "monospace" or "consolas" or "courier" => "JetBrains Mono",
            "korean" or "hangul" => "Noto Sans KR",
            "arabic" => "Noto Naskh Arabic",
            _ => "Noto Sans",
        };
    }
}

public sealed class SafeAreaModel
{
    [JsonPropertyName("left")]
    public float Left { get; set; } = 0.08f;

    [JsonPropertyName("top")]
    public float Top { get; set; } = 0.08f;

    [JsonPropertyName("right")]
    public float Right { get; set; } = 0.08f;

    [JsonPropertyName("bottom")]
    public float Bottom { get; set; } = 0.08f;

    public SafeAreaModel Clone() => new() { Left = Left, Top = Top, Right = Right, Bottom = Bottom };

    public void Clamp()
    {
        static float Sanitize(float v) => float.IsFinite(v) ? Math.Clamp(v, 0f, 0.85f) : 0.08f;
        Left = Sanitize(Left);
        Right = Sanitize(Right);
        if (Left + Right > 0.90f)
        {
            var k = 0.90f / (Left + Right);
            Left *= k;
            Right *= k;
        }
        Top = Sanitize(Top);
        Bottom = Sanitize(Bottom);
        if (Top + Bottom > 0.90f)
        {
            var k = 0.90f / (Top + Bottom);
            Top *= k;
            Bottom *= k;
        }
    }

    public static SafeAreaModel Stage() => new() { Left = 0.05f, Top = 0.05f, Right = 0.05f, Bottom = 0.05f };

    public static SafeAreaModel LedWide() => new() { Left = 0.04f, Top = 0.08f, Right = 0.04f, Bottom = 0.08f };

    public static SafeAreaModel LowerThird() => new() { Left = 0.05f, Top = 0.66f, Right = 0.05f, Bottom = 0.05f };
}

public sealed class OutlineStyleModel
{
    [JsonPropertyName("color")]
    public string Color { get; set; } = "#000000E6";

    [JsonPropertyName("width")]
    public float Width { get; set; } = 3.0f;

    public OutlineStyleModel Clone() => new() { Color = Color, Width = Width };
}

public sealed class ShadowStyleModel
{
    [JsonPropertyName("color")]
    public string Color { get; set; } = "#000000BE";

    [JsonPropertyName("offset_x")]
    public float OffsetX { get; set; }

    [JsonPropertyName("offset_y")]
    public float OffsetY { get; set; } = 4.0f;

    [JsonPropertyName("blur")]
    public float Blur { get; set; } = 8.0f;

    public ShadowStyleModel Clone() => new() { Color = Color, OffsetX = OffsetX, OffsetY = OffsetY, Blur = Blur };
}

public sealed class BackgroundBoxStyleModel
{
    [JsonPropertyName("color")]
    public string Color { get; set; } = "#162E51E0";

    [JsonPropertyName("padding_x")]
    public float PaddingX { get; set; } = 24.0f;

    [JsonPropertyName("padding_y")]
    public float PaddingY { get; set; } = 14.0f;

    [JsonPropertyName("radius")]
    public float Radius { get; set; } = 10.0f;

    [JsonPropertyName("full_width")]
    public bool FullWidth { get; set; }

    [JsonPropertyName("accent_bar_color")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? AccentBarColor { get; set; }

    [JsonPropertyName("accent_bar_width")]
    public float AccentBarWidth { get; set; }

    public BackgroundBoxStyleModel Clone() => new()
    {
        Color = Color,
        PaddingX = PaddingX,
        PaddingY = PaddingY,
        Radius = Radius,
        FullWidth = FullWidth,
        AccentBarColor = AccentBarColor,
        AccentBarWidth = AccentBarWidth,
    };
}

public sealed class SlotTextStyleModel
{
    [JsonPropertyName("font_family")]
    public string FontFamily { get; set; } = "Noto Sans";

    [JsonPropertyName("size")]
    public float Size { get; set; } = 72.0f;

    [JsonPropertyName("min_size")]
    public float MinSize { get; set; } = 24.0f;

    [JsonPropertyName("weight")]
    public int Weight { get; set; } = 400;

    [JsonPropertyName("color")]
    public string Color { get; set; } = "#FFFFFF";

    [JsonPropertyName("outline")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public OutlineStyleModel? Outline { get; set; }

    [JsonPropertyName("shadow")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public ShadowStyleModel? Shadow { get; set; }

    [JsonPropertyName("background_box")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public BackgroundBoxStyleModel? BackgroundBox { get; set; }

    [JsonPropertyName("line_height")]
    public float LineHeight { get; set; } = 1.25f;

    [JsonPropertyName("letter_spacing")]
    public float LetterSpacing { get; set; }

    [JsonPropertyName("align_h")]
    public string AlignH { get; set; } = "center";

    [JsonPropertyName("align_v")]
    public string AlignV { get; set; } = "middle";

    [JsonPropertyName("max_lines")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public uint? MaxLines { get; set; }

    [JsonPropertyName("visible")]
    public bool Visible { get; set; } = true;

    public SlotTextStyleModel Clone() => new()
    {
        FontFamily = FontFamily,
        Size = Size,
        MinSize = MinSize,
        Weight = Weight,
        Color = Color,
        Outline = Outline?.Clone(),
        Shadow = Shadow?.Clone(),
        BackgroundBox = BackgroundBox?.Clone(),
        LineHeight = LineHeight,
        LetterSpacing = LetterSpacing,
        AlignH = AlignH,
        AlignV = AlignV,
        MaxLines = MaxLines,
        Visible = Visible,
    };

    public void Normalize()
    {
        FontFamily = BundledFonts.Canonical(FontFamily);
        Size = float.IsFinite(Size) ? Math.Clamp(Size, 8f, 400f) : 72f;
        MinSize = float.IsFinite(MinSize) ? Math.Clamp(MinSize, 8f, Size) : Math.Min(24f, Size);
        Weight = Math.Clamp(Weight, 100, 900);
        LineHeight = float.IsFinite(LineHeight) ? Math.Clamp(LineHeight, 0.8f, 4.0f) : 1.25f;
        LetterSpacing = float.IsFinite(LetterSpacing) ? Math.Clamp(LetterSpacing, -24f, 64f) : 0f;
        Color = TemplateColorHelper.NormalizeHexOrDefault(Color, "#FFFFFF");
        AlignH = AlignH?.Trim().ToLowerInvariant() switch
        {
            "left" or "start" => "left",
            "right" or "end" => "right",
            "justify" or "justified" => "justify",
            _ => "center",
        };
        AlignV = AlignV?.Trim().ToLowerInvariant() switch
        {
            "top" or "start" => "top",
            "bottom" or "end" => "bottom",
            _ => "middle",
        };
        if (Outline is not null)
        {
            Outline.Color = TemplateColorHelper.NormalizeHexOrDefault(Outline.Color, "#000000E6");
            Outline.Width = float.IsFinite(Outline.Width) ? Math.Clamp(Outline.Width, 0f, 64f) : 0f;
        }
        if (Shadow is not null)
        {
            Shadow.Color = TemplateColorHelper.NormalizeHexOrDefault(Shadow.Color, "#000000BE");
            Shadow.OffsetX = float.IsFinite(Shadow.OffsetX) ? Math.Clamp(Shadow.OffsetX, -128f, 128f) : 0f;
            Shadow.OffsetY = float.IsFinite(Shadow.OffsetY) ? Math.Clamp(Shadow.OffsetY, -128f, 128f) : 4f;
            Shadow.Blur = float.IsFinite(Shadow.Blur) ? Math.Clamp(Shadow.Blur, 0f, 64f) : 8f;
        }
        if (BackgroundBox is not null)
        {
            BackgroundBox.Color = TemplateColorHelper.NormalizeHexOrDefault(BackgroundBox.Color, "#162E51E0");
            BackgroundBox.PaddingX = float.IsFinite(BackgroundBox.PaddingX) ? Math.Clamp(BackgroundBox.PaddingX, 0f, 240f) : 24f;
            BackgroundBox.PaddingY = float.IsFinite(BackgroundBox.PaddingY) ? Math.Clamp(BackgroundBox.PaddingY, 0f, 240f) : 14f;
            BackgroundBox.Radius = float.IsFinite(BackgroundBox.Radius) ? Math.Clamp(BackgroundBox.Radius, 0f, 128f) : 10f;
            BackgroundBox.AccentBarWidth = float.IsFinite(BackgroundBox.AccentBarWidth) ? Math.Clamp(BackgroundBox.AccentBarWidth, 0f, 48f) : 0f;
            if (!string.IsNullOrWhiteSpace(BackgroundBox.AccentBarColor))
            {
                BackgroundBox.AccentBarColor = TemplateColorHelper.NormalizeHexOrDefault(BackgroundBox.AccentBarColor, "#00BDE3");
            }
            else
            {
                BackgroundBox.AccentBarColor = null;
            }
        }
    }
}

public sealed class RoleSlotsModel
{
    [JsonPropertyName("body")]
    public SlotTextStyleModel Body { get; set; } = new();

    [JsonPropertyName("reference")]
    public SlotTextStyleModel Reference { get; set; } = DefaultReference();

    [JsonPropertyName("title")]
    public SlotTextStyleModel Title { get; set; } = DefaultTitle();

    [JsonPropertyName("footer")]
    public SlotTextStyleModel Footer { get; set; } = DefaultFooter();

    [JsonPropertyName("chords")]
    public SlotTextStyleModel Chords { get; set; } = DefaultChords();

    [JsonPropertyName("translation")]
    public SlotTextStyleModel Translation { get; set; } = DefaultTranslation();

    public static SlotTextStyleModel DefaultReference() => new()
    {
        FontFamily = "Noto Sans",
        Size = 36f,
        MinSize = 20f,
        Weight = 600,
        Color = "#E0E8F0F0",
        AlignH = "center",
        AlignV = "top",
        LineHeight = 1.2f,
    };

    public static SlotTextStyleModel DefaultTitle() => new()
    {
        FontFamily = "Noto Sans",
        Size = 84f,
        MinSize = 36f,
        Weight = 700,
        Color = "#FFFFFF",
        AlignH = "center",
        AlignV = "top",
        LineHeight = 1.15f,
    };

    public static SlotTextStyleModel DefaultFooter() => new()
    {
        FontFamily = "Noto Sans",
        Size = 24f,
        MinSize = 16f,
        Weight = 400,
        Color = "#D7E0EBD7",
        AlignH = "center",
        AlignV = "bottom",
        LineHeight = 1.25f,
    };

    public static SlotTextStyleModel DefaultChords() => new()
    {
        FontFamily = "JetBrains Mono",
        Size = 36f,
        MinSize = 18f,
        Weight = 700,
        Color = "#FFBE2E",
        AlignH = "left",
        AlignV = "top",
        LineHeight = 1.2f,
        Visible = false,
    };

    public static SlotTextStyleModel DefaultTranslation() => new()
    {
        FontFamily = "Noto Sans",
        Size = 44f,
        MinSize = 20f,
        Weight = 400,
        Color = "#DCE6F5EB",
        AlignH = "center",
        AlignV = "middle",
        LineHeight = 1.25f,
        Visible = true,
    };

    public RoleSlotsModel Clone() => new()
    {
        Body = Body.Clone(),
        Reference = Reference.Clone(),
        Title = Title.Clone(),
        Footer = Footer.Clone(),
        Chords = Chords.Clone(),
        Translation = Translation.Clone(),
    };

    public SlotTextStyleModel GetRole(string roleKey) => roleKey switch
    {
        "reference" => Reference,
        "title" => Title,
        "footer" => Footer,
        "chords" => Chords,
        "translation" => Translation,
        _ => Body,
    };

    public void Normalize()
    {
        Body.Normalize();
        Reference.Normalize();
        Title.Normalize();
        Footer.Normalize();
        Chords.Normalize();
        Translation.Normalize();
    }
}

public sealed class SlotTextStylePatchModel
{
    [JsonPropertyName("font_family")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? FontFamily { get; set; }

    [JsonPropertyName("size")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public float? Size { get; set; }

    [JsonPropertyName("min_size")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public float? MinSize { get; set; }

    [JsonPropertyName("weight")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public int? Weight { get; set; }

    [JsonPropertyName("color")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? Color { get; set; }

    [JsonPropertyName("outline")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public OutlineStyleModel? Outline { get; set; }

    [JsonPropertyName("shadow")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public ShadowStyleModel? Shadow { get; set; }

    [JsonPropertyName("background_box")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public BackgroundBoxStyleModel? BackgroundBox { get; set; }

    [JsonPropertyName("line_height")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public float? LineHeight { get; set; }

    [JsonPropertyName("letter_spacing")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public float? LetterSpacing { get; set; }

    [JsonPropertyName("align_h")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? AlignH { get; set; }

    [JsonPropertyName("align_v")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? AlignV { get; set; }

    [JsonPropertyName("max_lines")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public uint? MaxLines { get; set; }

    [JsonPropertyName("visible")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public bool? Visible { get; set; }

    public SlotTextStylePatchModel Clone() => new()
    {
        FontFamily = FontFamily,
        Size = Size,
        MinSize = MinSize,
        Weight = Weight,
        Color = Color,
        Outline = Outline?.Clone(),
        Shadow = Shadow?.Clone(),
        BackgroundBox = BackgroundBox?.Clone(),
        LineHeight = LineHeight,
        LetterSpacing = LetterSpacing,
        AlignH = AlignH,
        AlignV = AlignV,
        MaxLines = MaxLines,
        Visible = Visible,
    };
}

public sealed class RoleSlotsPatchModel
{
    [JsonPropertyName("body")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public SlotTextStylePatchModel? Body { get; set; }

    [JsonPropertyName("reference")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public SlotTextStylePatchModel? Reference { get; set; }

    [JsonPropertyName("title")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public SlotTextStylePatchModel? Title { get; set; }

    [JsonPropertyName("footer")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public SlotTextStylePatchModel? Footer { get; set; }

    [JsonPropertyName("chords")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public SlotTextStylePatchModel? Chords { get; set; }

    [JsonPropertyName("translation")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public SlotTextStylePatchModel? Translation { get; set; }

    public RoleSlotsPatchModel Clone() => new()
    {
        Body = Body?.Clone(),
        Reference = Reference?.Clone(),
        Title = Title?.Clone(),
        Footer = Footer?.Clone(),
        Chords = Chords?.Clone(),
        Translation = Translation?.Clone(),
    };
}

public sealed class AspectVariantStyleModel
{
    [JsonPropertyName("aspect_ratio")]
    public string AspectRatio { get; set; } = "16:9";

    [JsonPropertyName("reference_width")]
    public uint ReferenceWidth { get; set; } = 1920;

    [JsonPropertyName("reference_height")]
    public uint ReferenceHeight { get; set; } = 1080;

    [JsonPropertyName("max_lines")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public uint? MaxLines { get; set; }

    [JsonPropertyName("safe_area")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public SafeAreaModel? SafeArea { get; set; }

    [JsonPropertyName("roles")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public RoleSlotsPatchModel? Roles { get; set; }

    public AspectVariantStyleModel Clone() => new()
    {
        AspectRatio = AspectRatio,
        ReferenceWidth = ReferenceWidth,
        ReferenceHeight = ReferenceHeight,
        MaxLines = MaxLines,
        SafeArea = SafeArea?.Clone(),
        Roles = Roles?.Clone(),
    };

    public static AspectVariantStyleModel CreateForAspect(string aspect) => aspect switch
    {
        "32:9" => new AspectVariantStyleModel
        {
            AspectRatio = "32:9",
            ReferenceWidth = 3840,
            ReferenceHeight = 1080,
            MaxLines = 2,
            SafeArea = SafeAreaModel.LedWide(),
            Roles = new RoleSlotsPatchModel
            {
                Body = new SlotTextStylePatchModel { Size = 68f, MinSize = 32f, MaxLines = 2 },
            },
        },
        "4:3" => new AspectVariantStyleModel
        {
            AspectRatio = "4:3",
            ReferenceWidth = 1440,
            ReferenceHeight = 1080,
            MaxLines = 4,
            SafeArea = new SafeAreaModel { Left = 0.07f, Top = 0.08f, Right = 0.07f, Bottom = 0.08f },
            Roles = new RoleSlotsPatchModel
            {
                Body = new SlotTextStylePatchModel { Size = 64f, MinSize = 28f },
            },
        },
        "9:16" => new AspectVariantStyleModel
        {
            AspectRatio = "9:16",
            ReferenceWidth = 1080,
            ReferenceHeight = 1920,
            MaxLines = 5,
            SafeArea = new SafeAreaModel { Left = 0.10f, Top = 0.14f, Right = 0.10f, Bottom = 0.14f },
            Roles = new RoleSlotsPatchModel
            {
                Body = new SlotTextStylePatchModel { Size = 82f, MinSize = 36f },
            },
        },
        _ => new AspectVariantStyleModel
        {
            AspectRatio = "16:9",
            ReferenceWidth = 1920,
            ReferenceHeight = 1080,
            MaxLines = 4,
        },
    };
}

public static class TemplateColorHelper
{
    public static string NormalizeHexOrDefault(string? raw, string fallback)
    {
        if (string.IsNullOrWhiteSpace(raw))
        {
            return fallback;
        }
        var trimmed = raw.Trim();
        var digits = trimmed.StartsWith('#') ? trimmed[1..] : trimmed;
        if (digits.Length is not (3 or 4 or 6 or 8) || !digits.All(Uri.IsHexDigit))
        {
            return fallback;
        }
        if (digits.Length == 3)
        {
            return $"#{digits[0]}{digits[0]}{digits[1]}{digits[1]}{digits[2]}{digits[2]}".ToUpperInvariant();
        }
        if (digits.Length == 4)
        {
            return $"#{digits[0]}{digits[0]}{digits[1]}{digits[1]}{digits[2]}{digits[2]}{digits[3]}{digits[3]}".ToUpperInvariant();
        }
        return "#" + digits.ToUpperInvariant();
    }

    public static bool IsValidHex(string? raw)
    {
        if (string.IsNullOrWhiteSpace(raw))
        {
            return false;
        }
        var digits = raw.Trim().TrimStart('#');
        return digits.Length is 3 or 4 or 6 or 8 && digits.All(Uri.IsHexDigit);
    }
}

public sealed class TemplateStyleModel
{
    public const int MaxStyleJsonBytes = 65_536;
    public const int MaxVariants = 32;

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNameCaseInsensitive = true,
    };

    [JsonPropertyName("schema_version")]
    public uint SchemaVersion { get; set; } = 1;

    [JsonPropertyName("name")]
    public string Name { get; set; } = "Sanctuary Lyrics";

    [JsonPropertyName("reference_height")]
    public uint ReferenceHeight { get; set; } = 1080;

    [JsonPropertyName("safe_area")]
    public SafeAreaModel SafeArea { get; set; } = new();

    [JsonPropertyName("roles")]
    public RoleSlotsModel Roles { get; set; } = new();

    [JsonPropertyName("variants")]
    public List<AspectVariantStyleModel> Variants { get; set; } = [];

    public void Normalize()
    {
        SchemaVersion = Math.Max(1u, SchemaVersion);
        ReferenceHeight = Math.Clamp(ReferenceHeight == 0 ? 1080u : ReferenceHeight, 64u, 4320u);
        SafeArea ??= new SafeAreaModel();
        SafeArea.Clamp();
        Roles ??= new RoleSlotsModel();
        Roles.Normalize();
        Variants ??= [];
        if (Variants.Count > MaxVariants)
        {
            Variants.RemoveRange(MaxVariants, Variants.Count - MaxVariants);
        }
        foreach (var v in Variants)
        {
            v.SafeArea?.Clamp();
        }
    }

    public string ToJson()
    {
        Normalize();
        return JsonSerializer.Serialize(this, JsonOptions);
    }

    public static TemplateStyleModel FromJson(string? raw)
    {
        if (string.IsNullOrWhiteSpace(raw))
        {
            return StarterSanctuary();
        }
        try
        {
            using var doc = JsonDocument.Parse(raw);
            if (doc.RootElement.ValueKind != JsonValueKind.Object)
            {
                return StarterSanctuary();
            }
            var root = doc.RootElement;
            var isFull = root.TryGetProperty("roles", out _)
                || root.TryGetProperty("variants", out _)
                || root.TryGetProperty("safe_area", out _)
                || root.TryGetProperty("schema_version", out _);
            TemplateStyleModel? model = null;
            if (isFull)
            {
                model = JsonSerializer.Deserialize<TemplateStyleModel>(raw, JsonOptions);
            }
            model ??= new TemplateStyleModel();
            if (!isFull)
            {
                if (root.TryGetProperty("font", out var fontEl) || root.TryGetProperty("font_family", out fontEl))
                {
                    var font = BundledFonts.Canonical(fontEl.GetString());
                    model.Roles.Body.FontFamily = font;
                    model.Roles.Title.FontFamily = font;
                    model.Roles.Reference.FontFamily = font;
                    model.Roles.Footer.FontFamily = font;
                }
                if (root.TryGetProperty("size", out var sizeEl) && sizeEl.TryGetSingle(out var sz))
                {
                    model.Roles.Body.Size = sz;
                }
                if (root.TryGetProperty("min_size", out var minEl) && minEl.TryGetSingle(out var msz))
                {
                    model.Roles.Body.MinSize = msz;
                }
                if (root.TryGetProperty("margin", out var marginEl) && marginEl.TryGetSingle(out var margin))
                {
                    var m = Math.Clamp(margin, 0f, 0.45f);
                    model.SafeArea.Left = m;
                    model.SafeArea.Right = m;
                }
            }
            model.Normalize();
            return model;
        }
        catch (JsonException)
        {
            return StarterSanctuary();
        }
    }

    public static TemplateStyleModel FromProto(Template template)
    {
        if (template.Variants.Count == 0)
        {
            var empty = StarterSanctuary();
            empty.Name = string.IsNullOrWhiteSpace(template.Name) ? "Untitled" : template.Name;
            return empty;
        }
        if (template.Variants.Count == 1)
        {
            var parsed = FromJson(template.Variants[0].StyleJson);
            if (!string.IsNullOrWhiteSpace(template.Name))
            {
                parsed.Name = template.Name;
            }
            if (parsed.Variants.Count == 0)
            {
                var res = template.Variants[0].Resolution ?? new Resolution { Width = 1920, Height = 1080 };
                parsed.Variants.Add(new AspectVariantStyleModel
                {
                    AspectRatio = "16:9",
                    ReferenceWidth = res.Width > 0 ? res.Width : 1920,
                    ReferenceHeight = res.Height > 0 ? res.Height : 1080,
                    MaxLines = template.Variants[0].MaxLines > 0 ? template.Variants[0].MaxLines : 4,
                });
            }
            parsed.Normalize();
            return parsed;
        }

        // Multiple proto variants: choose base closest to 16:9.
        const float targetAr = 16f / 9f;
        var baseVariant = template.Variants
            .OrderBy(v => v.Resolution is { Height: > 0 } r ? Math.Abs((float)r.Width / r.Height - targetAr) : float.MaxValue)
            .First();
        var model = FromJson(baseVariant.StyleJson);
        if (!string.IsNullOrWhiteSpace(template.Name))
        {
            model.Name = template.Name;
        }
        model.Variants.Clear();
        foreach (var v in template.Variants)
        {
            var res = v.Resolution ?? new Resolution { Width = 1920, Height = 1080 };
            var rw = Math.Max(1u, res.Width);
            var rh = Math.Max(1u, res.Height);
            var ar = $"{rw}:{rh}";
            if (rw == 1920 && rh == 1080) ar = "16:9";
            else if (rw == 3840 && rh == 1080) ar = "32:9";
            else if (rw == 1440 && rh == 1080) ar = "4:3";
            else if (rw == 1080 && rh == 1920) ar = "9:16";
            model.Variants.Add(new AspectVariantStyleModel
            {
                AspectRatio = ar,
                ReferenceWidth = rw,
                ReferenceHeight = rh,
                MaxLines = v.MaxLines > 0 ? v.MaxLines : null,
            });
        }
        model.Normalize();
        return model;
    }

    public Template ToProto(Id? existingId = null)
    {
        Normalize();
        var v16 = Variants.FirstOrDefault(v => v.AspectRatio == "16:9") ?? Variants.FirstOrDefault();
        var template = new Template
        {
            Id = existingId is { Value.IsEmpty: false } ? existingId : LibraryIds.NewId(),
            Name = string.IsNullOrWhiteSpace(Name) ? "Untitled" : Name.Trim(),
        };
        template.Variants.Add(new TemplateVariant
        {
            Id = LibraryIds.NewId(),
            Resolution = new Resolution
            {
                Width = v16?.ReferenceWidth > 0 ? v16.ReferenceWidth : 1920u,
                Height = v16?.ReferenceHeight > 0 ? v16.ReferenceHeight : ReferenceHeight,
            },
            MaxLines = v16?.MaxLines ?? Roles.Body.MaxLines ?? 4u,
            StyleJson = ToJson(),
        });
        return template;
    }

    public static TemplateStyleModel StarterByKey(string lookKey) => lookKey switch
    {
        "warm-serif" or "Warm Serif" => StarterWarmSerif(),
        "lower-third" or "Stream Lower Third" => StarterLowerThird(),
        "stage-monitor" or "Stage Monitor" => StarterStageMonitor(),
        _ => StarterSanctuary(),
    };

    public static TemplateStyleModel StarterSanctuary() => new()
    {
        SchemaVersion = 1,
        Name = "Sanctuary Lyrics",
        ReferenceHeight = 1080,
        SafeArea = new SafeAreaModel(),
        Roles = new RoleSlotsModel
        {
            Body = new SlotTextStyleModel
            {
                FontFamily = "Manrope",
                Size = 74f,
                MinSize = 32f,
                Weight = 700,
                Color = "#FFFFFF",
                Outline = new OutlineStyleModel { Color = "#0A0E18D2", Width = 2.5f },
                Shadow = new ShadowStyleModel { Color = "#000000B9", OffsetX = 0f, OffsetY = 4f, Blur = 10f },
                LineHeight = 1.25f,
                LetterSpacing = 0.5f,
                AlignH = "center",
                AlignV = "middle",
                MaxLines = 4,
                Visible = true,
            },
            Reference = new SlotTextStyleModel
            {
                FontFamily = "Manrope",
                Size = 36f,
                MinSize = 22f,
                Weight = 700,
                Color = "#D8E8FFF5",
                Shadow = new ShadowStyleModel { Color = "#000000B4", OffsetX = 0f, OffsetY = 2f, Blur = 6f },
                AlignH = "center",
                AlignV = "top",
                LineHeight = 1.2f,
            },
            Title = new SlotTextStyleModel
            {
                FontFamily = "Manrope",
                Size = 88f,
                MinSize = 40f,
                Weight = 800,
                Color = "#FFFFFF",
                LetterSpacing = -0.6f,
                Shadow = new ShadowStyleModel(),
                AlignH = "center",
                AlignV = "top",
                LineHeight = 1.15f,
            },
            Footer = new SlotTextStyleModel
            {
                FontFamily = "Inter",
                Size = 24f,
                MinSize = 16f,
                Weight = 500,
                Color = "#E6ECF5DC",
                Shadow = new ShadowStyleModel { Color = "#000000AF", OffsetX = 0f, OffsetY = 2f, Blur = 4f },
                AlignH = "center",
                AlignV = "bottom",
                LineHeight = 1.25f,
            },
            Chords = RoleSlotsModel.DefaultChords(),
            Translation = new SlotTextStyleModel
            {
                FontFamily = "Inter",
                Size = 46f,
                MinSize = 22f,
                Weight = 500,
                Color = "#D7E4F8EB",
                Shadow = new ShadowStyleModel { Color = "#000000B4", OffsetX = 0f, OffsetY = 2f, Blur = 6f },
                AlignH = "center",
                AlignV = "middle",
                LineHeight = 1.25f,
                Visible = true,
            },
        },
        Variants =
        [
            AspectVariantStyleModel.CreateForAspect("16:9"),
            AspectVariantStyleModel.CreateForAspect("32:9"),
            AspectVariantStyleModel.CreateForAspect("4:3"),
            AspectVariantStyleModel.CreateForAspect("9:16"),
        ],
    };

    public static TemplateStyleModel StarterWarmSerif()
    {
        var s = StarterSanctuary();
        s.Name = "Warm Serif";
        s.Roles.Body.FontFamily = "Noto Serif";
        s.Roles.Body.Weight = 600;
        s.Roles.Body.Color = "#FFF8E8";
        s.Roles.Title.FontFamily = "Noto Serif";
        s.Roles.Title.Color = "#FFF4D6";
        s.Roles.Reference.FontFamily = "Noto Serif";
        return s;
    }

    public static TemplateStyleModel StarterLowerThird() => new()
    {
        SchemaVersion = 1,
        Name = "Stream Lower Third",
        ReferenceHeight = 1080,
        SafeArea = SafeAreaModel.LowerThird(),
        Roles = new RoleSlotsModel
        {
            Body = new SlotTextStyleModel
            {
                FontFamily = "Inter",
                Size = 46f,
                MinSize = 26f,
                Weight = 600,
                Color = "#FFFFFF",
                BackgroundBox = new BackgroundBoxStyleModel
                {
                    Color = "#162E51E6",
                    PaddingX = 24f,
                    PaddingY = 14f,
                    Radius = 8f,
                    FullWidth = false,
                    AccentBarColor = "#00BDE3",
                    AccentBarWidth = 8f,
                },
                LineHeight = 1.25f,
                LetterSpacing = 0f,
                AlignH = "left",
                AlignV = "bottom",
                MaxLines = 2,
                Visible = true,
            },
            Reference = new SlotTextStyleModel
            {
                FontFamily = "Inter",
                Size = 28f,
                MinSize = 18f,
                Weight = 700,
                Color = "#00BDE3",
                AlignH = "left",
                AlignV = "top",
                LineHeight = 1.2f,
            },
            Title = new SlotTextStyleModel
            {
                FontFamily = "Manrope",
                Size = 52f,
                MinSize = 28f,
                Weight = 700,
                Color = "#FFFFFF",
                AlignH = "left",
                AlignV = "top",
                LineHeight = 1.15f,
            },
            Footer = new SlotTextStyleModel
            {
                FontFamily = "Inter",
                Size = 20f,
                MinSize = 14f,
                Weight = 400,
                Color = "#C8D6E4DC",
                AlignH = "left",
                AlignV = "bottom",
                LineHeight = 1.25f,
            },
            Chords = RoleSlotsModel.DefaultChords(),
            Translation = new SlotTextStyleModel
            {
                FontFamily = "Inter",
                Size = 32f,
                MinSize = 18f,
                Weight = 400,
                Color = "#DCE6F5EB",
                AlignH = "left",
                AlignV = "bottom",
                LineHeight = 1.25f,
                Visible = true,
            },
        },
        Variants = [AspectVariantStyleModel.CreateForAspect("16:9")],
    };

    public static TemplateStyleModel StarterStageMonitor() => new()
    {
        SchemaVersion = 1,
        Name = "Stage Monitor",
        ReferenceHeight = 1080,
        SafeArea = SafeAreaModel.Stage(),
        Roles = new RoleSlotsModel
        {
            Body = new SlotTextStyleModel
            {
                FontFamily = "Inter",
                Size = 62f,
                MinSize = 28f,
                Weight = 600,
                Color = "#F5F5F7",
                LineHeight = 1.3f,
                AlignH = "left",
                AlignV = "middle",
                MaxLines = 4,
                Visible = true,
            },
            Chords = new SlotTextStyleModel
            {
                FontFamily = "JetBrains Mono",
                Size = 38f,
                MinSize = 20f,
                Weight = 700,
                Color = "#FFBE2E",
                AlignH = "left",
                AlignV = "top",
                LineHeight = 1.2f,
                Visible = true,
            },
            Title = new SlotTextStyleModel
            {
                FontFamily = "Manrope",
                Size = 44f,
                MinSize = 24f,
                Weight = 700,
                Color = "#00BDE3",
                AlignH = "left",
                AlignV = "top",
                LineHeight = 1.15f,
            },
            Reference = new SlotTextStyleModel
            {
                FontFamily = "Noto Sans",
                Size = 36f,
                MinSize = 20f,
                Weight = 600,
                Color = "#E0E8F0F0",
                AlignH = "left",
                AlignV = "top",
                LineHeight = 1.2f,
            },
            Footer = new SlotTextStyleModel
            {
                FontFamily = "Noto Sans",
                Size = 24f,
                MinSize = 16f,
                Weight = 400,
                Color = "#D7E0EBD7",
                AlignH = "left",
                AlignV = "bottom",
                LineHeight = 1.25f,
            },
            Translation = new SlotTextStyleModel
            {
                FontFamily = "Noto Sans",
                Size = 44f,
                MinSize = 20f,
                Weight = 400,
                Color = "#DCE6F5EB",
                AlignH = "left",
                AlignV = "middle",
                LineHeight = 1.25f,
                Visible = true,
            },
        },
        Variants = [AspectVariantStyleModel.CreateForAspect("16:9")],
    };
}
