#!/usr/bin/env bash
#
# Build every platform and push it to itch.io from this machine — the packages the release
# workflow (.github/workflows/release.yaml) makes, without GitHub Actions and without anybody
# pasting a secret into a chat.
#
#   ./scripts/deploy-itch.sh                  # all four channels (the normal case)
#   ./scripts/deploy-itch.sh web              # one, or several: web mac linux windows
#   ./scripts/deploy-itch.sh --dry-run        # build, check and package, do not push
#   ./scripts/deploy-itch.sh --allow-behind   # build even though origin/main has moved on
#
# All four is the default on purpose: a changed networked component or wire name breaks the join
# handshake, and a web build and a native download from different commits cannot race each other.
#
# Secrets come from `.env.deploy` at the repo root, which is gitignored and which only you write
# (`.env.deploy.example` is the committed template):
#
#   TURN_PASSWORD=...                                     # required
#   SIGNALLING_SERVER_URL=wss://signal.sigma-dev.eu/ws    # optional, this is the default
#   TURN_URL=turn:signal.sigma-dev.eu:3478                # optional, this is the default
#   TURN_USER=bevy_kart                                   # optional, this is the default
#   ITCH_PAGE=sigmatronic/bevy-kart                       # optional, this is the default
#
# The password never appears on a command line: the host builds read it from the environment and
# Docker is handed the variable *names* (`-e TURN_PASSWORD`), so a process list shows nothing.
#
# Needs, once:
#   - butler, logged in (`butler login`)
#   - web:     the Bevy CLI and the wasm32 target (`rustup target add wasm32-unknown-unknown`);
#              `--yes` lets the CLI fetch wasm-bindgen-cli at Cargo.lock's version and wasm-opt
#   - mac:     both Apple targets (`rustup target add x86_64-apple-darwin aarch64-apple-darwin`)
#   - linux:   Docker (OrbStack is started if it is installed)
#   - windows: `brew install mingw-w64` and `rustup target add x86_64-pc-windows-gnu` — the gnu
#              target is the local stand-in for CI's msvc one
#
# Linux builds in Docker in the background while the host builds the rest one after another. Logs
# go to tmp/deploy/logs/.

set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

# From release.yaml: `cargo_build_binary_name`, `app_package_name`, `app_display_name`, `app_id`.
name=bevy_kart
package_name=bevy-kart
display_name="Bevy Kart"
app_id=sigmatronic.bevy-kart

# --- Arguments --------------------------------------------------------------------------------

dry_run=false
allow_behind=false
targets=()
for arg in "$@"; do
  case "${arg}" in
    --dry-run|-n) dry_run=true ;;
    --allow-behind) allow_behind=true ;;
    all) targets+=(web mac linux windows) ;;
    web|mac|linux|windows) targets+=("${arg}") ;;
    -h|--help) sed -n '2,37p' "$0"; exit 0 ;;
    *) echo "unknown argument ${arg} (try --help)" >&2; exit 2 ;;
  esac
