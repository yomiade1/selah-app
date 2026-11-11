using Google.Protobuf;
using Selah.Contracts.V1;
using Xunit;

namespace Selah.Client.Tests;

public sealed class ContractTests
{
    [Fact]
    public void EnvelopeRoundTripPreservesActorAndCorrelation()
    {
        var original = new Envelope
        {
            Id = 42,
            CorrelationId = 7,
            Actor = new Actor
            {
                UserId = "local-owner",
                DeviceId = "test-device",
                Origin = ActorOrigin.Ui,
            },
            Query = new Query { GetStatus = new GetStatus() },
        };

        var decoded = Envelope.Parser.ParseFrom(original.ToByteArray());
        Assert.Equal(original, decoded);
    }

    [Fact]
    public void TruncatedEnvelopeIsRejected()
    {
        Assert.Throws<InvalidProtocolBufferException>(
            () => Envelope.Parser.ParseFrom(new byte[] { 0x1a, 0x7f, 0x01 }));
    }

    [Fact]
    public void WireFixtureIsSharedWithRustAndTypeScript()
    {
        var path = Path.Combine(AppContext.BaseDirectory, "fixtures", "status-query.hex");
        var bytes = Convert.FromHexString(File.ReadAllText(path).Trim());
        var message = Envelope.Parser.ParseFrom(bytes);
        Assert.Equal(42UL, message.Id);
        Assert.Equal(7UL, message.CorrelationId);
        Assert.Equal(ActorOrigin.Ui, message.Actor.Origin);
        Assert.Equal(bytes, message.ToByteArray());
    }

    [Fact]
    public void M2CommandQueryAndEventMessagesRoundTrip()
    {
        var idBytes = ByteString.CopyFrom(new byte[]
        {
            0x01, 0x92, 0x70, 0x00, 0x00, 0x00, 0x70, 0x00,
            0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
        });
        var groupBytes = ByteString.CopyFrom(new byte[]
        {
            0x01, 0x92, 0x70, 0x00, 0x00, 0x01, 0x70, 0x00,
            0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02,
        });
        var saveCmd = new Envelope
        {
            Id = 1001,
            Actor = new Actor { UserId = "local-owner", DeviceId = "ui", Origin = ActorOrigin.Ui },
            Command = new Command
            {
                UndoGroup = new Id { Value = groupBytes },
                SaveSong = new SaveSong
                {
                    Song = new Song
                    {
                        Id = new Id { Value = idBytes },
                        Title = "Amazing Grace",
                        PublicDomain = true,
                    },
                },
            },
        };
        Assert.Equal(saveCmd, Envelope.Parser.ParseFrom(saveCmd.ToByteArray()));

        var policyCmd = new Envelope
        {
            Id = 1002,
            Command = new Command
            {
                SetPolicySetting = new SetPolicySetting
                {
                    Key = "appearance.text-size",
                    ValueJson = "130",
                    Level = PolicyLevel.Machine,
                    Locked = true,
                },
            },
        };
        Assert.Equal(policyCmd, Envelope.Parser.ParseFrom(policyCmd.ToByteArray()));

        var listOpsQuery = new Envelope
        {
            Id = 1003,
            Query = new Query
            {
                ListOps = new ListOps { AfterSeq = 10, Limit = 50, DocId = new Id { Value = idBytes } },
            },
        };
        Assert.Equal(listOpsQuery, Envelope.Parser.ParseFrom(listOpsQuery.ToByteArray()));

        var libraryEvent = new Envelope
        {
            Id = 1004,
            Event = new Event
            {
                LibraryChanged = new LibraryChanged
                {
                    DocType = DocumentType.Song,
                    DocId = new Id { Value = idBytes },
                    Change = LibraryChangeKind.Saved,
                    OpSeq = 11,
                    UndoGroup = new Id { Value = groupBytes },
                    FromUndoRedo = true,
                },
            },
        };
        Assert.Equal(libraryEvent, Envelope.Parser.ParseFrom(libraryEvent.ToByteArray()));
    }

