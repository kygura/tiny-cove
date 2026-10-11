# GREYWILD — interface design brief

The glade is the star. The HUD looks like municipal wayfinding cast into the
landscape: a few flat concrete slabs at the edges of the screen, stamped with
heavy uppercase lettering, the way a park sign or a platform number is cast
into a retaining wall. The slabs are quiet. They do not animate for their own
sake, they never cover the centre of the screen, and they never compete with
the moss, the water or the sodium lamps in the valley.

The world renders at 240 px tall and is upscaled with nearest-neighbour. The
HUD is drawn at full window resolution on top of it, so it stays sharp. That
contrast between crunchy world and crisp HUD is intended: the HUD is the
signage, the world is the place.

Everything here is buildable from plain `bevy_ui` `Node`, `Button`, `Text`
and `BackgroundColor` / `BorderColor`, with the Bevy default font and no
image assets. The target is roughly 600–900 lines of Rust (budget in §7).

## 1. Visual language

### 1.1 Colour tokens

Two accents only. **Moss** means "selected / on / valid". **Sodium** means
"time, attention, or something is wrong". Everything else is concrete.

| Token | Hex | Use |
|---|---|---|
| `slab` | `#2F302D` at 92 % alpha | panel background (toolbar, strip, weather, hints) |
| `slab-deep` | `#262724` at 94 % | tooltip, slider track, swatch well, collapsed tabs |
| `cast` | `#40413D` | button background at rest |
| `cast-lit` | `#4E4F4A` | hovered button; active button fill |
| `cast-pressed` | `#353632` | button while pressed (80 ms flash) |
| `seam` | `#5A5B56` | 1 px panel and button borders, group dividers |
| `seam-dark` | `#1B1C1A` at 70 % | 2 px drop "drip" under panels, formwork tie-holes |
| `chalk` | `#E6E3DA` | all primary text and labels, slider knob |
| `chalk-dim` | `#A4A29A` | secondary text: key tags, group names, hint descriptions |
| `moss` | `#8DB360` | active band and border, toggle fill, slider fill, valid ghost tint |
| `sodium` | `#E8A33C` | clock text, time-slider fill, invalid hint line, erase hover |
| `disabled-text` | `#6E6F69` on `cast` `#3A3B37` | unavailable buttons (undo with empty history, Pitched on Tower) |
| `ghost-ok` | `#DDE8CF` at 45 % | world ghost, valid placement |
| `ghost-bad` | `#E8A33C` at 55 % | world ghost, invalid placement; erase target highlight |
| `ghost-pick` | `#8DB360` at 50 % | Style tool hover highlight |

Contrast against `slab`: `chalk` ≈ 11:1, `chalk-dim` ≈ 5.6:1, `moss` ≈ 6.4:1,
`sodium` ≈ 7:1. Text is never drawn on `moss` or `sodium` fills.

### 1.2 Shape

- **Corners:** square. Panels and slabs radius 0. Buttons, swatches and
  slider knobs radius 2 px, just enough to read as cast edges rather than
  pixels. Nothing rounder anywhere.
- **Borders:** 1 px `seam` on panels and buttons. The active button gets a
  1 px `moss` border plus a **3 px `moss` band** across its top edge (an
  absolutely positioned child node), like the coloured line band on a
  transit sign.
- **Drip shadow:** each panel has a 2 px `seam-dark` node directly under its
  bottom edge, no blur. Reads as the shadow line of a slab standing proud of
  the wall.
- **Tie-holes:** each panel carries two 3×3 `seam-dark` squares, inset 5 px
  from its top-left and top-right corners — the formwork tie marks of
  board-formed concrete. Decorative, absolute, not interactive. Buttons do
  not get them.
- **Dividers:** 1 px `seam` vertical lines between toolbar groups and
  between control clusters in the variant strip, inset 6 px top and bottom.

### 1.3 Spacing

