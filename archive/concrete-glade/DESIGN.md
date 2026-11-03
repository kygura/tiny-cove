# CONCRETE GLADE — design brief

## Mood

Tiny Glade's calm, but the calm of a place nobody lives in anymore. The palette is
ash, wet concrete and rust under a low brown sky. Light is weak and flat. The only
warm colours are sodium-yellow windows and a red warning lamp or two. Everything you
build is too big: the camera is the scale of a person, the structures are the scale of
a state.

## Palette (linear-ish sRGB hex, used for atlas tiles, sky and UI)

| Token | Hex | Use |
|---|---|---|
| sky / fog | `#6b6258` | clear colour and fog target (dusty brown-grey) |
| sky-low | `#4a433c` | fog near horizon / UI panel base |
| ash | `#7a766e` | ground |
| scorch | `#3d3833` | crater floors, scorch |
| asphalt | `#2c2b2a` | roads, with lighter `#4b4946` cracks |
| rubble | `#5e5a54` | rubble |
| concrete | `#9a958b` | main walls; board-form lines `#87827a` |
| concrete-dark | `#6a665f` | slabs, bands, bases; water streaks `#58544e` |
| window-dark | `#1d1f22` | unlit windows, slight blue |
| window-lit | `#e3b35a` | lit windows, emissive |
| rust | `#7b4a2e` | steel, tanks, antennas |
| deadwood | `#3b3029` | dead trees |
| lamp | `#d8452f` | warning lamps / floodlights, emissive |
| ui-ink | `#d9d3c7` | UI text |
| ui-accent | `#e3b35a` | selected tool, same as lit windows |

Atlas tiles are 16x16 px, drawn procedurally: concrete gets horizontal board-form
lines every 4 px and a few darker drip streaks; windows get a 1 px dark frame; ash
and scorch get value noise. Keep contrast low inside a tile; the PS1 dither adds grain.

## Rendering look

- Internal height 240 px. Chunky pixels are the point; never smooth the upscale.
- Vertex jitter visible when the camera moves, not so strong that walls tear apart.
- Fog starts ~25 world units from the camera and reaches full sky colour at ~90.
- Sun: low, warm-grey directional light baked into the shader as a simple N·L term
  on top of the per-vertex AO/face shade. No real-time shadows needed.
- Lit windows and lamps ignore fog for the first ~60 units so they glow through dust.

## HUD (full window resolution, sits on top of the upscaled image)

Layout, bottom-centre: one horizontal toolbar, 4 tool slots and a gap, then undo
and redo buttons. Nothing else on screen by default, like Tiny Glade.

- Toolbar panel: background `#4a433c` at 85% opacity, 2 px border `#2c2b2a`, square
  corners (brutalist; no rounding), 12 px padding, 8 px gap, 16 px from bottom edge.
- Tool slot: 72x56 px. Top line is a big glyph-like label (`WALL`, `BLOCK`, `ROAD`,
  `RAZE`) in ui-ink, 16 px. Under it the hotkey (`1`..`4`) at 11 px, 60% opacity.
- Selected slot: background ui-accent, text `#1d1f22`. Hovered slot: border ui-accent.
- Undo / redo slots: 40x56, labels `UNDO` / `REDO` at 11 px; dimmed to 35% opacity
  when the stack is empty.
- Hint line: one line of ui-ink text at 12 px, 60% opacity, centred 8 px above the
  toolbar. Content depends on tool and state, e.g.
  - Wall: `drag to pour a wall  ·  shift: dam`
  - Block idle: `drag a footprint`; block raising: `move mouse to raise  ·  click to cast  ·  esc to cancel`
  - Road: `drag to lay a road`
  - Raze: `drag over structures to demolish`
- Top-left, small and dim (11 px, 45% opacity): title `CONCRETE GLADE` and under it a
  running tally `POURED 12,480 m³` (solid structure voxels × 0.125 m³ scaled up for
  drama: multiply by 1000). It should feel like a bureaucratic counter.

## Interaction feedback

- Ghost preview: while dragging any tool, the structure being made is generated and
  shown live in the same voxel style (it is just the real world regenerated with the
  pending structure appended). Raze highlights by showing the world without the
  structures under the cursor.
- Cursor marker: a small rust-coloured voxel cube on the ground under the cursor.
- Release commits. Esc cancels any pending drag. Nothing ever errors at the player:
  degenerate input (zero-length wall, tiny footprint) is silently ignored.

## Ambience

- Ash particles: a few hundred tiny grey quads drifting slowly down and sideways
  around the camera focus, re-spawned above when they hit the ground.
- Smoke: 2-3 columns of dark cubes rising from tall blocks' rooftop stacks, growing
  and fading.
- Figures: tiny 1x2-voxel dark grey people walking single-file along roads, looping.
  They never stop. Spawn roughly one per 4 world units of road, cap 200.
- Sky: no skybox; the clear colour is the fog colour.
