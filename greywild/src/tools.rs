//! Drawing tools: turn pointer input into document edits.
//!
//! Contract shared with the HUD (`ui`) and the camera:
//! - [`ToolSettings`]: the selected tool and every variant knob. The HUD writes it, tools read it.
//!   Hotkeys (see [`Tool::hotkey`]) are handled here and also write it.
//! - [`ToolStatus`]: written by tools every frame for the HUD (hint line, invalid reason).
//! - [`PointerOverUi`]: set by the HUD each frame when the cursor is over a HUD node; tools and
//!   camera ignore pointer input (clicks, drags, scroll) while it is true.
//! - Undo / redo go straight to [`crate::world::Glade`].

use crate::camera::{CameraInput, CameraRig};
use crate::render::{GameCamera, LowRes, RenderTest, VIEW_HEIGHT, cursor_ray};
use crate::world::{BuiltScene, Glade};
use bevy::{
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit},
    prelude::*,
    window::PrimaryWindow,
};
use greywild_core::geom::{contains, is_closed, polyline_length};
use greywild_core::scene::{Hit, pick};
use greywild_core::terrain::Terrain;
use greywild_core::{
    Bridge, BridgeKind, ClutterKind, Colors, Document, Fence, FenceKind, Footprint, GLADE_HALF, Hanging, House,
    Material, Opening, OpeningKind, PaintLayer, Path, PathKind, Placed, Platform, PropKind, RoofStyle, Scene,
    StairKind, Stairs, Structure, StructureId, TerrainDab, TerrainOp, TreeKind, Wall,
};
use std::f32::consts::{FRAC_PI_4, TAU};

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
    /// Wall height in metres, 2..=14 (scroll while drawing a wall).
    pub wall_height: f32,
    /// Stair width in metres, 0.6..=2.4 (scroll while dragging stairs).
    pub stair_width: f32,
    /// Bridge deck width in metres, 1..=3 (scroll while dragging a bridge).
    pub bridge_width: f32,
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
            wall_height: 4.0,
            stair_width: 1.2,
            bridge_width: 1.6,
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
        app.init_resource::<ToolSettings>()
            .init_resource::<ToolStatus>()
            .init_resource::<PointerOverUi>()
            .init_resource::<ToolState>()
            .add_systems(Startup, thin_gizmos)
            .add_systems(Update, drive_tools.run_if(not(resource_exists::<RenderTest>)));
        smoke::install(app);
    }
}

mod smoke;

/// Test hook (`--tool-smoke`): replaces the OS cursor with the screen position of a world
/// point (plus a pixel offset), so the real ray / gesture code runs on synthetic input.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct CursorOverride {
    pub target: Option<Vec3>,
    /// Window-pixel offset added after projecting `target`.
    pub px: Vec2,
}

// =============================================================================================
// Tuning
// =============================================================================================

/// A dragged polyline gets a new point every this many metres.
const POINT_SPACING: f32 = 1.5;
/// A wall/fence ending this close to its start snaps closed.
const CLOSE_SNAP: f32 = 1.5;
/// Polyline simplification tolerance (metres).
const SIMPLIFY_EPS: f32 = 0.3;
/// Shortest wall / fence / path.
const MIN_LINE: f32 = 1.0;
/// Clicking this close to an existing opening upgrades it.
const OPENING_GRAB: f32 = 0.8;
/// Shortest bridge.
const MIN_BRIDGE: f32 = 3.0;
/// Steepest rise / run for non-ladder stairs (~51°).
const MAX_STAIR_SLOPE: f32 = 1.25;
/// Default raise heights.
const BLOCK_HEIGHT: f32 = 5.0;
const TOWER_HEIGHT: f32 = 12.0;
const DECK_HEIGHT: f32 = 1.5;
/// An RMB press that moves less than this (px) is a tap = cancel.
const RMB_TAP_PX: f32 = 6.0;

const OUTSIDE: &str = "OUTSIDE THE VALLEY";
const TOO_SHORT: &str = "TOO SHORT";
const TOO_STEEP: &str = "TOO STEEP";
const NEEDS_ANCHORS: &str = "NEEDS TWO ANCHORS";
const NEEDS_WALL: &str = "NEEDS A WALL";
const NEEDS_RING: &str = "NEEDS A CLOSED WALL RING";

// =============================================================================================
// Pure helpers (unit-tested)
// =============================================================================================

/// Distance from `p` to segment `a`–`b` (handles a zero-length segment).
fn seg_dist(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let l2 = ab.length_squared();
    let t = if l2 < 1e-12 { 0.0 } else { ((p - a).dot(ab) / l2).clamp(0.0, 1.0) };
    p.distance(a + ab * t)
}

/// Ramer–Douglas–Peucker simplification. Keeps both end points (so a closed ring stays closed)
/// and drops near-duplicate consecutive points.
pub fn simplify_polyline(pts: &[Vec2], eps: f32) -> Vec<Vec2> {
    let mut v: Vec<Vec2> = Vec::with_capacity(pts.len());
    for (i, p) in pts.iter().enumerate() {
        let last = i + 1 == pts.len();
        match v.last() {
            Some(q) if q.distance(*p) < 0.05 => {
                if last && v.len() > 1 {
                    *v.last_mut().unwrap() = *p;
                }
            }
            _ => v.push(*p),
        }
    }
    let n = v.len();
    if n < 3 {
        return v;
    }
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[n - 1] = true;
    let mut stack = vec![(0, n - 1)];
    while let Some((i, j)) = stack.pop() {
        let (mut best, mut k) = (0.0, 0);
        for m in i + 1..j {
            let d = seg_dist(v[m], v[i], v[j]);
            if d > best {
                best = d;
                k = m;
            }
        }
        if best > eps {
            keep[k] = true;
            stack.push((i, k));
            stack.push((k, j));
        }
    }
    v.into_iter().zip(keep).filter_map(|(p, k)| k.then_some(p)).collect()
}

/// Oriented rectangle spanned by the drag diagonal `a`→`b` in a frame rotated by `angle`
/// (radians, `Vec2::from_angle`). `square` forces equal sides (anchored at `a`).
/// Returns (centre, half extents).
pub fn rect_from_drag(a: Vec2, b: Vec2, angle: f32, square: bool) -> (Vec2, Vec2) {
    let ax = Vec2::from_angle(angle);
    let az = ax.perp();
    let d = b - a;
    let mut l = Vec2::new(d.dot(ax), d.dot(az));
    if square {
        let m = l.x.abs().max(l.y.abs());
        l = Vec2::new(m.copysign(l.x), m.copysign(l.y));
    }
    let center = a + ax * (l.x * 0.5) + az * (l.y * 0.5);
    (center, l.abs() * 0.5)
}

/// Brush dab centres from `last` (already dabbed) towards `cur`, every `spacing` metres.
/// The last returned point is the new `last`.
pub fn dab_points(last: Vec2, cur: Vec2, spacing: f32) -> Vec<Vec2> {
    let spacing = spacing.max(0.05);
    let d = cur.distance(last);
    let n = (d / spacing).floor() as usize;
    let dir = (cur - last).normalize_or_zero();
    (1..=n.min(512)).map(|k| last + dir * (spacing * k as f32)).collect()
}