Scale 2 / 4 / 6 / 8 / 12 / 16 px. Screen margin 16. Panel padding 8 (6 on
the toolbar). Gap between buttons inside a group 2. Gap between groups 12
(including the divider). Gap between label and control in a slider row 6.

### 1.4 Type

Bevy default font only (the bundled monospace). One weight exists, so the
"heavy" signage feel comes from uppercase, size, and high-contrast `chalk`
on dark slab; never fake bold with double-drawn text.

| Size | Use |
|---|---|
| 10 | key tags on buttons, group names on the toolbar, slider value readouts |
| 12 | button labels, variant labels, hint lines, tooltip body |
| 14 | tool name plate at the left end of the variant strip, tooltip title |
| 20 | clock |

Labels, buttons, plates and group names are UPPERCASE. Hint descriptions
and the second tooltip line are sentence case, because long uppercase runs
read poorly at 12 px. Line height 1.25.

### 1.5 Glyphs

No icons, no image assets. Every button carries an ASCII label. Where a
shape helps, it is built from nodes:

- **Toggle box:** 12×12 node with 1 px `chalk-dim` border; filled `moss`
  with a 4×4 `slab` square centred when on. The label sits to its right.
- **Slider:** 4 px tall `slab-deep` track with 1 px `seam` border; filled
  portion `moss` (or `sodium` for the time slider); knob a 8×16 `chalk`
  rectangle, radius 2. Value readout to the right at 10 px `chalk-dim`
  (`1.2 M`, `0.40`, `3`).
- **Time ticks:** three 1×6 `seam` nodes under the time track at 06, 12, 18.
- **Pause / play:** `||` and `>` as text. Undo / redo: `< UNDO`, `REDO >`.
- **Swatch:** 28×28 cell, `slab-deep`, holding a 20×20 colour square with a
  1 px `seam-dark` border. Selected: cell border 2 px `moss` and a 3 px
  `moss` band under the cell (shape, not just colour).

## 2. Layout and scale

Reference window 1280×720, logical px.

```
UiScale = clamp( min( round(h / 720 * 4) / 4 ,  floor(w / 1200 * 4) / 4 ),  0.75, 2.0 )
```

Quarter steps keep the default font crisp. The width term stops the toolbar
from overflowing on narrow or portrait windows. Panels anchor to window
edges with the 16 px margin and know nothing about the 240 px render
target.

```
+--------------------------------------------------------------------------+
| [< UNDO][REDO >]                                     +-------------------+ |
| +--------------------+                               | 14:20         ||  | |
| | WALL               |                               | ====|============ | |
| | DRAG   lay a wall  |                               | CLEAR  OVERCAST.. | |
| | ...                |        (the glade)            | [x] AUTO          | |
| +--------------------+                               +-------------------+ |
|                                                                            |
|            +--------------------------------------------------+            |
|            | WALL | CONCRETE EARTH TIMBER | []PLANTERS ... |   |  strip     |
|            +--------------------------------------------------+            |
|  +---------------------------------------------------------------------+  |
|  | BUILD                          DECORATE        TERRAIN    EDIT        |  |
|  | [WALL][FENCE][BLOCK]...         [WIN/DR]...    [RAISE]... [STYLE][X]  |  |
|  +---------------------------------------------------------------------+  |
+--------------------------------------------------------------------------+
```

| Region | Anchor | Size at scale 1.0 |
|---|---|---|
| `ToolBar` | bottom-centre, 16 from bottom | 18 buttons 58×52 in 4 groups; ≈ 1127 × 78 |
| `VariantStrip` | 8 above ToolBar, centred | auto width (max 1000); 44 per row, 1 or 2 rows; Clutter 88 |
| `WeatherPanel` | top-right | 244 wide, auto height (≈ 172 open, 44 collapsed) |
| `UndoRedo` | top-left | two 72×36 buttons, gap 4 |
| `HintsPanel` | top-left, 8 under UndoRedo | 272 wide, ≤ 7 lines; collapsed = 32×32 `?` tab |
| `Tooltip` | 12 above the hovered button, clamped inside the window | auto, max 260 wide |

