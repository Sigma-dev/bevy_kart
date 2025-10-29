use crate::{EasyP2P, NetworkedEventsExt, NetworkedId, P2PTransport};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

pub struct NetworkedTransformPlugin<
    T: P2PTransport,
    PlayerData: Serialize
        + for<'de> Deserialize<'de>
        + Clone
        + Send
        + Sync
        + core::fmt::Debug
        + 'static
        + Default
        + PartialEq,
    PlayerInputData: Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + core::fmt::Debug + 'static,
    Instantiations: Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + core::fmt::Debug + 'static,
>(std::marker::PhantomData<(T, PlayerData, PlayerInputData, Instantiations)>);

impl<T, PlayerData, PlayerInputData, Instantiations> Default
    for NetworkedTransformPlugin<T, PlayerData, PlayerInputData, Instantiations>
where
    T: P2PTransport,
    PlayerData: Serialize
        + for<'de> Deserialize<'de>
        + Clone
        + Send
        + Sync
        + core::fmt::Debug
        + 'static
        + Default
        + PartialEq,
    PlayerInputData:
        Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + core::fmt::Debug + 'static,
    Instantiations:
        Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + core::fmt::Debug + 'static,
{
    fn default() -> Self {
        Self(std::marker::PhantomData)
    }
}

impl<T, PlayerData, PlayerInputData, Instantiations> Plugin
    for NetworkedTransformPlugin<T, PlayerData, PlayerInputData, Instantiations>
where
    T: P2PTransport,
    PlayerData: Serialize
        + for<'de> Deserialize<'de>
        + Clone
        + Send
        + Sync
        + core::fmt::Debug
        + 'static
        + Default
        + PartialEq,
    PlayerInputData:
        Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + core::fmt::Debug + 'static,
    Instantiations:
        Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + core::fmt::Debug + 'static,
{
    fn build(&self, app: &mut App) {
        app.init_networked_event::<OnNetworkedTransformUpdate>()
            .add_systems(
                Update,
                (
                    networked_transform::<T, PlayerData, PlayerInputData, Instantiations>,
                    apply_networked_transform::<T, PlayerData, PlayerInputData, Instantiations>,
                ),
            );
    }
}

#[derive(Component)]
pub struct NetworkedTransform;

#[derive(Message, Clone, Debug, Serialize, Deserialize)]
struct OnNetworkedTransformUpdate(NetworkedId, (Vec3, Quat));

fn networked_transform<
    'w,
    's,
    T: P2PTransport,
    PlayerData: Serialize
        + for<'de> Deserialize<'de>
        + Clone
        + Send
        + Sync
        + core::fmt::Debug
        + 'static
        + Default
        + PartialEq,
    PlayerInputData: Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + core::fmt::Debug + 'static,
    Instantiations: Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + core::fmt::Debug + 'static,
>(
    easy: EasyP2P<'w, 's, T, PlayerData, PlayerInputData, Instantiations>,
    mut transforms: Query<(Entity, &mut Transform), With<NetworkedTransform>>,
    mut events_w: MessageWriter<OnNetworkedTransformUpdate>,
) {
    if !easy.is_host() {
        return;
    }
    for (entity, transform) in transforms.iter_mut() {
        let Some(networked_id) = easy.get_closest_networked_id(entity) else {
            continue;
        };
        events_w.write(OnNetworkedTransformUpdate(
            networked_id,
            (transform.translation, transform.rotation),
        ));
    }
}

fn apply_networked_transform<
    'w,
    's,
    T: P2PTransport,
    PlayerData: Serialize
        + for<'de> Deserialize<'de>
        + Clone
        + Send
        + Sync
        + core::fmt::Debug
        + 'static
        + Default
        + PartialEq,
    PlayerInputData: Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + core::fmt::Debug + 'static,
    Instantiations: Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync + core::fmt::Debug + 'static,
>(
    easy: EasyP2P<'w, 's, T, PlayerData, PlayerInputData, Instantiations>,
    mut transforms: Query<(Entity, &mut Transform), With<NetworkedTransform>>,
    mut events_r: MessageReader<OnNetworkedTransformUpdate>,
) {
    if easy.is_host() {
        return;
    }
    for OnNetworkedTransformUpdate(networked_id, (new_translation, new_rotation)) in events_r.read()
    {
        for (entity, mut transform) in transforms.iter_mut() {
            let closest_networked_id = easy.get_closest_networked_id(entity);
            if closest_networked_id == Some(networked_id.clone()) {
                transform.translation = *new_translation;
                transform.rotation = *new_rotation;
            }
        }
    }
}
