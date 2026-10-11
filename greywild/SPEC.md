# GREYWILD — spec

A cozy sandbox builder in the spirit of Tiny Glade, re-set in an eco-brutalist
world: board-formed concrete monoliths, cantilevered slabs, pilotis and
sawtooth roofs, slowly taken back by moss, ferns, birch and water. No goals,
no fail state, no inventory. You drag out walls, blocks, towers, paths,
stairs, decks and bridges onto a small valley and the game fills in the
details: formwork texture, deep slot windows, brise-soleil fins, planters,
ivy, rain chains, vent stacks, solar arrays. Deer graze, swifts circle the
towers, steam drifts from the vents.

Written in Rust with Bevy 0.20. The look is a PS1-era rasteriser: low-poly
procedural meshes, a 240 px tall internal target upscaled with
nearest-neighbour, vertices snapped to the low-res grid (wobble), affine
texture mapping on procedural atlases, 15-bit colour with ordered Bayer
dithering and distance fog. On top: cel-shading in hard bands with a real
shadow-casting sun (sharp shadows), cool shadow tint, rim light, 1-pixel
outlines, and weather (clear, overcast, rain, storm with lightning, fog,
snow). Orthographic isometric camera that orbits in 45° steps and zooms.

## Layout

Cargo workspace in `greywild/`, build cache shared with sibling projects
through `.cargo/config.toml` (`../target`).

- `core/` — crate `greywild_core`, **no Bevy dependency**. Document model,
  undo history, terrain heightfield, procedural mesh generation for every
  primitive. Deterministic and unit-tested. Output is plain `MeshData`.
- `src/` — crate `greywild` (Bevy): renderer, camera, tools/input, scene
  sync, weather + day/night, ambient life, HUD, screenshot CLI.

The project started from the engine of the sibling `thistle-glade`
prototype (PS1/cel renderer, core mesh pipeline) and re-themes and
finishes it.

## Model (greywild_core)

- World units are metres, y up. The valley is a 64 × 64 m square centred on
  the origin with a soft falloff to a cliff skirt.
- `Terrain`: 129 × 129 heightfield plus sculpt edits and paint layers
  (flowers / ferns / moss / gravel / water mask). Function of
  `(seed, dabs, flatten regions)`.
- `Structure` enum with stable ids; `Document { seed, terrain, structures }`;
  `History` with push / undo / redo.
- `build(&Document) -> Scene` is pure: per-structure `MeshData` plus
  markers (lamps, vent steam, doors, perches, light windows) that the Bevy
  side turns into lights, particles and agents. Interactions are resolved
  here: a path through a wall cuts a portal; a wall over water becomes an
  aqueduct on arches; terrain under blocks is flattened, paths follow
  the ground.
- Decoration randomness hashes `(seed, world position)`, so a live preview
  equals the committed result.

## Primitives

Every building primitive of Tiny Glade as of its September 2026 anniversary
update (1.16: stairs tool, platforms, new path/floor patterns, plaster
weathering slider, movable clutter, multiple lights), translated to
eco-brutalism. Brutalist architecture is the primitive: everything is cast,
stacked or cantilevered, and nature grows into it.

