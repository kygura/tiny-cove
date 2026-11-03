//! Building tools: a pure drag state machine plus the systems feeding it pointer and
//! keyboard input. Commits go to `Doc`; previews go to `Ghost`.

use crate::camera::pointer_over_ui;
use crate::game::{Doc, EditCommand, Ghost, Grid, Tool, ToolState};
use crate::generate::hits;
use crate::render::{GameCamera, LowRes, VoxelMaterialHandle, cube_mesh, cursor_ray};
use crate::scene::{SceneSync, time_seed};
use crate::structure::{Structure, StructureId, World, polyline_len};
use crate::voxel::{GROUND_Y, SIZE_X, SIZE_Y, SIZE_Z, VOXEL_SIZE, Voxel, VoxelGrid};
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

pub struct ToolsPlugin;

pub const WALL_HEIGHT: u32 = 16;
pub const DAM_HEIGHT: u32 = 32;
/// Footprint preview height and the raise-phase range, in voxels.
pub const BLOCK_START_H: u32 = 8;
pub const BLOCK_MIN_H: u32 = 6;
pub const BLOCK_MAX_H: u32 = 48;
/// Screen pixels of vertical mouse travel per voxel of block height.
const PX_PER_VOXEL: f32 = 6.0;
/// Minimum distance between sampled path points, world units.
const SAMPLE: f32 = 0.5;
const MIN_WALL_LEN: f32 = 2.0;
const MIN_ROAD_LEN: f32 = 1.5;
/// 4 voxels: the generator ignores smaller footprints.
const MIN_BLOCK_SIDE: f32 = 2.0;
const RAZE_RADIUS: f32 = 1.0;
/// Cursor marker edge length, world units.
const MARKER_SIZE: f32 = 1.0;
const HALF: Vec2 = Vec2::new(SIZE_X as f32 * VOXEL_SIZE / 2.0, SIZE_Z as f32 * VOXEL_SIZE / 2.0);

/// In-progress gesture. Path variants keep the live cursor point as their last point.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Drag {
    #[default]
    Idle,
    Wall { points: Vec<Vec2>, dam: bool },
    Road { points: Vec<Vec2> },
    Rect { a: Vec2, b: Vec2 },
    /// Footprint placed; vertical mouse motion from `y0` sets the height.
    Raise { a: Vec2, b: Vec2, y0: f32, height: u32 },
    Raze { ids: Vec<StructureId> },
}

/// One pointer sample: ground point (for Raze, the xz of the voxel under the
/// cursor), cursor y in window pixels, and Shift.
#[derive(Clone, Copy, Debug)]
pub struct Pointer {
    pub ground: Vec2,
    pub y: f32,
    pub shift: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Commit(Structure),
    Remove(Vec<StructureId>),
}

fn track(points: &mut Vec<Vec2>, p: Vec2) {
    let n = points.len();
    points[n - 1] = p;
    if points[n - 1].distance(points[n - 2]) >= SAMPLE {
        points.push(p);
    }
}

fn raze_hits(ids: &mut Vec<StructureId>, doc: &World, p: Vec2) {
    for placed in &doc.placed {
        if !ids.contains(&placed.id) && hits(&placed.structure, p, RAZE_RADIUS) {
            ids.push(placed.id);
        }
    }
}

fn block(a: Vec2, b: Vec2, height: u32) -> Structure {
    Structure::Block { min: a.min(b), max: a.max(b), height }
}

/// Left button pressed. Only the raise phase commits on press.
pub fn on_press(drag: &mut Drag, tool: Tool, ptr: Pointer, doc: &World) -> Option<Action> {
    if let Drag::Raise { a, b, height, .. } = *drag {
        *drag = Drag::Idle;
        return Some(Action::Commit(block(a, b, height)));
    }
    let p = ptr.ground;
    *drag = match tool {
        Tool::Wall => Drag::Wall { points: vec![p, p], dam: ptr.shift },
        Tool::Road => Drag::Road { points: vec![p, p] },
        Tool::Block => Drag::Rect { a: p, b: p },
        Tool::Raze => {
            let mut ids = Vec::new();
            raze_hits(&mut ids, doc, p);
            Drag::Raze { ids }
        }
    };
    None
}

