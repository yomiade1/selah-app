import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { create, fromBinary, toBinary } from "@bufbuild/protobuf";
import {
    EnvelopeSchema,
    ActorOrigin,
} from "../dist/gen/selah/v1/envelope_pb.js";
import {
    AccidentalSpelling,
    BibleFormat,
    CopyrightPlacement,
    DocumentType,
    LibraryChangeKind,
    ReferencePlacement,
    SearchScope,
    SectionKind,
    SongPasteFormat,
} from "../dist/gen/selah/v1/library_pb.js";
import { PolicyLevel } from "../dist/gen/selah/v1/settings_pb.js";

test("envelope round trip preserves actor, request and correlation", () => {
    const original = create(EnvelopeSchema, {
        id: 42n,
        correlationId: 7n,
        actor: {
            userId: "local-owner",
            deviceId: "test-device",
            origin: ActorOrigin.UI,
        },
        body: {
            case: "query",
            value: { request: { case: "getStatus", value: {} } },
        },
    });
    assert.deepEqual(
        fromBinary(EnvelopeSchema, toBinary(EnvelopeSchema, original)),
        original,
    );
});

test("truncated length-delimited envelope is rejected", () => {
    assert.throws(() =>
        fromBinary(EnvelopeSchema, new Uint8Array([0x1a, 0x7f, 0x01])),
    );
});

test("wire fixture is shared with Rust and C#", () => {
    const hex = readFileSync(
        new URL(
            "../../../tests/fixtures/proto/status-query.hex",
            import.meta.url,
        ),
        "utf8",
    ).trim();
    const bytes = Buffer.from(hex, "hex");
    const message = fromBinary(EnvelopeSchema, bytes);
    assert.equal(message.id, 42n);
    assert.equal(message.correlationId, 7n);
    assert.equal(message.actor.origin, ActorOrigin.UI);
    assert.equal(
        Buffer.from(toBinary(EnvelopeSchema, message)).toString("hex"),
        hex,
    );
});

test("M2 command, query and event envelopes round trip", () => {
    const docId = new Uint8Array([
        0x01, 0x92, 0x70, 0x00, 0x00, 0x00, 0x70, 0x00, 0x80, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x01,
    ]);
    const undoGroup = new Uint8Array([
        0x01, 0x92, 0x70, 0x00, 0x00, 0x01, 0x70, 0x00, 0x80, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x02,
    ]);
    const saveCmd = create(EnvelopeSchema, {
        id: 1001n,
        actor: {
            userId: "local-owner",
            deviceId: "ui",
            origin: ActorOrigin.UI,
        },
        body: {
            case: "command",
            value: {
                undoGroup: { value: undoGroup },
                action: {
                    case: "saveSong",
                    value: {
                        song: {
                            id: { value: docId },
                            title: "Amazing Grace",
                            publicDomain: true,
                        },
                    },
                },
            },
        },
    });
    assert.deepEqual(
        fromBinary(EnvelopeSchema, toBinary(EnvelopeSchema, saveCmd)),
        saveCmd,
    );

    const policyCmd = create(EnvelopeSchema, {
        id: 1002n,
        body: {
            case: "command",
            value: {
                action: {
                    case: "setPolicySetting",
                    value: {
                        key: "appearance.text-size",
                        valueJson: "130",
                        level: PolicyLevel.MACHINE,
                        locked: true,
                    },
                },
            },
        },
    });
    assert.deepEqual(
        fromBinary(EnvelopeSchema, toBinary(EnvelopeSchema, policyCmd)),
        policyCmd,
    );

    const listOpsQuery = create(EnvelopeSchema, {
        id: 1003n,
        body: {
            case: "query",
            value: {
                request: {
                    case: "listOps",
                    value: {
                        afterSeq: 10n,
                        limit: 50,
                        docId: { value: docId },
                    },
                },
            },
        },
    });
    assert.deepEqual(
        fromBinary(EnvelopeSchema, toBinary(EnvelopeSchema, listOpsQuery)),
        listOpsQuery,
    );

    const libraryEvent = create(EnvelopeSchema, {
        id: 1004n,
        body: {
            case: "event",
            value: {
                notification: {
                    case: "libraryChanged",
                    value: {
                        docType: DocumentType.SONG,
                        docId: { value: docId },
                        change: LibraryChangeKind.SAVED,
                        opSeq: 11n,
                        undoGroup: { value: undoGroup },
                        fromUndoRedo: true,
                    },
                },
            },
        },
    });
    assert.deepEqual(
        fromBinary(EnvelopeSchema, toBinary(EnvelopeSchema, libraryEvent)),
        libraryEvent,
    );
});