/// Window → door → wide portal → window.
pub fn next_opening(k: OpeningKind) -> OpeningKind {
    match k {
        OpeningKind::Window => OpeningKind::Door,
        OpeningKind::Door => OpeningKind::WideDoor,
        OpeningKind::WideDoor => OpeningKind::Window,
    }
}

/// Ground point under a world ray (terrain heightfield march + bisection).
pub fn ground_hit(terrain: &Terrain, origin: Vec3, dir: Vec3) -> Option<Vec3> {
    terrain.raycast(origin, dir)
}

/// `p` moved along the 45° direction from `from` closest to it.
fn snap45(from: Vec2, p: Vec2) -> Vec2 {
    let d = p - from;
    if d.length_squared() < 1e-8 {
        return p;
    }
    let a = (d.y.atan2(d.x) / FRAC_PI_4).round() * FRAC_PI_4;
    let dir = Vec2::from_angle(a);
    from + dir * d.dot(dir).max(0.0)
}

/// True if `end` should snap onto the start of `pts` (closing the ring).
fn close_snap(pts: &[Vec2], end: Vec2) -> bool {
    pts.len() >= 3 && end.distance(pts[0]) <= CLOSE_SNAP && polyline_length(pts) >= 3.0 * CLOSE_SNAP
}

/// Deterministic 0..1 hash of a position.
fn hash01(p: Vec2, salt: u32) -> f32 {
    let x = (p.x * 100.0).round() as i32 as u32;
    let z = (p.y * 100.0).round() as i32 as u32;
    let mut h = x.wrapping_mul(0x9E37_79B1) ^ z.wrapping_mul(0x85EB_CA77) ^ salt.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h & 0xFF_FFFF) as f32 / 16_777_216.0
}

/// Quantise to 0.1 m (stable previews, fewer rebuilds).
fn q(v: f32) -> f32 {
    (v * 10.0).round() / 10.0
}
fn q2(v: Vec2) -> Vec2 {
    Vec2::new(q(v.x), q(v.y))
}

fn inside_valley(p: Vec2) -> bool {
    p.abs().max_element() <= GLADE_HALF
}

/// Roof pitch fraction (model) from degrees (settings) and back.
fn pitch_frac(deg: f32) -> f32 {
    deg.clamp(15.0, 60.0).to_radians().tan()
}
fn pitch_deg(frac: f32) -> f32 {
    frac.max(0.0).atan().to_degrees().clamp(15.0, 60.0)
}

fn cycle_roof(r: RoofStyle, dir: i32, round: bool) -> RoofStyle {
    let all: &[RoofStyle] = if round {
        &[RoofStyle::Shell, RoofStyle::Green]
    } else {
        &[RoofStyle::Pitched, RoofStyle::Sawtooth, RoofStyle::Shell, RoofStyle::Green]
    };
    let i = all.iter().position(|x| *x == r).unwrap_or(0) as i32;
    all[(i + dir).rem_euclid(all.len() as i32) as usize]
}

fn cycle_fence(k: FenceKind, dir: i32) -> FenceKind {
    let all = [FenceKind::Railing, FenceKind::Balustrade, FenceKind::Gabion, FenceKind::Hedge];
    let i = all.iter().position(|x| *x == k).unwrap_or(0) as i32;
    all[(i + dir).rem_euclid(4) as usize]
}

/// Document plus one structure.
fn with_structure(doc: &Document, s: Structure) -> Document {
    let mut d = doc.clone();
    d.add(s);
    d
}

// --- structure builders from settings -------------------------------------------------------

fn line_structure(tool: Tool, s: &ToolSettings, points: Vec<Vec2>) -> Structure {
    match tool {
        Tool::Fence => Structure::Fence(Fence { points, kind: s.fence, colors: s.colors }),
        Tool::Path => Structure::Path(Path { points, width: s.path_width, kind: s.path }),
        _ => Structure::Wall(Wall {
            points,
            height: s.wall_height,
            thickness: s.wall_thickness,
            material: s.material,
            planters: s.planters,
            ruined: s.ruined,
            foundation: s.plinth,
            roofed: false,
            roof: s.roof,
            roof_pitch: pitch_frac(s.roof_pitch_deg),
            openings: Vec::new(),
            colors: s.colors,
            overgrowth: s.overgrowth,
        }),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Shape {
    Rect { center: Vec2, half: Vec2, angle: f32 },
    Circle { center: Vec2, radius: f32 },
}

impl Shape {
    fn invalid(&self) -> Option<&'static str> {
        match *self {
            Shape::Rect { center, half, angle } => {
                if half.min_element() < 0.75 {
                    return Some(TOO_SHORT);
                }
                let poly = greywild_core::geom::footprint_polygon(&Footprint::Rect { center, half, angle });
                (!poly.iter().all(|p| inside_valley(*p)) || half.max_element() > 16.0).then_some(OUTSIDE)
            }
            Shape::Circle { center, radius } => {
                if radius < 1.0 {
                    return Some(TOO_SHORT);
                }
                (!inside_valley(center.abs() + Vec2::splat(radius)) || radius > 10.0).then_some(OUTSIDE)
            }
        }
    }
}

fn shape_structure(tool: Tool, s: &ToolSettings, shape: Shape, height: f32) -> Structure {
    match (tool, shape) {
        (Tool::Deck, Shape::Rect { center, half, angle }) => {
            Structure::Platform(Platform { center, half, angle, height, colors: s.colors })
        }
        (_, shape) => {
            let (footprint, round) = match shape {
                Shape::Rect { center, half, angle } => (Footprint::Rect { center, half, angle }, false),
                Shape::Circle { center, radius } => (Footprint::Round { center, radius }, true),
            };
            let roof = if round && matches!(s.roof, RoofStyle::Pitched | RoofStyle::Sawtooth) { RoofStyle::Shell } else { s.roof };
            Structure::House(House {
                footprint,
                wall_height: height,
                roof,
                roof_pitch: pitch_frac(s.roof_pitch_deg),
                material: s.material,
                fins: s.fins && !round,
                foundation: s.plinth,
                planters: s.planters,
                openings: Vec::new(),
                colors: s.colors,
                overgrowth: s.overgrowth,
            })
        }
    }
}

fn span_structure(tool: Tool, s: &ToolSettings, a: Vec3, b: Vec3) -> Structure {
    if tool == Tool::Bridge {
        Structure::Bridge(Bridge { a, b, width: s.bridge_width, kind: s.bridge, hanging: s.hanging, colors: s.colors })
    } else {
        Structure::Stairs(Stairs { from: a, to: b, width: s.stair_width, kind: s.stairs, colors: s.colors })
    }
}

/// Why a span (stairs / bridge) from `a` to `b` cannot be placed.
pub fn span_invalid(tool: Tool, kind: StairKind, a: Vec3, b: Vec3, anchored: (bool, bool)) -> Option<&'static str> {
    if !inside_valley(a.xz()) || !inside_valley(b.xz()) {
        return Some(OUTSIDE);
    }
    let run = a.xz().distance(b.xz());
    let rise = (b.y - a.y).abs();
    if tool == Tool::Bridge {
        return (run < MIN_BRIDGE || !(anchored.0 && anchored.1)).then_some(NEEDS_ANCHORS);
    }
    if run.hypot(rise) < 0.8 {
        return Some(TOO_SHORT);
    }
    (kind != StairKind::Ladder && rise > run * MAX_STAIR_SLOPE).then_some(TOO_STEEP)
}

