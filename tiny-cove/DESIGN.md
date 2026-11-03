**Panel verdict.** Scores (vibrancy / cozy coherence / 15-bit survival / cel hue-shift / coastal-tribal fit / implementability): POSTER 10/6/6/8/7/7 = 7.3, TEXTILE "Folk Pigment" 9/9/7/8/10/9 = 8.7, PS1 "Arcade Vivid" 8/7/10/10/7/9 = 8.5.  
**Winner: TEXTILE "Folk Pigment".** Its named dye swatches (madder, woad, cochineal, saffron, malachite) tie the palette to the people who live in the cove, and its woad-cloth HUD gives Tiny Cove its own identity.  
**Grafted:** from PS1, 5-bit-exact hexes, the `shade()` band function, analogous gradient mid-stops, tint-space atlas tiles and lit/shadow split swatch chips. From POSTER, the complementary-adjacency rule for the demo cove, lilac driftwood grain, foam-shade, the lagoon colour, the fog-never-grey rule and the ink-not-black outline.

# TINY COVE: design

This file replaces the greywild HUD brief. Part A (§Art) is the render contract and Part B (§HUD) is the interface contract. `core/src/palette.rs`, the shaders, `src/render/atlas.rs`, `src/weather.rs` and `src/ui.rs` follow it literally.

---

# PART A: §Art, "Folk Pigment"

The island is painted with the dyes its people grind, boil and trade: madder root, woad, cochineal, saffron, ochre earth and crushed malachite. Each is pushed past what a natural dye can manage and then run through a PS1. Sea and sky are the only colours the islanders did not make, and they are just as loud. Every frame should read like a travel poster: big flat fields of saturated colour, each set against its complement. Saffron thatch sits against a cobalt sky, coral trim against turquoise water, and magenta thrift against lime grass. The demo cove (`core/src/demo.rs`) is laid out so that all three of those pairs are on screen in the noon shot.

## A0. Rules for every world pixel

1. **5-bit-exact albedos.** Every hex in Part A uses only these channel bytes: `00 08 10 19 21 29 31 3A 42 4A 52 5A 63 6B 73 7B 84 8C 94 9C A5 AD B5 BD C5 CE D6 DE E6 EF F7 FF`. A flat face in full noon sun therefore quantises to itself, and the post pass's Bayer dither stays quiet on it. The dither only appears in gradients: sky, fog, sea depth and night. Part B (the HUD) is exempt because it is drawn unquantised at full resolution.
2. **White is reserved.** `#FFFFFF` is used only for sea foam, whitecaps, snowflakes and the lightning flash. Painted and bleached whites stop at `F7`, so nothing on land is brighter than the surf.
3. **No grey, no mud, no black.** Shadows, fog, metal, outline and storm are all hued. The anti-mud floor in A3 enforces this after lighting, so it holds even when a token is weak.
4. **Value comes from hue.** Dark areas are indigo, ultramarine, teal or plum. No world pixel has V < 0.22 except the outline.
5. **Analogous gradients only.** A sky, fog or depth gradient never dithers directly between two complements, because a 2×2 orange/blue checker reads as grey. Every gradient in this file gives a mid stop within about 60° of hue of both ends.
6. **Pattern is pigment.** Spirals, zigzags, fish-scale, chevrons and dot-rows appear only on *made* things: carvings, hide stripes, pennants, shutters and the HUD. Nature gets texture but no motifs.

## A1. Tokens (albedo, sRGB)

These are the values `palette.rs` returns from its named functions (`srgb(hex)` converts them to linear). The token name is the function name.

### Ground and stone

| Token | Hex | Use |
|---|---|---|
| `grass` | `#6BD642` | grass and lawn albedo |
| `grass_tip` | `#ADEF4A` | sunlit blade ticks (accent texel), rim on turf crests |
| `marram` | `#DEE65A` | DuneGrass paint layer |
| `sand` | `#F7D68C` | dry beach, saffron-cream |
| `wet_sand` | `#DEA54A` | honey band above the current tide line |
| `tide_mark` | `#CE7B31` | 1-px wrack line at the high-tide mark |
| `dirt` | `#D6733A` | red-ochre earth, track shoulders, hut floors, log-step fill |
| `rock` | `#CE9C84` | rose-granite boulders, sea stacks, breakwater boulders |
| `cliff` | `#B5635A` | red-sandstone headland faces |
| `cliff_band` | `#EFA55A` | ochre strata stripe on cliffs and stacks |
| `dry_stone` | `#E6C594` | dyke, beacon and gatepost stones (warm buff) |
| `dry_stone_alt` | `#BDA5EF` | every third dyke stone is lilac slate, to give the courses rhythm |
| `stone_joint` | `#84529C` | joints and undercuts in coursed stone |
| `shell` | `#F7EFE6` | crushed-shell track |
| `shell_nacre` | `#FFADA5` | shell chips, shell strings, pink sand specks |
| `pebble_lilac` | `#A59CEF` | pebbles A |
| `pebble_rose` | `#EFB59C` | pebbles B |
| `flagstone` | `#5A73CE` | blue slate slabs |
| `flagstone_edge` | `#7B8CEF` | sunlit slab edge |
| `lichen` | `#FF9C19` | weathering clumps, orange |
| `lichen_yellow` | `#EFE63A` | weathering clumps, second colour |
| `salt_bleach` | `#F7EFD6` | weathering lerp target for wood, daub and thatch |
| `wrack` | `#CE9C10` | Wrack paint layer (amber kelp, never brown) |

### Building materials

| Token | Hex | Use |
|---|---|---|
| `thatch` | `#EFB53A` | golden straw (equals roof swatch 0) |
| `thatch_tip` | `#FFCE6B` | tuft tips, bound ridge |
| `thatch_bind` | `#D67B19` | ridge binding, trimmed eave edge |
| `turf` | `#5ADE4A` | turf roof and turf dyke top, brighter than grass so it reads as a roof |
| `sod` | `#9C4A29` | sod-band edges on turf roofs and turf dykes |
| `hide` | `#EFA56B` | stretched hide, door curtains |
| `hide_seam` | `#DE8C4A` | stitched seams |
| `sailcloth` | `#F7E6AD` | pennant ground, hide-roof light panels |
| `driftwood` | `#EFD6B5` | bleached posts, palisade stakes, driftwood windbreak, woodpile |
| `driftwood_grain` | `#C5A5C5` | grain lines on driftwood (lilac) |
| `timber` | `#BD6329` | oiled corner posts, jetty piles, sleepers, mooring posts |
| `palm_trunk` | `#CE8C4A` | ringed palm trunk |
| `bark` | `#8C4231` | trunks of sea pine, fig and cypress, log steps |
| `bark_hi` | `#B55A42` | bark ridges |
| `boards` | `#DE9C5A` | jetty deck, boardwalk, plank walls, bridge planks |
| `boards_hi` | `#EFBD7B` | sun-bleached board edge |
| `board_gap` | `#7B3129` | plank gaps, bark fissures |
| `rope` | `#EFC56B` | lashings, rope coils, rope bridges, nets |
| `reed` | `#D6C54A` | reed screens, baskets, wattle weave |
| `fiber_dark` | `#B58C29` | rope twist and weave crossings |
| `clay` | `#DE7342` | jars, firepit daub, crab-pot weights |
| `iron` | `#4A3A73` | beacon fire basket, hoops (plum-indigo, never black) |
| `iron_hi` | `#6B5AA5` | rivets, hammered highlights |
| `iron_hot` | `#FF6B29` | basket rim while the fire burns |

### Plants

| Token | Hex | Use |
|---|---|---|
| `leaf_sea_pine` | `#21AD73` | sea pine umbrella (blue-emerald) |
| `leaf_palm` | `#8CDE3A` | palm fronds (acid lime) |
| `leaf_cypress` | `#108C63` | cypress flame, the darkest foliage (dark through saturation) |
| `leaf_fig` | `#42C54A` | fig canopy |
| `fig_fruit` | `#B5317B` | fig fruit dots |
| `snag` | `#EFE6CE` | bleached dead tree (shades lilac) |
| `petal_sea_pink` | `#FF5AB5` | SeaPinks layer, crest thrift on turf dykes |
| `petal_sea_pink_deep` | `#E63A9C` | thrift centres, second dither colour |
| `petal_gorse` | `#FFD619` | gorse on Bush |
| `petal_flax` | `#5A8CFF` | flax on Bush |

### Fire and light

| Token | Hex | Use |
|---|---|---|
| `flame_white` | `#FFF7AD` | 1-px hottest flame core pixel |
| `glow_core` | `#FFE65A` | flame core, lantern glass centre |
| `glow_fire` | `#FF7B19` | flame body |
| `glow_ember` | `#FF3A31` | flame tips, sparks, embers |
| `ember_fade` | `#84195A` | dying spark (fades to plum, never grey) |
| `window_light` | `#FFB542` | night window slots, lantern glass rim |

