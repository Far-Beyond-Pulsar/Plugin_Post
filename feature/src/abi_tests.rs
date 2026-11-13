//! The volume resolver against the Rust uniform ABI, on a GPU (an
//! adapter is required; the tests fail rather than skip).

use super::*;
use helio_lens_api::*;
use helio_pass_postprocess::*;
use wgpu::util::DeviceExt;

fn gpu() -> (wgpu::Device, wgpu::Queue) {
    pollster::block_on(async {
        let instance = wgpu::Instance::new(helio_core::wgpu_instance_descriptor().with_env());
        let adapter = instance
            .request_adapter(&Default::default())
            .await
            .expect("GPU adapter required for post-process ABI regressions");
        adapter.request_device(&Default::default()).await.unwrap()
    })
}

/// `cs_volume_blend` over one unbound volume overriding a single property,
/// against the CPU blender, for every property. Both name properties by bit,
/// so this fails if either side's numbering or struct layout drifts.
#[test]
fn resolver_matches_the_rust_blender_property_by_property() {
    let (device, queue) = gpu();
    let shader = crate::volume_blend::shader(&device);
    let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };
    let storage = |read_only| wgpu::BufferBindingType::Storage { read_only };
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("PostProcess ABI layout"),
        entries: &[
            entry(0, wgpu::BufferBindingType::Uniform),
            entry(1, storage(true)),
            entry(15, storage(true)),
            entry(16, storage(false)),
            entry(17, storage(false)),
            entry(20, storage(true)),
        ],
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("PostProcess ABI resolver"),
        layout: Some(
            &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            }),
        ),
        module: &shader,
        entry_point: Some("cs_volume_blend"),
        compilation_options: Default::default(),
        cache: None,
    });

    // Baseline all zero; the volume's settings every word distinct and
    // non-zero, so overriding a property is visible and unambiguous.
    let baseline: GpuPostProcessUniforms = bytemuck::Zeroable::zeroed();
    let mut volume_settings: GpuPostProcessUniforms = bytemuck::Zeroable::zeroed();
    for (i, word) in
        bytemuck::cast_slice_mut::<_, f32>(bytemuck::bytes_of_mut(&mut volume_settings))
            .iter_mut()
            .enumerate()
    {
        *word = 1.0 + i as f32 * 0.0625;
    }
    let size = std::mem::size_of::<GpuPostProcessUniforms>() as u64;
    let buffer = |label, contents: &[u8], usage| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents,
            usage,
        })
    };
    let defaults = buffer(
        "defaults",
        bytemuck::bytes_of(&baseline),
        wgpu::BufferUsages::UNIFORM,
    );
    let cameras = buffer(
        "cameras",
        &[0u8; 2 * std::mem::size_of::<helio_core::GpuCameraUniforms>()],
        wgpu::BufferUsages::STORAGE,
    );
    let camera_rows = buffer(
        "camera rows",
        &[0u8; std::mem::size_of::<CameraPostProcessRow>()],
        wgpu::BufferUsages::STORAGE,
    );
    let output = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("output"),
        size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let lens_output = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("lens output"),
        size: std::mem::size_of::<GpuLensOverrides>() as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let lens_readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("lens readback"),
        size: std::mem::size_of::<GpuLensOverrides>() as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    // The resolver over `volumes` from the camera at the origin: the post
    // settings and the lens overrides, each against the CPU blender.
    let resolve_volumes = |volumes: &[GpuPostProcessVolume]| {
        let volumes_buffer = buffer(
            "volumes",
            bytemuck::cast_slice(volumes),
            wgpu::BufferUsages::STORAGE,
        );
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: defaults.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: cameras.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 15,
                    resource: volumes_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 16,
                    resource: output.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 17,
                    resource: lens_output.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 20,
                    resource: camera_rows.as_entire_binding(),
                },
            ],
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, size);
        encoder.copy_buffer_to_buffer(
            &lens_output,
            0,
            &lens_readback,
            0,
            std::mem::size_of::<GpuLensOverrides>() as u64,
        );
        queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
        let (lens_tx, lens_rx) = std::sync::mpsc::channel();
        lens_readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |r| lens_tx.send(r).unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        rx.recv().unwrap().unwrap();
        lens_rx.recv().unwrap().unwrap();
        let gpu = readback.slice(..).get_mapped_range().unwrap().to_vec();
        readback.unmap();
        let gpu_lens: GpuLensOverrides =
            bytemuck::pod_read_unaligned(&lens_readback.slice(..).get_mapped_range().unwrap());
        lens_readback.unmap();
        let cpu = PostProcessBlender::blend_gpu([0.0; 3], volumes, baseline);
        let cpu_lens = PostProcessBlender::blend_lens([0.0; 3], volumes);
        (gpu, bytemuck::bytes_of(&cpu).to_vec(), gpu_lens, cpu_lens)
    };
    let unbound = |override_mask: [u32; 4], lens: GpuLensVolumeOverrides, priority, weight| {
        GpuPostProcessVolume {
            bounds_min: [0.0; 4],
            bounds_max: [0.0; 4],
            priority,
            blend_radius: 0.0,
            blend_weight: weight,
            unbound: 1,
            override_mask,
            settings: volume_settings,
            lens,
        }
    };
    let resolve_mask = |override_mask: [u32; 4]| -> (Vec<u8>, Vec<u8>) {
        let volume = unbound(override_mask, GpuLensVolumeOverrides::default(), 0.0, 1.0);
        let (gpu, cpu, gpu_lens, _) = resolve_volumes(&[volume]);
        // A post property never touches the lens overrides.
        assert_eq!(gpu_lens, GpuLensOverrides::IDENTITY);
        (gpu, cpu)
    };
    let resolve = |property: usize| {
        let mut override_mask = PostProcessProperty::NONE;
        PostProcessProperty::set(&mut override_mask, property, true);
        resolve_mask(override_mask)
    };
    // With no override the resolver returns the baseline: the reference every
    // property is compared against.
    let reference = resolve_mask(PostProcessProperty::NONE).0;

    // The bytes of local exposure's pad stay reserved: no property may
    // write them.
    use std::mem::offset_of as off;
    let reserved = [off!(GpuPostProcessUniforms, pad_local_exposure)
        ..off!(GpuPostProcessUniforms, pad_local_exposure) + 4];
    for property in 0..PostProcessProperty::COUNT {
        // The fog and optics blocks' retired numbers override nothing.
        if PostProcessProperty::RETIRED
            .iter()
            .any(|range| range.contains(&property))
        {
            continue;
        }
        let (gpu, cpu) = resolve(property);
        assert_eq!(
            gpu, cpu,
            "property {property}: the resolver and the Rust blender disagree"
        );
        for range in &reserved {
            assert_eq!(
                gpu[range.clone()],
                reference[range.clone()],
                "property {property} wrote a reserved word"
            );
        }
        assert!(gpu != reference, "property {property} overrides nothing");
    }

    // Named properties land on their fields: only those bytes change.
    use std::mem::{offset_of, size_of};
    let fields: [(usize, usize, usize); 8] = [
        (
            PostProcessProperty::BLOOM_TINT,
            offset_of!(GpuPostProcessUniforms, bloom_tint),
            12,
        ),
        (
            PostProcessProperty::MOTION_BLUR_ENABLED,
            offset_of!(GpuPostProcessUniforms, motion_blur_enabled),
            4,
        ),
        (
            PostProcessProperty::BLEND_WEIGHT_EXPOSURE,
            offset_of!(GpuPostProcessUniforms, blend_weight_exposure),
            4,
        ),
        (
            PostProcessProperty::GAIN_COLOR,
            offset_of!(GpuPostProcessUniforms, gain_color),
            12,
        ),
        (
            PostProcessProperty::LUT_PLATFORM,
            offset_of!(GpuPostProcessUniforms, lut_platform),
            4,
        ),
        (
            PostProcessProperty::LOCAL_EXPOSURE_HIGHLIGHT_CONTRAST,
            offset_of!(GpuPostProcessUniforms, local_exposure_highlight_contrast),
            4,
        ),
        (
            PostProcessProperty::LOCAL_EXPOSURE_SHADOW_CONTRAST,
            offset_of!(GpuPostProcessUniforms, local_exposure_shadow_contrast),
            4,
        ),
        (
            PostProcessProperty::LOCAL_EXPOSURE_DETAIL_STRENGTH,
            offset_of!(GpuPostProcessUniforms, local_exposure_detail_strength),
            4,
        ),
    ];
    for (property, offset, len) in fields {
        let (gpu, _) = resolve(property);
        let changed: Vec<usize> = (0..gpu.len()).filter(|&i| gpu[i] != reference[i]).collect();
        assert!(
            !changed.is_empty() && changed.iter().all(|i| (offset..offset + len).contains(i)),
            "property {property} should change only bytes {offset}..{}: {changed:?}",
            offset + len
        );
    }
    assert_eq!(size_of::<GpuPostProcessUniforms>(), 368);
    assert_eq!(offset_of!(GpuPostProcessUniforms, hdr_output_mode), 272);
    assert_eq!(
        offset_of!(GpuPostProcessUniforms, local_exposure_highlight_contrast),
        192
    );

    // The lens overrides: two overlapping volumes with different priorities
    // and weights, each overriding some of the three values, through the
    // resolver and the lens contract's own blend. The post settings of the
    // same volumes (here unmasked) stay the baseline.
    let none = PostProcessProperty::NONE;
    let first = unbound(
        none,
        GpuLensVolumeOverrides {
            mask: OVERRIDE_FOCUS_DISTANCE | OVERRIDE_CHROMATIC_ABERRATION,
            focus_distance_m: 12.0,
            vignette_strength: 0.9,
            chromatic_aberration: 0.01,
        },
        1.0,
        0.5,
    );
    let second = unbound(
        none,
        GpuLensVolumeOverrides {
            mask: OVERRIDE_FOCUS_DISTANCE | OVERRIDE_VIGNETTE_STRENGTH,
            focus_distance_m: 2.0,
            vignette_strength: 0.7,
            chromatic_aberration: 0.5,
        },
        2.0,
        0.25,
    );
    // Row order is reversed against priority: the resolver sorts.
    let (gpu, cpu, gpu_lens, cpu_lens) = resolve_volumes(&[second, first]);
    assert_eq!(gpu, cpu);
    for (a, b) in bytemuck::bytes_of(&gpu_lens)
        .chunks(4)
        .zip(bytemuck::bytes_of(&cpu_lens).chunks(4))
    {
        let (a, b) = (
            f32::from_le_bytes(a.try_into().unwrap()),
            f32::from_le_bytes(b.try_into().unwrap()),
        );
        assert!((a - b).abs() < 1e-6, "{gpu_lens:?} vs {cpu_lens:?}");
    }
    // Against the camera's own focus 4 m: the first (priority 1) blends 4 -> 12
    // by 0.5, the second (priority 2) then 8 -> 2 by 0.25.
    let focus = helio_lens_api::apply(gpu_lens.focus_distance, 4.0);
    assert!((focus - (8.0 + (2.0 - 8.0) * 0.25)).abs() < 1e-4, "{focus}");
    // Only the second overrides the vignette, only the first the aberration.
    let vignette = helio_lens_api::apply(gpu_lens.vignette_strength, 0.2);
    assert!(
        (vignette - (0.2 + (0.7 - 0.2) * 0.25)).abs() < 1e-5,
        "{vignette}"
    );
    let aberration = helio_lens_api::apply(gpu_lens.chromatic_aberration, 0.002);
    assert!(
        (aberration - (0.002 + (0.01 - 0.002) * 0.5)).abs() < 1e-6,
        "{aberration}"
    );
}
