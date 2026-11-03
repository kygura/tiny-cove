# Tiny Cove

![Tiny Cove footage](tiny-cove/docs/footage/tiny-cove.gif)

The main project in this repository is **Tiny Cove**. It is a cozy, goal-free
island builder written in Rust with Bevy, with a PS1-style cel-shaded look,
sharp light and six kinds of weather.

**[Watch the footage (MP4, 87 s)](tiny-cove/docs/footage/tiny-cove.mp4)** ·
**[Read the Tiny Cove README](tiny-cove/README.md)**

```sh
cd tiny-cove
cargo run --release -- --demo   # start in the showcase cove
```

| noon | storm | sea fog |
|---|---|---|
| ![](tiny-cove/docs/noon.png) | ![](tiny-cove/docs/storm.png) | ![](tiny-cove/docs/seafog.png) |

## Archive

Earlier prototypes live in [`archive/`](archive). Each one is a standalone Cargo
workspace (`cd archive/<name> && cargo run --release`), and none of them is maintained.

| Project | What it is |
|---|---|
| [`greywild`](archive/greywild) | Eco-brutalist valley builder with board-formed concrete taken back by moss and birch. Tiny Cove's engine started as a fork of it. |
| [`thistle-glade`](archive/thistle-glade) | Medieval glade builder with walls, houses, towers, sheep and ducks |
| [`hollow-acre`](archive/hollow-acre) | Brutalist concrete poured into regrown post-apocalyptic farmland with aether crystals |
| [`concrete-glade`](archive/concrete-glade) | Brutalist megastructures on a dead glade |

Tiny Cove, greywild, thistle-glade and hollow-acre share one build cache in `target/` at
the repository root. concrete-glade keeps its own.
