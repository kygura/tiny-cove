//! The live document and its built scene, kept in sync with ECS entities.
//!
//! Contract used by tools / ui / life / shot:
//! - [`Glade`] resource: undo history + an optional live preview document. Tools call
//!   `set_preview` while dragging and `commit` on release; the world rebuilds the scene from
//!   `doc()` whenever `revision` changes.
//! - [`BuiltScene`] resource: the latest built `glade_core::Scene` (terrain, markers…), for
//!   picking (`glade_core::scene::pick`) and for life/weather to read markers and terrain.
//! - Entities: every spawned scene entity carries [`SceneEntity`]; structure parts also carry
//!   [`PartOf`] with their structure id (for hover highlight / eraser).
//!
//! STUB bodies for sync — owned by the world task.

use bevy::prelude::*;
use glade_core::{Document, History, Scene, StructureId};
use std::sync::Arc;

#[derive(Resource)]
pub struct Glade {
    history: History,
    preview: Option<Document>,
    /// Bumped on every change of `doc()`.
    pub revision: u64,
}

impl Glade {
    pub fn new(doc: Document) -> Self {
        Self { history: History::new(doc), preview: None, revision: 1 }
    }
    /// What should be shown: the preview if any, else the committed document.
    pub fn doc(&self) -> &Document {
        self.preview.as_ref().unwrap_or(self.history.current())
    }
    /// The committed document (ignores the preview).
    pub fn committed(&self) -> &Document {
        self.history.current()
    }
    pub fn has_preview(&self) -> bool {
        self.preview.is_some()
    }
    pub fn set_preview(&mut self, doc: Option<Document>) {
        if self.preview != doc {
            self.preview = doc;
            self.revision += 1;
        }
    }
    /// Commits `doc` (clears any preview).
    pub fn commit(&mut self, doc: Document) {
        self.preview = None;
        self.history.push(doc);
        self.revision += 1;
    }
    pub fn undo(&mut self) -> bool {
        self.preview = None;
        let ok = self.history.undo();
        self.revision += 1;
        ok
    }
    pub fn redo(&mut self) -> bool {
        self.preview = None;
        let ok = self.history.redo();
        self.revision += 1;
        ok
    }
    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }
    /// Replace everything (new glade): fresh history.
    pub fn reset(&mut self, doc: Document) {
        *self = Self { revision: self.revision + 1, ..Self::new(doc) };
    }
}

/// Latest built scene (`None` until the first build).
#[derive(Resource, Default, Clone)]
pub struct BuiltScene(pub Option<Arc<Scene>>);

/// Marker on every entity the world spawns from the scene.
#[derive(Component)]
pub struct SceneEntity;

/// Structure a mesh entity belongs to.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartOf(pub StructureId);

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BuiltScene>();
    }
}