The centre of the screen (the middle 60 % horizontally between the top
panels and the strip) is never covered by any HUD node.

### 2.1 ToolBar

A single `slab` with tie-holes and drip shadow. Four groups, each headed by
its group name at 10 px `chalk-dim` (row height 14), separated by `seam`
dividers. Below 1180 logical px of available width, buttons narrow to 52
and group gaps shrink to 6; the bar never wraps.

**ToolButton anatomy (58×52):** key tag top-left at 10 px `chalk-dim`
(`1`, `G`, `X`), label bottom-centre at 12 px `chalk`. Active: `cast-lit`
fill, `moss` border, 3 px `moss` top band. Hover: `cast-lit` fill only.

| Group | Key | Label | Tool | Tooltip second line |
|---|---|---|---|---|
| **BUILD** | `1` | `WALL` | Wall | Drag a line of cast concrete, earth or timber |
| | `2` | `FENCE` | Fence | Drag a railing, balustrade, gabion or hedge |
| | `3` | `BLOCK` | Block | Drag a footprint, lift it, click to cast |
| | `4` | `TOWER` | Tower | Drag a circle for a silo tower |
| | `5` | `PATH` | Path | Drag a path; it cuts portals through walls |
| | `6` | `STAIR` | Stairs | Drag from ground up to a wall top |
| | `7` | `DECK` | Deck (platform) | Drag a concrete deck on pilotis |
| | `8` | `BRIDGE` | Bridge | Drag across water or between walls |
| **DECORATE** | `9` | `WIN/DR` | Window / Door | Click a wall: slot, then door, then portal |
| | `0` | `ROOF` | Roof | Click inside a closed wall ring |
| | `G` | `TREE` | Tree | Click to plant |
| | `C` | `CLUTTER` | Clutter | Click to place a prop |
| | `P` | `PAINT` | Paint | Brush wildflowers, ferns, moss or gravel |
| **TERRAIN** | `R` | `RAISE` | Raise / Lower | Drag to raise; Shift to lower |
| | `F` | `FLAT` | Flatten | Drag to level the ground |
| | `V` | `WATER` | Water | Drag to carve ponds and streams |
| **EDIT** | `I` | `STYLE` | Style | Click a structure to copy its style |
| | `X` | `ERASE` | Erase | Click or drag to remove |

### 2.2 VariantStrip

A `slab` panel, centred above the toolbar, showing only the active tool's
controls. It opens with a **tool name plate**: a 14 px `chalk` uppercase
label (`WALL`, `BLOCK`, `WATER`) on `slab-deep`, with a 3 px `moss` band
down its left edge, followed by a divider. Then control clusters left to
right, separated by dividers.

Row A holds form controls (material, kind, toggles, sliders). Row B, when
the tool has colours, holds swatch groups, each prefixed with a 10 px
`chalk-dim` label (`ROOF`, `CONCRETE`, `ACCENT`). A tool with no colour
controls is one row. Erase hides the strip entirely.

Control widgets:

- **Segmented row:** buttons ≥ 32 tall, width = label + 16, 12 px labels,
  joined with 2 px gaps; the selected segment uses the active treatment
  (moss band + border). Exactly one selected.
- **Toggle:** toggle box + 12 px label, whole row is the hit target (≥ 32
  tall).
- **Slider row:** 12 px label, 96 px track, 10 px value readout. Click or
  drag on the track; `-` / `=` nudge the focused slider by one step.
- **Swatch group:** 28×28 swatches, gap 2.

Controls that do not apply to the current selection are shown
`disabled`, not hidden, so the strip does not jump in width while the
player flips segments (e.g. `PLANTERS` on Block is disabled unless the roof
is `GREEN`).

