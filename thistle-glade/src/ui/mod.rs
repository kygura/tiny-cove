//! HUD (DESIGN.md): ToolBar, VariantStrip, WeatherPanel, UndoRedo, HintsPanel, tooltips.
//!
//! Hover comes from bevy_picking's `HoverMap` (UI backend); any hovered HUD node sets
//! [`PointerOverUi`] before the tools run so clicks never leak into the world.
//! `--ui-tool NAME` starts with that tool selected (for HUD screenshots).

// Bevy systems take many params and filtered queries by design.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

pub mod strip;

use crate::env::Environment;
use crate::tools::{ActiveTool, DrawState, PointerOverUi, Tool, ToolHover, ToolSettings, ToolSystems};
use crate::weather::{Clock, Weather, WeatherKind};
use crate::world::Glade;
use bevy::{
    picking::{hover::HoverMap, pointer::PointerId},
    prelude::*,
    text::LineHeight,
    ui::UiGlobalTransform,
    window::PrimaryWindow,
};
use glade_core::{Colors, palette};
use strip::{Flag, Item, Palette, Seg, SliderField};

// --- Tokens ----------------------------------------------------------------------------------

const PARCHMENT: Color = Color::srgba(0.953, 0.906, 0.812, 0.92);
const PARCHMENT_DEEP: Color = Color::srgb_u8(0xE9, 0xD9, 0xB8);
const INK: Color = Color::srgb_u8(0x3A, 0x2A, 0x1E);
const INK_SOFT: Color = Color::srgb_u8(0x6F, 0x5A, 0x44);
const WOOD: Color = Color::srgb_u8(0x8C, 0x6A, 0x43);
const WOOD_DARK: Color = Color::srgb_u8(0x6B, 0x4A, 0x2E);
const ACCENT: Color = Color::srgb_u8(0xC0, 0x56, 0x3A);
const HOVER: Color = Color::srgb_u8(0xFB, 0xF2, 0xDF);
const ACTIVE: Color = Color::srgb_u8(0xF2, 0xD4, 0xB8);
const DISABLED_TEXT: Color = Color::srgb_u8(0xB5, 0xA4, 0x8E);
const DISABLED_BG: Color = Color::srgb_u8(0xED, 0xE1, 0xCB);
const INVALID: Color = Color::srgb_u8(0xC8, 0x4A, 0x3A);

const MARGIN: f32 = 16.0;
const TOOLBAR_H: f32 = 64.0;
const TOOLTIP_DELAY: f32 = 0.45;
const FLASH_SECS: f32 = 0.08;

fn swatch_color(p: Palette, i: u8) -> Color {
    let c = Colors { roof: i, wall: i, trim: i };
    let v = match p {
        Palette::Roof => palette::roof(c),
        Palette::Wall => palette::wall(c),
        Palette::Trim => palette::trim(c),
    };
    Color::linear_rgb(v.x, v.y, v.z)
}

// --- Components / resources -----------------------------------------------------------------

/// What pressing a HUD button does.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub enum UiAction {
    Tool(Tool),
    Undo,
    Redo,
    Pause,
    /// `None` = Auto.
    Weather(Option<WeatherKind>),
    Snow,
    Hints,
    Seg(Seg, u8),
    Toggle(Flag),
    Swatch(Palette, u8),
}

/// Slider hit area (32 px tall); children: the bar with [`SliderFill`] and [`SliderKnob`].
#[derive(Component, Clone, Copy)]
pub struct Slider(pub SliderField);
#[derive(Component)]
struct SliderFill(SliderField);
#[derive(Component)]
struct SliderKnob(SliderField);
#[derive(Component)]
struct SliderValue(SliderField);

/// Tooltip shown after hovering: title + one `ink-soft` line.
#[derive(Component, Clone)]
pub struct TooltipText(pub String, pub String);

/// Press flash countdown.
#[derive(Component)]
struct Flash(f32);

/// The coloured square inside a swatch cell.
#[derive(Component)]
struct SwatchSquare;
#[derive(Component)]
struct StyleSquare(Palette);

#[derive(Component)]
struct ButtonLabel;
#[derive(Component)]
struct GroupGap;
#[derive(Component)]
struct StripPanel;
#[derive(Component)]
struct ClockLabel;
#[derive(Component)]
struct HintsBody;
#[derive(Component)]
struct HintsTab;
#[derive(Component)]
struct HintLine(usize);
#[derive(Component)]
struct HintKey;
#[derive(Component)]
struct HintText;
#[derive(Component)]
struct TooltipNode;
#[derive(Component)]
struct TooltipTitle;
#[derive(Component)]
struct TooltipLine;

