//! Particle meshes for [`super::ParticleMaterial`].
//!
//! The billboard is built in the vertex shader: each particle is 4 vertices that share
//! its centre `POSITION`, with the quad corner in `UV_0`, `(size, kind)` in `UV_1`, the
//! colour + alpha in `COLOR` and a world direction in `NORMAL` (streak orientation).
//!
//! Cheapest way to drive a particle system (rain, snow, smoke, fireflies, birds):
//! keep the particles in a plain `Vec<Particle>` on the CPU, spawn ONE entity with
//! `Mesh3d(meshes.add(particle_mesh(&[])))`, `MeshMaterial3d(assets.particle.clone())`
//! and `NoFrustumCulling`, then every frame call
//! `write_particle_mesh(meshes.get_mut(&handle).unwrap(), &particles)`.
//! Particles are alpha-tested with a 4×4 Bayer screen-door, so `color.w` is opacity.
//! A single moving particle (e.g. one bird) can instead use `particle_mesh(&[p])` with
//! `p.pos = Vec3::ZERO` and move the entity's `Transform`.

use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};

/// Shape drawn by the particle fragment shader.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum ParticleKind {
    /// Round flake (snow), sun/sky lit.
    #[default]
    Flake = 0,
    /// 1-pixel-wide streak along `dir` (rain), unlit.
    Streak = 1,
    /// Soft round puff with dithered falloff (chimney smoke, dust), lit.
    Puff = 2,
    /// Flapping V silhouette (birds), lit.
    Bird = 3,
    /// Bright dot with dithered halo (fireflies, sparks), unlit, less fog.
    Spark = 4,
}

/// One particle. `color` is linear RGB + opacity (screen-door dithered).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Particle {
    pub pos: Vec3,
    /// World size in metres (streak: length).
    pub size: f32,
    pub kind: ParticleKind,
    pub color: Vec4,
    /// Streak direction (e.g. rain velocity); ignored by other kinds.
    pub dir: Vec3,
}

impl Particle {
    pub fn new(kind: ParticleKind, pos: Vec3, size: f32, color: Vec4) -> Self {
        Self { pos, size, kind, color, dir: Vec3::NEG_Y }
    }
}

const CORNERS: [[f32; 2]; 4] = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]];

/// Builds a particle mesh (4 vertices + 6 indices per particle).
pub fn particle_mesh(particles: &[Particle]) -> Mesh {
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    write_particle_mesh(&mut mesh, particles);
    mesh
}

/// Rewrites `mesh` in place with `particles` (reuse one mesh per particle system).
/// An empty list writes one invisible particle (avoids zero-sized GPU buffers).
pub fn write_particle_mesh(mesh: &mut Mesh, particles: &[Particle]) {
    let hidden = [Particle::new(ParticleKind::Flake, Vec3::ZERO, 0.0, Vec4::ZERO)];
    let particles = if particles.is_empty() { &hidden[..] } else { particles };
    let n = particles.len() * 4;
    let mut pos = Vec::with_capacity(n);
    let mut uv0 = Vec::with_capacity(n);
    let mut uv1 = Vec::with_capacity(n);
    let mut col = Vec::with_capacity(n);
    let mut dir = Vec::with_capacity(n);
    let mut idx = Vec::with_capacity(particles.len() * 6);
    for (i, p) in particles.iter().enumerate() {
        let base = (i * 4) as u32;
        for c in CORNERS {
            pos.push(p.pos.to_array());
            uv0.push(c);
            uv1.push([p.size, p.kind as u8 as f32]);
            col.push(p.color.to_array());
            dir.push(p.dir.to_array());
        }
        idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv0);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, uv1);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, dir);
    mesh.insert_indices(Indices::U32(idx));
}

/// A single particle centred on the entity origin (move it with `Transform`).
pub fn particle_quad_mesh(kind: ParticleKind, size: f32, color: Vec4) -> Mesh {
    particle_mesh(&[Particle::new(kind, Vec3::ZERO, size, color)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_sizes() {
        let p = Particle::new(ParticleKind::Puff, Vec3::ONE, 0.5, Vec4::ONE);
        let m = particle_mesh(&[p, p]);
        assert_eq!(m.count_vertices(), 8);
        assert_eq!(m.indices().map(|i| i.len()), Some(12));
        let mut m = m;
        write_particle_mesh(&mut m, &[]);
        assert_eq!(m.count_vertices(), 4);
    }
}
