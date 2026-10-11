//! The document: an ordered list of terrain strokes and structures, plus undo history.
//!
//! Everything here is plain data. `crate::scene::build` turns a [`Document`] into meshes.
//! World units are metres, `y` is up, the glade spans `x, z ∈ [-GLADE_HALF, GLADE_HALF]`.

use glam::{Vec2, Vec3};

/// Half the side length of the buildable glade, in metres.
pub const GLADE_HALF: f32 = 32.0;

/// Stable identifier of a structure inside a document.
pub type StructureId = u32;

// ---------------------------------------------------------------------------------------------
// Terrain
// ---------------------------------------------------------------------------------------------

/// What a terrain brush dab does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TerrainOp {
    /// Raise (positive) or lower (negative) the ground by up to `amount` metres at the centre.
    Sculpt { amount: f32 },
    /// Pull the ground towards `height` with the given strength in 0..=1.
    Flatten { height: f32, strength: f32 },
    /// Carve water: lowers the bed and marks it wet. `depth` metres at the centre.
    Water { depth: f32 },
    /// Paint a vegetation / ground layer with strength in 0..=1 (negative erases).
    Paint { layer: PaintLayer, strength: f32 },
}

/// Paintable ground layers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PaintLayer {
    /// Meadow flowers (mixed colours).
    Flowers,
    /// Round bushes.
    Bushes,
    /// Golden grain / wheat.
    Grain,
    /// Bare dirt.
    Dirt,
    /// Remove all painted layers.
    Clear,
}

/// One brush dab. A drag stroke is a sequence of dabs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerrainDab {
    pub center: Vec2,
    /// Radius of the brush in metres. Falloff is smooth (cosine) to zero at the radius.
    pub radius: f32,
    pub op: TerrainOp,
}

// ---------------------------------------------------------------------------------------------
// Shared style enums
// ---------------------------------------------------------------------------------------------

/// Main building material of walls and houses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Material {
    /// Rough fieldstone blocks.
    #[default]
    Stone,
    /// Lime-washed plaster over stone (houses). Walls treat it as stone.
    Plaster,
    /// Vertical wooden planks.
    Wood,
}

/// Roof shapes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum RoofStyle {
    /// Two slopes, gable ends (rectangles). Round footprints fall back to `Cone`.
    #[default]
    Gable,
    /// Four slopes meeting in a ridge.
    Hip,
    /// Steep cone (round footprints) or pyramid (rectangles).
    Cone,
    /// Flat roof with a low parapet; battlements if the structure asks for them.
    Flat,
}

/// Palette indices. The renderer owns the actual colours (see `palette` module).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Colors {
    /// Index into the roof palette (0..ROOF_COLORS).
    pub roof: u8,
    /// Index into the wall/plaster palette (0..WALL_COLORS).
    pub wall: u8,
    /// Index into the wood/trim palette (0..TRIM_COLORS), used by timber, doors, shutters, railings.
    pub trim: u8,
}

impl Default for Colors {
    fn default() -> Self {
        Self { roof: 0, wall: 0, trim: 0 }
    }
}

pub const ROOF_COLORS: u8 = 6;
pub const WALL_COLORS: u8 = 5;
pub const TRIM_COLORS: u8 = 5;

/// A hole the player placed in a wall or house wall.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Opening {
    /// Distance in metres along the host's centre polyline (walls) or perimeter (houses,
    /// measured along `geom::footprint_polygon` order from its first corner).
    pub along: f32,
    pub kind: OpeningKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OpeningKind {
    /// Small shuttered window, ~0.8 m wide, sill at 1.0 m.
    Window,
    /// Wooden door, ~1.1 m wide, 2.0 m tall (arched top on stone).
    Door,
    /// Wide doorway / arch, ~2.4 m wide.
    WideDoor,
}

// ---------------------------------------------------------------------------------------------
// Structures
// ---------------------------------------------------------------------------------------------