test("M2 search queries and results round-trip", () => {
    const docId = new Uint8Array([
        0x01, 0x92, 0x70, 0x00, 0x00, 0x00, 0x70, 0x00, 0x80, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x01,
    ]);

    const searchLibraryQuery = create(EnvelopeSchema, {
        id: 1005n,
        body: {
            case: "query",
            value: {
                request: {
                    case: "searchLibrary",
                    value: {
                        query: "amzing grace",
                        scope: SearchScope.ALL,
                        translation: "KJV",
                        limit: 20,
                    },
                },
            },
        },
    });
    assert.deepEqual(
        fromBinary(
            EnvelopeSchema,
            toBinary(EnvelopeSchema, searchLibraryQuery),
        ),
        searchLibraryQuery,
    );

    const searchReply = create(EnvelopeSchema, {
        id: 1006n,
        correlationId: 1005n,
        body: {
            case: "reply",
            value: {
                result: {
                    case: "librarySearchResults",
                    value: {
                        songs: [
                            {
                                song: {
                                    id: { value: docId },
                                    title: "Amazing Grace",
                                    publicDomain: true,
                                },
                                score: 42.5,
                                snippet: "Amazing grace how sweet the sound",
                                matchedFields: ["title", "lyrics"],
                            },
                        ],
                        scripture: [
                            {
                                translation: "KJV",
                                book: "EPH",
                                bookName: "Ephesians",
                                chapter: 2,
                                verse: 8,
                                text: "For by grace are ye saved through faith",
                                score: 15.0,
                            },
                        ],
                    },
                },
            },
        },
    });
    assert.deepEqual(
        fromBinary(EnvelopeSchema, toBinary(EnvelopeSchema, searchReply)),
        searchReply,
    );
});

test("M2 song parse, transpose and split queries round-trip", () => {
    const secId = new Uint8Array([
        0x01, 0x92, 0x70, 0x00, 0x00, 0x00, 0x70, 0x00, 0x80, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x02,
    ]);

    const parseQuery = create(EnvelopeSchema, {
        id: 1007n,
        body: {
            case: "query",
            value: {
                request: {
                    case: "parsePastedSong",
                    value: {
                        rawText: "[G]Amazing grace",
                        format: SongPasteFormat.CHORD_PRO,
                        defaultTitle: "Amazing Grace",
                    },
                },
            },
        },
    });
    assert.deepEqual(
        fromBinary(EnvelopeSchema, toBinary(EnvelopeSchema, parseQuery)),
        parseQuery,
    );

    const transposeQuery = create(EnvelopeSchema, {
        id: 1010n,
        body: {
            case: "query",
            value: {
                request: {
                    case: "transposeSong",
                    value: {
                        song: {
                            id: { value: secId },
                            title: "Amazing Grace",
                            key: "G",
                        },
                        semitones: 3,
                        accidentalSpelling: AccidentalSpelling.FLATS,
                    },
                },
            },
        },
    });
    assert.deepEqual(
        fromBinary(EnvelopeSchema, toBinary(EnvelopeSchema, transposeQuery)),
        transposeQuery,
    );

    const splitQuery = create(EnvelopeSchema, {
        id: 1008n,
        body: {
            case: "query",
            value: {
                request: {
                    case: "splitSongSlides",
                    value: {
                        song: {
                            id: { value: secId },
                            title: "Amazing Grace",
                            publicDomain: true,
                        },
                        maxLines: 2,
                        copyrightPlacement: CopyrightPlacement.FIRST_AND_LAST,
                        titleSlide: false,
                        blankSlideAtEnd: true,
                        translationLanguage: "yo",
                    },
                },
            },
        },
    });
    assert.deepEqual(
        fromBinary(EnvelopeSchema, toBinary(EnvelopeSchema, splitQuery)),
        splitQuery,
    );

    const splitReply = create(EnvelopeSchema, {
        id: 1009n,
        correlationId: 1008n,
        body: {
            case: "reply",
            value: {
                result: {
                    case: "generatedSongSlides",
                    value: {
                        copyrightLine: "Amazing Grace · Public Domain",
                        slides: [
                            {
                                sectionId: { value: secId },
                                sectionLabel: "Verse 1",
                                sectionKind: SectionKind.VERSE,
                                sectionHotkey: "1",
                                partIndex: 1,
                                partCount: 2,
                                lines: [
                                    {
                                        text: "Amazing grace",
                                        chords: [{ position: 0, text: "G" }],
                                    },
                                ],
                                translationLines: ["Ọọrẹ-ọfẹ"],
                                footerCopyright:
                                    "Amazing Grace · Public Domain",
                            },
                        ],
                    },
                },
            },
        },
    });
    assert.deepEqual(
        fromBinary(EnvelopeSchema, toBinary(EnvelopeSchema, splitReply)),
        splitReply,
    );
});

