//! HUD (DESIGN.md "HUD"): bottom-centre toolbar + hint line, top-left title and
//! tallies, top-right weather picker, clock, pause button and key hint.
//!
//! Reads `ToolState`, `Sky`, `World`, `DisplayedGrid` (and `Ghost` for the floor
//! count in the Block hint); writes only [`UiCommand`] messages.
//!
//! The default font is ASCII-only, so `·` and `³` are drawn: the middle dot as a
//! small square node, the superscript 3 as a smaller raised digit.

use crate::game::{DisplayedGrid, Ghost, Sky, Tool, ToolPhase, ToolState, UiCommand, WeatherKind, World};
use acre_core::{SIZE_X, SIZE_Y, SIZE_Z, Structure, Voxel, VoxelGrid};
use bevy::picking::{Pickable, hover::Hovered};
use bevy::prelude::*;
use bevy::ui::Pressed;
use bevy::ui_widgets::{Activate, ActivateOnPress, Button};

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_hud)
            .add_observer(activate_slot)
            .add_systems(Update, (style_slots, update_hint, update_clock, update_tally));
    }
}

// ---------------------------------------------------------------- palette

const PANEL: u32 = 0x2f2b33;
const PANEL_ALPHA: f32 = 0.82;
const BORDER: u32 = 0xd6cbbd;
const INK: u32 = 0xf3ece1;
const ACCENT: u32 = 0xffc35c;
const ACCENT_INK: u32 = 0x2f2b33;
const HOVER: u32 = 0x4a4450;
const DIM: f32 = 0.45;
const DISABLED: f32 = 0.30;

fn rgba(hex: u32, a: f32) -> Color {
    let c = |shift: u32| ((hex >> shift) & 0xff) as f32 / 255.0;
    Color::srgba(c(16), c(8), c(0), a)
}

fn pressed_accent() -> Color {
    let c = |shift: u32| ((ACCENT >> shift) & 0xff) as f32 / 255.0 * 0.85;
    Color::srgb(c(16), c(8), c(0))
}

// ---------------------------------------------------------------- components

#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
enum Slot {
    Tool(Tool),
    Undo,
    Redo,
    Weather(WeatherKind),
    Pause,
}

/// Base opacity of a text line inside a slot (before state colouring).
#[derive(Component)]
struct TextAlpha(f32);

#[derive(Component)]
struct HintRow;

#[derive(Component)]
struct ClockText;

#[derive(Component)]
struct PhaseText;

#[derive(Component)]
struct PouredText;

#[derive(Component)]
struct HarvestText;

// ---------------------------------------------------------------- pure helpers

/// Hint line for the active tool and phase (DESIGN.md hint table, verbatim).
pub fn hint_text(tool: Tool, phase: ToolPhase, spire_kind: u8, floors: Option<u8>) -> String {
    match tool {
        Tool::Wall => "drag to pour a wall  ·  shift: rampart".into(),
        Tool::Block if phase == ToolPhase::Raising => format!(
            "move mouse up/down to set floors ({})  ·  click to cast  ·  esc cancels",
            floors.unwrap_or(1)
        ),
        Tool::Block => "drag a footprint".into(),
        Tool::Path => "drag to lay a gravel path  ·  cuts gates through walls".into(),
        Tool::Field => "drag to sow a field".into(),
        Tool::Spire => format!("click to raise a {}  ·  tab: next kind", spire_name(spire_kind)),
        Tool::Raze => "drag over structures to clear them".into(),
    }
}

pub fn spire_name(kind: u8) -> &'static str {
    match kind % 3 {
        0 => "CRYSTAL",
        1 => "MONOLITH",
        _ => "BLOSSOM TREE",
    }
}

/// `1234567` -> `1,234,567`.
pub fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// `POURED n m³`: concrete voxels × 0.125, rounded.
pub fn poured_label(concrete_voxels: u64) -> String {
    let m3 = (concrete_voxels as f64 * 0.125).round() as u64;
    format!("POURED {} m³", thousands(m3))
}