/// Footprint of a house or tower.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Footprint {
    /// Oriented rectangle. `half` are half extents along the local x/z axes, rotated by `angle`
    /// radians in the (x, z) plane, turning +x towards +z (`Vec2::from_angle`).
    Rect { center: Vec2, half: Vec2, angle: f32 },
    /// Circle. A tall round house is a tower.
    Round { center: Vec2, radius: f32 },
}

/// Wall: drawn as a polyline. A closed polyline (first == last point) encloses a courtyard
/// that the roof tool can cover.
#[derive(Clone, Debug, PartialEq)]
pub struct Wall {
    /// Centre line on the ground plane, at least 2 points. Closed if first ≈ last.
    pub points: Vec<Vec2>,
    /// Height above the ground at each point (top of the wall walk, without battlements).
    pub height: f32,
    /// Thickness in metres (0.4 .. 2.0).
    pub thickness: f32,
    pub material: Material,
    /// Merlons along the top.
    pub battlements: bool,
    /// Jagged, broken top (ruins; the snip tool).
    pub ruined: bool,
    /// Height of a darker stone plinth/foundation (0 = none).
    pub foundation: f32,
    /// Closed walls only: put a roof over the enclosed area.
    pub roofed: bool,
    pub roof: RoofStyle,
    pub roof_pitch: f32,
    pub openings: Vec<Opening>,
    pub colors: Colors,
}