/// Window / door click on a hit structure. Returns the edited document or an invalid reason.
pub fn opening_click(doc: &Document, hit: &Hit, place: OpeningKind, remove: bool) -> Result<Document, &'static str> {
    let id = hit.id.ok_or(NEEDS_WALL)?;
    let mut d = doc.clone();
    let openings = match d.get_mut(id) {
        Some(Structure::Wall(w)) => &mut w.openings,
        Some(Structure::House(h)) => &mut h.openings,
        _ => return Err(NEEDS_WALL),
    };
    let near = openings
        .iter()
        .enumerate()
        .map(|(i, o)| (i, (o.along - hit.along).abs()))
        .filter(|(_, dd)| *dd < OPENING_GRAB)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i);
    match (near, remove) {
        (Some(i), true) => {
            openings.remove(i);
        }
        (None, true) => return Err(NEEDS_WALL),
        (Some(i), false) => openings[i].kind = next_opening(openings[i].kind),
        (None, false) => openings.push(Opening { along: hit.along, kind: place }),
    }
    Ok(d)
}

/// Roof click: closed wall ring (hit directly or containing the hit point) or a block.
/// `remove` takes the roof off a ring.
pub fn roof_click(doc: &Document, hit: &Hit, s: &ToolSettings, remove: bool) -> Result<Document, &'static str> {
    let mut d = doc.clone();
    let p = hit.point.xz();
    let direct = hit.id.filter(|id| match doc.get(*id) {
        Some(Structure::Wall(w)) => is_closed(&w.points),
        Some(Structure::House(_)) => true,
        _ => false,
    });
    let target = direct.or_else(|| {
        doc.structures
            .iter()
            .rev()
            .find(|(_, st)| matches!(st, Structure::Wall(w) if is_closed(&w.points) && contains(&w.points, p)))
            .map(|(id, _)| *id)
    });
    match target.and_then(|id| d.get_mut(id)) {
        Some(Structure::Wall(w)) => {
            w.roofed = !remove;
            w.roof = s.roof;
            w.roof_pitch = pitch_frac(s.roof_pitch_deg);
            w.planters |= s.roof == RoofStyle::Green && s.planters;
        }
        Some(Structure::House(h)) if !remove => {
            let round = matches!(h.footprint, Footprint::Round { .. });
            h.roof = if round && matches!(s.roof, RoofStyle::Pitched | RoofStyle::Sawtooth) { RoofStyle::Shell } else { s.roof };
            h.roof_pitch = pitch_frac(s.roof_pitch_deg);
            h.planters = s.planters;
        }
        _ => return Err(NEEDS_RING),
    }
    Ok(d)
}

/// Style picker: copy a structure's style into the settings.
pub fn copy_style(st: &Structure, s: &mut ToolSettings) {
    match st {
        Structure::Wall(w) => {
            s.material = w.material;
            s.planters = w.planters;
            s.ruined = w.ruined;
            s.wall_thickness = w.thickness.clamp(0.3, 1.2);
            s.plinth = w.foundation.clamp(0.0, 2.0);
            s.overgrowth = w.overgrowth.clamp(0.0, 1.0);
            s.roof = w.roof;
            s.roof_pitch_deg = pitch_deg(w.roof_pitch);
            s.wall_height = w.height.clamp(2.0, 14.0);
            s.colors = w.colors;
        }
        Structure::House(h) => {
            s.material = h.material;
            s.roof = h.roof;
            s.roof_pitch_deg = pitch_deg(h.roof_pitch);
            s.fins = h.fins;
            s.plinth = h.foundation.clamp(0.0, 2.0);
            s.planters = h.planters;
            s.overgrowth = h.overgrowth.clamp(0.0, 1.0);
            s.colors = h.colors;
        }
        Structure::Fence(f) => {
            s.fence = f.kind;
            s.colors = f.colors;
        }
        Structure::Path(p) => {
            s.path = p.kind;
            s.path_width = p.width.clamp(0.8, 4.0);
        }
        Structure::Stairs(st) => {
            s.stairs = st.kind;
            s.stair_width = st.width.clamp(0.6, 2.4);
            s.colors = st.colors;
        }
        Structure::Platform(p) => s.colors = p.colors,
        Structure::Bridge(b) => {
            s.bridge = b.kind;
            s.hanging = b.hanging;
            s.bridge_width = b.width.clamp(1.0, 3.0);
            s.colors = b.colors;
        }
        Structure::Placed(p) => {
            match p.kind {
                PropKind::Tree(k) => s.tree = k,
                PropKind::Clutter(k) => s.clutter = k,
            }
            s.colors = p.colors;
        }
    }
}

/// Style picker with Shift: apply the settings' style to a structure (geometry untouched).
pub fn apply_style(st: &mut Structure, s: &ToolSettings) {
    match st {
        Structure::Wall(w) => {
            w.material = s.material;
            w.planters = s.planters;
            w.ruined = s.ruined;
            w.thickness = s.wall_thickness;
            w.foundation = s.plinth;
            w.overgrowth = s.overgrowth;
            w.roof = s.roof;
            w.roof_pitch = pitch_frac(s.roof_pitch_deg);
            w.colors = s.colors;
        }
        Structure::House(h) => {
            let round = matches!(h.footprint, Footprint::Round { .. });
            h.material = s.material;
            h.roof = if round && matches!(s.roof, RoofStyle::Pitched | RoofStyle::Sawtooth) { RoofStyle::Shell } else { s.roof };
            h.roof_pitch = pitch_frac(s.roof_pitch_deg);
            h.fins = s.fins && !round;
            h.foundation = s.plinth;
            h.planters = s.planters;
            h.overgrowth = s.overgrowth;
            h.colors = s.colors;
        }
        Structure::Fence(f) => {
            f.kind = s.fence;
            f.colors = s.colors;
        }
        Structure::Path(p) => p.kind = s.path,
        Structure::Stairs(st) => {
            st.kind = s.stairs;
            st.colors = s.colors;
        }
        Structure::Platform(p) => p.colors = s.colors,
        Structure::Bridge(b) => {
            b.kind = s.bridge;
            b.hanging = s.hanging;
            b.colors = s.colors;
        }
        Structure::Placed(p) => {
            p.kind = match p.kind {
                PropKind::Tree(_) => PropKind::Tree(s.tree),
                PropKind::Clutter(_) => PropKind::Clutter(s.clutter),
            };
            p.colors = s.colors;
        }
    }
}

/// The brush dab a terrain tool emits at `center`.
fn brush_dab(s: &ToolSettings, center: Vec2, shift: bool, level: f32) -> TerrainDab {
    let radius = s.brush_radius.clamp(1.0, 8.0);
    let op = match s.tool {
        Tool::Sculpt => {
            let sign = if s.lower ^ shift { -1.0 } else { 1.0 };
            TerrainOp::Sculpt { amount: sign * s.brush_strength.clamp(0.1, 1.0) * 0.35 }
        }
        Tool::Flatten => TerrainOp::Flatten { height: level, strength: s.brush_strength.clamp(0.1, 1.0) },
        Tool::Water => TerrainOp::Water { depth: s.water_depth.clamp(0.3, 1.5) * 0.4 },
        // Shift erases the selected layer; the Clear layer always clears everything.
        _ => match s.paint {
            PaintLayer::Clear => TerrainOp::Paint { layer: PaintLayer::Clear, strength: 1.0 },
            layer => TerrainOp::Paint { layer, strength: if shift { -0.6 } else { 0.6 } },
        },
    };
    TerrainDab { center, radius, op }
}

