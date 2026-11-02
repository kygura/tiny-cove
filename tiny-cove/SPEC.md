# TINY COVE — spec

A cozy, goal-free diorama builder set on a small sunlit island. You build the
home of a fictional seafaring people. Bleached driftwood, thick thatch, dry-stone
dykes, painted carved poles and outrigger canoes ring a turquoise cove. Drag out
dykes, huts, roundhouses, beacon towers, shell tracks, cliff steps, jetties and
rope bridges, and the game fills in the details: lashings, hanging fish and
nets, shuttered windows, smoke holes, mooring posts, crab pots, stilts over
the water. The tide breathes in and out twice a day. Gulls ride the wind,
crabs scuttle along the waterline, and canoes bob at the jetties. When night
falls the beacons are lit.

There is no inventory, no goal and no fail state.

Written in Rust with Bevy 0.20. The look is a PS1-era rasteriser: low-poly
procedural meshes, a 240 px tall internal target upscaled with
nearest-neighbour, vertices snapped to the low-res grid (wobble), affine
texture mapping on procedural atlases, 15-bit colour with ordered Bayer
dithering and distance fog. On top of that it adds cel shading in hard bands
from a real shadow-casting sun (sharp shadows), coloured shadow tint, rim light,
1-pixel outlines, and weather. The camera is an orthographic isometric view
that orbits in 45° steps and zooms.

## Art direction: vibrant

The palette is **vibrant and saturated**, like a hand-painted travel poster run
through a PS1. Avoid greys and mud. Avoid photographic realism.

- Sea: electric turquoise in the shallows (`~#19E0D0`), deepening through
  cyan-teal to saturated cobalt (`~#1050C8`) offshore. Foam is bright white
  bands at the shoreline.
- Sand is warm saffron-cream (`~#F6D58A`). Wet sand at the waterline is a
  deeper honey colour.
- Grass is a lush lime-to-emerald (`~#6CD443` lit, `~#1F9E5A` shadow band).
- Thatch is golden straw (`~#F2B33D`). Turf roofs are bright green.
- Painted trim and carvings use pigment colours: madder red/coral
  (`~#FF5A4E`), hot magenta (`~#E8399A`), indigo-cobalt (`~#3555FF`),
  saffron (`~#FFC21A`) and sea-glass green (`~#2FE0A0`).
- Daub limewash comes in chalk white, ochre, terracotta and sky blue, all
  high-chroma.
- Shadows are never black or grey. They are tinted, saturated violet-blue
  (`~#4A3AA8`) by day and deep indigo at night.
- Sunset is magenta-to-tangerine and night is ultramarine. Lanterns and
  beacon fires are hot orange.

Every number above is a starting point. `DESIGN.md` §Art holds the final
tokens, and the renderer and `core/src/palette.rs` follow them.

## Layout

Cargo workspace in `tiny-cove/`. Its build cache is shared with sibling
projects through `.cargo/config.toml` (`../target`).

- `core/` holds the crate `cairn_cove_core`, which has **no Bevy
  dependency**. It owns the document model, undo history, island terrain
  heightfield and procedural mesh generation for every primitive. It is
  deterministic and unit-tested, and its output is plain `MeshData` plus
  `Marker`s.
- `src/` holds the crate `cairn_cove` (Bevy): renderer, sea, camera,
  tools/input, scene sync, weather, day/night and tide, ambient life, HUD and
  the screenshot CLI.

The engine started as a fork of the sibling `greywild` prototype (PS1/cel
renderer, core mesh pipeline, tool framework). Tiny Cove re-themes all of it
and adds the sea, tide, stilts, jetties, canoes, beacons and wind.

## World

- Units are metres and y is up. The buildable island is a 64 × 64 m area
  centred on the origin (`GLADE_HALF = 32`).
- `SEA_LEVEL = 0.0` is mean sea level. The tide is a render-side oscillation
  of ±`TIDE_RANGE` (0.3 m) with two tides per game day. Core logic always uses
  mean sea level.
