//! What the post-process plugin registers for the editor (the `editor`
//! feature): its environment mixer entry.

use post_editor::level_editor;
use ui_level_editor::extend::{CreateEntry, EnvironmentListing};

// ── Environment mixer ───────────────────────────────────────────────────────

inventory::submit! {
    CreateEntry {
        origin: pulsar_plugins::origin!(),
        id: "environment_post_process_volume",
        menu_label: None,
        environment_label: Some("Post Process"),
        quick_add: None,
        offered: None,
        order: 430,
        create: level_editor::add_post_process_volume,
    }
}

inventory::submit! {
    EnvironmentListing {
        origin: pulsar_plugins::origin!(),
        class_name: "PostProcessVolumeComponent",
        label: "Post Process Volume",
        order: 30,
        create_entry: Some("environment_post_process_volume"),
    }
}
