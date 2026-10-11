//! Tool input: keys, picking, drag gestures, ghost preview, commit/undo/redo.
//! The gesture state machine ([`Gesture`]) is pure so it can be tested without a window.

use crate::camera::{CursorRay, cursor_ray, pointer_over_ui};
use crate::render::{GameCamera, LowRes};
use crate::scene::{CommittedGrid, random_seed};
use crate::game::{Ghost, Tool, ToolPhase, ToolState, UiCommand, World};
use crate::render::{ToonMaterial, toon_material};
use acre_core::{Document, History, Structure, VoxelGrid, ground_height};
use bevy::light::NotShadowCaster;
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

/// Minimum spacing between recorded freehand points (world units).
pub const POINT_SPACING: f32 = 0.75;
/// Simplification tolerance for freehand strokes.
pub const SIMPLIFY_EPS: f32 = 0.35;
/// Raze brush radius.
pub const RAZE_RADIUS: f32 = 1.5;
/// Screen pixels of vertical mouse travel per extra floor.
pub const PX_PER_FLOOR: f32 = 18.0;
/// Max recorded stroke points before the stroke is thinned.
const MAX_POINTS: usize = 256;
const MIN_STROKE_LEN: f32 = 2.0;
const MIN_RECT_SIDE: f32 = 2.0;
const MAP_HALF: f32 = 48.0;
const MAP_LIMIT: f32 = MAP_HALF - 2.0;

type P2 = [f32; 2];

// ------------------------------------------------------------------ pure helpers

/// Ramer–Douglas–Peucker polyline simplification.
pub fn simplify(pts: &[P2], eps: f32) -> Vec<P2> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let (a, b) = (pts[0], pts[pts.len() - 1]);
    let (mut far, mut far_d) = (0, 0.0_f32);
    for (i, &p) in pts.iter().enumerate().skip(1).take(pts.len() - 2) {
        let d = seg_dist(p, a, b);
        if d > far_d {
            (far, far_d) = (i, d);
        }
    }
    if far_d <= eps {
        return vec![a, b];
    }
    let mut left = simplify(&pts[..=far], eps);
    left.pop();
    left.extend(simplify(&pts[far..], eps));
    left
}

fn seg_dist(p: P2, a: P2, b: P2) -> f32 {
    let ab = [b[0] - a[0], b[1] - a[1]];
    let l2 = ab[0] * ab[0] + ab[1] * ab[1];
    let t = if l2 > 0.0 { (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / l2).clamp(0.0, 1.0) } else { 0.0 };
    ((p[0] - a[0] - ab[0] * t).powi(2) + (p[1] - a[1] - ab[1] * t).powi(2)).sqrt()
}

fn dist(a: P2, b: P2) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

fn path_len(p: &[P2]) -> f32 {
    p.windows(2).map(|w| dist(w[0], w[1])).sum()
}

/// Axis-aligned rectangle spanned by two drag corners, `None` if a side is shorter than `min_side`.
pub fn rect_from_drag(a: P2, b: P2, min_side: f32) -> Option<(P2, P2)> {
    let (min, max) = ([a[0].min(b[0]), a[1].min(b[1])], [a[0].max(b[0]), a[1].max(b[1])]);
    (max[0] - min[0] >= min_side && max[1] - min[1] >= min_side).then_some((min, max))
}

/// Floors from vertical mouse travel: moving up (negative `dy_px` = cursor above start) adds floors.
pub fn floors_from_drag(dy_px: f32) -> u8 {
    if !dy_px.is_finite() {
        return 1;
    }
    (1.0 + (-dy_px / PX_PER_FLOOR).floor()).clamp(1.0, 12.0) as u8
}

