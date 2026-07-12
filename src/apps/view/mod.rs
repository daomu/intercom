//! View layer: embedded-graphics command-mode renderers for each app.
//!
//! View functions read model state (Launcher / SettingsApp / IntercomApp) +
//! a `RenderCtx` snapshot, and draw directly into a `Rgb565Buf` framebuffer.
//! There is no retained scene graph — touch hit-testing is procedural
//! (`hit_test(x, y) -> HitTarget`), derived from current layout state.

pub mod intercom_view;
pub mod launcher_view;
pub mod settings_view;
pub mod status_bar;
pub mod volume_panel;
