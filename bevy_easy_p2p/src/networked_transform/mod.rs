use crate::api::EasyP2PData;
use crate::networked_event::NetworkedEventsExt;
use crate::schedules::EasyProcess;
use crate::{EasyP2P, NetworkedEntity, schedules::EasyHydrate};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

pub struct NetworkedTransformPlugin<T: EasyP2PData>(std::marker::PhantomData<T>);

impl<T: EasyP2PData> Default for NetworkedTransformPlugin<T> {
    fn default() -> Self {
        Self(std::marker::PhantomData)
    }
}

impl<T: EasyP2PData> Plugin for NetworkedTransformPlugin<T> {
    fn build(&self, app: &mut App) {
        app.init_networked_event::<OnNetworkedTransformUpdate, T>()
            .add_systems(EasyHydrate, apply_networked_transform::<T>)
            .add_systems(EasyProcess, send_networked_transform::<T>);
    }
}

#[derive(Component)]
pub struct NetworkedTransform;

#[derive(Message, Clone, Debug, Serialize, Deserialize)]
struct OnNetworkedTransformUpdate(u64, (Vec3, Quat));

fn send_networked_transform<'w, 's, T: EasyP2PData>(
    easy: EasyP2P<'w, 's, T>,
    transforms: Query<(&NetworkedEntity, &Transform), With<NetworkedTransform>>,
    mut events_w: MessageWriter<OnNetworkedTransformUpdate>,
) {
    if !easy.is_host() {
        return;
    }
    for (networked_entity, transform) in transforms.iter() {
        events_w.write(OnNetworkedTransformUpdate(
            networked_entity.uuid(),
            (transform.translation, transform.rotation),
        ));
    }
}

fn apply_networked_transform<'w, 's, T: EasyP2PData>(
    easy: EasyP2P<'w, 's, T>,
    mut transforms: Query<(&NetworkedEntity, &mut Transform), With<NetworkedTransform>>,
    mut events_r: MessageReader<OnNetworkedTransformUpdate>,
) {
    if easy.is_host() {
        return;
    }
    for OnNetworkedTransformUpdate(networked_id, (new_translation, new_rotation)) in events_r.read()
    {
        for (entity, mut transform) in transforms.iter_mut() {
            if entity.uuid() == *networked_id {
                transform.translation = *new_translation;
                transform.rotation = *new_rotation;
            }
        }
    }
}
