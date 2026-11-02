//! CAIRN COVE document: an ordered list of terrain strokes and structures, plus undo history.
//!
//! Everything here is plain data. `crate::scene::build` turns a [`Document`] into meshes.
//! World units are metres, `y` is up, the buildable island spans `x, z ∈ [-GLADE_HALF, GLADE_HALF]`.
//! Ground below [`SEA_LEVEL`] is sea.

use glam::{Vec2, Vec3};

/// Half the side length of the buildable island area, in metres.
pub const GLADE_HALF: f32 = 32.0;

/// Mean sea level. Ground below it is sea. Core logic always uses this value.
pub const SEA_LEVEL: f32 = 0.0;

/// The render-side tide oscillates by ± this many metres around [`SEA_LEVEL`].
pub const TIDE_RANGE: f32 = 0.3;

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
    /// Carve a lagoon / tide pool: lowers the bed and marks it wet. `depth` metres at the centre.
    Water { depth: f32 },
    /// Paint a vegetation / ground layer with strength in 0..=1 (negative erases).
    Paint { layer: PaintLayer, strength: f32 },
}

/// Paintable ground layers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PaintLayer {
    /// Sea pinks: a thrift flower carpet in magenta and pink.
    SeaPinks,
    /// Marram dune-grass tufts.
    DuneGrass,
    /// Wrack: seaweed strewn along the shore.
    Wrack,
    /// Beach pebbles.
    Pebbles,
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

/// Wall material of huts, roundhouses and beacons.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Material {
    /// Wattle-and-daub limewashed in the wall colour, with timber corner posts.
    #[default]
    Wattle,
    /// Vertical driftwood planks with painted bands.
    Plank,
    /// Dry stone laid in courses.
    Stone,
}

/// Construction of a dyke (the `Wall` structure).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum WallKind {
    /// Flat stones stacked in courses, upright copestones on the crest.
    #[default]
    DryStone,
    /// A stone base under bands of turf / sod with a grassy top.
    Turf,
    /// Sharpened driftwood stakes with a lashed rail.
    Palisade,
}

/// Roof styles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum RoofStyle {
    /// Thick steep hipped thatch with a bound ridge and trimmed eaves. A cone with a topknot
    /// on round footprints.
    #[default]
    Thatch,
    /// Low grassy roof with sod edges (a low dome on round footprints).
    Turf,
    /// Hide or sailcloth stretched on poles, low and tent-like (a conical tent on round
    /// footprints).
    Hide,
    /// Upturned-hull roof: curved keel ridge, planked and painted strakes. Round footprints
    /// fall back to a thatch cone.
    Boat,
}

/// Palette indices. The renderer owns the actual colours (see `palette` module).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Colors {
    /// Index into the roof palette (0..ROOF_COLORS).
    pub roof: u8,
    /// Index into the daub limewash palette (0..WALL_COLORS).
    pub wall: u8,
    /// Index into the pigment / trim palette (0..TRIM_COLORS): carvings, shutters, painted
    /// bands, doors, railings.
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
    /// Distance in metres along the host's centre polyline (dykes) or perimeter (houses,
    /// measured along `geom::footprint_polygon` order from its first corner).
    pub along: f32,
    pub kind: OpeningKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OpeningKind {
    /// Small shuttered window, ~0.8 m wide, sill at 1.0 m.
    Window,
    /// Hide-curtain doorway, ~1.1 m wide, 2.0 m tall.
    Door,
    /// Wide boat door, ~2.4 m wide.
    WideDoor,
}

// ---------------------------------------------------------------------------------------------
// Structures
// ---------------------------------------------------------------------------------------------

/// Footprint of a hut (rect) or a roundhouse / beacon (round).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Footprint {
    /// Oriented rectangle. `half` are half extents along the local x/z axes, rotated by `angle`
    /// radians in the (x, z) plane, turning +x towards +z (`Vec2::from_angle`).
    Rect { center: Vec2, half: Vec2, angle: f32 },
    /// Circle. A tall round house (wall height ≥ 2.5 × radius) is a beacon.
    Round { center: Vec2, radius: f32 },
}

/// Dyke: drawn as a polyline. Over the sea it becomes a breakwater. A closed polyline
/// (first == last point) encloses a yard that the roof tool can cover.
#[derive(Clone, Debug, PartialEq)]
pub struct Wall {
    /// Centre line on the ground plane, at least 2 points. Closed if first ≈ last.
    pub points: Vec<Vec2>,
    /// Height above the ground at each point (top of the dyke, without the crest).
    pub height: f32,
    /// Thickness in metres (0.4 .. 2.0).
    pub thickness: f32,
    pub kind: WallKind,
    /// Crest: upright copestones (dry stone), painted stake tips (palisade) or flowering
    /// thrift (turf).
    pub crest: bool,
    /// Tumbled: collapsed sections with spilled stones at the foot.
    pub tumbled: bool,
    /// Height of a boulder footing (0 = none).
    pub footing: f32,
    /// Closed walls only: put a roof over the enclosed area.
    pub roofed: bool,
    pub roof: RoofStyle,
    pub roof_pitch: f32,
    pub openings: Vec<Opening>,
    pub colors: Colors,
    /// 0..=1: salt bleaching and orange/yellow lichen.
    pub weathering: f32,
}

