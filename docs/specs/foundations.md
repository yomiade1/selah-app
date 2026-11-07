# Milestone 1 foundations (in progress)

The repository now declares a Rust 1.91.0 workspace, a .NET 10.0.100
solution, and a Node 22 / pnpm 10.20.0 workspace. None of these declarations
constitutes evidence that the application builds or that milestone 1 is done.

## Setup

On Windows, `./tools/setup.ps1 -CheckOnly` reports missing prerequisites
without installing anything. After H-01 approval, `./tools/setup.ps1 -Install`
installs the required toolchains through winget, including the C++ workload.
Installation may require elevation or a restart depending on the machine.
This is a developer setup requirement; the future Selah installer must remain
per-user and require no elevation.

On Linux/macOS, `bash tools/setup.sh --check` checks the same versions.
Cloud provisioning remains in `tools/cloud-setup.sh`.

NuGet uses locked restore by default. For the initial lockfile only, restore
with `dotnet restore app/Selah.sln -p:UpdatePackageLock=true`, review and commit
the generated `packages.lock.json`, then use locked restore for future runs.
Likewise generate and commit `Cargo.lock` and `pnpm-lock.yaml` before claiming
M1.1 or M1.2 complete. Those lockfiles have not yet been generated.

## Contracts

The six protobuf source files declare the initial common, render, settings,
help, error and envelope types. Field numbers are stable once accepted.
Envelope actors carry attribution; they do not authenticate the sender.
The IPC implementation must bind actors to verified transport peers before
any permission check. It must reject missing actors and unknown commands.

The emergency endpoint has a separate message permitting only ClearEverything
and ShowLogo. Its transport and priority handling are not yet implemented.

`tools/codegen.ps1` runs buf lint, generates TypeScript, and builds the Rust
and C# types. It requires dependencies and lockfiles to be restored first.
Rust uses vendored protoc without modifying process environment variables;
C# uses Grpc.Tools. Neither requires a global protoc installation.

## Outstanding validation

- Restore dependencies, generate lockfiles, and verify all selected package versions.
- Run buf lint and fix any diagnostics before accepting the contracts.
- Compile and run the Rust contract round-trip and malformed-input tests.
- Compile the C# generated contracts and check the TypeScript generated output.
- Implement pinned FFmpeg/font downloads and synthetic media generation.
- Complete M1.3 through M1.13 and the full quality gate.

`python -m unittest discover -s tests/tools -v` checks toolchain failure
handling offline. It is a tooling check, not the milestone quality gate.
