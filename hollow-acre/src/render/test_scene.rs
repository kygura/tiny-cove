//! Placeholder scene exercising the render seams (`voxel_mesh`, `VoxelMaterials`,
//! `toon_material`). Runs only while the [`TestScene`] resource exists; the scene
//! agent removes it once real world meshing is in place.

use super::palette::kind::*;
use super::{VoxelMaterials, toon_material, voxel_mesh};
use crate::render::ToonMaterial;
use bevy::prelude::*;

/// Marker resource: spawn the placeholder test scene at startup.
#[derive(Resource, Default)]
pub struct TestScene;

const NX: usize = 72;
const NY: usize = 40;
const NZ: usize = 72;
const VOXEL: f32 = 0.5;
const ORIGIN: Vec3 = Vec3::new(-(NX as f32) * VOXEL / 2.0, 0.0, -(NZ as f32) * VOXEL / 2.0);

struct Grid(Vec<u8>);

impl Grid {
    fn get(&self, x: i32, y: i32, z: i32) -> u8 {
        if x < 0 || y < 0 || z < 0 || x >= NX as i32 || y >= NY as i32 || z >= NZ as i32 {
            return AIR;
        }
        self.0[(y as usize * NZ + z as usize) * NX + x as usize]
    }
    fn set(&mut self, x: usize, y: usize, z: usize, v: u8) {
        if x < NX && y < NY && z < NZ {
            self.0[(y * NZ + z) * NX + x] = v;
        }
    }
    fn fill(&mut self, min: [usize; 3], max: [usize; 3], v: u8) {
        for y in min[1]..max[1] {
            for z in min[2]..max[2] {
                for x in min[0]..max[0] {
                    self.set(x, y, z, v);
                }
            }
        }
    }
    fn solid(&self, x: i32, y: i32, z: i32) -> bool {
        !matches!(self.get(x, y, z), AIR | WATER)
    }
}

fn build_grid() -> Grid {
    let mut g = Grid(vec![AIR; NX * NY * NZ]);
    // Rolling terrain: soil below, grass/meadow on top.
    for z in 0..NZ {
        for x in 0..NX {
            let h = 4.0 + 1.5 * ((x as f32) * 0.15).sin() + 1.2 * ((z as f32) * 0.11).cos();
            let h = h.round() as usize;
            for y in 0..h {
                g.set(x, y, z, SOIL);
            }
            let top = if (x * 7 + z * 13) % 11 == 0 { MEADOW } else { GRASS };
            g.set(x, h, z, top);
        }
    }
    // Pond: carve and fill with water up to a level.
    for z in 44..62 {
        for x in 8..28 {
            let d = ((x as f32 - 18.0) / 10.0).powi(2) + ((z as f32 - 53.0) / 9.0).powi(2);
            if d < 1.0 {
                for y in 2..NY {
                    g.set(x, y, z, AIR);
                }
                g.set(x, 1, z, SAND);
                g.set(x, 2, z, WATER);
                g.set(x, 3, z, WATER);
            }
        }
    }
    // Concrete blocks of various heights with dark slabs and windows.
    let blocks = [([30, 20], [42, 30], 22), ([44, 18], [52, 26], 14), ([36, 34], [46, 42], 30), ([54, 30], [60, 40], 8)];
    for (min, max, top) in blocks {
        g.fill([min[0], 2, min[1]], [max[0], top, max[1]], CONCRETE);
        let mut y = 6;
        while y < top {
            g.fill([min[0], y, min[1]], [max[0], y + 1, max[1]], CONCRETE_DARK);
            y += 4;
        }
        // Window rows on the +z / -z faces.
        for y in (7..top.saturating_sub(1)).step_by(4) {
            for x in (min[0] + 1..max[0] - 1).step_by(3) {
                let v = if (x + y) % 3 == 0 { WINDOW_LIT } else { GLASS };
                g.set(x, y + 1, max[1] - 1, v);
                g.set(x, y + 1, min[1], v);
            }
        }
    }
    // Rusty tank + solar on a roof, a lamp post, ivy.
    g.fill([32, 22, 22], [35, 25, 25], RUST);
    g.fill([37, 22, 21], [41, 23, 29], SOLAR);
    g.fill([28, 6, 30], [29, 12, 31], STEEL);
    g.set(28, 12, 30, LAMP);
    g.fill([30, 6, 30], [33, 10, 31], IVY);
    // Gravel path and asphalt strip.
    g.fill([4, 0, 32], [68, 1, 34], GRAVEL);
    for x in 4..68 {
        for z in 32..34 {
            let h = (0..NY).rev().find(|&y| g.get(x as i32, y as i32, z as i32) != AIR).unwrap_or(0);
            g.set(x, h, z, GRAVEL);
        }
        for z in 64..68 {
            let h = (0..NY).rev().find(|&y| g.get(x as i32, y as i32, z as i32) != AIR).unwrap_or(0);
            g.set(x, h, z, ASPHALT);
        }
    }
    // Crystal cluster on a rock base.
    g.fill([58, 4, 50], [65, 6, 57], ROCK);
    for (x, z, h) in [(60, 52, 16), (62, 54, 12), (59, 55, 9), (63, 51, 7)] {
        g.fill([x, 5, z], [x + 2, h, z + 2], CRYSTAL);
    }
    // Monolith with runes, blossom tree, crops.
    g.fill([12, 6, 14], [18, 8, 20], CONCRETE_DARK);
    g.fill([14, 11, 16], [17, 24, 17], MONOLITH);
    for y in (12..23).step_by(2) {
        g.set(15, y, 16, RUNE);
    }
    g.fill([22, 5, 8], [24, 14, 10], WOOD);
    g.fill([18, 13, 4], [29, 19, 14], BLOSSOM);
    g.fill([20, 14, 6], [26, 18, 12], LEAVES);
    g.fill([4, 6, 22], [14, 7, 30], CROP);
    g
}

