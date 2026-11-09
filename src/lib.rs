//! Post-Process Volumes (`pulsar.post`): the plugin's root crate, the one
//! place the plugin registers everything it contributes.
//!
//! The plugin's other crates hold the code: the feature crate
//! (`helio-pass-post-volumes`: the blend that publishes the frame's post
//! settings over the host's baseline), `post_component` (the post-process
//! volume and camera baseline components, their rows and their row join)
//! and `post_editor` (the environment mixer's entry). None of them registers
//! a contribution; this crate imports them and submits their registrations,
//! so this file and the two modules below are the whole list of what the
//! plugin brings:
//!
//! - [`runtime`]: what a game links. The component classes, their scene
//!   rows and the Helio feature.
//! - `editor` (with the `editor` feature): what the editor adds. The
//!   environment mixer's post-process volume.
//!
//! Every registration names this crate as its origin
//! (`pulsar_plugins::origin!()`), so the plugin gate skips all of them while
//! a project disables the plugin, and the plugin catalog lists them under
//! `pulsar.post`. Other plugins never depend on this crate; hosts reach it
//! through the installed set or a generated game, a game without the
//! `editor` feature.

// The plugin's descriptor (`pulsar-plugin.toml`): linking this crate
// installs the plugin; a project enables it or not (`pulsar_plugins`).
pulsar_plugins::installed!("pulsar-plugin.toml");

pub use post_component;
#[cfg(feature = "editor")]
pub use post_editor;

pub mod runtime;

#[cfg(feature = "editor")]
pub mod editor;
