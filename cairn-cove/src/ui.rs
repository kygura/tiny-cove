//! HUD (see DESIGN.md). Reads/writes `tools::ToolSettings`, reads `tools::ToolStatus`,
//! sets `tools::PointerOverUi`, drives `weather::{Clock, Weather}` and `world::Glade` undo/redo.
//!
//! Layout: static panels are spawned once in `spawn_hud`; the variant strip and the hint list are
//! rebuilt when the tool changes. Every button carries an [`Action`]; `sync_btn` derives its
//! [`Btn`] state, `paint` turns that into colours, `click` applies presses.
#![allow(deprecated)] // `Interaction` is deprecated in 0.20 but is still what drives hover/press.

mod strip;

use crate::tools::{PointerOverUi, Tool, ToolSettings, ToolStatus};
use crate::weather::{Clock, Weather, WeatherKind};
use crate::world::Glade;
use bevy::{prelude::*, text::LineHeight, ui::RelativeCursorPosition, ui::UiSystems, window::PrimaryWindow};
use cairn_cove_core::{ClutterKind, Colors};
use strip::{C, CLUTTER, GROUPS, Opt, Sl, yes};

// ------------------------------------------------------------------ tokens
const fn hex(h: u32) -> Color {
    Color::srgb(((h >> 16) & 255) as f32 / 255.0, ((h >> 8) & 255) as f32 / 255.0, (h & 255) as f32 / 255.0)
}
const fn hexa(h: u32, a: f32) -> Color {
    Color::srgba(((h >> 16) & 255) as f32 / 255.0, ((h >> 8) & 255) as f32 / 255.0, (h & 255) as f32 / 255.0, a)
}
const SLAB: Color = hexa(0x2F302D, 0.92);
const DEEP: Color = hexa(0x262724, 0.94);
const CAST: Color = hex(0x40413D);
const LIT: Color = hex(0x4E4F4A);
const PRESS: Color = hex(0x353632);
const SEAM: Color = hex(0x5A5B56);
const DRIP: Color = hexa(0x1B1C1A, 0.7);
const CHALK: Color = hex(0xE6E3DA);
const DIM: Color = hex(0xA4A29A);
const MOSS: Color = hex(0x8DB360);
const SODIUM: Color = hex(0xE8A33C);
const DIS_TXT: Color = hex(0x6E6F69);
const DIS_BG: Color = hex(0x3A3B37);

/// Swatch palettes (DESIGN 2.3), by slot (0 roof, 1 concrete, 2 accent); tables live in core.
fn palette(slot: u8) -> &'static [(&'static str, u32)] {
    use cairn_cove_core::palette::{ACCENT_SWATCHES, CONCRETE_SWATCHES, ROOF_SWATCHES};
    match slot {
        0 => &ROOF_SWATCHES,
        1 => &CONCRETE_SWATCHES,
        _ => &ACCENT_SWATCHES,
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
}
/// Derived button state. `now` is the "now showing" marker of the auto weather.
#[derive(Component, Default, PartialEq, Clone, Copy)]
struct Btn {
    sel: bool,
    en: bool,
    now: bool,
}
#[derive(Component)]
struct Flash(f32);
#[derive(Component)]
struct Band;
#[derive(Component)]
struct Lbl;
#[derive(Component)]
struct Tog;
#[derive(Component)]
struct Panel;
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
    time_fill: Entity,
    time_knob: Entity,
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
            .add_systems(Update, (ui_scale, rebuild_strip, rebuild_hints, keys, click, sliders, sync_btn, paint, sync_text, tooltip).chain());
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
    (Node { border: UiRect::all(px(1)), ..node }, BackgroundColor(bg), BorderColor::all(SEAM), RelativeCursorPosition::default(), Panel)
}
/// Drip shadow and tie-holes.
fn deco(p: &mut ChildSpawnerCommands) {
    p.spawn((Node { left: px(0), right: px(0), bottom: px(-3), height: px(2), ..abs() }, BackgroundColor(DRIP)));
    for left in [true, false] {
        let (l, r) = if left { (px(5), auto()) } else { (auto(), px(5)) };
        p.spawn((Node { left: l, right: r, top: px(5), width: px(3), height: px(3), ..abs() }, BackgroundColor(DRIP)));
    }
}
fn button(mut node: Node, a: Action) -> impl Bundle {
    if node.border == UiRect::default() {
        node.border = UiRect::all(px(1));
    }
    node.justify_content = JustifyContent::Center;
    node.align_items = AlignItems::Center;
    (Node { border_radius: BorderRadius::all(px(2)), ..node }, BackgroundColor(CAST), BorderColor::all(SEAM), Interaction::None, Btn::default(), Flash(0.0), a)
}
fn band(n: Node) -> impl Bundle {
    (Node { position_type: PositionType::Absolute, ..n }, BackgroundColor(MOSS), Visibility::Hidden, Band)
}
fn top_band() -> impl Bundle {
    band(Node { left: px(0), right: px(0), top: px(0), height: px(3), ..default() })
}
fn divider() -> impl Bundle {
    (Node { width: px(1), align_self: AlignSelf::Stretch, margin: UiRect::vertical(px(6)), flex_shrink: 0.0, ..default() }, BackgroundColor(SEAM))
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
            t.spawn((Node { left: px(0), right: px(0), top, height: px(4), border: UiRect::all(px(1)), ..abs() }, BackgroundColor(DEEP), BorderColor::all(SEAM)));
            ids[0] = t.spawn((Node { left: px(0), top, height: px(4), width: px(0), ..abs() }, BackgroundColor(fill_c))).id();
            ids[1] = t
                .spawn((
                    Node { left: px(0), top: px((h - 16.0) / 2.0), width: px(8), height: px(16), margin: UiRect::left(px(-4)), border_radius: BorderRadius::all(px(2)), ..abs() },
                    BackgroundColor(CHALK),
                ))
                .id();
        })
        .id();
    (t, ids[0], ids[1])
}

