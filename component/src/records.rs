//! The post-process plugin's legacy record migrations
//! (`pulsar_class::records::RecordMigrationRegistration`), run on every level
//! (and class prefab) load with the engine's own.
//!
//! The lens left the post settings: its flare settings are the camera lens's
//! (`CameraLensComponent.flare`), and a post-process volume may override only
//! the lens's artistic parameters (`LensOverridesProps`). Saved records carry
//! the old shape flattened into their data:
//!
//! - `CameraPostProcessComponent.lens_flare` (the camera baseline's flare)
//!   moves to the object's `CameraLensComponent.flare`. Without a lens on the
//!   object, a flare that was enabled gets a flare-only lens for the same
//!   view: f/64 with its circle of confusion capped at one pixel, so the
//!   defocus changes nothing, and the flare is what the baseline showed.
//! - `PostProcessVolumeComponent.lens_flare` and `.override_lens_flare` are
//!   dropped: a volume no longer blends the flare (the physical lens stays
//!   per camera). The override masks themselves are not saved: they derive
//!   from the switches every load, and the numbers of the retired bits
//!   (63..=83) are not reused.
//!
//! Records saved after the move have none of those fields, so the migration
//! changes nothing on them.

use serde_json::{Map, Value};

const CAMERA_POST: &str = "CameraPostProcessComponent";
const VOLUME_POST: &str = "PostProcessVolumeComponent";
const LENS: &str = "CameraLensComponent";

/// What [`move_lens_flare`] did, for reports.
pub const LENS_FLARE_MOVED: &str = "moved the post-process lens flare to the camera's lens and dropped volumes' lens flare overrides";

fn class_of(entry: &Value) -> Option<&str> {
    entry.get("class_name").and_then(Value::as_str)
}

/// A flare-only lens for `view_id` carrying `flare`: the class's defaults,
/// with optics that leave the image sharp.
fn flare_only_lens(view_id: Value, flare: Value) -> Value {
    let mut data = pulsar_reflection::REGISTRY
        .create_instance(LENS)
        .and_then(|instance| instance.to_json().ok())
        .and_then(|value| match value {
            Value::Object(map) => Some(map),
            _ => None,
        })
        .unwrap_or_else(Map::new);
    data.insert("view_id".into(), view_id);
    data.insert("f_number".into(), Value::from(64.0));
    data.insert("max_coc_px".into(), Value::from(1.0));
    data.insert("flare".into(), flare);
    serde_json::json!({ "class_name": LENS, "enabled": true, "data": Value::Object(data) })
}

