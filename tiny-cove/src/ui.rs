//! HUD, "Woad Cloth" (DESIGN.md part B). Reads/writes `tools::ToolSettings`, reads
//! `tools::ToolStatus`, sets `tools::PointerOverUi`, drives `weather::{Clock, Weather}` and
//! `world::Glade` undo/redo.
//!
//! Layout: static panels are spawned once in `spawn_hud`; the variant strip and the hint list are
//! rebuilt when the tool changes. Every button carries an [`Action`]; `sync_btn` derives its
//! [`Btn`] state, `paint` turns that into colours, `click` applies presses.
#![allow(deprecated)] // `Interaction` is deprecated in 0.20 but is still what drives hover/press.

mod strip;

use crate::env::Tide;
use crate::tools::{PointerOverUi, Tool, ToolSettings, ToolStatus};
use crate::weather::{Clock, Weather, WeatherKind};
use crate::world::Glade;
use bevy::{prelude::*, text::{LineBreak, LineHeight}, ui::RelativeCursorPosition, ui::UiSystems, window::PrimaryWindow};
use cairn_cove_core::{ClutterKind, Colors, TIDE_RANGE};
use strip::{C, CLUTTER, GROUPS, Opt, Sl, yes};

// ------------------------------------------------------------------ tokens (DESIGN B1)
const fn hex(h: u32) -> Color {
    Color::srgb(((h >> 16) & 255) as f32 / 255.0, ((h >> 8) & 255) as f32 / 255.0, (h & 255) as f32 / 255.0)
}
const fn hexa(h: u32, a: f32) -> Color {
    Color::srgba(((h >> 16) & 255) as f32 / 255.0, ((h >> 8) & 255) as f32 / 255.0, (h & 255) as f32 / 255.0, a)
}
/// Panel background.
const CLOTH: Color = hexa(0x1C1A5C, 0.92);
/// Tooltip, slider track, swatch well, name plate.
const DEEP: Color = hexa(0x141244, 0.94);
/// Button at rest.
const TABLET: Color = hex(0x2A2878);
/// Hover and active fill.
const LIT: Color = hex(0x3A38A0);
/// 80 ms press flash.
const PRESS: Color = hex(0x221F66);
/// 1-px borders, dividers, time ticks.
const STITCH: Color = hex(0x5A56C8);
/// Drip shadow under panels, swatch inner border.
const WEFT: Color = hexa(0x0A0828, 0.7);
/// Primary text, slider knob.
const SHELL: Color = hex(0xFFF4DE);
/// Key tags, group names, glyphs at rest, readouts.
const DIM: Color = hex(0xB8B0E8);
/// Selected, on or valid.
const MALACHITE: Color = hex(0x2FE0A0);
/// Attention, invalid or erase.
const MADDER: Color = hex(0xFF5A4E);
/// Time, plus the decorative thread.
const SAFFRON: Color = hex(0xFFC21A);
/// Selvedge second thread and the "now showing" auto-weather band.
const COCHINEAL: Color = hex(0xE8399A);
/// Tide gauge fill only.
const TIDE: Color = hex(0x19E0D0);
const DIS_TXT: Color = hex(0x6A6299);
const DIS_BG: Color = hex(0x241D5A);
const VEIL: Color = hexa(0x241D5A, 0.72);

/// Swatch palettes by slot (0 roof, 1 wall, 2 trim); tables live in core.
fn palette(slot: u8) -> &'static [(&'static str, u32)] {
    use cairn_cove_core::palette::{ROOF_SWATCHES, TRIM_SWATCHES, WALL_SWATCHES};
    match slot {
        0 => &ROOF_SWATCHES,
        1 => &WALL_SWATCHES,
        _ => &TRIM_SWATCHES,
    }
}

fn color_get(c: &Colors, slot: u8) -> u8 {
    [c.roof, c.wall, c.trim][slot as usize]
}
fn color_set(c: &mut Colors, slot: u8, v: u8) {
    match slot {
        0 => c.roof = v,
        1 => c.wall = v,
        _ => c.trim = v,
    }
}

// ------------------------------------------------------------------ components / resources
#[derive(Component, Clone, Copy)]
enum Action {
    Set(fn(&mut ToolSettings)),
    Tool(Tool),
    Clutter(ClutterKind),
    Color(u8, u8),
    Weather(WeatherKind),
    Auto,
    Pause,
    Undo,
    Redo,
    Hints,
    Collapse,
}
/// Selected / enabled predicates for `Action::Set` and swatch groups.
#[derive(Component)]
struct Cond {
    sel: fn(&ToolSettings) -> bool,
    en: fn(&ToolSettings) -> bool,
    /// Hidden (`Display::None`, and never enabled) while false.
    show: fn(&ToolSettings) -> bool,
}
/// The toolbar panel and its tool buttons; `toolbar_density` resizes them under 1180 logical px.
#[derive(Component)]
struct ToolBar;
#[derive(Component)]
struct ToolBtn;
/// Derived button state. `now` is the "now showing" marker of the auto weather; `bad` is the
/// invalid treatment (the active tool while its placement is invalid).
#[derive(Component, Default, PartialEq, Clone, Copy)]
struct Btn {
    sel: bool,
    en: bool,
    now: bool,
    bad: bool,
}
#[derive(Component)]
struct Flash(f32);
/// Decoration child of a button, shown by `paint` according to the button's state.
#[derive(Component, Clone, Copy, PartialEq)]
enum Band {
    /// 3-px band across the top: selected (malachite) or "now showing" (cochineal).
    Top,
    /// Row of three 4x3 madder squares: the invalid treatment, a broken thread.
    Notch,
    /// Filled square of a toggle, or the swatch under-band.
    Fill,
    /// Dimming overlay on a disabled swatch.
    Veil,
}
#[derive(Component)]
struct Lbl;
/// Tool glyph text (also `Lbl`): painted separately from the label.
#[derive(Component)]
struct Glyph;
#[derive(Component)]
struct Tog;
/// Swatch hit node: transparent, only its border shows the selection.
#[derive(Component)]
struct Chip;
#[derive(Component)]
struct Panel;
/// Name plate text; the live text (BEACON, DYE: HUT) is set by `plate`.
#[derive(Component)]
struct Plate;
#[derive(Component)]
struct PlateBand;
/// Segmented-row membership for Tab / Shift+Tab: (row, index).
#[derive(Component)]
struct Seg(u8, usize);
#[derive(Component)]
struct Tip(String, String);
#[derive(Component)]
struct Slider {
    sl: Sl,
    fill: Entity,
    knob: Entity,
    text: Entity,
}
#[derive(Component)]
struct TimeTrack;

