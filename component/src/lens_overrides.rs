//! A post-process volume's overrides of the camera lens: only the lens's
//! artistic, blendable parameters (`helio_lens_api::overrides`). The physical
//! prescription (optics, elements, flare) stays on the camera's lens and never
//! blends, as in Unreal, whose post-process volumes blend depth of field,
//! vignette and chromatic aberration while the physical camera settings stay
//! on the camera component.
//!
//! The values are the lens plugin's contract (`GpuLensVolumeOverrides`); the
//! volume carries them in its row and the post-process blend blends them with
//! the volume's weight into the lens's overrides. They flatten into the
//! volume's saved data next to the other override switches.
use engine_class_derive::engine_class;
use helio_lens_api::{
    GpuLensVolumeOverrides, OVERRIDE_CHROMATIC_ABERRATION, OVERRIDE_FOCUS_DISTANCE,
    OVERRIDE_VIGNETTE_STRENGTH,
};

#[engine_class(no_register, clone, debug, serialize, deserialize)]
#[serde(default)]
#[category("Lens", category_color = "#E0B040")]
pub struct LensOverridesProps {
    /// Override the lens's manual focus distance.
    #[property(category = "Lens")]
    pub override_lens_focus_distance: bool,
    /// Metres. Ignored by a lens in autofocus.
    #[property(min = 0.05, max = 100000.0, step = 0.05, category = "Lens")]
    pub lens_focus_distance_m: f32,
    /// Override the amount of the lens's natural vignetting.
    #[property(category = "Lens")]
    pub override_lens_vignette_strength: bool,
    #[property(min = 0.0, max = 1.0, step = 0.01, category = "Lens")]
    pub lens_vignette_strength: f32,
    /// Override the first-order coefficient of the lens's lateral chromatic
    /// aberration.
    #[property(category = "Lens")]
    pub override_lens_chromatic_aberration: bool,
    #[property(min = 0.0, max = 0.05, step = 0.0005, category = "Lens")]
    pub lens_chromatic_aberration: f32,
}

impl Default for LensOverridesProps {
    /// Overrides nothing: a volume placed for grading must not silently
    /// replace the camera's lens.
    fn default() -> Self {
        Self {
            override_lens_focus_distance: false,
            lens_focus_distance_m: 3.0,
            override_lens_vignette_strength: false,
            lens_vignette_strength: 0.3,
            override_lens_chromatic_aberration: false,
            lens_chromatic_aberration: 0.003,
        }
    }
}

impl LensOverridesProps {
    /// The volume row's lens block.
    pub fn to_block(&self) -> GpuLensVolumeOverrides {
        let mut mask = 0;
        for (on, bit) in [
            (self.override_lens_focus_distance, OVERRIDE_FOCUS_DISTANCE),
            (
                self.override_lens_vignette_strength,
                OVERRIDE_VIGNETTE_STRENGTH,
            ),
            (
                self.override_lens_chromatic_aberration,
                OVERRIDE_CHROMATIC_ABERRATION,
            ),
        ] {
            if on {
                mask |= bit;
            }
        }
        GpuLensVolumeOverrides {
            mask,
            focus_distance_m: self.lens_focus_distance_m,
            vignette_strength: self.lens_vignette_strength,
            chromatic_aberration: self.lens_chromatic_aberration,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_overridden_by_default() {
        assert_eq!(LensOverridesProps::default().to_block().mask, 0);
    }

    #[test]
    fn each_switch_sets_its_own_bit() {
        let props = LensOverridesProps {
            override_lens_vignette_strength: true,
            override_lens_chromatic_aberration: true,
            lens_vignette_strength: 0.6,
            ..Default::default()
        };
        let block = props.to_block();
        assert_eq!(
            block.mask,
            OVERRIDE_VIGNETTE_STRENGTH | OVERRIDE_CHROMATIC_ABERRATION
        );
        assert_eq!(block.vignette_strength, 0.6);
    }
}
