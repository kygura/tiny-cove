//! HUD + ambience probe: screenshots the window with a fake document, then exits.
//! Usage: `cargo run --example hud_probe -- [out.png] [frames]`

use bevy::{prelude::*, render::view::screenshot::ScreenshotCaptured};
use concrete_glade::{
    ambience::AmbiencePlugin,
    game::{Doc, Grid, Tool, ToolState},
    render::{Ps1Plugin, request_screenshot},
    structure::{Structure, World},
    ui::HudPlugin,
    voxel::{GROUND_Y, Voxel},
};

#[derive(Resource)]
struct Probe {
    path: String,
    frames: u32,
}

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().unwrap_or_else(|| "hud_probe.png".into());
    let frames = args.next().and_then(|s| s.parse().ok()).unwrap_or(90);
    App::new()
        .add_plugins((DefaultPlugins, Ps1Plugin, HudPlugin, AmbiencePlugin))
        .insert_resource(Probe { path, frames })
        .add_systems(Startup, setup)
        .add_systems(Update, shoot)
        .run();
}

fn setup(mut commands: Commands, mut cam: Single<&mut Transform, With<concrete_glade::render::GameCamera>>) {
    **cam = Transform::from_xyz(2.0, 5.0, 24.0).looking_at(Vec3::new(0.0, 0.0, 6.0), Vec3::Y);
    let mut world = World::new(1);
    for (min, max, h) in [(-6.0, 0.0, 30), (4.0, 10.0, 20), (14.0, 18.0, 12), (-14.0, -10.0, 8)] {
        world.add(Structure::Block { min: Vec2::splat(min), max: Vec2::splat(max), height: h });
    }
    world.add(Structure::Road { points: vec![Vec2::new(-20.0, 8.0), Vec2::new(0.0, 8.0), Vec2::new(10.0, -4.0)] });
    let mut grid = Grid::default();
    for i in 0..4000 {
        let p = IVec3::new(i % 40, GROUND_Y + i / 1600, i / 40 % 40);
        grid.0.set(p, Voxel::Concrete);
    }
    commands.insert_resource(Doc(world));
    commands.insert_resource(grid);
    commands.insert_resource(ToolState {
        tool: Tool::Block,
        hint: "move mouse to raise  ·  click to cast  ·  esc to cancel".into(),
    });
}

fn shoot(mut commands: Commands, probe: Res<Probe>, mut frame: Local<u32>) {
    *frame += 1;
    if *frame == probe.frames {
        request_screenshot(&mut commands, probe.path.clone()).observe(
            |_: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
                exit.write(AppExit::Success);
            },
        );
    }
}
