//! What the post-process plugin registers for the editor (the `editor`
//! feature): its environment mixer entry.

use post_editor::level_editor;
use ui_level_editor::extend::{CreateEntry, EnvironmentListing};

/// Everything the post-process plugin registers for the editor.
pub(crate) fn build(p: &mut pulsar_plugins::PluginBuilder) {
    // ── Environment mixer ───────────────────────────────────────────────────

    p.add(CreateEntry {
        origin: p.origin(),
        id: "environment_post_process_volume",
        menu_label: None,
        environment_label: Some("Post Process"),
        quick_add: None,
        offered: None,
        order: 430,
        create: level_editor::add_post_process_volume,
    });

    p.add(EnvironmentListing {
        origin: p.origin(),
        class_name: "PostProcessVolumeComponent",
        label: "Post Process Volume",
        order: 30,
        create_entry: Some("environment_post_process_volume"),
    });
}
