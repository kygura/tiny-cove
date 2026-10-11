# GREYWILD — build map

Harness: Claude Code (cloud). Planner: claude-opus-5-5. Roster: opus, sonnet, haiku (Anthropic only; fable unverified).
Roles: designer/heavy/visual geometry = opus; worker/ui-impl = sonnet; reviewer = opus fresh context (≠ sonnet implementers).

**Destination:** SPEC.md "Done" — tests green, release build clean, five headless demo shots, every primitive placeable.

## Decisions so far
- Base: thistle-glade engine (PS1/cel renderer verified rendering headless in 4 s on lavapipe).
- Model enums re-themed to eco-brutalist names (commit "re-theme model enums").
- Tool/HUD contract in src/tools.rs (ToolSettings, ToolStatus, PointerOverUi).

## Tasks (parallel, disjoint files)
| # | Task | Files | Model | State |
|---|---|---|---|---|
| A | Walls + fences geometry | core/src/walls.rs | opus | frontier |
| B | Blocks, towers, roofs, openings | core/src/buildings.rs | opus | frontier |
| C | Paths, stairs, deck, bridges | core/src/paths.rs | sonnet | frontier |
| D | Trees, clutter, scatter | core/src/nature.rs | sonnet | frontier |
| E | Look: atlas, palette, shader, weather light, demo doc | src/render/*, core/src/palette.rs, core/src/demo.rs, src/weather.rs, src/env.rs | opus | frontier |
| F | Tools + input + camera keys | src/tools.rs, src/camera.rs, src/world.rs | opus | frontier |
| G | HUD per DESIGN.md | src/ui.rs | sonnet | frontier |
| H | Ambient life | src/life.rs | sonnet | frontier |
| V | Verification gate (review, run, shots) | — | opus | blocked on A–H |
