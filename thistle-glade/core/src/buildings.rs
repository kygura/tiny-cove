//! Houses, towers and roofs.
//!
//! STUB — owned by the buildings task. Keep the public signatures; replace the bodies.

use crate::mesh::{MeshData, Marker};
use crate::model::{Colors, House, RoofStyle};
use crate::terrain::Terrain;
use crate::walls::Cut;
use glam::Vec2;

/// House / tower mesh (walls, openings, roof, chimney, details) plus markers
/// (chimney smoke, lit windows, door spots, roof perches).
/// `cuts` are passages through the house walls (paths) and become doors / arches.
pub fn house_mesh(house: &House, cuts: &[Cut], terrain: &Terrain, seed: u64) -> (MeshData, Vec<Marker>) {
    let _ = (house, cuts, terrain, seed);
    (MeshData::default(), Vec::new())
}

/// Roof over an arbitrary simple polygon `poly` (counter-clockwise, as `geom::footprint_polygon`)
/// with eaves at height `base_y`. Used for houses and for roofed wall rings.
/// `pitch` as in [`House::roof_pitch`]. Non-rectangular polygons: hip/cone style roofs rise to
/// the centroid; gable falls back to hip.
pub fn roof_mesh(poly: &[Vec2], base_y: f32, style: RoofStyle, pitch: f32, colors: Colors, seed: u64) -> (MeshData, Vec<Marker>) {
    let _ = (poly, base_y, style, pitch, colors, seed);
    (MeshData::default(), Vec::new())
}
