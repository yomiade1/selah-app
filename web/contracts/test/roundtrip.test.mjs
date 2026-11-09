import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { create, fromBinary, toBinary } from "@bufbuild/protobuf";
import {
    EnvelopeSchema,
    ActorOrigin,
} from "../dist/gen/selah/v1/envelope_pb.js";
import {
    DocumentType,
    LibraryChangeKind,
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
