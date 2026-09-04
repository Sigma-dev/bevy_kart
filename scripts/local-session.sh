#!/usr/bin/env bash
#
# A real multiplayer session on this machine, in one command.
#
#   ./scripts/local-session.sh                # a host and one client
#   ./scripts/local-session.sh 4              # a host and three clients
#   ./scripts/local-session.sh --port 9095    # somewhere other than 9090
#   ./scripts/local-session.sh --manual       # leave everybody in the menu
#   ./scripts/local-session.sh --drive        # every kart drives itself
#
# Starts the signalling server, then that many copies of the game: the first hosts, the rest join
# it, and the host starts the race as soon as a second player is in the lobby. Nobody has to touch
# a menu; see `lobby::SessionParams`.
#
# `--manual` starts the same peers against the same server and stops there, in the menu, with the
# keyboard yours: host in one window, read the code, join with it in the others.
#
# `--drive` holds the throttle in every window, for watching the netcode with nobody at the wheel.
#
# Ctrl-C, or closing any window, takes the whole session down.

set -euo pipefail

peers=2
port=9090
manual=false
drive=false

usage() {
  cat <<'USAGE'
usage: local-session.sh [peers] [--port PORT] [--manual] [--drive]

  peers          how many copies of the game to start, 2 or more (default 2).
                 One hosts, the rest join it.
  -p, --port     port for the signalling server (default 9090). Useful when 9090
                 is taken, or to run two independent sessions side by side.
  -m, --manual   do not host or join anything. Every peer starts in the menu,
                 pointed at the server, and you drive them.
  -d, --drive    every kart holds the throttle and weaves on its own.
USAGE
}

while (( $# )); do
  case "$1" in
    -h|--help) usage; exit 0 ;;
    -p|--port)
      [[ $# -ge 2 ]] || { echo "$1 needs a port" >&2; exit 2; }
      port="$2"
      shift 2
      ;;
    --port=*) port="${1#*=}"; shift ;;
    -m|--manual) manual=true; shift ;;
    -d|--drive) drive=true; shift ;;
    -*) echo "unknown option $1" >&2; usage >&2; exit 2 ;;
    *) peers="$1"; shift ;;
  esac
done

if ! [[ "${peers}" =~ ^[0-9]+$ ]] || (( peers < 2 )); then
  echo "peers must be a number, 2 or more (got '${peers}')" >&2
  exit 2
fi
if ! [[ "${port}" =~ ^[0-9]+$ ]] || (( port < 1 || port > 65535 )); then
  echo "port must be a number between 1 and 65535 (got '${port}')" >&2
  exit 2
fi

cd "$(dirname "${BASH_SOURCE[0]}")/.."

# A bash /dev/tcp probe needs nothing installed. A successful connect means something is already
# serving this port.
if (echo >"/dev/tcp/127.0.0.1/${port}") 2>/dev/null; then
  echo "something is already listening on ${port}: stop it, or pass --port" >&2
  exit 1
fi

# How the game finds the server. Read at launch on native, so exporting it here is enough; the
# peers need no rebuild to point somewhere else.
export SIGNALLING_SERVER_URL="ws://127.0.0.1:${port}/ws"

# Where the game finds its assets. This starts the built binary directly rather than through
# `cargo run`, and bevy's asset root otherwise falls back to the directory holding the executable,
# which has no assets in it. Without this the session runs perfectly while showing nothing.
export BEVY_ASSET_ROOT="${PWD}"

if [[ "${drive}" == true ]]; then
  export KART_AUTODRIVE=1
fi

echo "building..."
cargo build --bin bevy_kart --example signalling_server

pids=()
cleanup() {
  for pid in "${pids[@]:-}"; do
    [[ -n "${pid}" ]] && kill "${pid}" 2>/dev/null || true
  done
  wait 2>/dev/null || true
}
trap cleanup EXIT INT TERM

./target/debug/examples/signalling_server "${port}" &
pids+=($!)

# The peers are handed the URL immediately, so the socket has to be listening before any of them
# starts or the first connection races the bind.
echo -n "waiting for the signalling server"
for _ in $(seq 50); do
  if (echo >"/dev/tcp/127.0.0.1/${port}") 2>/dev/null; then break; fi
  echo -n "."
  sleep 0.1
done
echo

if [[ "${manual}" == true ]]; then
  for index in $(seq 1 "${peers}"); do
    echo "starting peer ${index}"
    KART_NAME="P${index}" ./target/debug/bevy_kart &
    pids+=($!)
  done
else
  echo "starting host"
  # The race waits for everybody the script is about to start, not just the second player.
  KART_AUTOSTART=host KART_AUTOSTART_PLAYERS="${peers}" KART_NAME=HOST ./target/debug/bevy_kart &
  pids+=($!)

  # The lobby has to exist before anyone asks for it. The clients retry regardless; this only
  # keeps the first few seconds of log quiet.
  sleep 3

  for index in $(seq 2 "${peers}"); do
    echo "starting client ${index}"
    KART_AUTOSTART=join KART_NAME="P${index}" ./target/debug/bevy_kart &
    pids+=($!)
  done
fi

echo
if [[ "${manual}" == true ]]; then
  echo "${peers} peers waiting in the menu. Host in one, read the code, join with it in the others."
fi
echo "session running with ${peers} peers. Ctrl-C to stop."

# Any one of them exiting ends the session, rather than leaving orphaned windows behind. Polled
# rather than `wait -n`, which needs bash 4.3; macOS still ships 3.2.
while true; do
  for pid in "${pids[@]}"; do
    if ! kill -0 "${pid}" 2>/dev/null; then
      echo "a peer exited, stopping the session"
      exit 0
    fi
  done
  sleep 1
done
