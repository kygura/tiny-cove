//! Pure world model for Hollow Acre: voxels, structures, generation, meshing.
//! No engine dependencies.

mod buildings;
pub mod generate;
pub mod mesher;
pub mod noise;
pub mod structure;
pub mod terrain;
pub mod voxel;

pub use generate::{generate, ground_height, terrain_for};
pub use mesher::{ChunkMesh, mesh_chunk};
pub use structure::{Document, History, Structure};
pub use terrain::{Terrain, WATER_LEVEL};
pub use voxel::{
    CHUNK, CHUNKS_X, CHUNKS_Y, CHUNKS_Z, ORIGIN, SIZE_X, SIZE_Y, SIZE_Z, VOXEL_SIZE, Voxel, VoxelGrid,
    voxel_center, voxel_to_world, world_to_voxel,
};
