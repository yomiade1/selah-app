using Google.Protobuf;
using Selah.Contracts.V1;

namespace Selah.UI.ViewModels;

/// <summary>Helpers for RFC 9562 UUIDv7 document and section identifiers sent over IPC.</summary>
public static class LibraryIds
{
    public static Id NewId() => new() { Value = ByteString.CopyFrom(Guid.CreateVersion7().ToByteArray(bigEndian: true)) };

    public static bool Same(Id? left, Id? right) =>
        left is not null && right is not null && left.Value.Equals(right.Value);

    public static string Hex(Id? id) =>
        id is null || id.Value.IsEmpty ? string.Empty : Convert.ToHexStringLower(id.Value.Span);

    public static string SectionColorHex(SectionKind kind) => kind switch
    {
        SectionKind.Verse => "#2378C3",
        SectionKind.Chorus => "#C84281",
        SectionKind.Bridge => "#E66F0E",
        SectionKind.Tag => "#009EC1",
        _ => "#71767A",
    };

    public static Color SectionColor(SectionKind kind) => kind switch
    {
        SectionKind.Verse => new Color { Red = 0x23 / 255f, Green = 0x78 / 255f, Blue = 0xC3 / 255f, Alpha = 1f },
        SectionKind.Chorus => new Color { Red = 0xC8 / 255f, Green = 0x42 / 255f, Blue = 0x81 / 255f, Alpha = 1f },
        SectionKind.Bridge => new Color { Red = 0xE6 / 255f, Green = 0x6F / 255f, Blue = 0x0E / 255f, Alpha = 1f },
        SectionKind.Tag => new Color { Red = 0x00 / 255f, Green = 0x9E / 255f, Blue = 0xC1 / 255f, Alpha = 1f },
        _ => new Color { Red = 0x71 / 255f, Green = 0x76 / 255f, Blue = 0x7A / 255f, Alpha = 1f },
    };
}

/// <summary>Workspace modes shown in the main toolbar switcher.</summary>
public enum WorkspaceMode
{
    Preview,
    ServicePlanner,
    Library,
    SongEditor,
    SlideEditor,
    Templates,
    Live,
}

/// <summary>Sub-sections in the Library browser.</summary>
public enum LibraryTab
{
    Songs = 0,
    Media = 1,
    Bibles = 2,
}

/// <summary>USWDS 4-step workflow for creating and editing songs (prototype screen 03).</summary>
public enum SongEditorStep
{
    Paste = 0,
    Sections = 1,
    Arrange = 2,
    Review = 3,
}

/// <summary>Inspector tabs in the song editor.</summary>
public enum SongInspectorTab
{
    Song = 0,
    Layout = 1,
    Arrange = 2,
    Actions = 3,
}

/// <summary>Library, song, scripture, media, undo/redo, and exchange boundary (core over IPC; a fake in tests).</summary>
public interface ILibraryService
{
    event EventHandler<LibraryChanged>? LibraryChanged;

    event EventHandler<UndoStateChanged>? UndoStateChanged;

    Task<IReadOnlyList<Song>> ListSongsAsync(uint limit = 200, uint offset = 0);

    Task<SongSearchResults> SearchSongsAsync(string query, uint limit = 50, uint offset = 0);

    Task<Song?> GetSongAsync(Id id);

    Task SaveSongAsync(Song song, Id? undoGroup = null);

    Task DeleteSongAsync(Id id, Id? undoGroup = null);

    Task<ParsePastedSongReply> ParsePastedSongAsync(
        string rawText,
        SongPasteFormat format = SongPasteFormat.Unspecified,
        string defaultTitle = "");

    Task<Song> TransposeSongAsync(
        Song song,
        int semitones,
        AccidentalSpelling spelling = AccidentalSpelling.Unspecified);

    Task<GeneratedSongSlides> SplitSongSlidesAsync(SplitSongSlides request);

    Task SaveEditorBufferAsync(Id docId, DocumentType docType, string content);

    Task DiscardEditorBufferAsync(Id docId);

    Task<EditorBufferReply> GetEditorBufferAsync(Id docId);

    Task<UndoState> GetUndoStateAsync(Id? docId = null);

    Task UndoAsync(Id? docId = null);

    Task RedoAsync(Id? docId = null);

    Task<IReadOnlyList<BibleTranslation>> ListBiblesAsync(bool includeBooks = false);

    Task<BibleTranslation> ImportBibleAsync(ImportBible request);

    Task DeleteBibleAsync(string code);