    [Fact]
    public void M2SearchQueriesAndRepliesRoundTrip()
    {
        var idBytes = ByteString.CopyFrom(new byte[]
        {
            0x01, 0x92, 0x70, 0x00, 0x00, 0x00, 0x70, 0x00,
            0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
        });
        var searchSongsQuery = new Envelope
        {
            Id = 1005,
            Query = new Query
            {
                SearchSongs = new SearchSongs { Query = "amzing grace", Limit = 20, Offset = 0 },
            },
        };
        Assert.Equal(searchSongsQuery, Envelope.Parser.ParseFrom(searchSongsQuery.ToByteArray()));

        var searchMediaQuery = new Envelope
        {
            Id = 1006,
            Query = new Query
            {
                SearchMedia = new SearchMedia
                {
                    Query = "sunset backdrop",
                    Kind = MediaKind.Image,
                    Limit = 20,
                    Offset = 0,
                },
            },
        };
        Assert.Equal(searchMediaQuery, Envelope.Parser.ParseFrom(searchMediaQuery.ToByteArray()));

        var searchScriptureQuery = new Envelope
        {
            Id = 1007,
            Query = new Query
            {
                SearchScripture = new SearchScripture
                {
                    Query = "charity",
                    Translation = "KJV",
                    Limit = 20,
                    Offset = 0,
                },
            },
        };
        Assert.Equal(searchScriptureQuery, Envelope.Parser.ParseFrom(searchScriptureQuery.ToByteArray()));

        var searchLibraryQuery = new Envelope
        {
            Id = 1008,
            Query = new Query
            {
                SearchLibrary = new SearchLibrary
                {
                    Query = "grace",
                    Scope = SearchScope.All,
                    Translation = "KJV",
                    Limit = 10,
                },
            },
        };
        Assert.Equal(searchLibraryQuery, Envelope.Parser.ParseFrom(searchLibraryQuery.ToByteArray()));

        var searchReply = new Envelope
        {
            Id = 1009,
            CorrelationId = 1008,
            Reply = new Reply
            {
                LibrarySearchResults = new LibrarySearchResults
                {
                    Songs =
                    {
                        new SongSearchHit
                        {
                            Song = new Song
                            {
                                Id = new Id { Value = idBytes },
                                Title = "Amazing Grace",
                                PublicDomain = true,
                            },
                            Score = 42.5,
                            Snippet = "Amazing grace how sweet the sound",
                            MatchedFields = { "title", "lyrics" },
                        },
                    },
                    Scripture =
                    {
                        new ScriptureVerseHit
                        {
                            Translation = "KJV",
                            Book = "EPH",
                            BookName = "Ephesians",
                            Chapter = 2,
                            Verse = 8,
                            Text = "For by grace are ye saved through faith",
                            Score = 15.0,
                        },
                    },
                },
            },
        };
        Assert.Equal(searchReply, Envelope.Parser.ParseFrom(searchReply.ToByteArray()));

        var parseQuery = new Envelope
        {
            Id = 1010,
            Query = new Query
            {
                ParsePastedSong = new ParsePastedSong
                {
                    RawText = "{title: Amazing Grace}\n{sov}\n[G]Amazing grace\n{eov}",
                    Format = SongPasteFormat.ChordPro,
                    DefaultTitle = "Amazing Grace",
                },
            },
        };
        Assert.Equal(parseQuery, Envelope.Parser.ParseFrom(parseQuery.ToByteArray()));

        var transposeQuery = new Envelope
        {
            Id = 1011,
            Query = new Query
            {
                TransposeSong = new TransposeSong
                {
                    Song = new Song { Id = new Id { Value = idBytes }, Title = "Amazing Grace", Key = "G" },
                    Semitones = 3,
                    AccidentalSpelling = AccidentalSpelling.Flats,
                },
            },
        };
        Assert.Equal(transposeQuery, Envelope.Parser.ParseFrom(transposeQuery.ToByteArray()));

        var splitReply = new Envelope
        {
            Id = 1012,
            CorrelationId = 1010,
            Reply = new Reply
            {
                GeneratedSongSlides = new GeneratedSongSlides
                {
                    CopyrightLine = "“Amazing Grace” · John Newton · Public Domain",
                    Slides =
                    {
                        new GeneratedSlide
                        {
                            SectionId = new Id { Value = idBytes },
                            SectionLabel = "Verse 1",
                            SectionKind = SectionKind.Verse,
                            SectionHotkey = "1",
                            PartIndex = 1,
                            PartCount = 2,
                            Lines =
                            {
                                new LyricLine
                                {
                                    Text = "Amazing grace",
                                    Chords = { new Chord { Position = 0, Text = "G" } },
                                },
                            },
                            FooterCopyright = "“Amazing Grace” · John Newton · Public Domain",
                        },
                    },
                },
            },
        };
        Assert.Equal(splitReply, Envelope.Parser.ParseFrom(splitReply.ToByteArray()));
    }

