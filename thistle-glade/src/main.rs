use bevy::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window { title: "Thistle Glade".into(), ..default() }),
            ..default()
        }))
        .add_plugins(thistle_glade::GladePlugin)
        .run();
}
