//! VariantStrip contents per tool (DESIGN §2) and the HintsPanel text (DESIGN §3).

use crate::tools::{DrawState, Tool, ToolHover, ToolSettings};
use crate::weather::Weather;
use glade_core::{ClutterKind, FenceKind, Hanging, Material, PaintLayer, PathKind, RoofStyle, StairKind, TreeKind};

/// A segmented control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seg {
    WallMaterial,
    HouseMaterial,
    HouseRoof,
    Fence,
    Path,
    Stairs,
    Bridge,
    Railing,
    Hanging,
    RoofTool,
    Tree,
    Clutter,
    Paint,
}

const MATERIALS: [Material; 3] = [Material::Stone, Material::Plaster, Material::Wood];
const ROOFS: [RoofStyle; 4] = [RoofStyle::Gable, RoofStyle::Hip, RoofStyle::Cone, RoofStyle::Flat];
const FENCES: [FenceKind; 4] = [FenceKind::WoodRail, FenceKind::Picket, FenceKind::Stone, FenceKind::Wattle];
const PATHS: [PathKind; 4] = [PathKind::Dirt, PathKind::Cobble, PathKind::Flagstone, PathKind::Setts];
const STAIRS: [StairKind; 3] = [StairKind::Wood, StairKind::Stone, StairKind::Ladder];
const HANGING: [Hanging; 3] = [Hanging::None, Hanging::Lanterns, Hanging::Flags];
const TREES: [TreeKind; 4] = [TreeKind::Oak, TreeKind::Pine, TreeKind::Birch, TreeKind::Fruit];
pub const CLUTTER: [ClutterKind; 14] = [
    ClutterKind::Barrel,
    ClutterKind::Crate,
    ClutterKind::Haystack,
    ClutterKind::Well,
    ClutterKind::Cart,
    ClutterKind::Bench,
    ClutterKind::LanternPost,
    ClutterKind::FlagPole,
    ClutterKind::WashingLine,
    ClutterKind::Woodpile,
    ClutterKind::Rock,
    ClutterKind::Bush,
    ClutterKind::Beehive,
    ClutterKind::Scarecrow,
];
const PAINTS: [PaintLayer; 5] = [PaintLayer::Flowers, PaintLayer::Bushes, PaintLayer::Grain, PaintLayer::Dirt, PaintLayer::Clear];

fn idx<T: PartialEq>(all: &[T], v: &T) -> u8 {
    all.iter().position(|x| x == v).unwrap_or(0) as u8
}

