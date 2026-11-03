//! Voxel chunk -> triangle mesh data. Pure; the game converts it to a render mesh.

use crate::voxel::{CHUNK, CHUNKS_X, CHUNKS_Y, CHUNKS_Z, VOXEL_SIZE, Voxel, VoxelGrid, voxel_to_world};

/// Mesh buffers for one chunk, in world units. Flat face normals.
#[derive(Default, Debug, Clone)]
pub struct ChunkMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// Voxel kind (`Voxel as u8`) per vertex.
    pub voxels: Vec<u8>,
    /// Ambient occlusion per vertex, 0 (dark) ..= 1 (open).
    pub ao: Vec<f32>,
    pub indices: Vec<u32>,
}

impl ChunkMesh {
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }
}

const AO_LEVELS: [f32; 4] = [0.5, 0.7, 0.85, 1.0];
/// Water top surface sits this fraction of a voxel below the voxel's top.
const WATER_DROP: f32 = 0.125;

type V3 = [i32; 3];

/// One cube face. Corners are CCW seen from outside; `t1`/`t2` are in-plane axes.
struct Face {
    n: V3,
    corners: [V3; 4],
    t1: V3,
    t2: V3,
}

const FACES: [Face; 6] = [
    Face { n: [0, 1, 0], corners: [[0, 1, 0], [0, 1, 1], [1, 1, 1], [1, 1, 0]], t1: [0, 0, 1], t2: [1, 0, 0] },
    Face { n: [0, -1, 0], corners: [[0, 0, 0], [1, 0, 0], [1, 0, 1], [0, 0, 1]], t1: [1, 0, 0], t2: [0, 0, 1] },
    Face { n: [1, 0, 0], corners: [[1, 0, 0], [1, 1, 0], [1, 1, 1], [1, 0, 1]], t1: [0, 1, 0], t2: [0, 0, 1] },
    Face { n: [-1, 0, 0], corners: [[0, 0, 0], [0, 0, 1], [0, 1, 1], [0, 1, 0]], t1: [0, 0, 1], t2: [0, 1, 0] },
    Face { n: [0, 0, 1], corners: [[0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]], t1: [1, 0, 0], t2: [0, 1, 0] },
    Face { n: [0, 0, -1], corners: [[0, 0, 0], [0, 1, 0], [1, 1, 0], [1, 0, 0]], t1: [0, 1, 0], t2: [1, 0, 0] },
];

#[inline]
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
#[inline]
fn scale(a: V3, k: i32) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
#[inline]
fn dot(a: V3, b: V3) -> i32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Classic 0fps vertex AO: 0 (darkest) ..= 3 (open).
#[inline]
fn vertex_ao(side1: bool, side2: bool, corner: bool) -> usize {
    if side1 && side2 { 0 } else { 3 - (side1 as usize + side2 as usize + corner as usize) }
}

/// Meshes one 16^3 chunk. Out-of-range chunk coordinates give an empty mesh.
///
/// Solid voxels emit faces against `Air` and `Water`; `Water` emits only its
/// (lowered) top face against `Air`.
pub fn mesh_chunk(grid: &VoxelGrid, chunk: [usize; 3]) -> ChunkMesh {
    let mut m = ChunkMesh::default();
    if chunk[0] >= CHUNKS_X || chunk[1] >= CHUNKS_Y || chunk[2] >= CHUNKS_Z {
        return m;
    }
    let o = [(chunk[0] * CHUNK) as i32, (chunk[1] * CHUNK) as i32, (chunk[2] * CHUNK) as i32];
    let solid = |p: V3| grid.get(p[0], p[1], p[2]).is_solid();
    for y in 0..CHUNK as i32 {
        for z in 0..CHUNK as i32 {
            for x in 0..CHUNK as i32 {
                let p = [o[0] + x, o[1] + y, o[2] + z];
                let voxel = grid.get(p[0], p[1], p[2]);
                if voxel == Voxel::Air {
                    continue;
                }
                let base = voxel_to_world(p);
                if voxel == Voxel::Water {
                    if grid.get(p[0], p[1] + 1, p[2]) == Voxel::Air {
                        let f = &FACES[0];
                        push_face(&mut m, f, base, voxel, [3; 4], WATER_DROP);
                    }
                    continue;
                }
                for f in &FACES {
                    let q = add(p, f.n);
                    if solid(q) || (f.n[1] < 0 && p[1] == 0) {
                        continue;
                    }
                    let mut ao = [0usize; 4];
                    for (i, &cn) in f.corners.iter().enumerate() {
                        let d1 = scale(f.t1, dot(cn, f.t1) * 2 - 1);
                        let d2 = scale(f.t2, dot(cn, f.t2) * 2 - 1);
                        ao[i] = vertex_ao(solid(add(q, d1)), solid(add(q, d2)), solid(add(add(q, d1), d2)));
                    }
                    push_face(&mut m, f, base, voxel, ao, 0.0);
                }
            }
        }
    }
    m
}

