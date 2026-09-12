//! The local player's input, sampled for the tick about to run.
//!
//! `TickedInputPlugin` runs [`sample_local_input`] inside the tick loop, after a
//! client's rollback and before the tick, once per tick, and files what it
//! returns under the local player's uuid for that tick. It used to be an
//! `Update` system stamping `tick + 1` by hand: once per frame, so a frame that
//! ran two ticks fed the second its predecessor's keys, and after the tick, so a
//! keypress waited a frame.

use bevy::prelude::*;
use bevy_ticked::prelude::*;
use bevy_ticked_networking::prelude::*;

use crate::PlayerInput;
use crate::lobby::SessionParams;

/// The keyboard, or the autodrive script, as this tick's input.
///
/// `None` with no session: `LocalPlayer` is zero then, and there is no kart for
/// the input to reach.
pub(crate) fn sample_local_input(
    keys: Res<ButtonInput<KeyCode>>,
    tick: Res<CurrentTick>,
    local: Res<LocalPlayer>,
    params: Option<Res<SessionParams>>,
) -> Option<PlayerInput> {
    if local.0 == 0 {
        return None;
    }
    // `autodrive`: throttle held, steering flipped every 1.5 s, item on the
    // fifth second. Enough to keep every kart moving and colliding in a run
    // nobody is driving. Phased on the tick the input will run in.
    if params.is_some_and(|p| p.autodrive) {
        let next = tick.0 + 1;
        let phase = (next / 96).is_multiple_of(2);
        return Some(PlayerInput {
            forward: true,
            backward: false,
            left: phase,
            right: !phase,
            using_item: next % 320 < 4,
        });
    }
    Some(PlayerInput {
        forward: keys.pressed(KeyCode::KeyW) || keys.pressed(KeyCode::ArrowUp),
        backward: keys.pressed(KeyCode::KeyS) || keys.pressed(KeyCode::ArrowDown),
        left: keys.pressed(KeyCode::KeyA) || keys.pressed(KeyCode::ArrowLeft),
        right: keys.pressed(KeyCode::KeyD) || keys.pressed(KeyCode::ArrowRight),
        using_item: keys.pressed(KeyCode::Space),
    })
}