    [Fact]
    public void M2ScriptureMessagesRoundTrip()
    {
        var importCmd = new Envelope
        {
            Id = 2001,
            Command = new Command
            {
                ImportBible = new ImportBible
                {
                    SourcePath = "C:/bibles/sample.osis",
                    Format = BibleFormat.Osis,
                    Code = "SAMP",
                    Name = "Sample OSIS",
                    Language = "en",
                    ReplaceExisting = true,
                },
            },
        };
        Assert.Equal(importCmd, Envelope.Parser.ParseFrom(importCmd.ToByteArray()));

        var deleteCmd = new Envelope
        {
            Id = 2002,
            Command = new Command { DeleteBible = new DeleteBible { Code = "SAMP" } },
        };
        Assert.Equal(deleteCmd, Envelope.Parser.ParseFrom(deleteCmd.ToByteArray()));

        var parseRefQuery = new Envelope
        {
            Id = 2003,
            Query = new Query
            {
                ParseScriptureReference = new ParseScriptureReference
                {
                    Reference = "Jean 3, 16-18",
                    LanguageHint = "fr",
                },
            },
        };
        Assert.Equal(parseRefQuery, Envelope.Parser.ParseFrom(parseRefQuery.ToByteArray()));

        var passagesQuery = new Envelope
        {
            Id = 2004,
            Query = new Query
            {
                GetScripturePassages = new GetScripturePassages
                {
                    Translation = "KJV",
                    Reference = "John 3:16",
                    LanguageHint = "en",
                },
            },
        };
        Assert.Equal(passagesQuery, Envelope.Parser.ParseFrom(passagesQuery.ToByteArray()));

        var listBiblesQuery = new Envelope
        {
            Id = 2005,
            Query = new Query { ListBibles = new ListBibles { IncludeBooks = true } },
        };
        Assert.Equal(listBiblesQuery, Envelope.Parser.ParseFrom(listBiblesQuery.ToByteArray()));

        var splitQuery = new Envelope
        {
            Id = 2006,
            Query = new Query
            {
                SplitScriptureSlides = new SplitScriptureSlides
                {
                    Translation = "KJV",
                    TranslationName = "King James Version",
                    Reference = "John 3:16",
                    MaxLines = 4,
                    MaxCharsPerLine = 48,
                    VersesPerSlide = 1,
                    OneVersePerSlide = true,
                    ShowVerseNumbers = true,
                    ReferencePlacement = ReferencePlacement.SlideFooter,
                    TitleSlide = true,
                    BlankSlideAtEnd = false,
                },
            },
        };
        Assert.Equal(splitQuery, Envelope.Parser.ParseFrom(splitQuery.ToByteArray()));

        var bibleReply = new Envelope
        {
            Id = 2007,
            CorrelationId = 2005,
            Reply = new Reply
            {
                BibleList = new BibleList
                {
                    Bibles =
                    {
                        new BibleTranslation
                        {
                            Code = "KJV",
                            Name = "King James Version",
                            Language = "en",
                            BookCount = 66,
                            VerseCount = 31102,
                            IsBundled = true,
                            Books =
                            {
                                new BibleBookInfo
                                {
                                    BookNum = 1,
                                    Code = "GEN",
                                    Name = "Genesis",
                                    ChapterCount = 50,
                                    VerseCounts = { 31, 25 },
                                },
                            },
                        },
                    },
                },
            },
        };
        Assert.Equal(bibleReply, Envelope.Parser.ParseFrom(bibleReply.ToByteArray()));
    }

