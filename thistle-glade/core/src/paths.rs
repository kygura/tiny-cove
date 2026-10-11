//! Paths, stairs, platforms and bridges.
//!
//! STUB — owned by the paths task. Keep the public signatures; replace the bodies.

use crate::mesh::{MeshData, Marker};
use crate::model::{Bridge, Path, Platform, Stairs};
use crate::terrain::Terrain;

/// Path ribbon draped on the terrain (slightly above it), with individual stones / setts /
/// flagstones as geometry, stepping stones (a ford) where it crosses water.
pub fn path_mesh(path: &Path, terrain: &Terrain, seed: u64) -> MeshData {
    let _ = (path, terrain, seed);
    MeshData::default()
}

pub fn stairs_mesh(stairs: &Stairs, terrain: &Terrain, seed: u64) -> (MeshData, Vec<Marker>) {
    let _ = (stairs, terrain, seed);
    (MeshData::default(), Vec::new())
}

pub fn platform_mesh(platform: &Platform, terrain: &Terrain, seed: u64) -> (MeshData, Vec<Marker>) {
    let _ = (platform, terrain, seed);
    (MeshData::default(), Vec::new())
}

pub fn bridge_mesh(bridge: &Bridge, terrain: &Terrain, seed: u64) -> (MeshData, Vec<Marker>) {
    let _ = (bridge, terrain, seed);
    (MeshData::default(), Vec::new())
}