test("M2 scripture commands, queries and replies round-trip", () => {
    const importCmd = create(EnvelopeSchema, {
        id: 2001n,
        body: {
            case: "command",
            value: {
                action: {
                    case: "importBible",
                    value: {
                        sourcePath: "C:/bibles/sample.usfm",
                        format: BibleFormat.USFM,
                        code: "SAMP",
                        name: "Sample USFM",
                        language: "en",
                        replaceExisting: false,
                    },
                },
            },
        },
    });
    assert.deepEqual(
        fromBinary(EnvelopeSchema, toBinary(EnvelopeSchema, importCmd)),
        importCmd,
    );

    const deleteCmd = create(EnvelopeSchema, {
        id: 2002n,
        body: {
            case: "command",
            value: {
                action: {
                    case: "deleteBible",
                    value: { code: "SAMP" },
                },
            },
        },
    });
    assert.deepEqual(
        fromBinary(EnvelopeSchema, toBinary(EnvelopeSchema, deleteCmd)),
        deleteCmd,
    );

    const parseRefQuery = create(EnvelopeSchema, {
        id: 2003n,
        body: {
            case: "query",
            value: {
                request: {
                    case: "parseScriptureReference",
                    value: {
                        reference: "Jean 3, 16-18",
                        languageHint: "fr",
                    },
                },
            },
        },
    });
    assert.deepEqual(
        fromBinary(EnvelopeSchema, toBinary(EnvelopeSchema, parseRefQuery)),
        parseRefQuery,
    );

    const passagesQuery = create(EnvelopeSchema, {
        id: 2004n,
        body: {
            case: "query",
            value: {
                request: {
                    case: "getScripturePassages",
                    value: {
                        translation: "KJV",
                        reference: "John 3:16",
                        languageHint: "en",
                    },
                },
            },
        },
    });
    assert.deepEqual(
        fromBinary(EnvelopeSchema, toBinary(EnvelopeSchema, passagesQuery)),
        passagesQuery,
    );

    const splitQuery = create(EnvelopeSchema, {
        id: 2005n,
        body: {
            case: "query",
            value: {
                request: {
                    case: "splitScriptureSlides",
                    value: {
                        translation: "KJV",
                        translationName: "King James Version",
                        reference: "John 3:16",
                        maxLines: 4,
                        maxCharsPerLine: 48,
                        versesPerSlide: 1,
                        oneVersePerSlide: true,
                        showVerseNumbers: true,
                        referencePlacement: ReferencePlacement.SLIDE_FOOTER,
                        titleSlide: true,
                        blankSlideAtEnd: false,
                    },
                },
            },
        },
    });
    assert.deepEqual(
        fromBinary(EnvelopeSchema, toBinary(EnvelopeSchema, splitQuery)),
        splitQuery,
    );

    const listReply = create(EnvelopeSchema, {
        id: 2006n,
        correlationId: 2005n,
        body: {
            case: "reply",
            value: {
                result: {
                    case: "bibleList",
                    value: {
                        bibles: [
                            {
                                code: "KJV",
                                name: "King James Version",
                                language: "en",
                                bookCount: 66,
                                verseCount: 31102,
                                isBundled: true,
                                books: [
                                    {
                                        bookNum: 1,
                                        code: "GEN",
                                        name: "Genesis",
                                        chapterCount: 50,
                                        verseCounts: [31, 25],
                                    },
                                ],
                            },
                        ],
                    },
                },
            },
        },
    });
    assert.deepEqual(
        fromBinary(EnvelopeSchema, toBinary(EnvelopeSchema, listReply)),
        listReply,
    );
});

