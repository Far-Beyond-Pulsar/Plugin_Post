//! Post-Process Volumes (`pulsar.post`): the editor crate -- the
//! environment mixer's post-process volume entry ([`level_editor`]). It
//! registers nothing itself: the plugin's root crate (`post_plugin`, with
//! its `editor` feature) submits it through the level editor's extension
//! points. The inspector edits the components through their reflected
//! properties.

pub mod level_editor;
