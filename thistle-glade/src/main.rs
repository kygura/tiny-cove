use bevy::prelude::*;

fn main() {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window { title: "Thistle Glade".into(), ..default() }),
        ..default()
    }))
    .add_plugins(thistle_glade::GladePlugin);
    if let Some(test) = thistle_glade::render::RenderTest::from_args(std::env::args()) {
        app.insert_resource(test);
    }
    app.run();
}