pub fn harvest_label(crops: u64) -> String {
    format!("HARVEST {}", thousands(crops))
}

/// `13.666` -> `13:39` (minutes floored, wraps at 24 h).
pub fn clock_text(hour: f32) -> String {
    let minutes = ((hour.rem_euclid(24.0) * 60.0).floor() as u32) % (24 * 60);
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

/// Phase word: dawn 05–08, day 08–17, dusk 17–20, night 20–05.
pub fn phase_word(hour: f32) -> &'static str {
    let h = hour.rem_euclid(24.0);
    if (5.0..8.0).contains(&h) {
        "DAWN"
    } else if (8.0..17.0).contains(&h) {
        "DAY"
    } else if (17.0..20.0).contains(&h) {
        "DUSK"
    } else {
        "NIGHT"
    }
}

/// (Concrete + ConcreteDark, Crop) voxel counts.
pub fn count_tally(grid: &VoxelGrid) -> (u64, u64) {
    let (mut concrete, mut crop) = (0u64, 0u64);
    for y in 0..SIZE_Y as i32 {
        for z in 0..SIZE_Z as i32 {
            for x in 0..SIZE_X as i32 {
                match grid.get(x, y, z) {
                    Voxel::Concrete | Voxel::ConcreteDark => concrete += 1,
                    Voxel::Crop => crop += 1,
                    _ => {}
                }
            }
        }
    }
    (concrete, crop)
}

// ---------------------------------------------------------------- spawning

fn text(s: &str, size: f32, alpha: f32) -> impl Bundle {
    (
        Text::new(s),
        TextFont { font_size: FontSize::Px(size), ..default() },
        TextColor(rgba(INK, alpha)),
        TextAlpha(alpha),
        Pickable::IGNORE,
    )
}

