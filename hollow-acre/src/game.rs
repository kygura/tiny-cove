//! Shared game-state contract between scene, tools, weather, life and HUD.
//! Each resource has exactly one writer (named in its doc); everyone else reads.

use acre_core::{History, VoxelGrid};
use bevy::prelude::*;

/// The editable world. Writer: `tools` (push/undo/redo/new world).
/// `history.doc()` is the committed document.
#[derive(Resource)]
pub struct World {
    pub history: History,
}

/// What is on screen: committed doc plus the pending (ghost) structure, if any.
/// Writer: `scene`. Read by `life` for ground heights and obstacles.
#[derive(Resource, Default)]
pub struct DisplayedGrid {
    pub grid: VoxelGrid,
    /// Bumped every time `grid` changes.
    pub version: u64,
}

/// The pending structure being dragged, appended to the committed doc for display.
/// Writer: `tools`. Read by `scene`.
#[derive(Resource, Default)]
pub struct Ghost(pub Option<acre_core::Structure>);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Tool {
    #[default]
    Wall,
    Block,
    Path,
    Field,
    Spire,
    Raze,
}

impl Tool {
    pub const ALL: [Tool; 6] = [Tool::Wall, Tool::Block, Tool::Path, Tool::Field, Tool::Spire, Tool::Raze];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Wall => "WALL",
            Tool::Block => "BLOCK",
            Tool::Path => "PATH",
            Tool::Field => "FIELD",
            Tool::Spire => "SPIRE",
            Tool::Raze => "RAZE",
        }
    }
}

/// Phase of the current interaction, for hints and HUD.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ToolPhase {
    #[default]
    Idle,
    Dragging,
    /// Block tool only: footprint placed, mouse sets floors, click confirms.
    Raising,
}

/// Writer: `tools` (keys) and `ui` (button clicks) — both only set `tool`/`spire_kind`
/// through [`SelectTool`] messages handled by `tools`.
#[derive(Resource, Default)]
pub struct ToolState {
    pub tool: Tool,
    pub phase: ToolPhase,
    /// 0 crystal, 1 monolith, 2 blossom tree.
    pub spire_kind: u8,
    /// Shift held for tall walls.
    pub tall: bool,
}

/// HUD → tools requests.
#[derive(Message, Clone, Copy, Debug)]
pub enum UiCommand {
    SelectTool(Tool),
    Undo,
    Redo,
    SetWeather(WeatherKind),
    TogglePause,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum WeatherKind {
    #[default]
    Clear,
    Overcast,
    Rain,
    Storm,
    Mist,
}

impl WeatherKind {
    pub const ALL: [WeatherKind; 5] =
        [WeatherKind::Clear, WeatherKind::Overcast, WeatherKind::Rain, WeatherKind::Storm, WeatherKind::Mist];

    pub fn label(self) -> &'static str {
        match self {
            WeatherKind::Clear => "CLEAR",
            WeatherKind::Overcast => "OVERCAST",
            WeatherKind::Rain => "RAIN",
            WeatherKind::Storm => "STORM",
            WeatherKind::Mist => "MIST",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|w| w.label().eq_ignore_ascii_case(s))
    }
}

/// Clock and weather. Writer: `weather`.
#[derive(Resource)]
pub struct Sky {
    /// Hours, 0..24.
    pub hour: f32,
    pub paused: bool,
    /// Target weather (transitions blend towards it).
    pub weather: WeatherKind,
}

impl Default for Sky {
    fn default() -> Self {
        Self { hour: 15.0, paused: false, weather: WeatherKind::Clear }
    }
}

/// Scene readiness: true once every dirty chunk has been remeshed. Writer: `scene`.
#[derive(Resource, Default)]
pub struct MeshingDone(pub bool);

pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DisplayedGrid>()
            .init_resource::<Ghost>()
            .init_resource::<ToolState>()
            .init_resource::<Sky>()
            .init_resource::<MeshingDone>()
            .add_message::<UiCommand>();
    }
}
