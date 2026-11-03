//! `--record DIR`: deterministic footage, one PNG per frame.
//!
//! Software rendering runs at a few frames per second, so a live screen capture would stutter.
//! Instead the app time advances by exactly `1 / fps` per update
//! ([`TimeUpdateStrategy::ManualDuration`]) and every update is captured, so the frames encode
//! to a video that plays at true game speed. Everything that reads `Time` (sea, gulls, canoes,
//! wind, flutter, rain and snow, weather blends) animates normally.
//!
//! The driver waits for the scene and the render pipelines like `--demo-shot` does (the clock,
//! camera and weather held at clip time 0), then requests `DIR/frame_00001.png …` one per update:
//! frame `n` shows clip time `n / fps`. Screenshots are read back a few frames later; each one is
//! tied to the frame it was requested on and written on a worker thread. The app exits 0 once
//! every file is written, non-zero if any save failed or a frame missed its capture.
//!
//! Clip moves, all optional: `--orbit DEG` (yaw sweep, eased), `--zoom-to Z` (eased),
//! `--hours H` (time-lapse from `--time`, wrapping at midnight; without it the clock runs at
//! normal game speed) and `--weathers a,b,c` (switched at even intervals).
//!
//! With `--tool-smoke` the scripted build run is recorded instead: the smoke waits for
//! [`Recorder::ready`], calls [`Recorder::stop`] when its script is done and exits once
//! [`Recorder::finished`].

use crate::camera::CameraRig;
use crate::shot::{
    Cli, PendingPipelines, READY_FRAMES, Record, SETTLE_FRAMES, TIMEOUT_FRAMES, apply_cli, count_pending, hide_gizmos,
    hide_hud, save_png,
};
use crate::weather::{Clock, Weather};
use crate::world::BuiltScene;
use bevy::prelude::*;
use bevy::render::{
    Render, RenderApp, RenderSystems,
    view::screenshot::{Capturing, Screenshot, ScreenshotCaptured},
};
use bevy::time::TimeUpdateStrategy;
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};
use std::time::Duration;

/// Cap on a smoke recording (about an hour at 30 fps), so a stuck run cannot fill the disk.
const MAX_SMOKE_FRAMES: u32 = 108_000;

pub struct RecordPlugin;

impl Plugin for RecordPlugin {
    fn build(&self, app: &mut App) {
        let Some(cli) = app.world().get_resource::<Cli>().cloned() else { return };
        let Some(spec) = cli.record.clone() else { return };
        if !cli.errors.is_empty() {
            return;
        }
        let smoke = std::env::args().any(|a| a == "--tool-smoke");
        let pending = PendingPipelines::default();
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(1.0 / spec.fps as f64)))
            .insert_resource(Recorder::new(spec, smoke))
            .insert_resource(pending.clone())
            .add_systems(Last, capture);
        if smoke {
            // The build run keeps its own clock, camera, HUD and hover ring.
            app.add_systems(Startup, |rec: Res<Recorder>| info!("recording the tool smoke to {}", rec.spec.dir.display()));
        } else {
            app.add_systems(Startup, (apply_cli, start_weather, hide_gizmos).chain()).add_systems(PreUpdate, drive);
            if !cli.hud {
                app.add_systems(Last, hide_hud.before(capture));
            }
        }
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app.insert_resource(pending).add_systems(Render, count_pending.in_set(RenderSystems::Cleanup));
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    /// Waiting for the scene and pipelines; the clip is held at time 0.
    Warming,
    /// Capturing; the value is the next frame number (1-based).
    Recording(u32),
    /// Every frame requested; waiting for the files.
    Draining,
}

/// Frames saved / failed, shared with the save threads.
#[derive(Default)]
struct Saves {
    ok: AtomicU32,
    failed: AtomicU32,
}

/// Where the clip starts (after `--time` / `--yaw` / `--zoom` are applied).
#[derive(Clone, Copy, Debug)]
struct Start {
    hour: f32,
    yaw: f32,
    zoom: f32,
}

