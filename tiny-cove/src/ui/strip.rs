//! Data tables for the HUD (DESIGN.md B5-B8): variant-strip controls per tool, the prop grid,
//! tooltip text, toolbar groups and hint lines. Every per-tool difference lives here.

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
    /// Shown at all (a hidden option is never enabled either).
    pub show: fn(&S) -> bool,
}

/// A slider bound to one `f32` field. `zero` replaces the readout at 0 (`AUTO` for stilts).
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
    pub zero: &'static str,
}

/// A control cluster (dividers go between clusters).
pub enum C {
    /// Optional prefix label + exactly-one-selected segments.
    Seg(&'static str, Vec<Opt>),
    Tog(Opt),
    Sld(Sl),
    /// Swatch group: palette slot (0 roof, 1 wall, 2 trim), label, enabled.
    Sw(u8, &'static str, fn(&S) -> bool),
    Clutter,
}

macro_rules! o {
    ($l:expr, $f:ident = $v:expr) => {
        o!($l, $f = $v, yes)
    };
    ($l:expr, $f:ident = $v:expr, $en:expr) => {
        Opt { l: $l, set: |s| s.$f = $v, sel: |s| s.$f == $v, en: $en, show: yes }
    };
}
macro_rules! tog {
    ($l:expr, $f:ident) => {
        tog!($l, $f, yes)
    };
    ($l:expr, $f:ident, $en:expr) => {
        C::Tog(Opt { l: $l, set: |s| s.$f = !s.$f, sel: |s| s.$f, en: $en, show: yes })
    };
}
macro_rules! sld {
    ($l:expr, $f:ident, $lo:expr, $hi:expr, $step:expr, $dec:expr, $unit:expr) => {
        C::Sld(Sl { l: $l, get: |s| s.$f, set: |s, v| s.$f = v, lo: $lo, hi: $hi, step: $step, dec: $dec, unit: $unit, zero: "" })
    };
}

// ---- enable predicates (controls that do not apply are shown disabled, not hidden)
fn rope(s: &S) -> bool {
    matches!(s.bridge, BridgeKind::Rope { .. })
}
fn not_beacon(s: &S) -> bool {
    !s.ghost_beacon
}
fn beacon(s: &S) -> bool {
    s.ghost_beacon
}
// Trim swatches follow what each variant's model really paints with the trim colour: net floats
// (not driftwood or wattle), palisade stake tips (not dry stone or turf), the rope ladder
// (not stone or log steps), and the props below (not firepit, cairn, rock or woodpile).
fn painted_fence(s: &S) -> bool {
    s.fence == FenceKind::NetLine
}
fn palisade(s: &S) -> bool {
    s.wall_kind == WallKind::Palisade
}
fn ladder(s: &S) -> bool {
    s.stairs == StairKind::RopeLadder
}
fn painted_prop(s: &S) -> bool {
    !matches!(s.clutter, ClutterKind::Firepit | ClutterKind::Cairn | ClutterKind::Rock | ClutterKind::Woodpile)
}
/// Dye after a Shift+click pick: only the picked structure's material control stays live.
fn dye_hut(s: &S) -> bool {
    s.dye.is_none_or(|d| d == "HUT")
}
fn dye_dyke(s: &S) -> bool {
    s.dye.is_none_or(|d| d == "DYKE")
}

// ---- shared clusters
fn mat(en: fn(&S) -> bool) -> C {
    C::Seg("", vec![o!("WATTLE", material = Material::Wattle, en), o!("PLANK", material = Material::Plank, en), o!("STONE", material = Material::Stone, en)])
}
fn roofs(round: bool) -> C {
    // Round footprints fall back from Boat to a thatch cone.
    let boat: fn(&S) -> bool = if round { no } else { yes };
    let mut v = vec![o!("THATCH", roof = RoofStyle::Thatch), o!("TURF", roof = RoofStyle::Turf), o!("HIDE", roof = RoofStyle::Hide), o!("BOAT", roof = RoofStyle::Boat, boat)];
    if round {
        // A beacon has only two roofs: the thatch cone over the fire, or open (the model reads any
        // non-thatch style as open; hide stands for it). The four round roofs give way to the two.
        for o in &mut v {
            o.show = not_beacon;
        }
        v.push(Opt { l: "CONE", set: |s| s.roof = RoofStyle::Thatch, sel: |s| s.roof == RoofStyle::Thatch, en: yes, show: beacon });
        v.push(Opt { l: "OPEN", set: |s| s.roof = RoofStyle::Hide, sel: |s| s.roof != RoofStyle::Thatch, en: yes, show: beacon });
    }
    C::Seg("", v)
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
fn sw_roof() -> C {
    C::Sw(0, "ROOF", yes)
}
fn sw_wall() -> C {
    C::Sw(1, "WALL", yes)
}
fn sw_trim(en: fn(&S) -> bool) -> C {
    C::Sw(2, "TRIM", en)
}
fn seg(l: &'static str, v: Vec<Opt>) -> C {
    C::Seg(l, v)
}

fn railing(n: u8, l: &'static str) -> Opt {
    // `n` is baked in through a match so the fn pointers stay capture-free.
    macro_rules! r {
        ($n:expr) => {
            Opt { l, set: |s| s.bridge = BridgeKind::Rope { railing: $n }, sel: |s| s.bridge == BridgeKind::Rope { railing: $n }, en: rope, show: yes }
        };
    }
    match n {
        0 => r!(0),
        1 => r!(1),
        2 => r!(2),
        _ => r!(3),
    }
}

/// Row A (form controls) and row B (swatch groups) for a tool. Erase has none (strip hidden).
pub fn rows(tool: Tool) -> (Vec<C>, Vec<C>) {
    match tool {
        Tool::Dyke => (
            vec![
                seg("", vec![o!("DRY STONE", wall_kind = WallKind::DryStone), o!("TURF", wall_kind = WallKind::Turf), o!("PALISADE", wall_kind = WallKind::Palisade)]),
                tog!("CREST", crest),
                tog!("TUMBLED", tumbled),
                sld!("THICK", wall_thickness, 0.3, 1.2, 0.1, 1, "M"),
                sld!("FOOTING", footing, 0.0, 2.0, 0.1, 1, "M"),
                weathering(),
            ],
            vec![sw_trim(palisade)],
        ),
        Tool::Windbreak => (
            vec![seg("", vec![o!("WATTLE", fence = FenceKind::Wattle), o!("DRIFTWOOD", fence = FenceKind::Driftwood), o!("NETS", fence = FenceKind::NetLine), o!("REED", fence = FenceKind::Reed)])],
            vec![sw_trim(painted_fence)],
        ),
        Tool::Hut => (
            vec![
                mat(yes),
                roofs(false),
                pitch(),
                tog!("CARVED", carved),
                tog!("HANGINGS", hangings),
                C::Sld(Sl { l: "STILTS", get: |s| s.stilts, set: |s, v| s.stilts = v, lo: 0.0, hi: 3.0, step: 0.1, dec: 1, unit: "M", zero: "AUTO" }),
                weathering(),
            ],
            vec![sw_roof(), sw_wall(), sw_trim(yes)],
        ),
        Tool::Roundhouse => (
            vec![mat(not_beacon), roofs(true), tog!("CARVED", carved), tog!("HANGINGS", hangings, not_beacon), weathering()],
            vec![sw_roof(), sw_wall(), sw_trim(yes)],
        ),
        Tool::Track => (
            vec![
                seg("", vec![o!("SHELL", path = PathKind::Shell), o!("PEBBLE", path = PathKind::Pebble), o!("FLAGS", path = PathKind::Flagstone), o!("BOARDS", path = PathKind::Boardwalk), o!("STEPPING", path = PathKind::Stepping)]),
                sld!("WIDTH", path_width, 0.8, 4.0, 0.2, 1, "M"),
            ],
            vec![],
        ),
        Tool::Steps => (vec![seg("", vec![o!("STONE", stairs = StairKind::Stone), o!("LOG", stairs = StairKind::Log), o!("LADDER", stairs = StairKind::RopeLadder)])], vec![sw_trim(ladder)]),
        Tool::Jetty => (vec![], vec![sw_trim(yes)]),
        Tool::Bridge => (
            vec![
                seg(
                    "",
                    vec![
                        Opt { l: "ROPE", set: |s| if !rope(s) { s.bridge = BridgeKind::Rope { railing: 0 } }, sel: rope, en: yes, show: yes },
                        o!("LOG", bridge = BridgeKind::Log),
                        o!("CAUSEWAY", bridge = BridgeKind::Causeway),
                    ],
                ),
                seg("RAILING", vec![railing(0, "ROPE"), railing(1, "NET"), railing(2, "RAILS"), railing(3, "FLOATS")]),
                seg("HANGING", vec![o!("NONE", hanging = Hanging::None, rope), o!("LANTERNS", hanging = Hanging::Lanterns, rope), o!("PENNANTS", hanging = Hanging::Pennants, rope)]),
            ],
            vec![sw_trim(yes)],
        ),
        Tool::Opening => (vec![seg("PLACE", vec![o!("SHUTTER", opening = OpeningKind::Window), o!("DOOR", opening = OpeningKind::Door), o!("WIDE", opening = OpeningKind::WideDoor)])], vec![]),
        Tool::Roof => (vec![roofs(false), pitch()], vec![sw_roof()]),
        Tool::Tree => (
            vec![seg("", vec![o!("PINE", tree = TreeKind::SeaPine), o!("PALM", tree = TreeKind::Palm), o!("CYPRESS", tree = TreeKind::Cypress), o!("FIG", tree = TreeKind::Fig), o!("SNAG", tree = TreeKind::Snag)])],
            vec![],
        ),
        Tool::Prop => (vec![C::Clutter], vec![sw_trim(painted_prop)]),
        Tool::Paint => (
            vec![
                seg("", vec![o!("PINKS", paint = PaintLayer::SeaPinks), o!("DUNE", paint = PaintLayer::DuneGrass), o!("WRACK", paint = PaintLayer::Wrack), o!("PEBBLES", paint = PaintLayer::Pebbles), o!("CLEAR", paint = PaintLayer::Clear)]),
                sld!("BRUSH", brush_radius, 1.0, 8.0, 0.5, 1, "M"),
            ],
            vec![],
        ),
        Tool::Raise => (vec![seg("", vec![o!("RAISE", lower = false), o!("LOWER", lower = true)]), radius(), strength()], vec![]),
        Tool::Flatten => (vec![radius(), strength()], vec![]),
        Tool::Lagoon => (vec![radius(), sld!("DEPTH", water_depth, 0.3, 1.5, 0.1, 1, "M")], vec![]),
        Tool::Dye => (
            vec![
                seg("HUT", vec![o!("WATTLE", material = Material::Wattle, dye_hut), o!("PLANK", material = Material::Plank, dye_hut), o!("STONE", material = Material::Stone, dye_hut)]),
                seg("DYKE", vec![o!("DRY STONE", wall_kind = WallKind::DryStone, dye_dyke), o!("TURF", wall_kind = WallKind::Turf, dye_dyke), o!("PALISADE", wall_kind = WallKind::Palisade, dye_dyke)]),
                tog!("CARVED", carved),
                weathering(),
            ],
            vec![sw_roof(), sw_wall(), sw_trim(yes)],
        ),
        Tool::Erase => (vec![], vec![]),
    }
}

/// The 14 prop cells: (kind, cell label, tooltip name), 7 per row.
pub const CLUTTER: [(ClutterKind, &str, &str); 14] = [
    (ClutterKind::Canoe, "CANOE", "OUTRIGGER CANOE"),
    (ClutterKind::DryingRack, "RACK", "FISH DRYING RACK"),
    (ClutterKind::Firepit, "FIREPIT", "FIREPIT"),
    (ClutterKind::Cairn, "CAIRN", "CAIRN"),
    (ClutterKind::StandingStone, "STONE", "CARVED STANDING STONE"),
    (ClutterKind::CarvedPole, "POLE", "CARVED POLE"),
    (ClutterKind::NetRack, "NETS", "NET RACK"),
    (ClutterKind::Baskets, "BASKETS", "BASKETS AND JARS"),
    (ClutterKind::LanternPost, "LANTERN", "LANTERN POST"),
    (ClutterKind::Pennant, "PENNANT", "PENNANT POLE"),
    (ClutterKind::Rock, "ROCK", "ROCK"),
    (ClutterKind::Bush, "BUSH", "GORSE AND FLAX BUSH"),
    (ClutterKind::CrabPots, "POTS", "CRAB POTS"),
    (ClutterKind::Woodpile, "WOOD", "DRIFTWOOD PILE"),
];

/// Toolbar tooltip: (title without the key tag, second line). The title gets ` [key]` appended.
pub fn tip(t: Tool) -> (&'static str, &'static str) {
    match t {
        Tool::Dyke => ("DYKE", "Drag a dry-stone, turf or palisade dyke"),
        Tool::Windbreak => ("WINDBREAK", "Drag wattle, driftwood, nets or reed"),
        Tool::Hut => ("HUT", "Drag a footprint, lift it, click to build"),
        Tool::Roundhouse => ("ROUNDHOUSE / BEACON", "Drag a circle; raise it tall for a beacon"),
        Tool::Track => ("TRACK", "Drag a track; it cuts gaps through dykes"),
        Tool::Steps => ("STEPS", "Drag from the ground up to a ledge"),
        Tool::Jetty => ("JETTY", "Drag a deck; over the sea canoes moor"),
        Tool::Bridge => ("BRIDGE", "Drag across water or between ledges"),
        Tool::Opening => ("OPENING", "Click a wall: shutter, door, boat door"),
        Tool::Roof => ("ROOF", "Click inside a closed dyke ring"),
        Tool::Tree => ("TREE", "Click to plant"),
        Tool::Prop => ("PROP", "Click to place a prop"),
        Tool::Paint => ("PAINT", "Brush sea pinks, dune grass, wrack, pebbles"),
        Tool::Raise => ("RAISE / LOWER", "Drag to raise; Shift to lower"),
        Tool::Flatten => ("FLATTEN", "Drag to level the ground"),
        Tool::Lagoon => ("LAGOON", "Drag to carve tide pools"),
        Tool::Dye => ("DYE", "Click to dye a structure; Shift+click picks"),
        Tool::Erase => ("ERASE", "Click or drag to remove"),
    }
}

/// Name plate text (the tool name; Roundhouse and Dye add live text in `ui`).
pub fn plate(t: Tool) -> &'static str {
    match t {
        Tool::Windbreak => "WINDBREAK",
        Tool::Roundhouse => "ROUNDHOUSE",
        Tool::Raise => "RAISE",
        _ => t.label(),
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
pub const GROUPS: [&str; 4] = ["BUILD", "DRESS", "LAND", "EDIT"];

/// Noon shadow hex of each swatch (DESIGN A1 "noon shadow" column), by palette slot (0 roof,
/// 1 wall, 2 trim), read from `palette::*_SHADES`. The HUD shows lit over shadow in every chip.
pub fn shadow(slot: u8, i: usize) -> u32 {
    use cairn_cove_core::palette::{ROOF_SHADES, TRIM_SHADES, WALL_SHADES};
    let t: &[(u32, u32)] = match slot {
        0 => &ROOF_SHADES,
        1 => &WALL_SHADES,
        _ => &TRIM_SHADES,
    };
    t.get(i).map_or(0x2910C5, |s| s.1)
}

/// (key chip, description) lines, at most 6 (the name plate is the first line).
pub fn hints(t: Tool) -> &'static [(&'static str, &'static str)] {
    match t {
        Tool::Dyke => &[("DRAG", "Lay a dyke; click adds points"), ("CLICK", "On the last point to end"), ("SCROLL", "Dyke height"), ("SHIFT", "Snap 45 deg, close the ring"), ("ESC", "Cancel")],
        Tool::Windbreak => &[("DRAG", "Lay a windbreak"), ("CLICK", "On the last point to end"), ("SCROLL", "Windbreak kind"), ("SHIFT", "Snap 45 deg"), ("ESC", "Cancel")],
        Tool::Hut => &[("DRAG", "Footprint"), ("SCROLL", "Rotate the rectangle"), ("SHIFT", "Force a square"), ("MOVE", "Height, after release"), ("CLICK", "Build"), ("ESC", "Cancel")],
        Tool::Roundhouse => &[("DRAG", "Circle footprint"), ("MOVE", "Height; tall makes a beacon"), ("SCROLL", "Roof style"), ("CLICK", "Build"), ("ESC", "Cancel")],
        Tool::Track => &[("DRAG", "Lay a track; click adds points"), ("CLICK", "On the last point to end"), ("SCROLL", "Track width"), ("SHIFT", "Snap 45 deg"), ("ESC", "Cancel")],
        Tool::Steps => &[("DRAG", "From the ground to a ledge"), ("SCROLL", "Width"), ("ENDS", "Snap to dyke tops, jetties, floors"), ("ESC", "Cancel")],
        Tool::Jetty => &[("DRAG", "Footprint"), ("MOVE", "Deck height, after release"), ("CLICK", "Build"), ("ESC", "Cancel")],
        Tool::Bridge => &[("DRAG", "From anchor A to B"), ("SCROLL", "Width"), ("ENDS", "Snap to dyke tops and jetties"), ("ESC", "Cancel")],
        Tool::Opening => &[("CLICK", "Place the chosen opening"), ("CLICK", "On an opening cycles it"), ("SHIFT", "Click removes an opening")],
        Tool::Roof => &[("CLICK", "Roof the hovered dyke ring"), ("SCROLL", "Pitch"), ("SHIFT", "Scroll changes roof style")],
        Tool::Tree => &[("CLICK", "Plant"), ("SCROLL", "Rotate"), ("SHIFT", "Scroll scales 0.7 to 1.3")],
        Tool::Prop => &[("CLICK", "Place the prop"), ("TAB", "Next prop"), ("SCROLL", "Rotate"), ("SHIFT", "Scroll scales 0.7 to 1.3")],
        Tool::Paint => &[("DRAG", "Brush the layer"), ("SCROLL", "Brush size"), ("SHIFT", "Erase")],
        Tool::Raise => &[("DRAG", "Raise the ground"), ("SCROLL", "Radius"), ("SHIFT", "Lower")],
        Tool::Flatten => &[("DRAG", "Level the ground"), ("SCROLL", "Radius")],
        Tool::Lagoon => &[("DRAG", "Carve a tide pool"), ("SCROLL", "Radius")],
        Tool::Dye => &[("CLICK", "Dye a structure with the strip"), ("SHIFT", "Click picks its colours"), ("TAB", "Next material")],
        Tool::Erase => &[("CLICK", "Remove a structure or prop"), ("DRAG", "Remove many")],
    }
}

/// Hint lines while the footprint is fixed and the mouse sets the height (DESIGN B9 "Raising").
pub fn raising_hints(t: Tool) -> &'static [(&'static str, &'static str)] {
    match t {
        Tool::Jetty => &[("MOVE", "Deck height"), ("SCROLL", "Nudge the height"), ("CLICK", "Build"), ("ESC", "Cancel")],
        Tool::Roundhouse => &[("MOVE", "Height; tall makes a beacon"), ("SCROLL", "Roof style"), ("CLICK", "Build"), ("ESC", "Cancel")],
        _ => &[("MOVE", "Height"), ("SCROLL", "Pitch"), ("SHIFT+SCROLL", "Roof style"), ("CLICK", "Build"), ("ESC", "Cancel")],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// Every Opt reachable from a tool's strip, with its settings after pressing it.
    fn reach(tool: Tool) -> Vec<S> {
        let (a, b) = rows(tool);
        let mut out = Vec::new();
        for c in a.iter().chain(&b) {
            let opts: Vec<Opt> = match c {
                C::Seg(_, o) => o.clone(),
                C::Tog(o) => vec![*o],
                _ => vec![],
            };
            for o in opts {
                let mut s = S::default();
                (o.set)(&mut s);
                assert!((o.sel)(&s) || matches!(c, C::Tog(_)), "{tool:?} {} not selected after press", o.l);
                out.push(s);
            }
        }
        out
    }

    #[test]
    fn strip_reaches_every_variant() {
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for t in Tool::ALL {
            for s in reach(t) {
                seen.insert(format!("{:?}", s.wall_kind));
                seen.insert(format!("{:?}", s.material));
                seen.insert(format!("{:?}", s.roof));
                seen.insert(format!("{:?}", s.fence));
                seen.insert(format!("{:?}", s.path));
                seen.insert(format!("{:?}", s.stairs));
                seen.insert(format!("{:?}", s.bridge));
                seen.insert(format!("{:?}", s.hanging));
                seen.insert(format!("{:?}", s.opening));
                seen.insert(format!("{:?}", s.tree));
                seen.insert(format!("{:?}", s.paint));
            }
        }
        for want in [
            "DryStone", "Turf", "Palisade", "Wattle", "Plank", "Stone", "Thatch", "Hide", "Boat", "Driftwood", "NetLine", "Reed", "Shell", "Pebble", "Flagstone", "Boardwalk", "Stepping", "Log",
            "RopeLadder", "Rope { railing: 0 }", "Rope { railing: 1 }", "Rope { railing: 2 }", "Rope { railing: 3 }", "Causeway", "Lanterns", "Pennants", "Window", "Door", "WideDoor", "SeaPine", "Palm",
            "Cypress", "Fig", "Snag", "SeaPinks", "DuneGrass", "Wrack", "Pebbles", "Clear",
        ] {
            assert!(seen.contains(want), "no strip control reaches {want}");
        }
    }

    #[test]
    fn toggles_and_sliders_exist_per_spec() {
        let labels = |t: Tool| -> Vec<&'static str> {
            let (a, b) = rows(t);
            a.iter().chain(&b).filter_map(|c| match c {
                C::Tog(o) => Some(o.l),
                C::Sld(s) => Some(s.l),
                C::Sw(_, l, _) => Some(*l),
                _ => None,
            }).collect()
        };
        assert_eq!(labels(Tool::Dyke), ["CREST", "TUMBLED", "THICK", "FOOTING", "WEATHER", "TRIM"]);
        assert_eq!(labels(Tool::Hut), ["PITCH", "CARVED", "HANGINGS", "STILTS", "WEATHER", "ROOF", "WALL", "TRIM"]);
        assert_eq!(labels(Tool::Dye), ["CARVED", "WEATHER", "ROOF", "WALL", "TRIM"]);
        assert_eq!(labels(Tool::Track), ["WIDTH"]);
        assert!(rows(Tool::Erase).0.is_empty() && rows(Tool::Erase).1.is_empty());
    }

    #[test]
    fn prop_grid_has_every_clutter_kind_once() {
        let kinds: BTreeSet<String> = CLUTTER.iter().map(|c| format!("{:?}", c.0)).collect();
        assert_eq!(kinds.len(), 14);
    }

    #[test]
    fn tables_cover_every_tool() {
        for t in Tool::ALL {
            assert!(!tip(t).0.is_empty() && !t.glyph().is_empty() && !plate(t).is_empty());
            assert!(hints(t).len() <= 6 && !hints(t).is_empty(), "{t:?}");
            assert!(raising_hints(t).len() <= 6);
            assert!(tool_group(t) < GROUPS.len());
        }
    }

    #[test]
    fn roundhouse_strip_follows_beacon_ghost() {
        let (a, _) = rows(Tool::Roundhouse);
        let C::Seg(_, mats) = &a[0] else { panic!() };
        let mut s = S::default();
        assert!((mats[0].en)(&s));
        s.ghost_beacon = true;
        assert!(!(mats[0].en)(&s));
        let C::Seg(_, roofs) = &a[1] else { panic!() };
        assert!(!(roofs[3].en)(&s), "BOAT roof is disabled on round footprints");
    }

    #[test]
    fn dye_pick_enables_only_the_picked_material() {
        let (a, _) = rows(Tool::Dye);
        let (C::Seg(_, hut), C::Seg(_, dyke)) = (&a[0], &a[1]) else { panic!() };
        let mut s = S::default();
        assert!((hut[0].en)(&s) && (dyke[0].en)(&s));
        s.dye = Some("HUT");
        assert!((hut[0].en)(&s) && !(dyke[0].en)(&s));
        s.dye = Some("DYKE");
        assert!(!(hut[0].en)(&s) && (dyke[0].en)(&s));
        s.dye = Some("TRACK");
        assert!(!(hut[0].en)(&s) && !(dyke[0].en)(&s));
    }

    #[test]
    fn bridge_extras_only_for_rope() {
        let (a, _) = rows(Tool::Bridge);
        let C::Seg(_, rail) = &a[1] else { panic!() };
        let mut s = S::default();
        assert!(rail.iter().all(|o| (o.en)(&s)));
        s.bridge = BridgeKind::Log;
        assert!(rail.iter().all(|o| !(o.en)(&s)));
    }

    #[test]
    fn beacon_roundhouse_offers_cone_or_open_and_no_hangings() {
        let (a, _) = rows(Tool::Roundhouse);
        let C::Seg(_, roofs) = &a[1] else { panic!() };
        let C::Tog(hang) = &a[3] else { panic!() };
        let mut s = S::default();
        let shown = |s: &S| roofs.iter().filter(|o| (o.show)(s)).map(|o| o.l).collect::<Vec<_>>();
        assert_eq!(shown(&s), ["THATCH", "TURF", "HIDE", "BOAT"]);
        assert!((hang.en)(&s));
        s.ghost_beacon = true;
        assert_eq!(shown(&s), ["CONE", "OPEN"]);
        assert!(!(hang.en)(&s), "no hangings on a beacon");
        let (cone, open) = (&roofs[4], &roofs[5]);
        (open.set)(&mut s);
        assert!((open.sel)(&s) && !(cone.sel)(&s));
        (cone.set)(&mut s);
        assert!((cone.sel)(&s) && !(open.sel)(&s));
    }

    #[test]
    fn trim_swatches_follow_what_each_variant_paints() {
        let trim_en = |t: Tool, f: &dyn Fn(&mut S)| {
            let (_, b) = rows(t);
            let C::Sw(2, _, en) = b.iter().find(|c| matches!(c, C::Sw(2, ..))).unwrap() else { panic!() };
            let mut s = S::default();
            f(&mut s);
            en(&s)
        };
        assert!(trim_en(Tool::Windbreak, &|s| s.fence = FenceKind::NetLine));
        for k in [FenceKind::Wattle, FenceKind::Driftwood, FenceKind::Reed] {
            assert!(!trim_en(Tool::Windbreak, &|s| s.fence = k), "{k:?}");
        }
        assert!(trim_en(Tool::Dyke, &|s| s.wall_kind = WallKind::Palisade));
        assert!(!trim_en(Tool::Dyke, &|s| s.wall_kind = WallKind::DryStone) && !trim_en(Tool::Dyke, &|s| s.wall_kind = WallKind::Turf));
        assert!(trim_en(Tool::Steps, &|s| s.stairs = StairKind::RopeLadder) && !trim_en(Tool::Steps, &|s| s.stairs = StairKind::Stone));
        for k in [ClutterKind::DryingRack, ClutterKind::NetRack, ClutterKind::LanternPost, ClutterKind::Bush, ClutterKind::Canoe] {
            assert!(trim_en(Tool::Prop, &|s| s.clutter = k), "{k:?}");
        }
        for k in [ClutterKind::Firepit, ClutterKind::Cairn, ClutterKind::Rock, ClutterKind::Woodpile] {
            assert!(!trim_en(Tool::Prop, &|s| s.clutter = k), "{k:?}");
        }
    }

    #[test]
    fn chip_shadows_come_from_the_palette() {
        use cairn_cove_core::palette::{ROOF_SHADES, TRIM_SHADES, WALL_SHADES};
        for (slot, t) in [(0u8, &ROOF_SHADES[..]), (1, &WALL_SHADES[..]), (2, &TRIM_SHADES[..])] {
            for (i, (_, shadow_hex)) in t.iter().enumerate() {
                assert_eq!(shadow(slot, i), *shadow_hex, "slot {slot} swatch {i}");
            }
        }
    }

    #[test]
    fn every_swatch_has_a_shadow() {
        use cairn_cove_core::palette::{ROOF_SWATCHES, TRIM_SWATCHES, WALL_SWATCHES};
        // The fallback (indigo shadow) is only for out-of-range indices; in range they differ per swatch.
        for (slot, n) in [(0u8, ROOF_SWATCHES.len()), (1, WALL_SWATCHES.len()), (2, TRIM_SWATCHES.len())] {
            let all: BTreeSet<u32> = (0..n).map(|i| shadow(slot, i)).collect();
            assert!(all.len() >= n - 1, "slot {slot}");
        }
    }
}
