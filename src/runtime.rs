//! What the post-process plugin registers for a game (and the editor):
//! every runtime contribution, grouped by the engine seam it plugs into.

use engine_backend::scene::SceneRowsRegistration;
use engine_backend::scene::{ClassSpec, ComponentsBuilder};
use engine_backend::subsystems::render::RenderFeatureRegistration;
use post_component::{CameraPostProcessComponent, PostProcessVolumeComponent};

/// Everything the post-process plugin registers for a game.
pub(crate) fn build(p: &mut pulsar_plugins::PluginBuilder) {
    // ── Component classes ───────────────────────────────────────────────────

    p.components()
        .class::<PostProcessVolumeComponent>(ClassSpec::default())
        .class::<CameraPostProcessComponent>(ClassSpec::default());

    // ── Rendering ───────────────────────────────────────────────────────────

    // The volumes' and camera baselines' source rows and the row join that
    // places a volume at its owner.
    p.add(SceneRowsRegistration {
        origin: p.origin(),
        name: "post.rows",
        register_columns: post_component::register_columns,
        derivation: Some(post_component::derivation),
    });

    // The blend that resolves the frame's post settings; it reads no project
    // settings through the renderer config.
    p.add(RenderFeatureRegistration {
        origin: p.origin(),
        features: post_component::render_features,
        configure: |config| config,
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn registers_its_feature() {
        let features =
            engine_backend::subsystems::render::helio_renderer::render_features::plugin_features();
        assert!(
            features.ids().contains(&"helio.post_volumes"),
            "features: {:?}",
            features.ids()
        );
    }

    /// Every registration of the plugin names this root, so it resolves to
    /// the plugin's id.
    #[test]
    fn every_registration_resolves_to_the_plugin() {
        let ours: Vec<_> = pulsar_plugins::contributions()
            .into_iter()
            .filter(|summary| summary.origin == pulsar_plugins::origin!())
            .collect();
        assert!(
            ours.iter()
                .any(|summary| summary.category == "component.class")
        );
        for summary in &ours {
            assert_eq!(
                pulsar_plugins::plugin_of(summary.origin).map(|plugin| plugin.id.as_str()),
                Some("pulsar.post"),
                "{summary:?}"
            );
        }
    }
}