#[derive(Resource)]
struct HintsVisible(bool);
#[derive(Resource)]
struct ClockOpen(bool);
#[derive(Resource, Default)]
struct LastSlider(Option<Entity>);
#[derive(Resource)]
struct Ids {
    clock: Entity,
    phase: Entity,
    pause: Entity,
    status: Entity,
    invalid: Entity,
    invalid_text: Entity,
    time_fill: Entity,
    time_knob: Entity,
    tide_fill: Entity,
    tide_text: Entity,
    weather_body: Entity,
    hints_full: Entity,
    hints_tab: Entity,
    hint_list: Entity,
    strip_root: Entity,
    strip_body: Entity,
    tip: Entity,
    tip_title: Entity,
    tip_sub: Entity,
}

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(HintsVisible(true))
            .insert_resource(ClockOpen(true))
            .init_resource::<LastSlider>()
            .add_systems(Startup, spawn_hud)
            .add_systems(PreUpdate, pointer_over.after(UiSystems::Focus))
            .add_systems(Update, (ui_scale, toolbar_density, rebuild_strip, rebuild_hints, keys, click, sliders, sync_btn, show_opts, paint, sync_text, plate, tooltip).chain());
    }
}

// ------------------------------------------------------------------ spawn helpers
fn txt(s: impl Into<String>, size: f32, c: Color) -> impl Bundle {
    (Text::new(s), TextFont::from_font_size(size), TextColor(c), LineHeight::RelativeToFont(1.25))
}
fn abs() -> Node {
    Node { position_type: PositionType::Absolute, ..default() }
}
fn slab(node: Node, bg: Color) -> impl Bundle {
    (Node { border: UiRect::all(px(1)), ..node }, BackgroundColor(bg), BorderColor::all(STITCH), RelativeCursorPosition::default(), Panel)
}
/// Panel decoration: selvedge (saffron + cochineal thread), two tie knots and the drip.
fn deco(p: &mut ChildSpawnerCommands) {
    p.spawn((Node { left: px(0), right: px(0), top: px(0), height: px(2), ..abs() }, BackgroundColor(SAFFRON)));
    p.spawn((Node { left: px(0), right: px(0), top: px(3), height: px(1), ..abs() }, BackgroundColor(COCHINEAL)));
    for left in [true, false] {
        let (l, r) = if left { (px(6), auto()) } else { (auto(), px(6)) };
        p.spawn((Node { left: l, right: r, top: px(4), width: px(4), height: px(4), ..abs() }, BackgroundColor(SAFFRON)));
    }
    p.spawn((Node { left: px(0), right: px(0), bottom: px(-3), height: px(2), ..abs() }, BackgroundColor(WEFT)));
}
fn button(mut node: Node, a: Action) -> impl Bundle {
    if node.border == UiRect::default() {
        node.border = UiRect::all(px(1));
    }
    // Centre the content unless the caller laid it out already (tool buttons stack key / glyph / label).
    if node.justify_content == JustifyContent::default() {
        node.justify_content = JustifyContent::Center;
    }
    if node.align_items == AlignItems::default() {
        node.align_items = AlignItems::Center;
    }
    (Node { border_radius: BorderRadius::all(px(2)), ..node }, BackgroundColor(TABLET), BorderColor::all(STITCH), Interaction::None, Btn::default(), Flash(0.0), a)
}
fn band(kind: Band, c: Color, n: Node) -> impl Bundle {
    (Node { position_type: PositionType::Absolute, ..n }, BackgroundColor(c), Visibility::Hidden, kind)
}
fn top_band() -> impl Bundle {
    band(Band::Top, MALACHITE, Node { left: px(0), right: px(0), top: px(0), height: px(3), ..default() })
}
/// Invalid notch: three 4x3 madder squares, 4 px apart, across the top-left.
fn notch(p: &mut ChildSpawnerCommands) {
    p.spawn((Node { left: px(3), top: px(0), column_gap: px(4), ..abs() }, BackgroundColor(Color::NONE), Visibility::Hidden, Band::Notch)).with_children(|n| {
        for _ in 0..3 {
            n.spawn((Node { width: px(4), height: px(3), ..default() }, BackgroundColor(MADDER)));
        }
    });
}
fn divider() -> impl Bundle {
    (Node { width: px(1), align_self: AlignSelf::Stretch, margin: UiRect::vertical(px(6)), flex_shrink: 0.0, ..default() }, BackgroundColor(STITCH))
}
fn row(gap: i32) -> Node {
    Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: px(gap), ..default() }
}
fn col(gap: i32) -> Node {
    Node { flex_direction: FlexDirection::Column, row_gap: px(gap), ..default() }
}

/// Slider track (hit node, bar, fill, knob). Returns (track, fill, knob).
fn track(g: &mut ChildSpawnerCommands, w: Val, h: f32, fill_c: Color) -> (Entity, Entity, Entity) {
    let mut ids = [Entity::PLACEHOLDER; 2];
    let t = g
        .spawn((Node { width: w, height: px(h), flex_shrink: 0.0, ..default() }, Interaction::None, RelativeCursorPosition::default()))
        .with_children(|t| {
            let top = px((h - 4.0) / 2.0);
            t.spawn((Node { left: px(0), right: px(0), top, height: px(4), border: UiRect::all(px(1)), ..abs() }, BackgroundColor(DEEP), BorderColor::all(STITCH)));
            ids[0] = t.spawn((Node { left: px(0), top, height: px(4), width: px(0), ..abs() }, BackgroundColor(fill_c))).id();
            ids[1] = t
                .spawn((
                    Node { left: px(0), top: px((h - 16.0) / 2.0), width: px(8), height: px(16), margin: UiRect::left(px(-4)), border_radius: BorderRadius::all(px(2)), ..abs() },
                    BackgroundColor(SHELL),
                ))
                .id();
        })
        .id();
    (t, ids[0], ids[1])
}

/// Toolbar metrics (DESIGN B4): 58-px buttons with 8-px group gaps; under 1180 logical px of
/// width, 52 and 6.
const BTN_W: f32 = 58.0;
const BTN_W_NARROW: f32 = 52.0;
const BAR_GAP: f32 = 8.0;
const BAR_GAP_NARROW: f32 = 6.0;
const NARROW_BELOW: f32 = 1180.0;