#[derive(Resource)]
pub struct Recorder {
    spec: Record,
    smoke: bool,
    phase: Phase,
    /// Updates since the scene was built.
    frames: u32,
    /// Consecutive updates with every render pipeline compiled.
    ready_frames: u32,
    /// Updates spent waiting for the files.
    drain_frames: u32,
    requested: u32,
    /// Frames whose screenshot was not taken on the update that requested it.
    missed: u32,
    /// A smoke recording was cut off at [`MAX_SMOKE_FRAMES`].
    truncated: bool,
    last_shot: Option<Entity>,
    saves: Arc<Saves>,
    start: Option<Start>,
    stop: bool,
    began: Option<std::time::Instant>,
}

impl Recorder {
    fn new(spec: Record, smoke: bool) -> Self {
        Self {
            spec,
            smoke,
            phase: Phase::Warming,
            frames: 0,
            ready_frames: 0,
            drain_frames: 0,
            requested: 0,
            missed: 0,
            truncated: false,
            last_shot: None,
            saves: Arc::default(),
            start: None,
            stop: false,
            began: None,
        }
    }

    /// The scene is up and the pipelines are compiled: capture has begun.
    pub fn ready(&self) -> bool {
        self.phase != Phase::Warming
    }

    /// Stop requesting frames (the smoke script is done).
    pub fn stop(&mut self) {
        self.stop = true;
    }

    /// `Some(ok)` once recording stopped and every requested frame is on disk (or failed).
    pub fn finished(&self) -> Option<bool> {
        let (ok, failed) = (self.saves.ok.load(Ordering::Acquire), self.saves.failed.load(Ordering::Acquire));
        (self.phase == Phase::Draining && ok + failed >= self.requested).then_some(failed == 0 && self.missed == 0 && !self.truncated)
    }

    /// Whether frame `k` lies past the end of the recording. A smoke run that reaches
    /// [`MAX_SMOKE_FRAMES`] is cut off and marked truncated, so it fails rather than passing
    /// with incomplete footage.
    fn past_end(&mut self, k: u32) -> bool {
        if self.stop {
            return true;
        }
        if !self.smoke {
            return k > self.spec.frames();
        }
        if k > MAX_SMOKE_FRAMES {
            error!("record: the smoke recording reached its cap of {MAX_SMOKE_FRAMES} frames and was cut off");
            self.truncated = true;
            return true;
        }
        false
    }

    /// Gave up waiting for the files.
    pub fn timed_out(&self) -> bool {
        self.drain_frames > TIMEOUT_FRAMES
    }
}

/// Clip state at frame `k` of `n` (frame 0 = the held warm-up frame).
#[derive(Clone, Copy, Debug, PartialEq)]
struct ClipPose {
    hour: f32,
    yaw: Option<f32>,
    zoom: Option<f32>,
    /// Index into `--weathers`.
    weather: Option<usize>,
}

/// Smooth start and stop for camera moves.
fn ease(u: f32) -> f32 {
    let u = u.clamp(0.0, 1.0);
    u * u * (3.0 - 2.0 * u)
}

/// Where the clock, camera and weather are at frame `k` (clip time `k / fps`). `speed` is the
/// normal clock speed in game hours per second, used without `--hours`.
fn pose(spec: &Record, start: Start, speed: f32, k: u32) -> ClipPose {
    let n = spec.frames();
    let t = k as f32 / spec.fps as f32;
    let u = (k as f32 / n as f32).min(1.0);
    let hour = match spec.hours {
        Some(h) => start.hour + h * u,
        None => start.hour + speed * t,
    };
    let w = spec.weathers.len() as u32;
    ClipPose {
        hour: hour.rem_euclid(24.0),
        yaw: (spec.orbit != 0.0).then(|| start.yaw + spec.orbit * ease(u)),
        zoom: spec.zoom_to.map(|z| start.zoom + (z - start.zoom) * ease(u)),
        weather: (w > 0).then(|| ((k.saturating_sub(1) as u64 * w as u64 / n as u64) as u32).min(w - 1) as usize),
    }
}

/// `--weathers` starts on its first weather, snapped.
fn start_weather(rec: Res<Recorder>, mut weather: ResMut<Weather>) {
    if let Some(&k) = rec.spec.weathers.first() {
        weather.kind = k;
        weather.snap = true;
    }
}