// =============================================================================================
// Runtime
// =============================================================================================

/// In-progress gesture.
#[derive(Clone, Debug, Default)]
enum Phase {
    #[default]
    Idle,
    /// Wall / fence / path polyline. `held` = LMB down; `moved` = drag distance since press.
    Line { pts: Vec<Vec2>, end: Vec2, held: bool, moved: f32 },
    /// Block / tower / deck footprint drag.
    Footprint { a: Vec2, b: Vec2, angle: f32 },
    /// Footprint fixed; vertical mouse sets the height; click casts.
    Raise { shape: Shape, base: f32, anchor_y: f32 },
    /// Stairs / bridge A→B.
    Span { a: Vec3, b: Vec3, anchored: (bool, bool) },
    /// Tree / clutter held before release.
    Prop { angle: f32, scale: f32 },
    /// Terrain brush stroke (working document = committed + dabs so far).
    Stroke { doc: Document, last: Vec2, still: f32, level: f32 },
    /// Erase drag: structures removed so far.
    Erase { removed: Vec<StructureId> },
}

#[derive(Resource, Default)]
pub struct ToolState {
    phase: Phase,
    tool: Tool,
    scroll_acc: f32,
    rmb_travel: f32,
}

/// Per-frame pointer input in world terms.
struct Input {
    ray: Option<(Vec3, Vec3)>,
    ground: Option<Vec3>,
    cursor_y: f32,
    press: bool,
    held: bool,
    release: bool,
    shift: bool,
    steps: i32,
    dt: f32,
}

#[derive(Clone, Copy)]
enum Tint {
    Ok,
    Bad,
    Pick,
}

impl Tint {
    fn color(self) -> Color {
        match self {
            Tint::Ok => Color::srgb(0.62, 0.84, 0.5),
            Tint::Bad => Color::srgb(0.96, 0.62, 0.22),
            Tint::Pick => Color::srgb(0.55, 0.78, 0.95),
        }
    }
}

enum Mark {
    Ring(Vec3, f32, Tint),
    Part(StructureId, Tint),
}

/// Everything a tool handler touches this frame.
struct Cx<'a> {
    s: &'a mut ToolSettings,
    glade: &'a mut Glade,
    scene: &'a Scene,
    inp: &'a Input,
    rig: &'a CameraRig,
    win_h: f32,
    phase: Phase,
    preview: Option<Document>,
    invalid: Option<&'static str>,
    marks: Vec<Mark>,
}

impl Cx<'_> {
    fn pending_id(&self) -> StructureId {
        self.glade.committed().next_id
    }
    /// Pick in the shown scene, skipping the structure being drawn.
    fn pick(&self, origin: Vec3, dir: Vec3, skip: Option<StructureId>) -> Option<Hit> {
        let doc = self.glade.doc();
        let mut o = origin;
        for _ in 0..6 {
            let h = pick(doc, self.scene, o, dir)?;
            if h.id.is_some() && h.id == skip {
                o = h.point + dir.normalize_or_zero() * 0.05;
                continue;
            }
            return Some(h);
        }
        None
    }
    fn hover(&self) -> Option<Hit> {
        let (o, d) = self.inp.ray?;
        self.pick(o, d, None)
    }
    /// Point under the cursor, snapped to the top of the structure there (wall walk, deck,
    /// roof). Returns (point, structure id).
    fn surface(&self, skip: Option<StructureId>) -> Option<(Vec3, Option<StructureId>)> {
        let (o, d) = self.inp.ray?;
        let h = self.pick(o, d, skip)?;
        if h.id.is_none() || h.normal.y > 0.6 {
            return Some((h.point, h.id));
        }
        let inward = -h.normal.xz().normalize_or_zero() * 0.15;
        let top = h.point.xz() + inward;
        let down = self.pick(Vec3::new(top.x, h.point.y + 60.0, top.y), Vec3::NEG_Y, skip);
        Some(match down {
            Some(t) if t.id == h.id => (t.point, h.id),
            _ => (h.point, h.id),
        })
    }
    fn commit(&mut self, doc: Document) {
        self.glade.commit(doc);
        self.preview = None;
    }
    fn ring(&mut self, p: Vec3, r: f32) {
        let t = if self.invalid.is_some() { Tint::Bad } else { Tint::Ok };
        self.marks.push(Mark::Ring(p, r, t));
    }
}

fn thin_gizmos(mut store: ResMut<GizmoConfigStore>) {
    store.config_mut::<DefaultGizmoConfigGroup>().0.line.width = 1.0;
}

type RawInput<'w> = (
    Res<'w, ButtonInput<KeyCode>>,
    Res<'w, ButtonInput<MouseButton>>,
    Res<'w, AccumulatedMouseScroll>,
    Res<'w, AccumulatedMouseMotion>,
    Res<'w, Time>,
);

