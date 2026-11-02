//! Command line (`--demo`, `--demo-shot`, …) and the headless demo-shot driver.
//!
//! `--demo-shot out.png [--time H] [--weather NAME] [--seed N] [--yaw D] [--zoom S] [--focus X,Z] [--hud]`
//! builds the demo glade, freezes the clock/weather, waits for the scene plus a few frames,
//! saves a screenshot and exits (status 1 if the PNG could not be written). `--zoom` below 1
//! and `--focus` (ground x,z in metres) frame close-ups. Tool gizmos (the hover ring) are hidden
//! in shots. `--demo` just starts interactively with the demo glade.

use crate::camera::CameraRig;
use crate::render::LowResDisplay;
use crate::weather::{Clock, Weather, WeatherKind};
use crate::world::BuiltScene;
use bevy::prelude::*;
use bevy::render::{
    Render, RenderApp, RenderSystems,
    render_resource::{CachedPipelineState, PipelineCache},
    view::screenshot::{Screenshot, ScreenshotCaptured},
};
use std::path::{Path, PathBuf};
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
    /// Camera focus on the ground (x, z), for close-ups.
    pub focus: Option<Vec2>,
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
            focus: value("--focus").and_then(|v| {
                let (x, z) = v.split_once(',')?;
                let p = Vec2::new(x.trim().parse().ok()?, z.trim().parse().ok()?);
                p.is_finite().then_some(p)
            }),
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
            .add_systems(Startup, (apply_cli, hide_gizmos))
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
    if let Some(zoom) = cli.zoom.filter(|z| z.is_finite()) {
        rig.set_zoom(zoom);
    }
    if let Some(f) = cli.focus {
        rig.focus = Vec3::new(f.x, 0.0, f.y);
    }
}

/// Demo shots show the glade only: no tool hover ring or other gizmos.
fn hide_gizmos(store: Option<ResMut<GizmoConfigStore>>) {
    for (_, config, _) in store.into_iter().flat_map(|s| s.into_inner().iter_mut()) {
        config.enabled = false;
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
        let path = st.path.clone();
        commands.spawn(Screenshot::primary_window()).observe(move |shot: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
            // Scripts rely on the exit status: no PNG, no success.
            match save_png(shot.image.clone(), &path) {
                Ok(()) => {
                    info!("screenshot saved to {}", path.display());
                    exit.write(AppExit::Success);
                }
                Err(err) => {
                    error!("demo shot failed: {err}");
                    exit.write(AppExit::error());
                }
            }
        });
    }
}

/// Writes `image` as a PNG to `path`, creating its folder.
fn save_png(image: Image, path: &Path) -> Result<(), String> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    }
    let img = image.try_into_dynamic().map_err(|e| format!("converting screenshot: {e}"))?;
    img.to_rgb8().save(path).map_err(|e| format!("writing {}: {e}", path.display()))
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
    fn parses_focus_and_rejects_garbage() {
        assert_eq!(Cli::parse(a("bin --demo-shot x.png --focus 12.5,-3")).focus, Some(Vec2::new(12.5, -3.0)));
        for bad in ["12", "a,b", "1,NaN", ",", "1,2,3"] {
            assert_eq!(Cli::parse(a(&format!("bin --focus {bad}"))).focus, None, "{bad}");
        }
    }

    #[test]
    fn unwritable_shot_path_is_an_error() {
        let img = Image::new_fill(
            bevy::render::render_resource::Extent3d { width: 2, height: 2, depth_or_array_layers: 1 },
            bevy::render::render_resource::TextureDimension::D2,
            &[10, 20, 30, 255],
            bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
            bevy::asset::RenderAssetUsages::default(),
        );
        assert!(save_png(img.clone(), Path::new("/proc/nope/x.png")).is_err());
        let ok = std::env::temp_dir().join(format!("cairn-shot-{}.png", std::process::id()));
        assert!(save_png(img, &ok).is_ok() && ok.exists());
        let _ = std::fs::remove_file(ok);
    }

    #[test]
    fn plain_and_demo() {
        assert_eq!(Cli::parse(a("bin")), Cli::default());
        assert!(Cli::parse(a("bin --demo")).demo);
        assert_eq!(Cli::parse(a("bin --seed 9")).seed(), 9);
        assert_eq!(Cli::parse(a("bin --weather bogus")).weather, None);
    }
}
