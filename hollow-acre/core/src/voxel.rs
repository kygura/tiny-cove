//! The voxel grid: single source of truth that meshing reads. Pure data.

pub const SIZE_X: usize = 192;
pub const SIZE_Y: usize = 64;
pub const SIZE_Z: usize = 192;
pub const NX: i32 = SIZE_X as i32;
pub const NY: i32 = SIZE_Y as i32;
pub const NZ: i32 = SIZE_Z as i32;
/// Edge length of one voxel in world units.
pub const VOXEL_SIZE: f32 = 0.5;
/// World x/z of the grid's minimum corner.
pub const ORIGIN: f32 = -48.0;
/// Chunk edge in voxels.
pub const CHUNK: usize = 16;
pub const CHUNKS_X: usize = SIZE_X / CHUNK;
pub const CHUNKS_Y: usize = SIZE_Y / CHUNK;
pub const CHUNKS_Z: usize = SIZE_Z / CHUNK;

/// Material of one voxel. Render maps each kind to a palette colour.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Voxel {
    #[default]
    Air = 0,
    Soil,
    Grass,
    /// Tall-grass tint tuft.
    Meadow,
    Rock,
    Sand,
    Water,
    Asphalt,
    Gravel,
    Concrete,
    ConcreteDark,
    Glass,
    /// Emissive.
    WindowLit,
    Rust,
    Steel,
    Ivy,
    Wood,
    Leaves,
    Blossom,
    /// Emissive luminous wheat.
    Crop,
    /// Emissive.
    Crystal,
    Monolith,
    /// Emissive.
    Rune,
    /// Emissive.
    Lamp,
    Solar,
}

impl Voxel {
    pub const ALL: [Voxel; 25] = [
        Voxel::Air, Voxel::Soil, Voxel::Grass, Voxel::Meadow, Voxel::Rock, Voxel::Sand, Voxel::Water,
        Voxel::Asphalt, Voxel::Gravel, Voxel::Concrete, Voxel::ConcreteDark, Voxel::Glass,
        Voxel::WindowLit, Voxel::Rust, Voxel::Steel, Voxel::Ivy, Voxel::Wood, Voxel::Leaves,
        Voxel::Blossom, Voxel::Crop, Voxel::Crystal, Voxel::Monolith, Voxel::Rune, Voxel::Lamp,
        Voxel::Solar,
    ];

    /// Inverse of `as u8`; unknown values map to `Air`.
    pub fn from_u8(v: u8) -> Voxel {
        Self::ALL.get(v as usize).copied().unwrap_or(Voxel::Air)
    }

    /// Solid for culling and AO: everything except `Air` and `Water`.
    #[inline]
    pub fn is_solid(self) -> bool {
        !matches!(self, Voxel::Air | Voxel::Water)
    }

    pub fn is_emissive(self) -> bool {
        matches!(self, Voxel::WindowLit | Voxel::Crop | Voxel::Crystal | Voxel::Rune | Voxel::Lamp)
    }

    /// Soft plant matter that is cleared when a structure is built over it.
    pub fn is_vegetation(self) -> bool {
        matches!(self, Voxel::Meadow | Voxel::Wood | Voxel::Leaves | Voxel::Blossom)
    }

    /// Placed by structures (as opposed to terrain, plants and water).
    pub fn is_structure(self) -> bool {
        matches!(
            self,
            Voxel::Concrete | Voxel::ConcreteDark | Voxel::Glass | Voxel::WindowLit | Voxel::Rust
                | Voxel::Steel | Voxel::Ivy | Voxel::Monolith | Voxel::Rune | Voxel::Lamp | Voxel::Solar
        )
    }
}

/// Dense voxel grid, flat layout `x + z*SIZE_X + y*SIZE_X*SIZE_Z`.
#[derive(Clone, PartialEq, Eq)]
pub struct VoxelGrid {
    cells: Vec<Voxel>,
}

impl std::fmt::Debug for VoxelGrid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "VoxelGrid({}x{}x{})", SIZE_X, SIZE_Y, SIZE_Z)
    }
}

impl Default for VoxelGrid {
    fn default() -> Self {
        Self::new()
    }
}

impl VoxelGrid {
    pub fn new() -> Self {
        Self { cells: vec![Voxel::Air; SIZE_X * SIZE_Y * SIZE_Z] }
    }

    #[inline]
    pub fn in_bounds(x: i32, y: i32, z: i32) -> bool {
        x >= 0 && y >= 0 && z >= 0 && x < NX && y < NY && z < NZ
    }

    #[inline]
    fn idx(x: i32, y: i32, z: i32) -> usize {
        x as usize + z as usize * SIZE_X + y as usize * SIZE_X * SIZE_Z
    }

    /// Out-of-bounds reads return `Air`.
    #[inline]
    pub fn get(&self, x: i32, y: i32, z: i32) -> Voxel {
        if Self::in_bounds(x, y, z) { self.cells[Self::idx(x, y, z)] } else { Voxel::Air }
    }

    /// Out-of-bounds writes are ignored.
    #[inline]
    pub fn set(&mut self, x: i32, y: i32, z: i32, v: Voxel) {
        if Self::in_bounds(x, y, z) {
            let i = Self::idx(x, y, z);
            self.cells[i] = v;
        }
    }

