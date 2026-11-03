//! Voxel chunk -> triangle mesh data. Pure; conversion to a Bevy `Mesh` is separate.

use crate::voxel::{CHUNK, VOXEL_SIZE, Voxel, VoxelGrid, voxel_to_world};
use bevy::math::{IVec3, Vec3};

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

const ATLAS_TILES: f32 = 4.0;
/// Half a texel of a 16px tile, in tile units.
const INSET: f32 = 0.5 / 16.0;
const AO_LEVELS: [f32; 4] = [0.45, 0.65, 0.82, 1.0];

/// One cube face. Corners are CCW seen from outside; `t1`/`t2` are the in-plane axes.
struct Face {
    n: IVec3,
    corners: [IVec3; 4],
    t1: IVec3,
    t2: IVec3,
    shade: f32,
    /// (axis, flipped) that drive atlas u and v.
    u: (usize, bool),
    v: (usize, bool),
}

const fn c(x: i32, y: i32, z: i32) -> IVec3 {
    IVec3::new(x, y, z)
}

const FACES: [Face; 6] = [
    Face { n: c(0, 1, 0), corners: [c(0, 1, 0), c(0, 1, 1), c(1, 1, 1), c(1, 1, 0)], t1: c(0, 0, 1), t2: c(1, 0, 0), shade: 1.0, u: (0, false), v: (2, false) },
    Face { n: c(0, -1, 0), corners: [c(0, 0, 0), c(1, 0, 0), c(1, 0, 1), c(0, 0, 1)], t1: c(1, 0, 0), t2: c(0, 0, 1), shade: 0.5, u: (0, false), v: (2, false) },
    Face { n: c(1, 0, 0), corners: [c(1, 0, 0), c(1, 1, 0), c(1, 1, 1), c(1, 0, 1)], t1: c(0, 1, 0), t2: c(0, 0, 1), shade: 0.8, u: (2, true), v: (1, true) },
    Face { n: c(-1, 0, 0), corners: [c(0, 0, 0), c(0, 0, 1), c(0, 1, 1), c(0, 1, 0)], t1: c(0, 0, 1), t2: c(0, 1, 0), shade: 0.8, u: (2, false), v: (1, true) },
    Face { n: c(0, 0, 1), corners: [c(0, 0, 1), c(1, 0, 1), c(1, 1, 1), c(0, 1, 1)], t1: c(1, 0, 0), t2: c(0, 1, 0), shade: 0.68, u: (0, false), v: (1, true) },
    Face { n: c(0, 0, -1), corners: [c(0, 0, 0), c(0, 1, 0), c(1, 1, 0), c(1, 0, 0)], t1: c(0, 1, 0), t2: c(1, 0, 0), shade: 0.68, u: (0, true), v: (1, true) },
];

/// Classic 0fps vertex AO: 0 (darkest) ..= 3 (open).
#[inline]
fn vertex_ao(side1: bool, side2: bool, corner: bool) -> usize {
    if side1 && side2 { 0 } else { 3 - (side1 as usize + side2 as usize + corner as usize) }
}

