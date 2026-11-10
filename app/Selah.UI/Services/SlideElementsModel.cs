using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;
using Selah.Contracts.V1;

namespace Selah.UI.Services;

/// <summary>
/// C# model mirroring Rust <c>selah_render::slide_elements::SlideElementsDocument</c> (M3.7 / M3-A5).
/// Enforces the closed 6 built-in effects (<c>SEC-04</c>) and <c>SEC-03</c> bounds before sending to core.
/// </summary>
public enum SlideAlignMode
{
    Left,
    Center,
    Right,
    Top,
    Middle,
    Bottom,
}

public static class SlideElementKinds
{
    public const string Text = "text";
    public const string Shape = "shape";
    public const string Image = "image";
    public const string Video = "video";
}

public static class SlideShapeKinds
{
    public const string Rect = "rect";
    public const string RoundedRect = "rounded_rect";
    public const string Ellipse = "ellipse";
    public const string Line = "line";

    public static readonly IReadOnlyList<string> All = [Rect, RoundedRect, Ellipse, Line];
}

public static class SlideEffectKinds
{
    public static readonly IReadOnlyList<string> BuiltinKinds =
    [
        "blur",
        "colour_adjust",
        "vignette",
        "background_blur",
        "colour_key",
        "mask",
    ];

    public static bool IsAllowed(string? kind) =>
        kind is not null && BuiltinKinds.Contains(kind.Trim().ToLowerInvariant());

    public static EffectKind ToProtoKind(string? kind) => kind?.Trim().ToLowerInvariant() switch
    {
        "blur" => EffectKind.Blur,
        "colour_adjust" or "color_adjust" => EffectKind.ColourAdjust,
        "vignette" => EffectKind.Vignette,
        "background_blur" => EffectKind.BackgroundBlur,
        "colour_key" or "color_key" => EffectKind.ColourKey,
        "mask" => EffectKind.Mask,
        _ => EffectKind.Unspecified,
    };
}

public static class SlideTransitionKinds
{
    public static readonly IReadOnlyList<string> AllKinds =
    [
        "cut",
        "fade",
        "dissolve",
        "fade_through_black",
        "push",
        "wipe",
        "zoom",
        "slide",
    ];

    public static readonly IReadOnlyList<string> AllDirections =
    [
        "left",
        "right",
        "up",
        "down",
    ];

    public static readonly IReadOnlyList<string> AllEasings =
    [
        "linear",
        "ease_in",
        "ease_out",
        "ease_in_out",
    ];

    public static TransitionKind ToProtoKind(string? kind) => kind?.Trim().ToLowerInvariant() switch
    {
        "fade" => TransitionKind.Fade,
        "dissolve" => TransitionKind.Dissolve,
        "fade_through_black" => TransitionKind.FadeThroughBlack,
        "push" => TransitionKind.Push,
        "wipe" => TransitionKind.Wipe,
        "zoom" => TransitionKind.Zoom,
        "slide" => TransitionKind.Slide,
        _ => TransitionKind.Cut,
    };

    public static TransitionDirection ToProtoDirection(string? direction) => direction?.Trim().ToLowerInvariant() switch
    {
        "left" => TransitionDirection.Left,
        "right" => TransitionDirection.Right,
        "up" => TransitionDirection.Up,
        "down" => TransitionDirection.Down,
        _ => TransitionDirection.Left,
    };

    public static TransitionEasing ToProtoEasing(string? easing) => easing?.Trim().ToLowerInvariant() switch
    {
        "linear" => TransitionEasing.Linear,
        "ease_in" => TransitionEasing.EaseIn,
        "ease_out" => TransitionEasing.EaseOut,
        "ease_in_out" => TransitionEasing.EaseInOut,
        _ => TransitionEasing.EaseInOut,
    };
}

public sealed class SlideEffectEntryModel
{
    [JsonPropertyName("kind")]
    public string Kind { get; set; } = "vignette";