    Task<ParseScriptureReferenceReply> ParseScriptureReferenceAsync(string reference, string languageHint = "");

    Task<ScripturePassages> GetScripturePassagesAsync(string translation, string reference, string languageHint = "");

    Task<ScriptureSearchResults> SearchScriptureAsync(string query, string translation = "", uint limit = 50, uint offset = 0);

    Task<GeneratedScriptureSlides> SplitScriptureSlidesAsync(SplitScriptureSlides request);

    Task<IReadOnlyList<MediaItem>> ListMediaItemsAsync(uint limit = 200, uint offset = 0);

    Task<MediaSearchResults> SearchMediaAsync(string query, MediaKind kind = MediaKind.Unspecified, uint limit = 50, uint offset = 0);

    Task<MediaItem> ImportMediaAsync(string sourcePath, string name = "", IEnumerable<string>? tags = null);

    Task DeleteMediaItemAsync(Id id);

    Task<MediaThumbnail> GetMediaThumbnailAsync(Id mediaId, uint maxWidth = 320, uint maxHeight = 180);

    Task<SceneThumbnail> RenderSceneThumbnailAsync(Scene scene, uint outputIndex = 0, uint maxWidth = 320, uint maxHeight = 180);

    Task StagePreviewSceneAsync(Scene scene);

    Task<ExportedSelahBundle> ExportSelahBundleAsync(ExportSelahBundle request);

    Task<ImportedSelahBundle> ImportSelahBundleAsync(ImportSelahBundle request);

    Task<LibraryBackupInfo> CreateLibraryBackupAsync(string outputPath = "");

    Task<RestoreLibraryBackupResult> RestoreLibraryBackupAsync(string backupPath);

    Task<IReadOnlyList<Tag>> ListTagsAsync(uint limit = 200, uint offset = 0);

    Task SaveTagAsync(Tag tag);

    Task<LibrarySearchResults> SearchLibraryAsync(
        string query,
        SearchScope scope = SearchScope.All,
        string translation = "",
        uint limit = 20);

    Task<IReadOnlyList<Template>> ListTemplatesAsync(uint limit = 200, uint offset = 0);

    Task<Template?> GetTemplateAsync(Id id);

    Task SaveTemplateAsync(Template template, Id? undoGroup = null);

    Task DeleteTemplateAsync(Id id, Id? undoGroup = null);

    Task<IReadOnlyList<Presentation>> ListPresentationsAsync(uint limit = 200, uint offset = 0);

    Task<Presentation?> GetPresentationAsync(Id id);

    Task SavePresentationAsync(Presentation presentation, Id? undoGroup = null);

    Task DeletePresentationAsync(Id id, Id? undoGroup = null);

    Task<IReadOnlyList<OutputConfig>> GetOutputConfigsAsync();

    Task SetOutputConfigsAsync(IEnumerable<OutputConfig> outputs);

    Task<GetPolicySettingReply> GetPolicySettingAsync(string key);

    Task SetPolicySettingAsync(string key, string valueJson, PolicyLevel level = PolicyLevel.User, bool locked = false);

    Task<FlashRiskReport> AnalyzeFlashRiskAsync(AnalyzeFlashRisk request);

    Task PresentSceneAsync(Scene scene);

    Task<IReadOnlyList<Service>> ListServicesAsync(uint limit = 200, uint offset = 0);

    Task<Service?> GetServiceAsync(Id id);

    Task SaveServiceAsync(Service service, Id? undoGroup = null);

    Task DeleteServiceAsync(Id id, Id? undoGroup = null);

    Task<IReadOnlyList<ServiceType>> ListServiceTypesAsync(uint limit = 200, uint offset = 0);

    Task<ServiceType?> GetServiceTypeAsync(Id id);

    Task SaveServiceTypeAsync(ServiceType serviceType, Id? undoGroup = null);

    Task DeleteServiceTypeAsync(Id id, Id? undoGroup = null);

    Task<IReadOnlyList<Team>> ListTeamsAsync(uint limit = 200, uint offset = 0);

    Task<Team?> GetTeamAsync(Id id);

    Task SaveTeamAsync(Team team, Id? undoGroup = null);

    Task DeleteTeamAsync(Id id, Id? undoGroup = null);

    Task<Service> CreateServiceFromPreviousAsync(CreateServiceFromPrevious request, Id? undoGroup = null);

    Task<ServiceRuntimeProjection> ProjectServiceRuntimeAsync(ProjectServiceRuntime request);

    Task<ServicePrintExport> ExportServicePrintAsync(ExportServicePrint request);
}