pub fn on_move(drag: &mut Drag, ptr: Pointer, doc: &World) {
    match drag {
        Drag::Idle => {}
        Drag::Wall { points, dam } => {
            *dam = ptr.shift;
            track(points, ptr.ground);
        }
        Drag::Road { points } => track(points, ptr.ground),
        Drag::Rect { b, .. } => *b = ptr.ground,
        Drag::Raise { y0, height, .. } => {
            let h = BLOCK_START_H as f32 + (*y0 - ptr.y) / PX_PER_VOXEL;
            *height = (h.round() as u32).clamp(BLOCK_MIN_H, BLOCK_MAX_H);
        }
        Drag::Raze { ids } => raze_hits(ids, doc, ptr.ground),
    }
}

/// Left button released. Degenerate gestures are dropped silently.
pub fn on_release(drag: &mut Drag, ptr: Pointer) -> Option<Action> {
    match std::mem::take(drag) {
        Drag::Wall { points, dam } if polyline_len(&points) >= MIN_WALL_LEN => {
            let height = if dam { DAM_HEIGHT } else { WALL_HEIGHT };
            Some(Action::Commit(Structure::Wall { points, height, dam }))
        }
        Drag::Road { points } if polyline_len(&points) >= MIN_ROAD_LEN => {
            Some(Action::Commit(Structure::Road { points }))
        }
        Drag::Rect { a, b } if ((a - b).abs().cmpge(Vec2::splat(MIN_BLOCK_SIDE))).all() => {
            *drag = Drag::Raise { a, b, y0: ptr.y, height: BLOCK_START_H };
            None
        }
        r @ Drag::Raise { .. } => {
            *drag = r;
            None
        }
        Drag::Raze { ids } if !ids.is_empty() => Some(Action::Remove(ids)),
        _ => None,
    }
}

/// Preview for the current gesture.
pub fn ghost_of(drag: &Drag) -> Ghost {
    let pending = match drag {
        Drag::Wall { points, dam } if polyline_len(points) >= SAMPLE => Some(Structure::Wall {
            points: points.clone(),
            height: if *dam { DAM_HEIGHT } else { WALL_HEIGHT },
            dam: *dam,
        }),
        Drag::Road { points } if polyline_len(points) >= SAMPLE => Some(Structure::Road { points: points.clone() }),
        Drag::Rect { a, b } if a != b => Some(block(*a, *b, BLOCK_START_H)),
        Drag::Raise { a, b, height, .. } => Some(block(*a, *b, *height)),
        _ => None,
    };
    let hidden = match drag {
        Drag::Raze { ids } => ids.clone(),
        _ => Vec::new(),
    };
    Ghost { pending, hidden }
}

pub fn hint(tool: Tool, drag: &Drag) -> &'static str {
    match (tool, drag) {
        (Tool::Wall, _) => "drag to pour a wall  ·  shift: dam",
        (Tool::Block, Drag::Raise { .. }) => "move mouse to raise  ·  click to cast  ·  esc to cancel",
        (Tool::Block, _) => "drag a footprint",
        (Tool::Road, _) => "drag to lay a road",
        (Tool::Raze, _) => "drag over structures to demolish",
    }
}

/// First solid voxel along `ray` within `max_dist` world units (Amanatides-Woo DDA).
/// Returns the voxel and the world-space point where the ray enters it.
pub fn raycast_voxels(grid: &VoxelGrid, ray: &Ray3d, max_dist: f32) -> Option<(IVec3, Vec3)> {
    let dir = *ray.direction;
    let off = Vec3::new((SIZE_X / 2) as f32, GROUND_Y as f32, (SIZE_Z / 2) as f32);
    let o = ray.origin / VOXEL_SIZE + off;
    let mut cell = o.floor().as_ivec3();
    let mut step = IVec3::ZERO;
    let mut t_max = Vec3::INFINITY;
    let mut t_delta = Vec3::INFINITY;
    for i in 0..3 {
        if dir[i] > 0.0 {
            step[i] = 1;
            t_delta[i] = 1.0 / dir[i];
            t_max[i] = (cell[i] as f32 + 1.0 - o[i]) / dir[i];
        } else if dir[i] < 0.0 {
            step[i] = -1;
            t_delta[i] = -1.0 / dir[i];
            t_max[i] = (o[i] - cell[i] as f32) / -dir[i];
        }
    }
    let limit = max_dist / VOXEL_SIZE;
    let mut t = 0.0;
    while t <= limit {
        if grid.get(cell).is_solid() {
            return Some((cell, ray.origin + dir * t * VOXEL_SIZE));
        }
        let a = t_max.min_position();
        // Past the grid and moving away: nothing more to hit.
        if (step[a] > 0 && cell[a] >= [SIZE_X, SIZE_Y, SIZE_Z][a]) || (step[a] < 0 && cell[a] < 0) {
            return None;
        }
        t = t_max[a];
        cell[a] += step[a];
        t_max[a] += t_delta[a];
    }
    None
}

