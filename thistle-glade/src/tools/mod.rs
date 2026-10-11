//! Building tools: mouse/keyboard gestures → `glade_core` documents.
//!
//! While a gesture runs, the tool shows its result live through `Glade::set_preview` (the
//! preview *is* the result) and commits it on finish; Esc or a right-button tap cancels.
//! The document edits themselves are pure functions in [`ops`].
//!
//! Resources other plugins read or write:
//! - [`ActiveTool`], [`ToolSettings`], [`DrawState`], [`ToolHover`] (HUD).
//! - [`PointerOverUi`]: written by the HUD before [`ToolSystems`]; tools ignore the world then.
//! - [`ToolCapture`]: set every frame. `scroll` = the tool (or the HUD under the cursor) eats
//!   the mouse wheel this frame, so the camera must not zoom; `rmb` = a right-button *tap*
//!   cancelled a gesture this frame (right drags always orbit the camera).
//! - [`HistoryRequest`]: the HUD's undo/redo buttons; handled here so a running gesture is
//!   cancelled first.

pub mod ops;

use crate::camera::SCROLL_PIXELS_PER_LINE;
use crate::render::{GameCamera, LowRes, cursor_ray};
use crate::world::{BuiltScene, Glade};
use bevy::{
    camera::visibility::RenderLayers,
    ecs::system::SystemParam,
    input::mouse::{AccumulatedMouseScroll, MouseScrollUnit},
    prelude::*,
    window::PrimaryWindow,
};
use glade_core::{
    BridgeKind, ClutterKind, Colors, Document, FenceKind, Footprint, Hanging, Material, PaintLayer, PathKind, PropKind,
    RoofStyle, Scene, StairKind, Structure, StructureId, TerrainDab, TerrainOp, TreeKind,
    geom::{closed, footprint_polygon},
    scene::{Hit, pick},
};
use ops::Style;
use std::f32::consts::{FRAC_PI_4, PI};

/// Render layer the tool gizmos (highlights, brush rings) are drawn on; added to the
/// [`GameCamera`] at startup so they land in the low-res image, not on the UI camera.
pub const GIZMO_LAYER: usize = 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Tool {
    #[default]
    Wall,
    House,
    Tower,
    Fence,
    Path,
    Stairs,
    Platform,
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
    Eraser,
}

impl Tool {
    pub const ALL: [Tool; 18] = [
        Tool::Wall,
        Tool::House,
        Tool::Tower,
        Tool::Fence,
        Tool::Path,
        Tool::Stairs,
        Tool::Platform,
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
        Tool::Eraser,
    ];

    /// ToolBar button label.
    pub fn label(self) -> &'static str {
        match self {
            Tool::Wall => "Wall",
            Tool::House => "House",
            Tool::Tower => "Tower",
            Tool::Fence => "Fence",
            Tool::Path => "Path",
            Tool::Stairs => "Stairs",
            Tool::Platform => "Deck",
            Tool::Bridge => "Bridge",
            Tool::Opening => "Win/Dr",
            Tool::Roof => "Roof",
            Tool::Tree => "Tree",
            Tool::Clutter => "Clutter",
            Tool::Paint => "Paint",
            Tool::Sculpt => "Up/Dn",
            Tool::Flatten => "Flat",
            Tool::Water => "Water",
            Tool::Style => "Style",
            Tool::Eraser => "Erase",
        }
    }

    /// Full name (tooltips, `--ui-tool`).
    pub fn name(self) -> &'static str {
        match self {
            Tool::Platform => "Platform",
            Tool::Opening => "Window/Door",
            Tool::Sculpt => "Raise/Lower",
            Tool::Flatten => "Flatten",
            Tool::Eraser => "Eraser",
            t => t.label(),
        }
    }

    pub fn parse(s: &str) -> Option<Tool> {
        let s = s.to_ascii_lowercase();
        Tool::ALL.into_iter().find(|t| {
            [t.label(), t.name(), &format!("{t:?}")].iter().any(|n| n.to_ascii_lowercase().replace('/', "") == s.replace('/', ""))
        })
    }

    pub fn key(self) -> KeyCode {
        match self {
            Tool::Wall => KeyCode::Digit1,
            Tool::House => KeyCode::Digit2,
            Tool::Tower => KeyCode::Digit3,
            Tool::Fence => KeyCode::Digit4,
            Tool::Path => KeyCode::Digit5,
            Tool::Stairs => KeyCode::Digit6,
            Tool::Platform => KeyCode::Digit7,
            Tool::Bridge => KeyCode::Digit8,
            Tool::Opening => KeyCode::Digit9,
            Tool::Roof => KeyCode::Digit0,
            Tool::Tree => KeyCode::KeyG,
            Tool::Clutter => KeyCode::KeyC,
            Tool::Paint => KeyCode::KeyP,
            Tool::Sculpt => KeyCode::KeyR,
            Tool::Flatten => KeyCode::KeyF,
            Tool::Water => KeyCode::KeyV,
            Tool::Style => KeyCode::KeyI,
            Tool::Eraser => KeyCode::KeyX,
        }
    }

    pub fn key_label(self) -> &'static str {
        match self {
            Tool::Wall => "1",
            Tool::House => "2",
            Tool::Tower => "3",
            Tool::Fence => "4",
            Tool::Path => "5",
            Tool::Stairs => "6",
            Tool::Platform => "7",
            Tool::Bridge => "8",
            Tool::Opening => "9",
            Tool::Roof => "0",
            Tool::Tree => "G",
            Tool::Clutter => "C",
            Tool::Paint => "P",
            Tool::Sculpt => "R",
            Tool::Flatten => "F",
            Tool::Water => "V",
            Tool::Style => "I",
            Tool::Eraser => "X",
        }
    }

    /// One-line tooltip description.
    pub fn blurb(self) -> &'static str {
        match self {
            Tool::Wall => "Drag a line of stone or wood",
            Tool::House => "Drag a footprint, then raise it",
            Tool::Tower => "Drag a circle, then raise it",
            Tool::Fence => "Drag a fence line",
            Tool::Path => "Drag a path; it cuts gates",
            Tool::Stairs => "Drag from low to high",
            Tool::Platform => "Drag a wooden deck",
            Tool::Bridge => "Drag across a gap",
            Tool::Opening => "Click a wall to add a window",
            Tool::Roof => "Click a wall ring to roof it",
            Tool::Tree => "Click to plant a tree",
            Tool::Clutter => "Click to place props",
            Tool::Paint => "Paint flowers, bushes, grain",
            Tool::Sculpt => "Raise the ground; Shift lowers",
            Tool::Flatten => "Level the ground",
            Tool::Water => "Carve ponds and streams",
            Tool::Style => "Click to copy a style",
            Tool::Eraser => "Click or drag to remove",
        }
    }

    /// ToolBar group: 0 Build, 1 Decorate, 2 Terrain, 3 Edit.
    pub fn group(self) -> u8 {
        match self {
            Tool::Wall | Tool::House | Tool::Tower | Tool::Fence | Tool::Path | Tool::Stairs | Tool::Platform | Tool::Bridge => 0,
            Tool::Opening | Tool::Roof | Tool::Tree | Tool::Clutter | Tool::Paint => 1,
            Tool::Sculpt | Tool::Flatten | Tool::Water => 2,
            Tool::Style | Tool::Eraser => 3,
        }
    }

    fn is_brush(self) -> bool {
        matches!(self, Tool::Paint | Tool::Sculpt | Tool::Flatten | Tool::Water)
    }
}

