//! The post-process plugin's passes are nodes of render graphs: registered by the plugin's
//! root through its render feature (with the feature that owns each, the
//! default placement and the pins its pass declares), in the default graph in
//! the order the passes ran in before graphs were data, built from a graph
//! asset, bypassed by their Enabled parameter, and absent, with the reason,
//! while the plugin is disabled.

// The plugin as a game links it: its root, with every registration.
use post_plugin as _;

use std::sync::{Arc, Mutex};

use engine_backend::subsystems::render::helio_renderer::{pipelines, render_features};
use helio::{DebugDrawState, PassBuildContext, RendererConfig};
use helio_graph_asset::{GraphAsset, NodeInstance, PinKind, validate};
use pulsar_scenedb::gpu::{EngineGpuContext, GpuMirrorHandle, SceneGpuConfig, SceneGpuStore};

const PLUGIN: &str = "pulsar.post";

/// What each node is: kind, the feature that registers it, and the pass type
/// it builds (the end of the type's name), in default graph order.
const NODES: &[(&str, &str, &str)] = &[(
    "helio.pass.post_process_volume_blend",
    "helio.post_volumes",
    "PostProcessVolumeBlendPass",
)];

/// The plugin gate is process-wide state and a wgpu device is one at a time
/// (Pulsar-Native#1647): every test takes this first.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

fn device() -> Option<(Arc<wgpu::Device>, Arc<wgpu::Queue>)> {
    pollster::block_on(async {
        let instance = wgpu::Instance::new(helio_core::wgpu_instance_descriptor());
        let adapter = instance.request_adapter(&Default::default()).await.ok()?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_features: helio::required_wgpu_features(adapter.features()),
                required_limits: helio::required_wgpu_limits(adapter.limits()),
                experimental_features: helio::required_experimental_features(adapter.features()),
                ..Default::default()
            })
            .await
            .ok()?;
        Some((Arc::new(device), Arc::new(queue)))
    })
}