/// Marches a ray through `grid` and returns the first solid (non-air, non-vegetation) surface point.
pub fn pick_ground(grid: &VoxelGrid, ray: Ray3d) -> Option<Vec3> {
    let (o, d) = (ray.origin, *ray.direction);
    let solid = |p: Vec3| {
        let v = acre_core::world_to_voxel(p.to_array());
        let k = grid.get(v[0], v[1], v[2]);
        k != acre_core::Voxel::Air && !k.is_vegetation()
    };
    let (step, max_t) = (0.2, 500.0);
    let mut t = 0.0;
    let mut prev = 0.0;
    while t < max_t {
        let p = o + d * t;
        if p.y < -1.0 && d.y <= 0.0 {
            return None;
        }
        if solid(p) {
            let (mut lo, mut hi) = (prev, t);
            for _ in 0..8 {
                let mid = (lo + hi) * 0.5;
                if solid(o + d * mid) {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            let hit = o + d * hi;
            return (hit.x.abs() <= MAP_HALF && hit.z.abs() <= MAP_HALF).then_some(hit);
        }
        prev = t;
        t += step;
    }
    None
}

fn clamp_pt(p: P2) -> P2 {
    [p[0].clamp(-MAP_LIMIT, MAP_LIMIT), p[1].clamp(-MAP_LIMIT, MAP_LIMIT)]
}

// ------------------------------------------------------------------ gesture state machine

/// What a finished gesture asks the world to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Commit {
    Add(Structure),
    Remove(Vec<usize>),
}

/// Tool settings and committed document, as seen by a gesture.
pub struct Ctx<'a> {
    pub tool: Tool,
    pub tall: bool,
    pub spire_kind: u8,
    pub doc: &'a Document,
}

#[derive(Resource, Default, Debug)]
pub struct Gesture {
    phase: ToolPhase,
    /// Recorded stroke points (freehand), or `[start]` for rectangles.
    pts: Vec<P2>,
    cur: P2,
    hits: Vec<usize>,
    floors: u8,
    rect: Option<(P2, P2)>,
    y0: f32,
    /// Bumped on every visible change, so callers can skip recomputing the ghost.
    rev: u64,
}

impl Gesture {
    pub fn phase(&self) -> ToolPhase {
        self.phase
    }

    pub fn rev(&self) -> u64 {
        self.rev
    }

    pub fn cancel(&mut self) {
        let rev = self.rev + 1;
        *self = Gesture { rev, ..Gesture::default() };
    }

    /// Sets the rectangle of a Field/Block drag directly (screen-rect projected to the ground).
    pub fn set_rect(&mut self, min: P2, max: P2) {
        if self.phase == ToolPhase::Dragging && (self.pts[0] != min || self.cur != max) {
            self.pts = vec![min];
            self.cur = max;
            self.rev += 1;
        }
    }

    /// Left button pressed on ground point `p` at screen height `y_px`. May commit at once (Spire, Block confirm).
    pub fn press(&mut self, c: &Ctx, p: P2, y_px: f32) -> Option<Commit> {
        let p = clamp_pt(p);
        if self.phase == ToolPhase::Raising {
            let out = self.block_structure().map(Commit::Add);
            self.cancel();
            return out;
        }
        match c.tool {
            Tool::Spire => Some(Commit::Add(Structure::Spire { at: p, kind: c.spire_kind % 3 })),
            _ => {
                self.cancel();
                self.phase = ToolPhase::Dragging;
                self.pts = vec![p];
                self.cur = p;
                self.y0 = y_px;
                self.rev += 1;
                if c.tool == Tool::Raze {
                    self.raze_at(c, p);
                }
                None
            }
        }
    }

    /// Cursor moved to `p` while the button is held.
    pub fn drag(&mut self, c: &Ctx, p: P2) {
        if self.phase != ToolPhase::Dragging {
            return;
        }
        let p = clamp_pt(p);
        if p == self.cur && c.tool != Tool::Raze {
            return;
        }
        self.cur = p;
        self.rev += 1;
        match c.tool {
            Tool::Wall | Tool::Path => {
                if self.pts.last().is_none_or(|&l| dist(l, p) >= POINT_SPACING) {
                    self.pts.push(p);
                    if self.pts.len() >= MAX_POINTS {
                        self.pts = simplify(&self.pts, SIMPLIFY_EPS);
                        if self.pts.len() >= MAX_POINTS * 3 / 4 {
                            let last = self.pts[self.pts.len() - 1];
                            self.pts = self.pts.iter().step_by(2).copied().collect();
                            self.pts.push(last);
                        }
                    }
                }
            }
            Tool::Raze => {
                if let Some(&l) = self.pts.last() {
                    // Sample along the move so quick drags don't skip structures.
                    let n = (dist(l, p) / (RAZE_RADIUS * 0.5)).ceil().max(1.0) as usize;
                    for i in 1..=n {
                        let t = i as f32 / n as f32;
                        self.raze_at(c, [l[0] + (p[0] - l[0]) * t, l[1] + (p[1] - l[1]) * t]);
                    }
                }
                self.pts = vec![p];
            }
            _ => {}
        }
    }