    [JsonPropertyName("enabled")]
    public bool Enabled { get; set; } = true;

    [JsonPropertyName("intensity")]
    public float Intensity { get; set; } = 0.5f;

    [JsonPropertyName("params_json")]
    public string ParamsJson { get; set; } = string.Empty;

    public SlideEffectEntryModel Clone() => new()
    {
        Kind = Kind,
        Enabled = Enabled,
        Intensity = Intensity,
        ParamsJson = ParamsJson,
    };
}

public sealed class SlideBackgroundSpecModel
{
    [JsonPropertyName("mode")]
    public string Mode { get; set; } = "solid"; // "none" | "solid" | "image" | "video"

    [JsonPropertyName("media_id")]
    public string MediaId { get; set; } = string.Empty;

    [JsonPropertyName("content_hash")]
    public string ContentHash { get; set; } = string.Empty;

    [JsonPropertyName("media_extension")]
    public string MediaExtension { get; set; } = string.Empty;

    [JsonPropertyName("media_kind")]
    public string MediaKind { get; set; } = string.Empty;

    [JsonPropertyName("solid_color")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public float[]? SolidColor { get; set; } = [0.06f, 0.09f, 0.14f, 1.0f];

    [JsonPropertyName("fit")]
    public string Fit { get; set; } = "fill";

    [JsonPropertyName("blur_radius")]
    public float BlurRadius { get; set; }

    [JsonPropertyName("opacity")]
    public float Opacity { get; set; } = 1.0f;

    public SlideBackgroundSpecModel Clone() => new()
    {
        Mode = Mode,
        MediaId = MediaId,
        ContentHash = ContentHash,
        MediaExtension = MediaExtension,
        MediaKind = MediaKind,
        SolidColor = SolidColor?.ToArray(),
        Fit = Fit,
        BlurRadius = BlurRadius,
        Opacity = Opacity,
    };
}

public sealed class SlideTransitionSpecModel
{
    [JsonPropertyName("kind")]
    public string Kind { get; set; } = "fade";

    [JsonPropertyName("direction")]
    public string Direction { get; set; } = "left";

    [JsonPropertyName("easing")]
    public string Easing { get; set; } = "ease_in_out";

    [JsonPropertyName("duration_ms")]
    public uint DurationMs { get; set; } = 350;

    public SlideTransitionSpecModel Clone() => new()
    {
        Kind = Kind,
        Direction = Direction,
        Easing = Easing,
        DurationMs = DurationMs,
    };

    public TransitionSpec ToProto() => new()
    {
        Kind = SlideTransitionKinds.ToProtoKind(Kind),
        Direction = SlideTransitionKinds.ToProtoDirection(Direction),
        Easing = SlideTransitionKinds.ToProtoEasing(Easing),
        DurationMs = DurationMs,
    };
}

public sealed class SlideElementModel
{
    [JsonPropertyName("id")]
    public string Id { get; set; } = Guid.NewGuid().ToString("N");

    [JsonPropertyName("name")]
    public string Name { get; set; } = "Element";

    [JsonPropertyName("kind")]
    public string Kind { get; set; } = SlideElementKinds.Text;