#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ActiveTool(pub Tool);

/// Interaction state shown by the world ghost and the HUD (see DESIGN §3).
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DrawState {
    #[default]
    Idle,
    Hover,
    Drawing,
    Raising,
    Invalid,
}

/// See the module docs.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ToolCapture {
    pub scroll: bool,
    pub rmb: bool,
}

/// Undo (`Some(false)`) / redo (`Some(true)`) requested by the HUD this frame.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct HistoryRequest(pub Option<bool>);

/// True while the cursor is over a HUD node (written by the HUD).
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct PointerOverUi(pub bool);

/// What the cursor is over, for the hints panel.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct ToolHover {
    /// Reason the current placement is invalid.
    pub invalid: Option<&'static str>,
    /// Name of the hovered structure (structure tools).
    pub target: Option<&'static str>,
    /// A gesture (stroke, drag, raise) is in progress.
    pub busy: bool,
    /// Polyline in click-to-add-points mode.
    pub clicking: bool,
}

/// House / tower variant settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HouseStyle {
    pub material: Material,
    pub roof: RoofStyle,
    pub roof_pitch: f32,
    pub half_timber: bool,
    pub battlements: bool,
    pub foundation: f32,
    /// Eave height used by the next placement (set in the Raising phase).
    pub height: f32,
}

/// Every variant the VariantStrip shows, plus the shared colours.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct ToolSettings {
    pub colors: Colors,
    pub wall_material: Material,
    pub wall_battlements: bool,
    pub wall_ruined: bool,
    pub wall_foundation: f32,
    pub wall_height: f32,
    pub house: HouseStyle,
    pub tower: HouseStyle,
    pub fence_kind: FenceKind,
    pub path_kind: PathKind,
    pub path_width: f32,
    pub stair_kind: StairKind,
    pub stair_width: f32,
    pub platform_height: f32,
    /// 0 rope, 1 stone, 2 wood.
    pub bridge_kind: u8,
    /// Rope railing design 0..=3 (shown 1–4).
    pub bridge_railing: u8,
    pub hanging: Hanging,
    pub bridge_width: f32,
    /// Roof tool.
    pub roof_style: RoofStyle,
    pub roof_pitch: f32,
    pub tree_kind: TreeKind,
    pub clutter_kind: ClutterKind,
    pub prop_angle: f32,
    pub prop_scale: f32,
    pub paint_layer: PaintLayer,
    pub brush_radius: f32,
    /// 0..1.
    pub brush_strength: f32,
}