    [Fact]
    public void M2MediaAndExchangeMessagesRoundTrip()
    {
        var idBytes = ByteString.CopyFrom(new byte[]
        {
            0x01, 0x92, 0x70, 0x00, 0x00, 0x00, 0x70, 0x00,
            0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x09,
        });

        var importMedia = new Envelope
        {
            Id = 3001,
            Command = new Command
            {
                ImportMedia = new ImportMedia
                {
                    SourcePath = "C:/media/backdrop.png",
                    Name = "Sunday Backdrop",
                    Tags = { "Background", "Blue" },
                },
            },
        };
        Assert.Equal(importMedia, Envelope.Parser.ParseFrom(importMedia.ToByteArray()));

        var thumbQuery = new Envelope
        {
            Id = 3002,
            Query = new Query
            {
                GetMediaThumbnail = new GetMediaThumbnail
                {
                    MediaId = new Id { Value = idBytes },
                    MaxWidth = 320,
                    MaxHeight = 180,
                },
            },
        };
        Assert.Equal(thumbQuery, Envelope.Parser.ParseFrom(thumbQuery.ToByteArray()));

        var thumbReply = new Envelope
        {
            Id = 3003,
            CorrelationId = 3002,
            Reply = new Reply
            {
                MediaThumbnail = new MediaThumbnail
                {
                    MediaId = new Id { Value = idBytes },
                    Width = 320,
                    Height = 180,
                    PngBytes = ByteString.CopyFrom(new byte[] { 0x89, 0x50, 0x4E, 0x47 }),
                },
            },
        };
        Assert.Equal(thumbReply, Envelope.Parser.ParseFrom(thumbReply.ToByteArray()));

        var exportCmd = new Envelope
        {
            Id = 3004,
            Command = new Command
            {
                ExportSelahBundle = new ExportSelahBundle
                {
                    OutputPath = "C:/export/sunday.selah",
                    SongIds = { new Id { Value = idBytes } },
                    IncludeMedia = true,
                },
            },
        };
        Assert.Equal(exportCmd, Envelope.Parser.ParseFrom(exportCmd.ToByteArray()));

        var exportReply = new Envelope
        {
            Id = 3005,
            CorrelationId = 3004,
            Reply = new Reply
            {
                ExportedSelahBundle = new ExportedSelahBundle
                {
                    OutputPath = "C:/export/sunday.selah",
                    CanonicalManifestJson = "{\"format\":\"selah-bundle\",\"schema_version\":1}",
                    SongCount = 1,
                    MediaCount = 2,
                    TotalBytes = 4096,
                },
            },
        };
        Assert.Equal(exportReply, Envelope.Parser.ParseFrom(exportReply.ToByteArray()));

        var importBundleCmd = new Envelope
        {
            Id = 3006,
            Command = new Command
            {
                ImportSelahBundle = new ImportSelahBundle
                {
                    SourcePath = "C:/export/sunday.selah",
                    OverwriteExisting = true,
                },
            },
        };
        Assert.Equal(importBundleCmd, Envelope.Parser.ParseFrom(importBundleCmd.ToByteArray()));

        var backupCmd = new Envelope
        {
            Id = 3007,
            Command = new Command
            {
                CreateLibraryBackup = new CreateLibraryBackup { OutputPath = "C:/backups/lib.zip" },
            },
        };
        Assert.Equal(backupCmd, Envelope.Parser.ParseFrom(backupCmd.ToByteArray()));

        var restoreCmd = new Envelope
        {
            Id = 3008,
            Command = new Command
            {
                RestoreLibraryBackup = new RestoreLibraryBackup { BackupPath = "C:/backups/lib.zip" },
            },
        };
        Assert.Equal(restoreCmd, Envelope.Parser.ParseFrom(restoreCmd.ToByteArray()));

        var restoreReply = new Envelope
        {
            Id = 3009,
            CorrelationId = 3008,
            Reply = new Reply
            {
                RestoreLibraryBackupResult = new RestoreLibraryBackupResult
                {
                    SchemaVersion = 3,
                    SongCount = 5,
                    MediaCount = 2,
                    BibleCount = 2,
                },
            },
        };
        Assert.Equal(restoreReply, Envelope.Parser.ParseFrom(restoreReply.ToByteArray()));
    }