#[derive(Resource)]
pub struct HintsVisible(pub bool);

impl Default for HintsVisible {
    fn default() -> Self {
        Self(true)
    }
}

#[derive(Resource, Default)]
struct UiPointer {
    /// Hovered widget (button or slider).
    hovered: Option<Entity>,
    /// Slider being dragged.
    dragging: Option<Entity>,
    tip_for: Option<Entity>,
    tip_since: f32,
    tip_suppressed: bool,
    /// Snow slider value while the snow toggle is off.
    snow_amount: f32,
}

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HintsVisible>()
            .insert_resource(UiPointer { snow_amount: 0.6, ..default() })
            .add_systems(Startup, spawn_hud)
            .add_systems(Update, (ui_pointer, ui_keys, ui_press).chain().before(ToolSystems))
            .add_systems(Update, (ui_scale, rebuild_strip, style_buttons, update_sliders, update_texts, update_hints, tooltip).after(ToolSystems));
        let args: Vec<String> = std::env::args().collect();
        if let Some(t) = args.iter().position(|a| a == "--ui-tool").and_then(|i| args.get(i + 1)).and_then(|n| Tool::parse(n)) {
            app.insert_resource(ActiveTool(t));
        }
    }
}

// --- Builders -------------------------------------------------------------------------------

fn text(s: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (Text::new(s), TextFont::from_font_size(size), TextColor(color), LineHeight::RelativeToFont(1.3), Pickable::IGNORE)
}

fn panel(bg: Color) -> impl Bundle {
    (
        BackgroundColor(bg),
        BorderColor::all(WOOD),
        BoxShadow(vec![ShadowStyle {
            color: WOOD_DARK,
            x_offset: px(0),
            y_offset: px(1),
            spread_radius: px(0),
            blur_radius: px(0),
        }]),
    )
}

fn panel_node() -> Node {
    Node { padding: UiRect::all(px(8)), border: UiRect::all(px(2)), border_radius: BorderRadius::all(px(10)), ..default() }
}

/// A HUD button: fixed size if `size` is given, else auto width ≥ 32 with padding.
fn button(p: &mut ChildSpawnerCommands, action: UiAction, label: &str, size: Option<Vec2>, font: f32) -> Entity {
    let mut node = Node {
        border: UiRect::all(px(1)),
        padding: UiRect::axes(px(7), px(1)),
        border_radius: BorderRadius::all(px(6)),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        min_width: px(32),
        min_height: px(32),
        ..default()
    };
    if let Some(s) = size {
        node.width = px(s.x);
        node.height = px(s.y);
        node.min_width = px(s.x.min(32.0));
        node.min_height = px(s.y.min(32.0));
        node.padding = UiRect::all(px(1));
    }
    p.spawn((node, action, BackgroundColor(PARCHMENT), BorderColor::all(WOOD_DARK)))
        .with_children(|b| {
            b.spawn((text(label, font, INK), ButtonLabel));
        })
        .id()
}

fn slider_row(p: &mut ChildSpawnerCommands, field: SliderField, label_w: f32) {
    p.spawn(Node { align_items: AlignItems::Center, column_gap: px(6), ..default() }).insert(Pickable::IGNORE).with_children(|r| {
        if label_w > 0.0 {
            r.spawn((text(field.label(), 12.0, INK), Node { width: px(label_w), ..default() }));
        }
        r.spawn((Slider(field), Node { width: px(120), height: px(32), align_items: AlignItems::Center, ..default() }))
            .with_children(|t| {
                t.spawn((
                    Node {
                        width: percent(100),
                        height: px(10),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(5)),
                        ..default()
                    },
                    BackgroundColor(PARCHMENT_DEEP),
                    BorderColor::all(WOOD_DARK),
                    Pickable::IGNORE,
                ))
                .with_children(|bar| {
                    bar.spawn((
                        SliderFill(field),
                        Node { width: percent(50), height: percent(100), border_radius: BorderRadius::all(px(4)), ..default() },
                        BackgroundColor(ACCENT),
                        Pickable::IGNORE,
                    ));
                    bar.spawn((
                        SliderKnob(field),
                        Node {
                            position_type: PositionType::Absolute,
                            left: percent(50),
                            top: px(-5),
                            width: px(8),
                            height: px(18),
                            margin: UiRect::left(px(-4)),
                            border_radius: BorderRadius::all(px(3)),
                            ..default()
                        },
                        BackgroundColor(INK),
                        Pickable::IGNORE,
                    ));
                });
            });
        r.spawn((text("", 11.0, INK_SOFT), SliderValue(field), Node { width: px(36), ..default() }));
    });
}