    fn raze_at(&mut self, c: &Ctx, p: P2) {
        for (i, s) in c.doc.structures.iter().enumerate() {
            if !self.hits.contains(&i) && s.touches(p, RAZE_RADIUS) {
                self.hits.push(i);
                self.rev += 1;
            }
        }
    }

    /// Button released. Block footprints move to the Raising phase.
    pub fn release(&mut self, c: &Ctx, y_px: f32) -> Option<Commit> {
        if self.phase != ToolPhase::Dragging {
            return None;
        }
        let out = match c.tool {
            Tool::Wall | Tool::Path => self.stroke_structure(c).map(Commit::Add),
            Tool::Field => self.rect_structure().map(Commit::Add),
            Tool::Raze => (!self.hits.is_empty()).then(|| Commit::Remove(std::mem::take(&mut self.hits))),
            Tool::Block => {
                if let Some(r) = rect_from_drag(self.pts[0], self.cur, MIN_RECT_SIDE * 1.5) {
                    self.rect = Some(r);
                    self.phase = ToolPhase::Raising;
                    self.y0 = y_px;
                    self.floors = 1;
                    return None;
                }
                None
            }
            Tool::Spire => None,
        };
        self.cancel();
        out
    }

    /// Mouse y (screen px) while raising.
    pub fn raise(&mut self, y_px: f32) {
        if self.phase == ToolPhase::Raising {
            let f = floors_from_drag(y_px - self.y0);
            if f != self.floors {
                self.floors = f;
                self.rev += 1;
            }
        }
    }

    pub fn floors(&self) -> u8 {
        self.floors
    }

    fn stroke(&self) -> Vec<P2> {
        let mut pts = self.pts.clone();
        if pts.last().is_none_or(|&l| l != self.cur) {
            pts.push(self.cur);
        }
        simplify(&pts, SIMPLIFY_EPS)
    }

    fn stroke_structure(&self, c: &Ctx) -> Option<Structure> {
        let path = self.stroke();
        if path.len() < 2 || path_len(&path) < MIN_STROKE_LEN {
            return None;
        }
        Some(if c.tool == Tool::Wall { Structure::Wall { path, tall: c.tall } } else { Structure::Path { path } })
    }

    fn rect_structure(&self) -> Option<Structure> {
        rect_from_drag(self.pts[0], self.cur, MIN_RECT_SIDE).map(|(min, max)| Structure::Field { min, max })
    }

    fn block_structure(&self) -> Option<Structure> {
        self.rect.map(|(min, max)| Structure::Block { min, max, floors: self.floors.clamp(1, 12) })
    }

    /// The live preview for the current gesture.
    pub fn ghost(&self, c: &Ctx) -> Ghost {
        match (self.phase, c.tool) {
            (ToolPhase::Idle, _) => Ghost::default(),
            (ToolPhase::Raising, _) => Ghost { add: self.block_structure(), remove: vec![] },
            (_, Tool::Wall | Tool::Path) => Ghost { add: self.stroke_structure(c), remove: vec![] },
            (_, Tool::Field) => Ghost { add: self.rect_structure(), remove: vec![] },
            (_, Tool::Block) => Ghost {
                add: rect_from_drag(self.pts[0], self.cur, MIN_RECT_SIDE * 1.5)
                    .map(|(min, max)| Structure::Block { min, max, floors: 1 }),
                remove: vec![],
            },
            (_, Tool::Raze) => {
                let mut remove = self.hits.clone();
                remove.sort_unstable();
                Ghost { add: None, remove }
            }
            _ => Ghost::default(),
        }
    }
}