| Tool | Row A | Row B |
|---|---|---|
| Wall | `CONCRETE EARTH TIMBER` · `[ ] PLANTERS` (planter parapet) · `[ ] RUINED` · `THICK` 0.3–1.2 m · `PLINTH` 0–2 m · `OVERGROWTH` 0–1 | `CONCRETE` ×5 · `ACCENT` ×5 |
| Fence | `RAILING BALUSTRADE GABION HEDGE` | `ACCENT` ×5 (Railing only; disabled otherwise) |
| Block | `CONCRETE EARTH TIMBER` · `PITCHED SAWTOOTH SHELL GREEN` · `PITCH` 15–60° · `[ ] FINS` · `[ ] PLANTERS` (Green only) · `PLINTH` 0–2 m · `OVERGROWTH` 0–1 | `ROOF` ×6 · `CONCRETE` ×5 · `ACCENT` ×5 |
| Tower | `CONCRETE EARTH TIMBER` · `PITCHED SAWTOOTH SHELL GREEN` (Pitched and Sawtooth disabled) · `[ ] PLANTERS` (Green only) · `PLINTH` 0–2 m · `OVERGROWTH` 0–1 | `ROOF` ×6 · `CONCRETE` ×5 · `ACCENT` ×5 |
| Path | `GRAVEL PAVERS SLABS POURED STEPPING` · `WIDTH` 0.8–4 m | — |
| Stairs | `STEEL CONCRETE LADDER` | `ACCENT` ×5 (steel parts and railing) |
| Deck | `PLINTH` 0–2 m · `OVERGROWTH` 0–1 | `CONCRETE` ×5 · `ACCENT` ×5 |
| Bridge | `CABLE ARCH GIRDER` · `RAILING 1 2 3 4` (Cable only) · `HANGING NONE LAMPS VINES` (Cable only) | `CONCRETE` ×5 (Arch) · `ACCENT` ×5 (Cable, Girder) |
| Window / Door | `PLACE: SLOT DOOR PORTAL` — what a click on bare wall creates; a click on an existing opening still upgrades it one step | — |
| Roof | `PITCHED SAWTOOTH SHELL GREEN` · `PITCH` 15–60° · `[ ] PLANTERS` (Green only) | `ROOF` ×6 |
| Tree | `OAK PINE BIRCH FRUIT FERN` | — |
| Clutter | 14-cell grid, 7 per row, cells 72×36, 12 px labels (table below) | — |
| Paint | `FLOWERS FERNS MOSS GRAVEL ERASE` · `BRUSH` 1–8 m | — |
| Raise / Lower | `RAISE LOWER` · `RADIUS` 1–8 m · `STRENGTH` 0.1–1 | — |
| Flatten | `RADIUS` 1–8 m · `STRENGTH` 0.1–1 | — |
| Water | `RADIUS` 1–8 m · `DEPTH` 0.3–1.5 m | — |
| Style | read-out of the copied style: material label, roof style label, `FINS` toggle state, overgrowth readout; all editable in place | `ROOF` · `CONCRETE` · `ACCENT` (the copied colours, editable) |
| Erase | strip hidden | — |

Clutter cell labels (row 1 then row 2):

| Kind | Label | Kind | Label |
|---|---|---|---|
| RainBarrel | `BARREL` | LampPost | `LAMP` |
| Planter | `PLANTER` | WindTurbine | `TURBINE` |
| Compost | `COMPOST` | WashingLine | `WASHING` |
| Cistern | `CISTERN` | LogPile | `LOGS` |
| Wheelbarrow | `BARROW` | Rock | `ROCK` |
| Bench | `BENCH` | Bush | `BUSH` |
| Beehive | `HIVE` | Solar | `SOLAR` |

Row 1: BARREL PLANTER COMPOST CISTERN BARROW BENCH LAMP. Row 2: TURBINE
WASHING LOGS ROCK BUSH HIVE SOLAR. Each cell's tooltip gives the full name
(`RAIN BARREL`, `WIND TURBINE`, `SOLAR ARRAY`, `SODIUM LAMP POST`).

### 2.3 Palettes

These are the authoritative values for `core/src/palette.rs`; the HUD reads
them from there, never duplicates them. Swatch index order is as listed.