fn spawn_hud(mut commands: Commands) {
    let none = Entity::PLACEHOLDER;
    let mut ids = Ids {
        clock: none, phase: none, pause: none, status: none, invalid: none, time_fill: none, time_knob: none, weather_body: none, hints_full: none, hints_tab: none,
        hint_list: none, strip_root: none, strip_body: none, tip: none, tip_title: none, tip_sub: none,
    };

    // ---- ToolBar
    commands.spawn(Node { left: px(0), right: px(0), bottom: px(16), justify_content: JustifyContent::Center, padding: UiRect::horizontal(px(16)), ..abs() }).with_children(|w| {
        w.spawn(slab(Node { padding: UiRect::all(px(6)), column_gap: px(5), max_width: percent(100), ..default() }, SLAB)).with_children(|bar| {
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
                            r.spawn((
                                button(
                                    Node { width: px(58), min_width: px(52), height: px(52), flex_shrink: 1.0, flex_direction: FlexDirection::Column, justify_content: JustifyContent::FlexEnd, padding: UiRect::bottom(px(6)), ..default() },
                                    Action::Tool(t),
                                ),
                                Tip(format!("{}  [{key}]", t.label()), strip::tip(t).into()),
                            ))
                            .with_children(|b| {
                                b.spawn(top_band());
                                b.spawn((txt(key, 10.0, DIM), Node { left: px(3), top: px(1), ..abs() }));
                                b.spawn((txt(t.label(), 12.0, CHALK), Lbl));
                            });
                        }
                    });
                });
            }
        });
    });

    // ---- VariantStrip (content rebuilt by `rebuild_strip`)
    commands.spawn(Node { left: px(0), right: px(0), bottom: px(104), justify_content: JustifyContent::Center, padding: UiRect::horizontal(px(16)), ..abs() }).with_children(|w| {
        ids.strip_root = w.spawn(slab(Node { padding: UiRect::all(px(8)), max_width: px(1000), display: Display::None, ..default() }, SLAB)).with_children(|p| {
            deco(p);
            ids.strip_body = p.spawn(col(4)).id();
        }).id();
    });

    // ---- WeatherPanel
    commands.spawn(Node { right: px(16), top: px(16), ..abs() }).with_children(|w| {
        w.spawn(slab(Node { width: px(244), padding: UiRect::all(px(8)), flex_direction: FlexDirection::Column, row_gap: px(6), ..default() }, SLAB)).with_children(|p| {
            deco(p);
            p.spawn(Node { margin: UiRect::top(px(2)), ..row(8) }).with_children(|r| {
                r.spawn((Node { flex_shrink: 0.0, ..default() }, Interaction::None, Action::Collapse)).with_children(|c| {
                    ids.clock = c.spawn(txt("12:00", 20.0, SODIUM)).id();
                });
                ids.phase = r.spawn(txt("DAY", 10.0, DIM)).id();
                r.spawn((button(Node { width: px(32), height: px(32), margin: UiRect::left(Val::Auto), ..default() }, Action::Pause), Tip("PAUSE TIME  [T]".into(), "Freeze or resume the clock".into()))).with_children(|b| {
                    ids.pause = b.spawn((txt("||", 12.0, CHALK), Lbl)).id();
                });
            });
            ids.weather_body = p
                .spawn(col(6))
                .with_children(|b| {
                    b.spawn(col(0)).with_children(|t| {
                        let (tr, fill, knob) = track(t, px(228), 20.0, SODIUM);
                        t.commands().entity(tr).insert(TimeTrack);
                        ids.time_fill = fill;
                        ids.time_knob = knob;
                        t.spawn(Node { width: px(228), height: px(6), ..default() }).with_children(|k| {
                            for pct in [25.0, 50.0, 75.0] {
                                k.spawn((Node { left: percent(pct), width: px(1), height: px(6), ..abs() }, BackgroundColor(SEAM)));
                            }
                        });
                        t.spawn(Node { width: px(228), justify_content: JustifyContent::SpaceBetween, ..default() }).with_children(|k| {
                            k.spawn(txt("00", 10.0, DIM));
                            k.spawn(txt("24", 10.0, DIM));
                        });
                    });
                    b.spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: px(2), row_gap: px(2), ..default() }).with_children(|g| {
                        for k in WeatherKind::ALL {
                            g.spawn(button(Node { width: px(74), height: px(32), ..default() }, Action::Weather(k))).with_children(|b| {
                                b.spawn(top_band());
                                b.spawn((txt(k.name().to_uppercase(), 12.0, CHALK), Lbl));
                            });
                        }
                    });
                    b.spawn(Node { justify_content: JustifyContent::SpaceBetween, ..row(6) }).with_children(|r| {
                        r.spawn((button(Node { min_height: px(32), padding: UiRect::horizontal(px(8)), column_gap: px(6), ..default() }, Action::Auto), Tog)).with_children(|t| {
                            toggle_box(t);
                            t.spawn((txt("AUTO CYCLE", 12.0, CHALK), Lbl));
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
                b.spawn((txt(label, 12.0, CHALK), Lbl));
                b.spawn((txt(key, 10.0, DIM), Node { right: px(3), top: px(1), ..abs() }));
            });
        }
    });

    // ---- HintsPanel (full + collapsed tab)
    commands.spawn(Node { left: px(16), top: px(60), ..abs() }).with_children(|w| {
        ids.hints_full = w
            .spawn(slab(Node { width: px(272), padding: UiRect::all(px(8)), flex_direction: FlexDirection::Column, row_gap: px(4), ..default() }, SLAB))
            .with_children(|p| {
                deco(p);
                p.spawn((button(Node { right: px(0), top: px(0), width: px(32), height: px(32), ..abs() }, Action::Hints), Tip("HINTS  [H]".into(), "Collapse the hints".into()))).with_children(|b| {
                    b.spawn((txt("x", 12.0, CHALK), Lbl));
                });
                ids.invalid = p.spawn((txt("", 12.0, SODIUM), Node { display: Display::None, ..default() })).id();
                ids.hint_list = p.spawn(col(4)).id();
            })
            .id();
        ids.hints_tab = w
            .spawn((button(Node { width: px(32), height: px(32), display: Display::None, ..default() }, Action::Hints), Tip("HINTS  [H]".into(), "Show the hints".into())))
            .with_children(|b| {
                b.spawn((txt("?", 12.0, CHALK), Lbl));
            })
            .id();
    });

    // ---- Tooltip
    ids.tip = commands
        .spawn((
            Node { max_width: px(260), padding: UiRect::all(px(6)), border: UiRect::all(px(1)), flex_direction: FlexDirection::Column, display: Display::None, ..abs() },
            BackgroundColor(DEEP),
            BorderColor::all(SEAM),
            GlobalZIndex(1000),
        ))
        .with_children(|t| {
            ids.tip_title = t.spawn(txt("", 14.0, CHALK)).id();
            ids.tip_sub = t.spawn(txt("", 12.0, DIM)).id();
        })
        .id();

    commands.insert_resource(ids);
}

fn toggle_box(t: &mut ChildSpawnerCommands) {
    t.spawn((Node { width: px(12), height: px(12), border: UiRect::all(px(1)), flex_shrink: 0.0, ..default() }, BorderColor::all(DIM))).with_children(|b| {
        b.spawn((
            Node { left: px(0), right: px(0), top: px(0), bottom: px(0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..abs() },
            BackgroundColor(MOSS),
            Visibility::Hidden,
            Band,
        ))
        .with_children(|f| {
            f.spawn((Node { width: px(4), height: px(4), ..default() }, BackgroundColor(SLAB)));
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
        b.spawn((txt(o.l, 12.0, CHALK), Lbl));
    });
}

fn fill_strip(p: &mut ChildSpawnerCommands, tool: Tool) {
    let (a, b) = strip::rows(tool);
    let mut seg_row = 0u8;
    for (i, ctls) in [a, b].iter().enumerate() {
        if ctls.is_empty() {
            continue;
        }
        p.spawn(Node { flex_wrap: FlexWrap::Wrap, row_gap: px(4), ..row(6) }).with_children(|r| {
            if i == 0 {
                r.spawn((Node { padding: UiRect { left: px(12), right: px(10), top: px(6), bottom: px(6) }, align_items: AlignItems::Center, ..default() }, BackgroundColor(DEEP))).with_children(|n| {
                    n.spawn((Node { left: px(0), top: px(0), bottom: px(0), width: px(3), ..abs() }, BackgroundColor(MOSS)));
                    n.spawn(txt(tool.label(), 14.0, CHALK));
                });
            }
            for (k, c) in ctls.iter().enumerate() {
                if i == 0 || k > 0 {
                    r.spawn(divider());
                }
                cluster(r, c, &mut seg_row);
            }
        });
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
                    seg_button(g, o, Action::Set(o.set), Some(Cond { sel: o.sel, en: o.en }), Seg(grp, i));
                }
            });
        }
        C::Tog(o) => {
            r.spawn((button(Node { min_height: px(32), padding: UiRect::horizontal(px(8)), column_gap: px(6), ..default() }, Action::Set(o.set)), Cond { sel: o.sel, en: o.en }, Tog)).with_children(|t| {
                toggle_box(t);
                t.spawn((txt(o.l, 12.0, CHALK), Lbl));
            });
        }
        C::Sld(sl) => {
            r.spawn(Node { min_height: px(32), ..row(6) }).with_children(|g| {
                g.spawn(txt(sl.l, 12.0, CHALK));
                let (tr, fill, knob) = track(g, px(96), 32.0, MOSS);
                let text = g.spawn(txt("", 10.0, DIM)).id();
                g.commands().entity(tr).insert(Slider { sl: *sl, fill, knob, text });
            });
        }
        C::Sw(slot, label, en) => {
            r.spawn(row(0)).with_children(|g| {
                g.spawn((txt(*label, 10.0, DIM), Node { margin: UiRect::right(px(4)), ..default() }));
                for (i, (name, h)) in palette(*slot).iter().enumerate() {
                    g.spawn((
                        button(Node { width: px(32), height: px(32), padding: UiRect::all(px(4)), border: UiRect::all(px(2)), ..default() }, Action::Color(*slot, i as u8)),
                        Cond { sel: yes, en: *en },
                        Tip((*label).into(), (*name).into()),
                    ))
                    .with_children(|b| {
                        b.spawn((Node { width: px(20), height: px(20), border: UiRect::all(px(1)), ..default() }, BackgroundColor(hex(*h)), BorderColor::all(DRIP)));
                        b.spawn(band(Node { left: px(0), right: px(0), bottom: px(-5), height: px(3), ..default() }));
                    });
                }
            });
        }
        C::Clutter => {
            r.spawn(Node { width: px(7.0 * 72.0 + 6.0 * 2.0), flex_wrap: FlexWrap::Wrap, column_gap: px(2), row_gap: px(2), ..default() }).with_children(|g| {
                for (i, (k, l, full)) in CLUTTER.iter().enumerate() {
                    g.spawn((button(Node { width: px(72), height: px(36), ..default() }, Action::Clutter(*k)), Seg(0, i), Tip((*full).into(), "Click to place".into()))).with_children(|b| {
                        b.spawn(top_band());
                        b.spawn((txt(*l, 12.0, CHALK), Lbl));
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

fn rebuild_hints(mut commands: Commands, s: Res<ToolSettings>, status: Res<ToolStatus>, ids: Res<Ids>, mut cur: Local<Option<(Tool, bool)>>) {
    let drawing = status.drawing;
    if *cur == Some((s.tool, drawing)) {
        return;
    }
    *cur = Some((s.tool, drawing));
    let tool = s.tool;
    commands.entity(ids.hint_list).despawn_children().with_children(|p| {
        p.spawn(txt(tool.label(), 12.0, CHALK));
        for (key, desc) in strip::hints(tool).iter().filter(|(k, _)| !(drawing && *k == "DRAG")) {
            p.spawn(row(6)).with_children(|r| {
                r.spawn((Node { padding: UiRect::axes(px(4), px(2)), flex_shrink: 0.0, ..default() }, BackgroundColor(DEEP))).with_children(|c| {
                    c.spawn(txt(*key, 10.0, CHALK));
                });
                r.spawn((txt(*desc, 12.0, DIM), Node { flex_basis: px(0), flex_grow: 1.0, ..default() }));
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
            Action::Set(_) => c.map_or((false, true, false), |c| ((c.sel)(&s), (c.en)(&s), false)),
            Action::Tool(t) => (s.tool == t, !status.drawing || s.tool == t, false),
            Action::Clutter(k) => (s.clutter == k, true, false),
            Action::Color(slot, i) => (color_get(&s.colors, slot) == i, c.is_none_or(|c| (c.en)(&s)), false),
            Action::Weather(k) => (!w.auto && w.kind == k, true, w.auto && w.kind == k),
            Action::Auto => (w.auto, true, false),
            Action::Pause => (clock.paused, true, false),
            Action::Undo => (false, glade.can_undo(), false),
            Action::Redo => (false, glade.can_redo(), false),
            Action::Hints | Action::Collapse => (false, true, false),
        };
        let n = Btn { sel, en, now };
        if *b != n {
            *b = n;
        }
    }
}

fn paint(
    time: Res<Time>,
    mut q: Query<(Entity, &Btn, &Interaction, &mut Flash, &mut BackgroundColor, &mut BorderColor, Has<Tog>)>,
    kids: Query<&Children>,
    mut vis: Query<&mut Visibility, With<Band>>,
    mut tcol: Query<&mut TextColor, With<Lbl>>,
) {
    for (e, b, i, mut fl, mut bg, mut bd, tog) in &mut q {
        fl.0 = if *i == Interaction::Pressed { 0.08 } else { (fl.0 - time.delta_secs()).max(0.0) };
        let edge = if b.sel && !tog { MOSS } else { SEAM };
        let (fill, edge) = if !b.en {
            (DIS_BG, SEAM)
        } else if fl.0 > 0.0 {
            (PRESS, edge)
        } else if b.sel && !tog {
            (LIT, MOSS)
        } else if *i != Interaction::None {
            (LIT, edge)
        } else {
            (CAST, edge)
        };
        if bg.0 != fill {
            bg.0 = fill;
        }
        let edge = BorderColor::all(edge);
        if *bd != edge {
            *bd = edge;
        }
        let (show, text) = (b.sel || b.now, if b.en { CHALK } else { DIS_TXT });
        for d in kids.iter_descendants(e) {
            if let Ok(mut v) = vis.get_mut(d) {
                v.set_if_neq(if show { Visibility::Inherited } else { Visibility::Hidden });
            }
            if let Ok(mut t) = tcol.get_mut(d) {
                if t.0 != text {
                    t.0 = text;
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

fn place(nodes: &mut Query<&mut Node>, fill: Entity, knob: Entity, f: f32) {
    let v = percent(f * 100.0);
    if let Ok(n) = nodes.get(fill)
        && n.width != v
    {
        nodes.get_mut(fill).unwrap().width = v;
    }
    if let Ok(n) = nodes.get(knob)
        && n.left != v
    {
        nodes.get_mut(knob).unwrap().left = v;
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
    let phase = match h {
        5.0..8.0 => "DAWN",
        8.0..17.0 => "DAY",
        17.0..20.0 => "DUSK",
        _ => "NIGHT",
    };
    set_text(&mut texts, ids.phase, phase.into());
    set_text(&mut texts, ids.pause, if clock.paused { ">" } else { "||" }.into());
    set_text(&mut texts, ids.status, if weather.auto { "CYCLING" } else { "HELD" }.into());
    place(&mut nodes, ids.time_fill, ids.time_knob, h / 24.0);
    show(&mut nodes, ids.weather_body, open.0);
    show(&mut nodes, ids.hints_full, hv.0);
    show(&mut nodes, ids.hints_tab, !hv.0);
    set_text(&mut texts, ids.invalid, status.invalid.clone().unwrap_or_default());
    show(&mut nodes, ids.invalid, status.invalid.is_some());
    for x in &sl {
        let v = (x.sl.get)(&s);
        place(&mut nodes, x.fill, x.knob, ((v - x.sl.lo) / (x.sl.hi - x.sl.lo)).clamp(0.0, 1.0));
        let unit = if x.sl.unit.is_empty() { String::new() } else { format!(" {}", x.sl.unit) };
        set_text(&mut texts, x.text, format!("{v:.d$}{unit}", d = x.sl.dec));
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
