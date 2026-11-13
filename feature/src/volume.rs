//! A post-process volume as the volume blend reads it: its row ABI
//! ([`GpuPostProcessVolume`], the settings it overrides embedded), its CPU
//! descriptor, and the CPU reference of the GPU blend ([`PostProcessBlender`]).
//! The settings themselves and their override bits are the frame's
//! post-settings contract (`helio_pass_postprocess`).

use bytemuck::{Pod, Zeroable};
use helio_lens_api::{GpuLensOverrides, GpuLensVolumeOverrides};
use helio_pass_postprocess::{GpuPostProcessUniforms, PostProcessProperty, PostProcessSettings};

// ── GpuPostProcessVolume ───────────────────────────────────────────────────────
//
// Per-volume GPU struct read by the cs_volume_blend compute shader.
// The `unbound` flag marks volumes that apply regardless of camera position.

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct GpuPostProcessVolume {
    pub bounds_min: [f32; 4],
    pub bounds_max: [f32; 4],
    pub priority: f32,
    pub blend_radius: f32,
    pub blend_weight: f32, // 0-1, global volume opacity
    pub unbound: u32,      // 0 = bounded, 1 = applies everywhere
    // 16 bytes, not 8. `settings` contains vec3s, so WGSL gives it 16-byte
    // alignment and places it at offset 64 — while #[repr(C)] would happily put
    // it at 56 after an 8-byte pad. That 8-byte skew made the GPU read every
    // volume's settings shifted (and stride 424 against WGSL's 432, so each
    // successive volume drifted further). Padding to 64 here makes both sides
    // agree; the asserts below keep them that way.
    pub override_mask: [u32; 4],
    pub settings: GpuPostProcessUniforms,
    /// The lens parameters this volume may override (`helio_lens_api`): the
    /// lens plugin's blendable block, blended into the lens's overrides
    /// rather than the post settings.
    pub lens: GpuLensVolumeOverrides,
}

// WGSL places `settings` at 64 because GpuPostProcessUniforms aligns to 16.
const _: () = assert!(std::mem::offset_of!(GpuPostProcessVolume, settings) == 64);
// Storage-buffer array stride must match WGSL's, which rounds to the 16-byte alignment.
// 64 (header) + 368 (settings) + 16 (the lens block) = 448.
const _: () = assert!(std::mem::offset_of!(GpuPostProcessVolume, lens) == 432);
const _: () = assert!(std::mem::size_of::<GpuPostProcessVolume>() == 448);
const _: () = assert!(std::mem::size_of::<GpuPostProcessVolume>() % 16 == 0);

// ── PostProcessVolume descriptor (CPU-side) ────────────────────────────────────

#[derive(Clone, Debug)]
pub struct PostProcessVolumeDescriptor {
    pub bounds_min: [f32; 3],
    pub bounds_max: [f32; 3],
    pub priority: f32,
    pub blend_radius: f32,
    pub blend_weight: f32,
    pub unbound: bool, // infinite volume (camera always inside)
    pub override_mask: [u32; 4],
    pub settings: PostProcessSettings,
    pub lens: GpuLensVolumeOverrides,
}

impl Default for PostProcessVolumeDescriptor {
    fn default() -> Self {
        Self {
            bounds_min: [-1000.0, -1000.0, -1000.0],
            bounds_max: [1000.0, 1000.0, 1000.0],
            priority: 0.0,
            blend_radius: 200.0,
            blend_weight: 1.0,
            unbound: false,
            override_mask: PostProcessProperty::ALL,
            settings: PostProcessSettings::default(),
            lens: GpuLensVolumeOverrides::default(),
        }
    }
}

impl PostProcessVolumeDescriptor {
    pub fn to_gpu(&self) -> GpuPostProcessVolume {
        GpuPostProcessVolume {
            bounds_min: [
                self.bounds_min[0],
                self.bounds_min[1],
                self.bounds_min[2],
                0.0,
            ],
            bounds_max: [
                self.bounds_max[0],
                self.bounds_max[1],
                self.bounds_max[2],
                0.0,
            ],
            priority: self.priority,
            blend_radius: self.blend_radius,
            blend_weight: self.blend_weight,
            unbound: self.unbound as u32,
            override_mask: self.override_mask,
            settings: self.settings.to_gpu(),
            lens: self.lens,
        }
    }
}