- Island terrain, a function of the seed:
  - grassy interior 0.5–3 m above sea level;
  - a sand beach ring that slopes gently into the water;
  - one rocky headland with cliffs (up to ~7 m) on a random side;
  - a sheltered cove bay that cuts into the island on another side;
  - a few offshore sea stacks;
  - beyond the island the seabed falls away towards `FLOOR`.

  The sea is a large animated plane at sea level that reaches the fogged
  horizon.
- Anything whose ground is below `SEA_LEVEL` is **sea**. The Lagoon tool
  carves inland water (tide pools and lagoons) with its own level, as the
  water mask did before.

## Building kit

The kit comes from the setting: a fishing people who build with stone, turf,
driftwood, reed and hide. Rust type names stay generic (`Wall`, `House`,
`Fence`, `Path`, `Stairs`, `Platform`, `Bridge`, `Placed`); the variants and
the player-facing names are the setting's.

| Tool (key) | Gesture | Behaviour |
|---|---|---|
| **Dyke** (1) | drag a polyline | A wall of `WallKind::DryStone` (stacked flat stones in courses, upright copestones), `Turf` (stone base under turf/sod bands with a grassy top) or `Palisade` (sharpened driftwood stakes with a lashed rail). It has height and thickness. **crest**: upright copestones on dry stone, painted stake tips on palisade, flowering thrift on turf. **tumbled**: collapsed sections with spilled stones at the foot. **footing**: a boulder footing. A dyke laid over the sea becomes a **breakwater**: a rubble mole of big boulders with a walkable flat top. A track crossing a dyke cuts a gap flanked by two **standing-stone gateposts**, with a lintel stone when the dyke is tall enough. A closed ring can be roofed. |
| **Windbreak** (2) | drag a polyline | `FenceKind::Wattle` (woven panels between stakes), `Driftwood` (crooked bleached post-and-rail), `NetLine` (posts with fishing nets draped between, bright floats), `Reed` (bundled reed screen). It leaves gaps where tracks cross. |
| **Hut** (3) | drag a rectangle, move the mouse to raise it, click to place | A longhut. `Material::Wattle` is wattle-and-daub with limewash in the wall colour and timber corner posts. `Plank` is vertical planks with painted bands. `Stone` is dry stone. Roofs (`RoofStyle`): `Thatch` (thick steep hipped thatch with a bound ridge and trimmed eaves), `Turf` (low grassy roof with sod edges), `Hide` (stretched hide/sailcloth on poles, low tent-like) and `Boat` (an upturned-hull roof: curved keel ridge, planked and painted strakes). Pitch slider. **carved**: crossed carved gable horns and painted bands. **hangings**: drying fish, net bundles and shell strings under the eaves. **stilts**: posts under the floor. A hut whose footprint is over the sea or a lagoon, or on ground sloping more than ~0.8 m across, gets stilts automatically, with the floor ≥ 1.2 m above sea level and a short ladder at the door. Otherwise the ground under the hut is flattened. Automatic details: a door (hide curtain or plank), small shuttered windows in the trim colour, a smoke hole with smoke, and a lantern by the door. |
| **Roundhouse / Beacon** (4) | drag a circle, raise it | A low round footprint gives a **roundhouse**: a round wall with a cone roof. Thatch gets a topknot, Turf a low dome, Hide a conical tent with poles poking out of the apex, and `Boat` falls back to a thatch cone. A tall one (wall height ≥ 2.5 × radius) is a **beacon**: a tapered dry-stone tower with wind holes and a top platform with an iron fire basket. The basket burns at night and in storms and fog, with flame particles and a hot orange light. Roof options on a beacon are a thatch cone above the fire, or open. |
| **Track** (5) | drag a polyline | `PathKind::Shell` (crushed white shell with a pebble kerb), `Pebble` (cobbled beach pebbles), `Flagstone` (slate slabs with grass joints), `Boardwalk` (planks on sleepers, laid across sand and marsh) and `Stepping` (stepping stones). It has a width, cuts dyke gaps, and becomes stepping stones across water. |
| **Steps** (6) | drag from one point to another (ground, dyke top, jetty, hut floor) | `StairKind::Stone` (cut-stone steps), `Log` (log-and-earth steps held by stakes), `RopeLadder`. |
| **Jetty** (7) | drag a rectangle | A plank deck on log piles. Over water the piles run down to the seabed. Mooring posts with rope coils stand at the corners, a ladder goes down to the water, and every jetty over the sea gets one or two canoes moored alongside (`Boat` markers) that bob with the swell. On land it is a raised stage. |
| **Bridge** (8) | drag between two points | `BridgeKind::Rope { railing }` (sagging plank-and-rope bridge with 4 railing designs: plain rope, netting, plank balusters, knotted rope with floats), `Log` (a big driftwood log trestle) and `Causeway` (a stacked stone causeway with culvert arches). `Hanging`: none, lanterns, or pennants that flutter in the wind. |
| **Opening** (9) | click a hut or dyke wall | Cycles shuttered window → doorway → wide boat door → nothing. |
| **Roof** (0) | click inside a closed dyke ring | Roofs the ring with any roof style. |
| **Tree** (G) | click / drag to scatter | `TreeKind::SeaPine` (wind-bent umbrella pine), `Palm` (leaning palm, fronds), `Cypress` (tall dark flame), `Fig` (broad canopy with fruit) and `Snag` (bleached dead driftwood tree). |
| **Prop** (C) | click to place, scroll to rotate | `ClutterKind::Canoe` (outrigger canoe; on water it floats), `DryingRack` (fish on a rack), `Firepit` (stone ring with fire), `Cairn` (stacked stones), `StandingStone` (carved spiral), `CarvedPole` (painted pole with a bird top), `NetRack`, `Baskets` (woven baskets and clay jars), `LanternPost`, `Pennant` (tall pole with a cloth pennant streaming in the wind), `Rock`, `Bush` (flowering gorse/flax), `CrabPots` and `Woodpile` (driftwood). |
| **Paint** (P) | brush | `PaintLayer::SeaPinks` (thrift flower carpet in magenta/pink), `DuneGrass` (marram tufts), `Wrack` (seaweed on the shore), `Pebbles`, and `Clear` to erase. |
| **Raise / lower** (R) | brush | Hold Shift to lower. |
| **Flatten** (F) | brush | Flattens to the height under the cursor at the start of the stroke. |
| **Lagoon** (V) | brush | Carves a tide pool or lagoon with its own water surface. |
| **Dye** (I) | click a structure | Recolours roof, wall or trim from the vibrant swatches. Also sets material, carved, and the **weathering** slider (salt bleaching and orange/yellow lichen). |
| **Erase** (X) | click / drag | Removes a structure. |