| Tool | Tiny Glade original | Greywild behaviour |
|---|---|---|
| Wall | stone wall polyline, height, battlements, ruined, foundation, buttresses, wall walk, gate cutting, roofable ring, spans water | board-formed concrete / rammed earth / charred-timber wall; height + thickness; **planter parapet** (crenellated planter boxes with spilling greenery) instead of battlements; **ruined** = broken top with exposed rebar; plinth height; raking buttress fins on tall walls; walkway with low upstands on top of thick walls (automatic); a path through it cuts a portal; a closed ring can be roofed; spans water as an aqueduct on arches |
| Fence | wooden rail, picket, dry-stone, wattle | steel railing, concrete balustrade, gabion, hedge planter; gaps where paths cross |
| House → **Block** | rectangle + raise; material; roofs gable/hip/cone/flat with pitch; half-timbering; auto windows, door, chimney; decor; foundation | rectangle + raise; concrete / rammed earth / timber; roofs **Pitched** (sharp slab gable), **Sawtooth** (north-light), **Shell** (pyramid / dome-cone), **Green** (flat sedum roof with parapet); pitch slider; **fins** (brise-soleil grid) instead of half-timber; auto deep slot windows, recessed door, vent stack with steam; balconies, rain chains, planters, ivy; plinth; very low walls give a pavilion canopy on pilotis |
| Tower | circle; tall → cone roof, arrow slits, flat + battlements | round silo tower: ribbed shell, slit windows, shell/cone roof or green roof with planters |
| Roof | click inside closed wall ring | same, any roof style |
| Window / Door | click wall to add window; click again → door → wide door | slot window → recessed door → wide portal |
| Path | dirt, cobble ×3, flagstone, setts; width; cuts gates; ford stones | gravel, pavers, slabs with grass joints, poured concrete, stepping slabs; width; cuts portals; stepping stones across water |
| Stairs | wood with railings, stone, ladder; ground → wall top | steel stair with railing, cast concrete steps, steel ladder |
| Platform | wooden deck on posts | concrete deck on pilotis |
| Bridge | rope (4 railings, lanterns/flags), stone arch, wooden trestle | cable bridge (4 railing designs, hanging lamps or vines), concrete arch, steel girder |
| Terrain | raise/lower, flatten, water | same |
| Paint | flowers, bushes, grain, dirt, eraser | wildflowers, ferns, moss, gravel, eraser |
| Trees | oak, pine, birch, fruit | oak, pine, birch, fruit, tree fern |
| Clutter | barrel, crate, haystack, well, cart, bench, lantern, flag, washing line, woodpile, rock, bush, beehive, scarecrow | rain barrel, planter, compost heap, cistern, wheelbarrow, concrete bench, sodium lamp post, wind turbine, washing line, log pile, rock, bush, beehive, solar array |
| Colour / style | recolour roof/wall/trim; material; half-timber; plaster peeling; style picker | recolour roof/concrete/accent; material; fins; **overgrowth** slider (moss, ivy, staining — the brutalist version of peeling plaster); style picker |
| Eraser | click/drag remove | same |
| Weather / time | fog, snow, time of day | clear, overcast, rain, storm, fog, snow; time of day |

Not ported: group select/move/rotate, Steam Workshop, mod editor.

## Rendering

- Low-res target 240 px tall, nearest upscale, UI at full res on top.
- Custom material for opaque geometry: clip-space vertex snap; affine UVs;
  procedural atlas (board-formed concrete, bush-hammered plinth, rammed earth
  strata, charred timber, steel, sedum, standing-seam metal, precast panels,
  pavers, slabs, poured concrete, glazing, moss, bark, leaves, water…);
  cel lighting in 3 hard bands from N·L × shadow map; cool shadow tint; rim
  light; emissive windows/lamps at night; 15-bit quantise + 4×4 Bayer
  dither; fog.
- Post: sky gradient, 1-px outline from depth/normal edges, shore foam.
- Overgrowth tint on up-facing concrete driven by the model.

## Weather + time

Day/night clock (1 game hour ≈ 30 s; T pauses, `[` `]` scrub). Weather
states clear, overcast, rain, storm (lightning), fog, snow; auto-cycle with
smooth blends; HUD override. Rain/snow particles, wet darkening, snow tint
on up-facing surfaces. Sharp sun shadows at all times of day.

## Ambient life

Deer wander on grass avoiding structures; swifts circle towers; steam from
vent stacks; fireflies at night near ferns/water; ducks on ponds; lit
windows and sodium lamps at night (nearest N point lights).

## Controls

| Input | Action |
|---|---|
| 1–9, 0 / HUD | pick tool |
| LMB drag | draw; for blocks/towers drag footprint, move mouse to set height, click to place |
| Scroll while drawing | tool variant |
| Ctrl+Z / Ctrl+Y | undo / redo |
| Esc / RMB tap | cancel |
| RMB drag, Q/E | orbit |
| Scroll (idle) | zoom |
| WASD / MMB drag | pan |
| T, `[` `]` | pause time, scrub hour |
| F12 | screenshot |

## CLI

`--demo-shot out.png [--time H] [--weather NAME] [--seed N]` builds a
scripted demo valley, renders a few frames, saves a PNG and exits.

## Done

1. `cargo test --workspace` passes (determinism, undo/redo, portal cutting,
   aqueduct detection, flattening, every primitive and variant emits
   non-empty finite meshes).
2. `cargo build --release` succeeds with no warnings in project code.
3. Headless demo shots render for noon clear, dusk rain, night clear, storm
   and snow, each showing concrete walls with a portal, blocks with
   different roofs, a silo tower, paths, fence, pond with bridge, trees,
   ferns and moss; visibly PS1 + cel-shaded with sharp shadows.
   (`VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json WGPU_BACKEND=vulkan
   xvfb-run -a -s "-screen 0 1280x720x24" cargo run --release -- --demo-shot out.png`)
4. Every primitive above is reachable from the HUD/keys and placeable in
   the interactive game.
