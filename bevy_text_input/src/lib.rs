use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
    },
    prelude::*,
};

pub struct TextInputPlugin;

pub mod prelude;

impl Plugin for TextInputPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                handle_inputs,
                handle_default_focus,
                handle_focus,
                handle_inputs_size,
            ),
        );
    }
}

#[derive(EntityEvent)]
pub struct InputFieldChange {
    entity: Entity,
    text: String,
}

impl InputFieldChange {
    pub fn text(&self) -> &str {
        &self.text
    }
}

#[derive(EntityEvent)]
pub struct InputFieldSubmit {
    entity: Entity,
    text: String,
}

impl InputFieldSubmit {
    pub fn text(&self) -> &str {
        &self.text
    }
}

#[derive(Component)]
#[require(Text)]
pub struct TextInput {
    capitalize: bool,
    no_spaces: bool,
    max_characters: Option<usize>,
    can_submit: bool,
}

impl TextInput {
    pub fn new(capitalize: bool, no_spaces: bool, can_submit: bool) -> Self {
        Self {
            capitalize,
            no_spaces,
            max_characters: None,
            can_submit,
        }
    }

    pub fn with_max_characters(mut self, max_characters: usize) -> Self {
        self.max_characters = Some(max_characters);
        self
    }
}

#[derive(Resource)]
struct TextInputFocus(Entity);

fn handle_inputs(
    mut commands: Commands,
    focus: Option<Res<TextInputFocus>>,
    mut evr_kbd: MessageReader<KeyboardInput>,
    mut inputs: Query<(Entity, &mut Text, &TextInput)>,
) {
    let Some(focus) = focus.map(|focus| focus.0) else {
        return;
    };
    for ev in evr_kbd.read() {
        // We don't care about key releases, only key presses
        if ev.state == ButtonState::Released {
            continue;
        }
        for (entity, mut input, input_field) in inputs.iter_mut() {
            if entity != focus {
                continue;
            }

            let mut write_char = |str: &str| {
                if input_field.max_characters.is_some()
                    && input.0.len() >= input_field.max_characters.unwrap()
                {
                    return;
                }
                input.0.push_str(str);
                commands.entity(entity).trigger(|entity| InputFieldChange {
                    entity: entity,
                    text: input.0.clone(),
                });
            };
            match &ev.logical_key {
                // Handle pressing Enter to finish the input
                Key::Enter => {
                    if !input_field.can_submit {
                        continue;
                    }
                    commands.entity(entity).trigger(|entity| InputFieldSubmit {
                        entity: entity,
                        text: input.0.clone(),
                    });

                    input.0.clear();
                    commands.entity(entity).trigger(|entity| InputFieldChange {
                        entity: entity,
                        text: input.0.clone(),
                    });
                }
                // Handle pressing Backspace to delete last char
                Key::Backspace => {
                    input.0.pop();
                    commands.entity(entity).trigger(|entity| InputFieldChange {
                        entity: entity,
                        text: input.0.clone(),
                    });
                }
                Key::Space => {
                    if !input_field.no_spaces {
                        write_char(" ");
                    }
                }
                // Handle key presses that produce text characters
                Key::Character(str) => {
                    // Ignore any input that contains control (special) characters
                    if str.chars().any(|c| c.is_control()) {
                        continue;
                    }
                    if input_field.capitalize {
                        write_char(str.to_uppercase().as_str());
                    } else {
                        write_char(str.as_str());
                    }
                }
                _ => {}
            }
        }
    }
}

fn handle_inputs_size(mut inputs: Query<(&mut Node, &TextInput)>) {
    for (mut node, input) in inputs.iter_mut() {
        if let Some(max_characters) = input.max_characters {
            node.width = px(max_characters as f32 * 12.0);
        }
    }
}

fn handle_default_focus(mut commands: Commands, inputs: Query<Entity, With<TextInput>>) {
    match inputs.iter().len() {
        0 => {
            commands.remove_resource::<TextInputFocus>();
        }
        1 => {
            commands.insert_resource(TextInputFocus(inputs.iter().next().unwrap()));
        }
        _ => {}
    }
}

fn handle_focus(
    mut commands: Commands,
    mut pressed_r: MessageReader<Pointer<Press>>,
    inputs: Query<Entity, With<TextInput>>,
) {
    for pointer in pressed_r.read() {
        if inputs.contains(pointer.entity) {
            commands.insert_resource(TextInputFocus(pointer.entity));
        }
    }
}
