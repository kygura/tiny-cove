//! Hollow Acre: cozy iso pixel-art brutalist builder.

pub mod camera;
pub mod demo;
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
    .add_plugins((render::ToonRenderPlugin, camera::CameraPlugin, shot::ShotPlugin))
    .add_plugins((
        game::GamePlugin,
        scene::ScenePlugin,
        demo::DemoPlugin,
        tools::ToolsPlugin,
        weather::WeatherPlugin,
        life::LifePlugin,
        ui::UiPlugin,
    ));
    app.run();
}
