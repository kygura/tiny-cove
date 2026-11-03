//! The voxel grid: the single source of truth that meshing reads.
//! Pure data, no Bevy dependencies beyond math types.

use bevy::math::{IVec3, Vec3};

/// World size in voxels.
pub const SIZE_X: i32 = 160;
pub const SIZE_Y: i32 = 64;
pub const SIZE_Z: i32 = 160;
/// Edge length of one voxel in world units.
pub const VOXEL_SIZE: f32 = 0.5;
/// Edge length of a meshing chunk, in voxels.
pub const CHUNK: i32 = 16;
pub const CHUNKS_X: i32 = SIZE_X / CHUNK;
pub const CHUNKS_Y: i32 = SIZE_Y / CHUNK;
pub const CHUNKS_Z: i32 = SIZE_Z / CHUNK;
/// Height (in voxels) of the ground surface. Voxels with y < GROUND_Y are ground.
pub const GROUND_Y: i32 = 4;

/// Material of one voxel. `Air` is empty; everything else is solid.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Voxel {
    #[default]
    Air = 0,
    /// Bare dead earth / ash.
    Ash,
    /// Darker scorched dirt, crater floors.
    Scorch,
    /// Road surface.
    Asphalt,
    /// Rubble chunks.
    Rubble,
    /// Main board-formed concrete.
    Concrete,
    /// Darker, stained concrete (bands, bases, slabs).
    ConcreteDark,
    /// Unlit window.
    WindowDark,
    /// Lit window (sickly sodium yellow).
    WindowLit,
    /// Rusted steel: railings, antennas, tanks, machinery.
    Rust,
    /// Dead tree bark.
    DeadWood,
    /// Floodlight / warning lamp (emissive-ish).
    Lamp,
}

impl Voxel {
    pub fn is_solid(self) -> bool {
        self != Voxel::Air
    }
    /// Placed by structures (as opposed to terrain).
    pub fn is_structure(self) -> bool {
        matches!(
            self,
            Voxel::Concrete | Voxel::ConcreteDark | Voxel::WindowDark | Voxel::WindowLit | Voxel::Rust | Voxel::Lamp
        )
    }
    /// Tile index into the 4x4 texture atlas: tile i sits at column i % 4, row i / 4.
    pub fn atlas_tile(self) -> u32 {
        self as u32
    }
}

/// Dense voxel grid, flat layout `x + z*SIZE_X + y*SIZE_X*SIZE_Z`.
#[derive(Clone, PartialEq, Eq)]
pub struct VoxelGrid {
    cells: Vec<Voxel>,
}

impl Default for VoxelGrid {
    fn default() -> Self {
        Self::new()
    }
}

impl VoxelGrid {
    pub fn new() -> Self {
        Self { cells: vec![Voxel::Air; (SIZE_X * SIZE_Y * SIZE_Z) as usize] }
    }

    #[inline]
    pub fn in_bounds(p: IVec3) -> bool {
        p.x >= 0 && p.y >= 0 && p.z >= 0 && p.x < SIZE_X && p.y < SIZE_Y && p.z < SIZE_Z
    }

    #[inline]
    fn idx(p: IVec3) -> usize {
        (p.x + p.z * SIZE_X + p.y * SIZE_X * SIZE_Z) as usize
    }

    /// Out-of-bounds reads return `Air`.
    #[inline]
    pub fn get(&self, p: IVec3) -> Voxel {
        if Self::in_bounds(p) { self.cells[Self::idx(p)] } else { Voxel::Air }
    }

    /// Out-of-bounds writes are ignored.
    #[inline]
    pub fn set(&mut self, p: IVec3, v: Voxel) {
        if Self::in_bounds(p) {
            let i = Self::idx(p);
            self.cells[i] = v;
        }
    }

    /// Highest solid voxel y in column (x, z), or -1 if empty.
    pub fn top(&self, x: i32, z: i32) -> i32 {
        (0..SIZE_Y).rev().find(|&y| self.get(IVec3::new(x, y, z)).is_solid()).unwrap_or(-1)
    }

    /// Chunk coordinates whose contents differ between `self` and `other`.
    pub fn diff_chunks(&self, other: &VoxelGrid) -> Vec<IVec3> {
        let mut out = Vec::new();
        for cy in 0..CHUNKS_Y {
            for cz in 0..CHUNKS_Z {
                for cx in 0..CHUNKS_X {
                    let c = IVec3::new(cx, cy, cz);
                    if self.chunk_differs(other, c) {
                        out.push(c);
                    }
                }
            }
        }
        out
    }

    fn chunk_differs(&self, other: &VoxelGrid, c: IVec3) -> bool {
        let o = c * CHUNK;
        for y in o.y..o.y + CHUNK {
            for z in o.z..o.z + CHUNK {
                let a = Self::idx(IVec3::new(o.x, y, z));
                let b = a + CHUNK as usize;
                if self.cells[a..b] != other.cells[a..b] {
                    return true;
                }
            }
        }
        false
    }
}

/// World-space position of the minimum corner of voxel `p`.
/// The grid is centred on the origin in x/z; y = 0 is the bottom of the grid,
/// and the ground surface sits at world y = 0 (i.e. voxel GROUND_Y's bottom face).
pub fn voxel_to_world(p: IVec3) -> Vec3 {
    Vec3::new(
        (p.x - SIZE_X / 2) as f32 * VOXEL_SIZE,
        (p.y - GROUND_Y) as f32 * VOXEL_SIZE,
        (p.z - SIZE_Z / 2) as f32 * VOXEL_SIZE,
    )
}

/// Voxel containing world point `w`.
pub fn world_to_voxel(w: Vec3) -> IVec3 {
    IVec3::new(
        (w.x / VOXEL_SIZE).floor() as i32 + SIZE_X / 2,
        (w.y / VOXEL_SIZE).floor() as i32 + GROUND_Y,
        (w.z / VOXEL_SIZE).floor() as i32 + SIZE_Z / 2,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_world_voxel() {
        let p = IVec3::new(10, GROUND_Y, 77);
        assert_eq!(world_to_voxel(voxel_to_world(p) + Vec3::splat(0.25)), p);
    }

    #[test]
    fn diff_detects_single_change() {
        let a = VoxelGrid::new();
        let mut b = a.clone();
        b.set(IVec3::new(17, 33, 5), Voxel::Concrete);
        assert_eq!(a.diff_chunks(&b), vec![IVec3::new(1, 2, 0)]);
    }
}