fn panel(padding: f32, gap: f32) -> impl Bundle {
    (
        Node {
            padding: UiRect::all(px(padding)),
            column_gap: px(gap),
            border: UiRect::all(px(2)),
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(rgba(PANEL, PANEL_ALPHA)),
        BorderColor::all(rgba(BORDER, 1.0)),
    )
}

/// Spawns `s` as a row of text runs; `·` becomes a drawn square dot with two
/// monospace spaces of room either side, `³` a small raised `3`.
fn spawn_rich(parent: &mut ChildSpawnerCommands, s: &str, size: f32, alpha: f32) {
    let color = rgba(INK, alpha);
    let dot = (size * 0.2).round().max(2.0);
    // FiraMono advance is 0.6 em; "  ·  " spans five cells.
    let side = ((5.0 * 0.6 * size - dot) / 2.0).round();
    let mut run = String::new();
    let flush = |p: &mut ChildSpawnerCommands, run: &mut String| {
        let t = run.trim();
        if !t.is_empty() {
            p.spawn(text(t, size, alpha));
        }
        run.clear();
    };
    for ch in s.chars() {
        match ch {
            '·' => {
                flush(parent, &mut run);
                parent.spawn((
                    Node {
                        width: px(dot),
                        height: px(dot),
                        margin: UiRect::horizontal(px(side)),
                        align_self: AlignSelf::Center,
                        ..default()
                    },
                    BackgroundColor(color),
                    Pickable::IGNORE,
                ));
            }
            '³' => {
                flush(parent, &mut run);
                parent.spawn((
                    text("3", (size * 0.65).round(), alpha),
                    Node { align_self: AlignSelf::FlexStart, ..default() },
                ));
            }
            c => run.push(c),
        }
    }
    flush(parent, &mut run);
}

fn rich_row() -> impl Bundle {
    (Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)
}

fn slot(
    parent: &mut ChildSpawnerCommands,
    slot: Slot,
    (w, h): (f32, f32),
    label: (&str, f32),
    hotkey: Option<(&str, f32)>,
) {
    parent
        .spawn((
            Button,
            ActivateOnPress,
            Hovered::default(),
            slot,
            Node {
                width: px(w),
                height: px(h),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(px(2)),
                ..default()
            },
            BackgroundColor(Color::NONE),
            BorderColor::all(Color::NONE),
        ))
        .with_children(|b| {
            b.spawn(text(label.0, label.1, 1.0));
            if let Some((k, size)) = hotkey {
                b.spawn(text(k, size, 0.55));
            }
        });
}

fn spawn_hud(mut commands: Commands) {
    // Bottom centre: hint line 10 px above the toolbar, toolbar 20 px from bottom.
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                bottom: px(20),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(10),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|root| {
            root.spawn((HintRow, rich_row()));
            root.spawn(panel(10.0, 6.0)).with_children(|bar| {
                for (i, t) in Tool::ALL.into_iter().enumerate() {
                    let key = (i + 1).to_string();
                    slot(bar, Slot::Tool(t), (76.0, 58.0), (t.label(), 16.0), Some((&key, 11.0)));
                }
                // 14 px visual spacer each side of the divider (the 6 px gap included).
                bar.spawn((
                    Node { width: px(2), height: px(58), margin: UiRect::horizontal(px(8)), ..default() },
                    BackgroundColor(rgba(BORDER, 1.0)),
                ));
                slot(bar, Slot::Undo, (52.0, 58.0), ("UNDO", 12.0), Some(("CTRL Z", 10.0)));
                slot(bar, Slot::Redo, (52.0, 58.0), ("REDO", 12.0), Some(("CTRL Y", 10.0)));
            });
        });

    // Top left: title and tallies, no panel.
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: px(16),
                left: px(16),
                flex_direction: FlexDirection::Column,
                row_gap: px(4),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|c| {
            c.spawn(text("HOLLOW ACRE", 14.0, 0.6));
            c.spawn((Node { flex_direction: FlexDirection::Column, ..default() }, Pickable::IGNORE))
                .with_children(|lines| {
                    lines.spawn((PouredText, rich_row())).with_children(|r| spawn_rich(r, &poured_label(0), 11.0, DIM));
                    lines.spawn((HarvestText, text(&harvest_label(0), 11.0, DIM)));
                });
        });

    // Top right: weather picker, clock row, key hint.
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: px(16),
                right: px(16),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexEnd,
                row_gap: px(6),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|c| {
            c.spawn(panel(6.0, 4.0)).with_children(|p| {
                for w in WeatherKind::ALL {
                    slot(p, Slot::Weather(w), (70.0, 28.0), (w.label(), 11.0), None);
                }
            });
            c.spawn(panel(6.0, 8.0)).with_children(|p| {
                p.spawn((ClockText, text("15:00", 16.0, 1.0)));
                slot(p, Slot::Pause, (28.0, 28.0), ("II", 12.0), None);
                p.spawn((PhaseText, text("DAY", 11.0, DIM)));
            });
            c.spawn(rich_row()).with_children(|r| spawn_rich(r, "T pause  ·  [ ] scrub", 10.0, DIM));
        });
}

// ---------------------------------------------------------------- systems

fn activate_slot(
    ev: On<Activate>,
    slots: Query<&Slot>,
    world: Option<Res<World>>,
    mut out: MessageWriter<UiCommand>,
) {
    let Ok(s) = slots.get(ev.entity) else { return };
    let (can_undo, can_redo) = history_flags(world.as_deref());
    let cmd = match *s {
        Slot::Tool(t) => UiCommand::SelectTool(t),
        Slot::Undo if can_undo => UiCommand::Undo,
        Slot::Redo if can_redo => UiCommand::Redo,
        Slot::Undo | Slot::Redo => return,
        Slot::Weather(w) => UiCommand::SetWeather(w),
        Slot::Pause => UiCommand::TogglePause,
    };
    out.write(cmd);
}

