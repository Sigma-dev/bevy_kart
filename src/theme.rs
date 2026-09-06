use bevy::prelude::*;

pub const RESOLUTION: Vec2 = Vec2::new(256., 144.);

/// Drawing order. All world z, with one exception: [`SpriteLayers::Wheels`] is
/// negative and is only ever used on a *child* of a kart, where it is an offset
/// from the kart's own z rather than a place in the world.
///
/// The track image is spawned with no `Transform` of its own, so it sits at
/// z = 0. Anything meant to be seen on the track has to be above that:
/// [`SpriteLayers::OnTrack`], not [`SpriteLayers::OnGround`], which is behind it.
pub enum SpriteLayers {
    Background,
    OnGround,
    /// Relative to the parent kart, not the world. See the note above.
    Wheels,
    /// On the track and under the karts: the things a kart drives over.
    OnTrack,
    Car,
    AboveCar,
}

impl SpriteLayers {
    pub fn to_z(&self) -> f32 {
        match self {
            SpriteLayers::Background => -100.,
            SpriteLayers::OnGround => -10.,
            SpriteLayers::Wheels => -1.,
            SpriteLayers::OnTrack => 1.,
            SpriteLayers::Car => 10.,
            SpriteLayers::AboveCar => 100.,
        }
    }
}

pub enum AppColors {
    Dark,
    Road,
    Grass,
}

impl AppColors {
    pub fn color(&self) -> Color {
        match self {
            AppColors::Dark => Srgba::hex("2e222f").unwrap().into(),
            AppColors::Road => Srgba::hex("323353").unwrap().into(),
            AppColors::Grass => Srgba::hex("239063").unwrap().into(),
        }
    }
}