/// (button width, group gap) for a window `logical_w` UI pixels wide.
fn toolbar_metrics(logical_w: f32) -> (f32, f32) {
    if logical_w < NARROW_BELOW { (BTN_W_NARROW, BAR_GAP_NARROW) } else { (BTN_W, BAR_GAP) }
}

fn spawn_hud(mut commands: Commands) {
    let none = Entity::PLACEHOLDER;
    let mut ids = Ids {
        clock: none, phase: none, pause: none, status: none, invalid: none, invalid_text: none, time_fill: none, time_knob: none, tide_fill: none, tide_text: none, weather_body: none,
        hints_full: none, hints_tab: none, hint_list: none, strip_root: none, strip_body: none, tip: none, tip_title: none, tip_sub: none,
    };

    // ---- ToolBar
    commands.spawn(Node { left: px(0), right: px(0), bottom: px(16), justify_content: JustifyContent::Center, padding: UiRect::horizontal(px(16)), ..abs() }).with_children(|w| {
        w.spawn((slab(Node { padding: UiRect::all(px(6)), column_gap: px(BAR_GAP), max_width: percent(100), ..default() }, CLOTH), ToolBar)).with_children(|bar| {
            deco(bar);
            for (gi, name) in GROUPS.iter().enumerate() {
                if gi > 0 {
                    bar.spawn(divider());
                }
                bar.spawn(col(0)).with_children(|g| {
                    g.spawn((txt(*name, 10.0, DIM), Node { height: px(14), ..default() }));
                    g.spawn(row(2)).with_children(|r| {
                        for t in Tool::ALL.into_iter().filter(|t| strip::tool_group(*t) == gi) {
                            let key = t.hotkey().1;
                            let (title, sub) = strip::tip(t);
                            r.spawn((
                                button(
                                    Node {
                                        width: px(BTN_W),
                                        min_width: px(BTN_W_NARROW),
                                        height: px(56),
                                        flex_shrink: 1.0,
                                        flex_direction: FlexDirection::Column,
                                        justify_content: JustifyContent::FlexEnd,
                                        row_gap: px(2),
                                        padding: UiRect::bottom(px(4)),
                                        ..default()
                                    },
                                    Action::Tool(t),
                                ),
                                Tip(format!("{title}  [{key}]"), sub.into()),
                                ToolBtn,
                            ))
                            .with_children(|b| {
                                b.spawn(top_band());
                                notch(b);
                                b.spawn((txt(key, 10.0, DIM), Node { left: px(3), top: px(1), ..abs() }));
                                b.spawn(Node { left: px(0), right: px(0), top: px(14), justify_content: JustifyContent::Center, ..abs() }).with_children(|gl| {
                                    gl.spawn((txt(t.glyph(), 14.0, DIM), Lbl, Glyph));
                                });
                                b.spawn((txt(t.label(), 12.0, SHELL), Lbl, TextLayout { linebreak: LineBreak::NoWrap, ..default() }));
                            });
                        }
                    });
                });
            }
        });
    });

    // ---- VariantStrip (content rebuilt by `rebuild_strip`)
    commands.spawn(Node { left: px(0), right: px(0), bottom: px(110), justify_content: JustifyContent::Center, padding: UiRect::horizontal(px(16)), ..abs() }).with_children(|w| {
        ids.strip_root = w
            .spawn(slab(Node { padding: UiRect::all(px(8)), max_width: px(1000), display: Display::None, ..default() }, CLOTH))
            .with_children(|p| {
                deco(p);
                ids.strip_body = p.spawn(Node { margin: UiRect::top(px(4)), ..col(4) }).id();
            })
            .id();
    });

    // ---- WeatherPanel
    commands.spawn(Node { right: px(16), top: px(16), ..abs() }).with_children(|w| {
        w.spawn(slab(Node { width: px(244), padding: UiRect::all(px(8)), flex_direction: FlexDirection::Column, row_gap: px(4), ..default() }, CLOTH)).with_children(|p| {
            deco(p);
            p.spawn(Node { margin: UiRect::top(px(4)), ..row(8) }).with_children(|r| {
                r.spawn((Node { flex_shrink: 0.0, ..default() }, Interaction::None, Action::Collapse)).with_children(|c| {
                    ids.clock = c.spawn(txt("12:00", 20.0, SAFFRON)).id();
                });
                ids.phase = r.spawn(txt("DAY", 10.0, DIM)).id();
                r.spawn((button(Node { width: px(32), height: px(32), margin: UiRect::left(Val::Auto), ..default() }, Action::Pause), Tip("PAUSE TIME  [T]".into(), "Freeze or resume the clock".into()))).with_children(|b| {
                    b.spawn(top_band());
                    ids.pause = b.spawn((txt("||", 12.0, SHELL), Lbl)).id();
                });
            });
            ids.weather_body = p
                .spawn(col(4))
                .with_children(|b| {
                    // Time slider, with ticks at 06, 12, 18.
                    b.spawn(col(0)).with_children(|t| {
                        let (tr, fill, knob) = track(t, px(228), 20.0, SAFFRON);
                        t.commands().entity(tr).insert(TimeTrack);
                        ids.time_fill = fill;
                        ids.time_knob = knob;
                        t.spawn(Node { width: px(228), height: px(6), ..default() }).with_children(|k| {
                            for pct in [25.0, 50.0, 75.0] {
                                k.spawn((Node { left: percent(pct), width: px(1), height: px(6), ..abs() }, BackgroundColor(STITCH)));
                            }
                        });
                        t.spawn(Node { width: px(228), justify_content: JustifyContent::SpaceBetween, ..default() }).with_children(|k| {
                            k.spawn(txt("00", 10.0, DIM));
                            k.spawn(txt("24", 10.0, DIM));
                        });
                    });
                    // Tide row (read-only, follows the clock).
                    b.spawn(Node { width: px(228), height: px(16), ..row(4) }).with_children(|r| {
                        r.spawn((txt("TIDE", 10.0, DIM), Node { flex_shrink: 0.0, ..default() }));
                        r.spawn((Node { width: px(120), min_width: px(70), height: px(6), flex_shrink: 1.0, border: UiRect::all(px(1)), ..default() }, BackgroundColor(DEEP), BorderColor::all(STITCH))).with_children(|g| {
                            ids.tide_fill = g.spawn((Node { left: px(0), top: px(0), bottom: px(0), width: px(0), ..abs() }, BackgroundColor(TIDE))).id();
                            g.spawn((Node { left: percent(50.0), top: px(-3), width: px(1), height: px(10), ..abs() }, BackgroundColor(SHELL)));
                        });
                        ids.tide_text = r.spawn((txt("", 10.0, DIM), Node { flex_shrink: 0.0, margin: UiRect::left(Val::Auto), ..default() })).id();
                    });
                    // Weather grid: 74x32, three per row.
                    b.spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: px(2), row_gap: px(2), ..default() }).with_children(|g| {
                        for k in WeatherKind::ALL {
                            let name = if k == WeatherKind::Fog { "SEA FOG".to_string() } else { k.name().to_uppercase() };
                            g.spawn(button(Node { width: px(74), height: px(32), ..default() }, Action::Weather(k))).with_children(|b| {
                                b.spawn(top_band());
                                b.spawn((txt(name, 12.0, SHELL), Lbl));
                            });
                        }
                    });
                    b.spawn(Node { justify_content: JustifyContent::SpaceBetween, ..row(6) }).with_children(|r| {
                        r.spawn((button(Node { min_height: px(32), padding: UiRect::horizontal(px(8)), column_gap: px(6), ..default() }, Action::Auto), Tog)).with_children(|t| {
                            toggle_box(t);
                            t.spawn((txt("AUTO CYCLE", 12.0, SHELL), Lbl));
                        });
                        ids.status = r.spawn(txt("CYCLING", 10.0, DIM)).id();
                    });
                    b.spawn(txt("T PAUSE   [ ] HOUR", 10.0, DIM));
                })
                .id();
        });
    });

    // ---- UndoRedo
    commands.spawn(Node { left: px(16), top: px(16), column_gap: px(4), ..abs() }).with_children(|r| {
        for (label, key, a) in [("< UNDO", "^Z", Action::Undo), ("REDO >", "^Y", Action::Redo)] {
            r.spawn(button(Node { width: px(72), height: px(36), ..default() }, a)).with_children(|b| {
                b.spawn((txt(label, 12.0, SHELL), Lbl));
                b.spawn((txt(key, 10.0, DIM), Node { right: px(3), top: px(1), ..abs() }));
            });
        }
    });

    // ---- HintsPanel (full + collapsed tab)
    commands.spawn(Node { left: px(16), top: px(60), ..abs() }).with_children(|w| {
        ids.hints_full = w
            .spawn(slab(Node { width: px(272), padding: UiRect::all(px(8)), flex_direction: FlexDirection::Column, row_gap: px(4), ..default() }, CLOTH))
            .with_children(|p| {
                deco(p);
                p.spawn((button(Node { right: px(0), top: px(0), width: px(32), height: px(32), ..abs() }, Action::Hints), Tip("HINTS  [H]".into(), "Collapse the hints".into()))).with_children(|b| {
                    b.spawn((txt("x", 12.0, SHELL), Lbl));
                });
                // Invalid reason: madder text with the broken-thread notch at its left edge.
                ids.invalid = p
                    .spawn(Node { display: Display::None, margin: UiRect::top(px(4)), ..row(6) })
                    .with_children(|r| {
                        r.spawn(row(4)).with_children(|n| {
                            for _ in 0..3 {
                                n.spawn((Node { width: px(4), height: px(3), ..default() }, BackgroundColor(MADDER)));
                            }
                        });
                        ids.invalid_text = r.spawn(txt("", 12.0, MADDER)).id();
                    })
                    .id();
                ids.hint_list = p.spawn(Node { margin: UiRect::top(px(4)), ..col(4) }).id();
            })
            .id();
        ids.hints_tab = w
            .spawn((button(Node { width: px(32), height: px(32), display: Display::None, ..default() }, Action::Hints), Tip("HINTS  [H]".into(), "Show the hints".into())))
            .with_children(|b| {
                b.spawn((txt("?", 12.0, SHELL), Lbl));
            })
            .id();
    });

    // ---- Tooltip
    ids.tip = commands
        .spawn((
            Node { max_width: px(260), padding: UiRect::all(px(6)), border: UiRect::all(px(1)), flex_direction: FlexDirection::Column, display: Display::None, ..abs() },
            BackgroundColor(DEEP),
            BorderColor::all(STITCH),
            GlobalZIndex(1000),
        ))
        .with_children(|t| {
            ids.tip_title = t.spawn(txt("", 14.0, SHELL)).id();
            ids.tip_sub = t.spawn(txt("", 12.0, DIM)).id();
        })
        .id();

    commands.insert_resource(ids);
}

