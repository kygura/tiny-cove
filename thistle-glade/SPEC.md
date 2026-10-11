# THISTLE GLADE — spec

A cozy medieval sandbox builder in the spirit of Tiny Glade, written in Rust with
Bevy 0.20. No goals, no fail state, no inventory. You draw walls, houses, towers,
paths, fences and ponds onto a small grassy glade and the game fills in the
details: stones, windows, doors, chimneys, battlements, ivy, lanterns, flowers.
Sheep graze, birds circle, smoke curls from chimneys.

The look is a PS1-era rasteriser wearing Tiny Glade's palette: low-poly
procedural meshes, a ~240 px tall internal render target upscaled with
nearest-neighbour, vertices snapped to the low-res pixel grid (wobble), affine
(non-perspective) texture mapping on procedural texture atlases, 15-bit colour
with ordered Bayer dithering, and distance fog. On top of that: cel-shading in
hard bands with a real shadow-casting sun (sharp shadows), cool shadow tint,
rim light, and weather (clear, overcast, rain, storm with lightning, fog, snow).
The camera is an orthographic isometric view that orbits in 45° steps (smooth)
and zooms.

## Layout

Cargo workspace in `thistle-glade/`, sharing `../target` with sibling projects
(`.cargo/config.toml`).

- `core/` — crate `glade_core`, **no Bevy dependency**. Document model, history,
  terrain heightfield, procedural mesh generation for every primitive, all
  deterministic and unit-tested. Output is plain `MeshData` buffers.
- `src/` — binary + lib crate `thistle_glade` (Bevy). PS1/cel renderer, camera,
  tools/input, scene sync, weather + day/night, ambient life, HUD, screenshot CLI.

## Model (glade_core)

- Coordinates: world units = metres, y up. The glade is a 64 × 64 m square
  centred on the origin, with a soft falloff to a cliff/meadow skirt at the edge.
- `Terrain`: heightfield 129 × 129 samples over the glade plus an edit layer
  (sculpt deltas) and a paint layer (grass / flowers / dirt / water mask).
  Terrain is a function of `(seed, edits)`; deterministic.
- `Structure` enum — the primitives (see "Primitives" below). Each has a stable
  `id: u32` and plain f32 parameters, so they can be (de)serialised later.
- `Document { seed, terrain_edits, structures }`, `History` with
  `push / undo / redo / can_undo / can_redo`.
- `build(&Document) -> Scene` — pure. Produces per-structure `MeshData`
  (positions, normals, uv, color, material slot) plus a list of `Prop`
  placements (lanterns, smoke emitters, door spots for folk, bird perches…) the
  Bevy side turns into lights/particles/agents. Interactions are resolved here:
  a path crossing a wall cuts a gate/arch; a wall crossing water becomes a
  bridge; houses touching a wall merge into it; terrain under a house/path is
  flattened.
- Decoration randomness hashes `(seed, world position)`, never indices, so a
  live preview equals the committed result.

## Primitives

Ported from Tiny Glade as of its 1.16 "Wood, Flowers, Workshop & more" update
(Sep 2026). Types live in `core/src/model.rs`; generators in the named module.

| Tool | Tiny Glade behaviour ported | Module |
|---|---|---|
| Wall | drag a polyline; height + thickness; stone or wood; battlements; ruined (jagged) top; stone foundation of adjustable height; buttresses/supports on tall walls; wall walk; a path through it cuts an arched gate; a closed ring can be roofed (roof tool); spans water on arches | `walls` |
| Fence | drag a polyline: wooden rail, picket, low dry-stone, wattle; gaps where paths cross | `walls` |
| House | drag a rectangle, then raise; stone / plaster / wood; half-timbering; gable / hip / cone / flat roofs with adjustable pitch (low pitch → flat; very low walls → tent-like shelter); auto windows with shutters + door + chimney with smoke; window boxes, ivy, washing lines and haystacks as auto decor; stone foundation; paths through it become doors | `buildings` |
| Tower | drag a circle (round house); tall round houses get cone roofs, arrow-slit windows, or flat roof with battlements | `buildings` |
| Roof | click inside a closed wall ring to roof it; scroll for pitch / style on houses | `buildings` + `scene` |
| Window / Door | click a wall or house wall to place a window; clicking an existing window upgrades it to a door, a door to a wide doorway; shift-click removes | `buildings`, `walls` |
| Path | draw; width; dirt / cobble / flagstone / setts; cuts gates; stepping stones across water (ford) | `paths` |
| Stairs | drag from a low point to a high point (e.g. ground → wall top): wooden stairs with railings, stone steps, ladder | `paths` |
| Platform | drag a rectangle; wooden deck on posts | `paths` |
| Bridge | drag across a gap: rope bridge (4 railing designs, optional hanging lanterns or flags), stone arch, wooden trestle | `paths` |
| Terrain | raise / lower brush, flatten brush, water brush that carves ponds and rivers | `terrain` |
| Paint | flowers, bushes, grain, dirt, and an eraser | `terrain` (layers) + `nature` (scatter) |
| Trees | click to plant oak, pine, birch, fruit tree | `nature` |
| Clutter | barrel, crate, haystack, well, cart, bench, lantern post, flag pole, washing line, woodpile, rock, bush, beehive, scarecrow | `nature` |
| Colour / style | click a structure to recolour (roof / wall / trim palettes), toggle half-timber, switch material; style picker copies a structure's style to the next placements | Bevy `tools` |
| Eraser | click or drag to remove structures | Bevy `tools` |
| Fog + snow + time | HUD controls for weather, fog density, snow, time of day | Bevy `weather` |