    [Fact]
    public void M3MultiOutputAndLayerMessagesRoundTrip()
    {
        var cfgCmd = new Envelope
        {
            Id = 4001,
            Command = new Command
            {
                SetOutputConfigs = new SetOutputConfigs
                {
                    Outputs =
                    {
                        new OutputConfig
                        {
                            OutputIndex = 0,
                            Name = "Main Hall",
                            Role = OutputRole.Main,
                            Resolution = new Resolution { Width = 1920, Height = 1080 },
                            Enabled = true,
                            RefreshRateMhz = 60000,
                        },
                        new OutputConfig
                        {
                            OutputIndex = 1,
                            Name = "LED Wall",
                            Role = OutputRole.Main,
                            Resolution = new Resolution { Width = 3840, Height = 1080 },
                            Enabled = true,
                            RefreshRateMhz = 60000,
                        },
                    },
                },
            },
        };
        Assert.Equal(cfgCmd, Envelope.Parser.ParseFrom(cfgCmd.ToByteArray()));

        var presentCmd = new Envelope
        {
            Id = 4002,
            Command = new Command
            {
                PresentScene = new PresentScene
                {
                    Scene = new Scene
                    {
                        Revision = 12,
                        Outputs =
                        {
                            new OutputScene
                            {
                                OutputIndex = 0,
                                Role = OutputRole.Main,
                                Resolution = new Resolution { Width = 1920, Height = 1080 },
                                Transition = new TransitionSpec
                                {
                                    Kind = TransitionKind.Fade,
                                    DurationMs = 400,
                                    Easing = TransitionEasing.EaseInOut,
                                },
                                Layers =
                                {
                                    new LayerSpec
                                    {
                                        Kind = LayerKind.Media,
                                        Enabled = true,
                                        Media = new MediaLayerSpec
                                        {
                                            Gradient = new GradientSpec
                                            {
                                                Kind = GradientKind.Linear,
                                                AngleDegrees = 45,
                                                Stops =
                                                {
                                                    new ColorStop
                                                    {
                                                        Color = new Color { Red = 0.1f, Green = 0.2f, Blue = 0.5f, Alpha = 1.0f },
                                                        Position = 0.0f,
                                                    },
                                                    new ColorStop
                                                    {
                                                        Color = new Color { Red = 0.0f, Green = 0.0f, Blue = 0.1f, Alpha = 1.0f },
                                                        Position = 1.0f,
                                                    },
                                                },
                                            },
                                            Fit = MediaFitMode.Fill,
                                        },
                                    },
                                    new LayerSpec
                                    {
                                        Kind = LayerKind.Slide,
                                        Enabled = true,
                                        Slide = new SlideLayerSpec
                                        {
                                            Text = "Great is Thy faithfulness",
                                            TitleText = "Hymn",
                                        },
                                    },
                                    new LayerSpec
                                    {
                                        Kind = LayerKind.Props,
                                        Enabled = true,
                                        Props = new PropsLayerSpec { Label = "LIVE" },
                                    },
                                    new LayerSpec
                                    {
                                        Kind = LayerKind.Messages,
                                        Enabled = true,
                                        Messages = new MessagesLayerSpec { Text = "Nursery 4", Ticker = false },
                                    },
                                },
                            },
                        },
                        PreloadItems =
                        {
                            new PreloadHint
                            {
                                Key = "slide-2",
                                OutputIndex = 0,
                                Resolution = new Resolution { Width = 1920, Height = 1080 },
                                Text = "Morning by morning",
                            },
                        },
                    },
                },
            },
        };
        Assert.Equal(presentCmd, Envelope.Parser.ParseFrom(presentCmd.ToByteArray()));

        var statusReply = new Envelope
        {
            Id = 4003,
            CorrelationId = 4002,
            Reply = new Reply
            {
                Status = new Status
                {
                    Connected = true,
                    Live = true,
                    SceneRevision = 12,
                    PreviewSceneRevision = 4,
                    OutputStatuses =
                    {
                        new OutputFrameStatus
                        {
                            OutputIndex = 0,
                            SceneRevision = 12,
                            Resolution = new Resolution { Width = 1920, Height = 1080 },
                            Live = true,
                        },
                    },
                },
            },
        };
        Assert.Equal(statusReply, Envelope.Parser.ParseFrom(statusReply.ToByteArray()));
    }

