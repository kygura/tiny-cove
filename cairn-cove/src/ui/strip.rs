//! Data tables for the HUD: variant-strip controls per tool and hint lines per tool.

use crate::tools::{Tool, ToolSettings as S};
use cairn_cove_core::{BridgeKind, ClutterKind, FenceKind, Hanging, Material, OpeningKind, PaintLayer, PathKind, RoofStyle, StairKind, TreeKind, WallKind};

pub fn yes(_: &S) -> bool {
    true
}
fn no(_: &S) -> bool {
    false
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
    /// Swatch group: palette slot (0 roof, 1 daub, 2 trim), label, enabled.
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
    C::Seg("", vec![o!("WATTLE", material = Material::Wattle), o!("PLANK", material = Material::Plank), o!("STONE", material = Material::Stone)])
}
fn dyke_kind() -> C {
    C::Seg("", vec![o!("DRY STONE", wall_kind = WallKind::DryStone), o!("TURF", wall_kind = WallKind::Turf), o!("PALISADE", wall_kind = WallKind::Palisade)])
}
fn roofs(round: bool) -> C {
    // Round footprints fall back from Boat to a thatch cone.
    let lim: fn(&S) -> bool = if round { no } else { yes };
    C::Seg(
        "",
        vec![o!("THATCH", roof = RoofStyle::Thatch), o!("TURF", roof = RoofStyle::Turf), o!("HIDE", roof = RoofStyle::Hide), o!("BOAT", roof = RoofStyle::Boat, lim)],
    )
}
fn footing() -> C {
    sld!("FOOTING", footing, 0.0, 2.0, 0.1, 1, "M")
}
fn stilts() -> C {
    sld!("STILTS", stilts, 0.0, 3.0, 0.1, 1, "M")
}
fn weathering() -> C {
    sld!("WEATHER", weathering, 0.0, 1.0, 0.05, 2, "")
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
fn daub_sw(en: fn(&S) -> bool) -> C {
    C::Sw(1, "DAUB", en)
}
fn trim_sw(en: fn(&S) -> bool) -> C {
    C::Sw(2, "TRIM", en)
}

fn bridge_railing(n: u8, l: &'static str) -> Opt {
    // `n` is baked in through a match so the fn pointers stay capture-free.
    macro_rules! r {
        ($n:expr) => {
            Opt { l, set: |s| s.bridge = BridgeKind::Rope { railing: $n }, sel: |s| s.bridge == BridgeKind::Rope { railing: $n }, en: rope }
        };
    }
    match n {
        0 => r!(0),
        1 => r!(1),
        2 => r!(2),
        _ => r!(3),
    }
}
fn rope(s: &S) -> bool {
    matches!(s.bridge, BridgeKind::Rope { .. })
}
fn causeway(s: &S) -> bool {
    s.bridge == BridgeKind::Causeway
}
fn not_causeway(s: &S) -> bool {
    !causeway(s)
}
fn painted_fence(s: &S) -> bool {
    matches!(s.fence, FenceKind::Driftwood | FenceKind::NetLine)
}

/// Row A and row B controls for a tool. Erase has none (strip hidden).
pub fn rows(tool: Tool) -> (Vec<C>, Vec<C>) {
    let seg = |l: &'static str, v: Vec<Opt>| C::Seg(l, v);
    match tool {
        Tool::Dyke => (
            vec![dyke_kind(), tog!("CREST", crest), tog!("TUMBLED", tumbled), sld!("THICK", wall_thickness, 0.3, 1.2, 0.1, 1, "M"), footing(), weathering()],
            vec![daub_sw(yes), trim_sw(yes)],
        ),
        Tool::Windbreak => (
            vec![seg("", vec![o!("WATTLE", fence = FenceKind::Wattle), o!("DRIFTWOOD", fence = FenceKind::Driftwood), o!("NETS", fence = FenceKind::NetLine), o!("REED", fence = FenceKind::Reed)])],
            vec![trim_sw(painted_fence)],
        ),
        Tool::Hut => (
            vec![mat(), roofs(false), pitch(), tog!("CARVED", carved), tog!("HANGINGS", hangings), stilts(), weathering()],
            vec![roof_sw(), daub_sw(yes), trim_sw(yes)],
        ),
        Tool::Roundhouse => (vec![mat(), roofs(true), tog!("HANGINGS", hangings), stilts(), weathering()], vec![roof_sw(), daub_sw(yes), trim_sw(yes)]),
        Tool::Track => (
            vec![
                seg("", vec![o!("SHELL", path = PathKind::Shell), o!("PEBBLE", path = PathKind::Pebble), o!("FLAGS", path = PathKind::Flagstone), o!("BOARDS", path = PathKind::Boardwalk), o!("STEPPING", path = PathKind::Stepping)]),
                sld!("WIDTH", path_width, 0.8, 4.0, 0.2, 1, "M"),
            ],
            vec![],
        ),
        Tool::Steps => (
            vec![seg("", vec![o!("STONE", stairs = StairKind::Stone), o!("LOG", stairs = StairKind::Log), o!("ROPE", stairs = StairKind::RopeLadder)])],
            vec![trim_sw(yes)],
        ),
        Tool::Jetty => (vec![], vec![daub_sw(yes), trim_sw(yes)]),
        Tool::Bridge => (
            vec![
                seg(
                    "",
                    vec![
                        Opt { l: "ROPE", set: |s| if !rope(s) { s.bridge = BridgeKind::Rope { railing: 0 } }, sel: rope, en: yes },
                        o!("LOG", bridge = BridgeKind::Log),
                        o!("CAUSEWAY", bridge = BridgeKind::Causeway),
                    ],
                ),
                seg("RAILING", vec![bridge_railing(0, "1"), bridge_railing(1, "2"), bridge_railing(2, "3"), bridge_railing(3, "4")]),
                seg("HANGING", vec![o!("NONE", hanging = Hanging::None, rope), o!("LANTERNS", hanging = Hanging::Lanterns, rope), o!("PENNANTS", hanging = Hanging::Pennants, rope)]),
            ],
            vec![daub_sw(causeway), trim_sw(not_causeway)],
        ),
        Tool::Opening => (
            vec![seg("PLACE", vec![o!("WINDOW", opening = OpeningKind::Window), o!("DOOR", opening = OpeningKind::Door), o!("BOAT DOOR", opening = OpeningKind::WideDoor)])],
            vec![],
        ),
        Tool::Roof => (vec![roofs(false), pitch()], vec![roof_sw()]),
        Tool::Tree => (
            vec![seg("", vec![o!("SEA PINE", tree = TreeKind::SeaPine), o!("PALM", tree = TreeKind::Palm), o!("CYPRESS", tree = TreeKind::Cypress), o!("FIG", tree = TreeKind::Fig), o!("SNAG", tree = TreeKind::Snag)])],
            vec![],
        ),
        Tool::Prop => (vec![C::Clutter], vec![]),
        Tool::Paint => (
            vec![
                seg(
                    "",
                    vec![
                        o!("SEA PINKS", paint = PaintLayer::SeaPinks),
                        o!("DUNE GRASS", paint = PaintLayer::DuneGrass),
                        o!("WRACK", paint = PaintLayer::Wrack),
                        o!("PEBBLES", paint = PaintLayer::Pebbles),
                        o!("ERASE", paint = PaintLayer::Clear),
                    ],
                ),
                sld!("BRUSH", brush_radius, 1.0, 8.0, 0.5, 1, "M"),
            ],
            vec![],
        ),
        Tool::Raise => (vec![seg("", vec![o!("RAISE", lower = false), o!("LOWER", lower = true)]), radius(), strength()], vec![]),
        Tool::Flatten => (vec![radius(), strength()], vec![]),
        Tool::Lagoon => (vec![radius(), sld!("DEPTH", water_depth, 0.3, 1.5, 0.1, 1, "M")], vec![]),
        Tool::Dye => (vec![mat(), roofs(false), tog!("CARVED", carved), weathering()], vec![roof_sw(), daub_sw(yes), trim_sw(yes)]),
        Tool::Erase => (vec![], vec![]),
    }
}

/// The 14 clutter cells: (kind, cell label, full tooltip name), 7 per row.
pub const CLUTTER: [(ClutterKind, &str, &str); 14] = [
    (ClutterKind::Canoe, "CANOE", "OUTRIGGER CANOE"),
    (ClutterKind::DryingRack, "RACK", "FISH DRYING RACK"),
    (ClutterKind::Firepit, "FIRE", "FIREPIT"),
    (ClutterKind::Cairn, "CAIRN", "CAIRN"),
    (ClutterKind::StandingStone, "STONE", "STANDING STONE"),
    (ClutterKind::CarvedPole, "POLE", "CARVED POLE"),
    (ClutterKind::NetRack, "NETS", "NET RACK"),
    (ClutterKind::Baskets, "BASKETS", "BASKETS AND JARS"),
    (ClutterKind::LanternPost, "LANTERN", "LANTERN POST"),
    (ClutterKind::Pennant, "PENNANT", "PENNANT POLE"),
    (ClutterKind::Rock, "ROCK", "ROCK"),
    (ClutterKind::Bush, "GORSE", "FLOWERING GORSE"),
    (ClutterKind::CrabPots, "POTS", "CRAB POTS"),
    (ClutterKind::Woodpile, "WOOD", "DRIFTWOOD PILE"),
];

/// Toolbar tooltip second line (DESIGN section 2.1).
pub fn tip(t: Tool) -> &'static str {
    match t {
        Tool::Dyke => "Drag a dry-stone, turf or palisade dyke",
        Tool::Windbreak => "Drag wattle, driftwood, nets or reed",
        Tool::Hut => "Drag a footprint, lift it, click to build",
        Tool::Roundhouse => "Drag a circle; tall makes a beacon",
        Tool::Track => "Drag a track; it cuts gaps through dykes",
        Tool::Steps => "Drag from ground up to a dyke top",
        Tool::Jetty => "Drag a plank jetty out over the water",
        Tool::Bridge => "Drag across water or between dykes",
        Tool::Opening => "Click a wall: window, door, boat door",
        Tool::Roof => "Click inside a closed dyke ring",
        Tool::Tree => "Click to plant",
        Tool::Prop => "Click to place a prop",
        Tool::Paint => "Brush sea pinks, dune grass, wrack or pebbles",
        Tool::Raise => "Drag to raise; Shift to lower",
        Tool::Flatten => "Drag to level the ground",
        Tool::Lagoon => "Drag to carve tide pools and lagoons",
        Tool::Dye => "Click a structure to copy its colours",
        Tool::Erase => "Click or drag to remove",
    }
}

