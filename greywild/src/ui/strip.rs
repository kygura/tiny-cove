//! Data tables for the HUD: variant-strip controls per tool and hint lines per tool.

use crate::tools::{Tool, ToolSettings as S};
use greywild_core::{BridgeKind, ClutterKind, FenceKind, Hanging, Material, OpeningKind, PaintLayer, PathKind, RoofStyle, StairKind, TreeKind};

pub fn yes(_: &S) -> bool {
    true
}
fn no(_: &S) -> bool {
    false
}
fn green(s: &S) -> bool {
    s.roof == RoofStyle::Green
}

/// One selectable option (segment, toggle or clutter cell).
#[derive(Clone, Copy)]
pub struct Opt {
    pub l: &'static str,
    pub set: fn(&mut S),
    pub sel: fn(&S) -> bool,
    pub en: fn(&S) -> bool,
}

/// A slider bound to one `f32` field.
#[derive(Clone, Copy)]
pub struct Sl {
    pub l: &'static str,
    pub get: fn(&S) -> f32,
    pub set: fn(&mut S, f32),
    pub lo: f32,
    pub hi: f32,
    pub step: f32,
    pub dec: usize,
    pub unit: &'static str,
}

/// A control cluster (dividers go between clusters).
pub enum C {
    /// Optional prefix label + exactly-one-selected segments.
    Seg(&'static str, Vec<Opt>),
    Tog(Opt),
    Sld(Sl),
    /// Swatch group: palette slot (0 roof, 1 concrete, 2 accent), label, enabled.
    Sw(u8, &'static str, fn(&S) -> bool),
    Clutter,
}

macro_rules! o {
    ($l:expr, $f:ident = $v:expr) => {
        o!($l, $f = $v, yes)
    };
    ($l:expr, $f:ident = $v:expr, $en:expr) => {
        Opt { l: $l, set: |s| s.$f = $v, sel: |s| s.$f == $v, en: $en }
    };
}
macro_rules! tog {
    ($l:expr, $f:ident) => {
        tog!($l, $f, yes)
    };
    ($l:expr, $f:ident, $en:expr) => {
        C::Tog(Opt { l: $l, set: |s| s.$f = !s.$f, sel: |s| s.$f, en: $en })
    };
}
macro_rules! sld {
    ($l:expr, $f:ident, $lo:expr, $hi:expr, $step:expr, $dec:expr, $unit:expr) => {
        C::Sld(Sl { l: $l, get: |s| s.$f, set: |s, v| s.$f = v, lo: $lo, hi: $hi, step: $step, dec: $dec, unit: $unit })
    };
}

fn mat() -> C {
    C::Seg("", vec![o!("CONCRETE", material = Material::Concrete), o!("EARTH", material = Material::RammedEarth), o!("TIMBER", material = Material::Timber)])
}
fn roofs(tower: bool) -> C {
    let lim: fn(&S) -> bool = if tower { no } else { yes };
    C::Seg(
        "",
        vec![o!("PITCHED", roof = RoofStyle::Pitched, lim), o!("SAWTOOTH", roof = RoofStyle::Sawtooth, lim), o!("SHELL", roof = RoofStyle::Shell), o!("GREEN", roof = RoofStyle::Green)],
    )
}
fn plinth() -> C {
    sld!("PLINTH", plinth, 0.0, 2.0, 0.1, 1, "M")
}
fn overgrowth() -> C {
    sld!("OVERGROWTH", overgrowth, 0.0, 1.0, 0.05, 2, "")
}
fn pitch() -> C {
    sld!("PITCH", roof_pitch_deg, 15.0, 60.0, 5.0, 0, "\u{b0}")
}
fn radius() -> C {
    sld!("RADIUS", brush_radius, 1.0, 8.0, 0.5, 1, "M")
}
fn strength() -> C {
    sld!("STRENGTH", brush_strength, 0.1, 1.0, 0.1, 2, "")
}
fn roof_sw() -> C {
    C::Sw(0, "ROOF", yes)
}
fn conc_sw(en: fn(&S) -> bool) -> C {
    C::Sw(1, "CONCRETE", en)
}
fn acc_sw(en: fn(&S) -> bool) -> C {
    C::Sw(2, "ACCENT", en)
}

fn bridge_railing(n: u8, l: &'static str) -> Opt {
    // `n` is baked in through a match so the fn pointers stay capture-free.
    macro_rules! r {
        ($n:expr) => {
            Opt { l, set: |s| s.bridge = BridgeKind::Cable { railing: $n }, sel: |s| s.bridge == BridgeKind::Cable { railing: $n }, en: cable }
        };
    }
    match n {
        0 => r!(0),
        1 => r!(1),
        2 => r!(2),
        _ => r!(3),
    }
}
fn cable(s: &S) -> bool {
    matches!(s.bridge, BridgeKind::Cable { .. })
}
fn arch(s: &S) -> bool {
    s.bridge == BridgeKind::Arch
}
fn not_arch(s: &S) -> bool {
    !arch(s)
}
fn railing_fence(s: &S) -> bool {
    s.fence == FenceKind::Railing
}

/// Row A and row B controls for a tool. Erase has none (strip hidden).
pub fn rows(tool: Tool) -> (Vec<C>, Vec<C>) {
    let seg = |l: &'static str, v: Vec<Opt>| C::Seg(l, v);
    match tool {
        Tool::Wall => (
            vec![mat(), tog!("PLANTERS", planters), tog!("RUINED", ruined), sld!("THICK", wall_thickness, 0.3, 1.2, 0.1, 1, "M"), plinth(), overgrowth()],
            vec![conc_sw(yes), acc_sw(yes)],
        ),
        Tool::Fence => (
            vec![seg("", vec![o!("RAILING", fence = FenceKind::Railing), o!("BALUSTRADE", fence = FenceKind::Balustrade), o!("GABION", fence = FenceKind::Gabion), o!("HEDGE", fence = FenceKind::Hedge)])],
            vec![acc_sw(railing_fence)],
        ),
        Tool::Block => (
            vec![mat(), roofs(false), pitch(), tog!("FINS", fins), tog!("PLANTERS", planters, green), plinth(), overgrowth()],
            vec![roof_sw(), conc_sw(yes), acc_sw(yes)],
        ),
        Tool::Tower => (vec![mat(), roofs(true), tog!("PLANTERS", planters, green), plinth(), overgrowth()], vec![roof_sw(), conc_sw(yes), acc_sw(yes)]),
        Tool::Path => (
            vec![
                seg("", vec![o!("GRAVEL", path = PathKind::Gravel), o!("PAVERS", path = PathKind::Pavers), o!("SLABS", path = PathKind::Slabs), o!("POURED", path = PathKind::Poured), o!("STEPPING", path = PathKind::Stepping)]),
                sld!("WIDTH", path_width, 0.8, 4.0, 0.2, 1, "M"),
            ],
            vec![],
        ),
        Tool::Stairs => (
            vec![seg("", vec![o!("STEEL", stairs = StairKind::Steel), o!("CONCRETE", stairs = StairKind::Concrete), o!("LADDER", stairs = StairKind::Ladder)])],
            vec![acc_sw(yes)],
        ),
        Tool::Deck => (vec![], vec![conc_sw(yes), acc_sw(yes)]),
        Tool::Bridge => (
            vec![
                seg(
                    "",
                    vec![
                        Opt { l: "CABLE", set: |s| if !cable(s) { s.bridge = BridgeKind::Cable { railing: 0 } }, sel: cable, en: yes },
                        o!("ARCH", bridge = BridgeKind::Arch),
                        o!("GIRDER", bridge = BridgeKind::Girder),
                    ],
                ),
                seg("RAILING", vec![bridge_railing(0, "1"), bridge_railing(1, "2"), bridge_railing(2, "3"), bridge_railing(3, "4")]),
                seg("HANGING", vec![o!("NONE", hanging = Hanging::None, cable), o!("LAMPS", hanging = Hanging::Lamps, cable), o!("VINES", hanging = Hanging::Vines, cable)]),
            ],
            vec![conc_sw(arch), acc_sw(not_arch)],
        ),
        Tool::Opening => (
            vec![seg("PLACE", vec![o!("SLOT", opening = OpeningKind::Window), o!("DOOR", opening = OpeningKind::Door), o!("PORTAL", opening = OpeningKind::WideDoor)])],
            vec![],
        ),
        Tool::Roof => (vec![roofs(false), pitch(), tog!("PLANTERS", planters, green)], vec![roof_sw()]),
        Tool::Tree => (
            vec![seg("", vec![o!("OAK", tree = TreeKind::Oak), o!("PINE", tree = TreeKind::Pine), o!("BIRCH", tree = TreeKind::Birch), o!("FRUIT", tree = TreeKind::Fruit), o!("FERN", tree = TreeKind::Fern)])],
            vec![],
        ),
        Tool::Clutter => (vec![C::Clutter], vec![]),
        Tool::Paint => (
            vec![
                seg(
                    "",
                    vec![
                        o!("FLOWERS", paint = PaintLayer::Flowers),
                        o!("FERNS", paint = PaintLayer::Ferns),
                        o!("MOSS", paint = PaintLayer::Moss),
                        o!("GRAVEL", paint = PaintLayer::Gravel),
                        o!("ERASE", paint = PaintLayer::Clear),
                    ],
                ),
                sld!("BRUSH", brush_radius, 1.0, 8.0, 0.5, 1, "M"),
            ],
            vec![],
        ),
        Tool::Sculpt => (vec![seg("", vec![o!("RAISE", lower = false), o!("LOWER", lower = true)]), radius(), strength()], vec![]),
        Tool::Flatten => (vec![radius(), strength()], vec![]),
        Tool::Water => (vec![radius(), sld!("DEPTH", water_depth, 0.3, 1.5, 0.1, 1, "M")], vec![]),
        Tool::Style => (vec![mat(), roofs(false), tog!("FINS", fins), overgrowth()], vec![roof_sw(), conc_sw(yes), acc_sw(yes)]),
        Tool::Erase => (vec![], vec![]),
    }
}

/// The 14 clutter cells: (kind, cell label, full tooltip name), 7 per row.
pub const CLUTTER: [(ClutterKind, &str, &str); 14] = [
    (ClutterKind::RainBarrel, "BARREL", "RAIN BARREL"),
    (ClutterKind::Planter, "PLANTER", "PLANTER"),
    (ClutterKind::Compost, "COMPOST", "COMPOST HEAP"),
    (ClutterKind::Cistern, "CISTERN", "CISTERN"),
    (ClutterKind::Wheelbarrow, "BARROW", "WHEELBARROW"),
    (ClutterKind::Bench, "BENCH", "CONCRETE BENCH"),
    (ClutterKind::LampPost, "LAMP", "SODIUM LAMP POST"),
    (ClutterKind::WindTurbine, "TURBINE", "WIND TURBINE"),
    (ClutterKind::WashingLine, "WASHING", "WASHING LINE"),
    (ClutterKind::LogPile, "LOGS", "LOG PILE"),
    (ClutterKind::Rock, "ROCK", "ROCK"),
    (ClutterKind::Bush, "BUSH", "BUSH"),
    (ClutterKind::Beehive, "HIVE", "BEEHIVE"),
    (ClutterKind::Solar, "SOLAR", "SOLAR ARRAY"),
];

/// Toolbar tooltip second line (DESIGN section 2.1).
pub fn tip(t: Tool) -> &'static str {
    match t {
        Tool::Wall => "Drag a line of cast concrete, earth or timber",
        Tool::Fence => "Drag a railing, balustrade, gabion or hedge",
        Tool::Block => "Drag a footprint, lift it, click to cast",
        Tool::Tower => "Drag a circle for a silo tower",
        Tool::Path => "Drag a path; it cuts portals through walls",
        Tool::Stairs => "Drag from ground up to a wall top",
        Tool::Deck => "Drag a concrete deck on pilotis",
        Tool::Bridge => "Drag across water or between walls",
        Tool::Opening => "Click a wall: slot, then door, then portal",
        Tool::Roof => "Click inside a closed wall ring",
        Tool::Tree => "Click to plant",
        Tool::Clutter => "Click to place a prop",
        Tool::Paint => "Brush wildflowers, ferns, moss or gravel",
        Tool::Sculpt => "Drag to raise; Shift to lower",
        Tool::Flatten => "Drag to level the ground",
        Tool::Water => "Drag to carve ponds and streams",
        Tool::Style => "Click a structure to copy its style",
        Tool::Erase => "Click or drag to remove",
    }
}

pub fn tool_group(t: Tool) -> usize {
    match t {
        Tool::Wall | Tool::Fence | Tool::Block | Tool::Tower | Tool::Path | Tool::Stairs | Tool::Deck | Tool::Bridge => 0,
        Tool::Opening | Tool::Roof | Tool::Tree | Tool::Clutter | Tool::Paint => 1,
        Tool::Sculpt | Tool::Flatten | Tool::Water => 2,
        Tool::Style | Tool::Erase => 3,
    }
}
pub const GROUPS: [&str; 4] = ["BUILD", "DECORATE", "TERRAIN", "EDIT"];

/// (key chip, description) lines, at most 6 (the name plate is the 7th).
pub fn hints(t: Tool) -> &'static [(&'static str, &'static str)] {
    const SCROLL_H: (&str, &str) = ("SCROLL", "Wall height");
    match t {
        Tool::Wall => &[("DRAG", "Lay a wall; click adds points"), ("CLICK", "On the last point to end"), SCROLL_H, ("SHIFT", "Snap 45 deg, close the ring"), ("ESC", "Cancel")],
        Tool::Fence => &[("DRAG", "Lay a fence; click adds points"), ("CLICK", "On the last point to end"), ("SCROLL", "Fence kind"), ("SHIFT", "Snap 45 deg, close the ring"), ("ESC", "Cancel")],
        Tool::Block => &[("DRAG", "Footprint"), ("SCROLL", "Rotate the rectangle"), ("SHIFT", "Force a square"), ("MOVE", "Height, after release"), ("CLICK", "Cast"), ("ESC", "Cancel")],
        Tool::Tower => &[("DRAG", "Footprint"), ("MOVE", "Height, after release"), ("SCROLL", "Roof style"), ("CLICK", "Cast"), ("ESC", "Cancel")],
        Tool::Path => &[("DRAG", "Lay a path; click adds points"), ("CLICK", "On the last point to end"), ("SCROLL", "Path width"), ("SHIFT", "Snap 45 deg"), ("ESC", "Cancel")],
        Tool::Stairs => &[("DRAG", "From ground to a wall top"), ("SCROLL", "Width"), ("ENDS", "Snap to wall tops and decks"), ("ESC", "Cancel")],
        Tool::Deck => &[("DRAG", "Footprint"), ("MOVE", "Height, after release"), ("CLICK", "Cast"), ("ESC", "Cancel")],
        Tool::Bridge => &[("DRAG", "From anchor A to B"), ("SCROLL", "Width"), ("ENDS", "Snap to wall tops and decks"), ("ESC", "Cancel")],
        Tool::Opening => &[("CLICK", "Place the chosen opening"), ("CLICK", "On an opening upgrades it"), ("SHIFT", "Click removes an opening")],
        Tool::Roof => &[("CLICK", "Roof the hovered wall ring"), ("SCROLL", "Pitch"), ("SHIFT", "Scroll changes roof style")],
        Tool::Tree => &[("CLICK", "Plant"), ("SCROLL", "Rotate"), ("SHIFT", "Scroll scales 0.7 to 1.3")],
        Tool::Clutter => &[("CLICK", "Place the prop"), ("TAB", "Next prop"), ("SCROLL", "Rotate"), ("SHIFT", "Scroll scales 0.7 to 1.3")],
        Tool::Paint => &[("DRAG", "Brush the layer"), ("SCROLL", "Brush size"), ("SHIFT", "Erase")],
        Tool::Sculpt => &[("DRAG", "Raise the ground"), ("SCROLL", "Radius"), ("SHIFT", "Lower")],
        Tool::Flatten => &[("DRAG", "Level the ground"), ("SCROLL", "Radius")],
        Tool::Water => &[("DRAG", "Carve ponds and streams"), ("SCROLL", "Radius")],
        Tool::Style => &[("CLICK", "Copy a structure's style"), ("TAB", "Edit it in the strip")],
        Tool::Erase => &[("CLICK", "Remove a structure or prop"), ("DRAG", "Remove many")],
    }
}
