//! The scenery: trees, grass, flowers and the occasional fox.
//!
//! Lives here rather than in `menu::lobby`, which is where it started, because
//! two very different things want it now: the lobby's parallax background, which
//! scrolls it past the camera, and a race track, which scatters it over the
//! ground once and leaves it there.

use bevy::prelude::*;
use rand::distr::Distribution;
use rand::distr::weighted::WeightedIndex;
use rand::rng;

use crate::{AssetHandles, SpriteLayers};

#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum BackgroundElement {
    Tree,
    Grass,
    YellowFlower,
    RedFlower,
    BlueFlower,
    PurpleFlower,
    Fox,
    Worm,
    Cloud1,
    Cloud2,
}

impl BackgroundElement {
    pub fn as_sprite(&self, handles: &AssetHandles) -> Sprite {
        let index = match self {
            BackgroundElement::Tree => 0,
            BackgroundElement::Grass => 1,
            BackgroundElement::YellowFlower => 2,
            BackgroundElement::RedFlower => 3,
            BackgroundElement::BlueFlower => 4,
            BackgroundElement::PurpleFlower => 5,
            BackgroundElement::Fox => 6,
            BackgroundElement::Worm => 7,
            BackgroundElement::Cloud1 => 0,
            BackgroundElement::Cloud2 => 1,
        };
        let texture_and_atlas = match self {
            BackgroundElement::Cloud1 => {
                (handles.clouds_texture.clone(), handles.clouds_atlas.clone())
            }
            BackgroundElement::Cloud2 => {
                (handles.clouds_texture.clone(), handles.clouds_atlas.clone())
            }
            _ => (
                handles.background_elements_texture.clone(),
                handles.background_elements_atlas.clone(),
            ),
        };
        Sprite::from_atlas_image(
            texture_and_atlas.0,
            TextureAtlas {
                layout: texture_and_atlas.1,
                index,
            },
        )
    }

    pub fn pick_random() -> Self {
        let choices = [
            BackgroundElement::Tree,
            BackgroundElement::Grass,
            BackgroundElement::YellowFlower,
            BackgroundElement::RedFlower,
            BackgroundElement::BlueFlower,
            BackgroundElement::PurpleFlower,
            BackgroundElement::Fox,
            BackgroundElement::Worm,
            BackgroundElement::Cloud1,
            BackgroundElement::Cloud2,
        ];
        let weights = [5, 200, 10, 10, 10, 10, 2, 2, 2, 2];
        let dist = WeightedIndex::new(weights).unwrap();
        choices[dist.sample(&mut rng())]
    }

    pub fn speed(&self) -> f32 {
        match self {
            BackgroundElement::Cloud1 => 30.,
            BackgroundElement::Cloud2 => 45.,
            _ => 100.,
        }
    }

    pub fn layer(&self) -> SpriteLayers {
        match self {
            BackgroundElement::Cloud1 => SpriteLayers::AboveCar,
            BackgroundElement::Cloud2 => SpriteLayers::AboveCar,
            _ => SpriteLayers::OnGround,
        }
    }
}

/// The elements that belong on a race track's ground.
///
/// Everything except the clouds: those are 32x32, they belong to the parallax
/// menu background, and a cloud parked motionless in the middle of a field reads
/// as a bug rather than as weather.
pub const GROUND_ELEMENTS: &[BackgroundElement] = &[
    BackgroundElement::Tree,
    BackgroundElement::Grass,
    BackgroundElement::YellowFlower,
    BackgroundElement::RedFlower,
    BackgroundElement::BlueFlower,
    BackgroundElement::PurpleFlower,
    BackgroundElement::Fox,
    BackgroundElement::Worm,
];

/// Relative frequencies of [`GROUND_ELEMENTS`], mostly grass with the odd tree.
pub const GROUND_WEIGHTS: &[u32] = &[5, 200, 10, 10, 10, 10, 2, 2];

/// Pick a ground element from a number, without touching any global rng.
///
/// Takes the value rather than a generator so a caller can derive it from a
/// map's seed and an index and get the same answer on every machine, however
/// many elements it ends up placing.
pub fn ground_element_from(bits: u64) -> BackgroundElement {
    let total: u32 = GROUND_WEIGHTS.iter().sum();
    let mut pick = (bits % total as u64) as u32;
    for (element, weight) in GROUND_ELEMENTS.iter().zip(GROUND_WEIGHTS) {
        if pick < *weight {
            return *element;
        }
        pick -= *weight;
    }
    BackgroundElement::Grass
}

/// Marks anything scattered as decoration.
///
/// Decor is cosmetic and must never reach the simulation: no collider, no
/// `Position`, no networked identity. The marker is what makes that checkable --
/// see `decor_never_reaches_the_simulation`.
#[derive(Component)]
pub struct Decor;

/// Debug-only: fail loudly if decoration ever grows a physical body.
///
/// A rule nobody can see decays. This is twelve lines and it is what stops a
/// future "just give the trees colliders" from silently making the scenery part
/// of the rollback simulation, where its seeded placement would have to be
/// bit-identical across peers rather than merely pleasant.
#[cfg(debug_assertions)]
pub fn decor_never_reaches_the_simulation(
    offenders: Query<
        Entity,
        (
            With<Decor>,
            Or<(
                With<avian2d::prelude::Collider>,
                With<avian2d::prelude::Position>,
                With<bevy_ticked::prelude::TickTrackedEntity>,
            )>,
        ),
    >,
) {
    assert!(
        offenders.is_empty(),
        "decoration has been given a physical or networked component: {:?}",
        offenders.iter().collect::<Vec<_>>()
    );
}
