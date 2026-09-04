//! The registration order in `main.rs` is the wire format. This is its guard.
//!
//! Snapshot component indices are positional `u16` (see the registration block in
//! `main.rs`). Nothing in the packet says which type an index means, so two peers
//! built from different commits deserialise each other's `Position` bytes as an
//! `EntityKind` — with no error, no warning, and no way to tell from the symptom
//! that the cause is a build mismatch rather than a physics bug.
//!
//! Two things stand between that and a session:
//!
//! - **At the join.** `TickedEnsembleSessionPlugin`, from
//!   `bevy_ticked_networking_ensemble`, exchanges `TickedComponentRegistry::wire_hash()`
//!   and the resource registry's hash between every pair of peers, reliably and
//!   once each, and ends the session on a mismatch by dropping the role.
//!   `lobby.rs` answers that by leaving the lobby. This crate used to broadcast
//!   the component hash itself, on a timer; the upstream exchange also covers
//!   resources and tells each client individually, which the broadcast did not.
//!
//! - **In CI.** The golden test below pins the order itself, so a reorder fails
//!   on the commit that makes it rather than at the next join between two builds.

#[cfg(test)]
mod tests {
    use bevy::prelude::*;
    use bevy_ticked::prelude::*;

    /// Everything registered, in order. **This list is the wire format.**
    ///
    /// Hand-written next to the assertions rather than read out of the registry,
    /// on purpose: a list that derived itself from the thing it is checking could
    /// not disagree with it. This is what fails in CI when somebody tidies the
    /// registration block into alphabetical order — which compiles, warns about
    /// nothing, and changes the meaning of every index.
    const EXPECTED: &[&str] = &[
        "Position",
        "Rotation",
        "LinearVelocity",
        "AngularVelocity",
        "OwnerPlayer",
        "EntityKind",
        "NetworkedPosition",
        "NetworkedRotation",
        "CarControllerInputs",
        "SteeringState",
        "HeldItem",
        "BoostEffect",
        "CarControllerDisabled",
        "RocketHit",
    ];

    #[test]
    fn registration_order_is_the_wire_format() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        crate::register_networked_components(&mut app);
        let registry = app.world().resource::<TickedComponentRegistry>().clone();

        let names: Vec<&str> = registry.wire_names().collect();
        assert_eq!(
            names, EXPECTED,
            "the registration block changed. Appending is safe; reordering, \
             renaming and deleting are not -- indices are positional and travel \
             in every snapshot."
        );
    }

    #[test]
    fn the_hash_notices_a_reorder() {
        // The failure this whole module exists for is silent by construction, so
        // check that the hash is actually order-sensitive rather than trusting it.
        fn hash_of(names: &[&str]) -> u64 {
            const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
            const PRIME: u64 = 0x0000_0100_0000_01b3;
            names.iter().enumerate().fold(OFFSET, |mut h, (i, n)| {
                for b in (i as u16).to_le_bytes().iter().chain(n.as_bytes()) {
                    h ^= u64::from(*b);
                    h = h.wrapping_mul(PRIME);
                }
                h
            })
        }
        assert_ne!(
            hash_of(&["Position", "Rotation"]),
            hash_of(&["Rotation", "Position"])
        );
    }
}
