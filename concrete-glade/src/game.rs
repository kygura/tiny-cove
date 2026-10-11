//! Shared game-state types: the contract between tools, scene sync and the HUD.

use crate::structure::World;
use crate::voxel::VoxelGrid;
use bevy::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Tool {
    #[default]
    Wall,
    Block,
    Road,
    Raze,
}

impl Tool {
    pub const ALL: [Tool; 4] = [Tool::Wall, Tool::Block, Tool::Road, Tool::Raze];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Wall => "WALL",
            Tool::Block => "BLOCK",
            Tool::Road => "ROAD",
            Tool::Raze => "RAZE",
        }
    }
}

/// Current tool and the one-line hint the HUD shows (tools keep it up to date).
#[derive(Resource, Default)]
pub struct ToolState {
    pub tool: Tool,
    pub hint: String,
}

/// The editable document (structures + undo history).
#[derive(Resource)]
pub struct Doc(pub World);

/// The voxel grid currently displayed: the document plus any pending ghost structure.
/// Scene sync remeshes chunks whenever this changes.
#[derive(Resource, Default)]
pub struct Grid(pub VoxelGrid);

/// Commands the HUD (or keys) send; the tools module handles them.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditCommand {
    SelectTool(Tool),
    Undo,
    Redo,
    NewWorld,
}

/// Live preview state the tools maintain: a structure being drawn and structures
/// hidden by an in-progress demolish. Scene sync folds it into [`Grid`].
#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub struct Ghost {
    pub pending: Option<crate::structure::Structure>,
    pub hidden: Vec<crate::structure::StructureId>,
}
