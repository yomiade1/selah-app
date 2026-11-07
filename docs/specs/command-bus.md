# Command bus (M1)

How Selah's processes talk: the envelope, the commands, queries and events that exist after
M1, how errors come back, and how the IPC behaves. Contracts: `proto/selah/v1/` (ADR-003).
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
| `RecordFeedback` | no | `acknowledged` | Stored from M2 |
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
| `GetWatchdogStatus` | `WatchdogStatus` (watchdog only; core answers `SEL-IPC-0001`) |

## Events

| Event | From | When |
|---|---|---|
| `Status` | core | On any change to what the UI shows |
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
| App | `SEL-APP-0001` safe mode |
| Outputs | `SEL-OUT-0001` screen not connected, `SEL-OUT-0002` screen unplugged |

## Behaviour

- **Idempotence:** commands are idempotent when they carry the same `Envelope.id` (the data model enforces it from M2).
- **Reconnect:** clients reconnect with backoff; core re-sends the scene to every new engine connection; the UI resubscribes after every core connection.
- **Crashes:** see foundations.md §Processes; core reloads `live_state.bin`, the engine keeps the last frame.
- **Network:** core never opens a connection itself; slow work goes through `selah-net` (SEC-13).
