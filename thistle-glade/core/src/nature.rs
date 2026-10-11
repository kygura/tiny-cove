//! Trees, clutter props and painted vegetation scatter.
//!
//! STUB — owned by the nature task. Keep the public signatures; replace the bodies.

use crate::mesh::{MeshData, Marker};
use crate::model::Placed;
use crate::terrain::Terrain;
use glam::Vec2;

/// One placed tree or clutter object, standing on the terrain.
pub fn placed_mesh(placed: &Placed, terrain: &Terrain, seed: u64) -> (MeshData, Vec<Marker>) {
    let _ = (placed, terrain, seed);
    (MeshData::default(), Vec::new())
}

/// Vegetation scattered over the glade from the terrain's paint layers and a little everywhere
/// on grass (tufts, a few wild flowers): flowers, bushes, grain, grass tufts.
/// Nothing is scattered inside `blockers` (structure footprints / path ribbons) or on water.
pub fn scatter_mesh(terrain: &Terrain, seed: u64, blockers: &[Vec<Vec2>]) -> MeshData {
    let _ = (terrain, seed, blockers);
    MeshData::default()
}
