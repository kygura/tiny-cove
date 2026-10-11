//! THISTLE GLADE: a cosy medieval glade builder with a PS1 cel-shaded look.

pub mod env;
pub mod render;
pub mod weather;
pub mod world;

use bevy::prelude::*;

/// Adds every game plugin.
pub struct GladePlugin;

impl Plugin for GladePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<env::Environment>().add_plugins((render::RenderPlugin, world::WorldPlugin, weather::WeatherPlugin));
    }
}