Weather and time live in their own HUD panel (see Weather + time below).

Not included: group select/move, workshop/mod support.

## Model contract (`cairn_cove_core::model`)

`Document { seed, terrain: Vec<TerrainDab>, structures: Vec<(StructureId, Structure)>, next_id }`
and `History` (whole-document undo/redo) are unchanged from the fork. The enums
and fields change as follows; everything else is unchanged.

```rust
pub const GLADE_HALF: f32 = 32.0;
pub const SEA_LEVEL: f32 = 0.0;
pub const TIDE_RANGE: f32 = 0.3;

pub enum PaintLayer { SeaPinks, DuneGrass, Wrack, Pebbles, Clear }

/// Hut / roundhouse / beacon walls.
pub enum Material { #[default] Wattle, Plank, Stone }
/// Dyke construction.
pub enum WallKind { #[default] DryStone, Turf, Palisade }
pub enum RoofStyle { #[default] Thatch, Turf, Hide, Boat }

pub struct Colors { pub roof: u8, pub wall: u8, pub trim: u8 }   // indices into DESIGN.md swatches
pub const ROOF_COLORS: u8 = 6; pub const WALL_COLORS: u8 = 5; pub const TRIM_COLORS: u8 = 5;

pub enum OpeningKind { Window /* shuttered */, Door /* hide-curtain doorway */, WideDoor /* boat door */ }

pub struct Wall {
    pub points: Vec<Vec2>, pub height: f32, pub thickness: f32,
    pub kind: WallKind,
    pub crest: bool,      // copestones / painted tips / thrift
    pub tumbled: bool,    // collapsed sections
    pub footing: f32,     // boulder footing height (0 = none)
    pub roofed: bool, pub roof: RoofStyle, pub roof_pitch: f32,
    pub openings: Vec<Opening>, pub colors: Colors,
    pub weathering: f32,  // 0..=1 salt bleach + lichen
}

pub struct House {
    pub footprint: Footprint, pub wall_height: f32,
    pub roof: RoofStyle, pub roof_pitch: f32, pub material: Material,
    pub carved: bool,     // carved gable horns + painted bands
    pub hangings: bool,   // fish, nets, shell strings under the eaves
    pub stilts: f32,      // explicit stilt height (0 = automatic: only over water / steep ground)
    pub openings: Vec<Opening>, pub colors: Colors, pub weathering: f32,
}

pub enum FenceKind { #[default] Wattle, Driftwood, NetLine, Reed }
pub enum PathKind { #[default] Shell, Pebble, Flagstone, Boardwalk, Stepping }
pub enum StairKind { #[default] Stone, Log, RopeLadder }
// Platform = jetty / stage (fields unchanged: center, half, angle, height, colors)
pub enum BridgeKind { Rope { railing: u8 }, Log, Causeway }
pub enum Hanging { #[default] None, Lanterns, Pennants }
pub enum TreeKind { SeaPine, Palm, Cypress, Fig, Snag }
pub enum ClutterKind { Canoe, DryingRack, Firepit, Cairn, StandingStone, CarvedPole, NetRack,
                       Baskets, LanternPost, Pennant, Rock, Bush, CrabPots, Woodpile }
```

