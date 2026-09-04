# Common tasks. `just --list` for the full set.

default: run

# The game, on its own, against the public signalling server.
run:
    cargo run

# With the network overlay's condition simulator (delay / jitter / loss). F3 toggles the panel.
run-netdebug:
    cargo run --features netdebug

# Variadic, so `just session 4 --port 9095`, `just session --manual` and `just session --drive`
# all reach the script.
#
# A real multiplayer session on this machine: signalling server, a host, and N-1 clients that race.
session *args="2":
    ./scripts/local-session.sh {{args}}

# The same peers, against the same server, left in the menu for you to drive.
session-manual peers="2":
    ./scripts/local-session.sh {{peers}} --manual

# The signalling server on its own, for driving the game by hand in other terminals.
signalling port="9090":
    cargo run --example signalling_server -- {{port}}

# Tests, including the wire-format golden test.
test:
    cargo test

# The web build, as the release workflow makes it.
build-web:
    bevy build --release --yes web --bundle
