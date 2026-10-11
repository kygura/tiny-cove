//! THISTLE GLADE: a cosy medieval glade builder with a PS1 cel-shaded look.

pub mod camera;
pub mod env;
pub mod render;
pub mod shot;
pub mod weather;
pub mod world;

use bevy::prelude::*;

/// Adds every game plugin.
pub struct GladePlugin;

impl Plugin for GladePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<env::Environment>()
            .insert_resource(shot::Cli::parse(std::env::args()))
            .add_plugins((render::RenderPlugin, world::WorldPlugin, weather::WeatherPlugin, camera::CameraPlugin, shot::ShotPlugin));
    }
}