/// Tool runtime state (the drag plus where the cursor last touched the ground).
#[derive(Resource, Default)]
pub struct Tools {
    pub drag: Drag,
    last: Option<Pointer>,
}

#[derive(Component)]
pub struct CursorMarker;

impl Plugin for ToolsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<EditCommand>()
            .init_resource::<ToolState>()
            .init_resource::<Tools>()
            .add_systems(Startup, spawn_marker)
            .add_systems(
                Update,
                (key_commands, handle_commands, pointer).chain().before(SceneSync),
            );
    }
}

fn key_commands(keys: Res<ButtonInput<KeyCode>>, mut out: MessageWriter<EditCommand>) {
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let digits = [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4];
    for (k, t) in digits.into_iter().zip(Tool::ALL) {
        if keys.just_pressed(k) {
            out.write(EditCommand::SelectTool(t));
        }
    }
    if ctrl && keys.just_pressed(KeyCode::KeyZ) {
        out.write(if shift { EditCommand::Redo } else { EditCommand::Undo });
    }
    if ctrl && keys.just_pressed(KeyCode::KeyY) {
        out.write(EditCommand::Redo);
    }
    if !ctrl && keys.just_pressed(KeyCode::KeyN) {
        out.write(EditCommand::NewWorld);
    }
}

fn handle_commands(
    mut cmds: MessageReader<EditCommand>,
    mut state: ResMut<ToolState>,
    mut tools: ResMut<Tools>,
    mut doc: ResMut<Doc>,
) {
    for cmd in cmds.read() {
        tools.drag = Drag::Idle;
        let changed = match cmd {
            EditCommand::SelectTool(t) => {
                state.tool = *t;
                false
            }
            EditCommand::Undo => doc.bypass_change_detection().0.undo(),
            EditCommand::Redo => doc.bypass_change_detection().0.redo(),
            EditCommand::NewWorld => {
                doc.bypass_change_detection().0 = World::new(time_seed());
                true
            }
        };
        if changed {
            doc.set_changed();
        }
    }
}

fn spawn_marker(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, material: Res<VoxelMaterialHandle>) {
    commands.spawn((
        CursorMarker,
        Mesh3d(meshes.add(cube_mesh(Voxel::Rust))),
        MeshMaterial3d(material.0.clone()),
        Transform::from_scale(Vec3::splat(MARKER_SIZE)),
        Visibility::Hidden,
    ));
}

