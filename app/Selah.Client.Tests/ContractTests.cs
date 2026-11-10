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
}
