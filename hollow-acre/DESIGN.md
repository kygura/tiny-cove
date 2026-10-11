# HOLLOW ACRE — design brief

## Mood

A Sunday afternoon a hundred years after the end. Tiny Glade's softness and
Townscaper's toy-like delight, painted with Eastward's pixel warmth, Moebius's clean
ink lines and the Ghibli feeling that the grass has quietly won. The concrete is not
grim: it is sun-warmed, pale and a little pink, like a brutalist chapel in late summer
(Sable's big flat colour fields, not a ruin). Moss and ivy soften every edge. The
strange things that grew back are gentle: aether crystals hum teal, rune monoliths
drift above their plinths, wheat glows like embers at dusk. Night is blue and kind,
full of small warm lights. Nothing is threatening; even storms are pretty.

Rules of thumb: saturated but never neon in daylight; shadows are blue-violet, never
black; every scene has at least one warm and one cool accent.

## Palette

All hex are sRGB albedo. Emissive strength is a multiplier on the emissive colour
added after lighting (1.0 = colour shown at full value regardless of light).

| Voxel | Albedo | Emissive (colour, day / night strength) | Notes |
|---|---|---|---|
| Soil | `#7a5a43` | — | tilled rows, under grass |
| Grass | `#7fb24a` | — | main meadow top |
| Meadow | `#a7c25a` | — | tall-grass tufts, sunnier/yellower |
| Rock | `#8a8a93` | — | cool lavender-grey |
| Sand | `#e2cf9a` | — | pond shores |
| Water | `#4fa3b8` | — | see Water below |
| Asphalt | `#4b4a52` | — | old highway, slightly violet |
| Gravel | `#c8b89c` | — | paths, warm beige |
| Concrete | `#d6cbbd` | — | main pour: warm pale grey-beige |
| ConcreteDark | `#a89d92` | — | slabs, kerbs, plinths, board bands |
| Glass | `#5b7f99` | — | unlit windows, blue-teal |
| WindowLit | `#ffd27a` | `#ffc35c`, 0.25 / 1.4 | warm lamp-yellow |
| Rust | `#b5653a` | — | pylons, tanks, railings |
| Steel | `#7d8a96` | — | masts, turbines, rails |
| Ivy | `#4f8a3c` | — | darker, cooler green than Grass |
| Wood | `#8c6242` | — | trunks, planters |
| Leaves | `#5d9e45` | — | canopies, planters |
| Blossom | `#f2a7b8` | — | soft pink |
| Crop | `#f0c75a` | `#ffd86b`, 0.15 / 0.9 | luminous wheat |
| Crystal | `#7fe3d6` | `#5cf0e0`, 0.35 / 1.6 | aether teal |
| Monolith | `#5a5670` | — | dusky slate-violet stone |
| Rune | `#b9a2ff` | `#a98bff`, 0.5 / 1.8 | carved glyph lines |
| Lamp | `#fff0c2` | `#ffe2a0`, 0.2 / 2.0 | lamp heads |
| Solar | `#33486e` | — | deep blue panels |

Emissive strength interpolates with the sun factor `s` (1 day, 0 night):
`strength = mix(night, day, s)`.

Per-voxel variation: multiply albedo by `1 ± 0.04` from a hash of the voxel
position (value only, no hue change) so big walls are not flat paint.

### Outlines and shadow tint

- Outline colour = surface colour converted to HSV, value × 0.35, saturation × 1.2
  (clamped), hue shifted 15° toward blue/violet (toward 250°). Never pure black.
  For the sky silhouette (geometry against sky) use the outlined surface's rule too.
- Cel shadow tint: shadowed surfaces = albedo × `#6a6fa8` (multiply), i.e. cool
  blue-violet. At night use `#3a4270`.

### Time of day

Hours: dawn 05–08, day 08–17, dusk 17–20, night 20–05; blend linearly over the
first and last hour of each phase.

| | Dawn (06:30) | Day (13:00) | Dusk (18:30) | Night (00:00) |
|---|---|---|---|---|
| Sky / clear | `#f4c6a5` | `#a8d8ea` | `#e89a7a` | `#1d2347` |
| Sun colour | `#ffc89a` | `#fff4dc` | `#ffa36b` | moon `#9fb3e8` |
| Sun intensity (rel.) | 0.7 | 1.0 | 0.65 | 0.25 |
| Sun elevation | 12° | 55° | 10° | moon 40° |
| Ambient colour | `#b59ab8` | `#a9bfd6` | `#a585a6` | `#2c3463` |
| Ambient strength | 0.40 | 0.45 | 0.38 | 0.30 |

Sun azimuth sweeps east→west with the hour. At night the moon replaces the sun as
the shadow caster (same shadows, weaker, blue).

### Weather

Weather modifies the time-of-day result. Sky = mix(ToD sky, weather tint, amount).
Fog is exponential distance fog plus height fog in Mist; density per world unit.

| | Clear | Overcast | Rain | Storm | Mist |
|---|---|---|---|---|---|
| Sky tint, amount | — , 0 | `#b8bcc8`, 0.55 | `#8a93a8`, 0.65 | `#4e4a6b`, 0.75 | `#d9dde3`, 0.7 |
| Fog colour | sky | sky | `#8f98ab` | `#55516f` | `#dfe3e6` |
| Fog density | 0.002 | 0.006 | 0.012 | 0.016 | 0.035 + height fog below y=6 wu, 0.08 |
| Sun intensity × | 1.0 | 0.55 | 0.45 | 0.35 | 0.5 |
| Shadow strength | 1.0 | 0.5 | 0.4 | 0.35 | 0.3 |
| Ambient × | 1.0 | 1.15 | 1.1 | 1.0 | 1.25 |

At night, fog colour = mix(weather fog, night sky, 0.7). Transitions: 20 s ease.

- Lightning (Storm only): every 8–25 s random. Flash colour `#c9a8ff`, sets ambient
  to that colour at strength 1.6 for 2 frames, then 0.8 for 3 frames, then decays
  over 0.3 s. Sky jumps to `#8e7ad1` for the same 2 frames. No bolt geometry.

## Render look

- Internal resolution: about 270 px tall. Integer scale s = round(window height / 270),
  internal size = ceil(window / s), nearest-neighbour upscale filling the whole window
  (at most s−1 px cropped at the right/bottom edge; no letterbox). 1280×720 → 427×240 at ×3.
- Pixel snapping: camera position snapped to the low-res texel grid in view space.
- Cel lighting: `NdotL × shadow` → 3 bands.
  - `< 0.15` → shadow (shadow tint above)
  - `0.15 – 0.55` → mid: albedo × 0.82 lit by sun colour
  - `≥ 0.55` → lit: albedo × 1.0 lit by sun colour
  - Rim pop: top-facing faces in the lit band get +6% value. No specular otherwise.
  - Ambient added flat to all bands.
- Shadows: real sun shadow map, 2048², sampled with no PCF (hard edge), bias tuned
  so walls do not acne. Shadow edges land on low-res pixels; that crunch is wanted.
- Outlines (post, at internal res, 1 px):
  - Depth silhouettes: full outline colour, 100% strength. Threshold relative depth
    jump > 0.5 wu.
  - Normal creases (same depth, normal dot < 0.8): 40% blend toward outline colour.
  - Draw only on the nearer side of the edge.
  - Emissive pixels do not get crease outlines (silhouettes only).
- Vertex AO: subtle, final = `mix(1.0, ao, 0.35)`, applied before banding to the
  albedo only (does not shift bands).
- Water: flat top, no cel bands; colour = Water albedo × ambient + 0.3 × sun, sky
  colour mixed in 20%. Shore line: any water pixel whose neighbour (low-res) is
  non-water gets a 1 px foam `#e8f6f2` line. Sparkle: hashed texels flash `#ffffff`
  for 0.15 s, ~1 per 400 water pixels per second, only when sun intensity > 0.5.
  Rain adds rings: 1 px `#cfe6ee` plus-shapes for 0.2 s at random water pixels.
- Cloud shadows (Clear, Overcast): scrolling 2D noise in world xz, feature size
  ~18 wu, speed 1.2 wu/s along the wind, thresholded hard (cel) to multiply sun by
  0.55. Coverage 25% Clear, 60% Overcast. Off in Rain/Storm/Mist.
- Rain: streaks drawn at internal res, 1 px wide, 5 px long (Storm 8 px, slanted
  30°), colour `#c4d4ea` at 55% alpha (night `#7d8cc0`). ~600 visible (Storm 1100).
  Ground splashes: 1–2 px `#dfeaf5` dots lasting 0.1 s.
- Mist: height fog plus slow drifting mote sprites (2×1 px `#ffffff` 25%).
- Night glow: emissive is added after lighting and skips banding, fog only applied
  at 50%. Halo: yes, worth it — cheap low-res bloom: threshold emissive buffer,
  one 3×3 box blur at quarter of internal res, add back at 0.6. It makes the night
  scene read instantly as cozy.
- Lamps also add a small point light (radius 3 wu, warm `#ffd28a`, banded like the
  sun with 2 bands) at night only; cap 32 nearest to camera focus.

## Camera

- Pitch 35°, default yaw 45°. Ortho, height of view in world units: default 36,
  zoom range 14 – 80, scroll step ×1.12, smoothed (critically damped, 0.15 s).
- Orbit: Q/E 90°/s, right-drag 0.3°/px, both smoothed. Pan clamp: focus within
  x,z ∈ [-44, 44].
- Default focus: map centre at ground height.

## HUD

Bevy UI at full window resolution over the upscaled image. Default font only.
Brutalist: square corners, thick slabs, all-caps labels, but in warm soft colours.

Colours: panel `#2f2b33` at 82% opacity, border `#d6cbbd` 2 px (Concrete), ink
`#f3ece1`, accent `#ffc35c` (lit window), accent-ink `#2f2b33`, dim = ink at 45%.

Layout (all from the window edges):

- **Toolbar** bottom-centre, 20 px from bottom. Panel padding 10 px, gap 6 px.
  - 6 tool slots 76 × 58 px: label 16 px (`WALL BLOCK PATH FIELD SPIRE RAZE`) on
    top, hotkey `1`–`6` 11 px at 55% opacity under it.
  - 14 px spacer, then a 2 px vertical divider in border colour, 14 px spacer.
  - `UNDO`, `REDO`: 52 × 58 px, 12 px labels, hotkeys `CTRL Z` / `CTRL Y` at 10 px.
- **Hint line**: 13 px ink at 70%, centred 10 px above the toolbar, no panel.
- **Top-left**, 16 px inset, no panel: `HOLLOW ACRE` 14 px ink at 60%; under it
  (4 px gap) two 11 px dim lines: `POURED 1,240 m³` (solid Concrete+ConcreteDark
  voxels × 0.125, rounded, thousands commas) and `HARVEST 312` (Crop voxel count).
- **Top-right**, 16 px inset: weather picker = 5 slots 70 × 28 px in one panel
  (padding 6, gap 4): `CLEAR OVERCAST RAIN STORM MIST`, 11 px. Below it (6 px gap) a
  panel 1 row: time `13:40` 16 px, then a 28 × 28 `II` / `>` pause button, then the
  phase word `DAY` 11 px dim (DAWN / DAY / DUSK / NIGHT). Hint under it, 10 px dim:
  `T pause · [ ] scrub`.

States (all slots):

| State | Background | Border | Text |
|---|---|---|---|
| idle | panel | none | ink |
| hover | `#4a4450` | accent 2 px | ink |
| selected | accent | accent | accent-ink |
| pressed | accent × 0.85 | accent | accent-ink |
| disabled (undo/redo empty) | panel | none | ink 30% |

The current auto-chosen weather is shown selected; picking one pins it until the
next automatic change. All state changes are instant (no tweening) — brutalist.

Hint text (`·` is U+00B7 with two spaces each side):

- Wall: `drag to pour a wall  ·  shift: rampart`
- Block idle: `drag a footprint`
- Block raising: `move mouse up/down to set floors (N)  ·  click to cast  ·  esc cancels`
- Path: `drag to lay a gravel path  ·  cuts gates through walls`
- Field: `drag to sow a field`
- Spire: `click to raise a CRYSTAL  ·  tab: next kind` — kind names `CRYSTAL`,
  `MONOLITH`, `BLOSSOM TREE`.
- Raze: `drag over structures to clear them`
- Any tool while dragging: hint unchanged except Block raising.

## Interaction feedback

- Cursor marker: a 1-voxel outline square (wireframe, 1 px internal res) on the
  ground cell under the cursor, colour accent `#ffc35c`; Raze uses `#ff7a6b`. Shown
  for all tools when not dragging; for Spire it is a 3 × 3 cell square.
- Ghost preview: the real regenerated world, no tint. Structure voxels that are new
  relative to the committed world get a gentle emissive pulse of `#ffffff` at
  0.0–0.12 strength, 1.2 s sine, so you can see what you're placing. Pulse stops on
  commit.
- Raze: structures touched by the drag are shown removed (the real result); the
  drag rectangle is drawn on the ground as a 1 px `#ff7a6b` outline.
- Commit / undo / redo: no animation; the remesh is instant. A brief 0.25 s puff of
  6–10 dust motes (`#e8dfd2`, 1 px) at the edited bounds' corners on commit.
- Block raising: a floor counter label `4F` (13 px ink on panel) follows the cursor
  12 px to its right.

## Life and ambience

- **Robot sheep**: body 3 × 2 × 4 voxels-equivalent (1.5 × 1 × 2 wu) of off-white
  `#ece6da` wool blocks on 4 Steel `#7d8a96` stub legs (0.25 wu), a 1 × 1 × 1 wu
  head in `#5a5670` with one forward-facing eye `#5cf0e0` emissive (0.6 day / 2.0
  night), a tiny Steel antenna. Mesh is cubes, same cel/outline treatment. Grazing:
  walk 0.6 wu/s, stop 3–8 s with head dipped, wander within meadow (Grass/Meadow),
  turn away from non-terrain voxels within 2 wu. 8–14 per map, more on larger
  grass areas. Hop 0.1 wu bob while walking.
- **Fireflies**: night/dusk only (s < 0.4), within 4 wu of Crop voxels, 1 px
  `#e6ff8a` emissive 1.5, blinking (on 0.6 s, off 1–3 s), drifting slow sine paths
  0.3 wu/s. Cap 120.
- **Steam**: from each Block's rooftop water tank/vents: 2 × 2 px puffs `#f4f1ec` at
  60% → 0% over 3 s, rising 0.8 wu/s, drifting with wind. One puff per 0.5 s per
  source. Disabled in Storm (wind would shred it; it is fine to just fade it).
- **Crystal motes**: 4–8 per crystal cluster, 1 px `#5cf0e0` emissive 1.0, orbiting
  0.5–1.5 wu around, rising/falling ±0.4 wu over 4 s.
- **Monolith bob**: amplitude 0.25 wu, period 4.0 s sine, plus a slow 0.5°/s yaw
  turn. Hover gap above plinth 1.5 wu at rest.
- **Meadow wind**: none in geometry (voxels are static); wind only drives particles.

## Structure shape notes (for the generator)

Units in voxels (1 v = 0.5 wu).

- **Wall**: 2 v thick, 8 v tall (tall: 16 v and 4 v thick at base stepping to 2 v).
  Buttresses every 10 v, 1 v proud, full height. Board-form: every 3rd course is
  ConcreteDark. Slit windows 1 × 2 v Glass every 6 v at mid height (10% WindowLit).
  Walkway on top with 1 v parapet on both sides. Ivy patches from ground: ~25% of
  wall length, 2–5 v high, ragged tops. Lamp on parapet every 16 v.
- **Block**: floor height 4 v (3 v room + 1 v ConcreteDark slab overhanging 1 v).
  Ground floor pilotis: 2 × 2 v columns every 6 v, open between. Windows 2 v wide ×
  2 v tall in a 3 v grid, ~35% WindowLit. Recessed balconies on ~20% of window bays
  with a Leaves/Blossom planter. Stair core: 4 × 4 v Concrete tower 1 floor above
  roof on one corner. Roof: pick by hash — solar rack (Solar, tilted rows), or
  water tank (Rust drum 3 v dia + Steel legs), or turbine mast (Steel 10 v + 3-blade
  cross), or roof garden (Grass + Leaves + Blossom). Clamp floors 1–10.
- **Path**: 3 v wide Gravel, flush (replaces top voxel). Arched gate through walls/
  blocks: 3 v wide, 5 v tall, top 1 v rounded corners.
- **Field**: Soil rows on even z, Crop 1–2 v tall on odd z (hash height). Kerb 1 v
  ConcreteDark, 1 v above ground. Water channel 1 v wide on the min-x side inside
  the kerb.
- **Spire 0 (crystal)**: 5–9 shards, each 1–2 v wide, 4–14 v tall, leaning
  outward up to 25°, on a 7 v Rock base scatter.
- **Spire 1 (monolith)**: plinth disc radius 5 v, 2 v tall ConcreteDark with
  Concrete rim; monolith (separate entity) 3 × 12 × 2 v Monolith with Rune voxels
  in vertical glyph strips (~20% of front/back face).
- **Spire 2 (blossom tree)**: Wood trunk 2 × 2 v, 8 v tall, 2 branches; canopy
  sphere radius 6 v, 70% Blossom / 30% Leaves, noisy edge; 8–12 fallen Blossom
  voxels scattered on the ground within radius 7.
