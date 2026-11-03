use bevy::prelude::*;

/// Returns the app's exit status, so a failed `--demo-shot` or `--record` exits non-zero.
fn main() -> AppExit {
    let cli = cairn_cove::shot::Cli::parse(std::env::args());
    if !cli.errors.is_empty() {
        for e in &cli.errors {
            eprintln!("tiny-cove: {e}");
        }
        return AppExit::error();
    }
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window { title: "Tiny Cove".into(), ..default() }),
        ..default()
    }))
    .add_plugins(cairn_cove::GladePlugin);
    if let Some(test) = cairn_cove::render::RenderTest::from_args(std::env::args()) {
        app.insert_resource(test);
    }
    app.run()
}
