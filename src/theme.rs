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
///
/// The wall band is the one thing that sits *inside* a kart's own stack: at
/// [`SpriteLayers::Wall`], above where the wheels land and below the body. A
/// kart's wheels stick out past its collider, so a kart leaning on a wall has
/// its wheels over the band -- and a wheel drawn on top of a barrier reads as a
/// kart on top of the wall, where one drawn under it reads as a kart against it.
/// The body never overlaps the band, because the collider stops it, so which
/// side of the body the band is on only decides what shows through a physics
/// slip; the wheels are the case that matters.
pub enum SpriteLayers {
    Background,
    OnGround,
    /// Relative to the parent kart, not the world. See the note above.
    Wheels,
    /// On the road and under the karts: the things a kart drives over.
    OnTrack,
    /// The painted wall band: over a kart's wheels, under its body.
    Wall,
    Car,
    AboveCar,
}

impl SpriteLayers {
    pub fn to_z(&self) -> f32 {
        match self {
            SpriteLayers::Background => -100.,
            SpriteLayers::OnGround => -10.,
            SpriteLayers::Wheels => -2.,
            SpriteLayers::OnTrack => 1.,
            SpriteLayers::Wall => 9.,
            SpriteLayers::Car => 10.,
            SpriteLayers::AboveCar => 100.,
        }
    }
}

pub enum AppColors {
    Dark,
    Road,
    Grass,
    /// The red half of the red-and-white wall band the wall mesh draws down each
    /// road edge. The barriers are colliders only and have no colour of their
    /// own, so this is the only place the wall is painted.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The band has to land between a kart's wheels and its body, and the wheels
    /// are an offset from the kart rather than a place in the world -- so the
    /// three numbers only mean what the docs say if they are checked together.
    #[test]
    fn the_wall_sits_between_a_karts_wheels_and_its_body() {
        let kart = SpriteLayers::Car.to_z();
        let wheels = kart + SpriteLayers::Wheels.to_z();
        let wall = SpriteLayers::Wall.to_z();
        assert!(
            wheels < wall,
            "wheels ({wheels}) must draw under the wall ({wall})"
        );
        assert!(
            wall < kart,
            "the wall ({wall}) must draw under the kart ({kart})"
        );
    }
}