fn history_flags(world: Option<&World>) -> (bool, bool) {
    world.map_or((false, false), |w| (w.history.can_undo(), w.history.can_redo()))
}

type SlotView<'a> = (
    &'a Slot,
    &'a Hovered,
    Has<Pressed>,
    &'a mut BackgroundColor,
    &'a mut BorderColor,
    &'a Children,
);

fn style_slots(
    mut slots: Query<SlotView>,
    mut texts: Query<(&mut TextColor, &TextAlpha)>,
    tool: Res<ToolState>,
    sky: Res<Sky>,
    world: Option<Res<World>>,
) {
    let (can_undo, can_redo) = history_flags(world.as_deref());
    for (s, hovered, pressed, mut bg, mut border, children) in &mut slots {
        let (selected, disabled) = match *s {
            Slot::Tool(t) => (t == tool.tool, false),
            Slot::Undo => (false, !can_undo),
            Slot::Redo => (false, !can_redo),
            Slot::Weather(w) => (w == sky.weather, false),
            Slot::Pause => (sky.paused, false),
        };
        let accent = rgba(ACCENT, 1.0);
        let (b, edge, ink, ink_a) = if disabled {
            (Color::NONE, Color::NONE, INK, DISABLED)
        } else if pressed {
            (pressed_accent(), accent, ACCENT_INK, 1.0)
        } else if selected {
            (accent, accent, ACCENT_INK, 1.0)
        } else if hovered.get() {
            (rgba(HOVER, 1.0), accent, INK, 1.0)
        } else {
            (Color::NONE, Color::NONE, INK, 1.0)
        };
        bg.set_if_neq(BackgroundColor(b));
        border.set_if_neq(BorderColor::all(edge));
        for child in children.iter() {
            if let Ok((mut c, a)) = texts.get_mut(child) {
                c.set_if_neq(TextColor(rgba(ink, a.0 * ink_a)));
            }
        }
    }
}

fn update_hint(
    mut commands: Commands,
    tool: Res<ToolState>,
    ghost: Res<Ghost>,
    row: Single<Entity, With<HintRow>>,
    mut shown: Local<Option<String>>,
) {
    let floors = match &ghost.add {
        Some(Structure::Block { floors, .. }) => Some(*floors),
        _ => None,
    };
    let hint = hint_text(tool.tool, tool.phase, tool.spire_kind, floors);
    if shown.as_deref() == Some(hint.as_str()) {
        return;
    }
    commands
        .entity(*row)
        .despawn_related::<Children>()
        .with_children(|r| spawn_rich(r, &hint, 13.0, 0.7));
    *shown = Some(hint);
}

fn update_clock(
    sky: Res<Sky>,
    mut clock: Single<&mut Text, (With<ClockText>, Without<PhaseText>)>,
    mut phase: Single<&mut Text, (With<PhaseText>, Without<ClockText>)>,
    slots: Query<(&Slot, &Children)>,
    mut texts: Query<&mut Text, (Without<ClockText>, Without<PhaseText>)>,
) {
    let t = clock_text(sky.hour);
    if clock.0 != t {
        clock.0 = t;
    }
    let p = phase_word(sky.hour);
    if phase.0 != p {
        phase.0 = p.into();
    }
    let glyph = if sky.paused { ">" } else { "II" };
    let pause = slots.iter().filter(|(s, _)| **s == Slot::Pause).flat_map(|(_, c)| c.iter());
    for child in pause {
        if let Ok(mut txt) = texts.get_mut(child)
            && txt.0 != glyph
        {
            txt.0 = glyph.into();
        }
    }
}