**Roof (6)**

| # | Name | Hex |
|---|---|---|
| 0 | Sedum | `#6A8A3E` |
| 1 | Dry sedum | `#9A8F4E` |
| 2 | Verdigris | `#5FA08C` |
| 3 | Zinc | `#8C9499` |
| 4 | Concrete | `#A3A29C` |
| 5 | Bitumen | `#3E403F` |

**Concrete (5)**

| # | Name | Hex |
|---|---|---|
| 0 | Pale board-form | `#C9C6BD` |
| 1 | Mid grey | `#A09F99` |
| 2 | Sand | `#C4B396` |
| 3 | Charcoal | `#5A5B57` |
| 4 | Lime white | `#E4E1D8` |

**Accent (5)** — window frames, railings, fins, doors, steel stairs

| # | Name | Hex |
|---|---|---|
| 0 | Corten rust | `#9C4A26` |
| 1 | Oxidised copper | `#4F8F7E` |
| 2 | Black steel | `#2B2D2E` |
| 3 | Safety yellow | `#E2B227` |
| 4 | Terracotta | `#B9603F` |

Defaults: roof Sedum, concrete Pale board-form, accent Corten rust.

### 2.4 WeatherPanel

A `slab` with tie-holes, top-right. Rows, top to bottom:

1. **Clock row.** `ClockLabel` `14:20` at 20 px `sodium`, then a 10 px
   `chalk-dim` phase word (`DAWN DAY DUSK NIGHT`), then a 32×32
   `PauseButton` right-aligned showing `||` (running) or `>` (paused, with
   the active treatment so the paused state is visible at a glance).
   Clicking the clock text collapses / expands the rest of the panel.
2. **Time slider.** Full width (228), track filled `sodium` from 00:00 to
   the current hour, ticks at 06, 12, 18, readouts `00` and `24` at the
   ends in 10 px `chalk-dim`. Dragging scrubs the hour and pauses time;
   `[` / `]` step one hour.
3. **Weather grid.** Six `WeatherButton`s, 3 per row, 74×32 each, labels
   `CLEAR OVERCAST RAIN` / `STORM FOG SNOW`. The current state uses the
   active treatment. While Auto is on, the button for the *current* auto
   state shows a `moss` band but a `cast` fill (a "now showing" marker, not a
   selection).
4. **Auto row.** Full-width toggle `[x] AUTO CYCLE`. To its right a 10 px
   `chalk-dim` status: `CYCLING` or `HELD`. Picking any weather button turns
   Auto off and holds that state; ticking Auto resumes the smooth cycle from
   the held state.

### 2.5 UndoRedo

Two `cast` buttons, `< UNDO` and `REDO >`, 72×36, 12 px, each with a 10 px
`chalk-dim` key tag above-right (`^Z`, `^Y`). They are not on a slab, so they
read as two small plaques. Disabled treatment when the history has nothing
to undo or redo.

### 2.6 HintsPanel

A `slab` under UndoRedo, 272 wide. Each `HintLine` is a key chip (10 px
`chalk` uppercase on `slab-deep`, padding 2/4, e.g. `DRAG`, `SCROLL`,
`SHIFT`, `ESC`) followed by a 12 px `chalk-dim` description in sentence
case. At most 7 lines. The first line is the tool name plate in 12 px
`chalk` (`WALL`). `H` or the `x` in the panel's top-right corner collapses
it to a 32×32 `slab-deep` tab labelled `?`; the tab expands it again. The
panel never auto-hides.

## 3. Interaction and state

A `DrawState` resource (`Idle`, `Hover`, `Drawing`, `Raising`, `Invalid`)
drives both the world ghost and the HUD.