#[allow(clippy::too_many_arguments)]
fn drive_tools(
    input: RawInput,
    window: Option<Single<&Window, With<PrimaryWindow>>>,
    camera: Query<(&Camera, &GlobalTransform), With<GameCamera>>,
    low: Option<Res<LowRes>>,
    rig: Res<CameraRig>,
    over_ui: Res<PointerOverUi>,
    built: Res<BuiltScene>,
    mut glade: ResMut<Glade>,
    mut settings: ResMut<ToolSettings>,
    mut status: ResMut<ToolStatus>,
    mut cam_input: ResMut<CameraInput>,
    mut state: ResMut<ToolState>,
    mut gizmos: Gizmos,
    cursor_override: Option<Res<CursorOverride>>,
) {
    let (keys, mouse, scroll, motion, time) = input;
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::SuperLeft, KeyCode::SuperRight]);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let rev0 = glade.revision;
    let g = glade.bypass_change_detection();
    let mut s = settings.clone();
    let st = &mut *state;

    // --- keyboard: tool hotkeys, undo / redo, cancel ------------------------------------------
    let mut cancel = keys.just_pressed(KeyCode::Escape);
    if ctrl {
        if keys.just_pressed(KeyCode::KeyZ) {
            if shift { g.redo() } else { g.undo() };
            cancel = true;
        } else if keys.just_pressed(KeyCode::KeyY) {
            g.redo();
            cancel = true;
        }
    } else if let Some(t) = Tool::ALL.into_iter().find(|t| keys.just_pressed(t.hotkey().0)) {
        s.tool = t;
    }
    if s.tool != st.tool {
        st.tool = s.tool;
        cancel = true;
    }
    if mouse.just_pressed(MouseButton::Right) {
        st.rmb_travel = 0.0;
    }
    if mouse.pressed(MouseButton::Right) {
        st.rmb_travel += motion.delta.length();
    }
    if mouse.just_released(MouseButton::Right) && st.rmb_travel < RMB_TAP_PX {
        cancel = true;
    }
    if cancel {
        st.phase = Phase::Idle;
        g.set_preview(None);
    }

    // --- pointer ---------------------------------------------------------------------------------
    let lines = if scroll.unit == MouseScrollUnit::Line { scroll.delta.y } else { scroll.delta.y / 30.0 };
    let idle = matches!(st.phase, Phase::Idle);
    let blocked = over_ui.0 && cursor_override.is_none();
    let cursor = match cursor_override.as_deref() {
        Some(o) => match (o.target, window.as_ref(), camera.single().ok(), low.as_ref()) {
            (Some(t), Some(w), Some((cam, tf)), Some(low)) => {
                cam.world_to_viewport(tf, t).ok().map(|v| v * w.size() / low.size.as_vec2() + o.px)
            }
            _ => None,
        },
        None => window.as_ref().and_then(|w| w.cursor_position()),
    };
    let ray = match (cursor, window.as_ref(), camera.single().ok(), low.as_ref()) {
        (Some(c), Some(w), Some((cam, tf)), Some(low)) => cursor_ray(c, w, cam, tf, low.size).map(|r| (r.origin, *r.direction)),
        _ => None,
    };
    let scene = built.0.clone();
    let world_input = scene.is_some() && ray.is_some() && !(blocked && idle);
    // Scroll belongs to the tool while a gesture is in progress (or a prop is held).
    if !idle && !blocked {
        st.scroll_acc += lines;
        cam_input.scroll_captured = true;
    } else {
        st.scroll_acc = 0.0;
    }
    let steps = st.scroll_acc.trunc() as i32;
    st.scroll_acc -= steps as f32;

    let mut new_status = ToolStatus::default();
    if let (Some(scene), true) = (scene.as_deref(), world_input) {
        let ground = ray.and_then(|(o, d)| ground_hit(&scene.terrain, o, d));
        let inp = Input {
            ray,
            ground,
            cursor_y: cursor.map_or(0.0, |c| c.y),
            press: mouse.just_pressed(MouseButton::Left) && !blocked,
            held: mouse.pressed(MouseButton::Left),
            release: mouse.just_released(MouseButton::Left),
            shift,
            steps,
            dt: time.delta_secs(),
        };
        let win_h = window.as_ref().map_or(720.0, |w| w.height()).max(1.0);
        let mut cx = Cx {
            s: &mut s,
            glade: g,
            scene,
            inp: &inp,
            rig: &rig,
            win_h,
            phase: std::mem::take(&mut st.phase),
            preview: None,
            invalid: None,
            marks: Vec::new(),
        };
        match cx.s.tool {
            Tool::Wall | Tool::Fence | Tool::Path => line_tool(&mut cx),
            Tool::Block | Tool::Tower | Tool::Deck => shape_tool(&mut cx),
            Tool::Stairs | Tool::Bridge => span_tool(&mut cx),
            Tool::Tree | Tool::Clutter => prop_tool(&mut cx),
            Tool::Paint | Tool::Sculpt | Tool::Flatten | Tool::Water => brush_tool(&mut cx),
            Tool::Opening | Tool::Roof | Tool::Style => click_tool(&mut cx),
            Tool::Erase => erase_tool(&mut cx),
        }
        let drawing = !matches!(cx.phase, Phase::Idle);
        if drawing {
            cx.glade.set_preview(cx.preview.take());
        } else if cx.glade.has_preview() {
            cx.glade.set_preview(None);
        }
        new_status = ToolStatus { drawing, invalid: cx.invalid.map(str::to_string) };
        st.phase = cx.phase;
        for m in cx.marks {
            draw_mark(&mut gizmos, scene, m);
        }
    } else if !matches!(st.phase, Phase::Idle) {
        // Pointer left the world mid-gesture: keep the gesture, keep the preview.
        new_status.drawing = true;
        if mouse.just_released(MouseButton::Left) && matches!(st.phase, Phase::Stroke { .. } | Phase::Erase { .. }) {
            // Let the stroke end where it is.
            match std::mem::take(&mut st.phase) {
                Phase::Stroke { doc, .. } => g.commit(doc),
                Phase::Erase { removed } if !removed.is_empty() => {
                    let mut d = g.committed().clone();
                    d.structures.retain(|(id, _)| !removed.contains(id));
                    g.commit(d);
                }
                _ => g.set_preview(None),
            }
            new_status.drawing = false;
        }
    }

    if glade.revision != rev0 {
        glade.set_changed();
    }
    if s != *settings {
        *settings = s;
    }
    if new_status != *status {
        *status = new_status;
    }
}

fn draw_mark(gizmos: &mut Gizmos, scene: &Scene, m: Mark) {
    match m {
        Mark::Ring(p, r, t) => {
            let iso = Isometry3d::new(p + Vec3::Y * 0.08, Quat::from_rotation_x(std::f32::consts::FRAC_PI_2));
            gizmos.circle(iso, r, t.color()).resolution(24);
        }
        Mark::Part(id, t) => {
            if let Some((lo, hi)) = scene.parts.iter().find(|p| p.id == id).and_then(|p| p.mesh.bounds()) {
                let tf = Transform::from_translation((lo + hi) * 0.5).with_scale((hi - lo).max(Vec3::splat(0.1)) + 0.1);
                gizmos.cube(tf, t.color());
            }
        }
    }
}

// --- Wall / Fence / Path -------------------------------------------------------------------------

fn line_tool(cx: &mut Cx) {
    let tool = cx.s.tool;
    let inp = cx.inp;
    match std::mem::take(&mut cx.phase) {
        Phase::Idle => {
            let Some(g) = inp.ground else { return };
            let p = q2(g.xz());
            if !inside_valley(p) {
                cx.invalid = Some(OUTSIDE);
            } else if inp.press {
                cx.phase = Phase::Line { pts: vec![p], end: p, held: true, moved: 0.0 };
                return;
            }
            cx.ring(g, 0.4);
        }
        Phase::Line { mut pts, end: prev, mut held, mut moved } => {
            match tool {
                Tool::Wall => cx.s.wall_height = (cx.s.wall_height + inp.steps as f32 * 0.5).clamp(2.0, 14.0),
                Tool::Path => cx.s.path_width = (cx.s.path_width + inp.steps as f32 * 0.2).clamp(0.8, 4.0),
                _ if inp.steps != 0 => cx.s.fence = cycle_fence(cx.s.fence, inp.steps.signum()),
                _ => {}
            }
            let last = *pts.last().unwrap();
            let raw = inp.ground.map_or(prev, |g| q2(g.xz()));
            let mut end = if inp.shift { snap45(last, raw) } else { raw };
            let closing = tool != Tool::Path && close_snap(&pts, end);
            if closing {
                end = pts[0];
            }
            if held {
                moved += end.distance(prev);
                if !closing && end.distance(last) >= POINT_SPACING {
                    pts.push(end);
                }
            }
            let mut finish = false;
            if inp.press {
                // Click mode: each click adds a point; clicking the last point or the start ends.
                if closing || end.distance(last) < 0.6 {
                    finish = true;
                } else {
                    pts.push(end);
                    held = true;
                    moved = 0.0;
                }
            }
            if inp.release && held {
                held = false;
                finish |= moved > 1.0;
            }
            let mut all = pts.clone();
            if all.last().is_none_or(|l| l.distance(end) > 0.05) {
                all.push(end);
            }
            let all = simplify_polyline(&all, SIMPLIFY_EPS);
            let len = polyline_length(&all);
            cx.invalid = if !all.iter().all(|p| inside_valley(*p)) {
                Some(OUTSIDE)
            } else if len < MIN_LINE {
                (len > 0.3 || finish).then_some(TOO_SHORT)
            } else {
                None
            };
            let doc = (all.len() >= 2).then(|| with_structure(cx.glade.committed(), line_structure(tool, cx.s, all)));
            if finish {
                if let (Some(doc), None) = (doc, cx.invalid) {
                    cx.commit(doc);
                }
                return;
            }
            if let Some(g) = inp.ground {
                cx.ring(g, if closing { 0.8 } else { 0.4 });
            }
            cx.preview = doc;
            cx.phase = Phase::Line { pts, end, held, moved };
        }
        other => cx.phase = other,
    }
}

