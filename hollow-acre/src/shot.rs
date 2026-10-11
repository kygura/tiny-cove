//! Command-line options, F12 screenshots and the `--demo-shot` capture-and-exit flow.

use crate::render::{AtmosPreset, Atmosphere};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use std::path::{Path, PathBuf};

/// Parsed command line. Unknown or malformed options are reported and ignored.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct CliArgs {
    /// `--demo-shot <png>`: build the demo scene, save a screenshot there and exit.
    pub demo_shot: Option<PathBuf>,
    /// `--seed N`
    pub seed: Option<u64>,
    /// `--time H`, hours in 0..24.
    pub time: Option<f32>,
    /// `--weather NAME` (validated by the weather system).
    pub weather: Option<String>,
    /// `--atmos PRESET`: debug atmosphere preset (afternoon, dusk, night, overcast, storm, mist).
    pub atmos: Option<AtmosPreset>,
}

impl CliArgs {
    /// Parses `args` (without the program name). Returns the options and one warning per
    /// rejected argument.
    pub fn parse<I: IntoIterator<Item = String>>(args: I) -> (Self, Vec<String>) {
        let mut out = Self::default();
        let mut warnings = Vec::new();
        let mut it = args.into_iter().peekable();
        while let Some(arg) = it.next() {
            let (key, inline) = match arg.split_once('=') {
                Some((k, v)) if k.starts_with("--") => (k.to_string(), Some(v.to_string())),
                _ => (arg.clone(), None),
            };
            let known = matches!(key.as_str(), "--demo-shot" | "--seed" | "--time" | "--weather" | "--atmos");
            if !known {
                warnings.push(format!("unknown argument `{arg}`"));
                continue;
            }
            // A following `--option` is never taken as this option's value.
            let next_is_value = it.peek().is_some_and(|v| !v.starts_with("--"));
            let Some(value) = inline.or_else(|| if next_is_value { it.next() } else { None }) else {
                warnings.push(format!("`{key}` needs a value"));
                continue;
            };
            match key.as_str() {
                "--demo-shot" => {
                    if value.trim().is_empty() {
                        warnings.push(format!("`--demo-shot` needs a file path, got `{value}`"));
                    } else {
                        out.demo_shot = Some(PathBuf::from(value));
                    }
                }
                "--seed" => match value.parse::<u64>() {
                    Ok(s) => out.seed = Some(s),
                    Err(_) => warnings.push(format!("`--seed` expects a non-negative integer, got `{value}`")),
                },
                "--time" => match value.parse::<f32>() {
                    Ok(h) if h.is_finite() && (0.0..24.0).contains(&h) => out.time = Some(h),
                    _ => warnings.push(format!("`--time` expects hours in 0..24, got `{value}`")),
                },
                "--weather" => {
                    if value.trim().is_empty() {
                        warnings.push("`--weather` needs a name".into());
                    } else {
                        out.weather = Some(value.trim().to_string());
                    }
                }
                "--atmos" => match AtmosPreset::from_name(&value) {
                    Some(p) => out.atmos = Some(p),
                    None => warnings.push(format!("unknown `--atmos` preset `{value}`")),
                },
                _ => unreachable!(),
            }
        }
        (out, warnings)
    }

    /// Parses `std::env::args()`, printing warnings to stderr.
    pub fn from_env() -> Self {
        let (args, warnings) = Self::parse(std::env::args().skip(1));
        for w in warnings {
            eprintln!("hollow_acre: {w}");
        }
        args
    }
}

/// Pending demo capture. Frames are only counted while `hold` is false, so the scene
/// system can set `hold = true` until its meshes are ready.
#[derive(Resource, Debug)]
pub struct DemoShot {
    pub path: PathBuf,
    /// Frames to let pipelines compile and the scene settle before capturing.
    pub settle_frames: u32,
    pub hold: bool,
    frames: u32,
    held: u32,
    requested: bool,
}

/// Default settle time before the demo capture.
pub const DEMO_SETTLE_FRAMES: u32 = 90;
/// Exit with an error if the capture hasn't been written this many frames after settling.
const DEMO_TIMEOUT_FRAMES: u32 = 600;
/// Capture anyway (with a warning) after `hold` has been set this many frames.
const DEMO_HOLD_TIMEOUT_FRAMES: u32 = 600;

pub struct ShotPlugin;

