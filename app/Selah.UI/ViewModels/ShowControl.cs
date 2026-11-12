using Selah.Contracts.V1;

namespace Selah.UI.ViewModels;

/// <summary>The kinds of thing a producer sets up once and uses as a tile while live (M4.3).</summary>
public enum ShowControlKind
{
    Look,
    Prop,
    Message,
    Timer,
    Macro,
    ClearGroup,
}

/// <summary>One saved look, prop, message, timer, macro or clear group: exactly one is set.</summary>
public sealed record ShowControl(ShowControlKind Kind, Look? Look = null, Prop? Prop = null, LiveMessage? Message = null,
    Selah.Contracts.V1.Timer? Timer = null, Macro? Macro = null, ClearGroup? ClearGroup = null)
{
    public Id Id => Kind switch
    {
        ShowControlKind.Look => Look!.Id,
        ShowControlKind.Prop => Prop!.Id,
        ShowControlKind.Message => Message!.Id,
        ShowControlKind.Timer => Timer!.Id,
        ShowControlKind.Macro => Macro!.Id,
        _ => ClearGroup!.Id,
    };

    public string Name => Kind switch
    {
        ShowControlKind.Look => Look!.Name,
        ShowControlKind.Prop => Prop!.Name,
        ShowControlKind.Message => Message!.Name,
        ShowControlKind.Timer => Timer!.Name,
        ShowControlKind.Macro => Macro!.Name,
        _ => ClearGroup!.Name,
    };

    public static ShowControl Of(Look look) => new(ShowControlKind.Look, Look: look);

    public static ShowControl Of(Prop prop) => new(ShowControlKind.Prop, Prop: prop);

    public static ShowControl Of(LiveMessage message) => new(ShowControlKind.Message, Message: message);

    public static ShowControl Of(Selah.Contracts.V1.Timer timer) => new(ShowControlKind.Timer, Timer: timer);

    public static ShowControl Of(Macro macro) => new(ShowControlKind.Macro, Macro: macro);

    public static ShowControl Of(ClearGroup group) => new(ShowControlKind.ClearGroup, ClearGroup: group);
}