Point lights: a beacon or firepit `Fire` light is `#FF8C29`, and a `Lantern`/`WindowLight` light is `#FFA542`. Both use the existing 2-band clustered falloff. A fire's light intensity flickers by ±12% at about 8 Hz, stepped and not smoothed.

### Swatches (`ROOF_SWATCHES`, `WALL_SWATCHES`, `TRIM_SWATCHES`: `(name, hex)`)

The defaults are roof 0 Golden Straw, wall 0 Chalk Lime and trim 0 Madder. The demo cove must use every swatch at least once.

**Roof (6)**, index `Colors::roof`. Thatch and Hide roofs multiply their tint-space tile by the swatch. Turf roofs keep `turf` on top and take the swatch only on the sod edge and the ridge binding. Boat roofs paint the swatch on alternating strakes, and the other strakes are `boards`.

| # | Name | Hex | noon mid | noon shadow |
|---|---|---|---|---|
| 0 | Golden Straw | `#EFB53A` | `#CE7B3A` | `#AD3A29` |
| 1 | Sun-bleached | `#FFDE84` | `#D6A57B` | `#B56B63` |
| 2 | Madder Dyed | `#E6523A` | `#C5314A` | `#A51952` |
| 3 | Woad Dyed | `#426BFF` | `#523ADE` | `#3119C5` |
| 4 | Malachite | `#21CE8C` | `#21ADB5` | `#0873A5` |
| 5 | Cochineal | `#D63A8C` | `#B5319C` | `#9C19A5` |

**Wall (5)**, index `Colors::wall`. This is the daub limewash on Wattle walls, the painted bands on Plank walls, and the mortar wash on Stone walls.

| # | Name | Hex | noon mid | noon shadow |
|---|---|---|---|---|
| 0 | Chalk Lime | `#F7EFDE` | `#A59CD6` | `#847BBD` |
| 1 | Ochre Wash | `#F7B53A` | `#CE733A` | `#AD3A29` |
| 2 | Terracotta | `#E6633A` | `#C5313A` | `#A51942` |
| 3 | Sky Wash | `#5AC5FF` | `#527BDE` | `#3131C5` |
| 4 | Rose Lime | `#FF8CBD` | `#D67BBD` | `#B552B5` |

**Trim / pigment (5)**, index `Colors::trim`. Used on carvings, shutters, door fringes, painted bands, stake tips, pole birds, pennants, canoe strakes and net floats.

| # | Name | Hex | noon mid | noon shadow |
|---|---|---|---|---|
| 0 | Madder | `#FF5A4A` | `#D64263` | `#B52163` |
| 1 | Cochineal | `#E63A9C` | `#C531AD` | `#A519AD` |
| 2 | Indigo | `#3152FF` | `#4229DE` | `#2910C5` |
| 3 | Saffron | `#FFC519` | `#D67B21` | `#B54219` |
| 4 | Sea-glass | `#31DEA5` | `#31BDC5` | `#1973AD` |

The "noon mid" and "noon shadow" columns are the output of A3 at noon clear. `palette.rs` exposes them as `ROOF_SHADES`, `WALL_SHADES` and `TRIM_SHADES: [(u32, u32); N]` (mid, shadow), parallel to the swatch tables. The HUD's split chips read them from there. A unit test recomputes them with the CPU port of `shade()` and asserts they match within 1 LSB.

### Water, sky, ink and life

| Token | Hex | Use |
|---|---|---|
| `sea_shallow` | `#19DECE` | depth < 1.5 m |
| `sea_mid` | `#08ADD6` | depth 1.5–4 m |
| `sea_deep` | `#1052C5` | depth 4–10 m |
| `sea_abyss` | `#1929A5` | depth > 10 m and the fogged horizon |
| `foam` | `#FFFFFF` | shore foam bands, whitecaps |
| `foam_shade` | `#8CEFEF` | 1-px seaward edge under each foam band |
| `glint` | `#FFFFDE` | hard cel sun specular |
| `lagoon` | `#42EFC5` | Lagoon tool water, depth < 0.6 m (greener than the sea) |
| `lagoon_deep` | `#19C5BD` | lagoon depth ≥ 0.6 m |
| `lagoon_rim` | `#CEFFF7` | 1-px lagoon edge line |
| `outline_day` | `#31217B` | 1-px keyline by day (indigo-violet) |
| `outline_night` | `#10104A` | 1-px keyline at night |
| `shadow_day` | `#4A3AAD` | noon shadow tint (see A4 for every hour) |
| `shadow_night` | `#21197B` | night shadow tint |
| `smoke` | `#E6DEF7` | smoke puff lit side, 70% alpha via Bayer |
| `smoke_shade` | `#A59CDE` | smoke puff shadow side |
| `smoke_night` | `#5A4A9C` | smoke at night |
| `rain` | `#CEF7FF` | rain streak (1×3 low-res px, 60% Bayer) |
| `firefly` | `#DEFF5A` | 1-px emissive firefly |
| `gull` | `#F7EFDE` | gull body |
| `gull_tip` | `#31217B` | gull wingtips and eye |
| `crab` | `#FF5A31` | crab |
| `fish` | `#9CEFFF` | leaping-fish flash |
| `moon` | `#DEEFFF` | moon disc |
| `moon_mark` | `#A5BDF7` | 3–4 crater pixels on the moon |
| `star` | `#F7EFDE` | stars; 1 in 8 is `#FFADD6` (pink) and 1 in 8 is `#9CDEFF` (cyan) |

`palette.rs` keeps its existing function names where they exist (`thatch()`, `turf()`, `driftwood()`, `dry_stone()`, `daub(c)`, `shell()`, `sand()`, `wet_sand()`, `sea_shallow()`, `sea_deep()`, `pigment(i)`, `foam()`, `lichen()`, `shadow()`, …) and adds one function per token above. Greywild leftovers map like this: `stone()` → `dry_stone`, `foundation()` → `rock`, `wood()` → `timber`, `grass_dry()` → `marram`, `leaves()` → `leaf_fig`, `pine()` → `leaf_sea_pine`, `hay()` → `reed`, `metal()` → `iron`, `glow()` → `glow_core`, `white()` → `salt_bleach`, `flower(i)` → [`petal_sea_pink`, `petal_gorse`, `petal_flax`, `petal_sea_pink_deep`][i % 4].

**Weathering** (slider 0..1 = `w`): wood, daub, thatch and hide lerp toward `salt_bleach` by `0.6·w`. On stone, roofs and timber, hash-cell lichen clumps (cell = 4 texels in world space) cover `0.35·w` of cells as accent texels, two of every three `lichen` and the rest `lichen_yellow`. Weathering never moves toward grey.

## A2. Atlas tiles (25 × 32×32, `src/render/atlas.rs`)

The atlas stays **tint-space**: most texels are a near-white detail ramp that the shader multiplies by the vertex colour (the token or swatch). The ramp is lavender-biased so recesses shade cool, never grey:

`M0 #FFFFFF` (face) · `M1 #E6E6F7` (soft detail) · `M2 #C5C5EF` (joint, seam) · `M3 #9C9CDE` (deep groove)

Alpha classes (texel alpha):

| Alpha | Meaning |
|---|---|
| `1.0` | emissive, untinted |
| `0.75` | **accent**: absolute colour, not multiplied by the vertex tint, but lit and shaded. This class is new. |
| `0.5` | regular tinted |
| `0.0` | cutout, discarded |

Every tile uses at most 4 distinct texel values so it survives affine warping. The "Vertex tint" column names what the mesh writes as the vertex colour.

