//! Command line (`--demo`, `--demo-shot`, …) and the headless demo-shot driver.
//!
//! `--demo-shot out.png [--time H] [--weather NAME] [--seed N] [--yaw D] [--zoom S] [--hud]` builds the
//! demo glade, freezes the clock/weather, waits for the scene plus a few frames, saves a
//! screenshot and exits. `--demo` just starts interactively with the demo glade.

use crate::camera::CameraRig;
use crate::render::{LowResDisplay, request_screenshot};
use crate::weather::{Clock, Weather, WeatherKind};
use crate::world::BuiltScene;
use bevy::prelude::*;
use bevy::render::{
    Render, RenderApp, RenderSystems,
    render_resource::{CachedPipelineState, PipelineCache},
};
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

/// Frames to wait after the scene exists before capturing (shadows, fog, smoothing settle).
const SETTLE_FRAMES: u32 = 30;
/// Give up if the screenshot never arrives.
const TIMEOUT_FRAMES: u32 = 600;
/// Consecutive frames with no pipeline compiling before capturing (render runs a frame behind).
const READY_FRAMES: u32 = 5;

#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct Cli {
    pub demo: bool,
    pub shot: Option<PathBuf>,
    pub time: Option<f32>,
    pub weather: Option<WeatherKind>,
    pub seed: Option<u64>,
    pub yaw: Option<f32>,
    pub zoom: Option<f32>,
    /// Keep the HUD in the demo shot (default: glade only).
    pub hud: bool,
}

impl Cli {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Self {
        let args: Vec<String> = args.into_iter().collect();
        let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1));
        fn num<T: std::str::FromStr>(v: Option<&String>) -> Option<T> {
            v?.parse().ok()
        }
        Self {
            demo: args.iter().any(|a| a == "--demo"),
            shot: value("--demo-shot").map(PathBuf::from),
            time: num(value("--time")),
            weather: value("--weather").and_then(|w| WeatherKind::parse(w)),
            seed: num(value("--seed")),
            yaw: num(value("--yaw")),
            zoom: num(value("--zoom")),
            hud: args.iter().any(|a| a == "--hud"),
        }
    }

    /// `--seed`, else fixed for reproducible shots, else derived from the clock.
    pub fn seed(&self) -> u64 {
        self.seed.unwrap_or_else(|| {
            if self.shot.is_some() {
                7
            } else {
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(7, |d| d.as_millis() as u64)
            }
        })
    }
}

pub struct ShotPlugin;

impl Plugin for ShotPlugin {
    fn build(&self, app: &mut App) {
        let Some(cli) = app.world().get_resource::<Cli>().cloned() else { return };
        let Some(path) = cli.shot.clone() else { return };
        let pending = PendingPipelines::default();
        app.insert_resource(ShotState { path, frames: 0, ready_frames: 0, requested: false })
            .insert_resource(pending.clone())
            .add_systems(Startup, apply_cli)
            .add_systems(Last, capture);
        if !cli.hud {
            app.add_systems(Last, hide_hud.before(capture));
        }
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app.insert_resource(pending).add_systems(Render, count_pending.in_set(RenderSystems::Cleanup));
        }
    }
}

#[derive(Resource)]
struct ShotState {
    path: PathBuf,
    frames: u32,
    /// Consecutive frames with every render pipeline compiled.
    ready_frames: u32,
    requested: bool,
}

/// Render pipelines still queued / compiling, written by the render world each frame and
/// read by `capture`. Pipelines compile asynchronously (slowly on lavapipe); a capture
/// taken while e.g. the UI image pipeline that shows the low-res target is still
/// compiling comes out all black.
#[derive(Resource, Clone, Default)]
struct PendingPipelines(Arc<AtomicU32>);

fn count_pending(cache: Res<PipelineCache>, pending: Res<PendingPipelines>) {
    let n = cache.pipelines().filter(|p| matches!(p.state, CachedPipelineState::Queued | CachedPipelineState::Creating(_))).count()
        + cache.waiting_pipelines().count();
    pending.0.store(n as u32, Ordering::Relaxed);
}

fn apply_cli(cli: Res<Cli>, mut clock: ResMut<Clock>, mut weather: ResMut<Weather>, mut rig: ResMut<CameraRig>) {
    clock.hour = cli.time.unwrap_or(12.0).rem_euclid(24.0);
    clock.paused = true;
    weather.kind = cli.weather.unwrap_or_default();
    weather.auto = false;
    weather.snap = true;
    if let Some(yaw) = cli.yaw {
        rig.set_yaw(yaw);
    }
    if let Some(zoom) = cli.zoom {
        rig.set_zoom(zoom);
    }
}

/// Demo shots show the glade only: hide every root UI node (the HUD).
fn hide_hud(mut roots: Query<&mut Visibility, (With<Node>, Without<ChildOf>, Without<LowResDisplay>)>) {
    for mut v in &mut roots {
        if *v != Visibility::Hidden {
            *v = Visibility::Hidden;
        }
    }
}

fn capture(
    mut commands: Commands,
    mut st: ResMut<ShotState>,
    built: Res<BuiltScene>,
    pending: Res<PendingPipelines>,
    mut exit: MessageWriter<AppExit>,
) {
    if built.0.is_none() {
        return;
    }
    st.frames += 1;
    let waiting = pending.0.load(Ordering::Relaxed);
    st.ready_frames = if waiting == 0 { st.ready_frames + 1 } else { 0 };
    if st.frames == SETTLE_FRAMES && waiting > 0 {
        info!("demo shot: {waiting} pipelines still compiling after {SETTLE_FRAMES} frames, waiting");
    }
    if st.frames >= SETTLE_FRAMES + TIMEOUT_FRAMES {
        error!("demo shot: screenshot never arrived ({waiting} pipelines still compiling)");
        exit.write(AppExit::error());
    } else if st.frames >= SETTLE_FRAMES && st.ready_frames >= READY_FRAMES && !st.requested {
        st.requested = true;
        request_screenshot(&mut commands, st.path.clone()).observe(
            |_: On<bevy::render::view::screenshot::ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
                exit.write(AppExit::Success);
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn parses_demo_shot() {
        let c = Cli::parse(a("bin --demo-shot x.png --time 19.5 --weather Rain --seed 3 --yaw 225 --zoom 0.8"));
        assert_eq!(c.shot, Some("x.png".into()));
        assert_eq!((c.time, c.weather, c.seed, c.yaw, c.zoom), (Some(19.5), Some(WeatherKind::Rain), Some(3), Some(225.0), Some(0.8)));
        assert!(!c.demo && !c.hud);
        assert!(Cli::parse(a("bin --demo-shot x.png --hud")).hud);
    }

    #[test]
    fn plain_and_demo() {
        assert_eq!(Cli::parse(a("bin")), Cli::default());
        assert!(Cli::parse(a("bin --demo")).demo);
        assert_eq!(Cli::parse(a("bin --seed 9")).seed(), 9);
        assert_eq!(Cli::parse(a("bin --weather bogus")).weather, None);
    }
}
