//! Command line (`--demo`, `--demo-shot`, …) and the headless demo-shot driver.
//!
//! `--demo-shot out.png [--time H] [--weather NAME] [--seed N] [--yaw D] [--zoom S]` builds the
//! demo glade, freezes the clock/weather, waits for the scene plus a few frames, saves a
//! screenshot and exits. `--demo` just starts interactively with the demo glade.

use crate::camera::CameraRig;
use crate::render::request_screenshot;
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
        app.insert_resource(ShotState { path, frames: 0, requested: false })
            .add_systems(Startup, apply_cli)
            .add_systems(Last, (hide_hud, capture));
    }
}

#[derive(Resource)]
struct ShotState {
    path: PathBuf,
    frames: u32,
    requested: bool,
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
fn hide_hud(mut roots: Query<&mut Visibility, (With<Node>, Without<ChildOf>)>) {
    for mut v in &mut roots {
        if *v != Visibility::Hidden {
            *v = Visibility::Hidden;
        }
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
