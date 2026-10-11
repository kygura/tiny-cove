use bevy::prelude::*;
use bevy::render::view::screenshot::ScreenshotCaptured;
use concrete_glade::ambience::AmbiencePlugin;
use concrete_glade::camera::{CameraPlugin, Orbit, OrbitPose};
use concrete_glade::game::{Doc, Grid};
use concrete_glade::render::{Ps1Plugin, request_screenshot};
use concrete_glade::scene::{ScenePlugin, cli_arg, demo_world};
use concrete_glade::tools::ToolsPlugin;
use concrete_glade::ui::HudPlugin;

/// Frames to let the scene settle (pipelines compile, particles spread) before capture.
const DEMO_SETTLE_FRAMES: u32 = 120;
/// Give up (exit code 1) if the screenshot hasn't been written this many frames later.
const DEMO_TIMEOUT_FRAMES: u32 = 600;

#[derive(Resource)]
struct DemoShot {
    path: String,
    frames: u32,
}

fn main() {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "CONCRETE GLADE".into(),
            resolution: (1280, 720).into(),
            ..default()
        }),
        ..default()
    }))
    .add_plugins((Ps1Plugin, ScenePlugin, CameraPlugin, ToolsPlugin, HudPlugin, AmbiencePlugin));

    if let Some(path) = cli_arg("--demo-shot") {
        let seed = app.world().resource::<Doc>().0.seed;
        app.insert_resource(Doc(demo_world(seed)))
            .insert_resource(DemoShot { path, frames: 0 })
            .add_systems(Startup, |mut orbit: ResMut<Orbit>| {
                orbit.snap(OrbitPose {
                    focus: Vec2::new(-7.0, 8.0),
                    yaw: -25f32.to_radians(),
                    pitch: 29f32.to_radians(),
                    distance: 54.0,
                })
            })
            .add_systems(Last, demo_shot);
    }
    app.run();
}

/// Waits until the grid is meshed plus a few frames, then saves the window and exits.
fn demo_shot(
    mut commands: Commands,
    mut shot: ResMut<DemoShot>,
    grid: Res<Grid>,
    mut exit: MessageWriter<AppExit>,
) {
    if shot.frames == 0 && grid.0 == Default::default() {
        return;
    }
    shot.frames += 1;
    if shot.frames == DEMO_SETTLE_FRAMES + DEMO_TIMEOUT_FRAMES {
        error!("demo shot: screenshot never arrived");
        exit.write(AppExit::error());
    }
    if shot.frames == DEMO_SETTLE_FRAMES {
        info!("demo shot -> {}", shot.path);
        request_screenshot(&mut commands, shot.path.clone()).observe(
            |_: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
                exit.write(AppExit::Success);
            },
        );
    }
}
