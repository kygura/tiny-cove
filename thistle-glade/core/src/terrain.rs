//! Terrain heightfield, brush edits, paint layers and the ground/water/scatter meshes.
//!
//! STUB — owned by the terrain task. The public API below is the contract other modules use;
//! keep the signatures, replace the bodies.

use crate::mesh::MeshData;
use crate::model::{PaintLayer, TerrainDab};
use glam::{Vec2, Vec3};

/// Samples per side of the heightfield grid (covers `[-GLADE_HALF, GLADE_HALF]` and a skirt).
pub const GRID: usize = 129;

/// A region the scene flattens the ground under (house footprints, path beds…), applied
/// after the player's dabs. `poly` is a closed counter-clockwise polygon on the ground plane.
#[derive(Clone, Debug, PartialEq)]
pub struct FlattenRegion {
    pub poly: Vec<Vec2>,
    /// Target height; `None` = average of the current ground inside the polygon.
    pub height: Option<f32>,
    /// Blend distance outside the polygon in metres.
    pub margin: f32,
}

#[derive(Clone, Debug)]
pub struct Terrain {
    pub seed: u64,
}

impl Terrain {
    /// Builds the base glade from `seed`, applies `dabs` in order, then `flatten` regions.
    pub fn build(seed: u64, dabs: &[TerrainDab], flatten: &[FlattenRegion]) -> Self {
        let _ = (dabs, flatten);
        Self { seed }
    }

    /// Ground height at `p` (bilinear). Outside the grid: clamps.
    pub fn height(&self, p: Vec2) -> f32 {
        let _ = p;
        0.0
    }

    /// Ground normal at `p`.
    pub fn normal(&self, p: Vec2) -> Vec3 {
        let _ = p;
        Vec3::Y
    }

    /// Water surface height at `p` if `p` is under water.
    pub fn water_level(&self, p: Vec2) -> Option<f32> {
        let _ = p;
        None
    }

    /// Painted layer strength at `p` in 0..=1.
    pub fn paint(&self, layer: PaintLayer, p: Vec2) -> f32 {
        let _ = (layer, p);
        0.0
    }

    /// Ray–terrain intersection (for picking). Returns the hit point.
    pub fn raycast(&self, origin: Vec3, dir: Vec3) -> Option<Vec3> {
        if dir.y.abs() < 1e-6 {
            return None;
        }
        let t = -origin.y / dir.y;
        (t > 0.0).then(|| origin + dir * t)
    }

    /// Ground mesh (grass/dirt/rock by slope and paint, cliff skirt around the glade).
    pub fn ground_mesh(&self) -> MeshData {
        MeshData::default()
    }

    /// Water surface mesh (Surface::Water), one flat sheet per pond / river.
    pub fn water_mesh(&self) -> MeshData {
        MeshData::default()
    }
}
