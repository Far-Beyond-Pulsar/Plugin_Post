//! Post-process volumes in the level editor's environment mixer, which the
//! plugin's root registers through the level editor's extension points
//! (`ui_level_editor::extend`). Each add is one undoable command; the
//! components are this plugin's.

use ui_level_editor::LevelEditorState;
use ui_level_editor::commands::{CommandResult, TypedComponent};
use ui_level_editor::extend::add_object;
use ui_level_editor::scene_edit::{ObjectType, Transform};

/// The mixer's post-process volume: default settings in its owner's box.
pub fn add_post_process_volume(state: &mut LevelEditorState) -> CommandResult {
    add_object(
        state,
        "Post Process Volume",
        ObjectType::Empty,
        Transform::default(),
        vec![TypedComponent::new(
            post_component::PostProcessVolumeComponent::default(),
        )],
    )
}
