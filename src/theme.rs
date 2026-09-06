use bevy::prelude::*;

pub const RESOLUTION: Vec2 = Vec2::new(256., 144.);

/// Drawing order. All world z, with one exception: [`SpriteLayers::Wheels`] is
/// negative and is only ever used on a *child* of a kart, where it is an offset
/// from the kart's own z rather than a place in the world.
///
/// The road is a generated mesh at [`SpriteLayers::Background`], with the
/// start/finish band just above it at [`SpriteLayers::OnGround`]. Anything a
/// kart drives *over* goes at [`SpriteLayers::OnTrack`], between the two and the
/// karts themselves.
pub enum SpriteLayers {
    Background,
    OnGround,
    /// Relative to the parent kart, not the world. See the note above.
    Wheels,
    /// On the road and under the karts: the things a kart drives over.
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
    /// The red half of the red-and-white wall band the road mesh draws down each
    /// edge. The barriers are colliders only and have no colour of their own, so
    /// this is the only place the wall is painted.
    Kerb,
}

impl AppColors {
    pub fn color(&self) -> Color {
        match self {
            AppColors::Dark => Srgba::hex("2e222f").unwrap().into(),
            AppColors::Road => Srgba::hex("323353").unwrap().into(),
            AppColors::Grass => Srgba::hex("239063").unwrap().into(),
            // The palette's red, the one the karts, the items and the start
            // light are already drawn in -- not a near miss hand-mixed in linear
            // floats, which is what this was.
            AppColors::Kerb => Srgba::hex("ae2334").unwrap().into(),
        }
    }
}