    [Fact]
    public void M3MetricFitMessagesRoundTrip()
    {
        var tmplId = ByteString.CopyFrom(new byte[]
        {
            0x01, 0x92, 0x70, 0x00, 0x00, 0x00, 0x70, 0x00,
            0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x11,
        });

        var splitSongQuery = new Envelope
        {
            Id = 4101,
            Query = new Query
            {
                SplitSongSlides = new SplitSongSlides
                {
                    Song = new Song { Id = new Id { Value = tmplId }, Title = "Grace" },
                    MaxLines = 4,
                    ShrinkToFit = true,
                    TemplateId = new Id { Value = tmplId },
                    StyleJson = "{\"name\":\"Sanctuary\"}",
                    TargetOutputs =
                    {
                        new FitTargetOutput
                        {
                            OutputIndex = 0,
                            Name = "Main",
                            Role = OutputRole.Main,
                            Resolution = new Resolution { Width = 1920, Height = 1080 },
                            MaxLinesOverride = 4,
                        },
                        new FitTargetOutput
                        {
                            OutputIndex = 1,
                            Name = "LED",
                            Role = OutputRole.Main,
                            Resolution = new Resolution { Width = 3840, Height = 1080 },
                            MaxLinesOverride = 2,
                        },
                    },
                },
            },
        };
        Assert.Equal(splitSongQuery, Envelope.Parser.ParseFrom(splitSongQuery.ToByteArray()));

        var splitSongReply = new Envelope
        {
            Id = 4102,
            CorrelationId = 4101,
            Reply = new Reply
            {
                GeneratedSongSlides = new GeneratedSongSlides
                {
                    CopyrightLine = "Grace",
                    Slides =
                    {
                        new GeneratedSlide
                        {
                            SectionLabel = "Verse 1",
                            PartIndex = 1,
                            PartCount = 1,
                            AppliedSize = 64.0f,
                            Lines = { new LyricLine { Text = "Amazing grace" } },
                        },
                    },
                    FitReports =
                    {
                        new OutputFitReport
                        {
                            OutputIndex = 0,
                            OutputName = "Main",
                            Resolution = new Resolution { Width = 1920, Height = 1080 },
                            MatchedVariant = "16:9",
                            Fits = true,
                            AppliedSize = 64.0f,
                            MinSize = 32.0f,
                            OriginalSlideCount = 1,
                            FinalSlideCount = 1,
                            ExtraSplits = 0,
                            OverflowsSafeArea = false,
                            Summary = "Fits",
                        },
                    },
                    PerOutputSlides =
                    {
                        new OutputFittedSongSlides
                        {
                            OutputIndex = 0,
                            Slides =
                            {
                                new GeneratedSlide { SectionLabel = "Verse 1", AppliedSize = 64.0f },
                            },
                        },
                    },
                },
            },
        };
        Assert.Equal(splitSongReply, Envelope.Parser.ParseFrom(splitSongReply.ToByteArray()));

        var splitScriptureReply = new Envelope
        {
            Id = 4103,
            CorrelationId = 4101,
            Reply = new Reply
            {
                GeneratedScriptureSlides = new GeneratedScriptureSlides
                {
                    Translation = "KJV",
                    Slides =
                    {
                        new GeneratedScriptureSlide
                        {
                            Reference = "John 3:16 (KJV)",
                            PartIndex = 1,
                            PartCount = 1,
                            AppliedSize = 56.0f,
                            Lines = { "For God so loved the world" },
                        },
                    },
                    FitReports =
                    {
                        new OutputFitReport
                        {
                            OutputIndex = 0,
                            OutputName = "Main",
                            Resolution = new Resolution { Width = 1920, Height = 1080 },
                            Fits = true,
                            AppliedSize = 56.0f,
                            MinSize = 28.0f,
                            OriginalSlideCount = 1,
                            FinalSlideCount = 1,
                            WarningCodes = { "SHRUNK_TO_FIT" },
                            Summary = "Shrunk to fit",
                        },
                    },
                    PerOutputSlides =
                    {
                        new OutputFittedScriptureSlides
                        {
                            OutputIndex = 0,
                            Slides = { new GeneratedScriptureSlide { AppliedSize = 56.0f } },
                        },
                    },
                },
            },
        };
        Assert.Equal(splitScriptureReply, Envelope.Parser.ParseFrom(splitScriptureReply.ToByteArray()));
    }

