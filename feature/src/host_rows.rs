//! The post-process volume and camera baseline rows as SceneDB components, for standalone hosts
//! (examples, tests): each carries its contract row's exact fields
//! (`same_layout!`), so SceneDB mirrors it into the buffer the passes read.
//! Engines derive these rows from their own components instead. Behind the
//! `host-rows` feature.

use helio_components::same_layout;
use pulsar_scenedb_derive::SceneStore;

#[derive(SceneStore, bytemuck::Pod, bytemuck::Zeroable, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[gpu(layout = packed, buffer = "post_process_volumes")]
pub struct PostProcessVolumeComponent {
    #[gpu]
    pub bounds_min: [f32; 4],
    #[gpu]
    pub bounds_max: [f32; 4],
    #[gpu]
    pub priority: f32,
    #[gpu]
    pub blend_radius: f32,
    /// Zero means "inactive" -- `postprocess.wgsl`'s `cs_volume_blend`
    /// skips a row with `blend_weight <= 0.0` before ever evaluating its
    /// (otherwise degenerate, all-zero) bounds, which is what makes an
    /// unused row safely inert. See [`crate::MAX_PP_VOLUMES`]'s doc for the fixed-capacity iteration this enables.
    #[gpu]
    pub blend_weight: f32,
    #[gpu]
    pub unbound: u32,
    /// `PostProcessProperty` override bits, words 0..3 (`vec4<u32>` in WGSL).
    /// Scalars rather than `[u32; 4]`: reflection registers only specific
    /// array shapes, and this row derives reflection through `SceneStore`.
    #[gpu]
    pub override_mask_0: u32,
    #[gpu]
    pub override_mask_1: u32,
    #[gpu]
    pub override_mask_2: u32,
    #[gpu]
    pub override_mask_3: u32,
    /// `crate::GpuPostProcessUniforms`'s raw bytes (368): the packed derive
    /// needs `pulsar_scenedb::Pod` per field, which `GpuPostProcessUniforms`
    /// lacks; `bytemuck::cast` between the outer structs only needs equal
    /// size -- see `layout_matches_gpu_abi`.
    /// The settings' 92 words (`helio_pass_postprocess::GpuPostProcessUniforms`'s
    /// bytes) in the array shapes reflection registers: eleven of eight and one
    /// of four.
    #[gpu]
    pub settings_0: [u32; 8],
    #[gpu]
    pub settings_1: [u32; 8],
    #[gpu]
    pub settings_2: [u32; 8],
    #[gpu]
    pub settings_3: [u32; 8],
    #[gpu]
    pub settings_4: [u32; 8],
    #[gpu]
    pub settings_5: [u32; 8],
    #[gpu]
    pub settings_6: [u32; 8],
    #[gpu]
    pub settings_7: [u32; 8],
    #[gpu]
    pub settings_8: [u32; 8],
    #[gpu]
    pub settings_9: [u32; 8],
    #[gpu]
    pub settings_10: [u32; 8],
    #[gpu]
    pub settings_11: [u32; 4],
    /// The lens parameters the volume overrides (`helio_lens_api`'s
    /// `GpuLensVolumeOverrides` as four words).
    #[gpu]
    pub lens: [u32; 4],
}

same_layout!(PostProcessVolumeComponent, crate::PostProcessVolumeRow, crate::POST_PROCESS_VOLUMES, "post_process_volumes"; bounds_min, bounds_max, priority, blend_radius, blend_weight, unbound, override_mask_0, override_mask_1, override_mask_2, override_mask_3, lens);

/// Optional per-view baseline: the camera's own post-process settings. The first enabled row matching the camera's view id
/// (u32 bits of `jitter_frame.w`) wins; with none, the resolver's project
/// defaults apply. PP volumes then override on top. Removing the component
/// clears its packed row. `settings` holds the uniforms' bytes as
/// [`PostProcessVolumeComponent`] does.
#[derive(SceneStore, bytemuck::Pod, bytemuck::Zeroable, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[gpu(layout = packed, buffer = "camera_postprocess")]
pub struct CameraPostProcessComponent {
    #[gpu]
    pub view_id: u32,
    #[gpu]
    pub enabled: u32,
    #[gpu]
    pub _pad0: u32,
    #[gpu]
    pub _pad1: u32,
    /// The settings' 92 words (`helio_pass_postprocess::GpuPostProcessUniforms`'s
    /// bytes) in the array shapes reflection registers: eleven of eight and one
    /// of four.
    #[gpu]
    pub settings_0: [u32; 8],
    #[gpu]
    pub settings_1: [u32; 8],
    #[gpu]
    pub settings_2: [u32; 8],
    #[gpu]
    pub settings_3: [u32; 8],
    #[gpu]
    pub settings_4: [u32; 8],
    #[gpu]
    pub settings_5: [u32; 8],
    #[gpu]
    pub settings_6: [u32; 8],
    #[gpu]
    pub settings_7: [u32; 8],
    #[gpu]
    pub settings_8: [u32; 8],
    #[gpu]
    pub settings_9: [u32; 8],
    #[gpu]
    pub settings_10: [u32; 8],
    #[gpu]
    pub settings_11: [u32; 4],
}

same_layout!(CameraPostProcessComponent, crate::CameraPostProcessRow, crate::CAMERA_POSTPROCESS, "camera_postprocess"; view_id, enabled, _pad0, _pad1);

impl From<crate::GpuPostProcessVolume> for PostProcessVolumeComponent {
    fn from(value: crate::GpuPostProcessVolume) -> Self {
        crate::PostProcessVolumeRow::from(value).into()
    }
}

impl From<PostProcessVolumeComponent> for crate::GpuPostProcessVolume {
    fn from(value: PostProcessVolumeComponent) -> Self {
        crate::PostProcessVolumeRow::from(value).into()
    }
}
