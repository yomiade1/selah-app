# Command bus (M1, extended in M2.2)

How Selah's processes talk: the envelope, the commands, queries and events, how errors come back, and how the IPC behaves. Contracts: `proto/selah/v1/` (ADR-003).
Transport and security: [foundations.md](foundations.md) §IPC (SEC-02, SEC-03).

## Endpoints

Each user has one namespace (`Endpoint::for_current_user`); tests use their own.

| Endpoint | Server | Clients | Accepts |
|---|---|---|---|
| core | `selah-core` | UI, `selah-bench`, tests | Commands and queries below; `Subscribe` for events |
| engine | `selah-engine` | core | `RenderCommand`s; `GetStatus`; `Subscribe` for `FrameShown` |
| emergency | `selah-engine` | UI | Only `Render{ClearEverything}` and `Render{ShowLogo}` from a UI actor; anything else closes the connection (SAFE-03) |
| watchdog | `selah-watchdog` | every process | `Heartbeat` events (250 ms), `GetWatchdogStatus`, `RestartNormally` from the UI |

Shared memory: the engine writes preview frames to `<namespace>-preview-0.ring` (ADR-004).

## Envelope

```
Envelope { id, correlation_id, actor: Actor { user_id, device_id, origin }, body: Command | Query | Event | Reply | Error }
```

- `id` is unique per sender; a reply carries the request's id in `correlation_id`.
- `actor` is attribution, not proof. The receiver binds it to the transport peer, which
  `selah-ipc` has already checked is the same OS user (SEC-02), then authorises (SEC-14).
- Frames are length-prefixed protobuf, at most 16 MiB.

## Commands (UI → core)

Every command passes `permissions::authorize` first. In M1 the local owner (origins `UI` and
`Tour`) may run every command except `Render`; internal and not-yet-built origins (API, MIDI,
macro, timeline, sync) are refused with `SEL-SEC-0001`. A new command must be added to
`CommandKind`; the exhaustive match makes forgetting it a compile error, and the matrix test
covers every origin × command.

| Command | Changes the screens | Reply | Notes |
|---|---|---|---|
| `ShowTestPattern { pattern }` | yes | `acknowledged` | Live state changes, the scene goes to the engine, status is broadcast |
| `ClearOutputs {}` | yes | `acknowledged` | The normal-path clear (the emergency lane is separate) |
| `SetAppearance { appearance }` | no | `acknowledged` / `SEL-SET-0001` | Text size steps 90–200 % (ADR-009) |
| `RecordFeedback` | no | `acknowledged` | Stored in `help_feedback` from M2.2 |
| `SaveSong`, `DeleteSong`, `SaveTemplate`, `DeleteTemplate`, `SaveMediaItem`, `DeleteMediaItem`, `SavePresentation`, `DeletePresentation`, `SaveTag`, `DeleteTag` | no | `acknowledged` / `SEL-LIB-*` | Transactional library mutations with op log (`ops`), undo/redo inverses (`undo_entries`), idempotence (`command_idempotency`), and optional `undo_group` (`Command.undo_group`, field 100). Emits `LibraryChanged` + `UndoStateChanged` |
| `Undo`, `Redo` | no | `UndoState` / `SEL-LIB-0004` | Optional `doc_id` scopes undo/redo to one document; empty `doc_id` runs globally across undo groups. Emits compensating `LibraryChanged { from_undo_redo = true }` + `UndoStateChanged` |
| `SaveEditorBuffer`, `DiscardEditorBuffer` | no | `acknowledged` / `SEL-LIB-0002` | Crash-safe autosave buffer (`editor_buffers`), not added to undo stack |
| `SetPolicySetting`, `RemovePolicySetting` | no | `acknowledged` / `SEL-LIB-0003` | Policy settings (`User`, `Machine`, `Organisation`); refuses user overrides on locked keys |
| `SetUpdateSettings { update_feed_url, allow_update_check }` | no | `acknowledged` / `SEL-SET-0001` | Sets the network policy (SEC-13) |
| `CheckForUpdate {}` | no | `UpdateCheck` / `SEL-UPD-*` | Slow work |
| `InstallUpdate { version }` | no | `acknowledged` / `SEL-UPD-*` | Refused while live; slow work |
| `CreateSupportBundle {}` | no | `SupportBundle { path, report_url }` / `SEL-SUP-0001` | Slow work |
| `RunDiagnostics { quick }` | no | `DiagnosticsReport` / `SEL-DIAG-*` | Refused while live; slow work |
| `RestartNormally {}` | no | `SEL-IPC-0001` from core | Sent to the watchdog, which restarts core and the UI without safe mode |
| `Render` | — | `SEL-SEC-0001` | Only core produces render commands |

## Queries