| # | Surface | Vertex tint | Pattern (ramp texels; accents in *italics*) |
|---|---|---|---|
| 1 | Grass | `grass` | Scattered 1×3 px vertical blade ticks in M1 and 2×2 clumps in M2 on an M0 field, hashed, no grid. 6–8 *`grass_tip`* single-texel flecks per tile. No flowers (paint layers add those). |
| 2 | Dirt | `dirt` | M0 field, 2×2 pebble dots in M1, one 1-px meandering crack in M3. 3 *`pebble_rose`* dots. |
| 3 | Rock | `rock` | Big angular voronoi facets (5 cells). Facet faces alternate M0 and M1, edges are a 1-px M3 crack line, and the sun-side facet edge is a 1-px *`cliff_band`* highlight. Cliffs reuse it, with the mesh banding in `cliff` and `cliff_band` by world y every 1.2 m. |
| 4 | Sand | `sand` | M0 with sparse 1-px M1 grains, 1-px M2 ripple arcs every 6 px on a diagonal, and 2 rare *`shell_nacre`* specks. |
| 5 | DryStone | `dry_stone` | Flat stones 8–12 × 4 px in staggered courses (M0 faces, a 1-px M1 top edge), with 1-px *`stone_joint`* joints. Every third stone is *`dry_stone_alt`*. |
| 6 | Daub | wall swatch | Limewash brushed in horizontal strokes (M0, M1). 3–4 single-texel M2 wattle hints show through, plus one 1-px M3 hairline crack. |
| 7 | Plank | `boards` | Vertical planks 6 px wide with 1-px *`board_gap`* gaps and M1 grain ticks. Rows 12–17 are a painted band of *trim colour* with a 1-texel zigzag top edge (the mesh passes trim in the second vertex colour channel; see the note below). |
| 8 | Timber | `timber` | Long vertical grain streaks in M0, M1 and M2. A 3-px lashing ring of *`rope`* sits 4 px from the top. |
| 9 | Thatch | roof swatch | Staggered 4-px straw tufts drawn as downward chevrons: M0 tip row, M1 body, M2 root row. Every 8th row is a bound course in M3. Roof swatch 0 reproduces `thatch_tip`/`thatch`/`thatch_bind` closely. |
| 10 | Turf | `turf` | Top 10 px is a grass fringe (M0 field with M1 tufts, ragged lower edge). The bottom 6 px is a *`sod`* band with M2 earth lines. Two *`petal_sea_pink`* dots. |
| 11 | Hide | `hide` | Smooth M0 field, one dashed 1-px stitch seam in *`hide_seam`* and a soft M1 fold. Every other tile in u carries a painted spiral (7×7 px) in *trim colour*. |
| 12 | Carving | trim swatch | Deep-cut spiral and fish-scale relief. Raised parts are M0, bevels M1, and recesses *`bark`*. A 1-px *`salt_bleach`* top-edge highlight. |
| 13 | Shell | `shell` | Irregular 2–3 px chips in M0 and M1 on M1 ground, with *`shell_nacre`* chips (1 in 5) and *`pebble_lilac`* chip shadows. |
| 14 | Pebble | `pebble_lilac` | Packed 5–7 px rounds. Each has a 1-px M0 top-left highlight and an M1 body, and half of them are *`pebble_rose`*. Joints are *`wet_sand`*. |
| 15 | Flagstone | `flagstone` | 3–4 irregular slabs (M0, M1) with a 1-px *`flagstone_edge`* sun edge and 1–2 px *`grass`* joints. |
| 16 | Boards | `boards` | Horizontal 8-px deck planks, 1-px *`board_gap`* gaps, 2 M3 nail dots per plank and a 1-px *`boards_hi`* bleached top edge. |
| 17 | Window | trim swatch | A 2-px *`outline_day`* recess frame. Two slatted shutters (M0 slats, M2 gaps) with a chevron cut at the top. The centre 6×10 slot is *`outline_day`* by day and emissive *`window_light`* (alpha 1.0) at night via the existing `WindowLight` path. |
| 18 | Door | `hide` | Hide curtain hanging in 3 folds (M0, M1, M2) over a 2-px *`outline_day`* recess, with a dot-row hem in *trim colour* (1-px dots every 3 px). The Plank door variant reuses tile 7. |
| 19 | Bark | `bark` | Vertical fissured plates: M0 plates, *`bark_hi`* ridges, *`board_gap`* fissures. Palm and Snag reuse it with vertex tint `palm_trunk` and `snag`. The palm adds 1-px M2 rings every 5 px through a mesh UV offset. |
| 20 | Leaves | per tree kind | Scalloped 4–6 px clumps with an M0 lit upper edge, M1 body and M2 under-notch. About 25% of texels are cutout (alpha 0) holes. Tints: SeaPine `leaf_sea_pine`, Palm `leaf_palm`, Cypress `leaf_cypress`, Fig `leaf_fig` + *`fig_fruit`* 2×2 dots, Bush `leaf_fig` + petal accents from Petal. |
| 21 | Fiber | `rope` | Top half: 2-px diagonal rope twist (M0 / *`fiber_dark`*). Bottom half: diamond net mesh with M3 knots on cutout ground. Reed and wattle use vertex tint `reed`. Net floats are separate meshes in trim colour. |
| 22 | Petal | per paint layer | Five-dot 3-px rosettes on cutout ground. SeaPinks: *`petal_sea_pink`* with *`petal_sea_pink_deep`* centres. DuneGrass: `marram` tint on M0/M1 blades. Wrack: `wrack` tint on M0/M2 blobs. Pebbles: tile 14. Gorse and Flax rosettes have *`petal_gorse`* centres. |
| 23 | Metal | `iron` | Hammered 6-px plates (M0, M1), *`iron_hi`* rivets and a 1-px top-edge row in *`iron_hot`* (emissive only while the `Fire` marker is lit). |
| 24 | Glow | none (emissive) | A flame tongue with hard bands, no dither: *`glow_core`* centre with a 1-px `flame_white` heart, a *`glow_fire`* body and *`glow_ember`* tips. Lantern glass uses the same tile with the `window_light` rim on the outer 2 px. Everything is alpha 1.0. |
| 25 | Water | `lagoon` | 1-px hard caustic squiggles in M0 over an M1 field. Lagoons tint it by depth (`lagoon` / `lagoon_deep`). The sea does not use this tile (see A6). |

**Second tint.** Plank, Hide, Door and Window need the trim colour as well as the base tint. The mesh writes trim into the vertex colour's alpha as an index (`trim_index / 8`). The shader decodes it with `pigment(i)` and uses it for texels flagged "trim" (accent alpha 0.75 with the texel RGB = `#000000` sentinel). If the existing `MeshData` cannot carry it, Plank and Hide fall back to `boards` and `hide` with no band.

**Pennants and sails** are not tiles. They use Hide (sailcloth tint `sailcloth`) plus a per-instance trim colour. The motif is spiral, zigzag or fish-scale, chosen by `hash(seed, position) % 3`.

## A3. Cel shading (`glade.wesl`): three bands with hue-shifted shadows

Do the band maths in **sRGB-encoded** space: convert albedo linear→sRGB, shade, then convert back to linear before output. The post pass then quantises as it does now. Tonemapping stays `None`.

Band from `light = max(N·L, 0) × shadowmap`: **lit** when `light ≥ 0.30`, **mid** when `0.08 ≤ light < 0.30`, **shadow** below that. These thresholds are already in the shader.

**`shade(c, band, tint)`** (HSV, all in sRGB):

| | mid | shadow |
|---|---|---|
| hue rotation toward the tint hue | 20° (14° on the warm path) | 36° (25° on the warm path) |
| saturation × | 1.00 | 1.20 |
| value × | 0.90 | 0.78 |
| then mix toward `tint` | 8% | 15% |
| saturation floor | 0.25 if near-white | 0.40 (0.30 if near-white) |

- **Direction rule.** Let H be the albedo hue and T the tint hue. If 60° ≤ H < T, rotate positively (greens and cyans go through teal and blue). Otherwise rotate negatively, which is the *warm path*: yellows and reds go through orange, red and magenta. Never rotate past T. This rule is what keeps grass from going olive and thatch from going green.
- **Near-white** (albedo S < 0.15): take H = T and S = max(S, floor). Chalk Lime and shell therefore shade lilac.

**Band colours:**

```
lit    = rot(albedo, litRot) ⊙ sun·sunI
mid    = shade(albedo, 1, tint) ⊙ mix(sun·sunI, fill·fillI, 0.5)
shadow = shade(albedo, 2, tint) ⊙ fill·fillI
```

`sun` and `fill` are sRGB multipliers from A4. Noon is `#FFFFFF ×1.0` for both, so noon lit equals the albedo. `litRot` is the pre-rotation of the lit band toward the tint (dusk 10°, night 30°, otherwise 0).

**Anti-mud floor** (after lighting, before rim and lanterns): if V < 0.80 and S < 0.35, pull H toward T along the direction rule by `(0.35 − S)/0.35` of the remaining distance, then set S = 0.35. Then apply `V = max(V, 0.22)`. Glow, foam, glint and lightning pixels skip this step.

**Rim**: on faces where `1 − N·V > 0.62` on the back-lit side, add `rim × 0.6` once, about one low-res pixel wide. **Wet** surfaces get `V × 0.82, S × 1.15` (wet things get richer, not duller). **Snow** surfaces (up-facing, `n.y > 0.6`) skip `shade()` and use a fixed ramp: lit `#F7FFFF`, mid `#9CADFF`, shadow `#7B8CFF`.

**Calibration goldens** (noon clear, after the 15-bit quantise; tolerance ±1 LSB per channel). The render worker turns these into a unit test of a CPU port of the band function:

