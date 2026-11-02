# TINY COVE — build map

Harness: Claude Code (cloud, auto mode, ultracode on). The planner is the invoking session.
The roster is single-provider, so provider diversity is limited. Reviewers run in fresh
contexts on a different tier or effort than the implementer.

| Role | Tier | Effort |
|---|---|---|
| planner | frontier (this session) | xhigh |
| discussion | frontier, fresh context | max |
| designer | frontier (3-way judge panel) | high |
| heavy | frontier | high |
| worker | mid-tier | high |
| ui-impl | mid-tier | high |
| reviewer | frontier fresh context for mid-tier work, mid-tier for frontier work | high |
| drift-check | small | low |

**Destination:** SPEC.md "Done". Tests green, release build clean, six headless weather
shots that are vibrant, PS1 and cel-shaded, `--tool-smoke` passing for all 18 tools, merged
to `main`.

## Decisions so far
- Original game in the cozy-builder genre. It does not recreate any existing game's
  primitive set; the kit comes from the coastal setting (SPEC.md).
- Base: fork of the Greywild engine (PS1/cel renderer, Bevy-free core, tool framework).
- The rustc 1.97.0 → 1.99.1 update was needed for Bevy 0.20.
- Palette: vibrant and saturated, per a user directive given mid-build.
- `main` does not exist on the remote. It will be created from the default branch head and
  the build PR merged into it.
- Contract (fd7df3e): types re-themed, placeholder geometry mapped from old variants, 121 tests
  green. Beacon = round with wall_height ≥ 2.5 × radius (`buildings::is_beacon`). A floating
  canoe is a Boat marker with no mesh, so picking falls back to proximity (task E).
- Design (0832f67): the judge panel scored textile 8.7, PS1 8.5, poster 7.3. "Folk Pigment"
  art plus the "Woad Cloth" HUD. Hexes are snapped to 5 bits, and shade() has calibration
  goldens.
- Phase 2: the eight tasks ran in isolated worktrees. A–E passed first try and merged cleanly
  (110 core + 37 game tests). F, G and H hit the account's 5-hour usage limit mid-task. F's
  WIP was saved as b886a17 and they are re-running as phase 2b.
- Integration: scene and demo call the real `buildings::needs_stilts`, and hut tests use level
  ground (the island moved their expectations).
- `env::Wind` and `env::Tide` were promoted to main early so life (H) can read them while F
  finishes the look.
- A shared CARGO_TARGET_DIR across worktrees mixes up binaries, which A observed as a stale
  test binary. Phase 2b uses a private copy of the warm cache per task (target-F/G/H).
- Palette tokens missing at phase 2 were faked locally in walls, buildings, paths and terrain
  ('tok' helpers). F adds the real ones; the helpers get swapped at integration.
- Phase 3 (379b400): integration swapped the tok helpers for palette fns and made the sea edge a
  ring. Review panel: 7 lenses → 70 findings, of which skeptics confirmed 58 and refuted 12.
- Fix loop 1 (07e64d8): four parallel groups. All core rules fixed (cut merging, ring seams,
  jetty height, terraces, canoe depth). Sky shown above a horizon cut, storm/fog anti-mud,
  slanted rain, integer upscale, vertex-alpha pennant flutter, off-thread preview builds, one
  warm-light rule, and the Dye/Erase/tree-scatter/orbit-snap fixes. Ponytail findings were
  reported, not applied.
- User request: the game is renamed to Tiny Cove (b02a6ba). The folder, package and binary are
  tiny-cove; the internal crate names keep cairn_cove.
- Re-verify 2: Done 1–4 pass (127 + 105 tests, 0 warnings, 6 weather shots, smoke 78/78), and
  every loop-1 core fix holds. Reviewer shot scores: noon 7, dusk-rain 5, night 7, storm 4,
  seafog 5, snow 7.5. Fix loop 2 targets the look (fog, storm, dusk, sky band, lantern pools),
  shallow-angle dyke cuts, the camera clamp and the emissive rule.
- SPEC/DESIGN amended to match the shipped trim rows and the jetty mooring depth (63ac7c9).

## Tasks
| # | Task | Files | Model | Deps | State |
|---|---|---|---|---|---|
| 0 | Model contract re-theme, keep build green | core/*, src/* (mechanical) | frontier | — | done |
| 1 | Art direction + HUD brief (judge panel) | DESIGN.md | frontier ×4 | — | done |
| A | Dykes, breakwater, gateposts, windbreaks | core/src/walls.rs | frontier | 0,1 | done |
| B | Huts, roundhouses, beacons, roofs, stilts | core/src/buildings.rs | frontier | 0,1 | done |
| C | Tracks, steps, jetty, bridges | core/src/paths.rs | mid-tier | 0,1 | done |
| D | Trees, props | core/src/nature.rs | mid-tier | 0,1 | done |
| E | Island terrain, lagoon, scene rules, demo cove | core/src/terrain.rs, scene.rs, demo.rs | frontier | 0 | done |
| F | Look: atlas, palette, sea, wind, weather, sky | src/render/*, core/src/palette.rs, src/weather.rs, src/env.rs | frontier | 0,1 | done |
| G | Tools, HUD, smoke | src/tools.rs, src/tools/smoke.rs, src/ui.rs, src/ui/strip.rs, src/camera.rs | mid-tier | 0,1 | done |
| H | Life: gulls, crabs, fish, boats, smoke, fire | src/life.rs | mid-tier | 0 | done |
| V | Verification gate | — | mixed tiers | A–H | loop 2 of 3 |
