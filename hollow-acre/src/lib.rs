//! Hollow Acre: cozy iso pixel-art brutalist builder.

pub mod camera;
pub mod game;
pub mod life;
pub mod render;
pub mod scene;
pub mod shot;
pub mod tools;
pub mod ui;
pub mod weather;

use bevy::prelude::*;

/// Builds and runs the game.
pub fn run() {
    let args = shot::CliArgs::from_env();
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "HOLLOW ACRE".into(),
            resolution: (1280, 720).into(),
            ..default()
        }),
        ..default()
    }))
    .insert_resource(args)
    // Placeholder until the world/scene plugin exists; remove to drop the test scene.
    .insert_resource(render::test_scene::TestScene)
    .add_plugins((render::ToonRenderPlugin, camera::CameraPlugin, shot::ShotPlugin))
    .add_plugins((
        game::GamePlugin,
        scene::ScenePlugin,
        tools::ToolsPlugin,
        weather::WeatherPlugin,
        life::LifePlugin,
        ui::UiPlugin,
    ));
    app.run();
}
