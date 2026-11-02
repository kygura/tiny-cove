use bevy::prelude::*;

fn main() {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window { title: "Greywild".into(), ..default() }),
        ..default()
    }))
    .add_plugins(greywild::GladePlugin);
    if let Some(test) = greywild::render::RenderTest::from_args(std::env::args()) {
        app.insert_resource(test);
    }
    app.run();
}