/// Applies a commit to the world history.
pub fn apply_commit(world: &mut World, commit: Commit) {
    let mut doc = world.history.doc().clone();
    match commit {
        Commit::Add(s) => doc.structures.push(s),
        Commit::Remove(mut idx) => {
            idx.sort_unstable();
            let mut i = 0;
            doc.structures.retain(|_| {
                i += 1;
                idx.binary_search(&(i - 1)).is_err()
            });
        }
    }
    world.history.push(doc);
}

// ------------------------------------------------------------------ Bevy systems

#[derive(Component)]
struct CursorMarker;

#[derive(Resource)]
struct MarkerMats {
    normal: Handle<ToonMaterial>,
    raze: Handle<ToonMaterial>,
}

pub struct ToolsPlugin;

impl Plugin for ToolsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Gesture>()
            .init_resource::<GroundCursor>()
            .add_systems(Startup, spawn_marker)
            .add_systems(Update, (keys_and_commands, pointer, update_marker).chain());
    }
}

fn spawn_marker(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<ToonMaterial>>,
) {
    let m = MarkerMats {
        normal: mats.add(toon_material(Color::srgb_u8(0xff, 0xc3, 0x5c), Color::srgb_u8(0xff, 0xc3, 0x5c), 0.4, 0.8)),
        raze: mats.add(toon_material(Color::srgb_u8(0xff, 0x7a, 0x6b), Color::srgb_u8(0xff, 0x7a, 0x6b), 0.4, 0.8)),
    };
    commands.spawn((
        CursorMarker,
        Mesh3d(meshes.add(Cuboid::new(0.5, 0.5, 0.5))),
        MeshMaterial3d(m.normal.clone()),
        Transform::default(),
        Visibility::Hidden,
        NotShadowCaster,
    ));
    commands.insert_resource(m);
}

fn keys_and_commands(
    keys: Res<ButtonInput<KeyCode>>,
    mut cmds: MessageReader<UiCommand>,
    mut state: ResMut<ToolState>,
    mut world: ResMut<World>,
    mut gesture: ResMut<Gesture>,
    mut ghost: ResMut<Ghost>,
) {
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    state.tall = shift;
    let mut select = None;
    let (mut undo, mut redo) = (false, false);
    if !ctrl {
        for (k, t) in [
            (KeyCode::Digit1, Tool::Wall),
            (KeyCode::Digit2, Tool::Block),
            (KeyCode::Digit3, Tool::Path),
            (KeyCode::Digit4, Tool::Field),
            (KeyCode::Digit5, Tool::Spire),
            (KeyCode::Digit6, Tool::Raze),
        ] {
            if keys.just_pressed(k) {
                select = Some(t);
            }
        }
        if keys.just_pressed(KeyCode::Tab) {
            state.spire_kind = (state.spire_kind + 1) % 3;
        }
    } else if keys.just_pressed(KeyCode::KeyZ) {
        if shift { redo = true } else { undo = true }
    } else if keys.just_pressed(KeyCode::KeyY) {
        redo = true;
    }
    for c in cmds.read() {
        match *c {
            UiCommand::SelectTool(t) => select = Some(t),
            UiCommand::Undo => undo = true,
            UiCommand::Redo => redo = true,
            _ => {}
        }
    }
    let mut cancel = keys.just_pressed(KeyCode::Escape);
    if !ctrl && keys.just_pressed(KeyCode::KeyN) {
        world.history = History::new(Document::new(random_seed()));
        cancel = true;
    }
    if let Some(t) = select {
        state.tool = t;
        cancel = true;
    }
    if undo || redo {
        cancel = true;
        if undo {
            world.history.undo();
        } else {
            world.history.redo();
        }
    }
    if cancel {
        gesture.cancel();
        state.phase = ToolPhase::Idle;
        if *ghost != Ghost::default() {
            *ghost = Ghost::default();
        }
    }
}

/// Ground point under the cursor, `None` over UI or off-map.
#[derive(Resource, Default)]
pub struct GroundCursor(pub Option<Vec3>);

