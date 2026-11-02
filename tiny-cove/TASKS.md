# TINY COVE — build map

Harness: Claude Code (cloud, auto mode, ultracode on). Planner: claude-opus-5-5 @ xhigh.
Roster: opus (claude-opus-5-5), sonnet (claude-sonnet-5-5), haiku (claude-haiku-5-5), fable
(claude-fable-5-1). All Anthropic, so provider diversity is limited. Reviewers run in fresh
contexts on a different model or effort than the implementer.

| Role | Model | Effort |
|---|---|---|
| planner | opus (this session) | xhigh |
| discussion | opus fresh context | max |
| designer | opus (3-way judge panel) | high |
| heavy | opus | high |
| worker | sonnet | high |
| ui-impl | sonnet | high |
| reviewer | opus fresh context (≠ sonnet implementers); sonnet for opus-written tasks | high |
| drift-check | haiku | low |

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

## Tasks
| # | Task | Files | Model | Deps | State |
|---|---|---|---|---|---|
| 0 | Model contract re-theme, keep build green | core/*, src/* (mechanical) | opus | — | done |
| 1 | Art direction + HUD brief (judge panel) | DESIGN.md | opus ×4 | — | done |
| A | Dykes, breakwater, gateposts, windbreaks | core/src/walls.rs | opus | 0,1 | done |
| B | Huts, roundhouses, beacons, roofs, stilts | core/src/buildings.rs | opus | 0,1 | done |
| C | Tracks, steps, jetty, bridges | core/src/paths.rs | sonnet | 0,1 | done |
| D | Trees, props | core/src/nature.rs | sonnet | 0,1 | done |
| E | Island terrain, lagoon, scene rules, demo cove | core/src/terrain.rs, scene.rs, demo.rs | opus | 0 | done |
| F | Look: atlas, palette, sea, wind, weather, sky | src/render/*, core/src/palette.rs, src/weather.rs, src/env.rs | opus | 0,1 | frontier |
| G | Tools, HUD, smoke | src/tools.rs, src/tools/smoke.rs, src/ui.rs, src/ui/strip.rs, src/camera.rs | sonnet | 0,1 | frontier |
| H | Life: gulls, crabs, fish, boats, smoke, fire | src/life.rs | sonnet | 0 | frontier |
| V | Verification gate | — | opus/sonnet/haiku | A–H | blocked |