impl Default for ToolSettings {
    fn default() -> Self {
        Self {
            colors: Colors::default(),
            wall_material: Material::Stone,
            wall_battlements: true,
            wall_ruined: false,
            wall_foundation: 0.4,
            wall_height: 4.0,
            house: HouseStyle {
                material: Material::Plaster,
                roof: RoofStyle::Gable,
                roof_pitch: 0.8,
                half_timber: true,
                battlements: false,
                foundation: 0.3,
                height: 4.0,
            },
            tower: HouseStyle {
                material: Material::Stone,
                roof: RoofStyle::Cone,
                roof_pitch: 1.4,
                half_timber: false,
                battlements: true,
                foundation: 0.5,
                height: 9.0,
            },
            fence_kind: FenceKind::WoodRail,
            path_kind: PathKind::Dirt,
            path_width: 1.6,
            stair_kind: StairKind::Wood,
            stair_width: 1.0,
            platform_height: 1.5,
            bridge_kind: 0,
            bridge_railing: 0,
            hanging: Hanging::None,
            bridge_width: 1.4,
            roof_style: RoofStyle::Gable,
            roof_pitch: 0.8,
            tree_kind: TreeKind::Oak,
            clutter_kind: ClutterKind::Barrel,
            prop_angle: 0.0,
            prop_scale: 1.0,
            paint_layer: PaintLayer::Flowers,
            brush_radius: 3.0,
            brush_strength: 0.5,
        }
    }
}

impl ToolSettings {
    pub fn bridge_kind(&self) -> BridgeKind {
        match self.bridge_kind {
            0 => BridgeKind::Rope { railing: self.bridge_railing.min(3) },
            1 => BridgeKind::Stone,
            _ => BridgeKind::Wood,
        }
    }

    /// Style the Style tool applies with Shift+click (and shows as swatches).
    pub fn style(&self) -> Style {
        Style { colors: self.colors, material: Some(self.house.material), half_timber: Some(self.house.half_timber) }
    }

    pub fn house_style(&self, tool: Tool) -> &HouseStyle {
        if tool == Tool::Tower { &self.tower } else { &self.house }
    }

    fn house_style_mut(&mut self, tool: Tool) -> &mut HouseStyle {
        if tool == Tool::Tower { &mut self.tower } else { &mut self.house }
    }

    fn dab_op(&self, tool: Tool, alt: bool, flatten_height: f32) -> TerrainOp {
        let k = self.brush_strength.clamp(0.0, 1.0);
        match tool {
            Tool::Sculpt => TerrainOp::Sculpt { amount: (0.1 + 0.5 * k) * if alt { -1.0 } else { 1.0 } },
            Tool::Flatten => TerrainOp::Flatten { height: flatten_height, strength: 0.3 + 0.7 * k },
            Tool::Water => TerrainOp::Water { depth: 0.5 + 1.5 * k },
            _ => TerrainOp::Paint { layer: self.paint_layer, strength: if alt { -1.0 } else { 1.0 } },
        }
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ToolSystems;

pub struct ToolsPlugin;

impl Plugin for ToolsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ActiveTool>()
            .init_resource::<ToolSettings>()
            .init_resource::<DrawState>()
            .init_resource::<ToolCapture>()
            .init_resource::<PointerOverUi>()
            .init_resource::<HistoryRequest>()
            .init_resource::<ToolHover>()
            .init_resource::<ToolState>()
            .add_systems(Startup, setup_gizmos)
            .add_systems(Update, (tool_hotkeys, run_tools, draw_gizmos).chain().in_set(ToolSystems));
    }
}

fn setup_gizmos(
    mut commands: Commands,
    mut store: ResMut<GizmoConfigStore>,
    cams: Query<(Entity, Option<&RenderLayers>), With<GameCamera>>,
) {
    let (config, _) = store.config_mut::<DefaultGizmoConfigGroup>();
    config.render_layers = RenderLayers::layer(GIZMO_LAYER);
    config.line.width = 2.0;
    config.depth_bias = -1.0;
    for (e, layers) in &cams {
        let layers = layers.cloned().unwrap_or_default().with(GIZMO_LAYER);
        commands.entity(e).insert(layers);
    }
}

fn tool_hotkeys(keys: Res<ButtonInput<KeyCode>>, mut tool: ResMut<ActiveTool>) {
    if keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::SuperLeft, KeyCode::SuperRight]) {
        return;
    }
    for t in Tool::ALL {
        if keys.just_pressed(t.key()) && tool.0 != t {
            tool.0 = t;
        }
    }
}

#[derive(Clone, Debug, Default)]
enum Gesture {
    #[default]
    None,
    /// Wall / fence / path polyline. `clicking`: click-to-add-points mode.
    Line { pts: Vec<Vec2>, clicking: bool, press_px: Vec2 },
    /// House / tower / platform footprint drag.
    Rect { a: Vec2, b: Vec2, angle: f32 },
    /// House / tower footprint fixed; mouse up/down sets the height.
    Raise { fp: Footprint, y0: f32, h0: f32 },
    /// Stairs / bridge from `a`.
    Span { a: Vec3 },
    /// Terrain / paint brush stroke.
    Stroke { last: Vec2, dabs: Vec<TerrainDab>, flatten_h: f32, still: f32 },
    /// Eraser drag.
    Erase { ids: Vec<StructureId> },
    /// Tree / clutter pressed: scroll rotates until release plants it.
    Place,
}

/// Internal tool state (gesture in progress, cursor results for the gizmos).
#[derive(Resource, Default)]
pub struct ToolState {
    gesture: Gesture,
    scroll_acc: f32,
    last_scroll: f32,
    rmb_press: Option<Vec2>,
    /// Last prop ghost (position, angle, scale, kind) to avoid rebuilding every frame.
    ghost: Option<(Vec2, f32, f32, PropKind)>,
    ground: Option<Vec3>,
    highlight: Option<(StructureId, Color)>,
    /// Ring drawn around the cursor (brushes) in metres.
    ring: Option<f32>,
    last_tool: Option<Tool>,
}