/// 12x12 box; on, it is filled malachite with a 4x4 cloth square inside.
fn toggle_box(t: &mut ChildSpawnerCommands) {
    t.spawn((Node { width: px(12), height: px(12), border: UiRect::all(px(1)), flex_shrink: 0.0, ..default() }, BorderColor::all(DIM))).with_children(|b| {
        b.spawn((
            Node { left: px(0), right: px(0), top: px(0), bottom: px(0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..abs() },
            BackgroundColor(MALACHITE),
            Visibility::Hidden,
            Band::Fill,
        ))
        .with_children(|f| {
            f.spawn((Node { width: px(4), height: px(4), ..default() }, BackgroundColor(CLOTH)));
        });
    });
}

// ------------------------------------------------------------------ strip
fn seg_button(g: &mut ChildSpawnerCommands, o: &Opt, a: Action, cond: Option<Cond>, seg: Seg) {
    let mut e = g.spawn((button(Node { min_height: px(32), padding: UiRect::horizontal(px(8)), ..default() }, a), seg));
    if let Some(c) = cond {
        e.insert(c);
    }
    e.with_children(|b| {
        b.spawn(top_band());
        b.spawn((txt(o.l, 12.0, SHELL), Lbl));
    });
}

/// The name plate: 14-px shell text on a deep well with a 3-px band down its left edge.
fn name_plate(p: &mut ChildSpawnerCommands, text: &str) {
    p.spawn((Node { padding: UiRect { left: px(12), right: px(8), top: px(6), bottom: px(6) }, align_items: AlignItems::Center, ..default() }, BackgroundColor(DEEP))).with_children(|n| {
        n.spawn((Node { left: px(0), top: px(0), bottom: px(0), width: px(3), ..abs() }, BackgroundColor(MALACHITE), PlateBand));
        n.spawn((txt(text, 14.0, SHELL), Plate));
    });
}

fn fill_strip(p: &mut ChildSpawnerCommands, tool: Tool) {
    let (a, b) = strip::rows(tool);
    let mut seg_row = 0u8;
    let mut first = true;
    for ctls in [a, b] {
        if ctls.is_empty() {
            continue;
        }
        p.spawn(Node { flex_wrap: FlexWrap::Wrap, row_gap: px(4), ..row(6) }).with_children(|r| {
            if first {
                name_plate(r, strip::plate(tool));
            }
            for (k, c) in ctls.iter().enumerate() {
                if first || k > 0 {
                    r.spawn(divider());
                }
                cluster(r, c, &mut seg_row);
            }
        });
        first = false;
    }
}

fn cluster(r: &mut ChildSpawnerCommands, c: &C, seg_row: &mut u8) {
    match c {
        C::Seg(label, opts) => {
            let grp = *seg_row;
            *seg_row += 1;
            r.spawn(row(2)).with_children(|g| {
                if !label.is_empty() {
                    g.spawn((txt(*label, 10.0, DIM), Node { margin: UiRect::right(px(4)), ..default() }));
                }
                for (i, o) in opts.iter().enumerate() {
                    seg_button(g, o, Action::Set(o.set), Some(Cond { sel: o.sel, en: o.en, show: o.show }), Seg(grp, i));
                }
            });
        }
        C::Tog(o) => {
            r.spawn((button(Node { min_height: px(32), padding: UiRect::horizontal(px(8)), column_gap: px(6), ..default() }, Action::Set(o.set)), Cond { sel: o.sel, en: o.en, show: o.show }, Tog)).with_children(|t| {
                toggle_box(t);
                t.spawn((txt(o.l, 12.0, SHELL), Lbl));
            });
        }
        C::Sld(sl) => {
            r.spawn(Node { min_height: px(32), ..row(6) }).with_children(|g| {
                g.spawn(txt(sl.l, 12.0, SHELL));
                let (tr, fill, knob) = track(g, px(96), 32.0, MALACHITE);
                let text = g.spawn(txt("", 10.0, DIM)).id();
                g.commands().entity(tr).insert(Slider { sl: *sl, fill, knob, text });
            });
        }
        C::Sw(slot, label, en) => {
            r.spawn(row(0)).with_children(|g| {
                g.spawn((txt(*label, 10.0, DIM), Node { margin: UiRect::right(px(4)), ..default() }));
                for (i, (name, h)) in palette(*slot).iter().enumerate() {
                    // 32x32 hit node; 28x28 deep cell; 1-px WEFT border round a 20x20 chip: 13 px lit hex over 7 px noon-shadow hex.
                    g.spawn((
                        button(Node { width: px(32), height: px(32), border: UiRect::all(px(2)), ..default() }, Action::Color(*slot, i as u8)),
                        Cond { sel: yes, en: *en, show: yes },
                        Tip(name.to_uppercase(), format!("{} swatch", label.to_lowercase())),
                        Chip,
                    ))
                    .with_children(|b| {
                        b.spawn((Node { width: px(28), height: px(28), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() }, BackgroundColor(DEEP))).with_children(|cell| {
                            cell.spawn((Node { width: px(22), height: px(22), padding: UiRect::all(px(1)), flex_direction: FlexDirection::Column, ..default() }, BackgroundColor(WEFT))).with_children(|chip| {
                                chip.spawn((Node { height: px(13), ..default() }, BackgroundColor(hex(*h))));
                                chip.spawn((Node { height: px(7), ..default() }, BackgroundColor(hex(strip::shadow(*slot, i)))));
                            });
                        });
                        b.spawn(band(Band::Fill, MALACHITE, Node { left: px(2), right: px(2), bottom: px(-5), height: px(3), ..default() }));
                        b.spawn(band(Band::Veil, VEIL, Node { left: px(2), right: px(2), top: px(2), bottom: px(2), ..default() }));
                    });
                }
            });
        }
        C::Clutter => {
            r.spawn(Node { width: px(7.0 * 72.0 + 6.0 * 2.0), flex_wrap: FlexWrap::Wrap, column_gap: px(2), row_gap: px(2), ..default() }).with_children(|g| {
                for (i, (k, l, full)) in CLUTTER.iter().enumerate() {
                    g.spawn((button(Node { width: px(72), height: px(36), ..default() }, Action::Clutter(*k)), Seg(0, i), Tip((*full).into(), "Click to place".into()))).with_children(|b| {
                        b.spawn(top_band());
                        b.spawn((txt(*l, 12.0, SHELL), Lbl));
                    });
                }
            });
        }
    }
}

fn rebuild_strip(mut commands: Commands, s: Res<ToolSettings>, ids: Res<Ids>, mut cur: Local<Option<Tool>>, mut nodes: Query<&mut Node>) {
    if *cur == Some(s.tool) {
        return;
    }
    *cur = Some(s.tool);
    if let Ok(mut n) = nodes.get_mut(ids.strip_root) {
        n.display = if s.tool == Tool::Erase { Display::None } else { Display::Flex };
    }
    let tool = s.tool;
    commands.entity(ids.strip_body).despawn_children().with_children(|p| fill_strip(p, tool));
}

fn rebuild_hints(mut commands: Commands, s: Res<ToolSettings>, status: Res<ToolStatus>, ids: Res<Ids>, mut cur: Local<Option<(Tool, bool, bool)>>) {
    let key = (s.tool, status.drawing, status.raising);
    if *cur == Some(key) {
        return;
    }
    *cur = Some(key);
    let tool = s.tool;
    let lines: Vec<_> = if status.raising {
        strip::raising_hints(tool).to_vec()
    } else {
        strip::hints(tool).iter().copied().filter(|(k, _)| !(status.drawing && *k == "DRAG")).collect()
    };
    commands.entity(ids.hint_list).despawn_children().with_children(|p| {
        p.spawn((txt(strip::plate(tool), 12.0, SHELL), Plate));
        for (key, desc) in lines {
            p.spawn(row(6)).with_children(|r| {
                r.spawn((Node { padding: UiRect::axes(px(4), px(2)), flex_shrink: 0.0, ..default() }, BackgroundColor(DEEP))).with_children(|c| {
                    c.spawn(txt(key, 10.0, SHELL));
                });
                r.spawn((txt(desc, 12.0, DIM), Node { flex_basis: px(0), flex_grow: 1.0, ..default() }));
            });
        }
    });
}

// ------------------------------------------------------------------ systems
fn ui_scale(win: Query<&Window, With<PrimaryWindow>>, mut scale: ResMut<UiScale>) {
    let Ok(w) = win.single() else { return };
    let s = ((w.height() / 720.0 * 4.0).round() / 4.0).min((w.width() / 1200.0 * 4.0).floor() / 4.0).clamp(0.75, 2.0);
    if scale.0 != s {
        scale.0 = s;
    }
}

/// Under 1180 logical px of width the toolbar narrows its buttons to 52 and its group gaps to 6.
fn toolbar_density(
    win: Query<&Window, With<PrimaryWindow>>,
    scale: Res<UiScale>,
    mut bar: Query<&mut Node, (With<ToolBar>, Without<ToolBtn>)>,
    mut btns: Query<&mut Node, With<ToolBtn>>,
) {
    let Ok(w) = win.single() else { return };
    let (bw, gap) = toolbar_metrics(w.width() / scale.0.max(0.01));
    for mut n in &mut bar {
        if n.column_gap != px(gap) {
            n.column_gap = px(gap);
        }
    }
    for mut n in &mut btns {
        if n.width != px(bw) {
            n.width = px(bw);
        }
    }
}

/// Options whose `show` predicate is false (the roof choices a beacon does not have) are hidden.
fn show_opts(s: Res<ToolSettings>, mut q: Query<(&Cond, &mut Node)>) {
    for (c, mut n) in &mut q {
        let want = if (c.show)(&s) { Display::Flex } else { Display::None };
        if n.display != want {
            n.display = want;
        }
    }
}

fn pointer_over(panels: Query<&RelativeCursorPosition, With<Panel>>, ints: Query<&Interaction>, mut over: ResMut<PointerOverUi>) {
    let v = panels.iter().any(|r| r.cursor_over) || ints.iter().any(|i| *i != Interaction::None);
    if over.0 != v {
        over.0 = v;
    }
}

fn apply(a: Action, s: &mut ToolSettings) {
    match a {
        Action::Set(f) => f(s),
        Action::Clutter(k) => s.clutter = k,
        _ => {}
    }
}

fn keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut hv: ResMut<HintsVisible>,
    mut s: ResMut<ToolSettings>,
    last: Res<LastSlider>,
    sliders: Query<&Slider>,
    segs: Query<(&Seg, &Action, &Btn)>,
) {
    if keys.just_pressed(KeyCode::KeyH) {
        hv.0 = !hv.0;
    }
    if keys.just_pressed(KeyCode::Tab) {
        let row = if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) { 1 } else { 0 };
        let mut items: Vec<_> = segs.iter().filter(|(g, ..)| g.0 == row).collect();
        items.sort_by_key(|(g, ..)| g.1);
        if let Some(cur) = items.iter().position(|(_, _, b)| b.sel) {
            let n = items.len();
            if let Some((_, a, _)) = (1..n).map(|d| &items[(cur + d) % n]).find(|(_, _, b)| b.en) {
                apply(**a, &mut s);
            }
        } else if let Some((_, a, _)) = items.iter().find(|(_, _, b)| b.en) {
            apply(**a, &mut s);
        }
    }
    let dir = keys.just_pressed(KeyCode::Equal) as i32 - keys.just_pressed(KeyCode::Minus) as i32;
    if dir != 0
        && let Some(Ok(sl)) = last.0.map(|e| sliders.get(e))
    {
        let sl = &sl.sl;
        let v = ((sl.get)(&s) + dir as f32 * sl.step).clamp(sl.lo, sl.hi);
        (sl.set)(&mut s, v);
    }
}

