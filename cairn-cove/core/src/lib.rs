//! CAIRN COVE core: document model, island terrain and procedural geometry. No Bevy.

pub mod buildings;
pub mod demo;
pub mod geom;
pub mod mesh;
pub mod model;
pub mod nature;
pub mod palette;
pub mod paths;
pub mod scene;
pub mod terrain;
pub mod walls;

pub use mesh::{Marker, MarkerKind, MeshData, Surface};
pub use model::*;
pub use scene::{Scene, build};
