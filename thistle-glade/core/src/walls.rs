//! Walls and fences.
//!
//! STUB — owned by the walls task. Keep the public signatures; replace the bodies.

use crate::mesh::{MeshData, Marker};
use crate::model::{Fence, Wall};
use crate::terrain::Terrain;

/// A passage the scene asks a wall/fence/house to leave open (where a path crosses it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cut {
    /// Distance along the host polyline / perimeter, at the passage centre.
    pub along: f32,
    /// Clear width of the passage in metres.
    pub width: f32,
    /// Clear height (arch apex) in metres above the ground.
    pub height: f32,
}

/// Wall mesh plus markers (perches on top, lanterns by gates, windows…).
pub fn wall_mesh(wall: &Wall, cuts: &[Cut], terrain: &Terrain, seed: u64) -> (MeshData, Vec<Marker>) {
    let _ = (wall, cuts, terrain, seed);
    (MeshData::default(), Vec::new())
}

/// Fence mesh. Gaps where `cuts` are.
pub fn fence_mesh(fence: &Fence, cuts: &[Cut], terrain: &Terrain, seed: u64) -> (MeshData, Vec<Marker>) {
    let _ = (fence, cuts, terrain, seed);
    (MeshData::default(), Vec::new())
}