// --- Block / Tower / Deck -----------------------------------------------------------------------

fn default_height(tool: Tool) -> f32 {
    match tool {
        Tool::Tower => TOWER_HEIGHT,
        Tool::Deck => DECK_HEIGHT,
        _ => BLOCK_HEIGHT,
    }
}

fn height_range(tool: Tool) -> (f32, f32) {
    if tool == Tool::Deck { (0.3, 8.0) } else { (0.6, 24.0) }
}

fn shape_of(tool: Tool, a: Vec2, b: Vec2, angle: f32, shift: bool) -> Shape {
    if tool == Tool::Tower {
        Shape::Circle { center: a, radius: q(a.distance(b)) }
    } else {
        let (center, half) = rect_from_drag(a, b, angle, shift);
        Shape::Rect { center: q2(center), half: q2(half), angle }
    }
}

fn shape_tool(cx: &mut Cx) {
    let tool = cx.s.tool;
    let inp = cx.inp;
    match std::mem::take(&mut cx.phase) {
        Phase::Idle => {
            let Some(g) = inp.ground else { return };
            let p = q2(g.xz());
            if !inside_valley(p) {
                cx.invalid = Some(OUTSIDE);
            } else if inp.press {
                cx.phase = Phase::Footprint { a: p, b: p, angle: 0.0 };
                return;
            }
            cx.ring(g, 0.4);
        }
        Phase::Footprint { a, b, mut angle } => {
            let b = inp.ground.map_or(b, |g| q2(g.xz()));
            if tool != Tool::Tower {
                angle += inp.steps as f32 * 15f32.to_radians();
            }
            let shape = shape_of(tool, a, b, angle, inp.shift);
            cx.invalid = shape.invalid();
            if inp.release || !inp.held {
                if cx.invalid.is_none() {
                    let base = default_height(tool);
                    cx.preview = Some(with_structure(cx.glade.committed(), shape_structure(tool, cx.s, shape, base)));
                    cx.phase = Phase::Raise { shape, base, anchor_y: inp.cursor_y };
                } else {
                    cx.invalid = None;
                }
                return;
            }
            // Low ghost while dragging the footprint.
            let h = if tool == Tool::Deck { DECK_HEIGHT } else { 0.6 };
            cx.preview = Some(with_structure(cx.glade.committed(), shape_structure(tool, cx.s, shape, h)));
            if let Some(g) = inp.ground {
                cx.ring(g, 0.3);
            }
            cx.phase = Phase::Footprint { a, b, angle };
        }
        Phase::Raise { shape, mut base, anchor_y } => {
            let (lo, hi) = height_range(tool);
            match tool {
                Tool::Deck => base += inp.steps as f32 * 0.25,
                Tool::Tower if inp.steps != 0 => cx.s.roof = cycle_roof(cx.s.roof, inp.steps.signum(), true),
                _ if inp.steps != 0 && inp.shift => cx.s.roof = cycle_roof(cx.s.roof, inp.steps.signum(), false),
                _ => cx.s.roof_pitch_deg = (cx.s.roof_pitch_deg + inp.steps as f32 * 5.0).clamp(15.0, 60.0),
            }
            let per_px = VIEW_HEIGHT * cx.rig.zoom / (cx.win_h * cx.rig.pitch_deg.to_radians().cos().max(0.3));
            let height = q((base + (anchor_y - inp.cursor_y) * per_px).clamp(lo, hi));
            base = base.clamp(lo - 30.0, hi + 30.0);
            let doc = with_structure(cx.glade.committed(), shape_structure(tool, cx.s, shape, height));
            if inp.press {
                cx.commit(doc);
                return;
            }
            cx.preview = Some(doc);
            cx.phase = Phase::Raise { shape, base, anchor_y };
        }
        other => cx.phase = other,
    }
}

// --- Stairs / Bridge -----------------------------------------------------------------------------

/// End point of a span under the cursor: structure top, or ground (bridges sit a little above
/// ground / water). Second value: is it a valid anchor.
fn span_anchor(cx: &Cx, skip: Option<StructureId>) -> Option<(Vec3, bool)> {
    let (p, id) = cx.surface(skip)?;
    let p = Vec3::new(q(p.x), p.y, q(p.z));
    if id.is_some() || cx.s.tool == Tool::Stairs {
        return Some((p, true));
    }
    let wet = cx.scene.terrain.water_level(p.xz());
    let y = wet.map_or(p.y, |l| l.max(p.y)) + 0.4;
    Some((p.with_y(y), wet.is_none()))
}

fn span_tool(cx: &mut Cx) {
    let tool = cx.s.tool;
    let inp = cx.inp;
    match std::mem::take(&mut cx.phase) {
        Phase::Idle => {
            let Some((a, ok)) = span_anchor(cx, None) else { return };
            if tool == Tool::Bridge && !ok {
                cx.invalid = Some(NEEDS_ANCHORS);
            }
            if inp.press && inside_valley(a.xz()) {
                cx.phase = Phase::Span { a, b: a, anchored: (ok, ok) };
                return;
            }
            cx.ring(a, 0.4);
        }
        Phase::Span { a, b, anchored } => {
            if tool == Tool::Bridge {
                cx.s.bridge_width = (cx.s.bridge_width + inp.steps as f32 * 0.2).clamp(1.0, 3.0);
            } else {
                cx.s.stair_width = (cx.s.stair_width + inp.steps as f32 * 0.1).clamp(0.6, 2.4);
            }
            let skip = Some(cx.pending_id());
            let (b, ok_b) = span_anchor(cx, skip).unwrap_or((b, anchored.1));
            let anchored = (anchored.0, ok_b);
            cx.invalid = span_invalid(tool, cx.s.stairs, a, b, anchored);
            let doc = (a.distance(b) > 0.3).then(|| with_structure(cx.glade.committed(), span_structure(tool, cx.s, a, b)));
            if inp.release || !inp.held {
                if let (Some(doc), None) = (doc, cx.invalid) {
                    cx.commit(doc);
                }
                cx.invalid = None;
                return;
            }
            cx.ring(b, 0.4);
            cx.preview = doc;
            cx.phase = Phase::Span { a, b, anchored };
        }
        other => cx.phase = other,
    }
}

// --- Tree / Clutter ------------------------------------------------------------------------------