/// House (rect) or tower (round). Walls rise from the ground (flattened under the footprint).
#[derive(Clone, Debug, PartialEq)]
pub struct House {
    pub footprint: Footprint,
    /// Eave height above ground. 0.6 ..= 24. Low values make tent-like shelters.
    pub wall_height: f32,
    pub roof: RoofStyle,
    /// Roof rise as a fraction of the footprint's smaller half extent / radius (0 = flat).
    pub roof_pitch: f32,
    pub material: Material,
    /// Exposed timber framing on upper storeys (plaster/wood houses).
    pub half_timber: bool,
    /// Stone plinth height (0 = none).
    pub foundation: f32,
    /// Flat roofs only: crenellations.
    pub battlements: bool,
    /// Player-placed openings. If empty, windows and one door are generated automatically.
    pub openings: Vec<Opening>,
    pub colors: Colors,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum FenceKind {
    /// Wooden posts with two rails.
    #[default]
    WoodRail,
    /// Pointed wooden pickets.
    Picket,
    /// Low dry-stone wall (~0.8 m).
    Stone,
    /// Woven wattle hurdles.
    Wattle,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fence {
    pub points: Vec<Vec2>,
    pub kind: FenceKind,
    pub colors: Colors,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PathKind {
    /// Trodden dirt.
    #[default]
    Dirt,
    /// Rounded cobbles.
    Cobble,
    /// Large irregular flagstones.
    Flagstone,
    /// Neat rectangular setts in a herringbone-ish pattern.
    Setts,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Path {
    pub points: Vec<Vec2>,
    /// Width in metres (0.8 .. 4.0).
    pub width: f32,
    pub kind: PathKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum StairKind {
    /// Wooden steps with railings (railing colour = `Colors::trim`).
    #[default]
    Wood,
    /// Solid stone steps.
    Stone,
    /// Wooden ladder.
    Ladder,
}

/// Stairs between two 3-D points (from the ground to a wall top, between levels…).
#[derive(Clone, Debug, PartialEq)]
pub struct Stairs {
    pub from: Vec3,
    pub to: Vec3,
    pub width: f32,
    pub kind: StairKind,
    pub colors: Colors,
}

/// Wooden platform on posts.
#[derive(Clone, Debug, PartialEq)]
pub struct Platform {
    pub center: Vec2,
    pub half: Vec2,
    pub angle: f32,
    /// Deck height above the lowest ground under it.
    pub height: f32,
    pub colors: Colors,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BridgeKind {
    /// Sagging rope bridge with planks; `railing` 0..4 picks one of four railing designs.
    Rope { railing: u8 },
    /// Arched stone bridge.
    Stone,
    /// Flat wooden bridge on trestles.
    Wood,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Hanging {
    #[default]
    None,
    Lanterns,
    Flags,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bridge {
    /// Deck end points (y = deck height in world space).
    pub a: Vec3,
    pub b: Vec3,
    pub width: f32,
    pub kind: BridgeKind,
    /// Things hung from rope bridges' hand ropes.
    pub hanging: Hanging,
    pub colors: Colors,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TreeKind {
    Oak,
    Pine,
    Birch,
    /// Small fruit tree with blossom.
    Fruit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ClutterKind {
    Barrel,
    Crate,
    Haystack,
    Well,
    Cart,
    Bench,
    LanternPost,
    FlagPole,
    WashingLine,
    Woodpile,
    Rock,
    Bush,
    Beehive,
    Scarecrow,
}

/// Single placed object (tree or clutter).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PropKind {
    Tree(TreeKind),
    Clutter(ClutterKind),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub at: Vec2,
    pub angle: f32,
    pub scale: f32,
    pub kind: PropKind,
    pub colors: Colors,
}

/// All player-made things.
#[derive(Clone, Debug, PartialEq)]
pub enum Structure {
    Wall(Wall),
    House(House),
    Fence(Fence),
    Path(Path),
    Stairs(Stairs),
    Platform(Platform),
    Bridge(Bridge),
    Placed(Placed),
}

// ---------------------------------------------------------------------------------------------
// Document + history
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    pub seed: u64,
    pub terrain: Vec<TerrainDab>,
    /// Structures in placement order. Ids are unique and never reused within a document.
    pub structures: Vec<(StructureId, Structure)>,
    pub next_id: StructureId,
}

impl Document {
    pub fn new(seed: u64) -> Self {
        Self { seed, terrain: Vec::new(), structures: Vec::new(), next_id: 1 }
    }

    /// Appends a structure and returns its new id.
    pub fn add(&mut self, s: Structure) -> StructureId {
        let id = self.next_id;
        self.next_id += 1;
        self.structures.push((id, s));
        id
    }

    pub fn get(&self, id: StructureId) -> Option<&Structure> {
        self.structures.iter().find(|(i, _)| *i == id).map(|(_, s)| s)
    }

    pub fn get_mut(&mut self, id: StructureId) -> Option<&mut Structure> {
        self.structures.iter_mut().find(|(i, _)| *i == id).map(|(_, s)| s)
    }

    pub fn remove(&mut self, id: StructureId) -> Option<Structure> {
        let i = self.structures.iter().position(|(i, _)| *i == id)?;
        Some(self.structures.remove(i).1)
    }
}

/// Undo/redo as a stack of whole documents (documents are small).
#[derive(Clone, Debug)]
pub struct History {
    past: Vec<Document>,
    present: Document,
    future: Vec<Document>,
}

impl History {
    pub fn new(doc: Document) -> Self {
        Self { past: Vec::new(), present: doc, future: Vec::new() }
    }

    pub fn current(&self) -> &Document {
        &self.present
    }

    /// Commits `doc` as the new present. No-op if it equals the present.
    pub fn push(&mut self, doc: Document) {
        if doc == self.present {
            return;
        }
        self.past.push(std::mem::replace(&mut self.present, doc));
        self.future.clear();
    }

    pub fn undo(&mut self) -> bool {
        match self.past.pop() {
            Some(prev) => {
                self.future.push(std::mem::replace(&mut self.present, prev));
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        match self.future.pop() {
            Some(next) => {
                self.past.push(std::mem::replace(&mut self.present, next));
                true
            }
            None => false,
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_round_trip() {
        let mut h = History::new(Document::new(1));
        let mut d = h.current().clone();
        d.add(Structure::Placed(Placed {
            at: Vec2::ZERO,
            angle: 0.0,
            scale: 1.0,
            kind: PropKind::Tree(TreeKind::Oak),
            colors: Colors::default(),
        }));
        h.push(d.clone());
        assert!(h.can_undo());
        assert!(h.undo());
        assert!(h.current().structures.is_empty());
        assert!(h.redo());
        assert_eq!(h.current(), &d);
        h.push(d.clone()); // identical push is a no-op
        assert!(!h.can_redo());
        assert!(h.undo());
        assert!(!h.can_undo());
    }
}