| State | World | HUD |
|---|---|---|
| Idle | nothing | HintsPanel lists the active tool's gestures |
| Hover | single dab / prop / first point ghost in `ghost-ok` | the matching hint line's description turns `chalk` |
| Drawing | live polyline, rectangle, circle or stroke preview in `ghost-ok` | only in-stroke gestures listed; strip reflects scroll changes live (segment selection and slider knob move) |
| Raising | footprint fixed; mouse up/down sets height; scroll sets pitch | hints: `MOVE` height · `SCROLL` pitch · `SHIFT+SCROLL` roof · `CLICK` cast |
| Invalid | ghost tinted `ghost-bad`; click does nothing | first hint line in `sodium` with a reason: `OUTSIDE THE VALLEY`, `NEEDS A CLOSED WALL RING`, `NEEDS A WALL`, `TOO SHORT`, `TOO STEEP`, `NEEDS TWO ANCHORS` |

### 3.1 Per-tool gestures (and their hint lines)

- **Wall, Fence, Path:** press starts a polyline, drag or click adds
  points, click on the last point ends. Scroll = height (Wall), width
  (Path), kind (Fence). Shift snaps to 45° and closes the ring near the
  start. A path drawn through a wall shows the portal in the ghost.
- **Block, Tower:** drag footprint, release, enter Raising, click casts.
  Scroll during the footprint rotates the rectangle; Shift forces square
  or circle. Very low raises show the pilotis canopy in the ghost.
- **Deck:** drag footprint, release, Raising sets deck height, click casts.
- **Stairs, Bridge:** drag A to B. Scroll = width. Shift snaps end heights
  to the nearest wall top or deck. A bridge whose ends are not both on
  ground, wall or deck is Invalid (`NEEDS TWO ANCHORS`).
- **Roof:** a hovered closed wall ring highlights in `ghost-ok`; click
  roofs it. Scroll = pitch, Shift+scroll = roof style. No ring under the
  cursor is Invalid (`NEEDS A CLOSED WALL RING`).
- **Window / Door:** the hovered wall segment highlights; click places the
  strip's chosen opening; clicking an existing opening upgrades it
  (slot → door → portal); Shift+click removes it.
- **Tree, Clutter:** click places. Scroll rotates; Shift+scroll scales
  0.7–1.3. Scroll while idle over the Clutter grid is just zoom (the grid
  is picked by clicking or by Tab).
- **Paint, Raise/Lower, Flatten, Water:** drag dabs. Scroll = radius /
  brush. Shift inverts (Lower; Paint erases).
- **Style:** hovered structure highlights `ghost-pick`; click copies
  material, roof style, fins, overgrowth and the three colours into
  `ToolSettings` and the strip shows them.
- **Erase:** hovered structure or prop highlights `ghost-bad`; click or
  drag removes.
- Esc or an RMB tap cancels any state back to Idle. RMB drag always orbits.

### 3.2 Keyboard

Tools: `1` Wall · `2` Fence · `3` Block · `4` Tower · `5` Path · `6` Stairs ·
`7` Deck · `8` Bridge · `9` Window/Door · `0` Roof · `G` Tree · `C` Clutter ·
`P` Paint · `R` Raise/Lower · `F` Flatten · `V` Water · `I` Style · `X` Erase.

Strip: `Tab` cycles the first segmented row (material, kind, roof, weather
for none); `Shift+Tab` cycles the second segmented row where one exists;
`-` / `=` nudge the last-touched slider.

Global: `Ctrl+Z` / `Ctrl+Y` undo / redo · `T` pause time · `[` `]` scrub
one hour · `H` hints · `F12` screenshot · `Esc` cancel. Camera keys stay
camera: `WASD` pan, `Q` / `E` orbit. None of the tool keys collide with
them.

Every key appears somewhere on screen: tool keys as tags on the buttons,
undo/redo keys on the plaques, `T` `[` `]` as a 10 px `chalk-dim` line at
the bottom of the WeatherPanel (`T PAUSE   [ ] HOUR`), the rest in hints.

### 3.3 Feedback details