    /// Highest non-air voxel y in column (x, z), or -1 if empty.
    pub fn top(&self, x: i32, z: i32) -> i32 {
        (0..NY).rev().find(|&y| self.get(x, y, z) != Voxel::Air).unwrap_or(-1)
    }

    /// Chunks whose contents differ from `other`, plus every neighbouring chunk
    /// (including diagonals) of a changed voxel on a shared border, because
    /// meshing reads one voxel past the chunk edge. Sorted by (y, z, x).
    pub fn diff_chunks(&self, other: &VoxelGrid) -> Vec<[usize; 3]> {
        let mut flag = vec![false; CHUNKS_X * CHUNKS_Y * CHUNKS_Z];
        let c = CHUNK as i32;
        for y in 0..SIZE_Y {
            for z in 0..SIZE_Z {
                let row = Self::idx(0, y as i32, z as i32);
                for cx in 0..CHUNKS_X {
                    let (a, b) = (row + cx * CHUNK, row + (cx + 1) * CHUNK);
                    if self.cells[a..b] == other.cells[a..b] {
                        continue;
                    }
                    for k in 0..CHUNK {
                        if self.cells[a + k] == other.cells[a + k] {
                            continue;
                        }
                        let (lx, ly, lz) = (k as i32, y as i32 % c, z as i32 % c);
                        let span = |l: i32| match l {
                            0 => -1..=0,
                            15 => 0..=1,
                            _ => 0..=0,
                        };
                        for dy in span(ly) {
                            for dz in span(lz) {
                                for dx in span(lx) {
                                    let (nx, ny, nz) = (cx as i32 + dx, y as i32 / c + dy, z as i32 / c + dz);
                                    if (0..CHUNKS_X as i32).contains(&nx)
                                        && (0..CHUNKS_Y as i32).contains(&ny)
                                        && (0..CHUNKS_Z as i32).contains(&nz)
                                    {
                                        flag[nx as usize + nz as usize * CHUNKS_X + ny as usize * CHUNKS_X * CHUNKS_Z] = true;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        let mut out = Vec::new();
        for cy in 0..CHUNKS_Y {
            for cz in 0..CHUNKS_Z {
                for cx in 0..CHUNKS_X {
                    if flag[cx + cz * CHUNKS_X + cy * CHUNKS_X * CHUNKS_Z] {
                        out.push([cx, cy, cz]);
                    }
                }
            }
        }
        out
    }
}

/// World position of the minimum corner of voxel `p`.
pub fn voxel_to_world(p: [i32; 3]) -> [f32; 3] {
    [
        ORIGIN + p[0] as f32 * VOXEL_SIZE,
        p[1] as f32 * VOXEL_SIZE,
        ORIGIN + p[2] as f32 * VOXEL_SIZE,
    ]
}

/// World position of the centre of voxel `p`.
pub fn voxel_center(p: [i32; 3]) -> [f32; 3] {
    let w = voxel_to_world(p);
    [w[0] + VOXEL_SIZE * 0.5, w[1] + VOXEL_SIZE * 0.5, w[2] + VOXEL_SIZE * 0.5]
}

/// Voxel containing world point `w` (may be out of bounds).
pub fn world_to_voxel(w: [f32; 3]) -> [i32; 3] {
    [
        ((w[0] - ORIGIN) / VOXEL_SIZE).floor() as i32,
        (w[1] / VOXEL_SIZE).floor() as i32,
        ((w[2] - ORIGIN) / VOXEL_SIZE).floor() as i32,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_world_voxel() {
        let p = [10, 5, 77];
        let w = voxel_to_world(p);
        assert_eq!(world_to_voxel([w[0] + 0.25, w[1] + 0.25, w[2] + 0.25]), p);
        assert_eq!(voxel_to_world([0, 0, 0]), [-48.0, 0.0, -48.0]);
        assert_eq!(voxel_to_world([192, 0, 192]), [48.0, 0.0, 48.0]);
    }

    #[test]
    fn from_u8_roundtrip() {
        for v in Voxel::ALL {
            assert_eq!(Voxel::from_u8(v as u8), v);
        }
        assert_eq!(Voxel::from_u8(250), Voxel::Air);
    }

    #[test]
    fn oob_is_safe() {
        let mut g = VoxelGrid::new();
        g.set(-1, 0, 0, Voxel::Rock);
        g.set(0, 64, 0, Voxel::Rock);
        assert_eq!(g.get(-5, 3, 900), Voxel::Air);
    }

    #[test]
    fn diff_detects_single_change_and_neighbours() {
        let a = VoxelGrid::new();
        let mut b = a.clone();
        b.set(17, 33, 5, Voxel::Concrete);
        assert_eq!(a.diff_chunks(&b), vec![[1, 2, 0]]);
        assert!(a.diff_chunks(&a).is_empty());
        // Border voxel pulls in face neighbours; corner voxel pulls in diagonals.
        let mut c = a.clone();
        c.set(16, 16, 16, Voxel::Concrete);
        let d = a.diff_chunks(&c);
        assert!(d.contains(&[1, 1, 1]) && d.contains(&[0, 0, 0]) && d.contains(&[0, 1, 1]));
        assert_eq!(d.len(), 8);
        let mut e = a.clone();
        e.set(15, 5, 5, Voxel::Concrete);
        assert_eq!(a.diff_chunks(&e), vec![[0, 0, 0], [1, 0, 0]]);
    }
}