/// CPU reference for the production GPU resolver. Equal priorities retain input row order.
pub struct PostProcessBlender;
impl PostProcessBlender {
    pub fn blend(
        camera_pos: [f32; 3],
        volumes: &[GpuPostProcessVolume],
        camera_settings: &PostProcessSettings,
    ) -> GpuPostProcessUniforms {
        Self::blend_gpu(camera_pos, volumes, camera_settings.to_gpu())
    }

    pub fn blend_gpu(
        camera_pos: [f32; 3],
        volumes: &[GpuPostProcessVolume],
        baseline: GpuPostProcessUniforms,
    ) -> GpuPostProcessUniforms {
        let mut ordered: Vec<_> = volumes
            .iter()
            .filter(|v| {
                v.priority.is_finite() && v.blend_weight.is_finite() && v.blend_weight > 0.0
            })
            .collect();
        ordered.sort_by(|a, b| a.priority.partial_cmp(&b.priority).unwrap());
        let mut result = baseline;
        for v in ordered {
            let weight = volume_weight(camera_pos, v);
            if weight > 0.0 {
                result = blend_properties(result, &v.settings, weight, &v.override_mask);
            }
        }
        result
    }
}

pub fn volume_weight(pos: [f32; 3], v: &GpuPostProcessVolume) -> f32 {
    if !v.blend_weight.is_finite() || v.blend_weight <= 0.0 {
        return 0.0;
    }
    let weight = v.blend_weight.clamp(0.0, 1.0);
    if v.unbound != 0 {
        return weight;
    }
    let mut boundary = f32::INFINITY;
    for axis in 0..3 {
        if !(pos[axis] >= v.bounds_min[axis] && pos[axis] <= v.bounds_max[axis]) {
            return 0.0;
        }
        boundary = boundary
            .min(pos[axis] - v.bounds_min[axis])
            .min(v.bounds_max[axis] - pos[axis]);
    }
    if v.blend_radius > 0.0 {
        weight * (boundary / v.blend_radius).clamp(0.0, 1.0)
    } else {
        weight
    }
}

