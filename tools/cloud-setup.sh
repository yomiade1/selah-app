#!/usr/bin/env bash
# Provision a Claude Code cloud session (Ubuntu 24.04) for building Selah (ADR-023).
#
#   tools/cloud-setup.sh               full setup (paste this file into the environment's Setup script)
#   tools/cloud-setup.sh --if-missing  quick check; installs only what's missing (used by the SessionStart hook)
#
# Safe to run more than once. A failed step prints a warning and the script carries on, so a
# session can still start; `tools/verify.sh --cloud` reports anything still missing.
# Pinned third-party files (FFmpeg, fonts, Skia) are not fetched here: tools/setup.sh does that,
# with SHA-256 checks against tools/third_party.lock.json.
set -uo pipefail

MODE="full"
[ "${1:-}" = "--if-missing" ] && MODE="if-missing"

# The hook runs in every session, local ones too. Only cloud sessions are provisioned.
if [ "$MODE" = "if-missing" ] && [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

NOREPLY_EMAIL="173577567+yomiade1@users.noreply.github.com"
DOTNET_CHANNEL="10.0"
DOTNET_DIR="$HOME/.dotnet"

SUDO=""
if [ "$(id -u)" -ne 0 ] && command -v sudo >/dev/null 2>&1; then SUDO="sudo"; fi

warn() { echo "cloud-setup: WARNING: $*" >&2; }
note() { [ "$MODE" = "full" ] && echo "cloud-setup: $*"; return 0; }
have() { command -v "$1" >/dev/null 2>&1; }

APT_PACKAGES=(
  build-essential pkg-config clang libclang-dev cmake ninja-build curl ca-certificates
  git jq unzip zstd python3-yaml gh
  libssl-dev libfontconfig-dev libfreetype-dev libasound2-dev libudev-dev
  libx11-dev libxkbcommon-dev libwayland-dev libxcursor-dev libxrandr-dev libxi-dev
  mesa-vulkan-drivers libvulkan1 libegl1 xvfb
  gcc-mingw-w64-x86-64
)

dotnet_ok() { have dotnet && dotnet --list-sdks 2>/dev/null | grep -q "^${DOTNET_CHANNEL%%.*}\."; }

needs_install() {
  dotnet_ok || return 0
  have pnpm || return 0
  have clang || return 0
  have jq || return 0
  have x86_64-w64-mingw32-gcc || return 0
  return 1
}

install_apt() {
  note "installing system packages"
  export DEBIAN_FRONTEND=noninteractive
  $SUDO apt-get update -qq >/dev/null 2>&1 || { warn "apt-get update failed (is archive.ubuntu.com allowed?)"; return; }
  if ! $SUDO apt-get install -y -qq --no-install-recommends "${APT_PACKAGES[@]}" >/dev/null 2>&1; then
    # One unknown package fails the whole batch, so retry one by one.
    for p in "${APT_PACKAGES[@]}"; do
      $SUDO apt-get install -y -qq --no-install-recommends "$p" >/dev/null 2>&1 || warn "apt package not installed: $p"
    done
  fi
}

install_dotnet() {
  dotnet_ok && return
  note "installing the .NET ${DOTNET_CHANNEL} SDK"
  local tmp; tmp="$(mktemp -d)"
  if curl -fsSL https://dot.net/v1/dotnet-install.sh -o "$tmp/dotnet-install.sh"; then
    bash "$tmp/dotnet-install.sh" --channel "$DOTNET_CHANNEL" --install-dir "$DOTNET_DIR" >/dev/null \
      || warn ".NET SDK install failed (check builds.dotnet.microsoft.com is allowed)"
  else
    warn "could not download dotnet-install.sh (check dot.net is allowed)"
  fi
  rm -rf "$tmp"
  if [ -x "$DOTNET_DIR/dotnet" ]; then
    $SUDO ln -sf "$DOTNET_DIR/dotnet" /usr/local/bin/dotnet 2>/dev/null || true
  fi
}

install_pnpm() {
  have pnpm && return
  note "enabling pnpm"
  if have corepack; then
    $SUDO corepack enable pnpm >/dev/null 2>&1 || corepack enable pnpm >/dev/null 2>&1 || true
  fi
  have pnpm || $SUDO npm install -g pnpm >/dev/null 2>&1 || warn "pnpm not installed"
}

setup_rust() {
  if ! have rustup; then warn "rustup not found; Rust must be installed by the base image"; return; fi
  note "adding Rust components"
  rustup component add rustfmt clippy >/dev/null 2>&1 || warn "rustfmt/clippy not added"
  # Windows-only code is type-checked in the cloud: cargo check --target x86_64-pc-windows-gnu
  rustup target add x86_64-pc-windows-gnu >/dev/null 2>&1 || warn "windows-gnu target not added"
  # rust-toolchain.toml (M1.1) pins the toolchain; asking for it installs it now rather than mid-build.
  if [ -f rust-toolchain.toml ]; then rustup show active-toolchain >/dev/null 2>&1 || true; fi
}

export_env() {
  # Persist variables for the session's commands when run from a SessionStart hook.
  local lines=(
    "export DOTNET_ROOT=\"$DOTNET_DIR\""
    "export PATH=\"$DOTNET_DIR:\$HOME/.dotnet/tools:\$PATH\""
    "export DOTNET_CLI_TELEMETRY_OPTOUT=1"
    "export DOTNET_NOLOGO=1"
    "export SELAH_CLOUD=1"
    # skia-bindings reads the pinned Skia build fetched by tools/setup.sh --fetch (SEC-09).
    "export SKIA_BINARIES_URL=\"file://${CLAUDE_PROJECT_DIR:-$PWD}/third_party/skia/linux-x64/skia-binaries-{key}.tar.gz\""
  )
  # In GitHub Actions (the linux CI job runs this script too), later steps read GITHUB_PATH/GITHUB_ENV.
  if [ -n "${GITHUB_PATH:-}" ] && [ -x "$DOTNET_DIR/dotnet" ]; then echo "$DOTNET_DIR" >> "$GITHUB_PATH"; fi
  if [ -n "${GITHUB_ENV:-}" ] && [ -x "$DOTNET_DIR/dotnet" ]; then echo "DOTNET_ROOT=$DOTNET_DIR" >> "$GITHUB_ENV"; fi
  local target
  for target in "${CLAUDE_ENV_FILE:-}" "$HOME/.bashrc"; do
    [ -n "$target" ] || continue
    for l in "${lines[@]}"; do
      grep -qxF "$l" "$target" 2>/dev/null || echo "$l" >> "$target"
    done
  done
}

set_commit_email() {
  # Keep personal email addresses out of commit metadata (docs/CLOUD-DEVELOPMENT.md §3.1).
  git rev-parse --is-inside-work-tree >/dev/null 2>&1 || return
  local current; current="$(git config user.email || true)"
  case "$current" in
    *@users.noreply.github.com|noreply@anthropic.com) ;;
    *) git config user.email "$NOREPLY_EMAIL" ;;
  esac
  git config user.name >/dev/null 2>&1 || git config user.name "yomiade1"
}

cd "${CLAUDE_PROJECT_DIR:-$PWD}" 2>/dev/null || true

if [ "$MODE" = "full" ] || needs_install; then
  install_apt
  install_dotnet
  install_pnpm
  setup_rust
fi
export_env
set_commit_email

missing=()
dotnet_ok || missing+=(".NET ${DOTNET_CHANNEL} SDK")
for t in cargo pnpm clang jq x86_64-w64-mingw32-gcc; do have "$t" || missing+=("$t"); done
if [ ${#missing[@]} -gt 0 ]; then
  warn "still missing: ${missing[*]}"
else
  note "ready: $(cargo --version 2>/dev/null) · .NET $(dotnet --version 2>/dev/null) · pnpm $(pnpm --version 2>/dev/null)"
fi
exit 0
