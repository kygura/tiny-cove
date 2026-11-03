# THISTLE GLADE — interface design brief

The glade is the star. The HUD is a few small parchment slips: soft, quiet,
out of the way, never animated for its own sake, never covering the centre.

## 1. Visual language

| Token | Hex | Use |
|---|---|---|
| `parchment` | `#F3E7CF` at 92 % alpha | panel and button background |
| `parchment-deep` | `#E9D9B8` | variant strip, tooltip, slider track |
| `ink` | `#3A2A1E` | all text, glyphs, slider knob |
| `ink-soft` | `#6F5A44` | secondary text |
| `wood` | `#8C6A43` | panel borders, dividers |
| `wood-dark` | `#6B4A2E` | button borders, 1 px panel shadow |
| `accent` | `#C0563A` | active border, slider fill (never under text) |
| `hover` | `#FBF2DF` | hovered button background |
| `active` | `#F2D4B8` | active button background + `accent` border |
| `disabled` | `#B5A48E` on `#EDE1CB` | unavailable buttons |
| `invalid` | `#C84A3A` at 55 % | red ghost tint, invalid hint text |
| `ghost-ok` | `#FFFFFF` at 45 % | valid placement ghost |

- Radius: 6 px buttons/swatches, 10 px panels, 4 px tooltips.
- Borders: 2 px `wood` on panels, 1 px `wood-dark` on buttons, 2 px `accent` on
  the active button. Panels get a 1 px `wood-dark` bottom shadow node, no blur.
- Spacing scale 4 / 8 / 12 / 16 / 24 px: panel padding 8, screen margin 16,
  button gap 4, group gap 12.
- Type: Bevy default font only. 11 (swatch index, hint keys), 12 (hints,
  tooltips, variant labels), 14 (tool and weather labels), 16 (clock). Line
  height 1.3.
- Icons: none. Buttons carry ASCII labels: `Wall House Tower Fence Path Stairs
  Deck Bridge Win/Dr Roof Tree Clutter Paint Up/Dn Flat Water Style Erase`.
  Undo/redo `<` `>`, pause `||`, play `>`. Swatches are coloured squares.

## 2. Layout and scale

Reference window 1280×720. `UiScale = clamp(round(height/720 × 4)/4, 0.75,
2.0)`; quarter steps keep text crisp. Panels anchor to window edges with the
16 px margin, independent of the 240 px render target.

| Region | Anchor | Size at 1.0 |
|---|---|---|
| ToolBar | bottom-centre | 18 ToolButtons 56×48 in 4 groups, height 64 |
| VariantStrip | 8 px above ToolBar, centred | auto width, height 44 (88 for Clutter) |
| WeatherPanel | top-right | 236 wide |
| UndoRedo | top-left | two 40×36 buttons |
| HintsPanel | bottom-left | 260 wide, ≤ 7 lines; collapsed = 32×32 `?` tab |

ToolBar groups, divided by 1 px `wood` lines: **Build** Wall, House, Tower,
Fence, Path, Stairs, Platform, Bridge · **Decorate** Window/Door, Roof, Tree,
Clutter, Paint · **Terrain** Raise/Lower, Flatten, Water · **Edit** Style,
Eraser. Below 1150 px logical width group gaps shrink to 4 px; never wrap.

VariantStrip contents (segmented buttons ≥ 32×32; sliders 120 px wide):

| Tool | Controls |
|---|---|
| Wall | Stone/Wood; Battlements; Ruined; Foundation slider 0–2 m; roof, wall, trim swatches |
| House, Tower | Stone/Plaster/Wood; Gable/Hip/Cone/Flat; Half-timber; Battlements (Flat only); Foundation; three swatches |
| Fence | WoodRail/Picket/Stone/Wattle; trim swatch |
| Path | Dirt/Cobble/Flagstone/Setts; width 0.8–4 m |
| Stairs | Wood/Stone/Ladder; trim swatch |
| Platform | trim swatch |
| Bridge | Rope/Stone/Wood; railing 1–4 and hanging None/Lanterns/Flags (Rope only); trim swatch |
| Roof | Gable/Hip/Cone/Flat; pitch slider; roof swatch |
| Tree | Oak/Pine/Birch/Fruit |
| Clutter | 14-cell grid, 7 per row, text labels |
| Paint | Flowers/Bushes/Grain/Dirt/Clear; brush radius 1–8 m |
| Raise/Lower, Flatten, Water | radius; strength |
| Style | three swatches showing the copied style |
| Eraser, Window/Door | strip hidden |

