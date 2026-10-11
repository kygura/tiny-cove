use bevy::prelude::*;
use concrete_glade::render::Ps1Plugin;

fn main() {
    App::new().add_plugins((DefaultPlugins, Ps1Plugin)).run();
}
