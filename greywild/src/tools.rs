//! Drawing tools: turn pointer input into document edits.
//!
//! Contract shared with the HUD (`ui`) and the camera:
//! - [`ToolSettings`]: the selected tool and every variant knob. The HUD writes it, tools read it.
//!   Hotkeys (see [`Tool::hotkey`]) are handled here and also write it.
//! - [`ToolStatus`]: written by tools every frame for the HUD (hint line, invalid reason).
//! - [`PointerOverUi`]: set by the HUD each frame when the cursor is over a HUD node; tools and
//!   camera ignore pointer input (clicks, drags, scroll) while it is true.
//! - Undo / redo go straight to [`crate::world::Glade`].

use bevy::prelude::*;
use greywild_core::{
    BridgeKind, ClutterKind, Colors, FenceKind, Hanging, Material, OpeningKind, PaintLayer, PathKind, RoofStyle,
    StairKind, TreeKind,
};

/// Every tool, in toolbar order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Tool {
    #[default]
    Wall,
    Fence,
    Block,
    Tower,
    Path,
    Stairs,
    Deck,
    Bridge,
    Opening,
    Roof,
    Tree,
    Clutter,
    Paint,
    Sculpt,
    Flatten,
    Water,
    Style,
    Erase,
}

impl Tool {
    pub const ALL: [Tool; 18] = [
        Tool::Wall,
        Tool::Fence,
        Tool::Block,
        Tool::Tower,
        Tool::Path,
        Tool::Stairs,
        Tool::Deck,
        Tool::Bridge,
        Tool::Opening,
        Tool::Roof,
        Tool::Tree,
        Tool::Clutter,
        Tool::Paint,
        Tool::Sculpt,
        Tool::Flatten,
        Tool::Water,
        Tool::Style,
        Tool::Erase,
    ];

    /// Toolbar label (DESIGN.md §2.1).
    pub fn label(self) -> &'static str {
        match self {
            Tool::Wall => "WALL",
            Tool::Fence => "FENCE",
            Tool::Block => "BLOCK",
            Tool::Tower => "TOWER",
            Tool::Path => "PATH",
            Tool::Stairs => "STAIR",
            Tool::Deck => "DECK",
            Tool::Bridge => "BRIDGE",
            Tool::Opening => "WIN/DR",
            Tool::Roof => "ROOF",
            Tool::Tree => "TREE",
            Tool::Clutter => "CLUTTER",
            Tool::Paint => "PAINT",
            Tool::Sculpt => "RAISE",
            Tool::Flatten => "FLAT",
            Tool::Water => "WATER",
            Tool::Style => "STYLE",
            Tool::Erase => "ERASE",
        }
    }

    /// Selection hotkey and its on-screen tag.
    pub fn hotkey(self) -> (KeyCode, &'static str) {
        match self {
            Tool::Wall => (KeyCode::Digit1, "1"),
            Tool::Fence => (KeyCode::Digit2, "2"),
            Tool::Block => (KeyCode::Digit3, "3"),
            Tool::Tower => (KeyCode::Digit4, "4"),
            Tool::Path => (KeyCode::Digit5, "5"),
            Tool::Stairs => (KeyCode::Digit6, "6"),
            Tool::Deck => (KeyCode::Digit7, "7"),
            Tool::Bridge => (KeyCode::Digit8, "8"),
            Tool::Opening => (KeyCode::Digit9, "9"),
            Tool::Roof => (KeyCode::Digit0, "0"),
            Tool::Tree => (KeyCode::KeyG, "G"),
            Tool::Clutter => (KeyCode::KeyC, "C"),
            Tool::Paint => (KeyCode::KeyP, "P"),
            Tool::Sculpt => (KeyCode::KeyR, "R"),
            Tool::Flatten => (KeyCode::KeyF, "F"),
            Tool::Water => (KeyCode::KeyV, "V"),
            Tool::Style => (KeyCode::KeyI, "I"),
            Tool::Erase => (KeyCode::KeyX, "X"),
        }
    }
}

/// Selected tool plus every variant knob (DESIGN.md §2.2). Ranges are the HUD slider ranges.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct ToolSettings {
    pub tool: Tool,
    /// Walls, blocks, towers.
    pub material: Material,
    /// Blocks, towers, roof tool.
    pub roof: RoofStyle,
    /// Roof pitch in degrees, 15..=60.
    pub roof_pitch_deg: f32,
    pub fins: bool,
    pub planters: bool,
    pub ruined: bool,
    /// Wall thickness in metres, 0.3..=1.2.
    pub wall_thickness: f32,
    /// Plinth (foundation) height in metres, 0..=2.
    pub plinth: f32,
    /// 0..=1.
    pub overgrowth: f32,
    pub fence: FenceKind,
    pub path: PathKind,
    /// Path width in metres, 0.8..=4.
    pub path_width: f32,
    pub stairs: StairKind,
    /// `Cable { railing }` keeps the chosen railing design.
    pub bridge: BridgeKind,
    pub hanging: Hanging,
    /// What a click on bare wall places.
    pub opening: OpeningKind,
    pub tree: TreeKind,
    pub clutter: ClutterKind,
    /// `PaintLayer::Clear` is the paint eraser.
    pub paint: PaintLayer,
    /// Sculpt direction: false = raise, true = lower (Shift inverts while held).
    pub lower: bool,
    /// Brush radius in metres, 1..=8 (paint, sculpt, flatten, water).
    pub brush_radius: f32,
    /// Brush strength 0.1..=1 (sculpt, flatten).
    pub brush_strength: f32,
    /// Water depth in metres, 0.3..=1.5.
    pub water_depth: f32,
    pub colors: Colors,
}

impl Default for ToolSettings {
    fn default() -> Self {
        Self {
            tool: Tool::Wall,
            material: Material::Concrete,
            roof: RoofStyle::Green,
            roof_pitch_deg: 30.0,
            fins: false,
            planters: true,
            ruined: false,
            wall_thickness: 0.6,
            plinth: 0.4,
            overgrowth: 0.35,
            fence: FenceKind::Railing,
            path: PathKind::Slabs,
            path_width: 1.6,
            stairs: StairKind::Concrete,
            bridge: BridgeKind::Cable { railing: 0 },
            hanging: Hanging::None,
            opening: OpeningKind::Window,
            tree: TreeKind::Birch,
            clutter: ClutterKind::Planter,
            paint: PaintLayer::Ferns,
            lower: false,
            brush_radius: 3.0,
            brush_strength: 0.5,
            water_depth: 0.8,
            colors: Colors::default(),
        }
    }
}

/// Feedback from the active tool to the HUD.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct ToolStatus {
    /// A drag / placement is in progress (HUD dims the toolbar slightly, never hides it).
    pub drawing: bool,
    /// Short uppercase reason the current placement is invalid, e.g. `TOO STEEP`.
    pub invalid: Option<String>,
}

/// True while the cursor is over a HUD node (set by `ui` every frame).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PointerOverUi(pub bool);

pub struct ToolsPlugin;

impl Plugin for ToolsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ToolSettings>().init_resource::<ToolStatus>().init_resource::<PointerOverUi>();
    }
}
