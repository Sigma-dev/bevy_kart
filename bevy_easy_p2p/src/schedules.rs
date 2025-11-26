use bevy::{app::MainScheduleOrder, ecs::schedule::ScheduleLabel, prelude::*};

pub(crate) struct EasyP2PSchedulePlugin;

#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct TransportHydrate;

#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct EasyHydrate;

#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct TransportProcess;

#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct EasyProcess;

impl Plugin for EasyP2PSchedulePlugin {
    fn build(&self, app: &mut App) {
        app.init_schedule(TransportHydrate);
        app.init_schedule(EasyHydrate);
        app.init_schedule(TransportProcess);
        app.init_schedule(EasyProcess);
        app.world_mut()
            .resource_mut::<MainScheduleOrder>()
            .insert_after(PreUpdate, TransportHydrate);
        app.world_mut()
            .resource_mut::<MainScheduleOrder>()
            .insert_after(TransportHydrate, EasyHydrate);
        app.world_mut()
            .resource_mut::<MainScheduleOrder>()
            .insert_before(PostUpdate, EasyProcess);
        app.world_mut()
            .resource_mut::<MainScheduleOrder>()
            .insert_after(EasyProcess, TransportProcess);
    }
}
