# Bevy Kart

A 2D top-down kart racer in [Bevy](https://bevy.org) 0.19, played over WebRTC with
rollback netcode. It runs natively and in the browser from one codebase. Three laps
to win, items along the way, and a track editor built in.

This file is for people working on it. Everything below is a command you can run
from a fresh clone.

## Setup

**Rust.** Current stable works. CI pins `nightly-2026-03-02` only so that its two
runners agree with each other. Edition 2024, so anything older than 1.85 will not
build.

**Linux** needs the usual Bevy development packages:

```sh
sudo apt-get install --no-install-recommends libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev
```

**For the web build**, the [Bevy CLI](https://thebevyflock.github.io/bevy_cli/) and
the wasm target:

```sh
cargo install --git https://github.com/TheBevyFlock/bevy_cli --locked bevy_cli
rustup target add wasm32-unknown-unknown
```

The CLI installs `wasm-bindgen` and `wasm-opt` itself when you pass `--yes`.

## Run it

```sh
cargo run
```

That is the game against the public signalling server. Host a lobby, hand the
four-letter code to a friend, race.

| Keys | |
|---|---|
| `W A S D` or arrows | drive |
| `Space` | use the held item |
| `F4` | draw the racing line the lap counter measures against |
| `U` + `K` | host only: end the race now |

### A whole multiplayer session on one machine

```sh
./scripts/local-session.sh        # a signalling server, a host and one client, racing
./scripts/local-session.sh 4      # a host and three clients
./scripts/local-session.sh --manual   # everybody left in the menu for you to drive
./scripts/local-session.sh --drive    # every kart holds the throttle, for watching the netcode
./scripts/local-session.sh --port 9095
```

It builds, starts `examples/signalling_server.rs` on the port, and launches that
many copies of the game pointed at it. Closing any window ends the session.

To do the same by hand: run the server, then point each game at it.

```sh
cargo run --example signalling_server -- 9090
SIGNALLING_SERVER_URL=ws://127.0.0.1:9090/ws cargo run
```

### Starting the game with nobody at the keyboard

Every menu choice can be made from the command line, which is what the session
script does. On native these are environment variables; on the web the same
choices are URL parameters.

| Environment | URL | Effect |
|---|---|---|
| `KART_AUTOSTART=host` | `?host=1` | host a lobby |
| `KART_AUTOSTART=join` | `?join=1` | join the first lobby the server lists |
| `KART_AUTOSTART_PLAYERS=N` | `?autostart=N` | as host, start once `N` players are in |
| `KART_ROOM=CODE` | `?room=CODE` | join that lobby |
| `KART_NAME=NAME` | `?name=NAME` | play under this name |
| `KART_MAP=SLUG` | `?map=SLUG` | race this map, built-in or saved |
| `KART_AUTODRIVE=1` | `?autodrive=1` | hold the throttle and weave |
| `KART_EDITOR=1` | `?editor=1` | open the track editor |
| `KART_PERF=1` | `?perf=1` | log the frame-cost readout |

`KART_AUTOSTART=host` on its own starts the race at two players. `?host=1` does
not, because someone hosting from a link wants to press start themselves.

### Seeing what the netcode is doing

```sh
cargo run --features netdebug
```

`F3` opens the network overlay, with a condition simulator for delay, jitter and
loss. The feature is off by default and never in a shipped build.

## The web build

```sh
bevy run web                                  # build and serve on a local port
bevy run web --open --port 8080               # and open it in the browser
bevy build --release --yes web --bundle       # the bundle the release ships
```

The bundle lands in `target/bevy_web/web-release/bevy_kart/`. It is a static
directory: serve it from anywhere.

The browser cannot read environment variables, so the signalling server address
is baked in at build time. Without one it uses the public server.

```sh
SIGNALLING_SERVER_URL=ws://127.0.0.1:9090/ws bevy build --release --yes web --bundle
```

Release builds keep nothing below `warn!`. `bevy_ticked` enables
`release_max_level_warn` on both `log` and `tracing`, and those features unify
across the binary, so an `info!` anywhere is compiled out. Anything a script or a
measurement needs to read from the log has to be a `warn!`.

## Tests and checks

```sh
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo check --target wasm32-unknown-unknown
```

CI runs all three, the tests on both an x86 and an ARM runner. That pairing is
deliberate: every peer builds its own wall colliders from shared map data, and
the tests in `src/track/map/build.rs` pin that geometry to the bit. A golden hash
that only ever ran on one architecture would be a hash of that machine.

Two tests are worth knowing about before they surprise you:

- **`the_tick_does_not_read_the_frame`** in `tests/sim_is_deterministic.rs`. The
  modules with `TickedSimulation` systems may not read the frame clock, the
  keyboard, the wall clock or the thread's randomness: a replayed tick would
  differ from the first run. Frame-side code in those files is listed as an
  exception with the system it belongs to.
- **`the_shipped_maps_are_what_this_draws`** in `src/track/map/generate.rs`. The
  built-in maps are drawn by code and committed as JSON. Change the code and this
  fails until you regenerate; edit the JSON by hand and it fails until the code
  agrees. See below.

## Tracks

A track is a `MapData`: a closed spline of integer-coordinate nodes, a road width,
a start line, item boxes and decor settings. Integers so that every peer derives
the same walls to the bit, and so a map has a stable content hash, which is what
the network calls it by. `src/track/map/data.rs` says why in full.

**Built-ins** live in `assets/maps/*.json` and are compiled into the binary with
`include_str!`, so they exist before any asset loads and the headless tests can
see them. All but Classic are drawn by `src/track/map/generate.rs`, a test-only
module. To change one, edit its shape there and regenerate:

```sh
cargo test regenerate_the_built_in_maps -- --ignored
```

It refuses to write if any map would build with a warning. Classic was converted
once from the game's original sprite, kept as `scripts/classic-reference.png`,
and the JSON has been the track ever since.

**The editor** opens from the main menu, or with `KART_EDITOR=1`. It edits a
`MapData` and previews it through the same builder the race uses, so what you
see is what will be raced. `Tab` switches between the node and item-box tools,
`Ctrl-Z` and `Ctrl-Shift-Z` undo and redo, `F` frames the track, `Alt`-drag a
handle to break its mirror, `Shift`-drag a node to snap it. The panel lists the
rest. A map can be saved, exported as a file, or turned into a share code: a
string of letters and digits that pastes into a chat and decodes on any platform.

**Saved maps** go to `maps/` beside the working directory on native, which is the
repository root under `cargo run` and the session script, so a local session
shares one set. `KART_MAPS_DIR` overrides it. In the browser they go to local
storage. The directory is gitignored.

## Where things are

```
src/
  main.rs              plugin wiring, states, the networked components and messages by wire name
  screen.rs            Screen: which of the four screens is up, computed from the states
  lobby.rs             lobby lifecycle and the session parameters above
  map_sync.rs          getting the host's chosen map to every peer, and starting the race
  networking.rs        PlayerInput, EntityKind, and which entities this peer simulates
  input.rs             the local player's input, sampled once per tick
  entity_spawn.rs      what a tracked entity looks like when it appears and when it goes
  menu/                start menu, lobby UI, the map picker
  track/               the race: laps, positions, minimap, the starting grid
  track/map/           MapData, the deterministic builder, built-ins, storage, sharing
  track/map/generate.rs  the drawings behind the built-in maps (tests only)
  editor/              the track editor
  kart/, items/        karts and the things they throw at each other
  car_controller_2d/   driving physics, on avian2d inside the ticked loop
  camera.rs, hud.rs, decor.rs, theme.rs   presentation
  debug.rs             F4, the FPS counter, the perf readout
assets/               maps, sprites (.aseprite sources beside the .png), sounds
audio_manager/, bevy_timer/   small local crates
examples/signalling_server.rs  a real signalling server, for local sessions
scripts/local-session.sh       the one-command multiplayer session
```

The networking stack is `bevy_ticked` (a fixed 64 Hz tick with rollback),
`bevy_ticked_networking` (snapshots, prediction and interpolation over it),
`bevy_ticked_avian` (avian2d under the tick, replay-safe) and `bevy_ensemble`
(lobbies and WebRTC transport). All of it comes from git; `Cargo.toml` points at
it, and pins `bevy_ensemble` at the commit `bevy_ticked` pins, because the two
must agree about the transport's wire format. `bevy_ticked`'s `docs/MIGRATION.md`
and `docs/migration/bevy_kart.md` say what each upstream phase changed here.

On a client, only the local kart is simulated; every other tracked entity is
drawn from the host's history a couple of ticks behind (`networking::simulates`
is the question every ticked system asks). Spawns go through `TrackedSpawner`
and despawns through `despawn_ticked`, which leaves a tombstone a rewind can
revive -- and which `entity_spawn` turns into the explosion or pickup sound on
every peer.

## Conventions

- **Bevy without its defaults.** Every crate in the tree that depends on `bevy`
  says `default-features = false` and names what it needs. Cargo unifies features
  across the graph, so one crate taking the defaults puts `bevy_pbr` and its
  lookup tables back into the web download for everyone. If you add a bevy
  dependent, do the same.
- **The wall geometry is bit-deterministic.** Inside `src/track/map/build.rs`: no
  `sin`, `cos` or `atan2`, no `mul_add`, `Vec2` only. The file's header explains
  each rule. Anything a peer might build differently from another is a divergence
  rollback corrects and recreates every tick, forever.
- **Wire names are forever.** A networked component or message is registered
  under a string in `main.rs`, and that string is its identity on the wire: the
  order of the list means nothing, the Rust type can be renamed, and changing
  the string is a wire break the join handshake refuses by name.
- **The simulation obeys the rollback rules.** No frame clock, keyboard, wall
  clock or thread randomness inside a `TickedSimulation` system;
  `tests/sim_is_deterministic.rs` greps for the reaches.
- **Tools are Rust.** A generator or converter is a `#[cfg(test)]` module with an
  `#[ignore]`d regeneration test and a snapshot test holding the output to it, the
  way the maps are done. Nothing dev-only ships in the binary or the web bundle.
- **Clippy is clean at `-D warnings`.** `too_many_arguments` and `type_complexity`
  are allowed in `Cargo.toml`; everything else is fixed, not silenced.

## Releasing

`.github/workflows/release.yaml` is run by hand with a version number. It builds
native packages for Windows, macOS and Linux and the web bundle, attaches them to
a GitHub release, and can push to itch.io and GitHub Pages.