fn swatch(p: &mut ChildSpawnerCommands, pal: Palette, i: u8) {
    p.spawn((
        UiAction::Swatch(pal, i),
        Node { width: px(32), height: px(32), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
        BackgroundColor(Color::NONE),
        BorderColor::all(Color::NONE),
    ))
    .with_children(|c| {
        c.spawn((
            SwatchSquare,
            Node { width: px(24), height: px(24), border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(6)), ..default() },
            BackgroundColor(swatch_color(pal, i)),
            BorderColor::all(WOOD_DARK),
            Pickable::IGNORE,
        ));
    });
}

fn group(p: &mut ChildSpawnerCommands, gap: f32, f: impl FnOnce(&mut ChildSpawnerCommands)) {
    p.spawn((Node { align_items: AlignItems::Center, column_gap: px(gap), row_gap: px(4), ..default() }, Pickable::IGNORE))
        .with_children(f);
}

fn spawn_hud(mut commands: Commands, tool: Res<ActiveTool>) {
    // ToolBar (bottom centre).
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                bottom: px(MARGIN),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|root| {
            root.spawn((Node { height: px(TOOLBAR_H + 4.0), align_items: AlignItems::Center, ..panel_node() }, panel(PARCHMENT)))
                .with_children(|bar| {
                    for g in 0..4u8 {
                        if g > 0 {
                            bar.spawn((
                                GroupGap,
                                Node { width: px(1), height: px(40), margin: UiRect::horizontal(px(6)), ..default() },
                                BackgroundColor(WOOD),
                            ));
                        }
                        group(bar, 4.0, |grp| {
                            for t in Tool::ALL.into_iter().filter(|t| t.group() == g) {
                                let e = button(grp, UiAction::Tool(t), t.label(), Some(Vec2::new(56.0, 48.0)), 14.0);
                                grp.commands().entity(e).insert(TooltipText(format!("{}  ({})", t.name(), t.key_label()), t.blurb().into()));
                            }
                        });
                    }
                });
        });

    // VariantStrip (8 px above the toolbar, centred). Wraps rather than reaching the hints panel.
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                bottom: px(MARGIN + TOOLBAR_H + 4.0 + 8.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|root| {
            root.spawn((
                StripPanel,
                Node {
                    padding: UiRect::axes(px(8), px(4)),
                    min_height: px(44),
                    max_width: Val::Vw(54.0),
                    flex_wrap: FlexWrap::Wrap,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    column_gap: px(12),
                    row_gap: px(2),
                    ..panel_node()
                },
                panel(PARCHMENT_DEEP),
            ));
        });

    // UndoRedo (top left).
    commands
        .spawn((
            Node { position_type: PositionType::Absolute, left: px(MARGIN), top: px(MARGIN), column_gap: px(4), ..panel_node() },
            panel(PARCHMENT),
        ))
        .with_children(|p| {
            let u = button(p, UiAction::Undo, "<", Some(Vec2::new(40.0, 36.0)), 14.0);
            p.commands().entity(u).insert(TooltipText("Undo  (Ctrl+Z)".into(), "Step back".into()));
            let r = button(p, UiAction::Redo, ">", Some(Vec2::new(40.0, 36.0)), 14.0);
            p.commands().entity(r).insert(TooltipText("Redo  (Ctrl+Y)".into(), "Step forward".into()));
        });

    // WeatherPanel (top right).
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: px(MARGIN),
                top: px(MARGIN),
                width: px(236),
                flex_direction: FlexDirection::Column,
                row_gap: px(4),
                ..panel_node()
            },
            panel(PARCHMENT),
        ))
        .with_children(|p| {
            p.spawn((Node { align_items: AlignItems::Center, justify_content: JustifyContent::SpaceBetween, ..default() }, Pickable::IGNORE))
                .with_children(|r| {
                    r.spawn((text("12:00", 16.0, INK), ClockLabel));
                    let e = button(r, UiAction::Pause, "||", Some(Vec2::new(32.0, 32.0)), 14.0);
                    r.commands().entity(e).insert(TooltipText("Pause time  (T)".into(), "[ ] scrub the hour".into()));
                });
            group(p, 3.0, |r| {
                let names = ["Clr", "Ovc", "Rain", "Strm", "Fog", "Snow"];
                for (k, n) in WeatherKind::ALL.into_iter().zip(names) {
                    let e = button(r, UiAction::Weather(Some(k)), n, Some(Vec2::new(28.0, 28.0)), 11.0);
                    r.commands().entity(e).insert(TooltipText(format!("Weather: {}", k.name()), "Overrides Auto".into()));
                }
                let e = button(r, UiAction::Weather(None), "Auto", Some(Vec2::new(28.0, 28.0)), 11.0);
                r.commands().entity(e).insert(TooltipText("Auto weather".into(), "Cycles on its own".into()));
            });
            slider_row(p, SliderField::Fog, 44.0);
            p.spawn((Node { align_items: AlignItems::Center, column_gap: px(4), ..default() }, Pickable::IGNORE)).with_children(|r| {
                button(r, UiAction::Snow, "Snow", None, 12.0);
                r.spawn((Node::default(), Pickable::IGNORE)).with_children(|s| slider_row(s, SliderField::Snow, 0.0));
            });
        });

    // HintsPanel (bottom left, above the toolbar) + collapsed `?` tab.
    commands
        .spawn((
            HintsBody,
            Node {
                position_type: PositionType::Absolute,
                left: px(MARGIN),
                bottom: px(MARGIN + TOOLBAR_H + 4.0 + 8.0),
                width: px(260),
                flex_direction: FlexDirection::Column,
                ..panel_node()
            },
            panel(PARCHMENT),
        ))
        .with_children(|p| {
            for i in 0..7 {
                p.spawn((HintLine(i), Node { column_gap: px(8), ..default() }, Pickable::IGNORE)).with_children(|l| {
                    l.spawn((text("", 11.0, INK), HintKey, Node { width: px(80), flex_shrink: 0.0, ..default() }));
                    l.spawn((text("", 12.0, INK_SOFT), HintText));
                });
            }
        });
    commands
        .spawn((
            HintsTab,
            UiAction::Hints,
            Node {
                position_type: PositionType::Absolute,
                left: px(MARGIN),
                bottom: px(MARGIN + TOOLBAR_H + 4.0 + 8.0),
                width: px(32),
                height: px(32),
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(10)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                display: Display::None,
                ..default()
            },
            panel(PARCHMENT),
            TooltipText("Hints  (H)".into(), "Show the gesture list".into()),
        ))
        .with_children(|t| {
            t.spawn((text("?", 14.0, INK), ButtonLabel));
        });

    // Tooltip.
    commands
        .spawn((
            TooltipNode,
            Node {
                position_type: PositionType::Absolute,
                padding: UiRect::axes(px(8), px(4)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(4)),
                flex_direction: FlexDirection::Column,
                display: Display::None,
                ..default()
            },
            BackgroundColor(PARCHMENT_DEEP),
            BorderColor::all(WOOD_DARK),
            GlobalZIndex(100),
            Pickable::IGNORE,
        ))
        .with_children(|t| {
            t.spawn((text("", 12.0, INK), TooltipTitle));
            t.spawn((text("", 12.0, INK_SOFT), TooltipLine));
        });

    // The strip is filled by `rebuild_strip` (runs on the first frame: `ActiveTool` is new).
    let _ = tool;
}

