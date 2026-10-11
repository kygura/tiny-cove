//! Structures + seed -> voxel grid. Pure and deterministic.

use crate::structure::{Structure, World};
use crate::voxel::VoxelGrid;
use bevy::math::Vec2;

/// Builds the full voxel grid for a world: terrain first, then structures in order.
pub fn generate(world: &World) -> VoxelGrid {
    let _ = world;
    VoxelGrid::new()
}

/// True if the ground-plane point `p` (world x/z) is within `radius` of the
/// structure's footprint. Used by the demolish tool.
pub fn hits(structure: &Structure, p: Vec2, radius: f32) -> bool {
    let _ = (structure, p, radius);
    false
}