fn push_face(m: &mut ChunkMesh, f: &Face, base: [f32; 3], voxel: Voxel, ao: [usize; 4], drop: f32) {
    let first = m.positions.len() as u32;
    for (i, &cn) in f.corners.iter().enumerate() {
        let dy = if cn[1] == 1 { 1.0 - drop } else { cn[1] as f32 };
        m.positions.push([
            base[0] + cn[0] as f32 * VOXEL_SIZE,
            base[1] + dy * VOXEL_SIZE,
            base[2] + cn[2] as f32 * VOXEL_SIZE,
        ]);
        m.normals.push([f.n[0] as f32, f.n[1] as f32, f.n[2] as f32]);
        m.voxels.push(voxel as u8);
        m.ao.push(AO_LEVELS[ao[i]]);
    }
    // Flip the diagonal to avoid AO anisotropy artefacts.
    let quad: [u32; 6] = if ao[0] + ao[2] < ao[1] + ao[3] { [1, 2, 3, 1, 3, 0] } else { [0, 1, 2, 0, 2, 3] };
    m.indices.extend(quad.iter().map(|i| first + i));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid_with(voxels: &[([i32; 3], Voxel)]) -> VoxelGrid {
        let mut g = VoxelGrid::new();
        for &(p, v) in voxels {
            g.set(p[0], p[1], p[2], v);
        }
        g
    }

    #[test]
    fn empty_chunk_is_empty() {
        assert!(mesh_chunk(&VoxelGrid::new(), [0, 0, 0]).is_empty());
        assert!(mesh_chunk(&VoxelGrid::new(), [99, 0, 0]).is_empty());
    }

    #[test]
    fn single_voxel_six_faces() {
        let m = mesh_chunk(&grid_with(&[([5, 5, 5], Voxel::Concrete)]), [0, 0, 0]);
        assert_eq!((m.positions.len(), m.indices.len()), (24, 36));
        assert!(m.normals.len() == 24 && m.voxels.len() == 24 && m.ao.len() == 24);
        assert!(m.voxels.iter().all(|&v| v == Voxel::Concrete as u8));
    }

    #[test]
    fn adjacent_voxels_ten_faces() {
        let g = grid_with(&[([5, 5, 5], Voxel::Concrete), ([6, 5, 5], Voxel::Rust)]);
        assert_eq!(mesh_chunk(&g, [0, 0, 0]).indices.len(), 10 * 6);
    }

    #[test]
    fn border_culling_across_chunks() {
        let g = grid_with(&[([15, 5, 5], Voxel::Concrete), ([16, 5, 5], Voxel::Concrete)]);
        assert_eq!(mesh_chunk(&g, [0, 0, 0]).indices.len(), 5 * 6);
        assert_eq!(mesh_chunk(&g, [1, 0, 0]).indices.len(), 5 * 6);
    }

    #[test]
    fn bottom_layer_skips_underside() {
        let m = mesh_chunk(&grid_with(&[([5, 0, 5], Voxel::Rock)]), [0, 0, 0]);
        assert_eq!(m.positions.len(), 20);
    }

    #[test]
    fn water_top_only_and_lowered() {
        let g = grid_with(&[([5, 5, 5], Voxel::Water), ([6, 5, 5], Voxel::Water), ([5, 4, 5], Voxel::Sand)]);
        let m = mesh_chunk(&g, [0, 0, 0]);
        let water: Vec<usize> = (0..m.voxels.len()).filter(|&i| m.voxels[i] == Voxel::Water as u8).collect();
        assert_eq!(water.len(), 8); // two top quads
        assert!(water.iter().all(|&i| m.normals[i] == [0.0, 1.0, 0.0]));
        let top = (5.0 + 1.0) * VOXEL_SIZE;
        assert!(water.iter().all(|&i| (m.positions[i][1] - (top - 0.125 * VOXEL_SIZE)).abs() < 1e-6));
        // Sand under water still emits all faces except nothing hidden by water (water is non-solid).
        assert_eq!(m.voxels.iter().filter(|&&v| v == Voxel::Sand as u8).count(), 24);
    }

    #[test]
    fn water_under_solid_emits_nothing() {
        let g = grid_with(&[([5, 5, 5], Voxel::Water), ([5, 6, 5], Voxel::Concrete)]);
        let m = mesh_chunk(&g, [0, 0, 0]);
        assert!(!m.voxels.contains(&(Voxel::Water as u8)));
    }

    #[test]
    fn winding_is_ccw_outward() {
        let g = grid_with(&[
            ([5, 5, 5], Voxel::Concrete),
            ([6, 5, 5], Voxel::Concrete),
            ([5, 6, 6], Voxel::Concrete),
            ([8, 5, 8], Voxel::Concrete),
            ([8, 6, 8], Voxel::Concrete),
            ([10, 5, 10], Voxel::Water),
        ]);
        let m = mesh_chunk(&g, [0, 0, 0]);
        for t in m.indices.chunks(3) {
            let v = |i: u32| m.positions[i as usize];
            let (a, b, c) = (v(t[0]), v(t[1]), v(t[2]));
            let (u, w) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
            let cr = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
            let n = m.normals[t[0] as usize];
            assert!(cr[0] * n[0] + cr[1] * n[1] + cr[2] * n[2] > 0.0, "triangle {t:?} not CCW outward");
        }
    }

    #[test]
    fn ao_darkens_next_to_wall_and_ignores_water() {
        let open = grid_with(&[([5, 3, 5], Voxel::Soil)]);
        let walled = grid_with(&[([5, 3, 5], Voxel::Soil), ([6, 4, 5], Voxel::Concrete)]);
        let wet = grid_with(&[([5, 3, 5], Voxel::Soil), ([6, 4, 5], Voxel::Water)]);
        let top_ao = |g: &VoxelGrid| -> Vec<f32> {
            let m = mesh_chunk(g, [0, 0, 0]);
            (0..m.positions.len())
                .filter(|&i| m.normals[i] == [0.0, 1.0, 0.0] && m.voxels[i] == Voxel::Soil as u8)
                .map(|i| m.ao[i])
                .collect()
        };
        assert!(top_ao(&open).iter().all(|&a| a == 1.0));
        assert_eq!(top_ao(&walled).iter().filter(|&&a| a < 1.0).count(), 2);
        assert!(top_ao(&wet).iter().all(|&a| a == 1.0));
    }
}