| Albedo | lit | mid | shadow |
|---|---|---|---|
| `grass` | `#6BD642` | `#3AB54A` | `#21945A` |
| `sand` | `#F7D68C` | `#CEA584` | `#AD6B6B` |
| `wet_sand` | `#DEA54A` | `#BD734A` | `#9C3A3A` |
| `thatch` | `#EFB53A` | `#CE7B3A` | `#AD3A29` |
| `dry_stone` | `#E6C594` | `#C58484` | `#A56B73` |
| `driftwood` | `#EFD6B5` | `#CE84B5` | `#AD6B7B` |
| `turf` | `#5ADE4A` | `#42BD63` | `#299C73` |
| `leaf_palm` | `#8CDE3A` | `#4ABD3A` | `#199C3A` |
| `leaf_cypress` | `#108C63` | `#107B84` | `#084A73` |
| `boards` | `#DE9C5A` | `#BD6B5A` | `#9C3A42` |
| `flagstone` | `#5A73CE` | `#5A52B5` | `#4231A5` |
| `iron` | `#4A3A73` | `#3A316B` | `#312963` |

Other rigs for sanity: night `grass` lit is `#19523A`, night `sand` lit `#5A3A52` (plum, not grey), and night `thatch` lit `#522121` (maroon, not olive). At golden hour `thatch` lit is `#EF9421`, and at dusk `chalk` lit is `#CE8494`.

## A4. Light rig by time of day

Keyframes are interpolated in sRGB by the hour: 04:30 night → 06:00 dawn → 08:30 noon → 15:00 noon → 17:00 golden → 19:00 dusk → 20:30 night. Bands stay hard at every hour, and shadows stay sharp because the shadow map is never filtered. Sun altitude follows the hour and peaks at 60° at 12:00. The moon takes over the light direction from 20:30 to 04:30.

| | Dawn 06 | Noon 12 | Golden 17 | Dusk 19 | Night 23 |
|---|---|---|---|---|---|
| sun × sunI | `#FFC5A5` × 0.90 | `#FFFFFF` × 1.00 | `#FFD69C` × 1.00 | `#FFADC5` × 0.85 | moon `#A5ADFF` × 0.55 |
| fill × fillI | `#D6BDFF` × 0.85 | `#FFFFFF` × 1.00 | `#EFB5DE` × 0.90 | `#B594EF` × 0.78 | `#7B8CFF` × 0.62 |
| shadow tint | `#6342AD` | `#4A3AAD` | `#7329A5` | `#42198C` | `#21197B` |
| litRot | 0° | 0° | 0° | 10° | 30° |
| rim | `#FF9CCE` | `#CEF7FF` | `#FFD631` | `#FF4AAD` | `#63DEFF` |
| sky zenith → mid → horizon | `#5263DE` → `#C584D6` → `#FFB5A5` | `#1973F7` → `#42ADFF` → `#8CE6FF` | `#3A5AD6` → `#EF84A5` → `#FFC55A` | `#3A2194` → `#E63A9C` → `#FF7B42` | `#081063` → `#102994` → `#2152C5` |
| sun/moon disc | `#FFD6AD`, low | `#FFFFDE` | `#FFE673` | `#FF8C52`, half-set | moon `#DEEFFF` + `#A5BDF7` marks |
| fog | `#EFADC5` | `#B5EFFF` | `#FFBD84` | `#B54AA5` | `#19297B` |
| outline | `outline_day` | `outline_day` | `outline_day` | lerp 50% | `outline_night` |

**Sky.** The sky is a vertical gradient: zenith at the top of the screen, mid at 55% and horizon at the sea line. Bayer dither runs only across each stop boundary, 8 low-res px wide, so the sky reads as three posterised bands. The sun disc is a hard 6-px circle with a 1-px ring of the horizon colour. Stars show when sun altitude is below −6°: 1 px, hashed, about 1 per 120 sky pixels in the top 60% of the sky, twinkling by toggling 1 in 16 every 0.5 s. The moon disc is 5 px.

**Night emissives.** Glow, `WindowLight` and `Fire` run at full strength from 19:30 to 06:00. Lanterns light from 19:00. The dither amplitude rises from 1.0 to 1.5 LSB at night for extra crunch in the blues.

### Weather modifiers (deltas on the current time row, blended over 6 s)

Weather may move hue and value, but it never pushes a lit band below S 0.30. The anti-mud floor guarantees that.

| Weather | Delta |
|---|---|
| **Overcast** | sunI × 0.60 with the sun colour mixed 50% toward `#F7EFFF`. Fill → `#C5BDFF` × 1.25. Shadow tint → `#6B63CE` (sharp shadows, lower contrast). Rim off. Sky `#7BA5EF` → `#A5BDFF` → `#CEDEFF`. Fog `#C5CEFF`. Sea shallow → `#21C5BD` (jade). Lit-band saturation +10% to compensate. |
| **Rain** | sunI × 0.45. Fill `#8C9CF7` × 1.0. Tint `#4A3AB5`. Sky `#4A6BD6` → `#6B94EF` → `#9CB5FF`. Fog `#6B9CE6`, density × 1.4. Every up-facing surface is wet (A3 wet rule) and puddles get the existing hard glint in `glint`. Streaks are `rain`, slanted by wind. Sea mid → `#08A5C5`, foam bands +1 px, rain-ring ripples 1 px `rain`. |
| **Storm** | sun `#8CEFE6` × 0.30. Fill `#5A7BDE` × 0.90. Tint `#31107B`. Rim `#42FFE6` (every ridge carries a cyan hairline). Sky zenith `#29107B` → mid `#2952BD` → horizon `#19A5AD` (violet → blue → teal, analogous). Fog `#29639C`. Sea shallow `#10C5B5`, mid `#1084AD`, deep `#103A9C`, abyss `#10216B`, foam bands × 2 wide, whitecaps `foam` on 1 in 6 wave crests. **Lightning**, every 6–14 s, lasts 2 frames: the whole frame is lit as if sunI = 1.6 with sun `#E6DEFF`, and the sky is flat `#CEBDFF`. Then it snaps back with no fade. Beacons are lit, the only warm hue on screen. |
| **Sea fog** | sunI × 0.65. Fill = fog colour × 0.85. Fog colour by time: dawn `#FFCEDE`, noon `#CEF7EF`, golden `#FFD6AD`, dusk `#D68CC5`, night `#293A9C`. It is a height fog, dense below 3 m with a hard banded top edge, so roofs and beacons poke out. Sky above is clear but lerped 40% toward the fog colour. Sea turquoise shows only within about 6 m of shore. Outlines fade into the fog colour, never to grey. Beacon and lantern light makes a Bayer halo disc of `#FF9C42` at 30%, radius 2 m. Beacons are lit. |
| **Snow** | sun `#FFF7EF` × 0.90. Fill `#ADC5FF` × 1.0. Tint `#5A5AE6`. Sky `#73B5FF` → `#A5D6FF` → `#DEF7FF`. Fog `#CEE6FF`. Up-facing surfaces use the snow ramp (A3). Trim pigments, thatch eaves and vertical faces keep full chroma, so trim pops more than usual. Flakes are 1-px `#FFFFFF` with a `#9CADFF` pixel below. The sea moves one band deeper: shallow `#10A5CE`. |

## A5. Shadow and outline

The shadow tint is per hour (A4). It is applied through `shade()`, never as a multiply-to-black. Every world edge (silhouette and crease, as now) gets a 1-px outline, using `outline_day`/`outline_night` from the A4 row. Under fog the outline lerps toward the fog colour by fog amount. Over sea pixels the shore/silhouette edge uses `foam` instead (the current `shore` branch), so islands get a white keyline where they meet water.

## A6. Sea (`sea` material / `glade.wesl` water path)

- **Depth bands** (depth = water surface − seabed, including tide). Hard steps: `sea_shallow` < 1.5 m ≤ `sea_mid` < 4 m ≤ `sea_deep` < 10 m ≤ `sea_abyss`. At each step there is a 0.25 m-wide 50% Bayer checker seam, which is the only dither on water. Distance fog takes the far sea to `sea_abyss` and then to the fog colour.
- **Bands by time** (shallow / mid / deep / abyss / foam / glint):

| | shallow | mid | deep | abyss | foam | glint |
|---|---|---|---|---|---|---|
| Dawn | `#4ADEE6` | `#3194D6` | `#2952C5` | `#3131A5` | `#FFEFF7` | `#FFCEC5` |
| Noon | `#19DECE` | `#08ADD6` | `#1052C5` | `#1929A5` | `#FFFFFF` | `#FFFFDE` |
| Golden | `#3AE6B5` | `#19B5BD` | `#194AB5` | `#29299C` | `#FFF7DE` | `#FFD67B` |
| Dusk | `#31B5CE` | `#3A63C5` | `#2931A5` | `#29198C` | `#FFC5DE` | `#FF7BD6` |
| Night | `#1984A5` | `#105A9C` | `#10318C` | `#10196B` | `#9CBDFF` | `#CEF7FF` (moon) |

