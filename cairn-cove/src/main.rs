use bevy::prelude::*;

/// Returns the app's exit status, so a failed `--demo-shot` exits non-zero.
fn main() -> AppExit {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window { title: "Cairn Cove".into(), ..default() }),
        ..default()
    }))
    .add_plugins(cairn_cove::GladePlugin);
    if let Some(test) = cairn_cove::render::RenderTest::from_args(std::env::args()) {
        app.insert_resource(test);
    }
    app.run()
}
