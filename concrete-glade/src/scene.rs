//! Scene sync: owns the document and the displayed voxel grid, and keeps one mesh
//! entity per non-empty chunk in step with it (remeshing only dirty chunks).

use crate::game::{Doc, Ghost, Grid};
use crate::generate::{apply, terrain};
use crate::mesher::mesh_chunk;
use crate::render::{VoxelMaterialHandle, chunk_mesh_to_bevy};
use crate::structure::{Structure, StructureId, World};
use crate::voxel::{CHUNKS_X, CHUNKS_Y, CHUNKS_Z, VoxelGrid};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;

pub struct ScenePlugin;

/// Cached `terrain(seed)`; recomputed when the document's seed changes.
#[derive(Resource)]
pub struct Terrain {
    pub seed: u64,
    pub grid: VoxelGrid,
}

/// The grid the chunk entities currently show, and those entities.
#[derive(Resource, Default)]
struct Meshed {
    grid: VoxelGrid,
    chunks: HashMap<IVec3, Entity>,
}

#[derive(Component)]
pub struct ChunkEntity(pub IVec3);

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        let seed = cli_arg("--seed")
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(time_seed);
        app.insert_resource(Doc(World::new(seed)))
            .init_resource::<Grid>()
            .init_resource::<Ghost>()
            .init_resource::<Meshed>()
            .add_systems(Update, (sync_terrain, sync_grid, remesh).chain().in_set(SceneSync));
    }
}

/// Grid rebuild + remesh; runs after tools have updated `Doc` / `Ghost`.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct SceneSync;

/// Value following `name` on the command line, e.g. `cli_arg("--seed")`.
pub fn cli_arg(name: &str) -> Option<String> {
    let mut args = std::env::args().skip_while(|a| a != name);
    args.next()?;
    args.next()
}

pub fn time_seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64)
}

/// Same ordering as `generate()`: terrain, walls/blocks in order (minus `hidden`),
/// then `pending` if it is not a road, then all roads (committed, then pending).
pub fn rebuild_grid(
    doc: &World,
    terrain: &VoxelGrid,
    pending: Option<&Structure>,
    hidden: &[StructureId],
) -> VoxelGrid {
    let mut grid = terrain.clone();
    let shown = || doc.placed.iter().filter(|p| !hidden.contains(&p.id)).map(|p| &p.structure);
    let is_road = |s: &&Structure| matches!(s, Structure::Road { .. });
    for s in shown().filter(|s| !is_road(s)).chain(pending.filter(|s| !is_road(s))) {
        apply(&mut grid, s, doc.seed);
    }
    for s in shown().filter(is_road).chain(pending.filter(is_road)) {
        apply(&mut grid, s, doc.seed);
    }
    grid
}

fn sync_terrain(mut commands: Commands, doc: Res<Doc>, cached: Option<Res<Terrain>>) {
    if cached.is_none_or(|t| t.seed != doc.0.seed) {
        commands.insert_resource(Terrain { seed: doc.0.seed, grid: terrain(doc.0.seed) });
    }
}

fn sync_grid(doc: Res<Doc>, ghost: Res<Ghost>, terrain: Option<Res<Terrain>>, mut grid: ResMut<Grid>) {
    let Some(terrain) = terrain else { return };
    if !(doc.is_changed() || ghost.is_changed() || terrain.is_changed()) || terrain.seed != doc.0.seed {
        return;
    }
    let next = rebuild_grid(&doc.0, &terrain.grid, ghost.pending.as_ref(), &ghost.hidden);
    if next != grid.0 {
        grid.0 = next;
    }
}

/// Changed chunks plus their face neighbours: a voxel on a chunk border changes the
/// neighbour's face culling and AO too.
pub fn chunks_to_remesh(new: &VoxelGrid, old: &VoxelGrid) -> Vec<IVec3> {
    let dims = IVec3::new(CHUNKS_X, CHUNKS_Y, CHUNKS_Z);
    let mut out: Vec<IVec3> = Vec::new();
    for c in new.diff_chunks(old) {
        for d in [IVec3::ZERO, IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::NEG_Y, IVec3::Z, IVec3::NEG_Z] {
            let n = c + d;
            if n.cmpge(IVec3::ZERO).all() && n.cmplt(dims).all() && !out.contains(&n) {
                out.push(n);
            }
        }
    }
    out
}

fn remesh(
    mut commands: Commands,
    grid: Res<Grid>,
    mut meshed: ResMut<Meshed>,
    mut meshes: ResMut<Assets<Mesh>>,
    material: Res<VoxelMaterialHandle>,
    mut handles: Query<&mut Mesh3d>,
) {
    if !grid.is_changed() {
        return;
    }
    let start = std::time::Instant::now();
    let dirty = chunks_to_remesh(&grid.0, &meshed.grid);
    for &c in &dirty {
        let m = mesh_chunk(&grid.0, c);
        let existing = meshed.chunks.get(&c).copied();
        if m.indices.is_empty() {
            if let Some(e) = existing {
                if let Ok(h) = handles.get(e) {
                    meshes.remove(&h.0);
                }
                commands.entity(e).despawn();
                meshed.chunks.remove(&c);
            }
            continue;
        }
        let handle = meshes.add(chunk_mesh_to_bevy(&m));
        match existing.and_then(|e| handles.get_mut(e).ok()) {
            Some(mut h) => {
                let old = std::mem::replace(&mut h.0, handle);
                meshes.remove(&old);
            }
            None => {
                let e = commands
                    .spawn((ChunkEntity(c), Mesh3d(handle), MeshMaterial3d(material.0.clone()), Transform::IDENTITY))
                    .id();
                meshed.chunks.insert(c, e);
            }
        }
    }
    meshed.grid = grid.0.clone();
    if !dirty.is_empty() {
        debug!("remeshed {} chunks in {:?}", dirty.len(), start.elapsed());
    }
}

