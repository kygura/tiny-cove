//! `--demo-shot`: a scripted scene in the calm centre, framed for the screenshot.

use crate::camera::{CameraRig, RigPose};
use crate::game::{MeshingDone, World};
use crate::shot::{CliArgs, DemoShot};
use acre_core::{Document, History, Structure};
use bevy::prelude::*;

/// The scripted document (all within ~25 wu of the centre).
pub fn demo_document(seed: u64) -> Document {
    let structures = vec![
        Structure::Field { min: [14.0, -22.0], max: [26.0, -12.0] },
        // Tall rampart at the back, a lower wall with a gate, a short stub.
        Structure::Wall { path: vec![[-18.0, -14.0], [-6.0, -17.0], [8.0, -16.0]], tall: true },
        Structure::Wall { path: vec![[13.0, -9.0], [14.0, 0.0], [13.0, 9.0]], tall: false },
        Structure::Wall { path: vec![[-20.0, -2.0], [-18.0, 8.0]], tall: false },
        // Two overlapping blocks forming one mass.
        Structure::Block { min: [-10.0, -8.0], max: [-1.0, 1.0], floors: 7 },
        Structure::Block { min: [-4.0, -2.0], max: [5.0, 6.0], floors: 4 },
        // Path through the wall (gate).
        Structure::Path { path: vec![[-14.0, 12.0], [0.0, 11.0], [8.0, 7.0], [14.0, 0.5], [24.0, -3.0]] },
        Structure::Spire { at: [-14.0, -5.0], kind: 0 },
        Structure::Spire { at: [22.0, 6.0], kind: 1 },
        Structure::Spire { at: [-6.0, 16.0], kind: 2 },
    ];
    Document { seed, structures }
}

pub struct DemoPlugin;

impl Plugin for DemoPlugin {
    fn build(&self, app: &mut App) {
        let demo = app.world().get_resource::<CliArgs>().is_some_and(|a| a.demo_shot.is_some());
        if demo {
            if let Some(mut a) = app.world_mut().get_resource_mut::<CliArgs>() {
                a.weather.get_or_insert_with(|| "rain".into());
                a.time.get_or_insert(15.5);
            }
            app.add_systems(Startup, setup_demo).add_systems(Update, release_when_meshed);
        }
    }
}

fn setup_demo(
    args: Res<CliArgs>,
    mut world: ResMut<World>,
    mut rig: ResMut<CameraRig>,
    mut shot: ResMut<DemoShot>,
) {
    let seed = args.seed.unwrap_or(world.history.doc().seed);
    world.history = History::new(Document::new(seed));
    world.history.push(demo_document(seed));
    rig.snap(RigPose { focus: Vec3::new(1.0, 6.0, -7.0), yaw: 40f32.to_radians(), view_height: 44.0 });
    shot.hold = true;
}

fn release_when_meshed(done: Res<MeshingDone>, shot: Option<ResMut<DemoShot>>) {
    if let Some(mut shot) = shot
        && shot.hold
        && done.0
    {
        shot.hold = false;
    }
}