fn fill_strip(p: &mut ChildSpawnerCommands, tool: Tool) {
    for item in strip::strip_items(tool) {
        match item {
            Item::Segs(seg) => group(p, 0.0, |g| {
                for (i, l) in seg.labels().iter().enumerate() {
                    button(g, UiAction::Seg(seg, i as u8), l, None, 12.0);
                }
            }),
            Item::Toggle(f) => {
                button(p, UiAction::Toggle(f), f.label(), None, 12.0);
            }
            Item::Slider(f) => slider_row(p, f, 64.0),
            Item::Swatches(pal) => group(p, 0.0, |g| {
                for i in 0..pal.count() {
                    swatch(g, pal, i);
                }
            }),
            Item::Grid => {
                p.spawn((
                    Node {
                        display: Display::Grid,
                        grid_template_columns: RepeatedGridTrack::px(7, 76.0),
                        column_gap: px(4),
                        row_gap: px(4),
                        ..default()
                    },
                    Pickable::IGNORE,
                ))
                .with_children(|g| {
                    for (i, l) in Seg::Clutter.labels().iter().enumerate() {
                        button(g, UiAction::Seg(Seg::Clutter, i as u8), l, Some(Vec2::new(76.0, 36.0)), 12.0);
                    }
                });
            }
            Item::StyleShow => group(p, 8.0, |g| {
                g.spawn(text("Copied style", 12.0, INK));
                for pal in [Palette::Roof, Palette::Wall, Palette::Trim] {
                    g.spawn((Node { width: px(32), height: px(32), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },))
                        .with_children(|c| {
                            c.spawn((
                                StyleSquare(pal),
                                Node { width: px(24), height: px(24), border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(6)), ..default() },
                                BackgroundColor(swatch_color(pal, 0)),
                                BorderColor::all(WOOD_DARK),
                                Pickable::IGNORE,
                            ));
                        });
                }
            }),
        }
    }
}