fn click(
    q: Query<(&Action, Option<&Btn>, &Interaction), Changed<Interaction>>,
    mut s: ResMut<ToolSettings>,
    mut w: ResMut<Weather>,
    mut clock: ResMut<Clock>,
    mut glade: ResMut<Glade>,
    mut hv: ResMut<HintsVisible>,
    mut open: ResMut<ClockOpen>,
) {
    for (a, b, i) in &q {
        if *i != Interaction::Pressed || !b.is_none_or(|b| b.en) {
            continue;
        }
        match *a {
            Action::Set(_) | Action::Clutter(_) => apply(*a, &mut s),
            Action::Tool(t) => {
                if s.tool != t {
                    s.tool = t;
                }
            }
            Action::Color(slot, idx) => color_set(&mut s.colors, slot, idx),
            Action::Weather(k) => {
                w.kind = k;
                w.auto = false;
            }
            Action::Auto => w.auto = !w.auto,
            Action::Pause => clock.paused = !clock.paused,
            Action::Undo => {
                glade.undo();
            }
            Action::Redo => {
                glade.redo();
            }
            Action::Hints => hv.0 = !hv.0,
            Action::Collapse => open.0 = !open.0,
        }
    }
}

fn frac(rel: &RelativeCursorPosition) -> Option<f32> {
    rel.normalized.map(|n| (n.x + 0.5).clamp(0.0, 1.0))
}

