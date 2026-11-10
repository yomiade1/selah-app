using Avalonia;
using Avalonia.Controls;
using Avalonia.Interactivity;
using Avalonia.Platform.Storage;
using Selah.UI.Services;
using Selah.UI.ViewModels;

namespace Selah.UI.Views;

public sealed partial class LibraryView : UserControl
{
    public const double TwoColumnBreakpoint = 860;

    public LibraryView()
    {
        InitializeComponent();
        TextScale.Instance.PropertyChanged += (_, _) => UpdateResponsiveLayout();
    }

    private LibraryViewModel? Model => DataContext as LibraryViewModel;

    protected override void OnSizeChanged(SizeChangedEventArgs e)
    {
        base.OnSizeChanged(e);
        UpdateResponsiveLayout();
    }

    private void UpdateResponsiveLayout()
    {
        var effectiveWidth = Bounds.Width / TextScale.Instance.Factor;
        if (effectiveWidth <= 0)
        {
            return;
        }
        var wide = effectiveWidth >= TwoColumnBreakpoint;
        if (wide)
        {
            SongsSplitGrid.ColumnDefinitions = new ColumnDefinitions("300,*");
            Grid.SetColumn(SongsListPane, 0);
            Grid.SetRow(SongsListPane, 0);
            Grid.SetColumn(SongsDetailPane, 1);
            Grid.SetRow(SongsDetailPane, 0);

            ScriptureSplitGrid.ColumnDefinitions = new ColumnDefinitions("*,*");
            Grid.SetColumn(VersesPane, 0);
            Grid.SetRow(VersesPane, 0);
            Grid.SetColumn(ScriptureSlidesPane, 1);
            Grid.SetRow(ScriptureSlidesPane, 0);
        }
        else
        {
            SongsSplitGrid.ColumnDefinitions = new ColumnDefinitions("*");
            Grid.SetColumn(SongsListPane, 0);
            Grid.SetRow(SongsListPane, 0);
            Grid.SetColumn(SongsDetailPane, 0);
            Grid.SetRow(SongsDetailPane, 1);

            ScriptureSplitGrid.ColumnDefinitions = new ColumnDefinitions("*");
            Grid.SetColumn(VersesPane, 0);
            Grid.SetRow(VersesPane, 0);
            Grid.SetColumn(ScriptureSlidesPane, 0);
            Grid.SetRow(ScriptureSlidesPane, 1);
        }
    }

    private async void OnImportMediaClick(object? sender, RoutedEventArgs e)
    {
        if (Model is null)
        {
            return;
        }
        if (!string.IsNullOrWhiteSpace(Model.ImportMediaPath))
        {
            await Model.ImportMediaCommand.ExecuteAsync(null);
            return;
        }
        if (TopLevel.GetTopLevel(this)?.StorageProvider is { } storage)
        {
            var picked = await storage.OpenFilePickerAsync(new FilePickerOpenOptions { AllowMultiple = false });
            var local = picked.FirstOrDefault()?.TryGetLocalPath();
            if (!string.IsNullOrWhiteSpace(local))
            {
                await Model.ImportMediaFileAsync(local);
            }
        }
    }

    private async void OnImportBibleClick(object? sender, RoutedEventArgs e)
    {
        if (Model is null)
        {
            return;
        }
        if (!string.IsNullOrWhiteSpace(Model.ImportBiblePath))
        {
            await Model.ImportBibleCommand.ExecuteAsync(null);
            return;
        }
        if (TopLevel.GetTopLevel(this)?.StorageProvider is { } storage)
        {
            var picked = await storage.OpenFilePickerAsync(new FilePickerOpenOptions { AllowMultiple = false });
            var local = picked.FirstOrDefault()?.TryGetLocalPath();
            if (!string.IsNullOrWhiteSpace(local))
            {
                Model.ImportBiblePath = local;
                await Model.ImportBibleCommand.ExecuteAsync(null);
            }
        }
    }
}