// --- Systems --------------------------------------------------------------------------------

fn ui_scale(
    window: Query<&Window, (With<PrimaryWindow>, Changed<Window>)>,
    mut scale: ResMut<UiScale>,
    mut gaps: Query<&mut Node, With<GroupGap>>,
) {
    let Ok(w) = window.single() else { return };
    let s = ((w.height() / 720.0 * 4.0).round() / 4.0).clamp(0.75, 2.0);
    if scale.0 != s {
        scale.0 = s;
    }
    let gap = if w.width() / s < 1150.0 { 2.0 } else { 6.0 };
    for mut n in &mut gaps {
        if n.margin.left != px(gap) {
            n.margin = UiRect::horizontal(px(gap));
        }
    }
}

/// Hovered widget + whether the cursor is over the HUD at all.
fn ui_pointer(
    hover_map: Option<Res<HoverMap>>,
    nodes: Query<(), With<ComputedNode>>,
    widgets: Query<(), Or<(With<UiAction>, With<Slider>)>>,
    parents: Query<&ChildOf>,
    mut over: ResMut<PointerOverUi>,
    mut ptr: ResMut<UiPointer>,
    mouse: Res<ButtonInput<MouseButton>>,
) {
    let mut over_ui = false;
    let mut hovered = None;
    if let Some(hits) = hover_map.as_ref().and_then(|m| m.get(&PointerId::Mouse)) {
        for &e in hits.keys() {
            if !nodes.contains(e) {
                continue;
            }
            over_ui = true;
            let mut cur = Some(e);
            for _ in 0..4 {
                let Some(c) = cur else { break };
                if widgets.contains(c) {
                    hovered = Some(c);
                    break;
                }
                cur = parents.get(c).ok().map(|p| p.parent());
            }
        }
    }
    // A slider drag keeps the HUD in charge until release.
    if ptr.dragging.is_some() {
        over_ui = true;
        if !mouse.pressed(MouseButton::Left) {
            ptr.dragging = None;
        }
    }
    if over.0 != over_ui {
        over.0 = over_ui;
    }
    if ptr.hovered != hovered {
        ptr.hovered = hovered;
    }
}

fn ui_keys(
    keys: Res<ButtonInput<KeyCode>>,
    tool: Res<ActiveTool>,
    mut hints: ResMut<HintsVisible>,
    mut settings: ResMut<ToolSettings>,
    mut weather: ResMut<Weather>,
    mut ptr: ResMut<UiPointer>,
) {
    if keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]) {
        return;
    }
    if keys.just_pressed(KeyCode::KeyH) {
        hints.0 = !hints.0;
    }
    let items = strip::strip_items(tool.0);
    if keys.just_pressed(KeyCode::Tab)
        && let Some(seg) = items.iter().find_map(|i| match i {
            Item::Segs(s) => Some(*s),
            Item::Grid => Some(Seg::Clutter),
            _ => None,
        })
    {
        let n = seg.labels().len() as u8;
        let i = (seg.get(&settings, tool.0) + 1) % n;
        seg.set(&mut settings, tool.0, i);
    }
    let dir = if keys.just_pressed(KeyCode::Equal) {
        1.0
    } else if keys.just_pressed(KeyCode::Minus) {
        -1.0
    } else {
        0.0
    };
    if dir != 0.0
        && let Some(f) = items.iter().find_map(|i| if let Item::Slider(f) = i { Some(*f) } else { None })
    {
        let (lo, hi) = f.range();
        let mut snow = ptr.snow_amount;
        let v = f.get(&settings, tool.0, &weather, snow) + dir * (hi - lo) * 0.05;
        f.set(&mut settings, tool.0, &mut weather, &mut snow, v);
        ptr.snow_amount = snow;
    }
}