type GameCam<'w, 's> = Query<'w, 's, (&'static Camera, &'static GlobalTransform), With<GameCamera>>;

#[allow(clippy::too_many_arguments)]
fn pointer(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    ray: Res<CursorRay>,
    pick: Res<CommittedGrid>,
    window: Query<&Window, With<PrimaryWindow>>,
    (hover, nodes): (Option<Res<HoverMap>>, Query<(), With<Node>>),
    (low, cam): (Option<Res<LowRes>>, GameCam),
    mut state: ResMut<ToolState>,
    mut world: ResMut<World>,
    mut gesture: ResMut<Gesture>,
    mut ghost: ResMut<Ghost>,
    mut cursor: ResMut<GroundCursor>,
    (mut last_y, mut right_down, mut screen0, mut last_rev): (Local<f32>, Local<Option<Vec2>>, Local<Vec2>, Local<u64>),
) {
    let over_ui = pointer_over_ui(hover.as_deref(), &nodes);
    let ground = if over_ui { None } else { ray.0.and_then(|r| pick_ground(&pick.grid, r)) };
    cursor.0 = ground;
    let cursor_px = window.single().ok().and_then(|w| w.cursor_position());
    if let Some(c) = cursor_px {
        *last_y = c.y;
    }
    let y_px = *last_y;

    // Right click cancels only when released without moving (otherwise it was an orbit).
    if mouse.just_pressed(MouseButton::Right) {
        *right_down = cursor_px;
    }
    let right_click = mouse.just_released(MouseButton::Right)
        && matches!((*right_down, cursor_px), (Some(a), Some(b)) if a.distance(b) < 6.0);

    let (mut commit, new_ghost) = {
        let doc = world.history.doc();
        let ctx = Ctx { tool: state.tool, tall: state.tall, spire_kind: state.spire_kind, doc };
        let mut commit = None;
        if gesture.phase() != ToolPhase::Idle && right_click {
            gesture.cancel();
        } else if gesture.phase() == ToolPhase::Raising {
            gesture.raise(y_px);
            if mouse.just_pressed(MouseButton::Left) && !over_ui {
                commit = gesture.press(&ctx, [0.0, 0.0], y_px);
            }
        } else {
            let p = ground.map(|g| [g.x, g.z]);
            if mouse.just_pressed(MouseButton::Left)
                && !keys.pressed(KeyCode::AltLeft)
                && let Some(p) = p
            {
                commit = gesture.press(&ctx, p, y_px);
                *screen0 = cursor_px.unwrap_or_default();
            }
            if gesture.phase() == ToolPhase::Dragging && commit.is_none() {
                if mouse.pressed(MouseButton::Left) {
                    let rect_tool = matches!(ctx.tool, Tool::Field | Tool::Block);
                    let corners = (|| {
                        let (low, (camera, gt), now) = (low.as_deref()?, cam.single().ok()?, cursor_px?);
                        let at = |c: Vec2| cursor_ray(c, low, camera, gt).and_then(|r| pick_ground(&pick.grid, r));
                        let s = *screen0;
                        let pts = [at(s)?, at(now)?, at(Vec2::new(s.x, now.y))?, at(Vec2::new(now.x, s.y))?];
                        let lo = pts.iter().fold([f32::MAX; 2], |m, p| [m[0].min(p.x), m[1].min(p.z)]);
                        let hi = pts.iter().fold([f32::MIN; 2], |m, p| [m[0].max(p.x), m[1].max(p.z)]);
                        Some((clamp_pt(lo), clamp_pt(hi)))
                    })();
                    match (rect_tool, corners, p) {
                        (true, Some((lo, hi)), _) => gesture.set_rect(lo, hi),
                        (_, _, Some(p)) => gesture.drag(&ctx, p),
                        _ => {}
                    }
                }
                if !mouse.pressed(MouseButton::Left) {
                    commit = gesture.release(&ctx, y_px);
                }
            }
        }
        let g = (gesture.rev() != *last_rev).then(|| gesture.ghost(&ctx));
        *last_rev = gesture.rev();
        (commit, g)
    };

    if let Some(c) = commit.take() {
        apply_commit(&mut world, c);
        gesture.cancel();
        *last_rev = gesture.rev();
        if *ghost != Ghost::default() {
            *ghost = Ghost::default();
        }
    } else if let Some(g) = new_ghost
        && *ghost != g
    {
        *ghost = g;
    }
    if state.phase != gesture.phase() {
        state.phase = gesture.phase();
    }
}

