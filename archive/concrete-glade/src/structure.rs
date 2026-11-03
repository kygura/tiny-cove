//! High-level, editable world description. The voxel grid is derived from this.

use bevy::math::Vec2;

/// Stable id so demolish/undo can refer to structures.
pub type StructureId = u32;

/// Total length of a polyline.
pub fn polyline_len(points: &[Vec2]) -> f32 {
    points.windows(2).map(|w| w[0].distance(w[1])).sum()
}

/// A placed thing. Coordinates are world-space x/z on the ground plane.
#[derive(Clone, Debug, PartialEq)]
pub enum Structure {
    /// Thick concrete wall following a polyline.
    Wall { points: Vec<Vec2>, height: u32, dam: bool },
    /// Rectangular megablock. `min`/`max` are opposite footprint corners (any order),
    /// `height` in voxels.
    Block { min: Vec2, max: Vec2, height: u32 },
    /// Road painted into the ground along a polyline.
    Road { points: Vec<Vec2> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub id: StructureId,
    pub structure: Structure,
}

/// The editable document plus undo/redo history.
#[derive(Clone, Debug, Default)]
pub struct World {
    pub seed: u64,
    pub placed: Vec<Placed>,
    next_id: StructureId,
    undo: Vec<Vec<Placed>>,
    redo: Vec<Vec<Placed>>,
}

impl World {
    pub fn new(seed: u64) -> Self {
        Self { seed, ..Default::default() }
    }

    fn checkpoint(&mut self) {
        self.undo.push(self.placed.clone());
        self.redo.clear();
    }

    pub fn add(&mut self, structure: Structure) -> StructureId {
        self.checkpoint();
        let id = self.next_id;
        self.next_id += 1;
        self.placed.push(Placed { id, structure });
        id
    }

    /// Removes the given ids as one undoable step. Returns true if anything changed.
    pub fn remove(&mut self, ids: &[StructureId]) -> bool {
        if !self.placed.iter().any(|p| ids.contains(&p.id)) {
            return false;
        }
        self.checkpoint();
        self.placed.retain(|p| !ids.contains(&p.id));
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        match self.undo.pop() {
            Some(prev) => {
                self.redo.push(std::mem::replace(&mut self.placed, prev));
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        match self.redo.pop() {
            Some(next) => {
                self.undo.push(std::mem::replace(&mut self.placed, next));
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn road() -> Structure {
        Structure::Road { points: vec![Vec2::ZERO, Vec2::X] }
    }

    #[test]
    fn undo_redo_roundtrip() {
        let mut w = World::new(1);
        let a = w.add(road());
        w.add(road());
        assert!(w.remove(&[a]));
        assert_eq!(w.placed.len(), 1);
        assert!(w.undo());
        assert_eq!(w.placed.len(), 2);
        assert!(w.undo());
        assert!(w.undo());
        assert!(w.placed.is_empty());
        assert!(!w.undo());
        assert!(w.redo());
        assert!(w.redo());
        assert_eq!(w.placed.len(), 2);
    }

    #[test]
    fn new_edit_clears_redo() {
        let mut w = World::new(1);
        w.add(road());
        w.undo();
        w.add(road());
        assert!(!w.redo());
    }
}