test("M2 media and library exchange messages round-trip", () => {
    const docId = new Uint8Array([
        0x01, 0x92, 0x70, 0x00, 0x00, 0x00, 0x70, 0x00, 0x80, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x09,
    ]);

    const importMediaCmd = create(EnvelopeSchema, {
        id: 3001n,
        body: {
            case: "command",
            value: {
                action: {
                    case: "importMedia",
                    value: {
                        sourcePath: "C:/media/backdrop.png",
                        name: "Sunday Backdrop",
                        tags: ["Background", "Blue"],
                    },
                },
            },
        },
    });
    assert.deepEqual(
        fromBinary(EnvelopeSchema, toBinary(EnvelopeSchema, importMediaCmd)),
        importMediaCmd,
    );

    const thumbQuery = create(EnvelopeSchema, {
        id: 3002n,
        body: {
            case: "query",
            value: {
                request: {
                    case: "getMediaThumbnail",
                    value: {
                        mediaId: { value: docId },
                        maxWidth: 320,
                        maxHeight: 180,
                    },
                },
            },
        },
    });
    assert.deepEqual(
        fromBinary(EnvelopeSchema, toBinary(EnvelopeSchema, thumbQuery)),
        thumbQuery,
    );

    const exportCmd = create(EnvelopeSchema, {
        id: 3003n,
        body: {
            case: "command",
            value: {
                action: {
                    case: "exportSelahBundle",
                    value: {
                        outputPath: "C:/export/sunday.selah",
                        songIds: [{ value: docId }],
                        includeMedia: true,
                    },
                },
            },
        },
    });
    assert.deepEqual(
        fromBinary(EnvelopeSchema, toBinary(EnvelopeSchema, exportCmd)),
        exportCmd,
    );

    const importBundleCmd = create(EnvelopeSchema, {
        id: 3004n,
        body: {
            case: "command",
            value: {
                action: {
                    case: "importSelahBundle",
                    value: {
                        sourcePath: "C:/export/sunday.selah",
                        overwriteExisting: true,
                    },
                },
            },
        },
    });
    assert.deepEqual(
        fromBinary(EnvelopeSchema, toBinary(EnvelopeSchema, importBundleCmd)),
        importBundleCmd,
    );

    const backupCmd = create(EnvelopeSchema, {
        id: 3005n,
        body: {
            case: "command",
            value: {
                action: {
                    case: "createLibraryBackup",
                    value: { outputPath: "C:/backups/lib.zip" },
                },
            },
        },
    });
    assert.deepEqual(
        fromBinary(EnvelopeSchema, toBinary(EnvelopeSchema, backupCmd)),
        backupCmd,
    );

    const restoreCmd = create(EnvelopeSchema, {
        id: 3006n,
        body: {
            case: "command",
            value: {
                action: {
                    case: "restoreLibraryBackup",
                    value: { backupPath: "C:/backups/lib.zip" },
                },
            },
        },
    });
    assert.deepEqual(
        fromBinary(EnvelopeSchema, toBinary(EnvelopeSchema, restoreCmd)),
        restoreCmd,
    );
});
