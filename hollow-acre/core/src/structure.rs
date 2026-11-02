//! High-level editable world description + undo/redo history.

/// A placed thing. Coordinates are world units, x/z on the ground plane.
#[derive(Clone, Debug, PartialEq)]
pub enum Structure {
    /// Board-formed concrete wall along a polyline; `tall` makes a dam-like rampart.
    Wall { path: Vec<[f32; 2]>, tall: bool },
    /// Brutalist tower/terrace block, `floors` 1..=12.
    Block { min: [f32; 2], max: [f32; 2], floors: u8 },
    /// Gravel path flush in the terrain; carves arches through structures.
    Path { path: Vec<[f32; 2]> },
    /// Tilled field with luminous wheat.
    Field { min: [f32; 2], max: [f32; 2] },
    /// Landmark: 0 crystal cluster, 1 floating rune monolith, 2 blossom tree.
    Spire { at: [f32; 2], kind: u8 },
}

impl Structure {
    /// True if every coordinate is finite.
    pub fn is_finite(&self) -> bool {
        let f2 = |p: &[f32; 2]| p[0].is_finite() && p[1].is_finite();
        match self {
            Structure::Wall { path, .. } | Structure::Path { path } => path.iter().all(f2),
            Structure::Block { min, max, .. } | Structure::Field { min, max } => f2(min) && f2(max),
            Structure::Spire { at, .. } => f2(at),
        }
    }

    /// True if ground point `p` (world x/z) is within `radius` of the structure's
    /// footprint. For the Raze tool.
    pub fn touches(&self, p: [f32; 2], radius: f32) -> bool {
        let half = |voxels: f32| voxels * crate::voxel::VOXEL_SIZE / 2.0;
        match self {
            Structure::Wall { path, tall } => {
                polyline_dist(path, p) <= half(if *tall { 5.0 } else { 3.0 }) + radius
            }
            Structure::Path { path } => polyline_dist(path, p) <= half(3.0) + radius,
            Structure::Block { min, max, .. } | Structure::Field { min, max } => {
                let (lo, hi) = ([min[0].min(max[0]), min[1].min(max[1])], [min[0].max(max[0]), min[1].max(max[1])]);
                p[0] >= lo[0] - radius && p[0] <= hi[0] + radius && p[1] >= lo[1] - radius && p[1] <= hi[1] + radius
            }
            Structure::Spire { at, .. } => {
                let d = ((p[0] - at[0]).powi(2) + (p[1] - at[1]).powi(2)).sqrt();
                d <= 2.5 + radius
            }
        }
    }
}

fn polyline_dist(pts: &[[f32; 2]], p: [f32; 2]) -> f32 {
    let d2 = |a: [f32; 2], b: [f32; 2]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt();
    match pts {
        [] => f32::INFINITY,
        [a] => d2(*a, p),
        _ => pts
            .windows(2)
            .map(|w| {
                let (a, b) = (w[0], w[1]);
                let ab = [b[0] - a[0], b[1] - a[1]];
                let l2 = ab[0] * ab[0] + ab[1] * ab[1];
                let t = if l2 > 0.0 { (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / l2).clamp(0.0, 1.0) } else { 0.0 };
                d2([a[0] + ab[0] * t, a[1] + ab[1] * t], p)
            })
            .fold(f32::INFINITY, f32::min),
    }
}

/// The editable world: a seed plus structures in placement order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Document {
    pub seed: u64,
    pub structures: Vec<Structure>,
}

impl Document {
    /// Copy without structures that have non-finite coordinates.
    pub fn sanitized(&self) -> Document {
        Document { seed: self.seed, structures: self.structures.iter().filter(|s| s.is_finite()).cloned().collect() }
    }

    pub fn new(seed: u64) -> Self {
        Self { seed, structures: Vec::new() }
    }
}

const MAX_UNDO: usize = 256;

/// Undo/redo over whole-document snapshots.
#[derive(Clone, Debug, Default)]
pub struct History {
    current: Document,
    undo: Vec<Document>,
    redo: Vec<Document>,
}

impl History {
    pub fn new(doc: Document) -> Self {
        Self { current: doc, undo: Vec::new(), redo: Vec::new() }
    }

    pub fn doc(&self) -> &Document {
        &self.current
    }

    /// Commits `new_doc` as the current document (no-op if identical; structures with non-finite coordinates are dropped). Clears redo.
    pub fn push(&mut self, new_doc: Document) {
        let new_doc = new_doc.sanitized();
        if new_doc == self.current {
            return;
        }
        self.undo.push(std::mem::replace(&mut self.current, new_doc));
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Steps back; returns false if there was nothing to undo.
    pub fn undo(&mut self) -> bool {
        match self.undo.pop() {
            Some(prev) => {
                self.redo.push(std::mem::replace(&mut self.current, prev));
                true
            }
            None => false,
        }
    }

    /// Steps forward; returns false if there was nothing to redo.
    pub fn redo(&mut self) -> bool {
        match self.redo.pop() {
            Some(next) => {
                self.undo.push(std::mem::replace(&mut self.current, next));
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spire() -> Structure {
        Structure::Spire { at: [0.0, 0.0], kind: 0 }
    }

    #[test]
    fn undo_redo_roundtrip() {
        let mut h = History::new(Document::new(1));
        let mut d = h.doc().clone();
        d.structures.push(spire());
        h.push(d.clone());
        d.structures.push(spire());
        h.push(d);
        assert_eq!(h.doc().structures.len(), 2);
        assert!(h.undo() && h.undo());
        assert!(h.doc().structures.is_empty());
        assert!(!h.undo() && !h.can_undo() && h.can_redo());
        assert!(h.redo() && h.redo());
        assert_eq!(h.doc().structures.len(), 2);
        assert!(!h.redo());
    }

    #[test]
    fn new_edit_clears_redo() {
        let mut h = History::new(Document::new(1));
        let mut d = h.doc().clone();
        d.structures.push(spire());
        h.push(d);
        h.undo();
        let mut d2 = h.doc().clone();
        d2.structures.push(Structure::Spire { at: [1.0, 1.0], kind: 2 });
        h.push(d2);
        assert!(!h.can_redo());
    }

    #[test]
    fn touches_works() {
        let w = Structure::Wall { path: vec![[0.0, 0.0], [10.0, 0.0]], tall: false };
        assert!(w.touches([5.0, 0.5], 0.0) && !w.touches([5.0, 3.0], 0.5));
    }
}
