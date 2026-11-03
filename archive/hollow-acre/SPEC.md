# HOLLOW ACRE — spec

A Tiny Glade-like sandbox builder in Rust + Bevy 0.20. The setting is farmland a
century after the end of the world: rolling meadows have grown back over cracked
highways and rusted pylons, and strange things grew with them — aether crystals,
floating rune monoliths, luminous wheat. You draw brutalist concrete buildings into
this landscape. No goals, no fail state, every drag produces something plausible
and pleasant. Cozy, but concrete.

The look: 3D, orthographic isometric camera, rendered at low resolution and upscaled
with nearest-neighbour so it reads as pixel art. Cel-shaded (hard light bands, hard
shadows from a real shadow-casting sun), 1-pixel dark outlines from depth/normal
edges, a day/night cycle and weather (rain, storm, mist).

## Project layout

Cargo workspace in `hollow-acre/`:

- `core/` — crate `acre_core`. Pure Rust, **no Bevy dependency**. Voxel grid,
  structure document + undo/redo, deterministic world generation, chunk mesher.
  Unit-tested. Fast to compile.
- `src/` — binary + lib crate `hollow_acre` (Bevy). Rendering, camera, scene sync,
  tools, weather, life, HUD.

## Model (acre_core)

- `Voxel` is a `#[repr(u8)]` enum. Required kinds (render maps each to a palette
  colour; ones marked * are emissive):
  `Air, Soil, Grass, Meadow (tall-grass tint), Rock, Sand, Water, Asphalt, Gravel,
  Concrete, ConcreteDark, Glass, WindowLit*, Rust, Steel, Ivy, Wood, Leaves,
  Blossom, Crop*, Crystal*, Monolith, Rune*, Lamp*, Solar`.
  Water is non-solid for culling against solids (it is meshed as its own top
  surface only) — treat it as a translucent-ish but opaque-rendered flat layer.
- `VoxelGrid`: fixed 192 × 64 × 192 (x, y, z). One voxel = 0.5 world units.
  World origin: voxel (0,0,0) min corner at world (-48, 0, -48), so the map spans
  x,z ∈ [-48, 48]. Chunk size 16³ → 12 × 4 × 12 chunks.
- `Structure` enum (all coordinates in world units, f32 x/z on the ground plane):
  - `Wall { path: Vec<[f32;2]>, tall: bool }` — board-formed concrete wall along a
    polyline. Buttresses at intervals, slit windows, a walkway with parapet, ivy
    creeping up from the ground in patches, lamps on top every so often. `tall`
    makes it a dam-like rampart.
  - `Block { min: [f32;2], max: [f32;2], floors: u8 }` — brutalist tower/terrace
    block. Pilotis at ground level, floor slabs that overhang, grid of windows
    (some lit), recessed balconies with planters (Leaves/Blossom), stair core,
    rooftop: solar panels, water tank, a small wind turbine mast or a roof garden.
    Overlapping blocks merge visually because meshing only sees occupancy.
  - `Path { path: Vec<[f32;2]> }` — gravel path set flush into the terrain. Where
    it crosses a wall or block ground floor it carves an arched gate/passage.
  - `Field { min: [f32;2], max: [f32;2] }` — tilled soil rows planted with
    luminous wheat (Crop), a low concrete kerb around it, irrigation channel of
    Water along one side.
  - `Spire { at: [f32;2], kind: u8 }` — a fantastical landmark: kind 0 = aether
    crystal cluster, kind 1 = floating rune monolith (hovers above a circular
    concrete plinth), kind 2 = blossom tree (big round canopy).
- `Document { seed: u64, structures: Vec<Structure> }` with `History` providing
  `push(new_doc)`, `undo()`, `redo()`, `can_undo()`, `can_redo()`.
- `generate(&Document) -> VoxelGrid` — pure, deterministic. Order: terrain, then
  fields, walls, blocks, spires in placement order, then paths (which carve).
  Decoration randomness hashes (seed, voxel position), never the structure index,
  so a ghost preview matches what is committed.
- Terrain (from seed): rolling meadow heightmap (surface y roughly 6..18 voxels),
  grass top with soil beneath and rock deeper, a pond or two filled with water to
  a fixed water level, rock outcrops, a cracked old asphalt highway strip crossing
  the map with a rusted pylon or two, small crystal clusters, a few trees (some
  blossoming), meadow tufts. Leave a large calm buildable area in the middle.
- `ground_height(&VoxelGrid, x, z) -> f32` (world units) for picking/placing.
- Mesher: `mesh_chunk(&VoxelGrid, chunk: [usize;3]) -> ChunkMesh` with
  `positions: Vec<[f32;3]>` (world units), `normals`, `voxels: Vec<u8>` (voxel kind
  per vertex), `ao: Vec<f32>` (0..1 per vertex), `indices: Vec<u32>`. Hidden-face
  culling across chunk borders. Flat face normals. Water emits only its top faces,
  lowered by 1/8 voxel.
- `VoxelGrid::diff_chunks(&other) -> Vec<[usize;3]>` changed chunks, including
  neighbours of changed border voxels.

## Game flows (hollow_acre)

1. **Look around.** Orthographic isometric camera (pitch ~35°, yaw free).
   Right-drag or Q/E orbits smoothly, scroll zooms (clamped), WASD / arrows /
   middle-drag pan, clamped to the map. Pixel snapping keeps the low-res image
   stable when panning.
2. **Tools** (keys 1–6 and HUD buttons): Wall (Shift = tall), Block (drag
   footprint, release, move mouse vertically to set floors, click to confirm, Esc
   cancels), Path, Field (drag rectangle), Spire (click; Tab cycles kind), Raze
   (drag over things to remove whole structures they touch).
   While dragging, the world is regenerated with the pending structure appended
   and shown live (ghost preview = the real result). Degenerate input is ignored
   silently. Release commits.
3. **Undo/redo**: Ctrl+Z / Ctrl+Y (also Ctrl+Shift+Z). **N**: new world (new seed).
4. **Time and weather.** Day/night cycle (~6 min per day; T pauses, [ and ] scrub).
   Weather states Clear, Overcast, Rain, Storm, Mist, changing automatically every
   few minutes with smooth transitions; HUD lets the player pick one. Rain = streak
   particles + darker sky; Storm = rain + wind + occasional aether lightning (violet
   flash lighting the whole scene for a frame or two); Mist = dense height fog.
   Moving cloud shadows on the ground in Clear/Overcast. At night lit windows,
   lamps, crops, crystals and runes glow.
5. **Life.** Small grazing robot sheep wander meadows and avoid structures;
   fireflies near crops at night; steam from block rooftops; floating motes around
   crystals. The floating monolith bobs (separate entity riding over its plinth is
   fine).
6. **Screenshot**: F12 → `screenshots/<timestamp>.png`.
7. **Demo**: `--demo-shot <file.png>` builds a scripted scene (several walls,
   merged blocks, a path through a wall, a field, spires), sets afternoon light
   and rain, waits for meshing, writes the screenshot and exits. `--seed N` picks
   the seed. `--time H` (0..24) and `--weather NAME` override for the demo.

## Done

- `cargo test --workspace` passes (core: generation determinism, structure
  rules, carving, mesher culling, undo/redo, diff; game: tool state machine and
  picking math where testable without a window).
- `cargo clippy --workspace` has no errors.
- `cargo run --release -- --demo-shot out.png` (headless via lavapipe + xvfb)
  writes a PNG that visibly shows: pixelated iso view, cel bands, hard shadows,
  outlines, brutalist structures, meadow terrain with fantastical elements, rain.
- Interactive run starts without panics and all tools work.