impl ToolState {
    fn busy(&self) -> bool {
        !matches!(self.gesture, Gesture::None)
    }
}

#[derive(SystemParam)]
struct Inputs<'w> {
    mouse: Res<'w, ButtonInput<MouseButton>>,
    keys: Res<'w, ButtonInput<KeyCode>>,
    scroll: Res<'w, AccumulatedMouseScroll>,
    time: Res<'w, Time>,
    over_ui: Res<'w, PointerOverUi>,
    history: ResMut<'w, HistoryRequest>,
}

const GHOST_OK: Color = Color::srgba(1.0, 1.0, 1.0, 0.8);
const ACCENT: Color = Color::srgb(0.753, 0.337, 0.227);
const INVALID: Color = Color::srgba(0.784, 0.290, 0.227, 0.9);

/// Camera yaw snapped to 45°, used as the axes of dragged rectangles.
fn camera_axes_angle(cam_tf: &GlobalTransform) -> f32 {
    let f = cam_tf.forward();
    let a = f.z.atan2(f.x);
    (a / FRAC_PI_4).round() * FRAC_PI_4
}

fn ground_under(scene: Option<&Scene>, ray: Ray3d) -> Option<Vec3> {
    let (o, d) = (ray.origin, *ray.direction);
    scene.and_then(|s| s.terrain.raycast(o, d)).or_else(|| {
        // No scene yet: the y = 0 plane.
        (d.y.abs() > 1e-5).then(|| o + d * (-o.y / d.y)).filter(|_| -o.y / d.y > 0.0)
    })
}

/// Height of the ground (or the water surface above it) at `p`.
fn base_height(scene: Option<&Scene>, p: Vec2) -> f32 {
    scene.map_or(0.0, |s| {
        let g = s.terrain.height(p);
        s.terrain.water_level(p).map_or(g, |w| w.max(g))
    })
}

