use bevy::prelude::*;

pub struct TimerPlugin;

impl Plugin for TimerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, update_timer);
    }
}

#[derive(Component)]
pub struct Timer {
    running: bool,
    elapsed: f32,
    target_duration: Option<f32>,
}

impl Timer {
    pub fn new_stopped() -> Self {
        Self {
            running: false,
            elapsed: 0.,
            target_duration: None,
        }
    }

    pub fn new_running() -> Self {
        Self {
            running: true,
            elapsed: 0.,
            target_duration: None,
        }
    }

    pub fn with_target_duration(mut self, target_duration: f32) -> Self {
        self.target_duration = Some(target_duration);
        self
    }

    pub fn elapsed(&self) -> f32 {
        self.elapsed
    }

    pub fn target_duration(&self) -> Option<f32> {
        self.target_duration
    }
}

#[derive(EntityEvent)]
pub struct TimerFinished {
    entity: Entity,
}

fn update_timer(time: Res<Time>, mut commands: Commands, mut timers: Query<(Entity, &mut Timer)>) {
    for (entity, mut timer) in timers.iter_mut() {
        if timer.running {
            let before = timer.elapsed;
            let new = before + time.delta_secs();
            if let Some(target_duration) = timer.target_duration {
                if before < target_duration && new >= target_duration {
                    commands
                        .entity(entity)
                        .trigger(|entity| TimerFinished { entity });
                }
            } else {
                commands
                    .entity(entity)
                    .trigger(|entity| TimerFinished { entity });
            }
            timer.elapsed = new;
        }
    }
}
