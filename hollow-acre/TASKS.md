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
2. DESIGN.md — designer — frontier
3. core crate (model, generation, mesher) — worker — frontier
4. render pipeline + camera (low-res target, toon material, outlines, shadows) — heavy — after 2
5. scene sync + tools + demo mode — worker — after 3, 4
6. weather + time + life — worker — after 4
7. HUD — ui-impl — after 2, 5 contract
8. verification gate — reviewers + demo shot — after all

## Decisions so far

- Workspace: pure `acre_core` (no Bevy) + `hollow_acre` game crate; shared `../target`.
- Camera: ortho, free orbit (user choice). Building: freeform draw (user choice).
