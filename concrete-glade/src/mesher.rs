//! Voxel chunk -> triangle mesh data. Pure; conversion to a Bevy `Mesh` is separate.

use crate::voxel::VoxelGrid;
use bevy::math::IVec3;

/// Mesh buffers for one chunk, in world space.
/// `colors`: rgb = baked shade (face direction * ambient occlusion), a = emissive 0..1.
/// `uvs`: atlas-space UVs (see `Voxel::atlas_tile`).
#[derive(Default, Debug, Clone)]
pub struct ChunkMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub colors: Vec<[f32; 4]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

pub fn mesh_chunk(grid: &VoxelGrid, chunk: IVec3) -> ChunkMesh {
    let _ = (grid, chunk);
    ChunkMesh::default()
}