#[allow(clippy::too_many_arguments)]
fn run_tools(
    mut input: Inputs,
    window: Query<&Window, With<PrimaryWindow>>,
    cam: Query<(&Camera, &GlobalTransform), With<GameCamera>>,
    low: Option<Res<LowRes>>,
    built: Res<BuiltScene>,
    glade: Option<ResMut<Glade>>,
    tool: Res<ActiveTool>,
    mut settings: ResMut<ToolSettings>,
    mut draw: ResMut<DrawState>,
    mut capture: ResMut<ToolCapture>,
    mut hover: ResMut<ToolHover>,
    mut st: ResMut<ToolState>,
) {
    let Some(mut glade) = glade else { return };
    let Ok(window) = window.single() else { return };
    let now = input.time.elapsed_secs();
    let over_ui = input.over_ui.0;
    let keys = &input.keys;
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::SuperLeft, KeyCode::SuperRight]);
    let alt = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    let tool = tool.0;

    let mut next_capture = ToolCapture { scroll: over_ui, rmb: false };
    let mut next_hover = ToolHover::default();
    st.highlight = None;
    st.ring = None;

    // Wheel → whole steps (same notch size as the camera); partial trackpad scrolls
    // accumulate and are forgotten after a short pause.
    let raw = match input.scroll.unit {
        MouseScrollUnit::Line => input.scroll.delta.y,
        MouseScrollUnit::Pixel => input.scroll.delta.y / SCROLL_PIXELS_PER_LINE,
    };
    if raw != 0.0 {
        st.last_scroll = now;
    } else if now - st.last_scroll > 0.3 {
        st.scroll_acc = 0.0;
    }
    st.scroll_acc += raw;
    let steps = st.scroll_acc.trunc();
    st.scroll_acc -= steps;

    let cancel = |st: &mut ToolState, glade: &mut Glade| {
        st.gesture = Gesture::None;
        st.ghost = None;
        glade.set_preview(None);
    };

    if tool_changed(&mut st, tool) {
        cancel(&mut st, &mut glade);
    }

    // Undo / redo (keys or HUD buttons).
    let mut history = input.history.0.take();
    if ctrl && (keys.just_pressed(KeyCode::KeyZ) || keys.just_pressed(KeyCode::KeyY)) {
        history = Some(keys.just_pressed(KeyCode::KeyY) || shift);
    }
    if let Some(redo) = history {
        cancel(&mut st, &mut glade);
        if redo {
            glade.redo();
        } else {
            glade.undo();
        }
    }
    if keys.just_pressed(KeyCode::Escape) {
        cancel(&mut st, &mut glade);
    }
    let mouse = &input.mouse;
    // Right-button tap cancels; a drag is the camera's orbit.
    if mouse.just_pressed(MouseButton::Right) {
        st.rmb_press = window.cursor_position();
    }
    if mouse.just_released(MouseButton::Right) {
        let tap = st.rmb_press.zip(window.cursor_position()).is_some_and(|(a, b)| a.distance(b) < 6.0);
        if tap && st.busy() {
            cancel(&mut st, &mut glade);
            next_capture.rmb = true;
        }
        st.rmb_press = None;
    }

    // Cursor → world.
    let scene = built.0.clone();
    let scene = scene.as_deref();
    let cursor = window.cursor_position();
    let ray = match (cursor, cam.single(), &low) {
        (Some(c), Ok((camera, cam_tf)), Some(low)) if !over_ui => cursor_ray(c, window, camera, cam_tf, low.size),
        _ => None,
    };
    let ground = ray.and_then(|r| ground_under(scene, r));
    st.ground = ground;
    let ground2 = ground.map(|g| g.xz());
    let cam_angle = cam.single().map_or(0.0, |(_, tf)| camera_axes_angle(tf));
    let needs_pick = matches!(tool, Tool::Roof | Tool::Opening | Tool::Style | Tool::Eraser | Tool::Stairs | Tool::Bridge);
    let hit: Option<Hit> = if needs_pick {
        ray.zip(scene).and_then(|(r, s)| pick(glade.doc(), s, r.origin, *r.direction))
    } else {
        None
    };

    let lmb_down = mouse.just_pressed(MouseButton::Left) && !over_ui && cursor.is_some();
    let lmb_up = mouse.just_released(MouseButton::Left);
    let lmb_held = mouse.pressed(MouseButton::Left);
    let base = glade.committed();
    let mut state = if ground.is_some() { DrawState::Hover } else { DrawState::Idle };
    let mut preview: Option<Option<Document>> = None; // Some(x) → set_preview(x)
    let mut commit: Option<Document> = None;

    match tool {
        Tool::Wall | Tool::Fence | Tool::Path => {
            let apply = |pts: &[Vec2], s: &ToolSettings| match tool {
                Tool::Wall => ops::apply_wall_stroke(base, pts, s),
                Tool::Fence => ops::apply_fence_stroke(base, pts, s),
                _ => ops::apply_path_stroke(base, pts, s),
            };
            let gesture = std::mem::take(&mut st.gesture);
            st.gesture = match gesture {
                Gesture::None => {
                    if let Some(p) = ground2 {
                        st.ring = Some(0.4);
                        if lmb_down {
                            Gesture::Line { pts: vec![ops::clamp_to_glade(p)], clicking: false, press_px: cursor.unwrap_or_default() }
                        } else {
                            Gesture::None
                        }
                    } else {
                        Gesture::None
                    }
                }
                Gesture::Line { mut pts, mut clicking, press_px } => {
                    state = DrawState::Drawing;
                    next_capture.scroll = true;
                    if steps != 0.0 {
                        match tool {
                            Tool::Wall => settings.wall_height = (settings.wall_height + steps * 0.5).clamp(1.5, 12.0),
                            Tool::Path => settings.path_width = (settings.path_width + steps * 0.2).clamp(0.8, 4.0),
                            _ => settings.fence_kind = cycle(settings.fence_kind, &FENCES, steps),
                        }
                    }
                    let last = *pts.last().unwrap();
                    let mut cur = ground2.map_or(last, ops::clamp_to_glade);
                    if shift {
                        cur = ops::clamp_to_glade(ops::snap_45(last, cur));
                        if tool == Tool::Wall && pts.len() >= 3 && cur.distance(pts[0]) <= ops::CLOSE_DIST * 2.0 {
                            cur = pts[0];
                        }
                    }
                    let mut finish = false;
                    if !clicking && lmb_held && cur.distance(last) >= ops::POINT_SPACING {
                        pts.push(cur);
                    }
                    if !clicking && lmb_up {
                        let moved = cursor.is_some_and(|c| c.distance(press_px) > 6.0);
                        if pts.len() == 1 && !moved {
                            clicking = true;
                        } else {
                            finish = true;
                        }
                    } else if clicking && lmb_down {
                        if cur.distance(last) <= ops::POINT_SPACING
                            || (tool == Tool::Wall && pts.len() >= 3 && cur.distance(pts[0]) <= ops::CLOSE_DIST)
                        {
                            finish = true;
                        } else {
                            pts.push(cur);
                        }
                    }
                    let mut all = pts.clone();
                    if cur.distance(*all.last().unwrap()) > 0.05 {
                        all.push(cur);
                    }
                    let doc = apply(&all, &settings);
                    next_hover.clicking = clicking;
                    let started = pts.len() >= 2 || cur.distance(pts[0]) >= 0.5;
                    if doc.is_none() && started {
                        next_hover.invalid = Some(ops::TOO_SHORT);
                        state = DrawState::Invalid;
                    }
                    if finish {
                        commit = doc;
                        preview = Some(None);
                        Gesture::None
                    } else {
                        preview = Some(doc);
                        Gesture::Line { pts, clicking, press_px }
                    }
                }
                other => other,
            };
        }

        Tool::House | Tool::Tower | Tool::Platform => {
            let round = tool == Tool::Tower;
            let gesture = std::mem::take(&mut st.gesture);
            st.gesture = match gesture {
                Gesture::None => match ground2 {
                    Some(p) if !ops::inside_glade(p) => {
                        state = DrawState::Invalid;
                        next_hover.invalid = Some(ops::OUTSIDE);
                        Gesture::None
                    }
                    Some(p) => {
                        st.ring = Some(0.4);
                        if lmb_down { Gesture::Rect { a: p, b: p, angle: cam_angle } } else { Gesture::None }
                    }
                    None => Gesture::None,
                },
                Gesture::Rect { a, b, mut angle } => {
                    state = DrawState::Drawing;
                    next_capture.scroll = true;
                    let b = ground2.unwrap_or(b);
                    if steps != 0.0 {
                        if tool == Tool::Platform {
                            settings.platform_height = (settings.platform_height + steps * 0.25).clamp(0.5, 8.0);
                        } else {
                            angle += steps * PI / 12.0;
                        }
                    }
                    let fp = ops::footprint_from_drag(a, b, angle, round, shift);
                    let doc = if tool == Tool::Platform {
                        ops::apply_platform(base, a, b, angle, shift, &settings)
                    } else {
                        fp.and_then(|fp| ops::apply_house(base, fp, settings.house_style(tool), settings.colors))
                    };
                    if doc.is_none() && a.distance(b) >= 0.5 {
                        state = DrawState::Invalid;
                        let outside = fp.is_some_and(|fp| !footprint_polygon(&fp).iter().all(|p| ops::inside_glade(*p)));
                        next_hover.invalid = Some(if outside { ops::OUTSIDE } else { ops::TOO_SHORT });
                    }
                    if lmb_up {
                        match (tool, fp, &doc) {
                            (Tool::Platform, _, _) => {
                                commit = doc;
                                preview = Some(None);
                                Gesture::None
                            }
                            (_, Some(fp), Some(_)) => {
                                preview = Some(doc);
                                let y0 = cursor.map_or(0.0, |c| c.y);
                                Gesture::Raise { fp, y0, h0: settings.house_style(tool).height }
                            }
                            _ => {
                                preview = Some(None);
                                Gesture::None
                            }
                        }
                    } else {
                        preview = Some(doc);
                        Gesture::Rect { a, b, angle }
                    }
                }
                Gesture::Raise { fp, y0, h0 } => {
                    state = DrawState::Raising;
                    next_capture.scroll = true;
                    let hs = settings.house_style_mut(tool);
                    if let Some(c) = cursor {
                        hs.height = (h0 + (y0 - c.y) * 0.04).clamp(0.6, 24.0);
                    }
                    if steps != 0.0 {
                        if shift {
                            hs.roof = ops::next_roof(hs.roof);
                        } else {
                            hs.roof_pitch = (hs.roof_pitch + steps * 0.1).clamp(0.0, 2.5);
                        }
                    }
                    let doc = ops::apply_house(base, fp, settings.house_style(tool), settings.colors);
                    if lmb_down {
                        commit = doc;
                        preview = Some(None);
                        Gesture::None
                    } else {
                        preview = Some(doc);
                        Gesture::Raise { fp, y0, h0 }
                    }
                }
                other => other,
            };
        }

        Tool::Stairs | Tool::Bridge => {
            let new_id = base.next_id;
            let point = |h: Option<Hit>| -> Option<Vec3> {
                match h {
                    Some(h) if h.id != Some(new_id) => Some(h.point),
                    _ => ground,
                }
            };
            let snap = |p: Vec3| -> Vec3 {
                match (shift, ops::wall_top_near(base, p.xz(), 2.0)) {
                    (true, Some((_, h))) => Vec3::new(p.x, base_height(scene, p.xz()) + h, p.z),
                    _ => p,
                }
            };
            let gesture = std::mem::take(&mut st.gesture);
            st.gesture = match gesture {
                Gesture::None => {
                    if point(hit).is_some() {
                        st.ring = Some(0.4);
                    }
                    match point(hit) {
                        Some(p) if lmb_down => Gesture::Span { a: snap(p) },
                        _ => Gesture::None,
                    }
                }
                Gesture::Span { a } => {
                    state = DrawState::Drawing;
                    next_capture.scroll = true;
                    if steps != 0.0 {
                        let w = if tool == Tool::Stairs { &mut settings.stair_width } else { &mut settings.bridge_width };
                        *w = (*w + steps * 0.1).clamp(0.5, 3.0);
                    }
                    let a = snap(a);
                    let doc = point(hit).map(snap).and_then(|b| {
                        if tool == Tool::Stairs {
                            ops::apply_stairs(base, a, b, &settings)
                        } else {
                            let (ba, bb) = (base_height(scene, a.xz()), base_height(scene, b.xz()));
                            ops::apply_bridge(base, a, b, ba, bb, &settings)
                        }
                    });
                    let moved = point(hit).is_some_and(|b| b.distance(a) >= 0.5);
                    if doc.is_none() && moved {
                        state = DrawState::Invalid;
                        next_hover.invalid = Some(ops::TOO_SHORT);
                    }
                    if lmb_up {
                        commit = doc;
                        preview = Some(None);
                        Gesture::None
                    } else {
                        preview = Some(doc);
                        Gesture::Span { a }
                    }
                }
                other => other,
            };
        }

        t if t.is_brush() => {
            st.ring = ground.map(|_| settings.brush_radius);
            let gesture = std::mem::take(&mut st.gesture);
            let dab = |c: Vec2, s: &ToolSettings, fh: f32| TerrainDab { center: c, radius: s.brush_radius, op: s.dab_op(t, shift, fh) };
            st.gesture = match gesture {
                Gesture::None => match ground2 {
                    Some(p) if lmb_down => {
                        let fh = base_height(scene, p).min(scene.map_or(0.0, |s| s.terrain.height(p)));
                        let dabs = vec![dab(p, &settings, fh)];
                        preview = Some(ops::apply_dabs(base, &dabs));
                        Gesture::Stroke { last: p, dabs, flatten_h: fh, still: 0.0 }
                    }
                    _ => Gesture::None,
                },
                Gesture::Stroke { mut last, mut dabs, flatten_h, mut still } => {
                    state = DrawState::Drawing;
                    next_capture.scroll = true;
                    if steps != 0.0 {
                        settings.brush_radius = (settings.brush_radius + steps * 0.5).clamp(1.0, 8.0);
                    }
                    let n0 = dabs.len();
                    if let Some(cur) = ground2 {
                        let (pts, l) = ops::dabs_between(last, cur, ops::DAB_SPACING);
                        last = l;
                        dabs.extend(pts.into_iter().map(|c| dab(c, &settings, flatten_h)));
                        // Holding still keeps raising / carving.
                        still += input.time.delta_secs();
                        if dabs.len() > n0 {
                            still = 0.0;
                        } else if still > 0.15 && matches!(t, Tool::Sculpt | Tool::Water) {
                            still = 0.0;
                            dabs.push(dab(cur, &settings, flatten_h));
                        }
                    }
                    if lmb_up || !lmb_held {
                        commit = ops::apply_dabs(base, &dabs);
                        preview = Some(None);
                        Gesture::None
                    } else {
                        if dabs.len() > n0 {
                            preview = Some(ops::apply_dabs(base, &dabs));
                        }
                        Gesture::Stroke { last, dabs, flatten_h, still }
                    }
                }
                other => other,
            };
        }

        Tool::Tree | Tool::Clutter => {
            let kind = if tool == Tool::Tree {
                PropKind::Tree(settings.tree_kind)
            } else {
                PropKind::Clutter(settings.clutter_kind)
            };
            let held = matches!(st.gesture, Gesture::Place);
            if held && !lmb_held && !lmb_up {
                st.gesture = Gesture::None;
            }
            match ground2 {
                Some(p) if ops::inside_glade(p) => {
                    // Alt+scroll (or scroll while the button is held) rotates; plain scroll zooms.
                    if alt || held {
                        next_capture.scroll = true;
                        if steps != 0.0 {
                            if shift {
                                settings.prop_scale = (settings.prop_scale + steps * 0.05).clamp(0.7, 1.3);
                            } else {
                                settings.prop_angle = (settings.prop_angle + steps * PI / 12.0).rem_euclid(PI * 2.0);
                            }
                        }
                    }
                    let g = (p, settings.prop_angle, settings.prop_scale, kind);
                    let doc = || ops::apply_placed(base, p, g.1, g.2, kind, settings.colors);
                    if lmb_down {
                        st.gesture = Gesture::Place;
                    }
                    if held && lmb_up {
                        st.gesture = Gesture::None;
                        commit = doc();
                        preview = Some(None);
                        st.ghost = None;
                    } else if st.ghost.is_none_or(|o| o.0.distance(p) > 0.15 || o.1 != g.1 || o.2 != g.2 || o.3 != g.3) {
                        st.ghost = Some(g);
                        preview = Some(doc());
                    }
                }
                Some(_) => {
                    state = DrawState::Invalid;
                    next_hover.invalid = Some(ops::OUTSIDE);
                    st.ghost = None;
                    preview = Some(None);
                    if lmb_up {
                        st.gesture = Gesture::None;
                    }
                }
                None => {
                    st.ghost = None;
                    preview = Some(None);
                    if lmb_up {
                        st.gesture = Gesture::None;
                    }
                }
            }
        }

        Tool::Roof | Tool::Opening | Tool::Style | Tool::Eraser => {
            let doc_now = base;
            let target = hit.and_then(|h| h.id.and_then(|id| doc_now.get(id).map(|s| (h, id, s.clone()))));
            next_hover.target = target.as_ref().map(|(_, _, s)| ops::structure_name(s));
            match tool {
                Tool::Roof => {
                    if let Some((_, id, s)) = &target {
                        if ops::roofable(s) {
                            st.highlight = Some((*id, GHOST_OK));
                            // Alt+scroll: pitch (Alt+Shift: style); plain scroll zooms.
                            next_capture.scroll = alt;
                            if alt
                                && steps != 0.0
                                && let Some(d) = ops::adjust_roof(doc_now, *id, steps * 0.1, shift)
                            {
                                commit = Some(d);
                            }
                            if lmb_down
                                && let Some(d) = ops::apply_roof_click(
                                    doc_now,
                                    *id,
                                    settings.roof_style,
                                    settings.roof_pitch,
                                    settings.colors.roof,
                                )
                            {
                                commit = Some(d);
                            }
                        } else {
                            state = DrawState::Invalid;
                            next_hover.invalid = Some(ops::NEEDS_WALL);
                            st.highlight = Some((*id, INVALID));
                        }
                    }
                }
                Tool::Opening => {
                    if let Some((h, id, s)) = &target {
                        if ops::openable(s) {
                            st.highlight = Some((*id, GHOST_OK));
                            if lmb_down {
                                commit = ops::apply_opening_click(doc_now, *id, h.along, shift);
                            }
                        } else {
                            state = DrawState::Invalid;
                            next_hover.invalid = Some(ops::NEEDS_WALL);
                        }
                    }
                }
                Tool::Style => {
                    if let Some((_, id, s)) = &target {
                        st.highlight = Some((*id, ACCENT));
                        if lmb_down {
                            if shift {
                                commit = ops::apply_style(doc_now, *id, settings.style());
                            } else if let Some(style) = ops::style_of(s) {
                                ops::copy_style(&mut settings, style);
                            }
                        }
                    }
                }
                _ => {
                    // Eraser.
                    if let Some((_, id, _)) = &target {
                        st.highlight = Some((*id, INVALID));
                    }
                    let gesture = std::mem::take(&mut st.gesture);
                    st.gesture = match gesture {
                        Gesture::None if lmb_down => {
                            let ids: Vec<StructureId> = target.iter().map(|t| t.1).collect();
                            preview = Some(ops::erase(base, &ids));
                            Gesture::Erase { ids }
                        }
                        Gesture::Erase { mut ids } => {
                            state = DrawState::Drawing;
                            if let Some((_, id, _)) = &target
                                && !ids.contains(id)
                            {
                                ids.push(*id);
                                preview = Some(ops::erase(base, &ids));
                            }
                            if lmb_up || !lmb_held {
                                commit = ops::erase(base, &ids);
                                preview = Some(None);
                                Gesture::None
                            } else {
                                Gesture::Erase { ids }
                            }
                        }
                        other => other,
                    };
                }
            }
        }
        _ => {}
    }

    if let Some(doc) = commit {
        glade.commit(doc);
    } else if let Some(p) = preview {
        glade.set_preview(p);
    }
    if over_ui && !st.busy() {
        state = DrawState::Idle;
    }
    next_hover.busy = st.busy() && !matches!(st.gesture, Gesture::Place);
    if *capture != next_capture {
        *capture = next_capture;
    }
    if *hover != next_hover {
        *hover = next_hover;
    }
    if *draw != state {
        *draw = state;
    }
}