impl Seg {
    pub fn labels(self) -> &'static [&'static str] {
        match self {
            Seg::WallMaterial => &["Stone", "Wood"],
            Seg::HouseMaterial => &["Stone", "Plaster", "Wood"],
            Seg::HouseRoof | Seg::RoofTool => &["Gable", "Hip", "Cone", "Flat"],
            Seg::Fence => &["WoodRail", "Picket", "Stone", "Wattle"],
            Seg::Path => &["Dirt", "Cobble", "Flagstone", "Setts"],
            Seg::Stairs => &["Wood", "Stone", "Ladder"],
            Seg::Bridge => &["Rope", "Stone", "Wood"],
            Seg::Railing => &["1", "2", "3", "4"],
            Seg::Hanging => &["None", "Lanterns", "Flags"],
            Seg::Tree => &["Oak", "Pine", "Birch", "Fruit"],
            Seg::Clutter => &[
                "Barrel", "Crate", "Haystack", "Well", "Cart", "Bench", "Lantern", "Flag", "Washing", "Woodpile", "Rock",
                "Bush", "Beehive", "Scarecrow",
            ],
            Seg::Paint => &["Flowers", "Bushes", "Grain", "Dirt", "Clear"],
        }
    }

    pub fn get(self, s: &ToolSettings, tool: Tool) -> u8 {
        match self {
            Seg::WallMaterial => (s.wall_material == Material::Wood) as u8,
            Seg::HouseMaterial => idx(&MATERIALS, &s.house_style(tool).material),
            Seg::HouseRoof => idx(&ROOFS, &s.house_style(tool).roof),
            Seg::Fence => idx(&FENCES, &s.fence_kind),
            Seg::Path => idx(&PATHS, &s.path_kind),
            Seg::Stairs => idx(&STAIRS, &s.stair_kind),
            Seg::Bridge => s.bridge_kind,
            Seg::Railing => s.bridge_railing,
            Seg::Hanging => idx(&HANGING, &s.hanging),
            Seg::RoofTool => idx(&ROOFS, &s.roof_style),
            Seg::Tree => idx(&TREES, &s.tree_kind),
            Seg::Clutter => idx(&CLUTTER, &s.clutter_kind),
            Seg::Paint => idx(&PAINTS, &s.paint_layer),
        }
    }

    pub fn set(self, s: &mut ToolSettings, tool: Tool, i: u8) {
        let i = i as usize;
        let house = if tool == Tool::Tower { &mut s.tower } else { &mut s.house };
        match self {
            Seg::WallMaterial => s.wall_material = if i == 1 { Material::Wood } else { Material::Stone },
            Seg::HouseMaterial => house.material = MATERIALS[i % 3],
            Seg::HouseRoof => house.roof = ROOFS[i % 4],
            Seg::Fence => s.fence_kind = FENCES[i % 4],
            Seg::Path => s.path_kind = PATHS[i % 4],
            Seg::Stairs => s.stair_kind = STAIRS[i % 3],
            Seg::Bridge => s.bridge_kind = (i % 3) as u8,
            Seg::Railing => s.bridge_railing = (i % 4) as u8,
            Seg::Hanging => s.hanging = HANGING[i % 3],
            Seg::RoofTool => s.roof_style = ROOFS[i % 4],
            Seg::Tree => s.tree_kind = TREES[i % 4],
            Seg::Clutter => s.clutter_kind = CLUTTER[i % 14],
            Seg::Paint => s.paint_layer = PAINTS[i % 5],
        }
    }

    pub fn enabled(self, s: &ToolSettings) -> bool {
        match self {
            Seg::Railing | Seg::Hanging => s.bridge_kind == 0,
            _ => true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flag {
    WallBattlements,
    WallRuined,
    HalfTimber,
    HouseBattlements,
}

impl Flag {
    pub fn label(self) -> &'static str {
        match self {
            Flag::WallBattlements | Flag::HouseBattlements => "Battlements",
            Flag::WallRuined => "Ruined",
            Flag::HalfTimber => "Half-timber",
        }
    }

    pub fn get(self, s: &ToolSettings, tool: Tool) -> bool {
        match self {
            Flag::WallBattlements => s.wall_battlements,
            Flag::WallRuined => s.wall_ruined,
            Flag::HalfTimber => s.house_style(tool).half_timber,
            Flag::HouseBattlements => s.house_style(tool).battlements,
        }
    }

    pub fn toggle(self, s: &mut ToolSettings, tool: Tool) {
        let house = if tool == Tool::Tower { &mut s.tower } else { &mut s.house };
        match self {
            Flag::WallBattlements => s.wall_battlements ^= true,
            Flag::WallRuined => s.wall_ruined ^= true,
            Flag::HalfTimber => house.half_timber ^= true,
            Flag::HouseBattlements => house.battlements ^= true,
        }
    }

    pub fn enabled(self, s: &ToolSettings, tool: Tool) -> bool {
        match self {
            Flag::HouseBattlements => s.house_style(tool).roof == RoofStyle::Flat,
            _ => true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SliderField {
    WallFoundation,
    HouseFoundation,
    PathWidth,
    RoofPitch,
    BrushRadius,
    BrushStrength,
    Fog,
    Snow,
}

impl SliderField {
    pub fn label(self) -> &'static str {
        match self {
            SliderField::WallFoundation | SliderField::HouseFoundation => "Foundation",
            SliderField::PathWidth => "Width",
            SliderField::RoofPitch => "Pitch",
            SliderField::BrushRadius => "Radius",
            SliderField::BrushStrength => "Strength",
            SliderField::Fog => "Fog",
            SliderField::Snow => "Amount",
        }
    }

    pub fn range(self) -> (f32, f32) {
        match self {
            SliderField::WallFoundation | SliderField::HouseFoundation => (0.0, 2.0),
            SliderField::PathWidth => (0.8, 4.0),
            SliderField::RoofPitch => (0.0, 2.5),
            SliderField::BrushRadius => (1.0, 8.0),
            SliderField::BrushStrength | SliderField::Snow => (0.0, 1.0),
            SliderField::Fog => (0.0, 0.08),
        }
    }

    pub fn get(self, s: &ToolSettings, tool: Tool, w: &Weather, snow_amount: f32) -> f32 {
        match self {
            SliderField::WallFoundation => s.wall_foundation,
            SliderField::HouseFoundation => s.house_style(tool).foundation,
            SliderField::PathWidth => s.path_width,
            SliderField::RoofPitch => s.roof_pitch,
            SliderField::BrushRadius => s.brush_radius,
            SliderField::BrushStrength => s.brush_strength,
            SliderField::Fog => w.fog_extra,
            SliderField::Snow => w.snow_override.unwrap_or(snow_amount),
        }
    }

    /// Sets the value; `snow_amount` remembers the snow slider while the toggle is off.
    pub fn set(self, s: &mut ToolSettings, tool: Tool, w: &mut Weather, snow_amount: &mut f32, v: f32) {
        let (lo, hi) = self.range();
        let v = v.clamp(lo, hi);
        let house = if tool == Tool::Tower { &mut s.tower } else { &mut s.house };
        match self {
            SliderField::WallFoundation => s.wall_foundation = v,
            SliderField::HouseFoundation => house.foundation = v,
            SliderField::PathWidth => s.path_width = v,
            SliderField::RoofPitch => s.roof_pitch = v,
            SliderField::BrushRadius => s.brush_radius = v,
            SliderField::BrushStrength => s.brush_strength = v,
            SliderField::Fog => w.fog_extra = v,
            SliderField::Snow => {
                *snow_amount = v;
                if w.snow_override.is_some() {
                    w.snow_override = Some(v);
                }
            }
        }
    }

    pub fn format(self, v: f32) -> String {
        match self {
            SliderField::BrushStrength | SliderField::Fog | SliderField::Snow => {
                let (lo, hi) = self.range();
                format!("{:.0}%", (v - lo) / (hi - lo) * 100.0)
            }
            SliderField::RoofPitch => format!("{v:.1}"),
            _ => format!("{v:.1}m"),
        }
    }
}

/// Which colour palette a swatch row edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Palette {
    Roof,
    Wall,
    Trim,
}

impl Palette {
    pub fn count(self) -> u8 {
        match self {
            Palette::Roof => glade_core::ROOF_COLORS,
            Palette::Wall => glade_core::WALL_COLORS,
            Palette::Trim => glade_core::TRIM_COLORS,
        }
    }
}

/// One control group in the strip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Item {
    Segs(Seg),
    Toggle(Flag),
    Slider(SliderField),
    Swatches(Palette),
    /// Clutter: 14-cell grid, 7 per row.
    Grid,
    /// Style tool: the three copied colours (display only).
    StyleShow,
}

pub fn strip_items(tool: Tool) -> Vec<Item> {
    use Item::*;
    let colours = [Swatches(Palette::Roof), Swatches(Palette::Wall), Swatches(Palette::Trim)];
    let mut v = match tool {
        Tool::Wall => vec![Segs(Seg::WallMaterial), Toggle(Flag::WallBattlements), Toggle(Flag::WallRuined), Slider(SliderField::WallFoundation)],
        Tool::House | Tool::Tower => vec![
            Segs(Seg::HouseMaterial),
            Segs(Seg::HouseRoof),
            Toggle(Flag::HalfTimber),
            Toggle(Flag::HouseBattlements),
            Slider(SliderField::HouseFoundation),
        ],
        Tool::Fence | Tool::Stairs | Tool::Platform | Tool::Bridge => {
            let mut v = match tool {
                Tool::Fence => vec![Segs(Seg::Fence)],
                Tool::Stairs => vec![Segs(Seg::Stairs)],
                Tool::Bridge => vec![Segs(Seg::Bridge), Segs(Seg::Railing), Segs(Seg::Hanging)],
                _ => vec![],
            };
            v.push(Swatches(Palette::Trim));
            return v;
        }
        Tool::Path => return vec![Segs(Seg::Path), Slider(SliderField::PathWidth)],
        Tool::Roof => return vec![Segs(Seg::RoofTool), Slider(SliderField::RoofPitch), Swatches(Palette::Roof)],
        Tool::Tree => return vec![Segs(Seg::Tree)],
        Tool::Clutter => return vec![Grid],
        Tool::Paint => return vec![Segs(Seg::Paint), Slider(SliderField::BrushRadius)],
        Tool::Sculpt | Tool::Flatten | Tool::Water => return vec![Slider(SliderField::BrushRadius), Slider(SliderField::BrushStrength)],
        Tool::Style => return vec![StyleShow],
        Tool::Eraser | Tool::Opening => return vec![],
    };
    v.extend(colours);
    v
}

/// HintsPanel lines `(key, action)` for the tool in its current state (≤ 7).
pub fn hint_lines(tool: Tool, state: DrawState, hover: &ToolHover, s: &ToolSettings) -> Vec<(String, String)> {
    let l = |k: &str, a: &str| (k.to_string(), a.to_string());
    let scroll = match tool {
        Tool::Wall => format!("height {:.1} m", s.wall_height),
        Tool::Path => format!("width {:.1} m", s.path_width),
        Tool::Fence => "fence kind".into(),
        Tool::House | Tool::Tower => "rotate".into(),
        Tool::Platform => format!("height {:.1} m", s.platform_height),
        Tool::Stairs => format!("width {:.1} m", s.stair_width),
        Tool::Bridge => format!("width {:.1} m", s.bridge_width),
        _ => format!("radius {:.1} m", s.brush_radius),
    };
    let drawing = hover.busy && state != DrawState::Raising;
    let mut v: Vec<(String, String)> = if state == DrawState::Raising {
        let hs = s.house_style(tool);
        vec![
            l("Move", &format!("height {:.1} m", hs.height)),
            l("Scroll", &format!("pitch {:.1}", hs.roof_pitch)),
            l("Shift+Scroll", "style"),
            l("Click", "place"),
            l("Esc / RMB", "cancel"),
        ]
    } else if drawing {
        match tool {
            Tool::Wall | Tool::Fence | Tool::Path => vec![
                l("Release", "finish"),
                l("Click", "add a point"),
                ("Scroll".into(), scroll),
                l("Shift", if tool == Tool::Wall { "snap 45 deg, close ring" } else { "snap 45 deg" }),
                l("Esc / RMB", "cancel"),
            ],
            Tool::House | Tool::Tower | Tool::Platform => vec![
                l("Release", if tool == Tool::Platform { "place" } else { "then raise" }),
                ("Scroll".into(), scroll),
                l("Shift", "square"),
                l("Esc / RMB", "cancel"),
            ],
            Tool::Stairs | Tool::Bridge => {
                vec![l("Release", "place"), ("Scroll".into(), scroll), l("Shift", "snap to wall top"), l("Esc / RMB", "cancel")]
            }
            Tool::Eraser => vec![l("Release", "remove touched"), l("Esc / RMB", "cancel")],
            _ => vec![l("Release", "finish stroke"), ("Scroll".into(), scroll), l("Esc / RMB", "cancel")],
        }
    } else {
        match tool {
            Tool::Wall => vec![l("Drag", "draw a wall"), l("Click", "place points"), ("Scroll".into(), scroll), l("Shift", "snap 45 deg, close ring")],
            Tool::Fence => vec![l("Drag", "draw a fence"), l("Click", "place points"), ("Scroll".into(), scroll), l("Shift", "snap 45 deg")],
            Tool::Path => vec![l("Drag", "draw a path"), l("Click", "place points"), ("Scroll".into(), scroll), l("Shift", "snap 45 deg")],
            Tool::House => vec![l("Drag", "footprint, then raise"), l("Scroll", "rotate (dragging)"), l("Shift", "square")],
            Tool::Tower => vec![l("Drag", "circle, then raise"), l("Move", "height after release")],
            Tool::Platform => vec![l("Drag", "deck rectangle"), ("Scroll".into(), scroll)],
            Tool::Stairs => vec![l("Drag", "from low to high"), ("Scroll".into(), scroll), l("Shift", "snap to wall top")],
            Tool::Bridge => vec![l("Drag", "across a gap"), ("Scroll".into(), scroll), l("Shift", "snap to wall top")],
            Tool::Opening => vec![l("Click", "add a window"), l("On opening", "window>door>wide"), l("Shift+Click", "remove")],
            Tool::Roof => vec![l("Click ring", "roof / unroof"), l("Click house", "next roof style"), l("Scroll", "pitch"), l("Shift+Scroll", "style")],
            Tool::Tree | Tool::Clutter => vec![l("Click", "plant"), l("Scroll", "rotate"), l("Shift+Scroll", &format!("scale {:.2}", s.prop_scale))],
            Tool::Paint => vec![l("Drag", "paint"), l("Shift+Drag", "erase"), l("Scroll", "radius (painting)")],
            Tool::Sculpt => vec![l("Drag", "raise ground"), l("Shift+Drag", "lower"), l("Scroll", "radius (dragging)")],
            Tool::Flatten => vec![l("Drag", "level to start height"), l("Scroll", "radius (dragging)")],
            Tool::Water => vec![l("Drag", "carve water"), l("Scroll", "radius (dragging)")],
            Tool::Style => vec![l("Click", "copy its style"), l("Shift+Click", "apply copied style")],
            Tool::Eraser => vec![l("Click", "remove"), l("Drag", "remove all touched")],
        }
    };
    if let Some(t) = hover.target {
        v.push(l("Over", t));
    }
    if !drawing && state != DrawState::Raising {
        v.push(l("Ctrl+Z / Y", "undo / redo"));
        v.push(l("H", "hide hints"));
    }
    if let Some(msg) = hover.invalid {
        v.insert(0, l("!", msg));
    }
    v.truncate(7);
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::default;

    #[test]
    fn segments_round_trip() {
        let mut s = ToolSettings::default();
        for seg in [Seg::HouseMaterial, Seg::HouseRoof, Seg::Fence, Seg::Path, Seg::Stairs, Seg::Bridge, Seg::Clutter, Seg::Paint] {
            for i in 0..seg.labels().len() as u8 {
                seg.set(&mut s, Tool::Tower, i);
                assert_eq!(seg.get(&s, Tool::Tower), i, "{seg:?}");
            }
        }
        assert_eq!(Seg::Clutter.labels().len(), CLUTTER.len());
    }

    #[test]
    fn every_tool_has_hints() {
        let s = ToolSettings::default();
        for t in Tool::ALL {
            for (st, busy) in [(DrawState::Idle, false), (DrawState::Drawing, true), (DrawState::Raising, true)] {
                let h = hint_lines(t, st, &ToolHover { busy, ..default() }, &s);
                assert!(!h.is_empty() && h.len() <= 7);
            }
        }
        let inv = ToolHover { invalid: Some("Too short"), target: None, busy: true };
        assert_eq!(hint_lines(Tool::Wall, DrawState::Invalid, &inv, &s)[0].1, "Too short");
    }
}
