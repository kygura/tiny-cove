//! HUD: bottom-centre toolbar, hint line and the top-left title and pour tally.
//!
//! The default font is ASCII-only: the cubic-metre sign is written `m3`.
//!
//! Slots carry `Interaction` (deprecated in 0.20 but still driven by `ui_focus_system`;
//! the `Button` marker is private, so we spawn `Interaction` + `FocusPolicy::Block`).
#![allow(deprecated)]

use crate::game::{Doc, EditCommand, Grid, Tool, ToolState};
use crate::voxel::{GROUND_Y, SIZE_X, SIZE_Y, SIZE_Z, Voxel};
use bevy::{picking::Pickable, prelude::*, ui::FocusPolicy};

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ToolState>()
            .add_message::<EditCommand>()
            .add_systems(Startup, spawn_hud)
            .add_systems(Update, (click_slots, style_slots, update_hint, update_tally));
    }
}

fn rgba(hex: u32, a: f32) -> Color {
    let c = |shift: u32| ((hex >> shift) & 0xff) as f32 / 255.0;
    Color::srgba(c(16), c(8), c(0), a)
}

const PANEL: u32 = 0x4a433c;
const BORDER: u32 = 0x2c2b2a;
const INK: u32 = 0xd9d3c7;
const ACCENT: u32 = 0xe3b35a;
const DARK_INK: u32 = 0x1d1f22;
const DIM: f32 = 0.35;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Slot {
    Tool(Tool),
    Undo,
    Redo,
}

/// Base opacity of a slot's text line (before dimming).
#[derive(Component)]
struct TextAlpha(f32);

#[derive(Component)]
struct HintText;

#[derive(Component)]
struct TallyText;

fn text(s: &str, size: f32, alpha: f32) -> impl Bundle {
    (
        Text::new(s),
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(rgba(INK, alpha)),
        TextAlpha(alpha),
        Pickable::IGNORE,
    )
}

fn slot(parent: &mut ChildSpawnerCommands, slot: Slot, width: f32, label: &str, hotkey: Option<&str>) {
    let size = if hotkey.is_some() { 16.0 } else { 11.0 };
    parent
        .spawn((
            Interaction::None,
            FocusPolicy::Block,
            slot,
            Node {
                width: px(width),
                height: px(56),
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
            b.spawn(text(label, size, 1.0));
            if let Some(k) = hotkey {
                b.spawn(text(k, 11.0, 0.6));
            }
        });
}

fn spawn_hud(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                bottom: px(16),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(8),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|root| {
            root.spawn((HintText, text("", 12.0, 0.6)));
            root.spawn((
                Node {
                    padding: UiRect::all(px(12)),
                    column_gap: px(8),
                    border: UiRect::all(px(2)),
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(rgba(PANEL, 0.85)),
                BorderColor::all(rgba(BORDER, 1.0)),
            ))
            .with_children(|bar| {
                for (i, t) in Tool::ALL.into_iter().enumerate() {
                    slot(bar, Slot::Tool(t), 72.0, t.label(), Some(&(i + 1).to_string()));
                }
                bar.spawn(Node {
                    width: px(16),
                    ..default()
                });
                slot(bar, Slot::Undo, 40.0, "UNDO", None);
                slot(bar, Slot::Redo, 40.0, "REDO", None);
            });
        });

    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: px(12),
                left: px(12),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|c| {
            c.spawn(text("CONCRETE GLADE", 11.0, 0.45));
            c.spawn((TallyText, text(&tally_label(0), 11.0, 0.45)));
        });
}

fn click_slots(
    q: Query<(&Interaction, &Slot), Changed<Interaction>>,
    mut out: MessageWriter<EditCommand>,
) {
    for (i, s) in &q {
        if *i == Interaction::Pressed {
            out.write(match *s {
                Slot::Tool(t) => EditCommand::SelectTool(t),
                Slot::Undo => EditCommand::Undo,
                Slot::Redo => EditCommand::Redo,
            });
        }
    }
}

