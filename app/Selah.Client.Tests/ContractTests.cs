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
    }
}