    [JsonPropertyName("group_id")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? GroupId { get; set; }

    [JsonPropertyName("x")]
    public float X { get; set; } = 0.1f;

    [JsonPropertyName("y")]
    public float Y { get; set; } = 0.1f;

    [JsonPropertyName("width")]
    public float Width { get; set; } = 0.8f;

    [JsonPropertyName("height")]
    public float Height { get; set; } = 0.25f;

    [JsonPropertyName("opacity")]
    public float Opacity { get; set; } = 1.0f;

    [JsonPropertyName("visible")]
    public bool Visible { get; set; } = true;

    [JsonPropertyName("locked")]
    public bool Locked { get; set; }

    [JsonPropertyName("z_index")]
    public int ZIndex { get; set; }

    // Text
    [JsonPropertyName("text")]
    public string Text { get; set; } = string.Empty;

    [JsonPropertyName("font_family")]
    public string FontFamily { get; set; } = "Noto Sans";

    [JsonPropertyName("font_size")]
    public float FontSize { get; set; } = 48.0f;

    [JsonPropertyName("weight")]
    public int Weight { get; set; } = 600;

    [JsonPropertyName("color")]
    public string Color { get; set; } = "#FFFFFF";

    [JsonPropertyName("align_h")]
    public string AlignH { get; set; } = "center";

    [JsonPropertyName("align_v")]
    public string AlignV { get; set; } = "middle";

    [JsonPropertyName("line_height")]
    public float LineHeight { get; set; } = 1.25f;

    [JsonPropertyName("letter_spacing")]
    public float LetterSpacing { get; set; }

    [JsonPropertyName("outline")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public OutlineStyleModel? Outline { get; set; }

    [JsonPropertyName("shadow")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public ShadowStyleModel? Shadow { get; set; }

    [JsonPropertyName("background_box")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public BackgroundBoxStyleModel? BackgroundBox { get; set; }

    [JsonPropertyName("padding")]
    public float Padding { get; set; } = 8.0f;

    // Dynamic {clock} / {timer}
    [JsonPropertyName("clock_show_seconds")]
    public bool ClockShowSeconds { get; set; } = true;

    [JsonPropertyName("clock_24h")]
    public bool Clock24h { get; set; } = true;

    [JsonPropertyName("clock_offset_minutes")]
    public int ClockOffsetMinutes { get; set; }

    [JsonPropertyName("timer_target_unix_ms")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public long? TimerTargetUnixMs { get; set; }

    [JsonPropertyName("timer_duration_secs")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public uint? TimerDurationSecs { get; set; }

    [JsonPropertyName("timer_count_up")]
    public bool TimerCountUp { get; set; }

    // Shape
    [JsonPropertyName("shape_kind")]
    public string ShapeKind { get; set; } = SlideShapeKinds.Rect;

    [JsonPropertyName("fill_color")]
    public string FillColor { get; set; } = "#2378C3E0";

    [JsonPropertyName("stroke_color")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public string? StrokeColor { get; set; }

    [JsonPropertyName("stroke_width")]
    public float StrokeWidth { get; set; }

    [JsonPropertyName("corner_radius")]
    public float CornerRadius { get; set; } = 12.0f;

    // Media
    [JsonPropertyName("content_hash")]
    public string ContentHash { get; set; } = string.Empty;

    [JsonPropertyName("media_extension")]
    public string MediaExtension { get; set; } = string.Empty;

    [JsonPropertyName("fit")]
    public string Fit { get; set; } = "fit";

    [JsonPropertyName("blur_radius")]
    public float BlurRadius { get; set; }

    public SlideElementModel Clone() => new()
    {
        Id = Id,
        Name = Name,
        Kind = Kind,
        GroupId = GroupId,
        X = X,
        Y = Y,
        Width = Width,
        Height = Height,
        Opacity = Opacity,
        Visible = Visible,
        Locked = Locked,
        ZIndex = ZIndex,
        Text = Text,
        FontFamily = FontFamily,
        FontSize = FontSize,
        Weight = Weight,
        Color = Color,
        AlignH = AlignH,
        AlignV = AlignV,
        LineHeight = LineHeight,
        LetterSpacing = LetterSpacing,
        Outline = Outline?.Clone(),
        Shadow = Shadow?.Clone(),
        BackgroundBox = BackgroundBox?.Clone(),
        Padding = Padding,
        ClockShowSeconds = ClockShowSeconds,
        Clock24h = Clock24h,
        ClockOffsetMinutes = ClockOffsetMinutes,
        TimerTargetUnixMs = TimerTargetUnixMs,
        TimerDurationSecs = TimerDurationSecs,
        TimerCountUp = TimerCountUp,
        ShapeKind = ShapeKind,
        FillColor = FillColor,
        StrokeColor = StrokeColor,
        StrokeWidth = StrokeWidth,
        CornerRadius = CornerRadius,
        ContentHash = ContentHash,
        MediaExtension = MediaExtension,
        Fit = Fit,
        BlurRadius = BlurRadius,
    };

    public void Clamp()
    {
        FontFamily = BundledFonts.Canonical(FontFamily);
        X = float.IsFinite(X) ? Math.Clamp(X, -0.5f, 1.0f) : 0.1f;
        Y = float.IsFinite(Y) ? Math.Clamp(Y, -0.5f, 1.0f) : 0.1f;
        Width = float.IsFinite(Width) ? Math.Clamp(Width, 0.01f, 1.5f) : 0.3f;
        Height = float.IsFinite(Height) ? Math.Clamp(Height, 0.01f, 1.5f) : 0.2f;
        Opacity = float.IsFinite(Opacity) ? Math.Clamp(Opacity, 0f, 1f) : 1f;
        ZIndex = Math.Clamp(ZIndex, -10_000, 10_000);
        FontSize = float.IsFinite(FontSize) ? Math.Clamp(FontSize, 8f, 400f) : 48f;
        Weight = Math.Clamp(Weight, 100, 900);
        LineHeight = float.IsFinite(LineHeight) ? Math.Clamp(LineHeight, 0.8f, 4.0f) : 1.25f;
        LetterSpacing = float.IsFinite(LetterSpacing) ? Math.Clamp(LetterSpacing, -24f, 64f) : 0f;
        Padding = float.IsFinite(Padding) ? Math.Clamp(Padding, 0f, 240f) : 8f;
        ClockOffsetMinutes = Math.Clamp(ClockOffsetMinutes, -840, 840);
        StrokeWidth = float.IsFinite(StrokeWidth) ? Math.Clamp(StrokeWidth, 0f, 64f) : 0f;
        CornerRadius = float.IsFinite(CornerRadius) ? Math.Clamp(CornerRadius, 0f, 256f) : 12f;
        BlurRadius = float.IsFinite(BlurRadius) ? Math.Clamp(BlurRadius, 0f, 64f) : 0f;
        Color = TemplateColorHelper.NormalizeHexOrDefault(Color, "#FFFFFF");
        FillColor = TemplateColorHelper.NormalizeHexOrDefault(FillColor, "#2378C3E0");
        if (!string.IsNullOrWhiteSpace(StrokeColor))
        {
            StrokeColor = TemplateColorHelper.NormalizeHexOrDefault(StrokeColor, "#FFFFFF");
        }
        else
        {
            StrokeColor = null;
        }
    }
}

public sealed class SlideElementsDocumentModel
{
    public const int MaxElementsJsonBytes = 131_072;
    public const int MaxElements = 64;
    public const int MaxEffectsPerChain = 8;

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNameCaseInsensitive = true,
    };

    [JsonPropertyName("schema_version")]
    public uint SchemaVersion { get; set; } = 1;

    [JsonPropertyName("reference_height")]
    public uint ReferenceHeight { get; set; } = 1080;

    [JsonPropertyName("elements")]
    public List<SlideElementModel> Elements { get; set; } = [];

    [JsonPropertyName("background")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public SlideBackgroundSpecModel? Background { get; set; }

    [JsonPropertyName("transition")]
    [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public SlideTransitionSpecModel? Transition { get; set; }

    [JsonPropertyName("effects")]
    public List<SlideEffectEntryModel> Effects { get; set; } = [];

    public SlideElementsDocumentModel Clone() => new()
    {
        SchemaVersion = SchemaVersion,
        ReferenceHeight = ReferenceHeight,
        Elements = Elements.Select(e => e.Clone()).ToList(),
        Background = Background?.Clone(),
        Transition = Transition?.Clone(),
        Effects = Effects.Select(e => e.Clone()).ToList(),
    };

    public string ToJson()
    {
        foreach (var el in Elements)
        {
            el.Clamp();
        }
        var json = JsonSerializer.Serialize(this, JsonOptions);
        if (Encoding.UTF8.GetByteCount(json) > MaxElementsJsonBytes)
        {
            throw new InvalidOperationException("Slide elements JSON exceeds 128 KiB safety limit.");
        }
        return json;
    }

    public static SlideElementsDocumentModel FromJson(string? raw)
    {
        if (string.IsNullOrWhiteSpace(raw))
        {
            return new SlideElementsDocumentModel();
        }
        try
        {
            var trimmed = raw.Trim();
            if (trimmed.StartsWith('['))
            {
                var list = JsonSerializer.Deserialize<List<SlideElementModel>>(trimmed, JsonOptions) ?? [];
                return new SlideElementsDocumentModel { Elements = list };
            }
            var doc = JsonSerializer.Deserialize<SlideElementsDocumentModel>(trimmed, JsonOptions)
                ?? new SlideElementsDocumentModel();
            doc.Effects.RemoveAll(e => !SlideEffectKinds.IsAllowed(e.Kind));
            if (doc.Elements.Count > MaxElements)
            {
                doc.Elements.RemoveRange(MaxElements, doc.Elements.Count - MaxElements);
            }
            return doc;
        }
        catch (JsonException)
        {
            return new SlideElementsDocumentModel();
        }
    }

    /// <summary>Mirrors Rust <c>SlideElementsDocument::aligned_positions</c> verbatim.</summary>
    public static void AlignElements(IList<SlideElementModel> elements, IReadOnlyList<int> indices, SlideAlignMode mode)
    {
        if (indices.Count == 0)
        {
            return;
        }
        if (indices.Count == 1)
        {
            var idx = indices[0];
            if (idx >= 0 && idx < elements.Count && !elements[idx].Locked)
            {
                var el = elements[idx];
                switch (mode)
                {
                    case SlideAlignMode.Left: el.X = 0.05f; break;
                    case SlideAlignMode.Center: el.X = (1.0f - el.Width) * 0.5f; break;
                    case SlideAlignMode.Right: el.X = Math.Clamp(0.95f - el.Width, 0f, 1f); break;
                    case SlideAlignMode.Top: el.Y = 0.05f; break;
                    case SlideAlignMode.Middle: el.Y = (1.0f - el.Height) * 0.5f; break;
                    case SlideAlignMode.Bottom: el.Y = Math.Clamp(0.95f - el.Height, 0f, 1f); break;
                }
            }
            return;
        }

        var minX = float.MaxValue;
        var maxR = float.MinValue;
        var minY = float.MaxValue;
        var maxB = float.MinValue;
        foreach (var idx in indices)
        {
            if (idx >= 0 && idx < elements.Count)
            {
                var el = elements[idx];
                minX = Math.Min(minX, el.X);
                maxR = Math.Max(maxR, el.X + el.Width);
                minY = Math.Min(minY, el.Y);
                maxB = Math.Max(maxB, el.Y + el.Height);
            }
        }
        var midX = (minX + maxR) * 0.5f;
        var midY = (minY + maxB) * 0.5f;
        foreach (var idx in indices)
        {
            if (idx >= 0 && idx < elements.Count && !elements[idx].Locked)
            {
                var el = elements[idx];
                switch (mode)
                {
                    case SlideAlignMode.Left: el.X = minX; break;
                    case SlideAlignMode.Center: el.X = midX - el.Width * 0.5f; break;
                    case SlideAlignMode.Right: el.X = maxR - el.Width; break;
                    case SlideAlignMode.Top: el.Y = minY; break;
                    case SlideAlignMode.Middle: el.Y = midY - el.Height * 0.5f; break;
                    case SlideAlignMode.Bottom: el.Y = maxB - el.Height; break;
                }
            }
        }
    }
}
