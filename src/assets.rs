use bevy::prelude::*;

use crate::kart::{KART_COLORS_COUNT, KART_SIZE};
use crate::menu::lobby::BACKGROUND_ELEMENT_TYPES_COUNT;

#[derive(Resource)]
pub struct AssetHandles {
    pub numbers_texture: Handle<Image>,
    pub numbers_atlas: Handle<TextureAtlasLayout>,
    pub menu_background_texture: Handle<Image>,
    pub logo_texture: Handle<Image>,
    pub buttons_texture: Handle<Image>,
    pub buttons_atlas: Handle<TextureAtlasLayout>,
    pub arrow_texture: Handle<Image>,
    pub kick_texture: Handle<Image>,
    pub name_texture: Handle<Image>,
    pub traffic_light_texture: Handle<Image>,
    pub karts_texture: Handle<Image>,
    pub karts_atlas: Handle<TextureAtlasLayout>,
    pub wheel_texture: Handle<Image>,
    pub crate_texture: Handle<Image>,
    pub items_texture: Handle<Image>,
    pub rocket_texture: Handle<Image>,
    pub mine_texture: Handle<Image>,
    pub background_elements_texture: Handle<Image>,
    pub background_elements_atlas: Handle<TextureAtlasLayout>,
    pub clouds_texture: Handle<Image>,
    pub clouds_atlas: Handle<TextureAtlasLayout>,
}

pub fn load_assets(app: &mut App) {
    let asset_server = app.world().get_resource::<AssetServer>().unwrap().clone();
    let mut texture_atlases = app
        .world_mut()
        .get_resource_mut::<Assets<TextureAtlasLayout>>()
        .unwrap();
    let asset_handles = AssetHandles {
        numbers_texture: asset_server.load("sprites/numbers.png"),
        numbers_atlas: texture_atlases.add(TextureAtlasLayout::from_grid(
            UVec2::new(32, 32),
            10,
            1,
            None,
            None,
        )),
        menu_background_texture: asset_server.load("sprites/menu_background.png"),
        logo_texture: asset_server.load("sprites/logo.png"),
        buttons_texture: asset_server.load("sprites/buttons.png"),
        buttons_atlas: texture_atlases.add(TextureAtlasLayout::from_grid(
            UVec2::new(64, 16),
            2,
            4,
            None,
            None,
        )),
        arrow_texture: asset_server.load("sprites/arrow.png"),
        kick_texture: asset_server.load("sprites/kick.png"),
        name_texture: asset_server.load("sprites/name.png"),
        traffic_light_texture: asset_server.load("sprites/start_light.png"),
        karts_texture: asset_server.load("sprites/karts.png"),
        karts_atlas: texture_atlases.add(TextureAtlasLayout::from_grid(
            KART_SIZE,
            KART_COLORS_COUNT,
            1,
            None,
            None,
        )),
        wheel_texture: asset_server.load("sprites/wheel.png"),
        crate_texture: asset_server.load("sprites/crate.png"),
        items_texture: asset_server.load("sprites/items.png"),
        rocket_texture: asset_server.load("sprites/rocket.png"),
        mine_texture: asset_server.load("sprites/mine.png"),
        background_elements_texture: asset_server.load("sprites/nature.png"),
        background_elements_atlas: texture_atlases.add(TextureAtlasLayout::from_grid(
            UVec2::splat(8),
            BACKGROUND_ELEMENT_TYPES_COUNT as u32,
            1,
            None,
            None,
        )),
        clouds_texture: asset_server.load("sprites/clouds.png"),
        clouds_atlas: texture_atlases.add(TextureAtlasLayout::from_grid(
            UVec2::splat(32),
            2,
            1,
            None,
            None,
        )),
    };
    app.insert_resource(asset_handles);
}