pub fn tool_group(t: Tool) -> usize {
    match t {
        Tool::Dyke | Tool::Windbreak | Tool::Hut | Tool::Roundhouse | Tool::Track | Tool::Steps | Tool::Jetty | Tool::Bridge => 0,
        Tool::Opening | Tool::Roof | Tool::Tree | Tool::Prop | Tool::Paint => 1,
        Tool::Raise | Tool::Flatten | Tool::Lagoon => 2,
        Tool::Dye | Tool::Erase => 3,
    }
}
pub const GROUPS: [&str; 4] = ["BUILD", "DECORATE", "TERRAIN", "EDIT"];

/// (key chip, description) lines, at most 6 (the name plate is the 7th).
pub fn hints(t: Tool) -> &'static [(&'static str, &'static str)] {
    const SCROLL_H: (&str, &str) = ("SCROLL", "Dyke height");
    match t {
        Tool::Dyke => &[("DRAG", "Lay a dyke; click adds points"), ("CLICK", "On the last point to end"), SCROLL_H, ("SHIFT", "Snap 45 deg, close the ring"), ("ESC", "Cancel")],
        Tool::Windbreak => &[("DRAG", "Lay a windbreak; click adds points"), ("CLICK", "On the last point to end"), ("SCROLL", "Windbreak kind"), ("SHIFT", "Snap 45 deg, close the ring"), ("ESC", "Cancel")],
        Tool::Hut => &[("DRAG", "Footprint"), ("SCROLL", "Rotate the rectangle"), ("SHIFT", "Force a square"), ("MOVE", "Height, after release"), ("CLICK", "Build"), ("ESC", "Cancel")],
        Tool::Roundhouse => &[("DRAG", "Footprint"), ("MOVE", "Height, after release"), ("SCROLL", "Roof style"), ("CLICK", "Build"), ("ESC", "Cancel")],
        Tool::Track => &[("DRAG", "Lay a track; click adds points"), ("CLICK", "On the last point to end"), ("SCROLL", "Track width"), ("SHIFT", "Snap 45 deg"), ("ESC", "Cancel")],
        Tool::Steps => &[("DRAG", "From ground to a dyke top"), ("SCROLL", "Width"), ("ENDS", "Snap to dyke tops and jetties"), ("ESC", "Cancel")],
        Tool::Jetty => &[("DRAG", "Footprint"), ("MOVE", "Height, after release"), ("CLICK", "Build"), ("ESC", "Cancel")],
        Tool::Bridge => &[("DRAG", "From anchor A to B"), ("SCROLL", "Width"), ("ENDS", "Snap to dyke tops and jetties"), ("ESC", "Cancel")],
        Tool::Opening => &[("CLICK", "Place the chosen opening"), ("CLICK", "On an opening upgrades it"), ("SHIFT", "Click removes an opening")],
        Tool::Roof => &[("CLICK", "Roof the hovered dyke ring"), ("SCROLL", "Pitch"), ("SHIFT", "Scroll changes roof style")],
        Tool::Tree => &[("CLICK", "Plant"), ("SCROLL", "Rotate"), ("SHIFT", "Scroll scales 0.7 to 1.3")],
        Tool::Prop => &[("CLICK", "Place the prop"), ("TAB", "Next prop"), ("SCROLL", "Rotate"), ("SHIFT", "Scroll scales 0.7 to 1.3")],
        Tool::Paint => &[("DRAG", "Brush the layer"), ("SCROLL", "Brush size"), ("SHIFT", "Erase")],
        Tool::Raise => &[("DRAG", "Raise the ground"), ("SCROLL", "Radius"), ("SHIFT", "Lower")],
        Tool::Flatten => &[("DRAG", "Level the ground"), ("SCROLL", "Radius")],
        Tool::Lagoon => &[("DRAG", "Carve tide pools and lagoons"), ("SCROLL", "Radius")],
        Tool::Dye => &[("CLICK", "Copy a structure's colours"), ("TAB", "Edit it in the strip")],
        Tool::Erase => &[("CLICK", "Remove a structure or prop"), ("DRAG", "Remove many")],
    }
}