fn ui_press(
    mouse: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut ptr: ResMut<UiPointer>,
    actions: Query<&UiAction>,
    sliders: Query<(&Slider, &ComputedNode, &UiGlobalTransform)>,
    mut tool: ResMut<ActiveTool>,
    mut settings: ResMut<ToolSettings>,
    mut clock: ResMut<Clock>,
    mut weather: ResMut<Weather>,
    mut hints: ResMut<HintsVisible>,
    glade: Option<ResMut<Glade>>,
    mut commands: Commands,
) {
    if mouse.just_pressed(MouseButton::Left) {
        ptr.tip_suppressed = true;
        if let Some(e) = ptr.hovered {
            if sliders.contains(e) {
                ptr.dragging = Some(e);
            } else if let Ok(action) = actions.get(e) {
                commands.entity(e).insert(Flash(FLASH_SECS));
                let t = tool.0;
                match *action {
                    UiAction::Tool(n) => {
                        if tool.0 != n {
                            tool.0 = n;
                        }
                    }
                    UiAction::Undo => {
                        if let Some(mut g) = glade {
                            g.undo();
                        }
                    }
                    UiAction::Redo => {
                        if let Some(mut g) = glade {
                            g.redo();
                        }
                    }
                    UiAction::Pause => clock.paused = !clock.paused,
                    UiAction::Weather(Some(k)) => {
                        weather.kind = k;
                        weather.auto = false;
                    }
                    UiAction::Weather(None) => weather.auto = true,
                    UiAction::Snow => {
                        weather.snow_override = match weather.snow_override {
                            Some(_) => None,
                            None => Some(ptr.snow_amount),
                        }
                    }
                    UiAction::Hints => hints.0 = !hints.0,
                    UiAction::Seg(seg, i) => {
                        if seg.enabled(&settings) {
                            seg.set(&mut settings, t, i);
                        }
                    }
                    UiAction::Toggle(f) => {
                        if f.enabled(&settings, t) {
                            f.toggle(&mut settings, t);
                        }
                    }
                    UiAction::Swatch(pal, i) => match pal {
                        Palette::Roof => settings.colors.roof = i,
                        Palette::Wall => settings.colors.wall = i,
                        Palette::Trim => settings.colors.trim = i,
                    },
                }
            }
        }
    }
    // Slider drag: value from the cursor's position along the track.
    if let (Some(e), Ok(w)) = (ptr.dragging, window.single())
        && let (Ok((slider, node, tf)), Some(c)) = (sliders.get(e), w.physical_cursor_position())
        && let Some(n) = node.normalize_point(*tf, c)
    {
        let (lo, hi) = slider.0.range();
        let v = lo + (n.x + 0.5).clamp(0.0, 1.0) * (hi - lo);
        let mut snow = ptr.snow_amount;
        slider.0.set(&mut settings, tool.0, &mut weather, &mut snow, v);
        ptr.snow_amount = snow;
    }
}

fn rebuild_strip(tool: Res<ActiveTool>, mut strip: Query<(Entity, &mut Node), With<StripPanel>>, mut commands: Commands) {
    if !tool.is_changed() {
        return;
    }
    for (e, mut node) in &mut strip {
        commands.entity(e).despawn_children();
        let empty = strip::strip_items(tool.0).is_empty();
        node.display = if empty { Display::None } else { Display::Flex };
        if !empty {
            commands.entity(e).with_children(|p| fill_strip(p, tool.0));
        }
    }
}

