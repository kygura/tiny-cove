# Hollow Acre — build map

Harness: Claude Code (cloud). Planner: Opus 5.5 (this session). Single provider
(Anthropic) roster, so reviewer diversity is by model/context, not provider.

| Role | Model | Notes |
|---|---|---|
| planner | opus 5.5 | fixed |
| designer | opus | no visual-lane evidence on file → design marked degraded, extra drift check |
| heavy | opus | render pipeline, hard integration |
| worker | sonnet | core crate, gameplay systems |
| ui-impl | opus | HUD against DESIGN.md |
| reviewer | fresh opus context / sonnet for opus-written code | |
| grunt | haiku | unused so far |

**Destination:** cozy iso pixel-art cel-shaded brutalist builder (SPEC.md), demo shot verified headless.

## Tasks

1. SPEC.md — done (planner)
2. DESIGN.md — designer (opus) — done
3. core crate (model, generation, mesher) — worker (sonnet) — done, reviewed (opus), 6 findings fixed
4. render pipeline + camera — heavy (opus) — done (+ follow-ups, review fixes, drift fixes)
5. scene sync + tools + demo mode — worker (sonnet) — done (+ 11 review fixes)
6. weather + time + life — worker (sonnet) — done (+ floating monolith, review fixes)
7. HUD — ui-impl (opus) — done (+ legibility, tally cost)
8. verification gate — done: 2 reviewers (opus gameplay, sonnet render/UI + drift), live xdotool harness, 80 tests, clippy clean

## Decisions so far

- Workspace: pure `acre_core` (no Bevy) + `hollow_acre` game crate; shared `../target`.
- Camera: ortho, free orbit (user choice). Building: freeform draw (user choice).
- Design: 480×270-class internal res, 3 cel bands, blue-violet shadow tint, hue-shifted outlines, warm beige concrete.
- Render: ExtendedMaterial-style toon material, depth/normal prepass outline post-process, Bevy Bloom for emissives, single hard shadow cascade.
- Core: generation ~1.4 ms/doc, structures clamp to grid, NaN structures dropped.
- src/game.rs is the shared contract: one writer per resource; HUD talks to tools/weather via UiCommand messages.
- Layout: fill window at integer scale (no letterbox); DESIGN updated.
- Monolith slab is a bobbing entity in life, not voxels.