fn update_marker(
    ground: Res<GroundCursor>,
    state: Res<ToolState>,
    pick: Res<CommittedGrid>,
    mats: Option<Res<MarkerMats>>,
    mut q: Query<(&mut Transform, &mut Visibility, &mut MeshMaterial3d<ToonMaterial>), With<CursorMarker>>,
) {
    let Ok((mut tf, mut vis, mut mat)) = q.single_mut() else { return };
    let (Some(g), Some(mats)) = (ground.0, mats) else {
        *vis = Visibility::Hidden;
        return;
    };
    let cell = if state.tool == Tool::Spire { 1.5 } else { 0.5 };
    let snap = |v: f32| (v / 0.5).floor() * 0.5 + 0.25;
    let y = ground_height(&pick.grid, g.x, g.z);
    tf.translation = Vec3::new(snap(g.x), y + 0.15, snap(g.z));
    tf.scale = Vec3::new(cell / 0.5, 0.6, cell / 0.5);
    *vis = if state.phase == ToolPhase::Raising { Visibility::Hidden } else { Visibility::Inherited };
    let want = if state.tool == Tool::Raze { &mats.raze } else { &mats.normal };
    if mat.0 != *want {
        mat.0 = want.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acre_core::generate;

    fn ctx<'a>(tool: Tool, doc: &'a Document) -> Ctx<'a> {
        Ctx { tool, tall: false, spire_kind: 1, doc }
    }

    #[test]
    fn simplify_collapses_straight_line_and_keeps_corner() {
        let line: Vec<P2> = (0..20).map(|i| [i as f32, 0.01 * (i % 2) as f32]).collect();
        assert_eq!(simplify(&line, 0.35).len(), 2);
        let mut l: Vec<P2> = (0..10).map(|i| [i as f32, 0.0]).collect();
        l.extend((1..10).map(|i| [9.0, i as f32]));
        let s = simplify(&l, 0.35);
        assert_eq!(s, vec![[0.0, 0.0], [9.0, 0.0], [9.0, 9.0]]);
    }

    #[test]
    fn rect_and_floors() {
        assert_eq!(rect_from_drag([5.0, 1.0], [1.0, 4.0], 2.0), Some(([1.0, 1.0], [5.0, 4.0])));
        assert_eq!(rect_from_drag([0.0, 0.0], [5.0, 0.5], 2.0), None);
        assert_eq!(floors_from_drag(0.0), 1);
        assert_eq!(floors_from_drag(10.0), 1);
        assert_eq!(floors_from_drag(-PX_PER_FLOOR * 3.0), 4);
        assert_eq!(floors_from_drag(-10_000.0), 12);
        assert_eq!(floors_from_drag(f32::NAN), 1);
    }

    #[test]
    fn pick_hits_ground_plane() {
        let doc = Document::new(3);
        let grid = generate(&doc);
        let target = Vec3::new(2.0, 0.0, -3.0);
        let h = ground_height(&grid, target.x, target.z);
        let dir = Dir3::new(Vec3::new(-0.5, -0.6, -0.5)).unwrap();
        let origin = Vec3::new(target.x, h, target.z) - *dir * 60.0;
        let hit = pick_ground(&grid, Ray3d::new(origin, dir)).expect("hit");
        assert!((hit.x - target.x).abs() < 0.8 && (hit.z - target.z).abs() < 0.8, "{hit:?}");
        assert!((hit.y - h).abs() < 1.0);
        let up = Ray3d::new(Vec3::new(0.0, 40.0, 0.0), Dir3::Y);
        assert!(pick_ground(&grid, up).is_none());
    }

    #[test]
    fn wall_gesture_commits_simplified_stroke() {
        let doc = Document::new(1);
        let c = ctx(Tool::Wall, &doc);
        let mut g = Gesture::default();
        assert!(g.press(&c, [0.0, 0.0], 0.0).is_none());
        for i in 1..=12 {
            g.drag(&c, [i as f32, 0.0]);
        }
        assert!(matches!(g.ghost(&c).add, Some(Structure::Wall { .. })));
        match g.release(&c, 0.0) {
            Some(Commit::Add(Structure::Wall { path, tall })) => {
                assert_eq!(path.len(), 2);
                assert!(!tall);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(g.phase(), ToolPhase::Idle);
    }

    #[test]
    fn degenerate_click_is_ignored() {
        let doc = Document::new(1);
        for tool in [Tool::Wall, Tool::Path, Tool::Field, Tool::Block, Tool::Raze] {
            let c = ctx(tool, &doc);
            let mut g = Gesture::default();
            g.press(&c, [1.0, 1.0], 0.0);
            assert_eq!(g.release(&c, 0.0), None, "{tool:?}");
            assert_eq!(g.phase(), ToolPhase::Idle);
        }
    }

    #[test]
    fn block_flow_drag_raise_confirm() {
        let doc = Document::new(1);
        let c = ctx(Tool::Block, &doc);
        let mut g = Gesture::default();
        g.press(&c, [0.0, 0.0], 100.0);
        g.drag(&c, [6.0, 5.0]);
        assert_eq!(g.release(&c, 100.0), None);
        assert_eq!(g.phase(), ToolPhase::Raising);
        g.raise(100.0 - PX_PER_FLOOR * 4.0);
        assert_eq!(g.floors(), 5);
        assert_eq!(
            g.press(&c, [0.0, 0.0], 0.0),
            Some(Commit::Add(Structure::Block { min: [0.0, 0.0], max: [6.0, 5.0], floors: 5 }))
        );
        assert_eq!(g.phase(), ToolPhase::Idle);
    }

    #[test]
    fn raze_collects_touched_structures() {
        let doc = Document {
            seed: 1,
            structures: vec![
                Structure::Spire { at: [0.0, 0.0], kind: 0 },
                Structure::Spire { at: [20.0, 0.0], kind: 0 },
                Structure::Field { min: [4.0, -2.0], max: [8.0, 2.0] },
            ],
        };
        let c = ctx(Tool::Raze, &doc);
        let mut g = Gesture::default();
        g.press(&c, [0.0, 0.0], 0.0);
        g.drag(&c, [10.0, 0.0]);
        assert_eq!(g.ghost(&c).remove, vec![0, 2]);
        assert_eq!(g.release(&c, 0.0), Some(Commit::Remove(vec![0, 2])));

        let mut world = World { history: acre_core::History::new(doc) };
        apply_commit(&mut world, Commit::Remove(vec![0, 2]));
        assert_eq!(world.history.doc().structures.len(), 1);
    }

    #[test]
    fn raze_click_commits_same_frame() {
        let doc = Document { seed: 1, structures: vec![Structure::Spire { at: [0.0, 0.0], kind: 0 }] };
        let c = ctx(Tool::Raze, &doc);
        let mut g = Gesture::default();
        assert!(g.press(&c, [0.5, 0.0], 0.0).is_none());
        assert_eq!(g.release(&c, 0.0), Some(Commit::Remove(vec![0])));
    }

    #[test]
    fn stroke_points_are_capped() {
        let doc = Document::new(1);
        let c = ctx(Tool::Wall, &doc);
        let mut g = Gesture::default();
        g.press(&c, [-40.0, 0.0], 0.0);
        for i in 0..2000 {
            let t = i as f32 * 0.8;
            g.drag(&c, [-40.0 + (t * 0.05).cos() * 3.0 + t * 0.02, (t * 0.7).sin() * 30.0]);
        }
        assert!(g.pts.len() <= MAX_POINTS);
    }

    #[test]
    fn spire_places_on_click() {
        let doc = Document::new(1);
        let c = ctx(Tool::Spire, &doc);
        let mut g = Gesture::default();
        assert_eq!(g.press(&c, [3.0, 4.0], 0.0), Some(Commit::Add(Structure::Spire { at: [3.0, 4.0], kind: 1 })));
    }
}