fn sliders(
    q: Query<(Entity, &Slider, &Interaction, &RelativeCursorPosition)>,
    time: Query<(&Interaction, &RelativeCursorPosition), With<TimeTrack>>,
    mut s: ResMut<ToolSettings>,
    mut last: ResMut<LastSlider>,
    mut clock: ResMut<Clock>,
) {
    for (e, sl, i, rel) in &q {
        let (true, Some(f)) = (*i == Interaction::Pressed, frac(rel)) else { continue };
        let sl = &sl.sl;
        let v = (sl.lo + f * (sl.hi - sl.lo)).clamp(sl.lo, sl.hi);
        let v = (((v - sl.lo) / sl.step).round() * sl.step + sl.lo).clamp(sl.lo, sl.hi);
        if ((sl.get)(&s) - v).abs() > 1e-4 {
            (sl.set)(&mut s, v);
        }
        last.0 = Some(e);
    }
    for (i, rel) in &time {
        if let (Interaction::Pressed, Some(f)) = (*i, frac(rel)) {
            clock.hour = (f * 24.0).min(23.99);
            clock.paused = true;
        }
    }
}

fn sync_btn(s: Res<ToolSettings>, status: Res<ToolStatus>, w: Res<Weather>, clock: Res<Clock>, glade: Res<Glade>, mut q: Query<(&Action, Option<&Cond>, &mut Btn)>) {
    for (a, c, mut b) in &mut q {
        let (sel, en, now) = match *a {
            Action::Set(_) => c.map_or((false, true, false), |c| ((c.sel)(&s), (c.en)(&s) && (c.show)(&s), false)),
            // The toolbar never dims while drawing; picking another tool cancels the gesture.
            Action::Tool(t) => (s.tool == t, true, false),
            Action::Clutter(k) => (s.clutter == k, true, false),
            Action::Color(slot, i) => (color_get(&s.colors, slot) == i, c.is_none_or(|c| (c.en)(&s)), false),
            Action::Weather(k) => (!w.auto && w.kind == k, true, w.auto && w.kind == k),
            Action::Auto => (w.auto, true, false),
            Action::Pause => (clock.paused, true, false),
            Action::Undo => (false, glade.can_undo(), false),
            Action::Redo => (false, glade.can_redo(), false),
            Action::Hints | Action::Collapse => (false, true, false),
        };
        let bad = matches!(*a, Action::Tool(t) if s.tool == t) && status.invalid.is_some();
        let n = Btn { sel, en, now, bad };
        if *b != n {
            *b = n;
        }
    }
}