fn buffer(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::UNIFORM
            | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn configured() -> RendererConfig {
    // The renderer config with the plugins' settings, as a host makes it.
    render_features::configure(RendererConfig::new(
        320,
        180,
        wgpu::TextureFormat::Rgba8Unorm,
    ))
}

fn registry() -> helio_graph_asset::NodeRegistry {
    helio_presets::graphs::registry_with(&render_features::plugin_features())
}

/// The deferred preset's graph over the registry.
fn default_asset(registry: &helio_graph_asset::NodeRegistry) -> GraphAsset {
    helio_presets::graphs::deferred_graph_asset(registry)
}

/// Compiles `asset` and counts, per node of [`NODES`], the passes of its type
/// in the graph (`None`: no GPU).
fn pass_counts(asset: &GraphAsset) -> Option<Vec<usize>> {
    let (device, queue) = device()?;
    let gpu = EngineGpuContext::new(Arc::clone(&device), Arc::clone(&queue));
    let store = SceneGpuStore::new(
        &gpu,
        SceneGpuConfig {
            classes: Vec::new(),
            tombstone_headroom: 0,
            max_cells_metadata: 0,
        },
    );
    let camera = buffer(&device, 512);
    let debug_camera = buffer(&device, 64);
    let cull_stats = buffer(&device, 64);
    let mut env = helio_graph_asset::build_env(&PassBuildContext {
        device: &device,
        queue: &queue,
        config: configured(),
        debug_state: Arc::new(Mutex::new(DebugDrawState::default())),
        camera_buffer: &camera,
        debug_camera_buffer: &debug_camera,
        cull_stats_buffer: &cull_stats,
        owns_device: false,
        scene_db: GpuMirrorHandle::new(Arc::new(store), Arc::clone(&queue)),
    });
    let graph = helio_graph_asset::compile(asset, &registry(), &mut env)
        .unwrap_or_else(|errors| panic!("the graph builds: {errors:?}"));
    let types: Vec<String> = (0..graph.pass_count())
        .filter_map(|index| graph.pass_declaration(index))
        .map(|pass| pass.type_name.to_string())
        .collect();
    Some(
        NODES
            .iter()
            .map(|(_, _, pass)| types.iter().filter(|t| t.ends_with(pass)).count())
            .collect(),
    )
}

#[test]
fn the_plugin_registers_its_nodes_in_the_default_graph_order() {
    let _serial = serial();
    pulsar_plugins::deactivate();
    let registry = registry();
    let asset = default_asset(&registry);
    let mut last = None;
    for (kind, feature, _) in NODES {
        let node = registry
            .get(kind)
            .unwrap_or_else(|| panic!("{kind} is registered"));
        assert_eq!(registry.feature_of(kind), Some(*feature), "{kind}");
        assert!(
            node.generated,
            "{kind}: pins come from what the pass declares"
        );
        assert!(node.params.iter().any(|p| p.key == "enabled"), "{kind}");
        let at = asset
            .nodes
            .iter()
            .position(|n| n.kind == *kind)
            .unwrap_or_else(|| panic!("the default graph holds {kind}"));
        assert!(
            last.is_none_or(|last| last < at),
            "{kind} is out of the default pass order"
        );
        last = Some(at);
    }
}

#[test]
fn the_graph_builds_the_passes_and_enabled_off_bypasses_them() {
    let _serial = serial();
    pulsar_plugins::deactivate();
    let registry = registry();
    let mut asset = default_asset(&registry);
    let Some(built) = pass_counts(&asset) else {
        eprintln!("GPU_VALIDATION_SKIPPED_NO_ADAPTER: {PLUGIN} graph nodes");
        return;
    };
    for ((kind, _, pass), count) in NODES.iter().zip(&built) {
        eprintln!("{kind} builds {count} {pass}");
    }
    assert_eq!(built, vec![1], "the node builds the volume blend pass");

    // The Enabled parameter bypasses every one of the plugin's nodes.
    for node in asset
        .nodes
        .iter_mut()
        .filter(|n| NODES.iter().any(|(kind, _, _)| n.kind == *kind))
    {
        node.params.insert("enabled".into(), false.into());
    }
    assert_eq!(
        pass_counts(&asset),
        Some(vec![0; NODES.len()]),
        "no pass of the plugin builds"
    );
}

#[test]
fn the_probe_gives_each_node_the_pins_its_pass_declares() {
    let _serial = serial();
    pulsar_plugins::deactivate();
    let registry = pipelines::graph_node_registry();
    if pipelines::graph_pin_probe().is_none() {
        eprintln!("GPU_VALIDATION_SKIPPED_NO_ADAPTER: {PLUGIN} node pins");
        return;
    }
    for (kind, _, _) in NODES {
        let node = registry
            .get(kind)
            .unwrap_or_else(|| panic!("{kind} is registered"));
        let pins = node.pins(&Default::default());
        assert!(
            pins.inputs.iter().any(|p| p.ty.kind == PinKind::Flow),
            "{kind}: {pins:?}"
        );
        assert!(
            pins.outputs.iter().any(|p| p.ty.kind == PinKind::Flow),
            "{kind}: {pins:?}"
        );
        eprintln!("{kind} pins: {pins:?}");
    }
}

#[test]
fn a_disabled_plugin_has_no_nodes_and_a_graph_naming_them_says_so() {
    let _serial = serial();
    pulsar_plugins::deactivate();
    let with_plugin = registry();
    let mut asset = default_asset(&with_plugin);
    asset
        .nodes
        .retain(|n| !NODES.iter().any(|(kind, _, _)| n.kind == *kind));
    for (i, (kind, _, _)) in NODES.iter().enumerate() {
        asset.nodes.push(NodeInstance {
            id: format!("plugin_node_{i}"),
            kind: (*kind).into(),
            params: Default::default(),
            position: [0.0; 2],
        });
    }

    pulsar_plugins::activate(&pulsar_plugins::resolve(
        pulsar_plugins::installed(),
        &pulsar_plugins::ProjectPlugins::default().with(&[PLUGIN], false),
    ));
    let registry = pipelines::graph_node_registry();
    pulsar_plugins::deactivate();

    let validation = validate(&asset, &registry);
    let messages: Vec<_> = validation.errors().map(|d| d.message.clone()).collect();
    for (kind, _, _) in NODES {
        assert!(registry.get(kind).is_none(), "{kind} is unregistered");
        assert_eq!(
            registry.absence(kind),
            Some(format!("plugin {PLUGIN} disabled").as_str())
        );
        let expected = format!("missing node `{kind}` (plugin {PLUGIN} disabled)");
        assert!(messages.contains(&expected), "{expected}: {messages:?}");
    }
    // Enabled again, the registry follows.
    assert!(pipelines::graph_node_registry().get(NODES[0].0).is_some());
}