/// Moves a camera baseline's flare to the object's lens and drops a volume's
/// flare overrides. Returns whether the list held any of the old fields.
pub fn move_lens_flare(entries: &mut Vec<Value>) -> bool {
    let mut changed = false;
    let mut moved: Vec<(Value, Value)> = Vec::new(); // (view_id, flare)
    for entry in entries.iter_mut() {
        let class = class_of(entry).map(str::to_string);
        let Some(data) = entry.get_mut("data").and_then(Value::as_object_mut) else {
            continue;
        };
        match class.as_deref() {
            Some(CAMERA_POST) => {
                if let Some(flare) = data.remove("lens_flare") {
                    changed = true;
                    let view_id = data.get("view_id").cloned().unwrap_or(Value::from(0));
                    moved.push((view_id, flare));
                }
            }
            Some(VOLUME_POST) => {
                changed |= data.remove("lens_flare").is_some();
                changed |= data.remove("override_lens_flare").is_some();
            }
            _ => {}
        }
    }
    for (view_id, flare) in moved {
        let lens = entries.iter_mut().find(|entry| {
            class_of(entry) == Some(LENS)
                && entry
                    .get("data")
                    .and_then(|data| data.get("view_id"))
                    .map_or(view_id == Value::from(0), |lens_view| *lens_view == view_id)
        });
        match lens {
            // A lens that already has a flare (a record saved after the move)
            // keeps it.
            Some(lens) => {
                if let Some(data) = lens.get_mut("data").and_then(Value::as_object_mut) {
                    data.entry("flare").or_insert(flare);
                }
            }
            None => {
                let enabled = flare.get("enabled").and_then(Value::as_bool) == Some(true);
                if enabled {
                    entries.push(flare_only_lens(view_id, flare));
                }
            }
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn camera(view_id: u32, flare_enabled: bool) -> Value {
        json!({
            "class_name": CAMERA_POST, "enabled": true,
            "data": {
                "enabled": true, "view_id": view_id, "bloom_intensity": 0.5,
                "lens_flare": { "enabled": flare_enabled, "ghost_count": 6, "intensity": 0.8 },
            }
        })
    }

    fn volume() -> Value {
        json!({
            "class_name": VOLUME_POST, "enabled": true,
            "data": {
                "override_exposure": true, "override_lens_flare": true, "override_vignette": false,
                "lens_flare": { "enabled": true, "ghost_count": 2 },
                "bloom_intensity": 0.25,
            }
        })
    }

    #[test]
    fn a_camera_flare_moves_to_the_objects_lens() {
        let mut entries = vec![
            camera(0, true),
            json!({"class_name": LENS, "enabled": true, "data": {"view_id": 0, "f_number": 1.8}}),
        ];
        assert!(move_lens_flare(&mut entries));
        assert!(entries[0]["data"].get("lens_flare").is_none());
        assert_eq!(entries[0]["data"]["bloom_intensity"], 0.5);
        assert_eq!(entries[1]["data"]["flare"]["ghost_count"], 6);
        assert_eq!(entries[1]["data"]["f_number"], 1.8);
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn an_enabled_flare_without_a_lens_gets_a_flare_only_lens() {
        let mut entries = vec![camera(3, true)];
        assert!(move_lens_flare(&mut entries));
        assert_eq!(entries.len(), 2);
        let lens = &entries[1];
        assert_eq!(class_of(lens), Some(LENS));
        assert_eq!(lens["data"]["view_id"], 3);
        assert_eq!(lens["data"]["f_number"], 64.0);
        assert_eq!(lens["data"]["max_coc_px"], 1.0);
        assert_eq!(lens["data"]["flare"]["enabled"], true);
        assert_eq!(lens["data"]["flare"]["intensity"], 0.8);
    }

    #[test]
    fn a_disabled_flare_without_a_lens_is_dropped() {
        let mut entries = vec![camera(0, false)];
        assert!(move_lens_flare(&mut entries));
        assert_eq!(entries.len(), 1);
        assert!(entries[0]["data"].get("lens_flare").is_none());
    }

    #[test]
    fn a_lens_for_another_view_does_not_take_the_flare() {
        let mut entries = vec![
            camera(2, true),
            json!({"class_name": LENS, "enabled": true, "data": {"view_id": 5}}),
        ];
        assert!(move_lens_flare(&mut entries));
        assert_eq!(entries.len(), 3);
        assert!(entries[1]["data"].get("flare").is_none());
        assert_eq!(entries[2]["data"]["view_id"], 2);
    }

    #[test]
    fn a_volumes_flare_and_its_override_are_dropped() {
        let mut entries = vec![volume()];
        assert!(move_lens_flare(&mut entries));
        let data = &entries[0]["data"];
        assert!(data.get("lens_flare").is_none());
        assert!(data.get("override_lens_flare").is_none());
        // The rest of the volume, including its other override switches, is kept.
        assert_eq!(data["override_exposure"], true);
        assert_eq!(data["override_vignette"], false);
        assert_eq!(data["bloom_intensity"], 0.25);
    }

    #[test]
    fn records_saved_after_the_move_are_left_alone() {
        let mut entries = vec![
            json!({"class_name": CAMERA_POST, "enabled": true, "data": {"view_id": 0}}),
            json!({"class_name": VOLUME_POST, "enabled": true, "data": {"override_lens_focus_distance": true}}),
            json!({"class_name": LENS, "enabled": true, "data": {"flare": {"enabled": true}}}),
        ];
        let before = entries.clone();
        assert!(!move_lens_flare(&mut entries));
        assert_eq!(entries, before);
    }

    #[test]
    fn migrating_twice_changes_nothing_the_second_time() {
        let mut entries = vec![camera(0, true), volume()];
        assert!(move_lens_flare(&mut entries));
        let once = entries.clone();
        assert!(!move_lens_flare(&mut entries));
        assert_eq!(entries, once);
    }
}