fn blend_properties(
    mut a: GpuPostProcessUniforms,
    b: &GpuPostProcessUniforms,
    t: f32,
    mask: &[u32; 4],
) -> GpuPostProcessUniforms {
    if PostProcessProperty::contains(mask, 0) {
        if t >= 0.5 {
            a.exposure_mode = b.exposure_mode;
        }
    }
    if PostProcessProperty::contains(mask, 1) {
        a.exposure_compensation += (b.exposure_compensation - a.exposure_compensation) * t;
    }
    if PostProcessProperty::contains(mask, 2) {
        a.exposure_min += (b.exposure_min - a.exposure_min) * t;
    }
    if PostProcessProperty::contains(mask, 3) {
        a.exposure_max += (b.exposure_max - a.exposure_max) * t;
    }
    if PostProcessProperty::contains(mask, 4) {
        a.bloom_intensity += (b.bloom_intensity - a.bloom_intensity) * t;
    }
    if PostProcessProperty::contains(mask, 5) {
        a.bloom_threshold += (b.bloom_threshold - a.bloom_threshold) * t;
    }
    if PostProcessProperty::contains(mask, 6) {
        a.bloom_knee += (b.bloom_knee - a.bloom_knee) * t;
    }
    if PostProcessProperty::contains(mask, 7) {
        a.bloom_radius += (b.bloom_radius - a.bloom_radius) * t;
    }
    if PostProcessProperty::contains(mask, 8) {
        for c in 0..3 {
            a.bloom_tint[c] += (b.bloom_tint[c] - a.bloom_tint[c]) * t;
        }
    }
    if PostProcessProperty::contains(mask, 9) {
        if t >= 0.5 {
            a.bloom_enabled = b.bloom_enabled;
        }
    }
    if PostProcessProperty::contains(mask, 10) {
        for c in 0..3 {
            a.color_saturation[c] += (b.color_saturation[c] - a.color_saturation[c]) * t;
        }
    }
    if PostProcessProperty::contains(mask, 11) {
        a.exposure_speed_up += (b.exposure_speed_up - a.exposure_speed_up) * t;
    }
    if PostProcessProperty::contains(mask, 12) {
        for c in 0..3 {
            a.color_contrast[c] += (b.color_contrast[c] - a.color_contrast[c]) * t;
        }
    }
    if PostProcessProperty::contains(mask, 13) {
        a.exposure_speed_down += (b.exposure_speed_down - a.exposure_speed_down) * t;
    }
    if PostProcessProperty::contains(mask, 14) {
        for c in 0..3 {
            a.color_gamma[c] += (b.color_gamma[c] - a.color_gamma[c]) * t;
        }
    }
    if PostProcessProperty::contains(mask, 15) {
        for c in 0..3 {
            a.color_gain[c] += (b.color_gain[c] - a.color_gain[c]) * t;
        }
    }
    if PostProcessProperty::contains(mask, 16) {
        for c in 0..3 {
            a.color_offset[c] += (b.color_offset[c] - a.color_offset[c]) * t;
        }
    }
    if PostProcessProperty::contains(mask, 17) {
        a.white_temp += (b.white_temp - a.white_temp) * t;
    }
    if PostProcessProperty::contains(mask, 18) {
        a.white_tint += (b.white_tint - a.white_tint) * t;
    }
    if PostProcessProperty::contains(mask, 19) {
        if t >= 0.5 {
            a.white_balance_enabled = b.white_balance_enabled;
        }
    }
    if PostProcessProperty::contains(mask, 20) {
        if t >= 0.5 {
            a.tonemap_operator = b.tonemap_operator;
        }
    }
    if PostProcessProperty::contains(mask, 21) {
        a.tonemap_exposure += (b.tonemap_exposure - a.tonemap_exposure) * t;
    }
    if PostProcessProperty::contains(mask, 22) {
        a.tonemap_white_point += (b.tonemap_white_point - a.tonemap_white_point) * t;
    }
    if PostProcessProperty::contains(mask, 23) {
        a.vignette_intensity += (b.vignette_intensity - a.vignette_intensity) * t;
    }
    if PostProcessProperty::contains(mask, 24) {
        a.vignette_smoothness += (b.vignette_smoothness - a.vignette_smoothness) * t;
    }
    if PostProcessProperty::contains(mask, 25) {
        a.vignette_roundness += (b.vignette_roundness - a.vignette_roundness) * t;
    }
    if PostProcessProperty::contains(mask, 26) {
        for c in 0..3 {
            a.vignette_color[c] += (b.vignette_color[c] - a.vignette_color[c]) * t;
        }
    }
    if PostProcessProperty::contains(mask, 27) {
        if t >= 0.5 {
            a.vignette_enabled = b.vignette_enabled;
        }
    }
    if PostProcessProperty::contains(mask, 28) {
        a.grain_intensity += (b.grain_intensity - a.grain_intensity) * t;
    }
    if PostProcessProperty::contains(mask, 29) {
        a.grain_response += (b.grain_response - a.grain_response) * t;
    }
    if PostProcessProperty::contains(mask, 30) {
        a.grain_size += (b.grain_size - a.grain_size) * t;
    }
    if PostProcessProperty::contains(mask, 31) {
        if t >= 0.5 {
            a.grain_enabled = b.grain_enabled;
        }
    }
    if PostProcessProperty::contains(mask, 32) {
        a.motion_blur_amount += (b.motion_blur_amount - a.motion_blur_amount) * t;
    }
    if PostProcessProperty::contains(mask, 33) {
        a.motion_blur_max += (b.motion_blur_max - a.motion_blur_max) * t;
    }
    if PostProcessProperty::contains(mask, 34) {
        if t >= 0.5 {
            a.motion_blur_enabled = b.motion_blur_enabled;
        }
    }
    if PostProcessProperty::contains(mask, 35) {
        a.blend_weight_bloom += (b.blend_weight_bloom - a.blend_weight_bloom) * t;
    }
    if PostProcessProperty::contains(mask, 36) {
        a.blend_weight_motion_blur += (b.blend_weight_motion_blur - a.blend_weight_motion_blur) * t;
    }
    if PostProcessProperty::contains(mask, 37) {
        a.blend_weight_vignette += (b.blend_weight_vignette - a.blend_weight_vignette) * t;
    }
    if PostProcessProperty::contains(mask, 38) {
        a.blend_weight_grain += (b.blend_weight_grain - a.blend_weight_grain) * t;
    }
    if PostProcessProperty::contains(mask, 39) {
        a.blend_weight_exposure += (b.blend_weight_exposure - a.blend_weight_exposure) * t;
    }
    if PostProcessProperty::contains(mask, 50) {
        if t >= 0.5 {
            a.hdr_output_mode = b.hdr_output_mode;
        }
    }
    if PostProcessProperty::contains(mask, 51) {
        a.hdr_max_nits += (b.hdr_max_nits - a.hdr_max_nits) * t;
    }
    if PostProcessProperty::contains(mask, 52) {
        a.hdr_ui_brightness += (b.hdr_ui_brightness - a.hdr_ui_brightness) * t;
    }
    if PostProcessProperty::contains(mask, 53) {
        for c in 0..3 {
            a.lift_color[c] += (b.lift_color[c] - a.lift_color[c]) * t;
        }
    }
    if PostProcessProperty::contains(mask, 54) {
        for c in 0..3 {
            a.gamma_color[c] += (b.gamma_color[c] - a.gamma_color[c]) * t;
        }
    }
    if PostProcessProperty::contains(mask, 55) {
        for c in 0..3 {
            a.gain_color[c] += (b.gain_color[c] - a.gain_color[c]) * t;
        }
    }
    if PostProcessProperty::contains(mask, 56) {
        a.shadows_max += (b.shadows_max - a.shadows_max) * t;
    }
    if PostProcessProperty::contains(mask, 57) {
        a.highlights_min += (b.highlights_min - a.highlights_min) * t;
    }
    if PostProcessProperty::contains(mask, 58) {
        a.shadow_highlight_balance += (b.shadow_highlight_balance - a.shadow_highlight_balance) * t;
    }
    if PostProcessProperty::contains(mask, 59) {
        a.hue_shift += (b.hue_shift - a.hue_shift) * t;
    }
    if PostProcessProperty::contains(mask, 60) {
        if t >= 0.5 {
            a.lut_generation = b.lut_generation;
        }
    }
    if PostProcessProperty::contains(mask, 61) {
        a.lut_intensity += (b.lut_intensity - a.lut_intensity) * t;
    }
    if PostProcessProperty::contains(mask, 62) {
        if t >= 0.5 {
            a.lut_platform = b.lut_platform;
        }
    }
    if PostProcessProperty::contains(mask, 84) {
        a.local_exposure_highlight_contrast +=
            (b.local_exposure_highlight_contrast - a.local_exposure_highlight_contrast) * t;
    }
    if PostProcessProperty::contains(mask, 85) {
        a.local_exposure_shadow_contrast +=
            (b.local_exposure_shadow_contrast - a.local_exposure_shadow_contrast) * t;
    }
    if PostProcessProperty::contains(mask, 86) {
        a.local_exposure_detail_strength +=
            (b.local_exposure_detail_strength - a.local_exposure_detail_strength) * t;
    }
    a
}

impl PostProcessBlender {
    /// CPU reference for the GPU blend of the volumes' lens overrides: the
    /// same volumes, order and weights as [`Self::blend_gpu`], blended with
    /// the lens contract's rule (`helio_lens_api::GpuLensOverrides::blend`).
    pub fn blend_lens(camera_pos: [f32; 3], volumes: &[GpuPostProcessVolume]) -> GpuLensOverrides {
        let mut ordered: Vec<_> = volumes
            .iter()
            .filter(|v| {
                v.priority.is_finite() && v.blend_weight.is_finite() && v.blend_weight > 0.0
            })
            .collect();
        ordered.sort_by(|a, b| a.priority.partial_cmp(&b.priority).unwrap());
        let mut result = GpuLensOverrides::IDENTITY;
        for v in ordered {
            let weight = volume_weight(camera_pos, v);
            if weight > 0.0 {
                result = result.blend(&v.lens, weight);
            }
        }
        result
    }
}

/// Maximum active volumes blended per frame. SceneDB buffers can be larger:
/// rows are addressed by sparse entity index, so shaders scan the full buffer
/// before compacting into this fixed active-volume budget.
pub const MAX_PP_VOLUMES: u32 = 64;
const _: () = assert!(MAX_PP_VOLUMES == 64);
