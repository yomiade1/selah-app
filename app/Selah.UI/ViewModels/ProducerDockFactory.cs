using Dock.Model.Controls;
using Dock.Model.Core;
using Dock.Model.Mvvm;
using Dock.Model.Mvvm.Controls;
using Selah.UI.Services;

namespace Selah.UI.ViewModels;

/// <summary>A Producer pane. Each kind has its own view (data templates in LiveProducerView).</summary>
public abstract class ProducerPane : Tool
{
    protected ProducerPane(LiveProducerViewModel owner, string id, string titleKey)
    {
        Live = owner;
        Id = id;
        Title = Localizer.Instance[titleKey];
        // Panes can move and resize, but never close or float away: the live controls must stay put.
        CanClose = false;
        CanFloat = false;
        CanPin = false;
    }

    public LiveProducerViewModel Live { get; }
}

public sealed class ServiceOrderPane(LiveProducerViewModel owner) : ProducerPane(owner, "service-order", "live-pane-service-order");

public sealed class SlidesPane(LiveProducerViewModel owner) : ProducerPane(owner, "slides", "live-pane-slides");

public sealed class ProgramPane(LiveProducerViewModel owner) : ProducerPane(owner, "program", "live-pane-program");

public sealed class LayersPane(LiveProducerViewModel owner) : ProducerPane(owner, "layers", "live-pane-layers");

public sealed class TilesPane(LiveProducerViewModel owner) : ProducerPane(owner, "tiles", "live-pane-tiles");

/// <summary>The stage display editor (M4.6).</summary>
public sealed class StagePane(LiveProducerViewModel owner) : ProducerPane(owner, "stage", "live-pane-stage");

/// <summary>
/// Builds the Producer docking workspace (Dock.Avalonia, ADR-002) from a saved layout, and reads
/// the current sizes back so the person can save their own.
/// </summary>
public sealed class ProducerDockFactory(LiveProducerViewModel owner) : Factory
{
    private ProportionalDock? columns;
    private ProportionalDock? inspector;
    private ToolDock? inspectorTabs;

    public IRootDock Build(ProducerLayoutState state)
    {
        var order = new ToolDock { Id = "order-dock", Proportion = state.ServiceOrder, Alignment = Alignment.Left };
        order.VisibleDockables = CreateList<IDockable>(new ServiceOrderPane(owner));
        order.ActiveDockable = order.VisibleDockables[0];

        var slides = new ToolDock { Id = "slides-dock", Proportion = state.Slides };
        slides.VisibleDockables = CreateList<IDockable>(new SlidesPane(owner));
        slides.ActiveDockable = slides.VisibleDockables[0];

        var program = new ToolDock { Id = "program-dock", Proportion = state.Program };
        program.VisibleDockables = CreateList<IDockable>(new ProgramPane(owner));
        program.ActiveDockable = program.VisibleDockables[0];

        inspectorTabs = new ToolDock { Id = "inspector-tabs", Proportion = 1 - state.Program };
        var layers = new LayersPane(owner);
        var tiles = new TilesPane(owner);
        var stage = new StagePane(owner);
        inspectorTabs.VisibleDockables = CreateList<IDockable>(layers, tiles, stage);
        inspectorTabs.ActiveDockable = state.InspectorTab switch
        {
            "tiles" => tiles,
            "stage" => stage,
            _ => layers,
        };

        inspector = new ProportionalDock
        {
            Id = "inspector",
            Orientation = Orientation.Vertical,
            Proportion = state.Inspector,
            VisibleDockables = CreateList<IDockable>(program, new ProportionalDockSplitter(), inspectorTabs),
        };

        columns = new ProportionalDock
        {
            Id = "columns",
            Orientation = Orientation.Horizontal,
            VisibleDockables = CreateList<IDockable>(order, new ProportionalDockSplitter(), slides, new ProportionalDockSplitter(), inspector),
        };

        var root = CreateRootDock();
        root.Id = "producer";
        root.IsCollapsable = false;
        root.VisibleDockables = CreateList<IDockable>(columns);
        root.ActiveDockable = columns;
        root.DefaultDockable = columns;
        InitLayout(root);
        return root;
    }

    /// <summary>Every pane in the layout, in reading order (for tests and help anchors).</summary>
    public IReadOnlyList<string> PaneIds(IDockable root)
    {
        var ids = new List<string>();
        void Walk(IDockable dockable)
        {
            if (dockable is ProducerPane pane)
            {
                ids.Add(pane.Id);
            }
            if (dockable is IDock { VisibleDockables: { } children })
            {
                foreach (var child in children)
                {
                    Walk(child);
                }
            }
        }
        Walk(root);
        return ids;
    }

    /// <summary>Read the sizes and tabs the person has now.</summary>
    public ProducerLayoutState Capture()
    {
        var docks = columns?.VisibleDockables?.OfType<IDock>().ToList() ?? [];
        double Size(int i, double fallback) => i < docks.Count && double.IsFinite(docks[i].Proportion) ? docks[i].Proportion : fallback;
        var programDock = inspector?.VisibleDockables?.OfType<IDock>().FirstOrDefault();
        var tab = inspectorTabs?.ActiveDockable switch
        {
            TilesPane => "tiles",
            StagePane => "stage",
            _ => "layers",
        };
        return new ProducerLayoutState(
            "custom",
            Size(0, 0.22),
            Size(1, 0.48),
            Size(2, 0.30),
            programDock is { Proportion: var p } && double.IsFinite(p) ? p : 0.55,
            tab);
    }
}
