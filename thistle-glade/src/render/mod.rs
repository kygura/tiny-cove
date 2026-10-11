//! PS1 + cel-shaded renderer. STUB — owned by the render task.
//!
//! Contract (keep these public items):
//! - `RenderPlugin`
//! - `GladeAssets` resource (material handles) available after `PreStartup`
//! - `to_bevy_mesh(&MeshData) -> Mesh`
//! - `GameCamera` marker on the 3-D camera (others move it through its `Transform` /
//!   `Projection`), `cursor_ray(...)` helper for picking through the low-res target.

use bevy::prelude::*;

pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, _app: &mut App) {}
}
