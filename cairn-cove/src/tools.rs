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
use cairn_cove_core::buildings::{is_beacon, needs_stilts};
use cairn_cove_core::geom::{contains, footprint_polygon, is_closed, polyline_length};
use cairn_cove_core::scene::{Hit, pick};
use cairn_cove_core::terrain::Terrain;
use cairn_cove_core::{
    Bridge, BridgeKind, ClutterKind, Colors, Document, Fence, FenceKind, Footprint, GLADE_HALF, Hanging, House,
    Material, Opening, OpeningKind, PaintLayer, Path, PathKind, Placed, Platform, PropKind, RoofStyle, Scene,
    StairKind, Stairs, Structure, StructureId, TerrainDab, TerrainOp, TreeKind, Wall, WallKind,
};
use std::f32::consts::{FRAC_PI_4, TAU};

/// Every tool, in toolbar order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Tool {
    #[default]
    Dyke,
    Windbreak,
    Hut,
    Roundhouse,
    Track,
    Steps,
    Jetty,
    Bridge,
    Opening,
    Roof,
    Tree,
    Prop,
    Paint,
    Raise,
    Flatten,
    Lagoon,
    Dye,
    Erase,
}

impl Tool {
    pub const ALL: [Tool; 18] = [
        Tool::Dyke,
        Tool::Windbreak,
        Tool::Hut,
        Tool::Roundhouse,
        Tool::Track,
        Tool::Steps,
        Tool::Jetty,
        Tool::Bridge,
        Tool::Opening,
        Tool::Roof,
        Tool::Tree,
        Tool::Prop,
        Tool::Paint,
        Tool::Raise,
        Tool::Flatten,
        Tool::Lagoon,
        Tool::Dye,
        Tool::Erase,
    ];

    /// Toolbar label (DESIGN.md B5).
    pub fn label(self) -> &'static str {
        match self {
            Tool::Dyke => "DYKE",
            Tool::Windbreak => "W-BREAK",
            Tool::Hut => "HUT",
            Tool::Roundhouse => "ROUND",
            Tool::Track => "TRACK",
            Tool::Steps => "STEPS",
            Tool::Jetty => "JETTY",
            Tool::Bridge => "BRIDGE",
            Tool::Opening => "OPENING",
            Tool::Roof => "ROOF",
            Tool::Tree => "TREE",
            Tool::Prop => "PROP",
            Tool::Paint => "PAINT",
            Tool::Raise => "RAISE",
            Tool::Flatten => "FLATTEN",
            Tool::Lagoon => "LAGOON",
            Tool::Dye => "DYE",
            Tool::Erase => "ERASE",
        }
    }

    /// Three-character ASCII toolbar glyph (DESIGN.md B5).
    pub fn glyph(self) -> &'static str {
        match self {
            Tool::Dyke => "=#=",
            Tool::Windbreak => "|x|",
            Tool::Hut => "/^\\",
            Tool::Roundhouse => "(^)",
            Tool::Track => ":::",
            Tool::Steps => "_|^",
            Tool::Jetty => "TTT",
            Tool::Bridge => "\\_/",
            Tool::Opening => "[ ]",
            Tool::Roof => "/\\",
            Tool::Tree => "\\|/",
            Tool::Prop => ".8.",
            Tool::Paint => "~*~",
            Tool::Raise => "/+\\",
            Tool::Flatten => "-=-",
            Tool::Lagoon => "~~~",
            Tool::Dye => "(%)",
            Tool::Erase => ">x<",
        }
    }

    /// Selection hotkey and its on-screen tag.
    pub fn hotkey(self) -> (KeyCode, &'static str) {
        match self {
            Tool::Dyke => (KeyCode::Digit1, "1"),
            Tool::Windbreak => (KeyCode::Digit2, "2"),
            Tool::Hut => (KeyCode::Digit3, "3"),
            Tool::Roundhouse => (KeyCode::Digit4, "4"),
            Tool::Track => (KeyCode::Digit5, "5"),
            Tool::Steps => (KeyCode::Digit6, "6"),
            Tool::Jetty => (KeyCode::Digit7, "7"),
            Tool::Bridge => (KeyCode::Digit8, "8"),
            Tool::Opening => (KeyCode::Digit9, "9"),
            Tool::Roof => (KeyCode::Digit0, "0"),
            Tool::Tree => (KeyCode::KeyG, "G"),
            Tool::Prop => (KeyCode::KeyC, "C"),
            Tool::Paint => (KeyCode::KeyP, "P"),
            Tool::Raise => (KeyCode::KeyR, "R"),
            Tool::Flatten => (KeyCode::KeyF, "F"),
            Tool::Lagoon => (KeyCode::KeyV, "V"),
            Tool::Dye => (KeyCode::KeyI, "I"),
            Tool::Erase => (KeyCode::KeyX, "X"),
        }
    }
}