/// Recounts from `DisplayedGrid` only when its version changed, at most 4×/s.
fn update_tally(
    mut commands: Commands,
    grid: Res<DisplayedGrid>,
    time: Res<Time<Real>>,
    poured: Single<Entity, With<PouredText>>,
    mut harvest: Single<&mut Text, With<HarvestText>>,
    mut state: Local<(Option<u64>, f32, Option<(u64, u64)>)>,
) {
    let (counted, last_t, shown) = &mut *state;
    let now = time.elapsed_secs();
    if *counted == Some(grid.version) || (counted.is_some() && now - *last_t < 0.25) {
        return;
    }
    *counted = Some(grid.version);
    *last_t = now;
    let tally = count_tally(&grid.grid);
    if *shown == Some(tally) {
        return;
    }
    *shown = Some(tally);
    commands
        .entity(*poured)
        .despawn_related::<Children>()
        .with_children(|r| spawn_rich(r, &poured_label(tally.0), 11.0, DIM));
    harvest.0 = harvest_label(tally.1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints_per_tool_and_phase() {
        use ToolPhase::*;
        assert_eq!(hint_text(Tool::Wall, Idle, 0, None), "drag to pour a wall  ·  shift: rampart");
        assert_eq!(hint_text(Tool::Wall, Dragging, 0, None), "drag to pour a wall  ·  shift: rampart");
        assert_eq!(hint_text(Tool::Block, Idle, 0, None), "drag a footprint");
        assert_eq!(hint_text(Tool::Block, Dragging, 0, Some(3)), "drag a footprint");
        assert_eq!(
            hint_text(Tool::Block, Raising, 0, Some(4)),
            "move mouse up/down to set floors (4)  ·  click to cast  ·  esc cancels"
        );
        assert_eq!(
            hint_text(Tool::Path, Idle, 0, None),
            "drag to lay a gravel path  ·  cuts gates through walls"
        );
        assert_eq!(hint_text(Tool::Field, Idle, 0, None), "drag to sow a field");
        assert_eq!(hint_text(Tool::Spire, Idle, 0, None), "click to raise a CRYSTAL  ·  tab: next kind");
        assert_eq!(hint_text(Tool::Spire, Idle, 1, None), "click to raise a MONOLITH  ·  tab: next kind");
        assert_eq!(hint_text(Tool::Spire, Idle, 2, None), "click to raise a BLOSSOM TREE  ·  tab: next kind");
        assert_eq!(hint_text(Tool::Raze, Dragging, 0, None), "drag over structures to clear them");
    }

    #[test]
    fn number_formatting() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1000), "1,000");
        assert_eq!(thousands(1_234_567), "1,234,567");
        assert_eq!(poured_label(9920), "POURED 1,240 m³");
        assert_eq!(poured_label(3), "POURED 0 m³");
        assert_eq!(poured_label(4), "POURED 1 m³");
        assert_eq!(harvest_label(312), "HARVEST 312");
    }

    #[test]
    fn clock_and_phase() {
        assert_eq!(clock_text(13.0 + 40.0 / 60.0), "13:40");
        assert_eq!(clock_text(0.0), "00:00");
        assert_eq!(clock_text(23.999), "23:59");
        assert_eq!(clock_text(24.0), "00:00");
        assert_eq!(phase_word(6.5), "DAWN");
        assert_eq!(phase_word(13.0), "DAY");
        assert_eq!(phase_word(18.5), "DUSK");
        assert_eq!(phase_word(0.0), "NIGHT");
        assert_eq!(phase_word(4.99), "NIGHT");
        assert_eq!(phase_word(20.0), "NIGHT");
    }

    #[test]
    fn tally_counts_concrete_and_crops() {
        let mut g = VoxelGrid::new();
        g.set(1, 10, 1, Voxel::Concrete);
        g.set(2, 10, 1, Voxel::ConcreteDark);
        g.set(3, 10, 1, Voxel::Crop);
        g.set(4, 10, 1, Voxel::Gravel);
        assert_eq!(count_tally(&g), (2, 1));
    }
}