| Query | Reply |
|---|---|
| `GetStatus` | `Status { connected, safe_mode, scene_revision, engine_connected, engine_scene_revision, frame_checksum, version, live }` (the engine adds `renderer` and `output_resolution`) |
| `GetSettings` | `Settings` |
| `Subscribe` | `Status`; then the connection receives events |
| `SearchHelp`, `HelpForContext`, `GetArticle` | `HelpResults` / `Article` / `SEL-HELP-0001` |
| `GetDiagnostics` | `Diagnostics` (core asks the engine for its part) |
| `GetSong`, `ListSongs`, `GetTemplate`, `ListTemplates`, `GetMediaItem`, `ListMediaItems`, `GetPresentation`, `ListPresentations`, `GetTag`, `ListTags` | Typed document or list (`1..=200` per page) / `SEL-LIB-0001`, `SEL-LIB-0002` |
| `GetUndoState` | `UndoState { can_undo, can_redo, undo_depth, redo_depth }` |
| `GetEditorBuffer`, `ListEditorBuffers` | `EditorBufferReply` / `EditorBufferList` |
| `ListOps` | `OpList` paginated strictly by `seq > after_seq` (optional `doc_id` filter) |
| `GetPolicySetting` | `GetPolicySettingReply { found, setting }` (effective value, winning policy level, lock flag) |
| `GetWatchdogStatus` | `WatchdogStatus` (watchdog only; core answers `SEL-IPC-0001`) |

## Events

| Event | From | When |
|---|---|---|
| `Status` | core | On any change to what the UI shows |
| `LibraryChanged { doc_type, doc_id, change, op_seq, undo_group, from_undo_redo }` | core | On every committed or undone/redone library mutation (never on idempotent replay) |
| `UndoStateChanged { global, doc_id, document }` | core | After any mutation or undo/redo that updates undo/redo depth |
| `FrameShown { scene_revision, monotonic_nanoseconds, frame_checksum, command_received_nanoseconds, output_index, frame_period_nanoseconds }` | engine (forwarded by core) | Every presented frame of a new scene |
| `EmergencyApplied { show_logo, scene_revision }` | engine | After the emergency lane acted; core raises its revision above the engine's floor so a stale scene can't come back |
| `Heartbeat` | every process → watchdog | Every 250 ms |

Events go to subscribers with `try_send`: a slow UI drops events rather than holding up core.

## Slow work

Commands that touch the network or disk for long (`CheckForUpdate`, `InstallUpdate`,
`CreateSupportBundle`, `RunDiagnostics`, and the `GetDiagnostics` query) return
`Outcome.work` instead of a reply. The host runs the work on a blocking thread, then calls the
matching `finish_*` method on `CoreService`, which builds the reply. Other requests keep being
served meanwhile.

## Errors

`Error { code, message_key, args, fix_article_id }`. Codes are in `content/errors/errors.yaml`,
messages in `content/i18n/en/errors.ftl`, and each has a fix article. The UI shows the message
for `message_key` (`CoreException`), never internal text.

| Area | Codes |
|---|---|
| IPC | `SEL-IPC-0001` unsupported request, `SEL-IPC-0002` invalid request |
| Security | `SEL-SEC-0001` not permitted |
| Settings | `SEL-SET-0001` invalid setting |
| Help | `SEL-HELP-0001` article not found |
| Updates | `SEL-UPD-0001` off, `0002` feed unreadable, `0003` verification failed, `0004` live, `0005` installer didn't start |
| Support | `SEL-SUP-0001` bundle not saved |
| Diagnostics | `SEL-DIAG-0001` live, `SEL-DIAG-0002` benchmark failed |
| Library | `SEL-LIB-0001` item not found, `0002` invalid item or save failure, `0003` setting locked by policy, `0004` nothing to undo or redo |
| App | `SEL-APP-0001` safe mode |
| Outputs | `SEL-OUT-0001` screen not connected, `SEL-OUT-0002` screen unplugged |

## Behaviour

- **Idempotence:** library and policy commands record `(actor_origin, user_id, device_id, envelope_id)` alongside the BLAKE3 hash of the command and its encoded reply in `command_idempotency` (bounded to the 50,000 most recent entries). Replaying the same request returns the cached reply without duplicating ops or subscriber events; reusing an `Envelope.id` for a different command fails with `SEL-IPC-0002`. Both Rust and C# IPC clients seed initial envelope IDs from wall-clock time so IDs do not collide across process restarts while fitting within signed 64-bit SQLite integers.
- **Undo/redo and autosave:** each committed document save or delete appends an `ops` entry and a reversible `undo_entries` snapshot keyed by `undo_group`. Forward edits to one document prune only that document's undone redo stack, leaving other documents' per-document redo stacks intact. Editor drafts persist in `editor_buffers` independently of `undo_entries`.
- **Reconnect:** clients reconnect with backoff; core re-sends the scene to every new engine connection; the UI resubscribes after every core connection.
- **Crashes:** see foundations.md §Processes; core reloads `live_state.bin`, the engine keeps the last frame.
- **Network:** core never opens a connection itself; slow work goes through `selah-net` (SEC-13).
