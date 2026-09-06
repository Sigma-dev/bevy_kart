//! The race's on-screen readouts: what place you are in, and what you are holding.
//!
//! Lifted out of `spawn_track`, which used to build the track geometry, play the
//! countdown, spawn the karts and lay out this UI in one 250-line function.
//! None of it changed on the way.

use bevy::prelude::*;

use crate::items::{HeldItem, ItemType};
use crate::kart::LocalKart;
use crate::track::position::RacePosition;
use crate::{AppState, AssetHandles};

#[derive(Component)]
pub(crate) struct HeldItemIcon;

#[derive(Component)]
pub(crate) struct PositionUI;

pub(crate) fn spawn_race_hud(
    mut commands: Commands,
    asset_handles: Res<AssetHandles>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
) {
    let texture_handle = asset_handles.items_texture.clone();
    // One icon per item variant, so the atlas follows the enum.
    let texture_atlas =
        TextureAtlasLayout::from_grid(UVec2::splat(8), ItemType::ICON_COUNT, 1, None, None);
    let texture_atlas_handle = texture_atlas_layouts.add(texture_atlas);

    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(5.),
            bottom: Val::Px(5.),
            column_gap: px(8),
            ..default()
        },
        DespawnOnExit(AppState::Game),
        children![
            PositionUI,
            (
                Node {
                    height: Val::Px(100.),
                    width: Val::Px(100.),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                BackgroundColor(Color::srgb(0.18039, 0.13333, 0.18431)),
                children![
                    (
                        ImageNode::from_atlas_image(
                            texture_handle,
                            TextureAtlas::from(texture_atlas_handle)
                        ),
                        Node {
                            height: Val::Px(80.),
                            width: Val::Px(80.),
                            ..default()
                        },
                        Visibility::Hidden,
                        HeldItemIcon,
                    )
                ],
            )
        ],
    ));
}

pub(crate) fn update_held_item_icon(
    local_kart: Query<Option<&HeldItem>, With<LocalKart>>,
    mut held_item_icon: Query<(&mut Visibility, &mut ImageNode), With<HeldItemIcon>>,
) {
    let Ok(maybe_held) = local_kart.single() else {
        return;
    };
    for (mut visibility, mut image_node) in held_item_icon.iter_mut() {
        match maybe_held.and_then(|h| h.0.as_ref()) {
            Some(item) => {
                *visibility = Visibility::Visible;
                if let Some(atlas) = image_node.texture_atlas.as_mut() {
                    atlas.index = item.to_index();
                }
            }
            None => {
                *visibility = Visibility::Hidden;
            }
        }
    }
}

pub(crate) fn update_position_ui(
    mut commands: Commands,
    asset_handles: Res<AssetHandles>,
    counter: Query<Entity, With<PositionUI>>,
    local_kart: Query<&RacePosition, With<LocalKart>>,
    mut shown: Local<Option<(Entity, u32)>>,
) {
    let (Ok(counter), Ok(local_kart)) = (counter.single(), local_kart.single()) else {
        *shown = None;
        return;
    };
    let position = local_kart.position + 1;
    // Despawning and respawning the digits every frame relaid out the whole UI
    // tree every frame. Keyed on the entity as well as the value, so a fresh
    // race (new UI entity, same position) still gets its digits.
    if *shown == Some((counter, position)) {
        return;
    }
    *shown = Some((counter, position));
    commands
        .entity(counter)
        .insert(Node {
            height: px(100),
            ..default()
        })
        .despawn_children();

    let tens_part = position / 10;
    if tens_part > 0 {
        commands
            .entity(counter)
            .with_child((ImageNode::from_atlas_image(
                asset_handles.numbers_texture.clone(),
                TextureAtlas {
                    layout: asset_handles.numbers_atlas.clone(),
                    index: tens_part as usize,
                },
            ),));
    }
    commands
        .entity(counter)
        .with_child((ImageNode::from_atlas_image(
            asset_handles.numbers_texture.clone(),
            TextureAtlas {
                layout: asset_handles.numbers_atlas.clone(),
                index: (position % 10) as usize,
            },
        ),));
}
