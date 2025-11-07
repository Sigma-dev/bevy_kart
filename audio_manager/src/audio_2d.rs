use bevy::prelude::*;

use crate::{PlayAudio, SpatialSettings};

#[derive(Clone)]
pub enum SpatialSettings2D {
    Position(Vec2),
    Entity(Entity),
}

#[derive(Clone)]
pub struct PlayAudio2D {
    pub path: String,
    pub volume_mult: f32,
    pub one_shot: bool,
    pub spatial_settings: Option<SpatialSettings2D>,
}

impl PlayAudio2D {
    pub fn new_once(path: impl Into<String>) -> PlayAudio2D {
        PlayAudio2D {
            path: path.into(),
            volume_mult: 1.,
            one_shot: true,
            spatial_settings: None,
        }
    }

    pub fn new_repeating(path: impl Into<String>) -> PlayAudio2D {
        PlayAudio2D {
            path: path.into(),
            volume_mult: 1.,
            one_shot: false,
            spatial_settings: None,
        }
    }

    pub fn with_volume(&self, volume_mult: f32) -> PlayAudio2D {
        let mut new = self.clone();
        new.volume_mult = volume_mult;
        new
    }
    pub fn with_spatial(&self, spatial_settings: SpatialSettings2D) -> PlayAudio2D {
        let mut new = self.clone();
        new.spatial_settings = Some(spatial_settings);
        new
    }
}

impl PlayAudio for PlayAudio2D {
    fn is_one_shot(&self) -> bool {
        self.one_shot
    }
    fn volume_mult(&self) -> f32 {
        self.volume_mult
    }
    fn path(&self) -> String {
        self.path.clone()
    }
    fn get_spatial(&self) -> Option<SpatialSettings> {
        self.spatial_settings
            .clone()
            .map(|spatial_settings| match spatial_settings {
                SpatialSettings2D::Position(position) => {
                    SpatialSettings::Position(Transform::from_translation(position.extend(0.)))
                }
                SpatialSettings2D::Entity(entity) => SpatialSettings::Entity(entity),
            })
    }
}
