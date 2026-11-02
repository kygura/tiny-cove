# CAIRN COVE — build map

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

## Tasks
| # | Task | Files | Model | Deps | State |
|---|---|---|---|---|---|
| 0 | Model contract re-theme, keep build green | core/*, src/* (mechanical) | opus | — | frontier |
| 1 | Art direction + HUD brief (judge panel) | DESIGN.md | opus ×4 | — | frontier |
| A | Dykes, breakwater, gateposts, windbreaks | core/src/walls.rs | opus | 0,1 | blocked |
| B | Huts, roundhouses, beacons, roofs, stilts | core/src/buildings.rs | opus | 0,1 | blocked |
| C | Tracks, steps, jetty, bridges | core/src/paths.rs | sonnet | 0,1 | blocked |
| D | Trees, props | core/src/nature.rs | sonnet | 0,1 | blocked |
| E | Island terrain, lagoon, scene rules, demo cove | core/src/terrain.rs, scene.rs, demo.rs | opus | 0 | blocked |
| F | Look: atlas, palette, sea, wind, weather, sky | src/render/*, core/src/palette.rs, src/weather.rs, src/env.rs | opus | 0,1 | blocked |
| G | Tools, HUD, smoke | src/tools.rs, src/tools/smoke.rs, src/ui.rs, src/ui/strip.rs, src/camera.rs | sonnet | 0,1 | blocked |
| H | Life: gulls, crabs, fish, boats, smoke, fire | src/life.rs | sonnet | 0 | blocked |
| V | Verification gate | — | opus/sonnet/haiku | A–H | blocked |