`Terrain` gains `is_sea(p) -> bool` (`height(p) < SEA_LEVEL`). `water_level(p)`
returns `Some(SEA_LEVEL)` over the sea, the lagoon level over lagoons, and
`None` elsewhere.

`mesh::Surface` (one atlas tile each, 25 tiles): `Grass, Dirt, Rock, Sand,
DryStone, Daub, Plank, Timber, Thatch, Turf, Hide, Carving, Shell, Pebble,
Flagstone, Boards, Window, Door, Bark, Leaves, Fiber, Petal, Metal, Glow,
Water`. `Carving` is painted carved wood, tinted by the trim colour. `Fiber`
covers rope, nets, reed and weave. `Glow` is always emissive (lantern glass,
fire).

`mesh::MarkerKind` gains `Fire` (an open flame: firepits and beacon baskets;
flame particles plus a flickering light) and `Boat` (a moored canoe that the
Bevy side spawns as a bobbing entity; `dir` is the hull direction). It keeps
`Lantern, WindowLight, ChimneySmoke, Door, Perch, Pond, Flag`.

`palette.rs` exposes named colour functions for every material in this kit
(`thatch()`, `turf()`, `driftwood()`, `dry_stone()`, `daub(c)`, `shell()`,
`sand()`, `wet_sand()`, `sea_shallow()`, `sea_deep()`, `pigment(i)`, …) with
values from DESIGN.md §Art.

## Scene rules (`core/src/scene.rs`)

`build(&Document) -> Scene` stays pure.

- A track through a dyke cuts a gap with gateposts.
- A dyke segment over the sea becomes a breakwater.
- The ground under a hut is flattened unless the hut stands on stilts.
- Tracks follow the ground.
- A jetty over the sea emits `Boat` markers.
- A Canoe prop placed over water emits a `Boat` marker instead of a static
  mesh.
- Beacons and firepits emit `Fire` markers.
- Decoration randomness hashes `(seed, world position)`, so a live preview
  equals the committed result.

## Rendering

- Low-res target 240 px tall with nearest-neighbour upscale. The UI is drawn
  at full resolution on top.