    [Fact]
    public void M35FlashWarningAndTransitionsEffectsRoundTrip()
    {
        var flashQuery = new Envelope
        {
            Id = 5101,
            Query = new Query
            {
                AnalyzeFlashRisk = new AnalyzeFlashRisk
                {
                    Transition = new TransitionSpec
                    {
                        Kind = TransitionKind.FadeThroughBlack,
                        DurationMs = 200,
                        Direction = TransitionDirection.Left,
                        Easing = TransitionEasing.EaseInOut,
                    },
                    RepeatCadenceMs = 150,
                    FlashesPerSecondHint = 5.0f,
                },
            },
        };
        Assert.Equal(flashQuery, Envelope.Parser.ParseFrom(flashQuery.ToByteArray()));

        var flashReply = new Envelope
        {
            Id = 5102,
            CorrelationId = 5101,
            Reply = new Reply
            {
                FlashRiskReport = new FlashRiskReport
                {
                    FlashWarning = true,
                    FlashesPerSecond = 5.0f,
                    SaturatedRedRisk = false,
                    Reason = "Exceeds 3 flashes per second",
                    WarningCodes = { "FLASH_WARNING" },
                },
            },
        };
        Assert.Equal(flashReply, Envelope.Parser.ParseFrom(flashReply.ToByteArray()));

        var statusWithFlash = new Envelope
        {
            Id = 5103,
            Event = new Event
            {
                Status = new Status
                {
                    Connected = true,
                    Live = true,
                    FlashWarning = true,
                    OutputStatuses =
                    {
                        new OutputFrameStatus
                        {
                            OutputIndex = 0,
                            SceneRevision = 9,
                            Live = true,
                            FlashWarning = true,
                        },
                    },
                },
            },
        };
        Assert.Equal(statusWithFlash, Envelope.Parser.ParseFrom(statusWithFlash.ToByteArray()));
    }

    [Fact]
    public void M36SceneThumbnailRoundTrip()
    {
        var thumbQuery = new Envelope
        {
            Id = 6101,
            Query = new Query
            {
                RenderSceneThumbnail = new RenderSceneThumbnail
                {
                    OutputIndex = 0,
                    MaxWidth = 320,
                    MaxHeight = 180,
                    Scene = new Scene
                    {
                        Revision = 3,
                        Outputs =
                        {
                            new OutputScene
                            {
                                OutputIndex = 0,
                                Role = OutputRole.Main,
                                Resolution = new Resolution { Width = 1920, Height = 1080 },
                                Layers =
                                {
                                    new LayerSpec
                                    {
                                        Kind = LayerKind.Slide,
                                        Enabled = true,
                                        Slide = new SlideLayerSpec { Text = "Blessed Assurance" },
                                    },
                                },
                            },
                        },
                    },
                },
            },
        };
        Assert.Equal(thumbQuery, Envelope.Parser.ParseFrom(thumbQuery.ToByteArray()));

        var thumbReply = new Envelope
        {
            Id = 6102,
            CorrelationId = 6101,
            Reply = new Reply
            {
                SceneThumbnail = new SceneThumbnail
                {
                    PngBytes = ByteString.CopyFrom(new byte[] { 0x89, 0x50, 0x4E, 0x47 }),
                    Width = 320,
                    Height = 180,
                    CacheKey = "abc123",
                    FlashWarning = true,
                },
            },
        };
        Assert.Equal(thumbReply, Envelope.Parser.ParseFrom(thumbReply.ToByteArray()));

        var diagReply = new Envelope
        {
            Id = 6103,
            CorrelationId = 6101,
            Reply = new Reply
            {
                DiagnosticsReport = new DiagnosticsReport
                {
                    Path = "C:/support/selah-diagnostics.json",
                    IpcP99Ms = 0.096,
                    IpcPass = true,
                    VideoClip = "hevc-2160p60.mp4",
                    VideoDecode = "d3d11va",
                    VideoFrames = 60,
                    VideoDropped = 0,
                    VideoCompositeP99Ms = 13.5,
                    VideoPass = false,
                    M3P99Ms = 14.8,
                    M3Pass = false,
                },
            },
        };
        Assert.Equal(diagReply, Envelope.Parser.ParseFrom(diagReply.ToByteArray()));
    }