done
(( ${#targets[@]} )) || targets=(web mac linux windows)
has() { [[ " ${targets[*]} " == *" $1 "* ]]; }
if (( ${#targets[@]} < 4 )); then
  echo "note: pushing only ${targets[*]}; the other channels keep their older build, which may not" >&2
  echo "      be able to join this one if the networked components changed" >&2
fi

# --- Secrets and settings -------------------------------------------------------------------------

if [[ ! -f .env.deploy ]]; then
  echo "no .env.deploy: create it at the repo root with TURN_PASSWORD=... (see --help)" >&2
  exit 1
fi
set -a
# shellcheck disable=SC1091
source .env.deploy
set +a
export SIGNALLING_SERVER_URL="${SIGNALLING_SERVER_URL:-wss://signal.sigma-dev.eu/ws}"
export TURN_URL="${TURN_URL:-turn:signal.sigma-dev.eu:3478}"
export TURN_USER="${TURN_USER:-bevy_kart}"
itch_page="${ITCH_PAGE:-sigmatronic/bevy-kart}"
if [[ -z "${TURN_PASSWORD:-}" ]]; then
  echo "TURN_PASSWORD is empty in .env.deploy; refusing to ship a build with no relay" >&2
  exit 1
fi
export TURN_PASSWORD

# --- Preflight --------------------------------------------------------------------------------------

butler=$(command -v butler || true)
[[ -z "${butler}" && -x "${HOME}/.local/bin/butler" ]] && butler="${HOME}/.local/bin/butler"
if ! ${dry_run} && [[ -z "${butler}" ]]; then
  echo "butler is not installed: https://itch.io/docs/butler/installing.html, then \`butler login\`" >&2
  exit 1
fi

# A local `[patch]` bakes in whatever is sitting uncommitted in the crate next door, which is
# invisible in the artifact and unreproducible afterwards.
if grep -q '^\[patch' Cargo.toml; then
  echo "Cargo.toml has a [patch] section; a release would build the local checkout it points at." >&2
  echo "Remove it (or ship on purpose by editing this check out)." >&2
  exit 1
fi

# The remote moving on while the build compiles an older tree is how a release ships without
# somebody else's commit. Nothing else warns about it.
if git fetch --quiet 2>/dev/null; then
  behind=$(git rev-list --count 'HEAD..@{upstream}' 2>/dev/null || echo 0)
  if (( behind > 0 )) && ! ${allow_behind}; then
    echo "origin is ${behind} commit(s) ahead of HEAD; pull first, or pass --allow-behind" >&2
    git log --oneline 'HEAD..@{upstream}' >&2
    exit 1
  fi
else
  echo "note: git fetch failed; not checking whether origin has moved on" >&2
fi

cargo_version="$(cargo pkgid | sed 's/.*[#@]//')"
sha="$(git rev-parse --short HEAD)"
dirty="$(git diff --quiet HEAD -- src assets web audio_manager bevy_timer Cargo.toml Cargo.lock || echo -dirty)"
version="${cargo_version}+manual-${sha}${dirty}"
target_dir="$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"

stage=tmp/deploy
logs="${stage}/logs"
mkdir -p "${logs}"
started=$(date +%s)
echo "deploying ${version} (${targets[*]}) to ${itch_page}$(${dry_run} && echo ', dry run')"

# --- Build -----------------------------------------------------------------------------------------

# release.yaml builds every platform with its default features (its matrix sets no `features`),
# and bevy_kart has none, so `netdebug` stays out and no feature flag is passed here either.

linux_pid=""
if has linux; then
  command -v docker >/dev/null || { echo "linux needs Docker" >&2; exit 1; }
  command -v orb >/dev/null && orb start >/dev/null 2>&1 || true
  echo "== linux (docker, in the background) -> ${logs}/linux.log"
  # The crate caches are mounted so a warm build is minutes rather than twenty, and so the git
  # dependencies resolve from the host's checkouts. The output goes to its own directory under
  # target/ so it never fights the host's builds.
  docker run --rm --platform linux/amd64 \
    -e SIGNALLING_SERVER_URL -e TURN_URL -e TURN_USER -e TURN_PASSWORD \
    -e CARGO_TARGET_DIR=/work/target/linux \
    -v "${PWD}":/work -v "${HOME}/.cargo/registry":/usr/local/cargo/registry \
    -v "${HOME}/.cargo/git":/usr/local/cargo/git \
    -w /work rust:1-bookworm bash -c '
      apt-get update -qq && apt-get install -y -qq --no-install-recommends \
        libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev >/dev/null &&
      cargo build --locked --release' \
    >"${logs}/linux.log" 2>&1 &
  linux_pid=$!
fi

# One host build at a time, each to its own log; a failure stops the deploy before anything is
# pushed, so a stale binary from the last release can never be packaged by mistake.
host_build() {
  local label="$1"; shift
  echo "== ${label} -> ${logs}/${label}.log"
  if ! "$@" >"${logs}/${label}.log" 2>&1; then
    echo "${label} failed; last lines of its log:" >&2
    tail -20 "${logs}/${label}.log" >&2
    [[ -n "${linux_pid}" ]] && kill "${linux_pid}" 2>/dev/null
    exit 1
  fi
}

if has web; then
  # The CLI bundles `web/index.html` and `assets/` alongside the wasm.
  host_build web bevy build --locked --release --yes web --bundle
fi
if has mac; then
  export MACOSX_DEPLOYMENT_TARGET=11.0
  SDKROOT="$(xcrun --sdk macosx --show-sdk-path)"
  export SDKROOT
  host_build mac-arm64 cargo build --locked --release --target aarch64-apple-darwin
  host_build mac-x86_64 cargo build --locked --release --target x86_64-apple-darwin
fi
if has windows; then
  export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc
  host_build windows cargo build --locked --release --target x86_64-pc-windows-gnu
fi
if [[ -n "${linux_pid}" ]]; then
  echo "== waiting for linux"
  if ! wait "${linux_pid}"; then
    echo "linux failed; last lines of its log:" >&2
    tail -20 "${logs}/linux.log" >&2
    exit 1
  fi
fi

# --- Check the secrets were baked in --------------------------------------------------------------

# Every artifact is read; one that is missing, or older than this run, is a hard failure rather
# than a silent skip. Counts are printed, never the strings. The three TURN strings are the
# decisive signal (`ice_servers_from_env!` reads them with `option_env!`, so a build without them
# silently falls back to STUN). The signalling URL is checked too, except in the x86_64 macOS
# slice, which in run-2d has never carried it contiguously and runs correctly anyway; on native
# the URL is also read from the environment at launch, and the public one is the default.
artifacts=()
has web && artifacts+=("web=${target_dir}/bevy_web/web-release/${name}/build/${name}_bg.wasm")
has mac && artifacts+=("mac-arm64=${target_dir}/aarch64-apple-darwin/release/${name}" \
                       "mac-x86_64=${target_dir}/x86_64-apple-darwin/release/${name}")
# The linux build is wherever Docker put it: under this checkout, whatever CARGO_TARGET_DIR says.
has linux && artifacts+=("linux=${PWD}/target/linux/release/${name}")
has windows && artifacts+=("windows=${target_dir}/x86_64-pc-windows-gnu/release/${name}.exe")
# A build that fails stops the script where it runs, so whatever is here was built by this run or,
# when cargo had nothing to do (say, after a dry run of the same commit), is that same build. The
# time is shown for reading, not checked.
python3 - "${artifacts[@]}" <<'PY'
import os, sys, time
needles = {k: os.environ[k] for k in ("SIGNALLING_SERVER_URL", "TURN_URL", "TURN_USER", "TURN_PASSWORD")}
failed = False
print(f"{'artifact':12s} {'built':>8s} " + " ".join(f"{k.lower():>22s}" for k in needles))
for item in sys.argv[1:]:
    label, path = item.split("=", 1)
    if not os.path.exists(path):
        print(f"{label}: MISSING {path}"); failed = True; continue
    mtime = os.path.getmtime(path)
    blob = open(path, "rb").read()
    counts = {k: blob.count(v.encode()) for k, v in needles.items()}
    print(f"{label:12s} {time.strftime('%H:%M:%S', time.localtime(mtime)):>8s} "
          + " ".join(f"{counts[k]:>22d}" for k in needles))
    turn = all(counts[k] for k in ("TURN_URL", "TURN_USER", "TURN_PASSWORD"))
    if not turn:
        print(f"  {label}: TURN settings missing"); failed = True
    if counts["SIGNALLING_SERVER_URL"] == 0 and label != "mac-x86_64":
        print(f"  {label}: signalling URL missing"); failed = True
sys.exit(1 if failed else 0)
PY

# --- Package -----------------------------------------------------------------------------------------

# One directory per channel. A shared staging directory once let a glob sweep run-2d's macOS .app
# into its Linux and Windows zips without a word.
#
# Each is laid out as release.yaml zips it — the web bundle and the native builds under
# `bevy-kart/`, the app as `bevy-kart.app` with `bevy_kart` and `assets/` in Contents/MacOS — so
# the itch app sees the same paths whichever way a release was made. The one difference: the
# workflow wraps the app in a .dmg, and this pushes the .app itself, which the itch app can run.
package() {
  local channel="$1"
  rm -rf "${stage:?}/${channel}"
  mkdir -p "${stage}/${channel}"
  echo "${stage}/${channel}"
}

pushes=()
if has web; then
  dir=$(package web)
  cp -R "${target_dir}/bevy_web/web-release/${name}" "${dir}/${package_name}"
  pushes+=("web=${dir}")
fi
if has mac; then
  dir=$(package macos)
  app="${dir}/${package_name}.app/Contents"
  mkdir -p "${app}/MacOS"
  lipo "${target_dir}/aarch64-apple-darwin/release/${name}" \
       "${target_dir}/x86_64-apple-darwin/release/${name}" \
       -create -output "${app}/MacOS/${name}"
  cp -R assets "${app}/MacOS/"
  cat >"${app}/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
    <dict>
        <key>CFBundleDevelopmentRegion</key>
        <string>en</string>
        <key>CFBundleDisplayName</key>
        <string>${display_name}</string>
        <key>CFBundleExecutable</key>
        <string>${name}</string>
        <key>CFBundleIdentifier</key>
        <string>${app_id}</string>
        <key>CFBundleName</key>
        <string>${display_name}</string>
        <key>CFBundleShortVersionString</key>
        <string>${version}</string>
        <key>CFBundleVersion</key>
        <string>${version}</string>
        <key>CFBundleInfoDictionaryVersion</key>
        <string>6.0</string>
        <key>CFBundlePackageType</key>
        <string>APPL</string>
        <key>CFBundleSupportedPlatforms</key>
        <array>
            <string>MacOSX</string>
        </array>
    </dict>
</plist>
EOF
  pushes+=("macos=${dir}")
fi
if has linux; then
  dir=$(package linux)
  mkdir -p "${dir}/${package_name}"
  cp "${PWD}/target/linux/release/${name}" "${dir}/${package_name}/"
  cp -R assets "${dir}/${package_name}/"
  pushes+=("linux=${dir}")
fi
if has windows; then
  dir=$(package windows)
  mkdir -p "${dir}/${package_name}"
  cp "${target_dir}/x86_64-pc-windows-gnu/release/${name}.exe" "${dir}/${package_name}/"
  cp -R assets "${dir}/${package_name}/"
  pushes+=("windows=${dir}")
fi

# --- Push ----------------------------------------------------------------------------------------

# Channel names are release.yaml's: web, macos, linux, windows.
for item in "${pushes[@]}"; do
  channel="${item%%=*}" dir="${item#*=}"
  if ${dry_run}; then
    echo "dry run: would push ${dir} to ${itch_page}:${channel} as ${version}"
  else
    echo "== push ${channel}"
    "${butler}" push --fix-permissions --userversion="${version}" "${dir}" "${itch_page}:${channel}"
  fi
done

elapsed=$(( $(date +%s) - started ))
echo "done in $((elapsed / 60))m$((elapsed % 60))s: ${version}"
if ! ${dry_run}; then
  # A channel still being ingested shows its new build on a second row under the old one; that is
  # not a failure. Run `butler status ${itch_page}` again in a minute if one is lagging.
  "${butler}" status "${itch_page}"
fi