fn paint(
    time: Res<Time>,
    mut q: Query<(Entity, &Btn, &Interaction, &mut Flash, &mut BackgroundColor, &mut BorderColor, &Action, Has<Tog>, Has<Chip>), Without<Band>>,
    kids: Query<&Children>,
    mut deco: Query<(&Band, &mut Visibility, &mut BackgroundColor), Without<Btn>>,
    mut tcol: Query<(&mut TextColor, Has<Glyph>), With<Lbl>>,
) {
    for (e, b, i, mut fl, mut bg, mut bd, a, tog, chip) in &mut q {
        fl.0 = if *i == Interaction::Pressed { 0.08 } else { (fl.0 - time.delta_secs()).max(0.0) };
        let hot = *i != Interaction::None && b.en;
        let erase_hover = hot && matches!(*a, Action::Tool(Tool::Erase));
        let edge = if b.bad || erase_hover {
            MADDER
        } else if b.sel && !tog {
            MALACHITE
        } else if chip {
            Color::NONE
        } else {
            STITCH
        };
        let fill = if chip {
            Color::NONE
        } else if !b.en {
            DIS_BG
        } else if fl.0 > 0.0 {
            PRESS
        } else if (b.sel && !tog) || hot {
            LIT
        } else {
            TABLET
        };
        let edge = if !b.en && !chip { STITCH } else { edge };
        if bg.0 != fill {
            bg.0 = fill;
        }
        let edge = BorderColor::all(edge);
        if *bd != edge {
            *bd = edge;
        }
        let label = if b.en { SHELL } else { DIS_TXT };
        let glyph = if !b.en {
            DIS_TXT
        } else if b.sel {
            MALACHITE
        } else if hot {
            SHELL
        } else {
            DIM
        };
        for d in kids.iter_descendants(e) {
            if let Ok((kind, mut v, mut c)) = deco.get_mut(d) {
                let show = match kind {
                    Band::Top => (b.sel || b.now) && !b.bad,
                    Band::Notch => b.bad,
                    Band::Fill => b.sel,
                    Band::Veil => !b.en,
                };
                v.set_if_neq(if show { Visibility::Inherited } else { Visibility::Hidden });
                if matches!(kind, Band::Top) {
                    let want = if b.now && !b.sel { COCHINEAL } else { MALACHITE };
                    if c.0 != want {
                        c.0 = want;
                    }
                }
            }
            if let Ok((mut t, is_glyph)) = tcol.get_mut(d) {
                let want = if is_glyph { glyph } else { label };
                if t.0 != want {
                    t.0 = want;
                }
            }
        }
    }
}

fn set_text(texts: &mut Query<&mut Text>, e: Entity, s: String) {
    if let Ok(mut t) = texts.get_mut(e)
        && t.0 != s
    {
        t.0 = s;
    }
}

fn place(nodes: &mut Query<&mut Node>, fill: Entity, knob: Option<Entity>, f: f32) {
    let v = percent(f * 100.0);
    if let Ok(mut n) = nodes.get_mut(fill)
        && n.width != v
    {
        n.width = v;
    }
    if let Some(knob) = knob
        && let Ok(mut n) = nodes.get_mut(knob)
        && n.left != v
    {
        n.left = v;
    }
}

fn show(nodes: &mut Query<&mut Node>, e: Entity, on: bool) {
    if let Ok(mut n) = nodes.get_mut(e) {
        let d = if on { Display::Flex } else { Display::None };
        if n.display != d {
            n.display = d;
        }
    }
}

/// Clock phase word for the hour: DAWN 05-07, DAY 08-15, GOLDEN 16-17, DUSK 18-19, NIGHT 20-04.
fn phase_word(h: f32) -> &'static str {
    match h.rem_euclid(24.0) as u32 {
        5..=7 => "DAWN",
        8..=15 => "DAY",
        16..=17 => "GOLDEN",
        18..=19 => "DUSK",
        _ => "NIGHT",
    }
}

/// Tide readout: `RISING +0.21 M`; HIGH / LOW within 0.03 m of the extremes.
fn tide_readout(t: Tide) -> String {
    let word = if t.level >= TIDE_RANGE - 0.03 {
        "HIGH"
    } else if t.level <= -TIDE_RANGE + 0.03 {
        "LOW"
    } else if t.falling {
        "FALLING"
    } else {
        "RISING"
    };
    let level = if t.level.abs() < 0.005 { 0.0 } else { t.level };
    format!("{word} {level:+.2} M")
}

