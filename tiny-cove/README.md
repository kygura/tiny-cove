# TINY COVE

A cozy, goal-free builder set on a small sunlit island. You build a home for a
seafaring people. Drag out dry-stone dykes, thatched huts, roundhouses, beacon
towers, shell tracks, cliff steps, jetties and rope bridges, and the game adds
the lashings, hanging fish, shutters, smoke holes, mooring posts and stilts.
Over the course of a day the tide goes in and out twice. Gulls ride the wind,
crabs work the waterline and canoes bob at the jetties. When night falls the
beacons are lit.

The world is rendered like a PS1 game: a 240 px tall target upscaled with
nearest-neighbour, vertices snapped to that grid, affine textures, and 15-bit
colour with a Bayer dither. Lighting uses cel bands with shadows tinted violet
rather than grey, sharp sun shadows and 1-px outlines. Weather can be clear,
overcast, rain, storm, sea fog or snow. The palette is deliberately loud:
turquoise shallows, saffron sand, lime grass, golden thatch, and madder, woad
and cochineal paint.

Built with Rust and Bevy 0.20. `SPEC.md` describes the game and its building
kit, `DESIGN.md` defines the art and HUD, and `TASKS.md` is the build log.

![noon](docs/noon.png)

| dusk rain | night | storm |
|---|---|---|
| ![](docs/dusk-rain.png) | ![](docs/night.png) | ![](docs/storm.png) |
| **sea fog** | **snow** | **HUD** |
| ![](docs/seafog.png) | ![](docs/snow.png) | ![](docs/hud.png) |

## Run

```sh
cd tiny-cove
cargo run --release            # empty island
cargo run --release -- --demo  # start in the showcase cove
```

On Linux the build needs `libudev-dev`, `libasound2-dev`, `libwayland-dev` and
`libxkbcommon-dev`, plus Rust 1.97.1 or newer. The build cache is shared with the
sibling projects through `../target`.

## Building kit

| Key | Tool | What you get |
|---|---|---|
| 1 | Dyke | dry-stone, turf or palisade wall. Turns into a breakwater over the sea. A track through it leaves a gap with standing-stone gateposts. |
| 2 | Windbreak | wattle, driftwood rail, net line or reed screen |
| 3 | Hut | longhut in wattle-and-daub, plank or stone, with a thatch, turf, hide or upturned-boat roof. Stands on stilts over water or steep ground. |
| 4 | Round | roundhouse, or a beacon tower with a fire basket when raised tall |
| 5 | Track | crushed shell, pebbles, flagstones, boardwalk or stepping stones |
| 6 | Steps | cut stone, log steps or rope ladder |
| 7 | Jetty | plank deck on piles, with mooring posts and canoes alongside |
| 8 | Bridge | rope bridge (4 railings), driftwood log or stone causeway, with lanterns or pennants |
| 9 | Opening | cycles shuttered window, doorway and boat door |
| 0 | Roof | roofs a closed dyke ring |
| G | Tree | sea pine, palm, cypress, fig, bleached snag |
| C | Prop | canoe, drying rack, firepit, cairn, standing stone, carved pole, net rack, baskets, lantern post, pennant, rock, gorse, crab pots, woodpile |
| P | Paint | sea pinks, dune grass, wrack, pebbles |
| R / F / V | Raise, Flatten, Lagoon | shape the island; hold Shift to lower |
| I | Dye | recolour, set material and carving, salt weathering |
| X | Erase | remove a structure |

## Controls

| Input | Action |
|---|---|
| LMB drag | draw. For huts and roundhouses, drag the footprint, move the mouse to raise it, then click. |
| Scroll while drawing | height, roof style or rotation |
| Shift | 45° snap, lower terrain, remove an opening |
| Ctrl+Z / Ctrl+Y | undo / redo |
| Esc / RMB tap | cancel |
| RMB drag, Q/E | orbit |
| Scroll, WASD / MMB drag, PgUp/PgDn | zoom, pan, tilt |
| T, `[` `]` | pause time, scrub the hour |
| H | hide or show hints |
| F12 | screenshot |

## Verify

```sh
cargo test --release --workspace
cargo build --release
# headless (Mesa lavapipe + Xvfb):
export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json WGPU_BACKEND=vulkan
xvfb-run -a -s "-screen 0 1280x720x24" ../target/release/tiny-cove --demo-shot out.png --time 18.5 --weather rain [--hud] [--zoom S] [--yaw D]
xvfb-run -a -s "-screen 0 1280x720x24" ../target/release/tiny-cove --tool-smoke smoke.png   # drives all 18 tools, exits 0 on pass
```

## Layout

- `core/` is the `cairn_cove_core` crate and does not depend on Bevy. It holds:
  - the document and undo history;
  - the island terrain;
  - procedural meshes for every primitive (`walls`, `buildings`, `paths`, `nature`).

  It is deterministic and unit-tested.
- `src/` is the Bevy side:
  - `render`: the PS1/cel material, atlas, sea and post-processing;
  - `world`: keeps the scene in sync with the document;
  - `tools` and `ui`: input and HUD;
  - `weather` and `env`: day/night, weather, wind and tide;
  - `life`: gulls, crabs, fish, canoes, smoke, flames and fireflies;
  - `camera` and `shot`.

The engine started as a fork of the sibling `greywild` prototype and was rebuilt for
the coast.
