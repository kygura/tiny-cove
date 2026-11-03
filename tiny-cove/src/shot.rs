//! Command line (`--demo`, `--demo-shot`, …) and the headless demo-shot driver.
//!
//! `--demo-shot out.png [--time H] [--weather NAME] [--seed N] [--yaw D] [--zoom S] [--focus X,Z] [--hud]`
//! builds the demo glade, freezes the clock/weather, waits for the scene plus a few frames,
//! saves a screenshot and exits (status 1 if the PNG could not be written). `--zoom` below 1
//! and `--focus` (ground x,z in metres) frame close-ups. Tool gizmos (the hover ring) are hidden
//! in shots. `--demo` just starts interactively with the demo glade.
//!
//! `--record DIR [--seconds S] [--fps F] [--orbit DEG] [--zoom-to Z] [--hours H] [--weathers a,b]`
//! records a deterministic clip of the demo glade frame by frame (see [`crate::record`]); with
//! `--tool-smoke` it records the scripted build run instead.

use crate::camera::{CameraRig, MAX_ZOOM, MIN_ZOOM};
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
pub(crate) const SETTLE_FRAMES: u32 = 30;
/// Give up if the screenshot never arrives.
pub(crate) const TIMEOUT_FRAMES: u32 = 600;
/// Consecutive frames with no pipeline compiling before capturing (render runs a frame behind).
pub(crate) const READY_FRAMES: u32 = 5;

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
    /// `--record DIR` and its clip options.
    pub record: Option<Record>,
    /// Problems with the command line; `main` prints them and exits non-zero.
    pub errors: Vec<String>,
}

/// A `--record` clip: `frames()` frames at `fps`, frame `n` showing clip time `n / fps`.
#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub dir: PathBuf,
    pub seconds: f32,
    pub fps: u32,
    /// Total yaw sweep across the clip in degrees (eased in and out).
    pub orbit: f32,
    /// Zoom at the end of the clip (eased from the starting zoom).
    pub zoom_to: Option<f32>,
    /// Time-lapse: game hours the clock covers across the clip (else normal game speed).
    pub hours: Option<f32>,
    /// Weathers switched to at even intervals (the weather system blends between them).
    pub weathers: Vec<WeatherKind>,
}

impl Record {
    pub const DEFAULT_SECONDS: f32 = 8.0;
    pub const DEFAULT_FPS: u32 = 30;

    /// Number of frames in the clip (at least one).
    pub fn frames(&self) -> u32 {
        (self.seconds * self.fps as f32).round().max(1.0) as u32
    }
}

/// Flags that only make sense with `--record`.
const RECORD_FLAGS: [&str; 6] = ["--seconds", "--fps", "--orbit", "--zoom-to", "--hours", "--weathers"];