    [Fact]
    public void M41ServicePlannerMessagesRoundTrip()
    {
        var idBytes = ByteString.CopyFrom(new byte[]
        {
            0x01, 0x92, 0x70, 0x00, 0x00, 0x00, 0x70, 0x00,
            0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
        });
        var saveCmd = new Envelope
        {
            Id = 7101,
            Command = new Command
            {
                SaveService = new SaveService
                {
                    Service = new Service
                    {
                        Id = new Id { Value = idBytes },
                        Name = "Sunday Morning",
                        Series = "Romans",
                        Location = "Sanctuary",
                        Items =
                        {
                            new ServiceItem
                            {
                                Id = new Id { Value = idBytes },
                                Kind = ServiceItemKind.Countdown,
                                Title = "Countdown",
                                CountdownSeconds = 300,
                                DurationSeconds = 300,
                                StatusTag = "Ready",
                            },
                            new ServiceItem
                            {
                                Id = new Id { Value = idBytes },
                                Kind = ServiceItemKind.Header,
                                Title = "Worship",
                            },
                            new ServiceItem
                            {
                                Id = new Id { Value = idBytes },
                                Kind = ServiceItemKind.Audio,
                                Title = "Pad in G",
                                DurationSeconds = 180,
                            },
                        },
                    },
                },
            },
        };
        Assert.Equal(saveCmd, Envelope.Parser.ParseFrom(saveCmd.ToByteArray()));

        var projReply = new Envelope
        {
            Id = 7102,
            CorrelationId = 7101,
            Reply = new Reply
            {
                ServiceRuntimeProjection = new ServiceRuntimeProjection
                {
                    TotalDurationSeconds = 4500,
                    FormattedStartTime = "09:00",
                    FormattedEndTime = "10:15",
                    FormattedTotalDuration = "1 hr 15 min",
                    Items =
                    {
                        new ProjectedServiceItem
                        {
                            ItemId = new Id { Value = idBytes },
                            Index = 0,
                            DurationSeconds = 300,
                            FormattedTime = "09:00",
                            FormattedDuration = "5:00",
                        },
                    },
                },
            },
        };
        Assert.Equal(projReply, Envelope.Parser.ParseFrom(projReply.ToByteArray()));

        var exportCmd = new Envelope
        {
            Id = 7103,
            Command = new Command
            {
                ExportServicePrint = new ExportServicePrint
                {
                    ServiceId = new Id { Value = idBytes },
                    Kind = ServicePrintKind.Both,
                    OutputPath = "C:/temp/service.pdf",
                },
            },
        };
        Assert.Equal(exportCmd, Envelope.Parser.ParseFrom(exportCmd.ToByteArray()));
    }

    [Fact]
    public void M42LiveStateMachineMessagesRoundTrip()
    {
        var idBytes = Google.Protobuf.ByteString.CopyFrom(new byte[]
        {
            0x01, 0x92, 0x70, 0x00, 0x00, 0x00, 0x70, 0x00,
            0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x42,
        });

        var triggerCmd = new Envelope
        {
            Id = 7201,
            Command = new Command
            {
                TriggerSlide = new TriggerSlide
                {
                    ItemIndex = 1,
                    SlideIndex = 2,
                },
            },
        };
        Assert.Equal(triggerCmd, Envelope.Parser.ParseFrom(triggerCmd.ToByteArray()));

        var hotkeyCmd = new Envelope
        {
            Id = 7202,
            Command = new Command
            {
                TriggerSectionHotkey = new TriggerSectionHotkey { Hotkey = "C" },
            },
        };
        Assert.Equal(hotkeyCmd, Envelope.Parser.ParseFrom(hotkeyCmd.ToByteArray()));

        var liveStatusReply = new Envelope
        {
            Id = 7203,
            CorrelationId = 7201,
            Reply = new Reply
            {
                LiveStatus = new LiveStatus
                {
                    ServiceId = new Id { Value = idBytes },
                    ServiceName = "Sunday Morning",
                    HasActiveItem = true,
                    CurrentItemIndex = 1,
                    CurrentSlideIndex = 2,
                    HasNext = true,
                    NextItemIndex = 1,
                    NextSlideIndex = 3,
                    CurrentItemTitle = "Cornerstone",
                    CurrentSlideLabel = "Chorus",
                    CurrentSlideText = "Christ alone, Cornerstone",
                    SongKey = "D",
                    CanLiveUndo = true,
                    SceneRevision = 42,
                    Audio = new AudioLayerStatus
                    {
                        Playing = true,
                        MediaId = new Id { Value = idBytes },
                        Title = "Warm Pad.wav",
                        Volume = 0.8f,
                        LoopAudio = true,
                    },
                },
            },
        };
        Assert.Equal(liveStatusReply, Envelope.Parser.ParseFrom(liveStatusReply.ToByteArray()));
    }
}