Swatch colours from `core/src/palette.rs`: roof `#C0563A #4F6F8F #3F8A7A
#6C8A3A #D0A040 #7A4A6A`; wall `#EFE4CC #F7F3EA #E8C9B8 #D6C08E #B9C4C9`; trim
`#6B4A2E #8C6A43 #3D5A6C #8A3B2E #4E5E3A`. A Swatch is a 32×32 cell holding a
24×24 square with a 1 px `wood-dark` border; the selected one gets 2 px `accent`.

WeatherPanel rows: ClockLabel `12:30` + pause button; seven 28×28 buttons
`Clr Ovc Rain Strm Fog Snow Auto`; `Fog` slider; `Snow` toggle + intensity
slider. Auto is default; any other button clears it; Auto resumes cycling.

## 3. Interaction and state

A `DrawState` resource (`Idle`, `Hover`, `Drawing`, `Raising`, `Invalid`)
drives both the world ghost and the HUD.

| State | World | HUD |
|---|---|---|
| Idle | nothing | HintsPanel lists the active tool's gestures |
| Hover | single-dab / prop / first-point ghost in `ghost-ok` | the matching hint line switches to `ink` |
| Drawing | live stroke, polyline, rectangle or circle preview | only in-stroke gestures listed; VariantStrip reflects scroll changes live |
| Raising | footprint fixed; mouse up/down sets height; scroll sets pitch | hints: `Move  height · Scroll  pitch · Shift+Scroll  style · Click  place` |
| Invalid | ghost tinted `invalid`; click does nothing | first hint line in `invalid`: `Outside the glade`, `Needs a wall`, `Too short`, `Needs water below` |

Per-tool gestures:

- Wall, Fence, Path: press starts a polyline, drag/click adds points, click on
  the last point ends; scroll = height (wall) or width (path); Shift = snap to
  45° and close the ring near the start.
- House, Tower: drag footprint → release → Raising → click commits; scroll in
  footprint rotates; Shift forces square/circle.
- Stairs, Bridge, Platform: drag A→B; scroll = width; Shift snaps heights to
  the nearest wall top.
- Roof: hovered closed ring highlights `ghost-ok`; click roofs it; scroll =
  pitch, Shift+scroll = style.
- Window/Door: hovered wall segment highlights; click adds a window, clicking an
  opening upgrades it (Window → Door → WideDoor); Shift+click removes.
- Tree, Clutter: click plants; scroll rotates; Shift+scroll scales 0.7–1.3.
- Terrain brushes, Paint: drag dabs; scroll = radius; Shift inverts (lower,
  erase).
- Style: hover highlights in `accent`; click copies colours and material into
  ToolSettings. Eraser: hover highlights in `invalid`; click or drag removes.
- Esc or RMB tap cancels any state; RMB drag always orbits.

Keyboard: `1` Wall `2` House `3` Tower `4` Fence `5` Path `6` Stairs `7`
Platform `8` Bridge `9` Window/Door `0` Roof `G` Tree `C` Clutter `P` Paint
`R` Raise/Lower `F` Flatten `V` Water `I` Style `X` Eraser; `Tab` cycles the
strip's first segment; `Ctrl+Z`/`Ctrl+Y`; `T` pause, `[` `]` scrub; `H` hints;
`F12` screenshot; WASD/Q/E stay camera.

Tooltips appear 450 ms after hover rests, `parchment-deep`, 12 px, `Wall  (1)`
plus one `ink-soft` line (`Drag a line of stone or wood`); hide on press or
leave. A press flashes `active` for 80 ms. Undo/redo go `disabled` when History
has nothing. Hover over any HUD node blocks world input.

## 4. Components

| Component | Reads | Writes |
|---|---|---|
| `ToolBar` → `ToolGroup` → `ToolButton` | `ActiveTool` | `ActiveTool` |
| `VariantStrip` → `SegmentButton`, `ToggleButton`, `SliderRow`, `Swatch` | `ActiveTool`, `ToolSettings` | `ToolSettings` |
| `WeatherPanel` → `ClockLabel`, `PauseButton`, `WeatherButton`, `FogSlider`, `SnowToggle` | `Environment` | `Environment` |
| `UndoRedo` | `History` | `History` |
| `HintsPanel` → `HintLine` | `ActiveTool`, `DrawState`, `HintsVisible` | `HintsVisible` |
| `Tooltip` | hovered `TooltipText` | — |
| `GhostPreview` (world) | `DrawState`, `ToolSettings` | — |

`ToolSettings` holds every variant field above plus the shared `Colors`.
`Environment` holds hour, paused, weather override or Auto, fog 0–1, snow
on/intensity.

## 5. Accessibility

Text contrast ≥ 4.5:1 (tokens above). Every hit target ≥ 32×32 logical px,
swatches and the collapsed hints tab included. Every tool and variant is
keyboard reachable (keys above, `Tab`; sliders take `-`/`=`). Active state is
border plus fill, never colour alone. Hints never auto-hide.