impl Cli {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Self {
        let args: Vec<String> = args.into_iter().collect();
        let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1));
        fn num<T: std::str::FromStr>(v: Option<&String>) -> Option<T> {
            v?.parse().ok()
        }
        let has = |flag: &str| args.iter().any(|a| a == flag);
        let mut errors = Vec::new();
        let record = Self::parse_record(&args, &mut errors);
        if record.is_some() && has("--demo-shot") {
            errors.push("--record and --demo-shot cannot be combined".into());
        }
        Self {
            // A recorded clip shows the demo glade (the smoke run builds its own).
            demo: has("--demo") || (record.is_some() && !has("--tool-smoke")),
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
            record,
            errors,
        }
    }

    /// `--record DIR` plus its options; bad or stray values go to `errors`.
    fn parse_record(args: &[String], errors: &mut Vec<String>) -> Option<Record> {
        let value = |flag: &str| args.iter().position(|a| a == flag).map(|i| args.get(i + 1).filter(|v| !v.starts_with("--")));
        let Some(dir) = value("--record") else {
            for flag in RECORD_FLAGS.iter().filter(|f| args.iter().any(|a| a == *f)) {
                errors.push(format!("{flag} needs --record DIR"));
            }
            return None;
        };
        let mut bad = |msg: String| errors.push(msg);
        let dir = dir.cloned().map(PathBuf::from);
        if dir.is_none() {
            bad("--record needs an output folder".into());
        }
        // `Ok(None)`: flag absent. `Err`: present with a missing or invalid value.
        let number = |flag: &str, ok: &dyn Fn(f32) -> bool| -> Result<Option<f32>, ()> {
            match value(flag) {
                None => Ok(None),
                Some(v) => v.and_then(|v| v.parse::<f32>().ok()).filter(|x| x.is_finite() && ok(*x)).map(Some).ok_or(()),
            }
        };
        let seconds = number("--seconds", &|s| s > 0.0 && s <= 3600.0).unwrap_or_else(|_| {
            bad("--seconds expects a number of seconds in (0, 3600]".into());
            None
        });
        let fps = match value("--fps") {
            None => None,
            Some(v) => {
                let fps = v.and_then(|v| v.parse::<u32>().ok()).filter(|f| (1..=120).contains(f));
                if fps.is_none() {
                    bad("--fps expects a whole number from 1 to 120".into());
                }
                fps
            }
        };
        let orbit = number("--orbit", &|d| d.abs() <= 3600.0).unwrap_or_else(|_| {
            bad("--orbit expects degrees within ±3600".into());
            None
        });
        // Inside the camera's own range, or the eased move would stall at its clamp.
        let zoom_to = number("--zoom-to", &|z| (MIN_ZOOM..=MAX_ZOOM).contains(&z)).unwrap_or_else(|_| {
            bad(format!("--zoom-to expects a zoom from {MIN_ZOOM} to {MAX_ZOOM}"));
            None
        });
        let hours = number("--hours", &|h| h.abs() <= 240.0).unwrap_or_else(|_| {
            bad("--hours expects game hours within ±240".into());
            None
        });
        let weathers = match value("--weathers") {
            None => Vec::new(),
            Some(v) => {
                let names: Vec<&str> = v.map_or(vec![""], |v| v.split(',').map(str::trim).collect());
                let kinds: Vec<_> = names.iter().map(|n| WeatherKind::parse(n)).collect();
                if names.iter().any(|n| n.is_empty()) || kinds.iter().any(Option::is_none) {
                    let names: Vec<&str> = WeatherKind::ALL.iter().map(|k| k.name()).collect();
                    bad(format!("--weathers expects a comma list of {}", names.join(", ")));
                    Vec::new()
                } else {
                    kinds.into_iter().flatten().collect()
                }
            }
        };
        Some(Record {
            dir: dir?,
            seconds: seconds.unwrap_or(Record::DEFAULT_SECONDS),
            fps: fps.unwrap_or(Record::DEFAULT_FPS),
            orbit: orbit.unwrap_or(0.0),
            zoom_to,
            hours,
            weathers,
        })
    }

    /// `--seed`, else fixed for reproducible shots, else derived from the clock.
    pub fn seed(&self) -> u64 {
        self.seed.unwrap_or_else(|| {
            if self.shot.is_some() || self.record.is_some() {
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
pub(crate) struct PendingPipelines(pub(crate) Arc<AtomicU32>);

impl PendingPipelines {
    /// Pipelines the render world reported as still compiling last frame.
    pub(crate) fn waiting(&self) -> u32 {
        self.0.load(Ordering::Relaxed)
    }
}

pub(crate) fn count_pending(cache: Res<PipelineCache>, pending: Res<PendingPipelines>) {
    let n = cache.pipelines().filter(|p| matches!(p.state, CachedPipelineState::Queued | CachedPipelineState::Creating(_))).count()
        + cache.waiting_pipelines().count();
    pending.0.store(n as u32, Ordering::Relaxed);
}

pub(crate) fn apply_cli(cli: Res<Cli>, mut clock: ResMut<Clock>, mut weather: ResMut<Weather>, mut rig: ResMut<CameraRig>) {
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
pub(crate) fn hide_gizmos(store: Option<ResMut<GizmoConfigStore>>) {
    for (_, config, _) in store.into_iter().flat_map(|s| s.into_inner().iter_mut()) {
        config.enabled = false;
    }
}

/// Demo shots show the glade only: hide every root UI node (the HUD).
pub(crate) fn hide_hud(mut roots: Query<&mut Visibility, (With<Node>, Without<ChildOf>, Without<LowResDisplay>)>) {
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
    let waiting = pending.waiting();
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
pub(crate) fn save_png(image: Image, path: &Path) -> Result<(), String> {
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
    fn parses_record_defaults() {
        let c = Cli::parse(a("bin --record out/frames --time 17.5 --yaw 90"));
        let r = c.record.clone().expect("record");
        assert_eq!(r, Record { dir: "out/frames".into(), seconds: 8.0, fps: 30, orbit: 0.0, zoom_to: None, hours: None, weathers: vec![] });
        assert_eq!(r.frames(), 240);
        assert!(c.errors.is_empty() && c.demo && c.shot.is_none());
        assert_eq!((c.time, c.yaw, c.seed()), (Some(17.5), Some(90.0), 7));
        // The smoke run builds its own scene.
        assert!(!Cli::parse(a("bin --tool-smoke s.png --record f")).demo);
    }

    #[test]
    fn parses_record_options() {
        let c = Cli::parse(a("bin --record f --seconds 1.5 --fps 24 --orbit -60 --zoom-to 0.8 --hours 17 --weathers clear,Rain,seafog,snow"));
        assert!(c.errors.is_empty(), "{:?}", c.errors);
        let r = c.record.unwrap();
        assert_eq!((r.seconds, r.fps, r.orbit, r.zoom_to, r.hours), (1.5, 24, -60.0, Some(0.8), Some(17.0)));
        assert_eq!(r.weathers, vec![WeatherKind::Clear, WeatherKind::Rain, WeatherKind::Fog, WeatherKind::Snow]);
        assert_eq!(r.frames(), 36);
        assert_eq!(Cli::parse(a("bin --record f --seconds 0.01 --fps 1")).record.unwrap().frames(), 1);
    }

    #[test]
    fn rejects_bad_record_values() {
        for bad in [
            "--seconds 0",
            "--seconds -2",
            "--seconds abc",
            "--seconds NaN",
            "--seconds 99999",
            "--seconds",
            "--fps 0",
            "--fps 2.5",
            "--fps 500",
            "--fps x",
            "--orbit inf",
            "--orbit spin",
            "--zoom-to 0",
            "--zoom-to -1",
            "--zoom-to 0.1",
            "--zoom-to 10",
            "--hours 1e9",
            "--hours later",
            "--weathers clear,hail",
            "--weathers clear,,rain",
            "--weathers",
        ] {
            let c = Cli::parse(a(&format!("bin --record f {bad}")));
            assert_eq!(c.errors.len(), 1, "{bad}: {:?}", c.errors);
        }
        // Options without --record, --record without a folder, and --record with --demo-shot.
        assert_eq!(Cli::parse(a("bin --seconds 3 --orbit 20")).errors.len(), 2);
        assert_eq!(Cli::parse(a("bin --record")).errors.len(), 1);
        assert_eq!(Cli::parse(a("bin --record --seconds 2")).errors.len(), 1);
        assert_eq!(Cli::parse(a("bin --record f --demo-shot x.png")).errors.len(), 1);
        assert!(Cli::parse(a("bin --demo-shot x.png --weather rain")).errors.is_empty());
    }

    #[test]
    fn plain_and_demo() {
        assert_eq!(Cli::parse(a("bin")), Cli::default());
        assert!(Cli::parse(a("bin --demo")).demo);
        assert_eq!(Cli::parse(a("bin --seed 9")).seed(), 9);
        assert_eq!(Cli::parse(a("bin --weather bogus")).weather, None);
    }
}