Not ported: group select/move/rotate, Steam Workshop/mod editor, rollercoaster mods.

## Rendering

- Low-res target: height 240 px (aspect follows window), nearest upscale to the
  window, UI drawn at full res on top.
- Custom `Material` (WGSL/WESL) for everything opaque:
  - vertex: clip-space snap to the low-res grid;
  - fragment: affine UVs (divide-by-w trick via `noperspective`),
    procedural atlas sample (stone, brick, roof tiles, plaster, timber, wood,
    cobble, grass, dirt, water ripples), vertex colour tint,
    cel lighting = 3 bands from N·L × shadow (Bevy directional light shadow map
    sampled via PBR view bindings, or own shadow lookup), ambient sky term,
    rim light, emissive windows at night;
  - 15-bit quantise + 4×4 Bayer dither, linear fog to the horizon colour.
- Outline: 1 low-res pixel dark outline from depth/normal discontinuities
  (post-process on the low-res target), faded with fog.
- Water: own material, flat animated quantised ripples, reflection-ish
  sky colour, foam at edges.
- Sky: gradient clear colour from the weather/time palette.

## Weather + time

- Day/night clock (default: slow, 1 game hour ≈ 30 s; paused with T, scrubbed
  with `[` `]`). Sun/moon direction and colours from a keyed palette.
- Weather states: clear, overcast, rain, storm (rain + lightning flashes +
  wind), fog, snow. Auto-cycles with smooth blends; HUD picker overrides.
  Rain/snow are low-res particles (quads drawn in world through the same PS1
  pipeline), puddle darkening on terrain when wet, snow cover tint on up-facing
  surfaces when snowing.

## Ambient life

Sheep wander on grass and avoid structures; birds circle towers and land on
roofs; chimney smoke; fireflies at night near flowers/water; ducks on ponds;
lit windows + lanterns at night (point lights limited to the nearest N).

## Controls

| Input | Action |
|---|---|
| 1–9 / HUD | pick tool |
| LMB drag | draw with tool; for houses/towers drag footprint then move mouse to set height, click to place |
| Scroll while drawing | tool variant (roof style, wall height…) |
| Shift | alternate variant (see tool) |
| Ctrl+Z / Ctrl+Y | undo / redo |
| Esc / RMB tap | cancel |
| RMB drag, Q/E | orbit |
| Scroll (idle) | zoom |
| WASD / MMB drag | pan |
| T, `[` `]` | pause time, scrub hour |
| F12 | screenshot |

## CLI

`--demo-shot out.png [--time H] [--weather NAME] [--seed N]` builds a scripted
demo glade, renders a few frames, saves a screenshot and exits.

## Done

1. `cargo test -p glade_core` passes (generation determinism, undo/redo,
   gate cutting, bridge detection, terrain flattening, every primitive emits
   non-empty finite meshes).
2. `cargo build --release` succeeds with no warnings in project code.
3. Headless demo shots render for: noon clear, dusk rain, night clear, storm,
   snow — each showing walls with gate, houses with different roofs, a tower,
   paths, fence, pond with bridge, trees, flowers; visibly PS1 + cel-shaded.
   (`VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json WGPU_BACKEND=vulkan
   xvfb-run -a -s "-screen 0 1280x720x24" cargo run --release -- --demo-shot out.png`)
4. Every primitive in the list is reachable from the HUD/keys and placeable.