- **Foam.** Band 1 is a 2-low-res-px `foam` line at the current waterline, wrapping rocks, stacks, piles and breakwaters. Band 2 is a 1-px `foam` line 0.6 m seaward that breathes in and out by ±0.2 m with the swell. Each band has a 1-px `foam_shade` edge on its seaward side. In a storm both are doubled.
- **Glint.** A hard cel specular: a single `glint` pixel on wave-crest facets where `dot(R, V) > 0.985` and the face is lit. Only about 1 crest in 3 shows one. At night the moon glint uses the night glint colour, and each `Fire`/`Lantern` within 12 m adds a vertical broken reflection streak of `glow_fire` dashes (1 px wide, 2-px gaps) on the water below it.
- **Waves.** Keep the existing snapped vertex waves. Wave direction follows the wind. Storms raise the amplitude × 2.5 and add whitecaps.
- **Tide line and wet sand.** Ground between the current water level and +0.35 m above it is `wet_sand` (it moves with the tide). When the tide is falling, 1 in 40 wet-sand texels in sun shows a `glint` pixel (the glisten). A 1-px dotted `tide_mark` line marks the highest tide (`SEA_LEVEL + TIDE_RANGE + 0.05`), with 1 `wrack` dot every 4 px.
- **Lagoon water.** Flat `lagoon` / `lagoon_deep` with the Water tile's caustics, a 1-px `lagoon_rim` edge, and no foam. It takes the same time-of-day tint as the sea's shallow band (multiply by `sea_shallow(t) / sea_shallow(noon)`).

## A7. Fire, lanterns, smoke and life

- **Fire marker.** 6–10 flame particles per fire, 1–2 low-res px each. They live 0.6 s and step through `flame_white` → `glow_core` → `glow_fire` → `glow_ember` in four equal stages, then become one spark in `glow_ember` → `ember_fade`. Fire light is `#FF8C29` with a ±12% stepped flicker. Beacon baskets burn at night (19:30–06:00), in storms and in sea fog. Firepits burn from 18:00 to 07:00.
- **Lanterns.** The glass is a Glow tile. The light is `#FFA542`, lit from 19:00.
- **Smoke** from smoke holes and firepits: 2×2 px puffs with `smoke` on the lit side and `smoke_shade` on the shadow side, `smoke_night` at night, drifting with the wind and Bayer-faded over 3 s.
- **Life.** Gulls are `gull` with `gull_tip`. Crabs are `crab`. Fish flash `fish` for 2 frames at the top of a leap. Fireflies are 1-px emissive `firefly`, blinking in 0.4 s pulses over grass from 21:00 to 04:00.

## A8. Signature shots (the `--demo-shot` targets)

- **Noon clear.** The top third of the frame is a cobalt-to-aqua sky in three hard bands. Below it, a turquoise wedge of cove steps through cyan to cobalt, ringed by two white foam bands and a honey wet-sand strip. Three huts stand in a row: Chalk Lime under Golden Straw thatch, Sky Wash under a Madder Dyed hide roof, and Rose Lime with a Boat roof striped Woad and boards. Short violet shadows sit under the eaves. A buff dyke with lilac courses and thrift on its crest crosses lime grass, with gateposts on the white shell track. A Malachite-roofed roundhouse stands near a beacon on the red-sandstone headland. The jetty is warm boards, and at it two canoes with Madder strakes ride the turquoise.
- **Dusk rain.** The sky runs violet → magenta → tangerine and then blends to periwinkle as the rain arrives. Walls are rim-lit pink with plum shadows. Sand darkens to wet honey and rain streaks fall in pale cyan.
- **Night clear.** An ultramarine sky full of stars, with maroon thatch and plum sand under a cyan moon rim. Every window is a `window_light` slot. The beacon burns, with an orange streak broken across the water. Fireflies are out.
- **Storm.** The sky is a violet zenith over a teal horizon, above a cobalt sea that doubles its foam. Pennants stream flat, ridges carry cyan rims, and the beacon is the only warm point. Lightning freezes everything lilac-white for two frames.
- **Sea fog.** A mint-pearl bank swallows the shore up to 3 m, with roofs, beacons and palms poking out above it. Lantern halos are orange discs inside the fog.
- **Snow.** Blue-lilac snow shadows, with full-chroma shutters and pennants. The sea is one band deeper.

---

# PART B: §HUD, "Woad Cloth"

The island is the star. The HUD is a set of lengths of woad-dyed sailcloth hemmed with saffron thread, the cloth islanders hang from a pole to mark the landing. Panels are deep indigo and calm so the world carries the colour. Accents are the same pigments the islanders paint with. The HUD is drawn at full window resolution, unquantised and with no dither, over the 240-px world. Crisp cloth over a crunchy island is the intended contrast.

Everything is plain `bevy_ui` `Node`, `Button` (`Interaction`), `Text`, `BackgroundColor` and `BorderColor`, using the Bevy default font and no image assets. This is a **re-skin and re-theme of the existing `src/ui.rs` + `src/ui/strip.rs`**. The interaction architecture, the component boundaries, the data-table approach and the layout carry over unchanged. What changes is tokens, decoration, labels, glyphs, tables and one new tide row.

## B1. Colour tokens (`ui.rs` consts)

There are three accents with fixed meanings. **Malachite** means selected, on or valid. **Madder** means attention, invalid or erase. **Saffron** means time, plus the decorative thread. Everything else is cloth.

| Const (old name) | Token | Hex | Use |
|---|---|---|---|
| `CLOTH` (`SLAB`) | cloth | `#1C1A5C` @ 92% | panel background |
| `DEEP` | cloth-deep | `#141244` @ 94% | tooltip, slider track, swatch well, name plate, collapsed tab |
| `TABLET` (`CAST`) | tablet | `#2A2878` | button at rest |
| `LIT` | tablet-lit | `#3A38A0` | hover; active fill |
| `PRESS` | tablet-pressed | `#221F66` | 80 ms press flash |
| `STITCH` (`SEAM`) | stitch | `#5A56C8` | 1-px panel and button borders, dividers, time ticks |
| `WEFT` (`DRIP`) | weft-dark | `#0A0828` @ 70% | 2-px drip shadow under panels, swatch inner border |
| `SHELL` (`CHALK`) | shell-text | `#FFF4DE` | primary text, slider knob |
| `DIM` | shell-dim | `#B8B0E8` | key tags, group names, glyphs at rest, hint descriptions, readouts |
| `MALACHITE` (`MOSS`) | malachite | `#2FE0A0` | active band and border, toggle fill, slider fill, glyph when active |
| `MADDER` (`SODIUM`) | madder | `#FF5A4E` | invalid hint line, invalid notch, erase hover border |
| `SAFFRON` (new) | saffron | `#FFC21A` | clock text, time-slider fill, selvedge thread, knots |
| `COCHINEAL` (new) | cochineal | `#E8399A` | selvedge second thread, "now showing" auto-weather band |
| `TIDE` (new) | tide | `#19E0D0` | tide gauge fill only |
| `DIS_TXT` / `DIS_BG` | disabled | `#6A6299` on `#241D5A` | unavailable controls |
| (tools.rs `Tint::Ok`) | ghost-ok | `#A8FFD8` @ 45% | world ghost, valid |
| (tools.rs `Tint::Bad`) | ghost-bad | `#FF5A4E` @ 55% | world ghost, invalid; erase target |
| (tools.rs `Tint::Pick`) | ghost-pick | `#FFC21A` @ 50% | Dye hover |

Contrast on `cloth`: shell-text 14.2:1, shell-dim 7.7:1, malachite 9.1:1, saffron 9.6:1, madder 5.0:1, tide 9.3:1. Shell-dim on tablet-lit is 4.6:1. Text is never drawn on a malachite, madder, saffron, cochineal or tide fill. Cochineal is never used for text.

## B2. Shape and motif

- **Corners.** Panels radius 0. Buttons, swatch cells and slider knobs radius 2 px.
- **Borders.** 1 px `stitch` on panels and buttons.
- **Selvedge** (replaces the formwork tie-holes; spawned by `deco()`). Across the top edge of each panel, inside the border, are two absolute full-width stripes: 2 px `saffron` at `top: 0` and 1 px `cochineal` at `top: 3`. This is the hem thread.
- **Knots.** Two 4×4 `saffron` squares hang from the selvedge at `top: 4`, inset 6 px from the left and right edges. They are the tie knots that hang the cloth.
- **Drip.** A 2-px `weft` strip directly under each panel's bottom edge, with no blur.
- **Active treatment.** `LIT` fill, 1-px `malachite` border and a 3-px `malachite` band across the button top (the existing `Band` child).
- **Invalid treatment.** A 1-px `madder` border plus a **notched** band: a row of three 4×3 `madder` squares with 4-px gaps across the top-left. This is a broken thread, so invalid differs from active in shape and luminance as well as hue. The invalid hint line in the HintsPanel gets this notch at its left edge, and the text is prefixed `! `.
- **Dividers.** 1-px `stitch` vertical lines between toolbar groups and between strip clusters, inset 6 px top and bottom.