fn prop_tool(cx: &mut Cx) {
    let inp = cx.inp;
    let Some(g) = inp.ground else {
        if inp.release {
            cx.phase = Phase::Idle;
        }
        return;
    };
    let p = q2(g.xz());
    let phase = std::mem::take(&mut cx.phase);
    let (at, mut angle, mut scale) = match phase {
        Phase::Prop { angle, scale, .. } => (p, angle, scale),
        _ if inp.press => (p, hash01(p, 1) * TAU, 0.9 + hash01(p, 2) * 0.25),
        _ => {
            cx.invalid = (!inside_valley(p)).then_some(OUTSIDE);
            cx.ring(g, 0.5);
            return;
        }
    };
    if inp.shift {
        scale = (scale + inp.steps as f32 * 0.05).clamp(0.7, 1.3);
    } else {
        angle = (angle + inp.steps as f32 * 15f32.to_radians()).rem_euclid(TAU);
    }
    cx.invalid = (!inside_valley(at)).then_some(OUTSIDE);
    let kind = if cx.s.tool == Tool::Tree { PropKind::Tree(cx.s.tree) } else { PropKind::Clutter(cx.s.clutter) };
    let doc = with_structure(cx.glade.committed(), Structure::Placed(Placed { at, angle, scale, kind, colors: cx.s.colors }));
    if inp.release || !inp.held {
        if cx.invalid.is_none() {
            cx.commit(doc);
        }
        cx.invalid = None;
        return;
    }
    cx.ring(g, 0.5);
    cx.preview = Some(doc);
    cx.phase = Phase::Prop { angle, scale };
}

// --- Paint / Sculpt / Flatten / Water ------------------------------------------------------------

fn brush_tool(cx: &mut Cx) {
    let inp = cx.inp;
    cx.s.brush_radius = (cx.s.brush_radius + inp.steps as f32 * 0.5).clamp(1.0, 8.0);
    let radius = cx.s.brush_radius;
    let spacing = radius * 0.5;
    match std::mem::take(&mut cx.phase) {
        Phase::Idle => {
            let Some(g) = inp.ground else { return };
            let p = q2(g.xz());
            if !inside_valley(p) {
                cx.invalid = Some(OUTSIDE);
            } else if inp.press {
                let mut doc = cx.glade.committed().clone();
                doc.terrain.push(brush_dab(cx.s, p, inp.shift, g.y));
                cx.preview = Some(doc.clone());
                cx.phase = Phase::Stroke { doc, last: p, still: 0.0, level: g.y };
            }
            cx.ring(g, radius);
        }
        Phase::Stroke { mut doc, mut last, mut still, level } => {
            if let Some(g) = inp.ground {
                let p = q2(g.xz());
                let dabs = dab_points(last, p, spacing);
                for c in &dabs {
                    if inside_valley(*c) {
                        doc.terrain.push(brush_dab(cx.s, *c, inp.shift, level));
                    }
                }
                if let Some(l) = dabs.last() {
                    last = *l;
                    still = 0.0;
                } else if cx.s.tool == Tool::Sculpt && inp.held {
                    // Holding still keeps raising / lowering.
                    still += inp.dt;
                    if still > 0.12 {
                        still = 0.0;
                        doc.terrain.push(brush_dab(cx.s, last, inp.shift, level));
                    }
                }
                cx.ring(g, radius);
            }
            if inp.release || !inp.held {
                cx.commit(doc);
                return;
            }
            cx.preview = Some(doc.clone());
            cx.phase = Phase::Stroke { doc, last, still, level };
        }
        other => cx.phase = other,
    }
}

// --- Window/Door, Roof, Style --------------------------------------------------------------------

fn click_tool(cx: &mut Cx) {
    let inp = cx.inp;
    cx.phase = Phase::Idle;
    let Some(hit) = cx.hover() else { return };
    let committed = cx.glade.committed();
    match cx.s.tool {
        Tool::Opening => match opening_click(committed, &hit, cx.s.opening, inp.shift) {
            Ok(doc) => {
                cx.marks.push(Mark::Part(hit.id.unwrap_or_default(), Tint::Ok));
                cx.marks.push(Mark::Ring(hit.point, 0.5, Tint::Ok));
                if inp.press {
                    cx.commit(doc);
                }
            }
            Err(e) => {
                cx.invalid = Some(e);
                cx.marks.push(Mark::Ring(hit.point, 0.4, Tint::Bad));
            }
        },
        Tool::Roof => match roof_click(committed, &hit, cx.s, inp.shift) {
            Ok(doc) => {
                if let Some(id) = doc.structures.iter().zip(&committed.structures).find(|(a, b)| a != b).map(|(a, _)| a.0) {
                    cx.marks.push(Mark::Part(id, Tint::Ok));
                }
                if inp.press {
                    cx.commit(doc);
                }
            }
            Err(e) => {
                cx.invalid = Some(e);
                cx.marks.push(Mark::Ring(hit.point, 0.4, Tint::Bad));
            }
        },
        _ => {
            let Some(id) = hit.id else {
                cx.marks.push(Mark::Ring(hit.point, 0.4, Tint::Pick));
                return;
            };
            cx.marks.push(Mark::Part(id, Tint::Pick));
            if !inp.press {
                return;
            }
            if inp.shift {
                let mut doc = committed.clone();
                if let Some(st) = doc.get_mut(id) {
                    apply_style(st, cx.s);
                }
                cx.commit(doc);
            } else if let Some(st) = committed.get(id) {
                let st = st.clone();
                copy_style(&st, cx.s);
            }
        }
    }
}

// --- Erase ---------------------------------------------------------------------------------------