- **Tooltips:** after the pointer rests 450 ms on a button. `slab-deep`,
  1 px `seam`, padding 6. Title line 14 px `chalk` with key in brackets
  (`WALL  [1]`), second line 12 px `chalk-dim` from the table in §2.1 (or
  the full clutter name, or a swatch's colour name). Hidden on press or
  leave.
- **Press flash:** `cast-pressed` for 80 ms, then the resting / active
  treatment. No other HUD animation; no easing, no fades, no pulsing.
- **Scroll-driven changes** while drawing update the strip immediately,
  so the strip always tells the truth about what the next click will do.
- **Pointer capture:** while the pointer is over any HUD node (including
  the strip's padding and the collapsed hints tab), world input is blocked:
  no ghost, no dabs, no zoom.
- **Undo / redo** become `disabled` when `History` has nothing in that
  direction.

## 4. Components

| Component | Reads | Writes |
|---|---|---|
| `ToolBar` → `ToolGroup` → `ToolButton` | `ActiveTool` | `ActiveTool` |
| `VariantStrip` → `NamePlate`, `SegmentRow` → `SegmentButton`, `Toggle`, `SliderRow`, `SwatchGroup` → `Swatch`, `ClutterGrid` | `ActiveTool`, `ToolSettings` | `ToolSettings` |
| `WeatherPanel` → `ClockLabel`, `PauseButton`, `TimeSlider`, `WeatherButton`, `AutoToggle` | `Environment` | `Environment` |
| `UndoRedo` | `History` | `History` |
| `HintsPanel` → `HintLine` | `ActiveTool`, `DrawState`, `HintsVisible` | `HintsVisible` |
| `Tooltip` | hovered `TooltipText` | — |
| `GhostPreview` (world) | `DrawState`, `ToolSettings` | — |

`ToolSettings` holds every variant field in §2.2 per tool (material, roof
style, pitch, fins, planters, ruined, plinth, overgrowth, thickness, fence
kind, path kind and width, stair kind, bridge kind, railing, hanging,
opening kind, tree kind, clutter kind, paint layer, brush, radius,
strength, depth) plus the shared `Colors { roof, concrete, accent }`
palette indices. `Environment` holds hour, paused, weather override or
Auto, and the current auto state.

The strip is rebuilt (despawn children, respawn) only when `ActiveTool`
changes; while the tool stays the same, a single sync system updates
fills, bands, knob positions and readouts from `ToolSettings` each frame.

## 5. Accessibility

- Text contrast ≥ 4.5:1 for everything enabled (§1.1).
- Every hit target ≥ 32×32 logical px: segments, toggles, swatches (28 px
  cell plus 2 px gap counts as a 30 px target, so swatch buttons are
  given 32×32 hit nodes with the 28 px cell centred), the collapsed hints
  tab, the pause button.
- Selection is never colour alone: active buttons get the band and border,
  toggles get a filled box with an inner square, swatches get the border
  and the under-band, paused time swaps `||` for `>`.
- Every tool and variant is keyboard reachable (§3.2).
- Hints never auto-hide. Nothing flashes faster than the 80 ms press.

## 6. What the HUD does not do

No minimap, no resource counters, no notifications, no confirmation
dialogs, no splash or menu chrome during play, no modal panels. Screenshot
(`F12`) hides the HUD for that frame.

## 7. Implementation budget

Rough line counts for `src/hud/` in plain `bevy_ui`, to stay inside
600–900 lines:

| Piece | Lines |
|---|---|
| Tokens (`const` colours, sizes) and `UiScale` system | 50 |
| Widget spawners: panel slab (tie-holes, drip), button, segment row, toggle, slider row, swatch group | 170 |
| ToolBar + tool table | 90 |
| VariantStrip: per-tool spawn table + sync system + clutter grid | 220 |
| WeatherPanel + sync | 110 |
| UndoRedo + HintsPanel (hint text table per tool and state) | 110 |
| Tooltip, pointer-capture flag, press flash | 60 |
| Keyboard routing for tools, Tab, `-`/`=`, `H` | 60 |
| **Total** | **≈ 870** |

Keep every per-tool difference in data tables (labels, ranges, which
controls are enabled), not in branching UI code.