## B3. Spacing, scale and type

Spacing, `UiScale` and the region anchors are unchanged from the fork:

- Spacing scale is 2/4/6/8/12/16. Screen margin 16, panel padding 8 (6 on the toolbar).
- `UiScale = clamp(min(round(h/720·4)/4, floor(w/1200·4)/4), 0.75, 2.0)`.
- The central 60% of the screen in both axes is never covered by panels, except where the corner panels (HintsPanel, WeatherPanel) overlap its corners by a few dozen pixels at 1280 px and below. One further exception: the variant strip (B4) may rise into the bottom of that band while a tool is active. The toolbar plus one strip row cannot fit in the bottom 20%.

Bevy default font only. Labels, buttons, plates and group names are UPPERCASE. Hint descriptions and tooltip second lines are sentence case. Line height is 1.25.

| Size | Use |
|---|---|
| 10 | key tags, group names, slider readouts, tide readout, footer keys |
| 12 | button labels, segment labels, hint lines, tooltip body |
| 14 | tool glyphs, name plate, tooltip title |
| 20 | clock |

## B4. Layout

```
+-------------------------------------------------------------------------------+
| [< UNDO][REDO >]                                       +======================+ |
| +====================+                                 | 14:20 DAY       [||] | |
| | HUT                |                                 | ====|==============  | |
| | DRAG  footprint    |                                 | TIDE ~~~|    RISING  | |
| | ...                |          (the cove)             | CLEAR OVERCAST RAIN  | |
| +====================+                                 | STORM SEA FOG  SNOW  | |
|                                                        | [x] AUTO     CYCLING | |
|                                                        +======================+ |
|        +==========================================================+           |
|        | HUT | WATTLE PLANK STONE | THATCH TURF HIDE BOAT | ...   |  strip    |
|        |     | ROOF [][][][][][]  WALL [][][][][]  TRIM [][][][][] |           |
|        +==========================================================+           |
|  +=========================================================================+  |
|  | BUILD                                   DRESS           LAND      EDIT  |  |
|  | [=#=][|x|][/^\][(^)][:::][_|^][TTT][\_/] [[ ]][/\]... [/+\]...  [(%)][>x<]| |
|  +=========================================================================+  |
+-------------------------------------------------------------------------------+
```

`=` marks the saffron and cochineal selvedge.

| Region | Anchor | Size at scale 1.0 |
|---|---|---|
| `ToolBar` | bottom-centre, 16 from bottom | 18 buttons 58×56 in 4 groups, ≈ 1127 × 84 |
| `VariantStrip` | 8 above ToolBar (bottom 110), centred | auto width, max 1000; 44 per row, 1 or 2 rows; Prop grid 88 + swatch row |
| `WeatherPanel` | top-right | 244 wide; ≈ 240 open (the B7 rows need it), 44 collapsed |
| `UndoRedo` | top-left | two 72×36 buttons, gap 4 |
| `HintsPanel` | top-left, 8 under UndoRedo | 272 wide, ≤ 7 lines; collapsed = 32×32 `?` tab |
| `Tooltip` | 12 above the hovered button, clamped | auto, max 260 |

Below 1180 logical px of width, buttons narrow to 52 and group gaps shrink to 6. The bar never wraps.

## B5. ToolBar

**ToolButton anatomy (58×56).** The key tag sits top-left (10 px `DIM`). The glyph is centred at y 14–30 (14 px; `DIM` at rest, `SHELL` on hover, `MALACHITE` when active). The label is bottom-centre (12 px `SHELL`). The column is justified to the end with a 2-px gap. Button height grows from 52 to 56 to fit the glyph.

Rust `Tool` variants keep their generic names. Only `label()`, the new `glyph()`, `tip()` and the group table change.