fn erase_tool(cx: &mut Cx) {
    let inp = cx.inp;
    let hovered = cx.hover().and_then(|h| h.id);
    if let Some(id) = hovered {
        cx.marks.push(Mark::Part(id, Tint::Bad));
    } else if let Some(g) = inp.ground {
        cx.marks.push(Mark::Ring(g, 0.4, Tint::Bad));
    }
    let mut removed = match std::mem::take(&mut cx.phase) {
        Phase::Erase { removed } => removed,
        _ if inp.press => Vec::new(),
        _ => return,
    };
    if let Some(id) = hovered.filter(|id| !removed.contains(id)) {
        removed.push(id);
    }
    let mut doc = cx.glade.committed().clone();
    doc.structures.retain(|(id, _)| !removed.contains(id));
    if inp.release || !inp.held {
        if !removed.is_empty() {
            cx.commit(doc);
        }
        return;
    }
    cx.preview = (!removed.is_empty()).then_some(doc);
    cx.phase = Phase::Erase { removed };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simplify_drops_collinear_and_keeps_corners() {
        let pts: Vec<Vec2> = (0..=10).map(|i| Vec2::new(i as f32, 0.0)).chain((1..=5).map(|i| Vec2::new(10.0, i as f32))).collect();
        let s = simplify_polyline(&pts, 0.2);
        assert_eq!(s, vec![Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(10.0, 5.0)]);
    }

    #[test]
    fn simplify_keeps_closed_ring_closed() {
        let sq = [Vec2::ZERO, Vec2::new(2.0, 0.0), Vec2::new(4.0, 0.0), Vec2::new(4.0, 4.0), Vec2::new(0.0, 4.0), Vec2::ZERO];
        let s = simplify_polyline(&sq, 0.2);
        assert_eq!(s.len(), 5);
        assert!(is_closed(&s));
    }

    #[test]
    fn simplify_dedupes() {
        let s = simplify_polyline(&[Vec2::ZERO, Vec2::new(0.01, 0.0), Vec2::new(3.0, 0.0)], 0.2);
        assert_eq!(s, vec![Vec2::ZERO, Vec2::new(3.0, 0.0)]);
    }

    #[test]
    fn rect_from_drag_axis_aligned_and_rotated() {
        let (c, h) = rect_from_drag(Vec2::new(1.0, 1.0), Vec2::new(5.0, -1.0), 0.0, false);
        assert!(c.distance(Vec2::new(3.0, 0.0)) < 1e-5 && h.distance(Vec2::new(2.0, 1.0)) < 1e-5);
        let (_, h) = rect_from_drag(Vec2::ZERO, Vec2::new(4.0, 1.0), 0.0, true);
        assert!(h.distance(Vec2::splat(2.0)) < 1e-5);
        // 90° frame: x half comes from the z drag.
        let (c, h) = rect_from_drag(Vec2::ZERO, Vec2::new(2.0, 6.0), std::f32::consts::FRAC_PI_2, false);
        assert!(c.distance(Vec2::new(1.0, 3.0)) < 1e-4 && h.distance(Vec2::new(3.0, 1.0)) < 1e-4);
    }

    #[test]
    fn dab_spacing() {
        let d = dab_points(Vec2::ZERO, Vec2::new(5.2, 0.0), 1.5);
        assert_eq!(d.len(), 3);
        assert!(d[2].distance(Vec2::new(4.5, 0.0)) < 1e-5);
        assert!(dab_points(Vec2::ZERO, Vec2::new(1.0, 0.0), 1.5).is_empty());
    }

    #[test]
    fn opening_cycle() {
        let mut k = OpeningKind::Window;
        let seq: Vec<_> = (0..3).map(|_| {
            k = next_opening(k);
            k
        }).collect();
        assert_eq!(seq, [OpeningKind::Door, OpeningKind::WideDoor, OpeningKind::Window]);
    }

    fn wall_doc(points: Vec<Vec2>) -> (Document, StructureId) {
        let mut d = Document::new(1);
        let id = d.add(line_structure(Tool::Wall, &ToolSettings::default(), points));
        (d, id)
    }

    #[test]
    fn opening_click_places_upgrades_removes() {
        let (d, id) = wall_doc(vec![Vec2::new(-5.0, 0.0), Vec2::new(5.0, 0.0)]);
        let hit = Hit { id: Some(id), point: Vec3::new(0.0, 2.0, 0.0), normal: Vec3::Z, along: 5.0 };
        let d1 = opening_click(&d, &hit, OpeningKind::Window, false).unwrap();
        let d2 = opening_click(&d1, &Hit { along: 5.5, ..hit }, OpeningKind::Window, false).unwrap();
        let Some(Structure::Wall(w)) = d2.get(id) else { panic!() };
        assert_eq!(w.openings, vec![Opening { along: 5.0, kind: OpeningKind::Door }]);
        let d3 = opening_click(&d2, &hit, OpeningKind::Window, true).unwrap();
        let Some(Structure::Wall(w)) = d3.get(id) else { panic!() };
        assert!(w.openings.is_empty());
        let ground = Hit { id: None, ..hit };
        assert_eq!(opening_click(&d, &ground, OpeningKind::Window, false), Err(NEEDS_WALL));
    }

    #[test]
    fn roof_click_inside_ring() {
        let ring = vec![Vec2::ZERO, Vec2::new(6.0, 0.0), Vec2::new(6.0, 6.0), Vec2::new(0.0, 6.0), Vec2::ZERO];
        let (d, id) = wall_doc(ring);
        let hit = Hit { id: None, point: Vec3::new(3.0, 0.0, 3.0), normal: Vec3::Y, along: 0.0 };
        let d1 = roof_click(&d, &hit, &ToolSettings::default(), false).unwrap();
        assert!(matches!(d1.get(id), Some(Structure::Wall(w)) if w.roofed));
        let out = Hit { point: Vec3::new(10.0, 0.0, 3.0), ..hit };
        assert_eq!(roof_click(&d, &out, &ToolSettings::default(), false), Err(NEEDS_RING));
    }

    #[test]
    fn stairs_and_bridge_validity() {
        let a = Vec3::ZERO;
        assert_eq!(span_invalid(Tool::Stairs, StairKind::Steel, a, Vec3::new(1.0, 4.0, 0.0), (true, true)), Some(TOO_STEEP));
        assert_eq!(span_invalid(Tool::Stairs, StairKind::Ladder, a, Vec3::new(1.0, 4.0, 0.0), (true, true)), None);
        assert_eq!(span_invalid(Tool::Stairs, StairKind::Steel, a, Vec3::new(4.0, 3.0, 0.0), (true, true)), None);
        assert_eq!(span_invalid(Tool::Bridge, StairKind::Steel, a, Vec3::new(2.0, 0.0, 0.0), (true, true)), Some(NEEDS_ANCHORS));
        assert_eq!(span_invalid(Tool::Bridge, StairKind::Steel, a, Vec3::new(6.0, 0.0, 0.0), (true, false)), Some(NEEDS_ANCHORS));
        assert_eq!(span_invalid(Tool::Bridge, StairKind::Steel, a, Vec3::new(6.0, 0.0, 0.0), (true, true)), None);
    }

    #[test]
    fn ray_hits_terrain_surface() {
        let t = Terrain::build(3, &[], &[]);
        let o = Vec3::new(30.0, 60.0, 40.0);
        let target = Vec3::new(2.0, t.height(Vec2::new(2.0, -3.0)), -3.0);
        let p = ground_hit(&t, o, target - o).expect("hit");
        assert!((p.y - t.height(p.xz())).abs() < 0.05, "{p}");
        assert!(p.xz().distance(target.xz()) < 0.3, "{p} vs {target}");
    }

    #[test]
    fn style_round_trip() {
        let mut s = ToolSettings { material: Material::Timber, roof: RoofStyle::Sawtooth, roof_pitch_deg: 40.0, ..default() };
        let mut st = shape_structure(Tool::Block, &ToolSettings::default(), Shape::Rect { center: Vec2::ZERO, half: Vec2::ONE * 3.0, angle: 0.0 }, 5.0);
        apply_style(&mut st, &s);
        s = ToolSettings::default();
        copy_style(&st, &mut s);
        assert_eq!((s.material, s.roof), (Material::Timber, RoofStyle::Sawtooth));
        assert!((s.roof_pitch_deg - 40.0).abs() < 0.01);
    }

    #[test]
    fn close_snap_and_snap45() {
        let pts = [Vec2::ZERO, Vec2::new(5.0, 0.0), Vec2::new(5.0, 5.0)];
        assert!(close_snap(&pts, Vec2::new(0.5, 1.0)));
        assert!(!close_snap(&pts[..2], Vec2::new(0.5, 0.0)));
        let p = snap45(Vec2::ZERO, Vec2::new(4.0, 3.6));
        assert!((p.x - p.y).abs() < 1e-4);
    }
}
