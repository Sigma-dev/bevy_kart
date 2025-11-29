use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct NetTransform {
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

impl From<&Transform> for NetTransform {
    fn from(t: &Transform) -> Self {
        Self {
            translation: [t.translation.x, t.translation.y, t.translation.z],
            rotation: [t.rotation.x, t.rotation.y, t.rotation.z, t.rotation.w],
            scale: [t.scale.x, t.scale.y, t.scale.z],
        }
    }
}

impl From<&NetTransform> for Transform {
    fn from(nt: &NetTransform) -> Self {
        Transform::from_xyz(nt.translation[0], nt.translation[1], nt.translation[2])
            .with_rotation(Quat::from_xyzw(
                nt.rotation[0],
                nt.rotation[1],
                nt.rotation[2],
                nt.rotation[3],
            ))
            .with_scale(Vec3::new(nt.scale[0], nt.scale[1], nt.scale[2]))
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct InstantiationDataNet<Instantiations> {
    pub transform: NetTransform,
    pub uuid: u64,
    pub instantiation: Instantiations,
}

#[derive(Clone, Debug)]
pub struct InstantiationData<Instantiations> {
    pub transform: Transform,
    pub uuid: u64,
    pub instantiation: Instantiations,
}

impl<Instantiations: Clone> From<&InstantiationData<Instantiations>>
    for InstantiationDataNet<Instantiations>
{
    fn from(value: &InstantiationData<Instantiations>) -> Self {
        Self {
            transform: NetTransform::from(&value.transform),
            uuid: value.uuid,
            instantiation: value.instantiation.clone(),
        }
    }
}

impl<Instantiations: Clone> From<&InstantiationDataNet<Instantiations>>
    for InstantiationData<Instantiations>
{
    fn from(value: &InstantiationDataNet<Instantiations>) -> Self {
        Self {
            transform: Transform::from(&value.transform),
            uuid: value.uuid,
            instantiation: value.instantiation.clone(),
        }
    }
}

#[derive(Message, Clone)]
pub(crate) struct HandleInstantiation<Instantiations>(pub InstantiationData<Instantiations>);