- A custom opaque material does clip-space vertex snapping, affine UVs and a
  procedural atlas (25 tiles). Lighting is cel shading in 3 hard bands from
  N·L × the shadow map, with the saturated violet shadow tint, rim light,
  emissive windows/lamps/fire at night, 15-bit quantisation with a 4×4 Bayer
  dither, and fog.
- **Sea**: a large plane at sea level + tide. It has snapped vertex waves
  driven by wind and weather, and colour bands by depth (turquoise → cobalt).
  Foam bands mark the shoreline and wrap around rocks and stacks. Sun glints
  are a hard cel specular. Storms add whitecaps and swell.
- **Wet sand**: the ground darkens to honey in a band just above the current
  tide line.
- **Wind**: a shared wind vector sways leaves and fibres in the vertex shader
  and drives smoke, pennants, rain slant and wave direction.
- Post: sky gradient with sun and moon discs and stars at night, plus 1-px
  outlines from depth/normal edges.

## Weather, time and tide

The day/night clock runs at about 30 s per game hour. T pauses it and `[` `]`
scrub the hour. Weather states are clear, overcast, rain, storm (lightning
flashes, whitecaps, strong wind), sea fog (thick low fog bank) and snow. They
auto-cycle with smooth blends and the HUD can override them. Effects include
rain and snow particles, wet darkening, and a snow tint on up-facing surfaces.
Sun shadows stay sharp at all times of day. The tide follows the clock (two
cycles per day). Beacon fires burn at night, in storms and in fog.

## Ambient life

- Gulls circle beacons and perch on ridges.
- Crabs scuttle along the waterline on sand.
- Fish leap and splash offshore.
- Moored canoes bob at jetties.
- Smoke rises from smoke holes and firepits.
- Fireflies come out at night over grass.
- Flames flicker in beacons and firepits.

## Controls

| Input | Action |
|---|---|
| 1–9, 0, G C P R F V I X / HUD | pick tool |
| LMB drag | draw; for huts and roundhouses drag the footprint, move the mouse to set height, click to place |
| Scroll while drawing | tool variant (height, roof style, rotation…) |
| Shift | 45° snap, lower terrain, remove opening |
| Ctrl+Z / Ctrl+Y | undo / redo |
| Esc / RMB tap | cancel |
| RMB drag, Q/E | orbit (45° steps) |
| Scroll (idle), WASD / MMB drag, PgUp/PgDn | zoom, pan, tilt |
| T, `[` `]` | pause time, scrub hour |
| H | hide or show hints |
| F12 | screenshot |

## CLI

- `--demo` starts in the showcase cove.
- `--demo-shot out.png [--time H] [--weather NAME] [--seed N] [--hud]` builds
  the scripted demo cove, renders until pipelines are ready, saves a PNG and
  exits.
- `--tool-smoke out.png` drives every tool through real input and exits 0 on
  pass.

## Done

1. `cargo test --release --workspace` passes. It covers determinism,
   undo/redo, a dyke gap cut by a track, breakwater detection over the sea,
   automatic stilts over water and steep ground, flattening under huts, a jetty
   over the sea emitting `Boat` markers, and every primitive and variant
   emitting non-empty finite meshes.
2. `cargo build --release` succeeds with no warnings in project code.
3. Headless demo shots render for noon clear, dusk rain, night clear (beacons
   and lanterns lit), storm, sea fog and snow. Each shows the island with
   huts in different roof styles, a roundhouse, a beacon, a dyke with
   gateposts, a track, a jetty with moored canoes, a bridge, trees, props, sea
   foam and the beach. The shots are visibly PS1, cel-shaded with sharp
   shadows, and **vibrant**.
   (`VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json WGPU_BACKEND=vulkan
   xvfb-run -a -s "-screen 0 1280x720x24" ../target/release/tiny-cove --demo-shot out.png`)
4. `--tool-smoke` drives all 18 tools and exits 0. Every primitive above can
   be reached from the HUD or keys and placed in the interactive game.