pub fn mesh_chunk(grid: &VoxelGrid, chunk: IVec3) -> ChunkMesh {
    let mut m = ChunkMesh {
        positions: Vec::with_capacity(2048),
        normals: Vec::with_capacity(2048),
        colors: Vec::with_capacity(2048),
        uvs: Vec::with_capacity(2048),
        indices: Vec::with_capacity(3072),
    };
    let origin = chunk * CHUNK;
    for y in 0..CHUNK {
        for z in 0..CHUNK {
            for x in 0..CHUNK {
                let p = origin + IVec3::new(x, y, z);
                let voxel = grid.get(p);
                if !voxel.is_solid() {
                    continue;
                }
                let base = voxel_to_world(p);
                let tile = voxel.atlas_tile();
                let (tc, tr) = ((tile % 4) as f32, (tile / 4) as f32);
                let emissive = matches!(voxel, Voxel::WindowLit | Voxel::Lamp) as u8 as f32;
                for f in &FACES {
                    if grid.get(p + f.n).is_solid() || (f.n.y < 0 && p.y == 0) {
                        continue;
                    }
                    let first = m.positions.len() as u32;
                    let layer = p + f.n;
                    let mut ao = [0usize; 4];
                    for (i, &cn) in f.corners.iter().enumerate() {
                        let d1 = f.t1 * (cn.dot(f.t1) * 2 - 1);
                        let d2 = f.t2 * (cn.dot(f.t2) * 2 - 1);
                        ao[i] = vertex_ao(
                            grid.get(layer + d1).is_solid(),
                            grid.get(layer + d2).is_solid(),
                            grid.get(layer + d1 + d2).is_solid(),
                        );
                        let pos = base + Vec3::new(cn.x as f32, cn.y as f32, cn.z as f32) * VOXEL_SIZE;
                        let uv_axis = |(axis, flip): (usize, bool)| {
                            let t = cn[axis] as f32;
                            let t = if flip { 1.0 - t } else { t };
                            INSET + (1.0 - 2.0 * INSET) * t
                        };
                        let shade = f.shade * AO_LEVELS[ao[i]];
                        m.positions.push(pos.to_array());
                        m.normals.push([f.n.x as f32, f.n.y as f32, f.n.z as f32]);
                        m.colors.push([shade, shade, shade, emissive]);
                        m.uvs.push([(tc + uv_axis(f.u)) / ATLAS_TILES, (tr + uv_axis(f.v)) / ATLAS_TILES]);
                    }
                    let quad: [u32; 6] = if ao[0] + ao[2] < ao[1] + ao[3] { [1, 2, 3, 1, 3, 0] } else { [0, 1, 2, 0, 2, 3] };
                    m.indices.extend(quad.iter().map(|i| first + i));
                }
            }
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid_with(voxels: &[(IVec3, Voxel)]) -> VoxelGrid {
        let mut g = VoxelGrid::new();
        for &(p, v) in voxels {
            g.set(p, v);
        }
        g
    }

    #[test]
    fn empty_chunk_is_empty() {
        let m = mesh_chunk(&VoxelGrid::new(), IVec3::ZERO);
        assert!(m.positions.is_empty() && m.indices.is_empty());
    }

    #[test]
    fn single_voxel_six_faces() {
        let g = grid_with(&[(IVec3::new(5, 5, 5), Voxel::Concrete)]);
        let m = mesh_chunk(&g, IVec3::ZERO);
        assert_eq!((m.positions.len(), m.indices.len()), (24, 36));
        assert_eq!(m.normals.len(), 24);
        assert_eq!(m.colors.len(), 24);
        assert_eq!(m.uvs.len(), 24);
    }

    #[test]
    fn bottom_layer_skips_underside() {
        let g = grid_with(&[(IVec3::new(5, 0, 5), Voxel::Concrete)]);
        let m = mesh_chunk(&g, IVec3::ZERO);
        assert_eq!((m.positions.len(), m.indices.len()), (20, 30));
        assert!(m.normals.iter().all(|n| n[1] >= 0.0));
    }

    #[test]
    fn adjacent_voxels_ten_faces() {
        let g = grid_with(&[(IVec3::new(5, 5, 5), Voxel::Concrete), (IVec3::new(6, 5, 5), Voxel::Rust)]);
        assert_eq!(mesh_chunk(&g, IVec3::ZERO).indices.len(), 10 * 6);
    }

    #[test]
    fn shared_face_across_chunk_border_culled() {
        let g = grid_with(&[(IVec3::new(15, 5, 5), Voxel::Concrete), (IVec3::new(16, 5, 5), Voxel::Concrete)]);
        assert_eq!(mesh_chunk(&g, IVec3::new(0, 0, 0)).indices.len(), 5 * 6);
        assert_eq!(mesh_chunk(&g, IVec3::new(1, 0, 0)).indices.len(), 5 * 6);
    }

    #[test]
    fn winding_is_ccw_outward() {
        let g = grid_with(&[
            (IVec3::new(5, 5, 5), Voxel::Concrete),
            (IVec3::new(6, 5, 5), Voxel::Concrete),
            (IVec3::new(5, 6, 6), Voxel::Concrete),
            (IVec3::new(8, 5, 8), Voxel::Concrete),
            (IVec3::new(8, 6, 8), Voxel::Concrete),
        ]);
        let m = mesh_chunk(&g, IVec3::ZERO);
        assert!(!m.indices.is_empty());
        for t in m.indices.chunks(3) {
            let v = |i: u32| Vec3::from_array(m.positions[i as usize]);
            let n = Vec3::from_array(m.normals[t[0] as usize]);
            let cr = (v(t[1]) - v(t[0])).cross(v(t[2]) - v(t[0]));
            assert!(cr.dot(n) > 0.0, "triangle {t:?} not CCW outward");
        }
    }

    #[test]
    fn ao_darkens_next_to_wall() {
        let ground = IVec3::new(5, 3, 5);
        let open = grid_with(&[(ground, Voxel::Ash)]);
        let walled = grid_with(&[(ground, Voxel::Ash), (IVec3::new(6, 4, 5), Voxel::Concrete)]);
        let top_colors = |g: &VoxelGrid| -> Vec<f32> {
            let m = mesh_chunk(g, IVec3::ZERO);
            (0..m.positions.len())
                .filter(|&i| m.normals[i] == [0.0, 1.0, 0.0] && m.positions[i][1] == voxel_to_world(IVec3::new(5, 4, 5)).y)
                .map(|i| m.colors[i][0])
                .collect()
        };
        let o = top_colors(&open);
        assert_eq!(o.len(), 4);
        assert!(o.iter().all(|&c| c == 1.0));
        let w = top_colors(&walled);
        assert_eq!(w.len(), 4);
        assert_eq!(w.iter().filter(|&&c| c < 1.0).count(), 2);
    }

    #[test]
    fn emissive_alpha() {
        let g = grid_with(&[(IVec3::new(2, 2, 2), Voxel::WindowLit), (IVec3::new(8, 2, 8), Voxel::Lamp), (IVec3::new(12, 2, 12), Voxel::WindowDark)]);
        let m = mesh_chunk(&g, IVec3::ZERO);
        let lit = m.colors.iter().filter(|c| c[3] == 1.0).count();
        assert_eq!(lit, 48);
        assert_eq!(m.colors.iter().filter(|c| c[3] == 0.0).count(), 24);
    }

    #[test]
    fn uvs_inside_atlas_tile() {
        let v = Voxel::Rust;
        let g = grid_with(&[(IVec3::new(3, 3, 3), v)]);
        let m = mesh_chunk(&g, IVec3::ZERO);
        let t = v.atlas_tile();
        let (c0, r0) = ((t % 4) as f32 / 4.0, (t / 4) as f32 / 4.0);
        for uv in &m.uvs {
            assert!(uv[0] > c0 && uv[0] < c0 + 0.25, "u {uv:?}");
            assert!(uv[1] > r0 && uv[1] < r0 + 0.25, "v {uv:?}");
        }
    }

    #[test]
    fn side_face_v_increases_downward() {
        let g = grid_with(&[(IVec3::new(3, 3, 3), Voxel::Rust)]);
        let m = mesh_chunk(&g, IVec3::ZERO);
        for i in 0..m.positions.len() {
            if m.normals[i][1] == 0.0 {
                let top = m.positions[i][1] > voxel_to_world(IVec3::new(3, 3, 3)).y + 0.1;
                let row = (Voxel::Rust.atlas_tile() / 4) as f32 / 4.0;
                let local = m.uvs[i][1] - row;
                assert_eq!(top, local < 0.125, "{:?}", m.uvs[i]);
            }
        }
    }

    #[test]
    #[ignore]
    fn perf_noisy_chunk() {
        let mut g = VoxelGrid::new();
        let mut s = 12345u32;
        for y in 0..CHUNK {
            for z in 0..CHUNK {
                for x in 0..CHUNK {
                    s = s.wrapping_mul(1664525).wrapping_add(1013904223);
                    if (s >> 24) & 1 == 1 {
                        g.set(IVec3::new(x, y, z), Voxel::Rubble);
                    }
                }
            }
        }
        let n = 200;
        let t = std::time::Instant::now();
        let mut total = 0;
        for _ in 0..n {
            total += mesh_chunk(&g, IVec3::ZERO).indices.len();
        }
        eprintln!("noisy 16^3 chunk: {:?}/mesh ({} idx)", t.elapsed() / n, total / n as usize);
    }
}
