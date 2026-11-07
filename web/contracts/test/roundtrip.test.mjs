import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { create, fromBinary, toBinary } from "@bufbuild/protobuf";
import {
    EnvelopeSchema,
    ActorOrigin,
} from "../dist/gen/selah/v1/envelope_pb.js";

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