fn sync_text(
    ids: Res<Ids>,
    clock: Res<Clock>,
    weather: Res<Weather>,
    status: Res<ToolStatus>,
    hv: Res<HintsVisible>,
    open: Res<ClockOpen>,
    sl: Query<&Slider>,
    s: Res<ToolSettings>,
    mut texts: Query<&mut Text>,
    mut nodes: Query<&mut Node>,
) {
    let h = clock.hour.rem_euclid(24.0);
    set_text(&mut texts, ids.clock, format!("{:02}:{:02}", h as u32, ((h.fract() * 60.0) as u32).min(59)));
    set_text(&mut texts, ids.phase, phase_word(h).into());
    set_text(&mut texts, ids.pause, if clock.paused { ">" } else { "||" }.into());
    set_text(&mut texts, ids.status, if weather.auto { "CYCLING" } else { "HELD" }.into());
    place(&mut nodes, ids.time_fill, Some(ids.time_knob), h / 24.0);
    let tide = Tide::at(h);
    place(&mut nodes, ids.tide_fill, None, ((tide.level + TIDE_RANGE) / (2.0 * TIDE_RANGE)).clamp(0.0, 1.0));
    set_text(&mut texts, ids.tide_text, tide_readout(tide));
    show(&mut nodes, ids.weather_body, open.0);
    show(&mut nodes, ids.hints_full, hv.0);
    show(&mut nodes, ids.hints_tab, !hv.0);
    set_text(&mut texts, ids.invalid_text, format!("! {}", status.invalid.clone().unwrap_or_default()));
    show(&mut nodes, ids.invalid, status.invalid.is_some());
    for x in &sl {
        let v = (x.sl.get)(&s);
        place(&mut nodes, x.fill, Some(x.knob), ((v - x.sl.lo) / (x.sl.hi - x.sl.lo)).clamp(0.0, 1.0));
        let unit = if x.sl.unit.is_empty() { String::new() } else { format!(" {}", x.sl.unit) };
        let text = if v <= x.sl.lo && !x.sl.zero.is_empty() { x.sl.zero.to_string() } else { format!("{v:.d$}{unit}", d = x.sl.dec) };
        set_text(&mut texts, x.text, text);
    }
}

/// Name plate text and band: the tool name, with live BEACON (Roundhouse, saffron band) and
/// `DYE: HUT` (after a Shift+click pick).
fn plate(s: Res<ToolSettings>, mut texts: Query<&mut Text, With<Plate>>, mut bands: Query<&mut BackgroundColor, With<PlateBand>>) {
    let beacon = s.tool == Tool::Roundhouse && s.ghost_beacon;
    let name = match (s.tool, s.dye) {
        (Tool::Roundhouse, _) if beacon => "BEACON".to_string(),
        (Tool::Dye, Some(t)) => format!("DYE: {t}"),
        (t, _) => strip::plate(t).to_string(),
    };
    for mut t in &mut texts {
        if t.0 != name {
            t.0 = name.clone();
        }
    }
    for mut b in &mut bands {
        let want = if beacon { SAFFRON } else { MALACHITE };
        if b.0 != want {
            b.0 = want;
        }
    }
}

fn tooltip(
    time: Res<Time>,
    ids: Res<Ids>,
    scale: Res<UiScale>,
    win: Query<&Window, With<PrimaryWindow>>,
    tips: Query<(Entity, &Tip, &Interaction)>,
    mut st: Local<(Option<Entity>, f32)>,
    mut nodes: Query<&mut Node>,
    mut texts: Query<&mut Text>,
) {
    let hov = tips.iter().find(|(_, _, i)| **i == Interaction::Hovered);
    let Some((e, tip, _)) = hov else {
        st.0 = None;
        show(&mut nodes, ids.tip, false);
        return;
    };
    if st.0 != Some(e) {
        *st = (Some(e), 0.0);
        show(&mut nodes, ids.tip, false);
    }
    st.1 += time.delta_secs();
    let shown = nodes.get(ids.tip).is_ok_and(|n| n.display != Display::None);
    if st.1 < 0.45 || shown {
        return;
    }
    let (Ok(w), Some(c)) = (win.single(), win.single().ok().and_then(|w| w.cursor_position())) else { return };
    set_text(&mut texts, ids.tip_title, tip.0.clone());
    set_text(&mut texts, ids.tip_sub, tip.1.clone());
    let (ww, wh, c) = (w.width() / scale.0, w.height() / scale.0, c / scale.0);
    if let Ok(mut n) = nodes.get_mut(ids.tip) {
        n.display = Display::Flex;
        n.left = px((c.x - 20.0).clamp(4.0, (ww - 264.0).max(4.0)));
        n.bottom = px(wh - c.y + 12.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_words_follow_the_design() {
        let w = |h: f32| phase_word(h);
        assert_eq!([w(4.9), w(5.0), w(7.99), w(8.0), w(15.9), w(16.0), w(17.9), w(18.0), w(19.9), w(20.0), w(0.0)], ["NIGHT", "DAWN", "DAWN", "DAY", "DAY", "GOLDEN", "GOLDEN", "DUSK", "DUSK", "NIGHT", "NIGHT"]);
    }

    #[test]
    fn tide_readout_words_and_sign() {
        assert_eq!(tide_readout(Tide { level: 0.3, falling: true }), "HIGH +0.30 M");
        assert_eq!(tide_readout(Tide { level: -0.29, falling: false }), "LOW -0.29 M");
        assert_eq!(tide_readout(Tide { level: 0.21, falling: false }), "RISING +0.21 M");
        assert_eq!(tide_readout(Tide { level: -0.1, falling: true }), "FALLING -0.10 M");
        // The clock drives it: high water at 03:00, low at 09:00.
        assert!(tide_readout(Tide::at(3.0)).starts_with("HIGH"));
        assert!(tide_readout(Tide::at(9.0)).starts_with("LOW"));
    }

    #[test]
    fn toolbar_narrows_below_1180() {
        assert_eq!(toolbar_metrics(1280.0), (58.0, 8.0));
        assert_eq!(toolbar_metrics(1180.0), (58.0, 8.0));
        assert_eq!(toolbar_metrics(1179.0), (52.0, 6.0));
        assert_eq!(toolbar_metrics(900.0), (52.0, 6.0));
    }

    #[test]
    fn tokens_match_the_design_hexes() {
        let rgb = |c: Color| c.to_srgba().to_u8_array_no_alpha();
        assert_eq!(rgb(MALACHITE), [0x2F, 0xE0, 0xA0]);
        assert_eq!(rgb(MADDER), [0xFF, 0x5A, 0x4E]);
        assert_eq!(rgb(SAFFRON), [0xFF, 0xC2, 0x1A]);
        assert_eq!(rgb(COCHINEAL), [0xE8, 0x39, 0x9A]);
        assert_eq!(rgb(TIDE), [0x19, 0xE0, 0xD0]);
        assert_eq!(rgb(SHELL), [0xFF, 0xF4, 0xDE]);
    }
}
