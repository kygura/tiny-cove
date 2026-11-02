//! Command line (`--demo`, `--demo-shot`, …) and the headless demo-shot driver.
//!
//! `--demo-shot out.png [--time H] [--weather NAME] [--seed N] [--yaw D] [--zoom S]` builds the
//! demo glade, freezes the clock/weather, waits for the scene plus a few frames, saves a
//! screenshot and exits. `--demo` just starts interactively with the demo glade.

use crate::camera::CameraRig;
use crate::render::save_image;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use crate::weather::{Clock, Weather, WeatherKind};
use crate::world::BuiltScene;
use bevy::prelude::*;
use std::path::PathBuf;

/// Frames to wait after the scene exists before capturing (shadows, fog, smoothing settle).
const SETTLE_FRAMES: u32 = 30;
/// Give up if the screenshot never arrives.
const TIMEOUT_FRAMES: u32 = 600;

#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct Cli {
    pub demo: bool,
    /// `--showcase`: every primitive variant in a grid (visual QA).
    pub showcase: bool,
    /// `--focus X,Z`: camera focus on the ground plane.
    pub focus: Option<Vec2>,
    pub shot: Option<PathBuf>,
    pub time: Option<f32>,
    pub weather: Option<WeatherKind>,
    pub seed: Option<u64>,
    pub yaw: Option<f32>,
    pub zoom: Option<f32>,
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
            showcase: args.iter().any(|a| a == "--showcase"),
            focus: value("--focus").and_then(|v| {
                let (x, z) = v.split_once(',')?;
                Some(Vec2::new(x.trim().parse().ok()?, z.trim().parse().ok()?))
            }),
            shot: value("--demo-shot").map(PathBuf::from),
            time: num(value("--time")),
            weather: value("--weather").and_then(|w| WeatherKind::parse(w)),
            seed: num(value("--seed")),
            yaw: num(value("--yaw")),
            zoom: num(value("--zoom")),
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
        app.insert_resource(ShotState { path, frames: 0, requested: false, retries: 0 })
            .add_systems(Startup, apply_cli)
            .add_systems(Last, capture);
    }
}

#[derive(Resource)]
struct ShotState {
    path: PathBuf,
    frames: u32,
    requested: bool,
    retries: u32,
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
    if let Some(f) = cli.focus {
        rig.focus = Vec3::new(f.x, 0.0, f.y);
    }
}

fn capture(mut commands: Commands, mut st: ResMut<ShotState>, built: Res<BuiltScene>, mut exit: MessageWriter<AppExit>) {
    if built.0.is_none() {
        return;
    }
    st.frames += 1;
    if st.frames >= SETTLE_FRAMES + TIMEOUT_FRAMES {
        error!("demo shot: screenshot never arrived");
        exit.write(AppExit::error());
    } else if st.frames >= SETTLE_FRAMES && !st.requested {
        st.requested = true;
        // Headless software rendering occasionally hands back an all-black first frame:
        // check the capture and retry a few times before saving.
        commands.spawn(Screenshot::primary_window()).observe(
            |shot: On<ScreenshotCaptured>, mut st: ResMut<ShotState>, mut exit: MessageWriter<AppExit>| {
                if is_black(&shot.image) && st.retries < MAX_RETRIES {
                    st.retries += 1;
                    st.requested = false;
                    st.frames = SETTLE_FRAMES - 10;
                    warn!("demo shot: black frame, retrying ({}/{MAX_RETRIES})", st.retries);
                    return;
                }
                match save_image(shot.image.clone(), &st.path) {
                    Ok(()) => {
                        info!("screenshot saved to {}", st.path.display());
                        exit.write(AppExit::Success);
                    }
                    Err(err) => {
                        error!("screenshot failed: {err}");
                        exit.write(AppExit::error());
                    }
                }
            },
        );
    }
}

const MAX_RETRIES: u32 = 5;

/// True if every pixel is (near) black.
fn is_black(image: &Image) -> bool {
    image.data.as_ref().is_none_or(|d| d.chunks(4).all(|p| p.iter().take(3).all(|&c| c < 3)))
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
        assert!(!c.demo);
    }

    #[test]
    fn plain_and_demo() {
        assert_eq!(Cli::parse(a("bin")), Cli::default());
        assert!(Cli::parse(a("bin --demo")).demo);
        assert_eq!(Cli::parse(a("bin --seed 9")).seed(), 9);
        assert_eq!(Cli::parse(a("bin --weather bogus")).weather, None);
    }
}
