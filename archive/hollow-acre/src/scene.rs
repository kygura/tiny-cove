//! Owns the editable [`World`] and keeps the chunk meshes in sync with
//! committed document + ghost: regenerate, diff, remesh only dirty chunks.

use crate::game::{DisplayedGrid, Ghost, MeshingDone, World};
use crate::render::{LampLights, VoxelMaterials, voxel_mesh};
use crate::shot::CliArgs;
use acre_core::{CHUNKS_X, CHUNKS_Y, CHUNKS_Z, Document, History, SIZE_X, SIZE_Y, SIZE_Z, Voxel, VoxelGrid, generate, mesh_chunk, voxel_center};
use bevy::prelude::*;
use std::collections::{BTreeSet, HashMap};

/// Dirty chunks remeshed per frame at most (a drag touches a handful; a new world, all).
const REMESH_BUDGET: usize = 48;

#[derive(Component)]
pub struct ChunkMesh;

/// The committed document's voxels (no ghost). Writer: `scene`. Used for picking.
#[derive(Resource, Default)]
pub struct CommittedGrid {
    pub grid: VoxelGrid,
}

#[derive(Resource, Default)]
struct SceneState {
    committed: Option<Document>,
    /// Document currently shown in `DisplayedGrid`.
    shown: Option<Document>,
    pending: BTreeSet<[usize; 3]>,
    entities: HashMap<[usize; 3], Entity>,
}

pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        let seed = app
            .world()
            .get_resource::<CliArgs>()
            .and_then(|a| a.seed)
            .unwrap_or_else(random_seed);
        app.insert_resource(World { history: History::new(Document::new(seed)) })
            .init_resource::<SceneState>()
            .init_resource::<CommittedGrid>()
            .add_systems(Update, (sync_display, remesh).chain());
    }
}

pub fn random_seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1, |d| d.as_nanos() as u64 & 0xffff_ffff)
}

/// Committed doc plus the ghost edit.
pub fn display_doc(committed: &Document, ghost: &Ghost) -> Document {
    let mut doc = committed.clone();
    if !ghost.remove.is_empty() {
        let mut i = 0;
        doc.structures.retain(|_| {
            i += 1;
            !ghost.remove.contains(&(i - 1))
        });
    }
    if let Some(s) = &ghost.add {
        doc.structures.push(s.clone());
    }
    doc
}

fn sync_display(
    world: Res<World>,
    ghost: Res<Ghost>,
    mut state: ResMut<SceneState>,
    mut displayed: ResMut<DisplayedGrid>,
    mut committed: ResMut<CommittedGrid>,
    mut lamps: ResMut<LampLights>,
    mut done: ResMut<MeshingDone>,
) {
    if state.shown.is_some() && !world.is_changed() && !ghost.is_changed() {
        return;
    }
    let doc = display_doc(world.history.doc(), &ghost);
    if state.shown.as_ref() == Some(&doc) {
        return;
    }
    if state.committed.as_ref() != Some(world.history.doc()) {
        committed.grid = generate(world.history.doc());
        state.committed = Some(world.history.doc().clone());
        lamps.0 = find_lamps(&committed.grid);
    }
    let grid = if ghost.add.is_none() && ghost.remove.is_empty() { committed.grid.clone() } else { generate(&doc) };
    let dirty = if state.shown.is_none() {
        all_chunks()
    } else {
        displayed.grid.diff_chunks(&grid)
    };
    state.pending.extend(dirty);
    displayed.grid = grid;
    displayed.version += 1;
    state.shown = Some(doc);
    done.0 = false;
}

fn find_lamps(grid: &VoxelGrid) -> Vec<Vec3> {
    let mut out = Vec::new();
    for y in 0..SIZE_Y as i32 {
        for z in 0..SIZE_Z as i32 {
            for x in 0..SIZE_X as i32 {
                if grid.get(x, y, z) == Voxel::Lamp {
                    let c = voxel_center([x, y, z]);
                    out.push(Vec3::new(c[0], c[1] + 0.25, c[2]));
                }
            }
        }
    }
    out
}

fn all_chunks() -> Vec<[usize; 3]> {
    let mut v = Vec::new();
    for y in 0..CHUNKS_Y {
        for z in 0..CHUNKS_Z {
            for x in 0..CHUNKS_X {
                v.push([x, y, z]);
            }
        }
    }
    v
}

fn remesh(
    mut commands: Commands,
    mut state: ResMut<SceneState>,
    displayed: Res<DisplayedGrid>,
    mats: Option<Res<VoxelMaterials>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut done: ResMut<MeshingDone>,
) {
    let Some(mats) = mats else { return };
    if state.pending.is_empty() {
        if state.shown.is_some() {
            done.0 = true;
        }
        return;
    }
    // A full rebuild (new world) is meshed in one go; drags stay under the budget anyway.
    let take = if state.pending.len() > 3 * REMESH_BUDGET { usize::MAX } else { REMESH_BUDGET };
    let batch: Vec<_> = state.pending.iter().take(take).copied().collect();
    for c in batch {
        state.pending.remove(&c);
        if let Some(e) = state.entities.remove(&c) {
            commands.entity(e).despawn();
        }
        let m = mesh_chunk(&displayed.grid, c);
        if m.is_empty() {
            continue;
        }
        let e = commands
            .spawn((
                ChunkMesh,
                Name::new(format!("chunk {c:?}")),
                Mesh3d(meshes.add(voxel_mesh(&m.positions, &m.normals, &m.voxels, &m.ao, &m.indices))),
                MeshMaterial3d(mats.opaque.clone()),
            ))
            .id();
        state.entities.insert(c, e);
    }
    done.0 = state.pending.is_empty();
}

#[cfg(test)]
mod tests {
    use super::*;
    use acre_core::Structure;

    #[test]
    fn display_doc_applies_ghost() {
        let s = |x| Structure::Spire { at: [x, 0.0], kind: 0 };
        let committed = Document { seed: 1, structures: vec![s(0.0), s(1.0), s(2.0)] };
        let g = Ghost { add: Some(s(9.0)), remove: vec![1] };
        let d = display_doc(&committed, &g);
        assert_eq!(d.structures, vec![s(0.0), s(2.0), s(9.0)]);
        assert_eq!(display_doc(&committed, &Ghost::default()), committed);
    }
}