/// Puts the clock, camera and weather where the frame about to be rendered belongs.
fn drive(mut rec: ResMut<Recorder>, mut clock: ResMut<Clock>, mut weather: ResMut<Weather>, mut rig: ResMut<CameraRig>) {
    let k = match rec.phase {
        Phase::Warming => 0,
        Phase::Recording(k) => k,
        Phase::Draining => return,
    };
    let start = *rec.start.get_or_insert(Start { hour: clock.hour, yaw: rig.yaw_deg, zoom: rig.zoom });
    let p = pose(&rec.spec, start, clock.speed, k);
    // The clip sets the hour itself every frame; the clock's own tick stays out of it.
    clock.paused = true;
    clock.hour = p.hour;
    weather.auto = false;
    if let Some(i) = p.weather {
        let kind = rec.spec.weathers[i];
        if weather.kind != kind {
            weather.kind = kind;
        }
    }
    if let Some(yaw) = p.yaw {
        rig.set_yaw(yaw);
    }
    if let Some(zoom) = p.zoom {
        rig.set_zoom(zoom);
    }
}

fn capture(
    mut commands: Commands,
    mut rec: ResMut<Recorder>,
    built: Res<BuiltScene>,
    pending: Res<PendingPipelines>,
    shots: Query<Has<Capturing>, With<Screenshot>>,
    mut exit: MessageWriter<AppExit>,
) {
    let rec = &mut *rec;
    // The screenshot requested last update must have been picked up by that frame's extract,
    // otherwise it would show a later frame.
    if let Some(e) = rec.last_shot.take()
        && shots.get(e) == Ok(false)
    {
        error!("record: frame {} was not captured on the frame it was requested on", rec.requested);
        rec.missed += 1;
    }
    match rec.phase {
        Phase::Warming => {
            if built.0.is_none() {
                return;
            }
            rec.frames += 1;
            let waiting = pending.waiting();
            rec.ready_frames = if waiting == 0 { rec.ready_frames + 1 } else { 0 };
            if rec.frames >= SETTLE_FRAMES + TIMEOUT_FRAMES {
                error!("record: {waiting} pipelines still compiling after {} frames", rec.frames);
                rec.phase = Phase::Draining;
                rec.missed += 1;
            } else if rec.frames >= SETTLE_FRAMES && rec.ready_frames >= READY_FRAMES {
                if let Err(e) = std::fs::create_dir_all(&rec.spec.dir) {
                    error!("record: creating {}: {e}", rec.spec.dir.display());
                    exit.write(AppExit::error());
                    return;
                }
                let what = if rec.smoke { "the tool smoke".to_string() } else { format!("{} frames", rec.spec.frames()) };
                info!("record: warm after {} frames, capturing {what} at {} fps to {}", rec.frames, rec.spec.fps, rec.spec.dir.display());
                rec.began = Some(std::time::Instant::now());
                rec.phase = Phase::Recording(1);
            }
        }
        Phase::Recording(k) => {
            if rec.past_end(k) {
                rec.phase = Phase::Draining;
            } else {
                let path = rec.spec.dir.join(format!("frame_{k:05}.png"));
                let saves = rec.saves.clone();
                let id = commands
                    .spawn(Screenshot::primary_window())
                    .observe(move |shot: On<ScreenshotCaptured>| {
                        let (image, path, saves) = (shot.image.clone(), path.clone(), saves.clone());
                        // PNG encoding is slow next to a frame; keep it off the main thread.
                        std::thread::spawn(move || match save_png(image, &path) {
                            Ok(()) => {
                                saves.ok.fetch_add(1, Ordering::Release);
                            }
                            Err(err) => {
                                error!("record: {err}");
                                saves.failed.fetch_add(1, Ordering::Release);
                            }
                        });
                    })
                    .id();
                rec.last_shot = Some(id);
                rec.requested = k;
                rec.phase = Phase::Recording(k + 1);
                if k % 30 == 0 {
                    let secs = rec.began.map_or(0.0, |b| b.elapsed().as_secs_f32());
                    info!("record: frame {k} requested ({:.2} s per frame)", secs / k as f32);
                }
            }
        }
        Phase::Draining => {
            rec.drain_frames += 1;
            if rec.smoke {
                // The smoke run exits on its own once `finished()`.
                return;
            }
            if let Some(ok) = rec.finished() {
                let secs = rec.began.map_or(0.0, |b| b.elapsed().as_secs_f32());
                let saved = rec.saves.ok.load(Ordering::Acquire);
                if ok {
                    info!("record: {saved} frames written to {} in {secs:.1} s ({:.2} s per frame)", rec.spec.dir.display(), secs / saved.max(1) as f32);
                    exit.write(AppExit::Success);
                } else {
                    error!("record: {saved} of {} frames written, {} missed their frame", rec.requested, rec.missed);
                    exit.write(AppExit::error());
                }
            } else if rec.timed_out() {
                error!("record: gave up waiting for screenshots ({} of {} written)", rec.saves.ok.load(Ordering::Acquire), rec.requested);
                exit.write(AppExit::error());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::weather::WeatherKind;

    fn spec(seconds: f32, fps: u32) -> Record {
        Record { dir: "f".into(), seconds, fps, orbit: 0.0, zoom_to: None, hours: None, weathers: Vec::new() }
    }

    const START: Start = Start { hour: 23.0, yaw: 10.0, zoom: 2.0 };

    #[test]
    fn normal_speed_clock_and_time_lapse() {
        let s = spec(2.0, 30);
        assert_eq!(s.frames(), 60);
        // Frame n shows clip time n / fps at the normal clock speed.
        assert!((pose(&s, START, 1.0 / 30.0, 30).hour - (23.0 + 1.0 / 30.0)).abs() < 1e-5);
        assert_eq!(pose(&s, START, 1.0 / 30.0, 0).hour, 23.0);
        let lapse = Record { hours: Some(3.0), ..s };
        // Wraps past midnight and lands exactly on start + hours.
        assert!((pose(&lapse, START, 1.0 / 30.0, 60).hour - 2.0).abs() < 1e-4);
        assert!((pose(&lapse, START, 1.0 / 30.0, 30).hour - 0.5).abs() < 1e-4);
    }

    #[test]
    fn orbit_and_zoom_ease() {
        let s = Record { orbit: 60.0, zoom_to: Some(1.0), ..spec(1.0, 10) };
        let p0 = pose(&s, START, 0.0, 0);
        let (mid, end) = (pose(&s, START, 0.0, 5), pose(&s, START, 0.0, 10));
        assert_eq!((p0.yaw, p0.zoom), (Some(10.0), Some(2.0)));
        assert_eq!((mid.yaw, mid.zoom), (Some(40.0), Some(1.5)));
        assert_eq!((end.yaw, end.zoom), (Some(70.0), Some(1.0)));
        // Eased: the first step is smaller than the middle one.
        let d = |a: u32, b: u32| pose(&s, START, 0.0, b).yaw.unwrap() - pose(&s, START, 0.0, a).yaw.unwrap();
        assert!(d(0, 1) < d(5, 6));
        assert_eq!(pose(&spec(1.0, 10), START, 0.0, 5).yaw, None);
    }

    #[test]
    fn weathers_switch_at_even_intervals() {
        let s = Record { weathers: vec![WeatherKind::Clear, WeatherKind::Storm, WeatherKind::Snow], ..spec(1.0, 30) };
        let at = |k| pose(&s, START, 0.0, k).weather;
        assert_eq!([at(0), at(1), at(10), at(11), at(20), at(21), at(30)], [Some(0), Some(0), Some(0), Some(1), Some(1), Some(2), Some(2)]);
        assert_eq!(pose(&spec(1.0, 30), START, 0.0, 3).weather, None);
    }

    #[test]
    fn smoke_cap_fails_the_recording() {
        // A smoke run that hits the frame cap is cut off and reported as failed.
        let mut capped = Recorder::new(spec(1.0, 30), true);
        assert!(!capped.past_end(MAX_SMOKE_FRAMES));
        assert!(capped.past_end(MAX_SMOKE_FRAMES + 1));
        capped.phase = Phase::Draining;
        assert_eq!(capped.finished(), Some(false));
        // A smoke run whose script stops before the cap passes.
        let mut stopped = Recorder::new(spec(1.0, 30), true);
        stopped.stop();
        assert!(stopped.past_end(5));
        stopped.phase = Phase::Draining;
        assert_eq!(stopped.finished(), Some(true));
        // A clip ends after its last frame.
        let mut clip = Recorder::new(spec(1.0, 30), false);
        assert!(!clip.past_end(30) && clip.past_end(31));
        clip.phase = Phase::Draining;
        assert_eq!(clip.finished(), Some(true));
    }
}