| Group | Key | `Tool` | Label | Glyph | Tooltip title / second line |
|---|---|---|---|---|---|
| **BUILD** | `1` | `Wall` | `DYKE` | `=#=` | DYKE [1]: Drag a dry-stone, turf or palisade dyke |
| | `2` | `Fence` | `W-BREAK` | `\|x\|` | WINDBREAK [2]: Drag wattle, driftwood, nets or reed |
| | `3` | `Block` | `HUT` | `/^\` | HUT [3]: Drag a footprint, lift it, click to build |
| | `4` | `Tower` | `ROUND` | `(^)` | ROUNDHOUSE / BEACON [4]: Drag a circle; raise it tall for a beacon |
| | `5` | `Path` | `TRACK` | `:::` | TRACK [5]: Drag a track; it cuts gaps through dykes |
| | `6` | `Stairs` | `STEPS` | `_\|^` | STEPS [6]: Drag from the ground up to a ledge |
| | `7` | `Deck` | `JETTY` | `TTT` | JETTY [7]: Drag a deck; over the sea canoes moor |
| | `8` | `Bridge` | `BRIDGE` | `\_/` | BRIDGE [8]: Drag across water or between ledges |
| **DRESS** | `9` | `Opening` | `OPENING` | `[ ]` | OPENING [9]: Click a wall: shutter, door, boat door |
| | `0` | `Roof` | `ROOF` | `/\` | ROOF [0]: Click inside a closed dyke ring |
| | `G` | `Tree` | `TREE` | `\|/` | TREE [G]: Click to plant |
| | `C` | `Clutter` | `PROP` | `.8.` | PROP [C]: Click to place a prop |
| | `P` | `Paint` | `PAINT` | `~*~` | PAINT [P]: Brush sea pinks, dune grass, wrack, pebbles |
| **LAND** | `R` | `Sculpt` | `RAISE` | `/+\` | RAISE / LOWER [R]: Drag to raise; Shift to lower |
| | `F` | `Flatten` | `FLATTEN` | `-=-` | FLATTEN [F]: Drag to level the ground |
| | `V` | `Water` | `LAGOON` | `~~~` | LAGOON [V]: Drag to carve tide pools |
| **EDIT** | `I` | `Style` | `DYE` | `(%)` | DYE [I]: Click to dye a structure; Shift+click picks |
| | `X` | `Erase` | `ERASE` | `>x<` | ERASE [X]: Click or drag to remove |

`GROUPS = ["BUILD", "DRESS", "LAND", "EDIT"]`. While `ToolStatus::drawing` is set, the toolbar does not dim; it stays exactly as it is.

## B6. VariantStrip

The strip is a `CLOTH` panel showing only the active tool's controls. It opens with a **name plate**: 14-px `SHELL` text on `DEEP` with a 3-px `malachite` band down its left edge, followed by a divider. Row A holds the form controls. Row B holds the swatch groups, each prefixed by a 10-px `DIM` label. A tool without colour controls has one row. Erase hides the strip.

The widgets are the existing ones:

- **Segment rows**: ≥ 32 tall, one selected.
- **Toggle**: a 12×12 box. When on it is filled `malachite` with a 4×4 `CLOTH` inner square.
- **Slider rows**: 96-px track. The fill is `malachite`, or `saffron` for time. The knob is 8×16 `SHELL`.
- **Swatch groups.**

**Swatch chip (changed).** The 28×28 cell is `DEEP`. It holds a 20×20 chip with a 1-px `WEFT` border, split horizontally: the top 13 px is the swatch hex (lit band), and the bottom 7 px is the swatch's **noon shadow** hex from `*_SHADES`. The player sees the hue-shifted shadow before committing. Selected swatches get a 2-px `malachite` cell border plus a 3-px `malachite` under-band. The tooltip gives the swatch name (`WOAD DYED`).

Controls that do not apply are shown disabled, not hidden. Field names are `ToolSettings` fields; new or renamed ones are marked †.

| Tool | Row A | Row B |
|---|---|---|
| **Dyke** (`Wall`) | `DRY STONE` `TURF` `PALISADE` (`wall_kind`†: `WallKind`) · `[ ] CREST` (`crest`†, was `planters`) · `[ ] TUMBLED` (`tumbled`†, was `ruined`) · `THICK` 0.3–1.2 M (`wall_thickness`) · `FOOTING` 0–2 M (`footing`†, was `plinth`) · `WEATHER` 0–1 (`weathering`†, was `overgrowth`) | `TRIM` ×5; enabled for `Palisade` only (painted stake tips). Dry-stone copestones and gateposts are natural stone. |
| **Windbreak** (`Fence`) | `WATTLE` `DRIFTWOOD` `NETS` `REED` (`fence`: `Wattle`, `Driftwood`, `NetLine`, `Reed`) | `TRIM` ×5; enabled for `NetLine` (floats) only. Driftwood stays bleached and unpainted. |
| **Hut** (`Block`) | `WATTLE` `PLANK` `STONE` (`material`) · `THATCH` `TURF` `HIDE` `BOAT` (`roof`) · `PITCH` 15–60° · `[ ] CARVED` (`carved`†, was `fins`) · `[ ] HANGINGS` (`hangings`†) · `STILTS` 0–3 M (`stilts`†; readout `AUTO` at 0) · `WEATHER` 0–1 | `ROOF` ×6 · `WALL` ×5 · `TRIM` ×5 |
| **Roundhouse** (`Tower`) | `WATTLE` `PLANK` `STONE` (disabled while the ghost is a beacon, which is always stone) · `THATCH` `TURF` `HIDE` `BOAT` (`BOAT` disabled: it falls back to thatch on round footprints; `TURF` and `HIDE` read as "open" on a beacon) · `[ ] CARVED` · `[ ] HANGINGS` · `WEATHER` 0–1 | `ROOF` ×6 · `WALL` ×5 · `TRIM` ×5 |
| **Track** (`Path`) | `SHELL` `PEBBLE` `FLAGS` `BOARDS` `STEPPING` (`path`: `Shell`, `Pebble`, `Flagstone`, `Boardwalk`, `Stepping`) · `WIDTH` 0.8–4 M | none |
| **Steps** (`Stairs`) | `STONE` `LOG` `LADDER` (`stairs`: `Stone`, `Log`, `RopeLadder`) | `TRIM` ×5; enabled for `RopeLadder` only (rungs) |
| **Jetty** (`Deck`) | none (deck height is set by Raising) | `TRIM` ×5 (mooring-post tips, canoe strakes) |
| **Bridge** | `ROPE` `LOG` `CAUSEWAY` (`bridge`; `ROPE` keeps the last railing) · `RAILING` `ROPE` `NET` `RAILS` `FLOATS` (`Rope { railing: 0..=3 }`, Rope only) · `HANGING` `NONE` `LANTERNS` `PENNANTS` (`hanging`, Rope only) | `TRIM` ×5 (floats, pennants, lantern posts) |
| **Opening** | `PLACE` `SHUTTER` `DOOR` `WIDE` (`opening`: `Window`, `Door`, `WideDoor`) | none |
| **Roof** | `THATCH` `TURF` `HIDE` `BOAT` · `PITCH` 15–60° | `ROOF` ×6 |
| **Tree** | `PINE` `PALM` `CYPRESS` `FIG` `SNAG` (`tree`: `SeaPine`, `Palm`, `Cypress`, `Fig`, `Snag`) | none |
| **Prop** (`Clutter`) | 14-cell grid, 7 per row, cells 72×36 (table below) | `TRIM` ×5; disabled for Firepit, Cairn, Rock and Woodpile, enabled for every other prop |
| **Paint** | `PINKS` `DUNE` `WRACK` `PEBBLES` `CLEAR` (`paint`: `SeaPinks`, `DuneGrass`, `Wrack`, `Pebbles`, `Clear`) · `BRUSH` 1–8 M | none |
| **Raise** (`Sculpt`) | `RAISE` `LOWER` · `RADIUS` 1–8 M · `STRENGTH` 0.1–1 | none |
| **Flatten** | `RADIUS` 1–8 M · `STRENGTH` 0.1–1 | none |
| **Lagoon** (`Water`) | `RADIUS` 1–8 M · `DEPTH` 0.3–1.5 M | none |
| **Dye** (`Style`) | `HUT` `WATTLE` `PLANK` `STONE` (`material`) · `DYKE` `DRY STONE` `TURF` `PALISADE` (`wall_kind`) · `[ ] CARVED` · `WEATHER` 0–1 | `ROOF` ×6 · `WALL` ×5 · `TRIM` ×5 |
| **Erase** | strip hidden | none |

**Prop grid** (`CLUTTER` table: kind, cell label, tooltip name). Row 1 is CANOE RACK FIREPIT CAIRN STONE POLE NETS. Row 2 is BASKETS LANTERN PENNANT ROCK BUSH POTS WOOD.

| Kind | Label | Tooltip |
|---|---|---|
| `Canoe` | `CANOE` | OUTRIGGER CANOE |
| `DryingRack` | `RACK` | FISH DRYING RACK |
| `Firepit` | `FIREPIT` | FIREPIT |
| `Cairn` | `CAIRN` | CAIRN |
| `StandingStone` | `STONE` | CARVED STANDING STONE |
| `CarvedPole` | `POLE` | CARVED POLE |
| `NetRack` | `NETS` | NET RACK |
| `Baskets` | `BASKETS` | BASKETS AND JARS |
| `LanternPost` | `LANTERN` | LANTERN POST |
| `Pennant` | `PENNANT` | PENNANT POLE |
| `Rock` | `ROCK` | ROCK |
| `Bush` | `BUSH` | GORSE AND FLAX BUSH |
| `CrabPots` | `POTS` | CRAB POTS |
| `Woodpile` | `WOOD` | DRIFTWOOD PILE |

**Name plate text** is the tool name (`DYKE`, `HUT`, `LAGOON`), with two live exceptions:

- **Roundhouse.** While Raising, the plate reads `ROUNDHOUSE`, and flips to `BEACON` the moment wall height reaches 2.5 × radius. The plate's left band turns `saffron` while it is a beacon (the text changes as well as the colour).
- **Dye.** After a Shift+click pick, the plate reads `DYE: HUT` (or `DYKE`, and so on). The relevant material segment is then enabled and the other is disabled.

## B7. WeatherPanel (weather + time + tide)

A `CLOTH` panel with selvedge, knots and drip, 244 wide. Its rows, top to bottom:

1. **Clock row.** `ClockLabel` `14:20` at 20 px `saffron`. A 10-px `DIM` phase word: `DAWN` (05–07), `DAY` (08–15), `GOLDEN` (16–17), `DUSK` (18–19) or `NIGHT` (20–04). A 32×32 `PauseButton` on the right shows `||` while running, or `>` with the active treatment while paused. Clicking the clock collapses or expands the rest of the panel.
2. **Time slider.** 228 wide. The fill is `saffron` from 00 to the current hour. `stitch` ticks at 06, 12 and 18, and `00` and `24` readouts at the ends. Dragging scrubs the hour and pauses time. `[` and `]` step one hour.
3. **Tide row** (new; read-only, follows the clock). A 10-px `DIM` label `TIDE`. A 120×6 `DEEP` gauge with a 1-px `stitch` border, filled `tide` to `(tide + 0.3) / 0.6`, with a 1×10 `SHELL` mark at the mean (50%). A 10-px `DIM` readout on the right: `RISING`, `FALLING`, `HIGH` (within 0.03 m of +0.3) or `LOW` (within 0.03 m of −0.3), followed by the signed height (`RISING +0.21 M`). The row is 16 tall and takes no input.
4. **Weather grid.** Six `WeatherButton`s, 74×32, 3 per row: `CLEAR` `OVERCAST` `RAIN` / `STORM` `SEA FOG` `SNOW`. The current held state has the active treatment. While Auto is on, the *current auto state* shows a 3-px `cochineal` top band on a `TABLET` fill: a "now showing" marker, not a selection.
5. **Auto row.** A `[x] AUTO CYCLE` toggle, with a 10-px `DIM` status on the right: `CYCLING` or `HELD`. Picking any weather turns Auto off and holds that state. Ticking Auto resumes the cycle from the held state.
6. **Footer.** `T PAUSE   [ ] HOUR` at 10 px `DIM`.

## B8. UndoRedo, HintsPanel and Tooltip

- **UndoRedo.** Two `TABLET` plaques, `< UNDO` and `REDO >`, 72×36, with `^Z` / `^Y` key tags. They are not on a cloth panel. When `History` is empty in that direction they show the disabled treatment.
- **HintsPanel.** A `CLOTH` panel, 272 wide, under UndoRedo:
  - The first line is the name plate (12 px `SHELL`).
  - Each `HintLine` is a key chip (10 px `SHELL` uppercase on `DEEP`, padding 2/4) plus a 12-px `DIM` sentence-case description. There are at most 6 hint lines.
  - The invalid line (`ids.invalid`) appears above the list in `MADDER` with the notch and a `! ` prefix.
  - `H` or the `x` (32×32, top-right) collapses the panel to a `?` tab. It never auto-hides.
- **Tooltip.** Appears after a 450 ms rest. `DEEP` background, 1-px `stitch` border, padding 6. The title is 14 px `SHELL` with the key in brackets, and the second line is 12 px `DIM`. It hides on press or leave.

**Hint table** (`strip::hints`):

| Tool | Lines (chip: description) |
|---|---|
| Dyke | DRAG: Lay a dyke; click adds points · CLICK: On the last point to end · SCROLL: Dyke height · SHIFT: Snap 45 deg, close the ring · ESC: Cancel |
| Windbreak | DRAG: Lay a windbreak · CLICK: On the last point to end · SCROLL: Windbreak kind · SHIFT: Snap 45 deg · ESC: Cancel |
| Hut | DRAG: Footprint · SCROLL: Rotate the rectangle · SHIFT: Force a square · MOVE: Height, after release · CLICK: Build · ESC: Cancel |
| Roundhouse | DRAG: Circle footprint · MOVE: Height; tall makes a beacon · SCROLL: Roof style · CLICK: Build · ESC: Cancel |
| Track | DRAG: Lay a track; click adds points · CLICK: On the last point to end · SCROLL: Track width · SHIFT: Snap 45 deg · ESC: Cancel |
| Steps | DRAG: From the ground to a ledge · SCROLL: Width · ENDS: Snap to dyke tops, jetties, floors · ESC: Cancel |
| Jetty | DRAG: Footprint · MOVE: Deck height, after release · CLICK: Build · ESC: Cancel |
| Bridge | DRAG: From anchor A to B · SCROLL: Width · ENDS: Snap to dyke tops and jetties · ESC: Cancel |
| Opening | CLICK: Place the chosen opening · CLICK: On an opening cycles it · SHIFT: Click removes an opening |
| Roof | CLICK: Roof the hovered dyke ring · SCROLL: Pitch · SHIFT: Scroll changes roof style |
| Tree | CLICK: Plant · SCROLL: Rotate · SHIFT: Scroll scales 0.7 to 1.3 |
| Prop | CLICK: Place the prop · TAB: Next prop · SCROLL: Rotate · SHIFT: Scroll scales 0.7 to 1.3 |
| Paint | DRAG: Brush the layer · SCROLL: Brush size · SHIFT: Erase |
| Raise | DRAG: Raise the ground · SCROLL: Radius · SHIFT: Lower |
| Flatten | DRAG: Level the ground · SCROLL: Radius |
| Lagoon | DRAG: Carve a tide pool · SCROLL: Radius |
| Dye | CLICK: Dye a structure with the strip · SHIFT: Click picks its colours · TAB: Next material |
| Erase | CLICK: Remove a structure or prop · DRAG: Remove many |

## B9. States and transitions

| State | Visual |
|---|---|
| Rest | `TABLET` fill, 1-px `stitch` border, glyph `DIM`, label `SHELL` |
| Hover | `LIT` fill; glyph turns `SHELL`. On Erase targets in the world the ghost is `ghost-bad`. Hovering the Erase button itself gives a `madder` border |
| Active / selected | `LIT` fill, `malachite` border, 3-px `malachite` top band, glyph `MALACHITE` |
| Pressed | `PRESS` for 80 ms (`Flash`), then rest or active |
| Disabled | `DIS_BG` fill, `DIS_TXT` text, `stitch` border, no hover response |
| Invalid | Hint line in `MADDER` with notch and `! ` prefix; world ghost `ghost-bad`; click does nothing |
| Now showing (auto weather) | `TABLET` fill + 3-px `cochineal` band |

A `ToolStatus` (`drawing`, `invalid`) drives the HUD side of `DrawState`:

| DrawState | World | HUD |
|---|---|---|
| Idle | nothing | hints list the tool's gestures |
| Hover | single ghost in `ghost-ok` | none |
| Drawing | live preview in `ghost-ok` | the strip reflects scroll changes live (segments and knobs move) |
| Raising | footprint fixed; mouse sets height | hints switch to MOVE / SCROLL / SHIFT+SCROLL / CLICK. The Roundhouse plate flips to `BEACON` live |
| Invalid | ghost `ghost-bad` | invalid line shows one reason |

The invalid reasons (consts in `tools.rs`, renamed) are `OUTSIDE THE ISLAND`, `NEEDS A CLOSED DYKE RING`, `NEEDS A HUT OR DYKE`, `TOO SHORT`, `TOO STEEP` and `NEEDS TWO ANCHORS`.

**Transitions.**

- A tool change, by key or click, rebuilds the strip and hints in the same frame (`rebuild_strip`, `rebuild_hints`).
- Scroll-driven variant changes update the strip immediately.
- Picking a weather turns Auto off.
- A Dye Shift+click copies the structure's style into `ToolSettings` (`copy_style`), and a plain click applies it (`apply_style`).
- The only HUD animation is the 80-ms press flash. Nothing else eases, fades or pulses.
- Pointer capture is unchanged: any HUD node, including panel padding and the `?` tab, blocks world input.
- `F12` hides the HUD for the screenshot frame.

**Keyboard.**

- Tool keys are as in B5.
- `Tab` cycles the first segment row and `Shift+Tab` the second. On Prop, `Tab` cycles the props; on Dye it cycles the material.
- `-` / `=` nudge the last-touched slider.
- Global keys: `Ctrl+Z` / `Ctrl+Y`, `T`, `[` `]`, `H`, `F12`, `Esc`. Camera keys are unchanged: `WASD`, `Q`/`E`, `PgUp`/`PgDn`.

## B10. Components (matching `ui.rs` / `strip.rs`)

| Component / item | Role | Change |
|---|---|---|
| `Action` (`Tool`, `Seg`, `Toggle`, `Swatch`, `Clutter`, `Weather`, `Auto`, `Pause`, `Undo`, `Redo`, `Hints`, `Collapse`) | button behaviour | unchanged |
| `Cond`, `Btn`, `Flash`, `Band`, `Lbl`, `Tog`, `Panel`, `Seg`, `Tip`, `Slider`, `TimeTrack` | as now | `Lbl` also tags the glyph text (`Glyph` marker added so `paint` colours it separately) |
| `Ids` | entity handles | adds `tide_fill: Entity`, `tide_text: Entity` |
| `HintsVisible`, `ClockOpen`, `LastSlider` | resources | unchanged |
| `deco()` | panel decoration | spawns the selvedge (2), knots (2) and drip (1) in place of the tie-holes |
| `paint()` | per-frame button colours | uses the new consts and adds the invalid notch and the cochineal "now showing" band |
| `sync_text()` | clock, phase, status | adds the tide readout and gauge (reads `Clock` → tide via the existing tide function in `env.rs`/`weather.rs`) |
| `strip::rows()`, `CLUTTER`, `tip()`, `tool_group()`, `GROUPS`, `hints()` | data tables | rewritten per B5–B8 |
| `Tool::label()`, `Tool::glyph()` (new), `Tool::hotkey()` | tool metadata | labels per B5; hotkeys unchanged |
| `C::Sw(slot, label, en)` | swatch group | slot 0 `ROOF`, 1 `WALL`, 2 `TRIM`; chips read `*_SWATCHES` + `*_SHADES` |
| `ToolSettings` | variant state | adds `wall_kind`, `crest`, `tumbled`, `footing`, `carved`, `hangings`, `stilts`, `weathering`; drops `fins`, `planters`, `ruined`, `plinth`, `overgrowth` |

`ToolSettings` defaults:

- `wall_kind: DryStone`, `material: Wattle`, `roof: Thatch`, `crest: true`, `carved: false`, `hangings: true`, `stilts: 0.0`.
- `weathering: 0.25`, `footing: 0.0`, `fence: Wattle`, `path: Shell`, `stairs: Stone`.
- `bridge: Rope { railing: 0 }`, `tree: Palm`, `clutter: Canoe`, `paint: SeaPinks`.
- `colors: { roof: 0, wall: 0, trim: 0 }`.

**Ghost colours** stay gizmo lines in `tools.rs` (`Tint::{Ok, Bad, Pick}` → `ghost-ok`, `ghost-bad`, `ghost-pick`). They are not world-quantised.

## B11. Accessibility

- Every enabled text reaches at least 4.5:1 contrast (B1).
- Every hit target is at least 32×32. Swatch hit nodes are 32×32 with the 28-px cell centred.
- Selection is never colour alone: the band and border, the filled toggle with its inner square, the swatch under-band, `||` versus `>`, and the invalid notch with its `!` all carry shape.
- Malachite and madder differ strongly in luminance as well as hue, which helps deuteranopes.
- Every control can be reached from the keyboard.
- Hints never auto-hide, and nothing flashes faster than the 80-ms press.

## B12. What the HUD does not do

There is no minimap, no counters, no notifications, no confirmation dialogs, no modal panels and no menu chrome during play.

## B13. Line budget

The current total is about 1080 lines (ui.rs 791 + strip.rs 286). The re-skin should land at about **1200**:

| Piece | Δ lines |
|---|---|
| Token consts rename + 3 new accents | +4 |
| `deco()` selvedge, knots, drip | +4 |
| Glyph text per tool button + `Tool::glyph()` | +25 |
| Split swatch chips (`*_SHADES`) | +8 |
| Invalid notch + cochineal now-showing band in `paint()` | +15 |
| Tide row (spawn + sync) | +35 |
| Name-plate live text (Beacon / Dye target) | +15 |
| `strip.rs` tables rewritten (same shape; Dye gets two material rows) | +10 |
| **Total** | **≈ +116 → ≈ 1195**, hard cap 1250 |

Keep every per-tool difference in the `strip.rs` data tables, not in branching UI code.
