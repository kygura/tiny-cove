//! HUD (see DESIGN.md). Reads/writes `tools::ToolSettings`, reads `tools::ToolStatus`,
//! sets `tools::PointerOverUi`, drives `weather::{Clock, Weather}` and `world::Glade` undo/redo.

use bevy::prelude::*;

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, _app: &mut App) {}
}