fn style_slots(
    mut slots: Query<(&Slot, &Interaction, &mut BackgroundColor, &mut BorderColor, &Children)>,
    mut texts: Query<(&mut TextColor, &TextAlpha)>,
    tool: Res<ToolState>,
    doc: Option<Res<Doc>>,
) {
    let can_undo = doc.as_ref().is_none_or(|d| d.0.can_undo());
    let can_redo = doc.as_ref().is_none_or(|d| d.0.can_redo());
    for (s, interaction, mut bg, mut border, children) in &mut slots {
        let (selected, dim) = match s {
            Slot::Tool(t) => (*t == tool.tool, 1.0),
            Slot::Undo => (false, if can_undo { 1.0 } else { DIM }),
            Slot::Redo => (false, if can_redo { 1.0 } else { DIM }),
        };
        let hovered = *interaction != Interaction::None;
        bg.0 = if selected { rgba(ACCENT, 1.0) } else { Color::NONE };
        *border = BorderColor::all(if hovered { rgba(ACCENT, dim) } else { Color::NONE });
        let ink = if selected { DARK_INK } else { INK };
        for child in children.iter() {
            if let Ok((mut c, a)) = texts.get_mut(child) {
                c.0 = rgba(ink, a.0 * dim);
            }
        }
    }
}

fn update_hint(tool: Res<ToolState>, mut hint: Single<&mut Text, With<HintText>>) {
    // The built-in font is ASCII-only, so the middle dot becomes a bar.
    let shown = tool.hint.replace('·', "|");
    if tool.is_changed() && hint.0 != shown {
        hint.0 = shown;
    }
}

/// Recounting is ~1.6M cells, so it is throttled while a drag regenerates the grid.
fn update_tally(
    grid: Option<Res<Grid>>,
    time: Res<Time>,
    mut state: Local<(bool, f32)>,
    mut label: Single<&mut Text, With<TallyText>>,
) {
    let Some(grid) = grid else { return };
    let (dirty, last) = &mut *state;
    *dirty |= grid.is_changed();
    let now = time.elapsed_secs();
    if *dirty && now - *last > 0.25 {
        *dirty = false;
        *last = now;
        label.0 = tally_label(count_structure(&grid.0));
    }
}

fn is_structure(v: Voxel) -> bool {
    matches!(
        v,
        Voxel::Concrete
            | Voxel::ConcreteDark
            | Voxel::WindowDark
            | Voxel::WindowLit
            | Voxel::Rust
            | Voxel::Lamp
    )
}

fn count_structure(grid: &crate::voxel::VoxelGrid) -> u64 {
    let mut n = 0;
    for y in GROUND_Y..SIZE_Y {
        for z in 0..SIZE_Z {
            for x in 0..SIZE_X {
                n += is_structure(grid.get(IVec3::new(x, y, z))) as u64;
            }
        }
    }
    n
}

fn thousands(n: u64) -> String {
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

/// 0.125 m3 per voxel, times 1000 for drama.
fn tally_label(voxels: u64) -> String {
    format!("POURED {} m3", thousands(voxels * 125))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voxel::VoxelGrid;

    #[test]
    fn thousands_separators() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1000), "1,000");
        assert_eq!(thousands(12480), "12,480");
        assert_eq!(thousands(1234567), "1,234,567");
    }

    #[test]
    fn tally_counts_structure_above_ground_only() {
        let mut g = VoxelGrid::new();
        g.set(IVec3::new(1, 0, 1), Voxel::Concrete); // below ground
        g.set(IVec3::new(1, GROUND_Y, 1), Voxel::Concrete);
        g.set(IVec3::new(2, GROUND_Y + 3, 1), Voxel::WindowLit);
        g.set(IVec3::new(3, GROUND_Y, 1), Voxel::Asphalt);
        g.set(IVec3::new(4, GROUND_Y, 1), Voxel::DeadWood);
        assert_eq!(count_structure(&g), 2);
        assert_eq!(tally_label(2), "POURED 250 m3");
        assert_eq!(tally_label(99_840), "POURED 12,480,000 m3");
    }
}
