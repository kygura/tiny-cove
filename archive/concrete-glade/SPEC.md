# CONCRETE GLADE — spec

A Tiny Glade-like sandbox builder in Rust + Bevy 0.20. Instead of a cozy medieval
glade, you draw brutalist megastructures onto a dead, ash-grey wasteland. No goals,
no fail state. Every drag produces something plausible. The look is voxel geometry
rendered like a PS1 game: low internal resolution, wobbling vertex snapping, affine
texture warp, 15-bit color with ordered dithering, heavy distance fog.

## Core flows

1. **Orbit the glade.** Right-drag orbits, scroll zooms, WASD / middle-drag pans.
   The camera stays above the ground and inside the map bounds.
2. **Draw a wall** (tool 1). Left-drag paints a freehand path on the ground. While
   dragging, a ghost wall follows the cursor. On release the path is simplified and
   resampled, then rasterized into voxels as a thick concrete wall. Decoration rules:
   buttresses / pilasters at intervals, slit windows, a walkway parapet on top,
   occasional floodlights and rebar spikes. Hold Shift for a taller "dam" wall.
3. **Raise a block** (tool 2). Left-drag a rectangle footprint on the ground, release,
   then move the mouse up/down to set height and click to confirm (Esc cancels).
   Generated: floor slabs, window grids (some lit, sickly yellow), recessed balconies,
   stair/elevator cores, rooftop machinery, antennas, water tanks. Overlapping or
   adjacent blocks merge into one mass because the world is a voxel grid and meshing
   only sees occupancy.
4. **Lay a road** (tool 3). Left-drag paints a cracked asphalt/ash path into the
   ground layer. Tiny grey figures trudge along roads in lines.
5. **Demolish** (tool 4). Left-drag erases: removes whole structures whose voxels the
   cursor touches.
6. **Undo / redo**: Ctrl+Z / Ctrl+Y (also Ctrl+Shift+Z). New world: N.
7. **Screenshot**: F12 writes `screenshots/<timestamp>.png`.

## Model

- World state = ordered list of `Structure` values (Wall, Block, Road) plus a seed.
- `generate(structures, seed) -> VoxelGrid` is a pure, deterministic function.
  Undo/redo is a stack of structure lists. Demolish removes structures.
- `VoxelGrid` is a fixed 160 x 64 x 160 (x, y, z) grid of `Voxel` (u8-backed enum).
  One voxel = 0.5 world units.
- Meshing is per 16³ chunk: hidden-face culling, per-vertex ambient occlusion,
  vertex colors + texture atlas UVs. Only chunks whose voxels changed are remeshed.
- Terrain: ash ground layer with a few procedural craters, dead trees and rubble,
  generated from the seed and treated as an implicit first structure.

## Rendering

- The 3D camera renders into a low-res image (height 240 px, width from aspect).
- A full-screen pass upsamples it nearest-neighbour to the window.
- Custom voxel material: vertex positions snapped to the low-res pixel grid in clip
  space (PS1 jitter), affine (non perspective-correct) UV interpolation, nearest
  sampled 16x16 atlas tiles, vertex-color lighting, linear distance fog to a dusty
  brown-grey sky, then 15-bit quantization with 4x4 Bayer dithering.
- Ambience: drifting ash particles, slow sun, smoke from a few stacks, lit windows.

## Stack

Rust 2024, Bevy 0.20 (default features). No other runtime crates unless one is
clearly cheaper than writing it (e.g. a noise function is ~30 lines; write it).

## Done

- `cargo test` passes (generation determinism, wall/block/road rasterization, merge,
  undo stack, mesher face culling and chunk dirtiness).
- `cargo clippy --all-targets` has no errors.
- `cargo run --release -- --demo-shot out.png` builds a scripted scene (walls, merged
  blocks, roads), renders a few frames, writes the screenshot and exits 0. Run under
  `xvfb-run` with the lavapipe Vulkan driver in CI-like environments. The screenshot
  visibly shows the PS1 look (chunky pixels, dithering, fog) and the structures.
- Interactive `cargo run --release` supports every flow above.
