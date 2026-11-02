# GREYWILD — build map

Harness: Claude Code (cloud). Planner: claude-opus-5-5. Roster: opus, sonnet, haiku (Anthropic only; fable unverified).
Roles: designer/heavy/visual geometry = opus; worker/ui-impl = sonnet; reviewer = opus fresh context (≠ sonnet implementers).

**Destination:** SPEC.md "Done" — tests green, release build clean, five headless demo shots, every primitive placeable.

## Decisions so far
- Base: thistle-glade engine (PS1/cel renderer verified rendering headless in 4 s on lavapipe).
- Model enums re-themed to eco-brutalist names (commit "re-theme model enums").
- Tool/HUD contract in src/tools.rs (ToolSettings, ToolStatus, PointerOverUi).
- Walkway on walls is automatic (thickness >= 0.8, no planters) instead of a new model field.
- Paths follow terrain (SPEC reworded) rather than flattening it.
- Demo shots wait for all render pipelines to finish compiling (fixed intermittent black frames).
- Verification: 79 core + 36 game tests, 6 weather shots, --tool-smoke 59/59.

## Tasks (parallel, disjoint files)
| # | Task | Files | Model | State |
|---|---|---|---|---|
| A | Walls + fences geometry | core/src/walls.rs | opus | done |
| B | Blocks, towers, roofs, openings | core/src/buildings.rs | opus | done |
| C | Paths, stairs, deck, bridges | core/src/paths.rs | sonnet | done |
| D | Trees, clutter, scatter | core/src/nature.rs | sonnet | done |
| E | Look: atlas, palette, shader, weather light, demo doc | src/render/*, core/src/palette.rs, core/src/demo.rs, src/weather.rs, src/env.rs | opus | done |
| F | Tools + input + camera keys | src/tools.rs, src/camera.rs, src/world.rs | opus | done |
| G | HUD per DESIGN.md | src/ui.rs | sonnet | done |
| H | Ambient life | src/life.rs | sonnet | done |
| V | Verification gate (review, run, shots) | — | opus | done |
