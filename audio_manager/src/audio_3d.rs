use bevy::prelude::*;

use crate::{PlayAudio, SpatialSettings};

/// Where a 3D sound is, mirroring [`SpatialSettings2D`](crate::audio_2d::SpatialSettings2D).
///
/// This replaces an `Option<(Vec3, Option<Entity>)>` that could express
/// "positioned at nowhere in particular, attached to nothing" and had no way to
/// say which of the two it meant. An enum says it in the type.
#[derive(Clone, Copy, Debug)]
pub enum SpatialSettings3D {
    /// Play at a fixed point in the world.
    Position(Vec3),
    /// Follow an entity, as a child, so the sound moves with it.
    Entity(Entity),
}

#[derive(Clone)]
pub struct PlayAudio3D {
    pub path: String,
    pub volume_mult: f32,
    pub one_shot: bool,
    pub spatial_settings: Option<SpatialSettings3D>,
}

impl PlayAudio3D {
    pub fn new_once(path: impl Into<String>) -> PlayAudio3D {
        PlayAudio3D {
            path: path.into(),
            volume_mult: 1.,
            one_shot: true,
            spatial_settings: None,
        }
    }

    pub fn new_repeating(path: impl Into<String>) -> PlayAudio3D {
        PlayAudio3D {
            path: path.into(),
            volume_mult: 1.,
            one_shot: false,
            spatial_settings: None,
        }
    }

    pub fn with_volume(&self, volume_mult: f32) -> PlayAudio3D {
        let mut new = self.clone();
        new.volume_mult = volume_mult;
        new
    }

    pub fn with_spatial(&self, spatial_settings: SpatialSettings3D) -> PlayAudio3D {
        let mut new = self.clone();
        new.spatial_settings = Some(spatial_settings);
        new
    }
}

impl PlayAudio for PlayAudio3D {
    fn is_one_shot(&self) -> bool {
        self.one_shot
    }
    fn volume_mult(&self) -> f32 {
        self.volume_mult
    }
    fn path(&self) -> String {
        self.path.clone()
    }
    /// Used to `return None` unconditionally, while the struct carried a
    /// `spatial_settings` field that nothing read — so every 3D spatial call was
    /// silently non-positional, in the one dimension count where positional audio
    /// is the entire point.
    fn get_spatial(&self) -> Option<SpatialSettings> {
        self.spatial_settings.map(|settings| match settings {
            SpatialSettings3D::Position(position) => {
                SpatialSettings::Position(Transform::from_translation(position))
            }
            SpatialSettings3D::Entity(entity) => SpatialSettings::Entity(entity),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_positioned_sound_reports_its_position() {
        let sound = PlayAudio3D::new_once("boom.wav")
            .with_spatial(SpatialSettings3D::Position(Vec3::new(1.0, 2.0, 3.0)));
        match sound.get_spatial() {
            Some(SpatialSettings::Position(transform)) => {
                assert_eq!(transform.translation, Vec3::new(1.0, 2.0, 3.0));
            }
            other => panic!("expected a position, got {other:?}"),
        }
    }

    #[test]
    fn an_unpositioned_sound_is_still_unpositioned() {
        assert!(PlayAudio3D::new_once("ui.wav").get_spatial().is_none());
    }
}