fn style_buttons(
    time: Res<Time>,
    ptr: Res<UiPointer>,
    tool: Res<ActiveTool>,
    settings: Res<ToolSettings>,
    clock: Res<Clock>,
    weather: Res<Weather>,
    glade: Option<Res<Glade>>,
    mut buttons: Query<(Entity, &UiAction, &mut Node, &mut BackgroundColor, &mut BorderColor, Option<&mut Flash>, Option<&Children>)>,
    mut labels: Query<&mut TextColor, With<ButtonLabel>>,
    mut squares: Query<(&mut Node, &mut BorderColor), (With<SwatchSquare>, Without<UiAction>)>,
    mut commands: Commands,
) {
    let t = tool.0;
    for (e, action, mut node, mut bg, mut border, flash, children) in &mut buttons {
        let flashing = if let Some(mut f) = flash {
            f.0 -= time.delta_secs();
            if f.0 <= 0.0 {
                commands.entity(e).remove::<Flash>();
            }
            true
        } else {
            false
        };
        let (active, disabled) = match *action {
            UiAction::Tool(n) => (n == t, false),
            UiAction::Undo => (false, !glade.as_ref().is_some_and(|g| g.can_undo())),
            UiAction::Redo => (false, !glade.as_ref().is_some_and(|g| g.can_redo())),
            UiAction::Pause => (clock.paused, false),
            UiAction::Weather(Some(k)) => (!weather.auto && weather.kind == k, false),
            UiAction::Weather(None) => (weather.auto, false),
            UiAction::Snow => (weather.snow_override.is_some(), false),
            UiAction::Hints => (false, false),
            UiAction::Seg(seg, i) => (seg.get(&settings, t) == i, !seg.enabled(&settings)),
            UiAction::Toggle(f) => (f.get(&settings, t), !f.enabled(&settings, t)),
            UiAction::Swatch(pal, i) => {
                let sel = match pal {
                    Palette::Roof => settings.colors.roof,
                    Palette::Wall => settings.colors.wall,
                    Palette::Trim => settings.colors.trim,
                } == i;
                for c in children.into_iter().flatten() {
                    if let Ok((mut n, mut b)) = squares.get_mut(*c) {
                        let (w, col) = if sel { (2.0, ACCENT) } else { (1.0, WOOD_DARK) };
                        if n.border.left != px(w) {
                            n.border = UiRect::all(px(w));
                        }
                        b.set_if_neq(BorderColor::all(col));
                    }
                }
                let hov = ptr.hovered == Some(e);
                bg.set_if_neq(BackgroundColor(if hov { HOVER } else { Color::NONE }));
                continue;
            }
        };
        if matches!(action, UiAction::Hints) {
            continue;
        }
        let hovered = ptr.hovered == Some(e) && !disabled;
        let fill = if disabled {
            DISABLED_BG
        } else if active || flashing {
            ACTIVE
        } else if hovered {
            HOVER
        } else {
            PARCHMENT
        };
        bg.set_if_neq(BackgroundColor(fill));
        let (w, col) = if active && !disabled { (2.0, ACCENT) } else { (1.0, WOOD_DARK) };
        border.set_if_neq(BorderColor::all(col));
        if node.border.left != px(w) {
            node.border = UiRect::all(px(w));
            // Keep the content in place when the border thickens.
            node.padding = if node.width == Val::Auto { UiRect::axes(px(8.0 - w), px(2.0 - w)) } else { UiRect::all(px(2.0 - w)) };
        }
        for c in children.into_iter().flatten() {
            if let Ok(mut tc) = labels.get_mut(*c) {
                tc.set_if_neq(TextColor(if disabled { DISABLED_TEXT } else { INK }));
            }
        }
    }
}

fn update_sliders(
    tool: Res<ActiveTool>,
    settings: Res<ToolSettings>,
    weather: Res<Weather>,
    ptr: Res<UiPointer>,
    mut fills: Query<(&SliderFill, &mut Node), Without<SliderKnob>>,
    mut knobs: Query<(&SliderKnob, &mut Node), Without<SliderFill>>,
    mut values: Query<(&SliderValue, &mut Text)>,
) {
    let frac = |f: SliderField| {
        let (lo, hi) = f.range();
        ((f.get(&settings, tool.0, &weather, ptr.snow_amount) - lo) / (hi - lo)).clamp(0.0, 1.0) * 100.0
    };
    for (f, mut n) in &mut fills {
        let w = percent(frac(f.0));
        if n.width != w {
            n.width = w;
        }
    }
    for (k, mut n) in &mut knobs {
        let l = percent(frac(k.0));
        if n.left != l {
            n.left = l;
        }
    }
    for (v, mut t) in &mut values {
        let s = v.0.format(v.0.get(&settings, tool.0, &weather, ptr.snow_amount));
        if t.0 != s {
            t.0 = s;
        }
    }
}

fn update_texts(
    env: Res<Environment>,
    clock: Res<Clock>,
    settings: Res<ToolSettings>,
    actions: Query<(&UiAction, &Children)>,
    mut clock_label: Query<&mut Text, (With<ClockLabel>, Without<ButtonLabel>)>,
    mut labels: Query<&mut Text, (With<ButtonLabel>, Without<ClockLabel>)>,
    mut style: Query<(&StyleSquare, &mut BackgroundColor)>,
) {
    let h = env.hour.rem_euclid(24.0);
    let s = format!("{:02}:{:02}", h.floor() as u32, ((h.fract() * 60.0).floor() as u32).min(59));
    for mut t in &mut clock_label {
        if t.0 != s {
            t.0 = s.clone();
        }
    }
    for (a, children) in &actions {
        if *a == UiAction::Pause {
            let want = if clock.paused { ">" } else { "||" };
            for c in children {
                if let Ok(mut t) = labels.get_mut(*c)
                    && t.0 != want
                {
                    t.0 = want.into();
                }
            }
        }
    }
    for (sq, mut bg) in &mut style {
        let i = match sq.0 {
            Palette::Roof => settings.colors.roof,
            Palette::Wall => settings.colors.wall,
            Palette::Trim => settings.colors.trim,
        };
        bg.set_if_neq(BackgroundColor(swatch_color(sq.0, i)));
    }
}

