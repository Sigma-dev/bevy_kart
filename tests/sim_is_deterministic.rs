//! Source-text guardrails on the simulation: nothing that runs in the tick reads the frame.
//!
//! A rollback re-runs ticks, and a replayed tick has to produce what the first run did, on this
//! peer and on every other. The rules are `bevy_ticked`'s `docs/ROLLBACK_RULES.md`; this is the
//! grep that catches the reaches -- the frame clock, the keyboard, the thread's randomness, the
//! wall clock -- in the modules that hold `TickedSimulation` systems. The rest of the tree is
//! presentation and menus, and reads all of those on purpose.

use bevy_ticked_testing::source_guard::{DEFAULT_NEEDLES, Exception, SourceGuard};

fn guard() -> SourceGuard {
    SourceGuard::new(env!("CARGO_MANIFEST_DIR"))
        // The modules with systems in `TickedSimulation`.
        .sources(&["src/car_controller_2d", "src/items", "src/track/position"])
        .ban(DEFAULT_NEEDLES)
        .allow(&[Exception {
            path: "src/items/mod.rs",
            needle: "Sprite",
            reason: "`animate_rocket` runs in Update and flips the rocket's atlas frame; \
                         the tick moves Position, not the sprite",
            expires: None,
        }])
        .min_files(4)
}

#[test]
fn the_guardrail_bites() {
    guard().assert_it_bites();
}

#[test]
fn the_tick_does_not_read_the_frame() {
    guard().assert_clean();
}

#[test]
fn every_exception_is_still_needed() {
    guard().assert_every_exception_is_still_needed();
}

#[test]
fn no_exception_has_expired() {
    guard().assert_no_exception_has_expired();
}