const FENCES: [FenceKind; 4] = [FenceKind::WoodRail, FenceKind::Picket, FenceKind::Stone, FenceKind::Wattle];

fn cycle<T: Copy + PartialEq>(cur: T, all: &[T], steps: f32) -> T {
    let i = all.iter().position(|x| *x == cur).unwrap_or(0) as i32;
    all[(i + steps as i32).rem_euclid(all.len() as i32) as usize]
}

/// True when `tool` differs from the one seen last frame.
fn tool_changed(st: &mut ToolState, tool: Tool) -> bool {
    let changed = st.last_tool.is_some_and(|t| t != tool);
    st.last_tool = Some(tool);
    changed
}

/// Outlines of the hovered structure, the brush ring and the first-point marker, drawn on
/// [`GIZMO_LAYER`] into the low-res image.
fn draw_gizmos(mut gizmos: Gizmos, st: Res<ToolState>, built: Res<BuiltScene>, glade: Option<Res<Glade>>, draw: Res<DrawState>) {
    let scene = built.0.as_deref();
    let h = |p: Vec2| scene.map_or(0.0, |s| s.terrain.height(p));
    let ring = |gizmos: &mut Gizmos, c: Vec2, r: f32, color: Color| {
        let n = (r * 8.0).clamp(12.0, 48.0) as usize;
        gizmos.linestrip(
            (0..=n).map(|i| {
                let p = c + Vec2::from_angle(i as f32 / n as f32 * PI * 2.0) * r;
                Vec3::new(p.x, h(p) + 0.08, p.y)
            }),
            color,
        );
    };
    if let (Some(g), Some(r)) = (st.ground, st.ring) {
        let color = if *draw == DrawState::Invalid { INVALID } else { GHOST_OK };
        ring(&mut gizmos, g.xz(), r, color);
    }
    let (Some((id, color)), Some(glade)) = (st.highlight, glade) else { return };
    let Some(s) = glade.doc().get(id) else { return };
    let lift = |pts: &[Vec2], dy: f32| -> Vec<Vec3> { pts.iter().map(|p| Vec3::new(p.x, h(*p) + dy, p.y)).collect() };
    match s {
        Structure::Wall(w) => {
            gizmos.linestrip(lift(&w.points, 0.1), color);
            gizmos.linestrip(lift(&w.points, w.height + 0.1), color);
        }
        Structure::Fence(f) => gizmos.linestrip(lift(&f.points, 1.2), color),
        Structure::Path(p) => gizmos.linestrip(lift(&p.points, 0.1), color),
        Structure::House(hs) => {
            let ring = closed(&footprint_polygon(&hs.footprint));
            gizmos.linestrip(lift(&ring, 0.1), color);
            gizmos.linestrip(lift(&ring, hs.wall_height + 0.1), color);
        }
        Structure::Platform(p) => {
            let ring = closed(&footprint_polygon(&Footprint::Rect { center: p.center, half: p.half, angle: p.angle }));
            gizmos.linestrip(lift(&ring, p.height + 0.1), color);
        }
        Structure::Stairs(s) => {
            gizmos.line(s.from, s.to, color);
        }
        Structure::Bridge(b) => {
            gizmos.line(b.a, b.b, color);
        }
        Structure::Placed(p) => ring(&mut gizmos, p.at, 1.2 * p.scale, color),
    }
}