#[allow(clippy::too_many_arguments)]
fn pointer(
    window: Query<&Window, With<PrimaryWindow>>,
    cam: Query<(&Camera, &GlobalTransform), With<GameCamera>>,
    low: Res<LowRes>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    hover: Option<Res<HoverMap>>,
    nodes: Query<(), With<Node>>,
    grid: Res<Grid>,
    mut doc: ResMut<Doc>,
    mut ghost: ResMut<Ghost>,
    mut tools: ResMut<Tools>,
    mut state: ResMut<ToolState>,
    mut marker: Query<(&mut Transform, &mut Visibility), With<CursorMarker>>,
) {
    let tool = state.tool;
    let ray = window
        .single()
        .ok()
        .zip(cam.single().ok())
        .and_then(|(w, (c, gt))| Some((w.cursor_position()?, cursor_ray(w, c, gt, &low)?)));
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let sample = ray.and_then(|(cursor, ray)| {
        let at = if tool == Tool::Raze {
            raycast_voxels(&grid.0, &ray, 300.0).map(|(_, p)| p)
        } else {
            None
        };
        let at = at.or_else(|| {
            let d = ray.intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Y))?;
            Some(ray.get_point(d))
        })?;
        let ground = Vec2::new(at.x, at.z).clamp(-HALF, HALF);
        Some((Pointer { ground, y: cursor.y, shift }, at.y.max(0.0)))
    });
    let over_ui = pointer_over_ui(hover.as_deref(), &nodes);

    let t = &mut *tools;
    if let Some((ptr, _)) = sample {
        t.last = Some(ptr);
    }
    // Raise only needs the cursor's y, so keep going with the last ground point.
    let ptr = sample.map(|s| s.0).or(t.last.map(|p| Pointer { shift, ..p }));
    let mut action = None;
    if keys.just_pressed(KeyCode::Escape) {
        t.drag = Drag::Idle;
    } else if let Some(ptr) = ptr {
        if mouse.just_pressed(MouseButton::Left) && !over_ui {
            action = on_press(&mut t.drag, tool, ptr, &doc.0);
        } else if mouse.pressed(MouseButton::Left) || matches!(t.drag, Drag::Raise { .. }) {
            on_move(&mut t.drag, ptr, &doc.0);
        }
        if mouse.just_released(MouseButton::Left) {
            action = action.or(on_release(&mut t.drag, ptr));
        }
    }
    match action {
        Some(Action::Commit(s)) => {
            doc.0.add(s);
        }
        Some(Action::Remove(ids)) => {
            doc.0.remove(&ids);
        }
        None => {}
    }
    ghost.set_if_neq(ghost_of(&t.drag));
    let h = hint(tool, &t.drag);
    if state.hint != h {
        state.hint = h.to_string();
    }
    if let Ok((mut tf, mut vis)) = marker.single_mut() {
        let show = sample.is_some() && !over_ui;
        vis.set_if_neq(if show { Visibility::Visible } else { Visibility::Hidden });
        if let Some((ptr, y)) = sample {
            tf.translation = Vec3::new(ptr.ground.x, y + MARKER_SIZE / 2.0, ptr.ground.y);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voxel::world_to_voxel;

    fn at(x: f32, z: f32) -> Pointer {
        Pointer { ground: Vec2::new(x, z), y: 300.0, shift: false }
    }

    fn drag_path(tool: Tool, shift: bool, doc: &World) -> (Option<Action>, Ghost) {
        let mut d = Drag::Idle;
        let p = |x| Pointer { shift, ..at(x, 2.0) };
        assert_eq!(on_press(&mut d, tool, p(0.0), doc), None);
        let mut x = 0.0;
        while x < 10.0 {
            x += 0.2;
            on_move(&mut d, p(x), doc);
        }
        let g = ghost_of(&d);
        (on_release(&mut d, p(x)), g)
    }

    #[test]
    fn wall_drag_commits_wall_and_previews() {
        let doc = World::new(1);
        let (a, g) = drag_path(Tool::Wall, false, &doc);
        let Some(Action::Commit(Structure::Wall { points, height, dam })) = a else { panic!("{a:?}") };
        assert_eq!((height, dam), (WALL_HEIGHT, false));
        assert_eq!(points[0], Vec2::new(0.0, 2.0));
        assert!(points.last().unwrap().distance(Vec2::new(10.0, 2.0)) < 0.3);
        assert!(points.windows(2).all(|w| w[0].distance(w[1]) < SAMPLE + 0.25));
        assert!(matches!(g.pending, Some(Structure::Wall { .. })));
    }

    #[test]
    fn shift_wall_is_dam() {
        let (a, _) = drag_path(Tool::Wall, true, &World::new(1));
        assert!(matches!(a, Some(Action::Commit(Structure::Wall { height: DAM_HEIGHT, dam: true, .. }))));
    }

    #[test]
    fn road_drag_commits_road() {
        let (a, g) = drag_path(Tool::Road, false, &World::new(1));
        assert!(matches!(a, Some(Action::Commit(Structure::Road { .. }))));
        assert!(matches!(g.pending, Some(Structure::Road { .. })));
    }

    #[test]
    fn tiny_gestures_are_ignored() {
        let doc = World::new(1);
        for tool in [Tool::Wall, Tool::Road, Tool::Block, Tool::Raze] {
            let mut d = Drag::Idle;
            on_press(&mut d, tool, at(0.0, 0.0), &doc);
            on_move(&mut d, at(0.4, 0.0), &doc);
            assert_eq!(on_release(&mut d, at(0.4, 0.0)), None, "{tool:?}");
            assert_eq!(d, Drag::Idle);
            assert_eq!(ghost_of(&d), Ghost::default());
        }
    }

    #[test]
    fn block_footprint_then_raise_then_click() {
        let doc = World::new(1);
        let mut d = Drag::Idle;
        on_press(&mut d, Tool::Block, at(4.0, 4.0), &doc);
        on_move(&mut d, at(-2.0, 9.0), &doc);
        assert_eq!(hint(Tool::Block, &d), "drag a footprint");
        assert_eq!(on_release(&mut d, at(-2.0, 9.0)), None);
        assert!(matches!(d, Drag::Raise { height: BLOCK_START_H, .. }));
        assert!(hint(Tool::Block, &d).starts_with("move mouse to raise"));
        // 60 px up = 10 voxels taller.
        on_move(&mut d, Pointer { y: 240.0, ..at(9.0, 9.0) }, &doc);
        assert_eq!(on_release(&mut d, at(0.0, 0.0)), None, "stray release keeps raising");
        let a = on_press(&mut d, Tool::Block, at(9.0, 9.0), &doc);
        assert_eq!(
            a,
            Some(Action::Commit(Structure::Block {
                min: Vec2::new(-2.0, 4.0),
                max: Vec2::new(4.0, 9.0),
                height: BLOCK_START_H + 10
            }))
        );
        assert_eq!(d, Drag::Idle);
    }

    #[test]
    fn footprint_under_four_voxels_is_ignored() {
        let mut d = Drag::Idle;
        on_press(&mut d, Tool::Block, at(0.0, 0.0), &World::new(1));
        on_move(&mut d, at(1.9, 6.0), &World::new(1));
        assert_eq!(on_release(&mut d, at(1.9, 6.0)), None);
        assert_eq!(d, Drag::Idle);
    }

    #[test]
    fn raise_height_is_clamped() {
        let mut d = Drag::Raise { a: Vec2::ZERO, b: Vec2::ONE * 4.0, y0: 300.0, height: 8 };
        on_move(&mut d, Pointer { y: -5000.0, ..at(0.0, 0.0) }, &World::new(1));
        assert!(matches!(d, Drag::Raise { height: BLOCK_MAX_H, .. }));
        on_move(&mut d, Pointer { y: 5000.0, ..at(0.0, 0.0) }, &World::new(1));
        assert!(matches!(d, Drag::Raise { height: BLOCK_MIN_H, .. }));
    }

    #[test]
    fn raze_collects_hits_then_undo_restores() {
        let mut doc = World::new(1);
        let wall = doc.add(Structure::Wall { points: vec![Vec2::new(-5.0, 0.0), Vec2::new(5.0, 0.0)], height: 16, dam: false });
        let blk = doc.add(block(Vec2::new(10.0, 10.0), Vec2::new(14.0, 14.0), 10));
        let road = doc.add(Structure::Road { points: vec![Vec2::new(-20.0, -20.0), Vec2::new(-10.0, -20.0)] });
        let mut d = Drag::Idle;
        on_press(&mut d, Tool::Raze, at(0.0, 0.2), &doc);
        on_move(&mut d, at(12.0, 12.0), &doc);
        on_move(&mut d, at(30.0, 30.0), &doc);
        assert_eq!(ghost_of(&d).hidden, vec![wall, blk]);
        let a = on_release(&mut d, at(30.0, 30.0));
        assert_eq!(a, Some(Action::Remove(vec![wall, blk])));
        assert!(doc.remove(&[wall, blk]));
        assert_eq!(doc.placed.iter().map(|p| p.id).collect::<Vec<_>>(), vec![road]);
        assert!(doc.undo());
        assert_eq!(doc.placed.len(), 3);
        assert!(doc.redo());
        assert_eq!(doc.placed.len(), 1);
    }

    #[test]
    fn raycast_hits_top_of_voxel() {
        let mut g = VoxelGrid::new();
        let v = IVec3::new(80, 10, 80);
        g.set(v, Voxel::Concrete);
        let top = crate::voxel::voxel_to_world(v + IVec3::Y) + Vec3::new(0.25, 0.0, 0.25);
        let ray = Ray3d::new(top + Vec3::Y * 20.0, Dir3::NEG_Y);
        let (cell, p) = raycast_voxels(&g, &ray, 100.0).unwrap();
        assert_eq!(cell, v);
        assert!((p - top).length() < 1e-3, "{p} vs {top}");
        assert_eq!(raycast_voxels(&g, &ray, 5.0), None);
        let slanted = Ray3d::new(top + Vec3::new(-30.0, 30.0, 0.0), Dir3::new(Vec3::new(1.0, -1.0, 0.0)).unwrap());
        assert_eq!(raycast_voxels(&g, &slanted, 100.0).map(|h| h.0), Some(v));
        let miss = Ray3d::new(top + Vec3::new(3.0, 5.0, 0.0), Dir3::NEG_Y);
        assert_eq!(raycast_voxels(&g, &miss, 100.0), None);
        assert_eq!(world_to_voxel(p - Vec3::Y * 0.01), v);
    }

    #[test]
    fn raycast_from_outside_grid_hits_terrain() {
        let g = crate::generate::terrain(5);
        let ray = Ray3d::new(Vec3::new(60.0, 40.0, 60.0), Dir3::new(-Vec3::new(60.0, 40.0, 60.0)).unwrap());
        let (_, p) = raycast_voxels(&g, &ray, 300.0).unwrap();
        assert!(p.y.abs() < 6.0 && p.x.abs() < 40.5, "{p}");
    }
}
