use bevy::prelude::*;

pub const RESOLUTION: Vec2 = Vec2::new(256., 144.);

pub enum SpriteLayers {
    Background,
    OnGround,
    Wheels,
    Car,
    AboveCar,
}

impl SpriteLayers {
    pub fn to_z(&self) -> f32 {
        match self {
            SpriteLayers::Background => -100.,
            SpriteLayers::OnGround => -10.,
            SpriteLayers::Wheels => -1.,
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