/// Longhut (rect), roundhouse (low round) or beacon tower (tall round). The ground under it
/// is flattened unless it stands on stilts.
#[derive(Clone, Debug, PartialEq)]
pub struct House {
    pub footprint: Footprint,
    /// Eave height above ground (or above the stilt floor). 0.6 ..= 24.
    pub wall_height: f32,
    pub roof: RoofStyle,
    /// Roof rise as a fraction of the footprint's smaller half extent / radius (0 = flat).
    pub roof_pitch: f32,
    pub material: Material,
    /// Crossed carved gable horns and painted bands.
    pub carved: bool,
    /// Drying fish, net bundles and shell strings under the eaves.
    pub hangings: bool,
    /// Explicit stilt height in metres (0 = automatic: stilts only over water or steep ground).
    pub stilts: f32,
    /// Player-placed openings. If empty, windows and one door are generated automatically.
    pub openings: Vec<Opening>,
    pub colors: Colors,
    /// 0..=1: salt bleaching and orange/yellow lichen.
    pub weathering: f32,
}

/// Windbreak construction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum FenceKind {
    /// Woven wattle panels between stakes.
    #[default]
    Wattle,
    /// Crooked bleached driftwood post-and-rail.
    Driftwood,
    /// Posts with fishing nets draped between them and bright floats.
    NetLine,
    /// Bundled reed screen.
    Reed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fence {
    pub points: Vec<Vec2>,
    pub kind: FenceKind,
    pub colors: Colors,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PathKind {
    /// Crushed white shell with a pebble kerb.
    #[default]
    Shell,
    /// Cobbled beach pebbles.
    Pebble,
    /// Slate slabs with grass joints.
    Flagstone,
    /// Planks on sleepers, laid across sand and marsh.
    Boardwalk,
    /// Stepping stones.
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
    /// Cut-stone steps.
    #[default]
    Stone,
    /// Log-and-earth steps held by stakes.
    Log,
    /// Rope ladder.
    RopeLadder,
}

/// Steps between two 3-D points (ground, dyke top, jetty, hut floor…).
#[derive(Clone, Debug, PartialEq)]
pub struct Stairs {
    pub from: Vec3,
    pub to: Vec3,
    pub width: f32,
    pub kind: StairKind,
    pub colors: Colors,
}

/// Jetty / stage: a plank deck on log piles. Over water the piles reach the seabed and
/// moored canoes are emitted as `Boat` markers.
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
    /// Sagging plank-and-rope bridge; `railing` 0..4 picks plain rope, netting, plank
    /// balusters or knotted rope with floats.
    Rope { railing: u8 },
    /// A big driftwood log trestle.
    Log,
    /// Stacked stone causeway with culvert arches.
    Causeway,
}

/// Things hung from a rope bridge's hand ropes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Hanging {
    #[default]
    None,
    /// Lanterns (lit at night).
    Lanterns,
    /// Cloth pennants that flutter in the wind.
    Pennants,
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
    /// Wind-bent umbrella sea pine.
    SeaPine,
    /// Leaning palm with a crown of fronds.
    Palm,
    /// Tall dark flame-shaped cypress.
    Cypress,
    /// Broad-canopied fig with fruit.
    Fig,
    /// Bleached dead driftwood tree.
    Snag,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ClutterKind {
    /// Outrigger canoe. Placed over water it floats (a `Boat` marker instead of a mesh).
    Canoe,
    /// Fish drying on a rack.
    DryingRack,
    /// Stone ring with an open fire (a `Fire` marker).
    Firepit,
    /// Stacked stones, the island's namesake.
    Cairn,
    /// Standing stone carved with a spiral.
    StandingStone,
    /// Painted pole with a bird on top.
    CarvedPole,
    /// Rack of drying fishing nets.
    NetRack,
    /// Woven baskets and clay jars.
    Baskets,
    LanternPost,
    /// Tall pole with a cloth pennant streaming in the wind.
    Pennant,
    Rock,
    /// Flowering gorse / flax.
    Bush,
    CrabPots,
    /// Stacked driftwood.
    Woodpile,
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
            kind: PropKind::Tree(TreeKind::Snag),
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