/// Scripted showcase scene for `--demo-shot`: a curved perimeter wall with a road
/// gate, a dam, a merged megastructure, a tower and two roads.
pub fn demo_world(seed: u64) -> World {
    let mut w = World::new(seed);
    let arc = |r: f32, a0: f32, a1: f32, n: usize, c: Vec2| -> Vec<Vec2> {
        (0..=n)
            .map(|i| {
                let a = (a0 + (a1 - a0) * i as f32 / n as f32).to_radians();
                c + Vec2::new(a.cos(), a.sin()) * r
            })
            .collect()
    };
    w.add(Structure::Wall { points: arc(24.0, -20.0, 200.0, 24, Vec2::ZERO), height: 16, dam: false });
    w.add(Structure::Wall {
        points: vec![Vec2::new(-36.0, 4.0), Vec2::new(-30.0, -18.0), Vec2::new(-14.0, -34.0)],
        height: 32,
        dam: true,
    });
    // Two overlapping slabs that merge into one mass, plus an adjacent annex.
    w.add(Structure::Block { min: Vec2::new(-14.0, -10.0), max: Vec2::new(-2.0, 4.0), height: 30 });
    w.add(Structure::Block { min: Vec2::new(-5.0, -4.0), max: Vec2::new(9.0, 5.0), height: 20 });
    w.add(Structure::Block { min: Vec2::new(9.0, -2.0), max: Vec2::new(15.0, 5.0), height: 12 });
    w.add(Structure::Block { min: Vec2::new(-6.0, -24.0), max: Vec2::new(1.0, -17.0), height: 46 });
    // Enters through the perimeter wall (carving a gate) and runs to the block.
    w.add(Structure::Road {
        points: vec![Vec2::new(-36.0, 34.0), Vec2::new(-15.4, 18.4), Vec2::new(-5.0, 11.0), Vec2::new(3.0, 8.0)],
    });
    w.add(Structure::Road { points: vec![Vec2::new(40.0, 0.0), Vec2::new(24.0, 0.0), Vec2::new(17.0, 7.0), Vec2::new(3.0, 8.0)] });
    w
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generate::generate;

    #[test]
    fn rebuild_matches_generate() {
        let w = demo_world(7);
        let t = terrain(7);
        assert!(rebuild_grid(&w, &t, None, &[]) == generate(&w));
    }

    #[test]
    fn remesh_includes_face_neighbours() {
        let old = VoxelGrid::new();
        let mut new = old.clone();
        new.set(IVec3::new(15, 20, 3), crate::voxel::Voxel::Concrete);
        let set = chunks_to_remesh(&new, &old);
        assert!(set.contains(&IVec3::new(0, 1, 0)), "changed chunk");
        assert!(set.contains(&IVec3::new(1, 1, 0)), "+x neighbour across the face");
        assert!(set.iter().all(|c| c.cmpge(IVec3::ZERO).all()), "no out-of-grid chunks");
        assert_eq!(set.len(), 5, "{set:?}");
        assert!(chunks_to_remesh(&old, &old).is_empty());
    }

    #[test]
    fn rebuild_with_pending_and_hidden() {
        let mut w = demo_world(3);
        let t = terrain(3);
        let ids: Vec<_> = w.placed.iter().map(|p| p.id).take(2).collect();
        let pending = Structure::Block { min: Vec2::new(20.0, -20.0), max: Vec2::new(26.0, -12.0), height: 10 };
        let ghost = rebuild_grid(&w, &t, Some(&pending), &ids);
        w.remove(&ids);
        w.add(pending);
        assert!(ghost == generate(&w));
    }

    /// `cargo test --release -- --ignored --nocapture drag_frame_timing`
    #[test]
    #[ignore]
    fn drag_frame_timing() {
        let w = demo_world(9);
        let t = terrain(9);
        let mut prev = rebuild_grid(&w, &t, None, &[]);
        let mut points = vec![Vec2::new(-30.0, 30.0)];
        let (mut rebuild, mut mesh, mut chunks, n) = (0.0, 0.0, 0, 40);
        for i in 0..n {
            points.push(Vec2::new(-30.0 + i as f32 * 1.2, 30.0 - (i as f32 * 0.2).sin() * 4.0));
            let pending = Structure::Wall { points: points.clone(), height: 32, dam: true };
            let s = std::time::Instant::now();
            let g = rebuild_grid(&w, &t, Some(&pending), &[]);
            rebuild += s.elapsed().as_secs_f64();
            let s = std::time::Instant::now();
            let dirty = chunks_to_remesh(&g, &prev);
            chunks += dirty.len();
            for c in dirty {
                std::hint::black_box(chunk_mesh_to_bevy(&mesh_chunk(&g, c)));
            }
            mesh += s.elapsed().as_secs_f64();
            prev = g;
        }
        let ms = |x: f64| x * 1000.0 / n as f64;
        println!("per drag frame: rebuild {:.2} ms, diff+remesh {:.2} ms ({:.1} chunks)", ms(rebuild), ms(mesh), chunks as f64 / n as f64);
        let s = std::time::Instant::now();
        for c in rebuild_grid(&w, &t, None, &[]).diff_chunks(&VoxelGrid::new()) {
            std::hint::black_box(mesh_chunk(&prev, c));
        }
        println!("initial full mesh: {:.1} ms", s.elapsed().as_secs_f64() * 1000.0);
    }
}