impl Plugin for ShotPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<CliArgs>() {
            app.insert_resource(CliArgs::from_env());
        }
        let args = app.world().resource::<CliArgs>().clone();
        if let Some(path) = args.demo_shot {
            app.insert_resource(DemoShot {
                path,
                settle_frames: DEMO_SETTLE_FRAMES,
                hold: false,
                frames: 0,
                held: 0,
                requested: false,
            })
            .add_systems(Last, demo_shot);
        }
        if let Some(preset) = args.atmos {
            app.add_systems(Startup, move |mut atmos: ResMut<Atmosphere>| {
                *atmos = Atmosphere::preset(preset);
            });
        }
        app.add_systems(Update, screenshot_on_f12);
    }
}

/// Saves `image` as PNG/whatever the extension says, creating parent directories.
pub fn save_image(image: Image, path: &Path) -> Result<(), String> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    }
    let img = image
        .try_into_dynamic()
        .map_err(|e| format!("converting screenshot: {e}"))?;
    img.to_rgb8()
        .save(path)
        .map_err(|e| format!("writing {}: {e}", path.display()))
}

/// Captures the primary window (upscaled image + UI) and writes it to `path`.
/// Attach more observers (e.g. `On<ScreenshotCaptured>`) to react after capture.
pub fn request_screenshot<'a>(commands: &'a mut Commands, path: PathBuf) -> EntityCommands<'a> {
    let mut e = commands.spawn(Screenshot::primary_window());
    e.observe(move |shot: On<ScreenshotCaptured>| match save_image(shot.image.clone(), &path) {
        Ok(()) => info!("screenshot saved to {}", path.display()),
        Err(err) => error!("screenshot failed: {err}"),
    });
    e
}

fn screenshot_on_f12(mut commands: Commands, keys: Res<ButtonInput<KeyCode>>) {
    if keys.just_pressed(KeyCode::F12) {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        request_screenshot(&mut commands, PathBuf::from(format!("screenshots/{ts}.png")));
    }
}

fn demo_shot(mut commands: Commands, mut shot: ResMut<DemoShot>, mut exit: MessageWriter<AppExit>) {
    if shot.hold && shot.held < DEMO_HOLD_TIMEOUT_FRAMES {
        shot.held += 1;
        if shot.held == DEMO_HOLD_TIMEOUT_FRAMES {
            warn!("demo shot: scene still holding after {DEMO_HOLD_TIMEOUT_FRAMES} frames; capturing anyway");
        }
        return;
    }
    shot.frames += 1;
    if shot.frames >= shot.settle_frames + DEMO_TIMEOUT_FRAMES {
        error!("demo shot: screenshot never arrived");
        exit.write(AppExit::error());
        return;
    }
    if shot.frames >= shot.settle_frames && !shot.requested {
        shot.requested = true;
        let path = shot.path.clone();
        info!("demo shot -> {}", path.display());
        commands.spawn(Screenshot::primary_window()).observe(
            move |captured: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
                match save_image(captured.image.clone(), &path) {
                    Ok(()) => {
                        info!("demo shot saved to {}", path.display());
                        exit.write(AppExit::Success);
                    }
                    Err(err) => {
                        error!("demo shot failed: {err}");
                        exit.write(AppExit::error());
                    }
                }
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> (CliArgs, Vec<String>) {
        CliArgs::parse(s.split_whitespace().map(String::from))
    }

    #[test]
    fn parses_all_options() {
        let (a, w) = parse("--demo-shot out.png --seed 7 --time 22.5 --weather rain --atmos night");
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(a.demo_shot, Some(PathBuf::from("out.png")));
        assert_eq!(a.seed, Some(7));
        assert_eq!(a.time, Some(22.5));
        assert_eq!(a.weather.as_deref(), Some("rain"));
        assert_eq!(a.atmos, Some(AtmosPreset::Night));
    }

    #[test]
    fn rejects_bad_values() {
        let (a, w) = parse("--time 25 --seed -1 --bogus --atmos x --demo-shot");
        assert_eq!(a, CliArgs::default());
        assert_eq!(w.len(), 5, "{w:?}");
    }

    #[test]
    fn option_is_not_a_value() {
        let (a, w) = parse("--demo-shot --seed 3");
        assert_eq!(a.demo_shot, None);
        assert_eq!(a.seed, Some(3));
        assert_eq!(w.len(), 1, "{w:?}");
    }

    #[test]
    fn accepts_equals_form() {
        let (a, w) = parse("--seed=42 --time=0");
        assert!(w.is_empty());
        assert_eq!((a.seed, a.time), (Some(42), Some(0.0)));
    }
}