/// Selected tool plus every variant knob (DESIGN.md §2.2). Ranges are the HUD slider ranges.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct ToolSettings {
    pub tool: Tool,
    /// Huts, roundhouses, beacons.
    pub material: Material,
    /// Dykes.
    pub wall_kind: WallKind,
    /// Huts, roundhouses, roof tool.
    pub roof: RoofStyle,
    /// Roof pitch in degrees, 15..=60.
    pub roof_pitch_deg: f32,
    /// Huts: carved gable horns and painted bands.
    pub carved: bool,
    /// Huts: fish, nets and shell strings under the eaves.
    pub hangings: bool,
    /// Dykes: copestones / painted tips / thrift.
    pub crest: bool,
    /// Dykes: collapsed sections.
    pub tumbled: bool,
    /// Dyke thickness in metres, 0.3..=1.2.
    pub wall_thickness: f32,
    /// Dyke boulder footing height in metres, 0..=2.
    pub footing: f32,
    /// Hut stilt height in metres, 0..=3 (0 = automatic).
    pub stilts: f32,
    /// Salt bleaching and lichen, 0..=1.
    pub weathering: f32,
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
    /// Dye: what the last Shift+click pick copied (`HUT`, `DYKE`, ...). Only that structure's
    /// material control stays enabled; cleared when another tool is chosen.
    pub dye: Option<&'static str>,
    /// Written by the Roundhouse tool: the ghost being raised is tall enough to be a beacon.
    pub ghost_beacon: bool,
}

impl Default for ToolSettings {
    fn default() -> Self {
        Self {
            tool: Tool::Dyke,
            material: Material::Wattle,
            wall_kind: WallKind::DryStone,
            roof: RoofStyle::Thatch,
            roof_pitch_deg: 30.0,
            carved: false,
            hangings: true,
            crest: true,
            tumbled: false,
            wall_thickness: 0.6,
            footing: 0.0,
            stilts: 0.0,
            weathering: 0.25,
            fence: FenceKind::Wattle,
            path: PathKind::Shell,
            path_width: 1.6,
            stairs: StairKind::Stone,
            bridge: BridgeKind::Rope { railing: 0 },
            hanging: Hanging::None,
            opening: OpeningKind::Window,
            tree: TreeKind::Palm,
            clutter: ClutterKind::Canoe,
            paint: PaintLayer::SeaPinks,
            lower: false,
            brush_radius: 3.0,
            brush_strength: 0.5,
            water_depth: 0.8,
            colors: Colors::default(),
            wall_height: 4.0,
            stair_width: 1.2,
            bridge_width: 1.6,
            dye: None,
            ghost_beacon: false,
        }
    }
}

/// Feedback from the active tool to the HUD.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct ToolStatus {
    /// A drag / placement is in progress (the toolbar stays exactly as it is).
    pub drawing: bool,
    /// The footprint is fixed and the mouse sets the height (hints switch to MOVE / SCROLL / CLICK).
    pub raising: bool,
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
const BLOCK_HEIGHT: f32 = 2.4;
const ROUND_HEIGHT: f32 = 3.0;
const DECK_HEIGHT: f32 = 1.5;
/// An RMB press that moves less than this (px) is a tap = cancel.
const RMB_TAP_PX: f32 = 6.0;

const OUTSIDE: &str = "OUTSIDE THE ISLAND";
const TOO_SHORT: &str = "TOO SHORT";
const TOO_STEEP: &str = "TOO STEEP";
const NEEDS_ANCHORS: &str = "NEEDS TWO ANCHORS";
const NEEDS_WALL: &str = "NEEDS A HUT OR DYKE";
const NEEDS_RING: &str = "NEEDS A CLOSED DYKE RING";

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

/// Shuttered window → doorway → wide boat door → nothing (`None` removes the opening).
pub fn next_opening(k: OpeningKind) -> Option<OpeningKind> {
    match k {
        OpeningKind::Window => Some(OpeningKind::Door),
        OpeningKind::Door => Some(OpeningKind::WideDoor),
        OpeningKind::WideDoor => None,
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
        &[RoofStyle::Thatch, RoofStyle::Turf]
    } else {
        &[RoofStyle::Boat, RoofStyle::Hide, RoofStyle::Thatch, RoofStyle::Turf]
    };
    let i = all.iter().position(|x| *x == r).unwrap_or(0) as i32;
    all[(i + dir).rem_euclid(all.len() as i32) as usize]
}