fn update_hints(
    tool: Res<ActiveTool>,
    draw: Res<DrawState>,
    hover: Res<ToolHover>,
    settings: Res<ToolSettings>,
    visible: Res<HintsVisible>,
    mut body: Query<&mut Node, (With<HintsBody>, Without<HintsTab>, Without<HintLine>)>,
    mut tab: Query<&mut Node, (With<HintsTab>, Without<HintsBody>, Without<HintLine>)>,
    mut lines: Query<(&HintLine, &mut Node, &Children), (Without<HintsBody>, Without<HintsTab>)>,
    mut keys: Query<(&mut Text, &mut TextColor), (With<HintKey>, Without<HintText>)>,
    mut texts: Query<(&mut Text, &mut TextColor), (With<HintText>, Without<HintKey>)>,
) {
    let show = |b: bool| if b { Display::Flex } else { Display::None };
    for mut n in &mut body {
        if n.display != show(visible.0) {
            n.display = show(visible.0);
        }
    }
    for mut n in &mut tab {
        if n.display != show(!visible.0) {
            n.display = show(!visible.0);
        }
    }
    let content = strip::hint_lines(tool.0, *draw, &hover, &settings);
    let state = *draw;
    for (line, mut node, children) in &mut lines {
        let item = content.get(line.0);
        if node.display != show(item.is_some()) {
            node.display = show(item.is_some());
        }
        let Some((k, a)) = item else { continue };
        let invalid = line.0 == 0 && hover.invalid.is_some();
        let strong = match state {
            DrawState::Idle => false,
            DrawState::Hover => line.0 == 0,
            _ => true,
        };
        let col = if invalid {
            INVALID
        } else if strong {
            INK
        } else {
            INK_SOFT
        };
        for c in children {
            if let Ok((mut t, mut tc)) = keys.get_mut(*c) {
                if t.0 != *k {
                    t.0 = k.clone();
                }
                tc.set_if_neq(TextColor(if invalid { INVALID } else { INK }));
            }
            if let Ok((mut t, mut tc)) = texts.get_mut(*c) {
                if t.0 != *a {
                    t.0 = a.clone();
                }
                tc.set_if_neq(TextColor(col));
            }
        }
    }
}

fn tooltip(
    time: Res<Time>,
    mut ptr: ResMut<UiPointer>,
    window: Query<&Window, With<PrimaryWindow>>,
    scale: Res<UiScale>,
    tips: Query<&TooltipText>,
    mut node: Query<&mut Node, With<TooltipNode>>,
    mut title: Query<&mut Text, (With<TooltipTitle>, Without<TooltipLine>)>,
    mut line: Query<&mut Text, (With<TooltipLine>, Without<TooltipTitle>)>,
) {
    let now = time.elapsed_secs();
    let target = ptr.hovered.filter(|e| tips.contains(*e));
    if target != ptr.tip_for {
        ptr.tip_for = target;
        ptr.tip_since = now;
        ptr.tip_suppressed = false;
    }
    let Ok(mut n) = node.single_mut() else { return };
    let cursor = window.single().ok().and_then(|w| w.cursor_position().map(|c| (c, w.height())));
    let show = match (target, cursor) {
        (Some(e), Some((c, h))) if !ptr.tip_suppressed && now - ptr.tip_since >= TOOLTIP_DELAY => {
            let tip = tips.get(e).unwrap();
            for mut t in &mut title {
                if t.0 != tip.0 {
                    t.0 = tip.0.clone();
                }
            }
            for mut t in &mut line {
                if t.0 != tip.1 {
                    t.0 = tip.1.clone();
                }
            }
            let s = scale.0.max(0.01);
            let (left, bottom) = (px((c.x / s - 40.0).max(4.0)), px((h - c.y) / s + 18.0));
            if n.left != left || n.bottom != bottom {
                n.left = left;
                n.bottom = bottom;
            }
            true
        }
        _ => false,
    };
    let d = if show { Display::Flex } else { Display::None };
    if n.display != d {
        n.display = d;
    }
}
