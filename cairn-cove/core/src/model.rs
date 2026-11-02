//! GREYWILD document: an ordered list of terrain strokes and structures, plus undo history.
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
    /// Wildflowers (mixed colours).
    Flowers,
    /// Ferns.
    Ferns,
    /// Low moss carpet.
    Moss,
    /// Gravel ground.
    Gravel,
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
    /// Board-formed concrete.
    #[default]
    Concrete,
    /// Rammed earth in warm strata.
    RammedEarth,
    /// Charred vertical timber cladding over a concrete core.
    Timber,
}

/// Roof shapes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum RoofStyle {
    /// Sharp slab gable: two thick concrete planes, deep overhang, open gable ends.
    /// Round footprints fall back to `Shell`.
    #[default]
    Pitched,
    /// Saw-tooth north-light roof: repeated monopitch bays with glazed verticals.
    Sawtooth,
    /// Shell: shallow dome / cone (round footprints) or folded pyramid (rectangles).
    Shell,
    /// Flat green roof with a thick parapet; planter boxes if the structure asks for them.
    Green,
}

/// Palette indices. The renderer owns the actual colours (see `palette` module).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Colors {
    /// Index into the roof palette (0..ROOF_COLORS).
    pub roof: u8,
    /// Index into the wall/plaster palette (0..WALL_COLORS).
    pub wall: u8,
    /// Index into the wood/trim palette (0..TRIM_COLORS), used by timber, doors, louvres, railings.
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
    /// Deep slot window, ~0.8 m wide, sill at 1.0 m.
    Window,
    /// Recessed door, ~1.1 m wide, 2.0 m tall.
    Door,
    /// Wide portal, ~2.4 m wide.
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
    /// Height above the ground at each point (top of the wall, without coping or planters).
    pub height: f32,
    /// Thickness in metres (0.4 .. 2.0).
    pub thickness: f32,
    pub material: Material,
    /// Crenellated planter boxes with spilling greenery along the top.
    pub planters: bool,
    /// Broken top with exposed rebar (ruins; the snip tool).
    pub ruined: bool,
    /// Height of a darker concrete plinth/foundation (0 = none).
    pub foundation: f32,
    /// Closed walls only: put a roof over the enclosed area.
    pub roofed: bool,
    pub roof: RoofStyle,
    pub roof_pitch: f32,
    pub openings: Vec<Opening>,
    pub colors: Colors,
    /// 0..=1: moss, ivy and water staining (the brutalist "peeling plaster" slider).
    pub overgrowth: f32,
}

/// Block (rect) or silo tower (round). Walls rise from the ground (flattened under the footprint).
#[derive(Clone, Debug, PartialEq)]
pub struct House {
    pub footprint: Footprint,
    /// Eave height above ground. 0.6 ..= 24. Low values make tent-like shelters.
    pub wall_height: f32,
    pub roof: RoofStyle,
    /// Roof rise as a fraction of the footprint's smaller half extent / radius (0 = flat).
    pub roof_pitch: f32,
    pub material: Material,
    /// Brise-soleil: a grid of deep vertical concrete fins on the façades.
    pub fins: bool,
    /// Concrete plinth height (0 = none).
    pub foundation: f32,
    /// Green roofs only: planter boxes on the parapet.
    pub planters: bool,
    /// Player-placed openings. If empty, windows and one door are generated automatically.
    pub openings: Vec<Opening>,
    pub colors: Colors,
    /// 0..=1: moss, ivy and water staining.
    pub overgrowth: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum FenceKind {
    /// Steel tube posts with two rails.
    #[default]
    Railing,
    /// Low concrete balustrade with balusters (~1 m).
    Balustrade,
    /// Wire-cage gabion of rocks (~0.9 m).
    Gabion,
    /// Concrete planter trough with a clipped hedge.
    Hedge,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fence {
    pub points: Vec<Vec2>,
    pub kind: FenceKind,
    pub colors: Colors,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PathKind {
    /// Loose gravel with a concrete kerb.
    #[default]
    Gravel,
    /// Small concrete pavers.
    Pavers,
    /// Large concrete slabs with grass joints.
    Slabs,
    /// Poured concrete with expansion joints.
    Poured,
    /// Separate slabs set into the grass.
    Stepping,
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
    /// Open steel stair with railings (railing colour = `Colors::trim`).
    #[default]
    Steel,
    /// Solid cast concrete steps.
    Concrete,
    /// Steel ladder.
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

/// Concrete deck on pilotis.
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
    /// Light cable bridge with a sagging steel-mesh deck; `railing` 0..4 picks one of four
    /// railing designs.
    Cable { railing: u8 },
    /// Concrete arch bridge.
    Arch,
    /// Steel box-girder bridge on concrete piers.
    Girder,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Hanging {
    #[default]
    None,
    Lamps,
    Vines,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bridge {
    /// Deck end points (y = deck height in world space).
    pub a: Vec3,
    pub b: Vec3,
    pub width: f32,
    pub kind: BridgeKind,
    /// Things hung from Cable bridges' hand cables.
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
    /// Tree fern: rough trunk with a crown of fronds.
    Fern,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ClutterKind {
    RainBarrel,
    Planter,
    Compost,
    Cistern,
    Wheelbarrow,
    Bench,
    LampPost,
    WindTurbine,
    WashingLine,
    LogPile,
    Rock,
    Bush,
    Beehive,
    Solar,
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