fn cycle_fence(k: FenceKind, dir: i32) -> FenceKind {
    let all = [FenceKind::Driftwood, FenceKind::Wattle, FenceKind::NetLine, FenceKind::Reed];
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
        Tool::Windbreak => Structure::Fence(Fence { points, kind: s.fence, colors: s.colors }),
        Tool::Track => Structure::Path(Path { points, width: s.path_width, kind: s.path }),
        _ => Structure::Wall(Wall {
            points,
            height: s.wall_height,
            thickness: s.wall_thickness,
            kind: s.wall_kind,
            crest: s.crest,
            tumbled: s.tumbled,
            footing: s.footing,
            roofed: false,
            roof: s.roof,
            roof_pitch: pitch_frac(s.roof_pitch_deg),
            openings: Vec::new(),
            colors: s.colors,
            weathering: s.weathering,
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
                let poly = cairn_cove_core::geom::footprint_polygon(&Footprint::Rect { center, half, angle });
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
        (Tool::Jetty, Shape::Rect { center, half, angle }) => {
            Structure::Platform(Platform { center, half, angle, height, colors: s.colors })
        }
        (_, shape) => {
            let (footprint, round) = match shape {
                Shape::Rect { center, half, angle } => (Footprint::Rect { center, half, angle }, false),
                Shape::Circle { center, radius } => (Footprint::Round { center, radius }, true),
            };
            let roof = if round && s.roof == RoofStyle::Boat { RoofStyle::Thatch } else { s.roof };
            Structure::House(House {
                footprint,
                wall_height: height,
                roof,
                roof_pitch: pitch_frac(s.roof_pitch_deg),
                material: s.material,
                carved: s.carved,
                hangings: s.hangings,
                stilts: if round { 0.0 } else { s.stilts },
                openings: Vec::new(),
                colors: s.colors,
                weathering: s.weathering,
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

/// Floor height of a hut as the core builds it: flattened ground + 0.1 m, or on stilts the
/// highest ground under it plus the stilt height (never less than 1.2 m over water).
pub fn hut_floor(h: &House, terrain: &Terrain) -> f32 {
    let poly = footprint_polygon(&h.footprint);
    let c = poly.iter().copied().sum::<Vec2>() / poly.len().max(1) as f32;
    let (mut lo, mut hi, mut wet) = (f32::INFINITY, f32::NEG_INFINITY, false);
    for p in poly.iter().copied().chain([c]) {
        let g = terrain.height(p);
        lo = lo.min(g);
        hi = hi.max(g);
        wet |= terrain.water_level(p).is_some_and(|l| g < l);
    }
    if !needs_stilts(h, terrain) {
        return lo + 0.1;
    }
    let f = hi + h.stilts.clamp(0.0, 8.0).max(0.5);
    if wet { f.max(1.2) } else { f }
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
    (kind != StairKind::RopeLadder && rise > run * MAX_STAIR_SLOPE).then_some(TOO_STEEP)
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
        (Some(i), false) => match next_opening(openings[i].kind) {
            Some(k) => openings[i].kind = k,
            None => {
                openings.remove(i);
            }
        },
        (None, false) => openings.push(Opening { along: hit.along, kind: place }),
    }
    Ok(d)
}

/// A ray that passes straight through an opening (a doorway has no mesh to hit) still clicks it:
/// the nearest crossing of a dyke or hut wall, nearer than `max_t` along the ray, where the wall
/// has an opening at that spot. Returns the hit as if the wall had been picked there.
pub fn through_opening(doc: &Document, terrain: &Terrain, origin: Vec3, dir: Vec3, max_t: f32) -> Option<Hit> {
    let (o2, d2) = (origin.xz(), dir.xz());
    let mut best: Option<(f32, Hit)> = None;
    for (id, st) in &doc.structures {
        let (pts, closed, height, openings) = match st {
            Structure::Wall(w) => (w.points.clone(), false, w.height, &w.openings),
            Structure::House(h) => (footprint_polygon(&h.footprint), true, h.wall_height, &h.openings),
            _ => continue,
        };
        let n = pts.len();
        for i in 0..if closed { n } else { n.saturating_sub(1) } {
            let (a, e) = (pts[i], pts[(i + 1) % n] - pts[i]);
            let den = d2.perp_dot(e);
            if den.abs() < 1e-6 {
                continue;
            }
            let w = a - o2;
            let (t, u) = (w.perp_dot(e) / den, w.perp_dot(d2) / den);
            if !(0.0..=max_t).contains(&t) || !(0.0..=1.0).contains(&u) || best.as_ref().is_some_and(|(bt, _)| *bt <= t) {
                continue;
            }
            let point = origin + dir * t;
            let ground = terrain.height(point.xz());
            let along = cairn_cove_core::scene::along_of(st, point.xz());
            if point.y >= ground - 0.1 && point.y <= ground + height && openings.iter().any(|o| (o.along - along).abs() < OPENING_GRAB) {
                best = Some((t, Hit { id: Some(*id), point, normal: Vec3::new(e.y, 0.0, -e.x).normalize_or_zero(), along }));
            }
        }
    }
    best.map(|(_, h)| h)
}

/// `opening_click` on the picked hit; if that fails, retry on an opening the ray passed through.
fn opening_or_through(doc: &Document, terrain: &Terrain, ray: Option<(Vec3, Vec3)>, hit: &Hit, place: OpeningKind, remove: bool) -> Result<(Document, Hit), &'static str> {
    match opening_click(doc, hit, place, remove) {
        Ok(d) => Ok((d, *hit)),
        Err(e) => {
            let (o, d) = ray.ok_or(e)?;
            let max_t = (hit.point - o).dot(d.normalize_or_zero()) + 0.5;
            let th = through_opening(doc, terrain, o, d.normalize_or_zero(), max_t).ok_or(e)?;
            opening_click(doc, &th, place, remove).map(|d| (d, th))
        }
    }
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
            w.crest |= s.roof == RoofStyle::Turf && s.crest;
        }
        Some(Structure::House(h)) if !remove => {
            let round = matches!(h.footprint, Footprint::Round { .. });
            h.roof = if round && s.roof == RoofStyle::Boat { RoofStyle::Thatch } else { s.roof };
            h.roof_pitch = pitch_frac(s.roof_pitch_deg);
            h.hangings = s.hangings;
        }
        _ => return Err(NEEDS_RING),
    }
    Ok(d)
}

/// Name of what a Dye pick copied (shown in the name plate: `DYE: HUT`).
pub fn dye_name(st: &Structure) -> &'static str {
    match st {
        Structure::Wall(_) => "DYKE",
        Structure::House(_) => "HUT",
        Structure::Fence(_) => "WINDBREAK",
        Structure::Path(_) => "TRACK",
        Structure::Stairs(_) => "STEPS",
        Structure::Platform(_) => "JETTY",
        Structure::Bridge(_) => "BRIDGE",
        Structure::Placed(p) => match p.kind {
            PropKind::Tree(_) => "TREE",
            PropKind::Clutter(_) => "PROP",
        },
    }
}

/// Dye Shift+click: copy a structure's style into the settings.
pub fn copy_style(st: &Structure, s: &mut ToolSettings) {
    s.dye = Some(dye_name(st));
    match st {
        Structure::Wall(w) => {
            s.wall_kind = w.kind;
            s.crest = w.crest;
            s.tumbled = w.tumbled;
            s.wall_thickness = w.thickness.clamp(0.3, 1.2);
            s.footing = w.footing.clamp(0.0, 2.0);
            s.weathering = w.weathering.clamp(0.0, 1.0);
            s.roof = w.roof;
            s.roof_pitch_deg = pitch_deg(w.roof_pitch);
            s.wall_height = w.height.clamp(2.0, 14.0);
            s.colors = w.colors;
        }
        Structure::House(h) => {
            s.material = h.material;
            s.roof = h.roof;
            s.roof_pitch_deg = pitch_deg(h.roof_pitch);
            s.carved = h.carved;
            s.stilts = h.stilts.clamp(0.0, 3.0);
            s.hangings = h.hangings;
            s.weathering = h.weathering.clamp(0.0, 1.0);
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

/// Dye click: apply the settings' style to a structure (geometry untouched).
pub fn apply_style(st: &mut Structure, s: &ToolSettings) {
    match st {
        Structure::Wall(w) => {
            w.kind = s.wall_kind;
            w.crest = s.crest;
            w.tumbled = s.tumbled;
            w.thickness = s.wall_thickness;
            w.footing = s.footing;
            w.weathering = s.weathering;
            w.roof = s.roof;
            w.roof_pitch = pitch_frac(s.roof_pitch_deg);
            w.colors = s.colors;
        }
        Structure::House(h) => {
            let round = matches!(h.footprint, Footprint::Round { .. });
            h.material = s.material;
            h.roof = if round && s.roof == RoofStyle::Boat { RoofStyle::Thatch } else { s.roof };
            h.roof_pitch = pitch_frac(s.roof_pitch_deg);
            h.carved = s.carved;
            h.stilts = if round { 0.0 } else { s.stilts };
            h.hangings = s.hangings;
            h.weathering = s.weathering;
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
        Tool::Raise => {
            let sign = if s.lower ^ shift { -1.0 } else { 1.0 };
            TerrainOp::Sculpt { amount: sign * s.brush_strength.clamp(0.1, 1.0) * 0.35 }
        }
        Tool::Flatten => TerrainOp::Flatten { height: level, strength: s.brush_strength.clamp(0.1, 1.0) },
        Tool::Lagoon => TerrainOp::Water { depth: s.water_depth.clamp(0.3, 1.5) * 0.4 },
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
    /// What the current preview was built from.
    last_pending: Option<Pending>,
    /// Glade revision after this system last ran; a different value at the next run means
    /// someone else (HUD undo / redo, reset) changed the document under the gesture.
    revision: u64,
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
        // DESIGN B1: ghost-ok #A8FFD8 @45%, ghost-bad #FF5A4E @55%, ghost-pick #FFC21A @50%.
        match self {
            Tint::Ok => Color::srgba(0.659, 1.0, 0.847, 0.45),
            Tint::Bad => Color::srgba(1.0, 0.353, 0.306, 0.55),
            Tint::Pick => Color::srgba(1.0, 0.761, 0.102, 0.5),
        }
    }
}

enum Mark {
    Ring(Vec3, f32, Tint),
    Part(StructureId, Tint),
}

/// What the preview should show on top of the committed document. Compared frame to frame so
/// the preview document is only rebuilt when the gesture actually changed.
#[derive(Clone, Debug, PartialEq)]
enum Pending {
    Add(Structure),
    /// The stroke's working document, identified by its dab count.
    Stroke(usize),
    Remove(Vec<StructureId>),
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
    pending: Option<Pending>,
    invalid: Option<&'static str>,
    marks: Vec<Mark>,
    /// Footprint fixed, mouse sets the height.
    raising: bool,
    /// The roundhouse being raised is tall enough to be a beacon.
    beacon: bool,
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
        self.pending = None;
    }
    fn commit_add(&mut self, st: Structure) {
        let doc = with_structure(self.glade.committed(), st);
        self.commit(doc);
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
    let mut cancel = keys.just_pressed(KeyCode::Escape) || (st.revision != 0 && st.revision != g.revision);
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
    if s.tool != Tool::Dye {
        s.dye = None;
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
        st.last_pending = None;
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
    // The roof tool's scroll (pitch / style) works while hovering the world.
    let roof_scroll = idle && s.tool == Tool::Roof && ray.is_some() && scene.is_some();
    if (!idle || roof_scroll) && !blocked {
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
            pending: None,
            invalid: None,
            marks: Vec::new(),
            raising: false,
            beacon: false,
        };
        match cx.s.tool {
            Tool::Dyke | Tool::Windbreak | Tool::Track => line_tool(&mut cx),
            Tool::Hut | Tool::Roundhouse | Tool::Jetty => shape_tool(&mut cx),
            Tool::Steps | Tool::Bridge => span_tool(&mut cx),
            Tool::Tree | Tool::Prop => prop_tool(&mut cx),
            Tool::Paint | Tool::Raise | Tool::Flatten | Tool::Lagoon => brush_tool(&mut cx),
            Tool::Opening | Tool::Roof | Tool::Dye => click_tool(&mut cx),
            Tool::Erase => erase_tool(&mut cx),
        }
        let drawing = !matches!(cx.phase, Phase::Idle);
        if drawing {
            let pending = cx.pending.take();
            if pending.is_none() {
                cx.glade.set_preview(None);
            } else if pending != st.last_pending || !cx.glade.has_preview() {
                let base = cx.glade.committed();
                let doc = match (&pending, &cx.phase) {
                    (Some(Pending::Add(a)), _) => Some(with_structure(base, a.clone())),
                    (Some(Pending::Stroke(_)), Phase::Stroke { doc, .. }) => Some(doc.clone()),
                    (Some(Pending::Remove(ids)), _) => {
                        let mut d = base.clone();
                        d.structures.retain(|(id, _)| !ids.contains(id));
                        Some(d)
                    }
                    _ => None,
                };
                cx.glade.set_preview(doc);
            }
            st.last_pending = pending;
        } else if cx.glade.has_preview() {
            cx.glade.set_preview(None);
        }
        new_status = ToolStatus { drawing, raising: cx.raising, invalid: cx.invalid.map(str::to_string) };
        cx.s.ghost_beacon = cx.beacon;
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

    st.revision = glade.revision;
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
                Tool::Dyke => cx.s.wall_height = (cx.s.wall_height + inp.steps as f32 * 0.5).clamp(2.0, 14.0),
                Tool::Track => cx.s.path_width = (cx.s.path_width + inp.steps as f32 * 0.2).clamp(0.8, 4.0),
                _ if inp.steps != 0 => cx.s.fence = cycle_fence(cx.s.fence, inp.steps.signum()),
                _ => {}
            }
            let last = *pts.last().unwrap();
            let raw = inp.ground.map_or(prev, |g| q2(g.xz()));
            let mut end = if inp.shift { snap45(last, raw) } else { raw };
            let closing = tool != Tool::Track && close_snap(&pts, end);
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
            let st = (all.len() >= 2).then(|| line_structure(tool, cx.s, all));
            if finish {
                if let (Some(st), None) = (st, cx.invalid) {
                    cx.commit_add(st);
                }
                return;
            }
            if let Some(g) = inp.ground {
                cx.ring(g, if closing { 0.8 } else { 0.4 });
            }
            cx.pending = st.map(Pending::Add);
            cx.phase = Phase::Line { pts, end, held, moved };
        }
        other => cx.phase = other,
    }
}

// --- Block / Tower / Deck -----------------------------------------------------------------------

fn default_height(tool: Tool) -> f32 {
    match tool {
        Tool::Roundhouse => ROUND_HEIGHT,
        Tool::Jetty => DECK_HEIGHT,
        _ => BLOCK_HEIGHT,
    }
}

fn height_range(tool: Tool) -> (f32, f32) {
    match tool {
        Tool::Jetty => (0.3, 8.0),
        Tool::Roundhouse => (1.5, 30.0),
        _ => (1.2, 4.5),
    }
}

fn shape_of(tool: Tool, a: Vec2, b: Vec2, angle: f32, shift: bool) -> Shape {
    if tool == Tool::Roundhouse {
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
            if tool != Tool::Roundhouse {
                angle += inp.steps as f32 * 15f32.to_radians();
            }
            let shape = shape_of(tool, a, b, angle, inp.shift);
            cx.invalid = shape.invalid();
            if inp.release || !inp.held {
                if cx.invalid.is_none() {
                    let base = default_height(tool);
                    cx.pending = Some(Pending::Add(shape_structure(tool, cx.s, shape, base)));
                    cx.phase = Phase::Raise { shape, base, anchor_y: inp.cursor_y };
                } else {
                    cx.invalid = None;
                }
                return;
            }
            // Low ghost while dragging the footprint.
            let h = if tool == Tool::Jetty { DECK_HEIGHT } else { 0.6 };
            cx.pending = Some(Pending::Add(shape_structure(tool, cx.s, shape, h)));
            if let Some(g) = inp.ground {
                cx.ring(g, 0.3);
            }
            cx.phase = Phase::Footprint { a, b, angle };
        }
        Phase::Raise { shape, mut base, anchor_y } => {
            let (lo, hi) = height_range(tool);
            match tool {
                Tool::Jetty => base += inp.steps as f32 * 0.25,
                Tool::Roundhouse if inp.steps != 0 => cx.s.roof = cycle_roof(cx.s.roof, inp.steps.signum(), true),
                _ if inp.steps != 0 && inp.shift => cx.s.roof = cycle_roof(cx.s.roof, inp.steps.signum(), false),
                _ => cx.s.roof_pitch_deg = (cx.s.roof_pitch_deg + inp.steps as f32 * 5.0).clamp(15.0, 60.0),
            }
            let per_px = VIEW_HEIGHT * cx.rig.zoom / (cx.win_h * cx.rig.pitch_deg.to_radians().cos().max(0.3));
            let height = q((base + (anchor_y - inp.cursor_y) * per_px).clamp(lo, hi));
            base = base.clamp(lo - 30.0, hi + 30.0);
            let st = shape_structure(tool, cx.s, shape, height);
            if inp.press {
                cx.commit_add(st);
                return;
            }
            cx.raising = true;
            cx.beacon = matches!(&st, Structure::House(h) if is_beacon(h));
            cx.pending = Some(Pending::Add(st));
            cx.phase = Phase::Raise { shape, base, anchor_y };
        }
        other => cx.phase = other,
    }
}

// --- Stairs / Bridge -----------------------------------------------------------------------------

/// End point of a span under the cursor: structure top, or ground (bridges sit a little above
/// ground / water). Second value: is it a valid anchor.
fn span_anchor(cx: &Cx, skip: Option<StructureId>) -> Option<(Vec3, bool)> {
    // Steps onto a hut land on its floor (the raised floor of a stilted hut), not its roof.
    if cx.s.tool == Tool::Steps
        && let Some((o, d)) = cx.inp.ray
        && let Some(h) = cx.pick(o, d, skip)
        && h.normal.y < 0.6
        && let Some(Structure::House(hs)) = h.id.and_then(|id| cx.glade.doc().get(id))
    {
        let floor = hut_floor(hs, &cx.scene.terrain);
        return Some((Vec3::new(q(h.point.x), floor.min(h.point.y), q(h.point.z)), true));
    }
    let (p, id) = cx.surface(skip)?;
    let p = Vec3::new(q(p.x), p.y, q(p.z));
    if id.is_some() || cx.s.tool == Tool::Steps {
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
            let st = (a.distance(b) > 0.3).then(|| span_structure(tool, cx.s, a, b));
            if inp.release || !inp.held {
                if let (Some(st), None) = (st, cx.invalid) {
                    cx.commit_add(st);
                }
                cx.invalid = None;
                return;
            }
            cx.ring(b, 0.4);
            cx.pending = st.map(Pending::Add);
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
    let st = Structure::Placed(Placed { at, angle, scale, kind, colors: cx.s.colors });
    if inp.release || !inp.held {
        if cx.invalid.is_none() {
            cx.commit_add(st);
        }
        cx.invalid = None;
        return;
    }
    cx.ring(g, 0.5);
    cx.pending = Some(Pending::Add(st));
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
                cx.pending = Some(Pending::Stroke(doc.terrain.len()));
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
                } else if cx.s.tool == Tool::Raise && inp.held {
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
            cx.pending = Some(Pending::Stroke(doc.terrain.len()));
            cx.phase = Phase::Stroke { doc, last, still, level };
        }
        other => cx.phase = other,
    }
}

// --- Window/Door, Roof, Style --------------------------------------------------------------------

fn click_tool(cx: &mut Cx) {
    let inp = cx.inp;
    cx.phase = Phase::Idle;
    if cx.s.tool == Tool::Roof && inp.steps != 0 {
        if inp.shift {
            cx.s.roof = cycle_roof(cx.s.roof, inp.steps.signum(), false);
        } else {
            cx.s.roof_pitch_deg = (cx.s.roof_pitch_deg + inp.steps as f32 * 5.0).clamp(15.0, 60.0);
        }
    }
    let Some(hit) = cx.hover() else { return };
    let committed = cx.glade.committed();
    match cx.s.tool {
        Tool::Opening => match opening_or_through(committed, &cx.scene.terrain, cx.inp.ray, &hit, cx.s.opening, inp.shift) {
            Ok((doc, hit)) => {
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
                if let Some(st) = committed.get(id) {
                    let st = st.clone();
                    copy_style(&st, cx.s);
                }
            } else {
                let mut doc = committed.clone();
                if let Some(st) = doc.get_mut(id) {
                    apply_style(st, cx.s);
                }
                cx.commit(doc);
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
    if inp.release || !inp.held {
        if !removed.is_empty() {
            let mut doc = cx.glade.committed().clone();
            doc.structures.retain(|(id, _)| !removed.contains(id));
            cx.commit(doc);
        }
        return;
    }
    cx.pending = (!removed.is_empty()).then(|| Pending::Remove(removed.clone()));
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
        let mut k = Some(OpeningKind::Window);
        let mut seq = Vec::new();
        while let Some(cur) = k {
            k = next_opening(cur);
            seq.push(k);
        }
        assert_eq!(seq, [Some(OpeningKind::Door), Some(OpeningKind::WideDoor), None]);
    }

    fn wall_doc(points: Vec<Vec2>) -> (Document, StructureId) {
        let mut d = Document::new(1);
        let id = d.add(line_structure(Tool::Dyke, &ToolSettings::default(), points));
        (d, id)
    }

    #[test]
    fn opening_click_cycles_window_door_wide_none_and_shift_removes() {
        let (d, id) = wall_doc(vec![Vec2::new(-5.0, 0.0), Vec2::new(5.0, 0.0)]);
        let hit = Hit { id: Some(id), point: Vec3::new(0.0, 2.0, 0.0), normal: Vec3::Z, along: 5.0 };
        let near = Hit { along: 5.5, ..hit };
        let kinds = |d: &Document| match d.get(id) {
            Some(Structure::Wall(w)) => w.openings.iter().map(|o| o.kind).collect::<Vec<_>>(),
            _ => panic!(),
        };
        let d1 = opening_click(&d, &hit, OpeningKind::Window, false).unwrap();
        assert_eq!(kinds(&d1), [OpeningKind::Window]);
        let d2 = opening_click(&d1, &near, OpeningKind::Window, false).unwrap();
        assert_eq!(kinds(&d2), [OpeningKind::Door]);
        let d3 = opening_click(&d2, &near, OpeningKind::Window, false).unwrap();
        assert_eq!(kinds(&d3), [OpeningKind::WideDoor]);
        let d4 = opening_click(&d3, &near, OpeningKind::Window, false).unwrap();
        assert!(kinds(&d4).is_empty(), "third click on a wide door removes it");
        let d5 = opening_click(&d2, &hit, OpeningKind::Window, true).unwrap();
        assert!(kinds(&d5).is_empty(), "shift+click removes");
        let ground = Hit { id: None, ..hit };
        assert_eq!(opening_click(&d, &ground, OpeningKind::Window, false), Err(NEEDS_WALL));
        assert_eq!(NEEDS_WALL, "NEEDS A HUT OR DYKE");
    }

    #[test]
    fn a_ray_through_a_wide_door_still_clicks_it() {
        let (mut d, id) = wall_doc(vec![Vec2::new(-5.0, 0.0), Vec2::new(5.0, 0.0)]);
        if let Some(Structure::Wall(w)) = d.get_mut(id) {
            w.openings.push(Opening { along: 5.0, kind: OpeningKind::WideDoor });
        }
        let t = Terrain::build(1, &[], &[]);
        let dir = Vec3::new(1.0, 1.2, 1.0).normalize();
        let aim = |x: f32| {
            let target = Vec3::new(x, t.height(Vec2::new(x, 0.0)) + 1.0, 0.0);
            (target + dir * 20.0, -dir)
        };
        let (o, rd) = aim(0.0);
        let hit = through_opening(&d, &t, o, rd, 100.0).expect("ray through the doorway");
        assert_eq!(hit.id, Some(id));
        assert!((hit.along - 5.0).abs() < 0.3, "{}", hit.along);
        // Cycling: the doorway click removes the wide door.
        let (doc, _) = opening_or_through(&d, &t, Some((o, rd)), &Hit { id: None, point: Vec3::new(2.0, 0.0, -2.0), normal: Vec3::Y, along: 0.0 }, OpeningKind::Window, false).unwrap();
        assert!(matches!(doc.get(id), Some(Structure::Wall(w)) if w.openings.is_empty()));
        // A ray through solid wall far from the opening is not a hit.
        let (o, rd) = aim(3.5);
        assert!(through_opening(&d, &t, o, rd, 100.0).is_none());
    }

    #[test]
    fn hut_floor_follows_ground_and_stilts() {
        let t = Terrain::build(3, &[], &[]);
        let c = Vec2::new(0.0, 0.0);
        let hut = |stilts: f32| House {
            footprint: Footprint::Rect { center: c, half: Vec2::splat(2.0), angle: 0.0 },
            wall_height: 2.4,
            roof: RoofStyle::Thatch,
            roof_pitch: 0.6,
            material: Material::Wattle,
            carved: false,
            hangings: true,
            stilts,
            openings: Vec::new(),
            colors: Colors::default(),
            weathering: 0.0,
        };
        let ground = (0..16).map(|i| t.height(c + Vec2::from_angle(i as f32 * 0.4) * 2.0)).fold(f32::INFINITY, f32::min);
        let on_ground = hut_floor(&hut(0.0), &t);
        assert!(on_ground.is_finite());
        if !needs_stilts(&hut(0.0), &t) {
            assert!(on_ground >= ground - 0.5 && on_ground <= ground + 1.0);
        }
        let lifted = hut_floor(&hut(2.0), &t);
        assert!(lifted >= on_ground + 0.5, "{lifted} vs {on_ground}");
    }

    #[test]
    fn dye_pick_names_the_structure_and_tool_defaults_match_the_design() {
        let mut s = ToolSettings::default();
        let st = shape_structure(Tool::Hut, &s, Shape::Rect { center: Vec2::ZERO, half: Vec2::ONE * 3.0, angle: 0.0 }, 2.4);
        copy_style(&st, &mut s);
        assert_eq!(s.dye, Some("HUT"));
        let wall = line_structure(Tool::Dyke, &s, vec![Vec2::ZERO, Vec2::X * 4.0]);
        copy_style(&wall, &mut s);
        assert_eq!(s.dye, Some("DYKE"));
        let d = ToolSettings::default();
        assert_eq!((d.wall_kind, d.material, d.roof, d.crest, d.carved, d.hangings), (WallKind::DryStone, Material::Wattle, RoofStyle::Thatch, true, false, true));
        assert_eq!((d.stilts, d.weathering, d.footing, d.fence, d.path, d.stairs), (0.0, 0.25, 0.0, FenceKind::Wattle, PathKind::Shell, StairKind::Stone));
        assert_eq!((d.bridge, d.tree, d.clutter, d.paint, d.colors), (BridgeKind::Rope { railing: 0 }, TreeKind::Palm, ClutterKind::Canoe, PaintLayer::SeaPinks, Colors::default()));
    }

    #[test]
    fn roundhouse_beacon_threshold_and_stilt_rules() {
        let s = ToolSettings { stilts: 2.0, carved: true, ..default() };
        let round = |h: f32| shape_structure(Tool::Roundhouse, &s, Shape::Circle { center: Vec2::ZERO, radius: 3.0 }, h);
        let Structure::House(low) = round(ROUND_HEIGHT) else { panic!() };
        let Structure::House(tall) = round(7.5) else { panic!() };
        assert!(!is_beacon(&low) && is_beacon(&tall), "beacon at wall height >= 2.5 x radius");
        assert_eq!(low.stilts, 0.0, "round houses never take the hut stilts slider");
        assert!(low.carved, "carved passes through for the core to decide");
        let Structure::House(hut) = shape_structure(Tool::Hut, &s, Shape::Rect { center: Vec2::ZERO, half: Vec2::ONE * 3.0, angle: 0.0 }, 2.4) else { panic!() };
        assert_eq!(hut.stilts, 2.0);
        // Boat roofs fall back to thatch on round footprints.
        let boat = ToolSettings { roof: RoofStyle::Boat, ..default() };
        let Structure::House(r) = shape_structure(Tool::Roundhouse, &boat, Shape::Circle { center: Vec2::ZERO, radius: 3.0 }, 3.0) else { panic!() };
        assert_eq!(r.roof, RoofStyle::Thatch);
    }

    #[test]
    fn every_tool_has_a_unique_hotkey_label_and_glyph() {
        use std::collections::HashSet;
        let keys: HashSet<_> = Tool::ALL.iter().map(|t| t.hotkey().1).collect();
        let labels: HashSet<_> = Tool::ALL.iter().map(|t| t.label()).collect();
        let glyphs: HashSet<_> = Tool::ALL.iter().map(|t| t.glyph()).collect();
        assert_eq!((keys.len(), labels.len(), glyphs.len()), (18, 18, 18));
        assert_eq!(Tool::ALL.len(), 18);
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
        assert_eq!(span_invalid(Tool::Steps, StairKind::Log, a, Vec3::new(1.0, 4.0, 0.0), (true, true)), Some(TOO_STEEP));
        assert_eq!(span_invalid(Tool::Steps, StairKind::RopeLadder, a, Vec3::new(1.0, 4.0, 0.0), (true, true)), None);
        assert_eq!(span_invalid(Tool::Steps, StairKind::Log, a, Vec3::new(4.0, 3.0, 0.0), (true, true)), None);
        assert_eq!(span_invalid(Tool::Bridge, StairKind::Log, a, Vec3::new(2.0, 0.0, 0.0), (true, true)), Some(NEEDS_ANCHORS));
        assert_eq!(span_invalid(Tool::Bridge, StairKind::Log, a, Vec3::new(6.0, 0.0, 0.0), (true, false)), Some(NEEDS_ANCHORS));
        assert_eq!(span_invalid(Tool::Bridge, StairKind::Log, a, Vec3::new(6.0, 0.0, 0.0), (true, true)), None);
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
        let mut s = ToolSettings { material: Material::Plank, roof: RoofStyle::Hide, roof_pitch_deg: 40.0, ..default() };
        let mut st = shape_structure(Tool::Hut, &ToolSettings::default(), Shape::Rect { center: Vec2::ZERO, half: Vec2::ONE * 3.0, angle: 0.0 }, 5.0);
        apply_style(&mut st, &s);
        s = ToolSettings::default();
        copy_style(&st, &mut s);
        assert_eq!((s.material, s.roof), (Material::Plank, RoofStyle::Hide));
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