/// Naive culled mesher with classic 3-neighbour vertex AO (test data only).
type MeshData = (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<u8>, Vec<f32>, Vec<u32>);

fn mesh_grid(g: &Grid) -> MeshData {
    const DIRS: [[i32; 3]; 6] = [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]];
    let (mut pos, mut nrm, mut vox, mut ao, mut idx) = (vec![], vec![], vec![], vec![], vec![]);
    for y in 0..NY as i32 {
        for z in 0..NZ as i32 {
            for x in 0..NX as i32 {
                let v = g.get(x, y, z);
                if v == AIR {
                    continue;
                }
                for d in DIRS {
                    let (nx, ny, nz) = (x + d[0], y + d[1], z + d[2]);
                    let nv = g.get(nx, ny, nz);
                    let hidden = if v == WATER { d != [0, 1, 0] || nv != AIR } else { g.solid(nx, ny, nz) };
                    if hidden {
                        continue;
                    }
                    // Two tangent axes for this face.
                    let axis = d.iter().position(|&c| c != 0).unwrap_or(0);
                    let (u, w) = ((axis + 1) % 3, (axis + 2) % 3);
                    let base = pos.len() as u32;
                    let corners = [(0, 0), (1, 0), (1, 1), (0, 1)];
                    for (cu, cw) in corners {
                        let mut c = [x as f32, y as f32, z as f32];
                        if d[axis] > 0 {
                            c[axis] += 1.0;
                        }
                        c[u] += cu as f32;
                        c[w] += cw as f32;
                        if v == WATER {
                            c[1] -= 0.125;
                        }
                        // AO: sample the three voxels around this corner in the face's outer layer.
                        let side = |du: i32, dw: i32| {
                            let mut p = [nx, ny, nz];
                            p[u] += du;
                            p[w] += dw;
                            g.solid(p[0], p[1], p[2]) as i32
                        };
                        let su = if cu == 1 { 1 } else { -1 };
                        let sw = if cw == 1 { 1 } else { -1 };
                        let (s1, s2, s3) = (side(su, 0), side(0, sw), side(su, sw));
                        let occ = if s1 == 1 && s2 == 1 { 0 } else { 3 - (s1 + s2 + s3) };
                        pos.push((ORIGIN + Vec3::from(c) * VOXEL).to_array());
                        nrm.push([d[0] as f32, d[1] as f32, d[2] as f32]);
                        vox.push(v);
                        ao.push(occ as f32 / 3.0);
                    }
                    // Winding: CCW seen from outside.
                    let flip = d[axis] < 0;
                    let tri = if flip { [0, 2, 1, 0, 3, 2] } else { [0, 1, 2, 0, 2, 3] };
                    idx.extend(tri.iter().map(|&t| base + t));
                }
            }
        }
    }
    (pos, nrm, vox, ao, idx)
}

pub(super) fn spawn_test_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut toon: ResMut<Assets<ToonMaterial>>,
    voxel_materials: Res<VoxelMaterials>,
) {
    let grid = build_grid();
    let (p, n, v, ao, i) = mesh_grid(&grid);
    commands.spawn((
        Name::new("test chunk"),
        Mesh3d(meshes.add(voxel_mesh(&p, &n, &v, &ao, &i))),
        MeshMaterial3d(voxel_materials.opaque.clone()),
    ));
    // A toon prop: wool-white block with a glowing eye.
    let wool = toon.add(toon_material(Color::srgb_u8(0xec, 0xe6, 0xda), Color::BLACK, 0.0, 0.0));
    let eye = toon.add(toon_material(
        Color::srgb_u8(0x5c, 0xf0, 0xe0),
        Color::srgb_u8(0x5c, 0xf0, 0xe0),
        0.6,
        2.0,
    ));
    let body = meshes.add(Cuboid::new(1.5, 1.0, 2.0));
    let eye_mesh = meshes.add(Cuboid::new(0.25, 0.25, 0.1));
    commands
        .spawn((
            Name::new("test sheep"),
            Mesh3d(body),
            MeshMaterial3d(wool),
            Transform::from_xyz(4.0, 3.5, 10.0).with_rotation(Quat::from_rotation_y(0.6)),
        ))
        .with_child((Mesh3d(eye_mesh), MeshMaterial3d(eye), Transform::from_xyz(0.0, 0.2, 1.05)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mesh_is_consistent() {
        let (p, n, v, ao, i) = mesh_grid(&build_grid());
        assert!(!p.is_empty());
        assert_eq!(p.len(), n.len());
        assert_eq!(p.len(), v.len());
        assert_eq!(p.len(), ao.len());
        assert!(i.iter().all(|&k| (k as usize) < p.len()));
        assert!(v.contains(&WATER) && v.contains(&CRYSTAL));
    }
}
