//! Creatures of the ambient life: gull flight, crab scuttling, canoe bobbing and the small
//! meshes they are drawn with. Movement is plain data in / data out (no ECS), so it is
//! deterministic and unit-tested; `life.rs` only moves entities to the results.

use super::fx::tok;
use super::{approach, turn_towards};
use bevy::prelude::*;
use cairn_cove_core::{
    ClutterKind, Colors, MeshData, PropKind, Surface, TRIM_COLORS, SEA_LEVEL,
    mesh::Rng,
    model::Placed,
    nature::placed_mesh,
    palette,
    terrain::Terrain,
};

// ---------------------------------------------------------------- gulls

/// What the gulls can see of the island.
#[derive(Default, Debug, Clone)]
pub struct GullWorld {
    /// Tops of beacons / tall structures: gulls circle them.
    pub towers: Vec<Vec3>,
    /// Perch markers (ridges, dyke tops, beacon tops): gulls land here.
    pub perches: Vec<Vec3>,
    /// Open-sea sample points gulls wander over.
    pub sea: Vec<Vec2>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GullMode {
    /// Circling a tower top.
    Circle { center: Vec3, r: f32, dir: f32, alt: f32 },
    /// Wandering towards a point over the sea.
    Roam { target: Vec3 },
    /// Gliding down to a perch.
    Land { perch: usize },
    /// Sitting on a perch.
    Perched { perch: usize },
}

#[derive(Clone, Debug)]
pub struct Gull {
    pub rng: Rng,
    pub pos: Vec3,
    /// Yaw (rad): the direction the beak points, `atan2(x, z)`.
    pub heading: f32,
    pub bank: f32,
    pub mode: GullMode,
    pub timer: f32,
}

const CRUISE: f32 = 3.2;
const TURN_RATE: f32 = 2.2;

impl Gull {
    pub fn new(mut rng: Rng, w: &GullWorld, sea_level: f32) -> Self {
        let a = rng.range(0.0, std::f32::consts::TAU);
        let pos = Vec3::new(a.cos() * 8.0, sea_level + 6.0 + rng.range(0.0, 2.0), a.sin() * 8.0);
        let mut g = Self { rng, pos, heading: a + 1.5, bank: 0.0, mode: GullMode::Roam { target: pos }, timer: 0.0 };
        g.choose(w, sea_level, &|_| false);
        g
    }

    /// True when the gull sits on or is gliding to perch `i`.
    pub fn holds(&self, i: usize) -> bool {
        matches!(self.mode, GullMode::Land { perch } | GullMode::Perched { perch } if perch == i)
    }

    pub fn is_perched(&self) -> bool {
        matches!(self.mode, GullMode::Perched { .. })
    }

    /// Picks the next behaviour: land on a free perch, circle a tower or wander over the sea.
    pub fn choose(&mut self, w: &GullWorld, sea_level: f32, taken: &dyn Fn(usize) -> bool) {
        let free: Vec<usize> = (0..w.perches.len()).filter(|&i| !taken(i)).collect();
        let roll = self.rng.f();
        if roll < 0.3 && !free.is_empty() {
            let perch = free[(self.rng.next_u64() % free.len() as u64) as usize];
            self.mode = GullMode::Land { perch };
            self.timer = 40.0;
        } else if roll < 0.65 && !w.towers.is_empty() {
            let center = w.towers[(self.rng.next_u64() % w.towers.len() as u64) as usize];
            let dir = if self.rng.f() < 0.5 { 1.0 } else { -1.0 };
            self.mode = GullMode::Circle { center, r: self.rng.range(3.5, 6.5), dir, alt: self.rng.range(2.5, 5.0) };
            self.timer = self.rng.range(14.0, 30.0);
        } else {
            self.mode = GullMode::Roam { target: self.sea_target(w, sea_level) };
            self.timer = self.rng.range(10.0, 25.0);
        }
    }

    fn sea_target(&mut self, w: &GullWorld, sea_level: f32) -> Vec3 {
        let y = sea_level + self.rng.range(4.0, 8.0);
        if w.sea.is_empty() {
            return Vec3::new(self.rng.range(-10.0, 10.0), y, self.rng.range(-10.0, 10.0));
        }
        let p = w.sea[(self.rng.next_u64() % w.sea.len() as u64) as usize];
        Vec3::new(p.x, y, p.y)
    }

    /// Advances `dt` seconds. `taken(i)` says whether another gull holds perch `i`.
    pub fn step(&mut self, dt: f32, w: &GullWorld, sea_level: f32, wind: Vec2, taken: &dyn Fn(usize) -> bool) {
        self.timer -= dt;
        match self.mode {
            GullMode::Perched { perch } => {
                let Some(&p) = w.perches.get(perch) else { return self.choose(w, sea_level, taken) };
                self.pos = p;
                self.bank = approach(self.bank, 0.0, 2.0, dt);
                if wind.length() > 0.1 {
                    self.heading = turn_towards(self.heading, (-wind.x).atan2(-wind.y), 1.5 * dt);
                }
                if self.timer <= 0.0 {
                    self.choose(w, sea_level, taken);
                }
            }
            GullMode::Land { perch } => {
                // Another gull got there first (or the perch is gone): pick something else.
                let Some(&p) = w.perches.get(perch).filter(|_| !taken(perch)) else { return self.choose(w, sea_level, taken) };
                if (p - self.pos).length() < 0.12 {
                    self.pos = p;
                    self.mode = GullMode::Perched { perch };
                    self.timer = self.rng.range(6.0, 16.0);
                } else {
                    self.fly(dt, p, 2.6, (p - self.pos).xz().length() < 3.0);
                    if self.timer <= 0.0 {
                        self.choose(w, sea_level, taken);
                    }
                }
            }
            GullMode::Circle { center, r, dir, alt } => {
                let a = (self.pos.z - center.z).atan2(self.pos.x - center.x) + dir * 0.7;
                let goal = Vec3::new(center.x + a.cos() * r, center.y + alt, center.z + a.sin() * r);
                self.fly(dt, goal, CRUISE, false);
                if self.timer <= 0.0 {
                    self.choose(w, sea_level, taken);
                }
            }
            GullMode::Roam { target } => {
                let mut goal = target;
                if (target - self.pos).xz().length() < 2.0 {
                    goal = self.sea_target(w, sea_level);
                    self.mode = GullMode::Roam { target: goal };
                }
                self.fly(dt, goal, CRUISE, false);
                if self.timer <= 0.0 {
                    self.choose(w, sea_level, taken);
                }
            }
        }
    }

    /// Flies towards `goal`: turning at a limited rate (or straight at it when `direct`).
    fn fly(&mut self, dt: f32, goal: Vec3, speed: f32, direct: bool) {
        let flat = (goal - self.pos).xz();
        let want = flat.x.atan2(flat.y);
        let mut reach = f32::MAX;
        if direct {
            if flat.length() > 1e-3 {
                self.heading = want;
            }
            reach = flat.length();
        } else if flat.length() > 1e-3 {
            let new = turn_towards(self.heading, want, TURN_RATE * dt);
            let rate = (new - self.heading) / dt.max(1e-4);
            self.bank = approach(self.bank, (-rate * 0.35).clamp(-0.6, 0.6), 3.0, dt);
            self.heading = new;
        }
        let step = (speed * dt).min(reach);
        self.pos.x += self.heading.sin() * step;
        self.pos.z += self.heading.cos() * step;
        self.pos.y = approach(self.pos.y, goal.y, 1.6, dt);
    }

    /// Wing flap angle (rad) for time `t`: soaring in circles, flapping elsewhere, folded on a perch.
    pub fn wing_angle(&self, t: f32, phase: f32) -> f32 {
        match self.mode {
            GullMode::Perched { .. } => -0.9,
            GullMode::Circle { .. } => 0.2 + 0.2 * (t * 3.0 + phase).sin(),
            _ => 0.15 + 0.6 * (t * 9.0 + phase).sin(),
        }
    }
}

// ---------------------------------------------------------------- crabs

/// Terrain as a crab sees it.
pub struct Shore<'a> {
    pub height: &'a dyn Fn(Vec2) -> f32,
    /// Unit direction towards the sea (downhill), or zero on flat ground.
    pub seaward: &'a dyn Fn(Vec2) -> Vec2,
    /// Dry, unobstructed and inside the glade.
    pub ok: &'a dyn Fn(Vec2) -> bool,
}

/// Crabs hold this height above the current tide line (m): the wet-sand band.
pub const CRAB_BAND: f32 = 0.12;

#[derive(Clone, Debug)]
pub struct Crab {
    pub rng: Rng,
    pub pos: Vec2,
    /// Yaw (rad): crabs face the sea, so they move sideways along the waterline.
    pub yaw: f32,
    pub side: f32,
    pub move_t: f32,
    pub pause: f32,
}

impl Crab {
    pub fn new(mut rng: Rng, pos: Vec2) -> Self {
        let side = if rng.f() < 0.5 { 1.0 } else { -1.0 };
        let pause = rng.range(0.0, 2.0);
        Self { rng, pos, yaw: 0.0, side, move_t: 0.0, pause }
    }

    /// Advances `dt` seconds on `shore` at tide level `tide`; returns true while scuttling.
    pub fn step(&mut self, dt: f32, tide: f32, shore: &Shore) -> bool {
        let down = (shore.seaward)(self.pos);
        if down.length_squared() > 0.25 {
            self.yaw = turn_towards(self.yaw, down.x.atan2(down.y), 4.0 * dt);
            // Follow the tide line: slide seaward when too high, landward when too low.
            let err = (shore.height)(self.pos) - (tide + CRAB_BAND);
            let next = self.pos + down * (err * 2.0).clamp(-0.5, 0.5) * dt;
            if (shore.ok)(next) {
                self.pos = next;
            }
        }
        let mut moving = false;
        if self.pause > 0.0 {
            self.pause -= dt;
        } else if self.move_t > 0.0 {
            // Sideways along the shore: perpendicular to the facing direction.
            let tangent = Vec2::new(self.yaw.cos(), -self.yaw.sin()) * self.side;
            let next = self.pos + tangent * 0.9 * dt;
            if (shore.ok)(next) {
                self.pos = next;
                moving = true;
            } else {
                self.side = -self.side;
                self.move_t = 0.0;
                self.pause = 0.4;
            }
            self.move_t -= dt;
            if self.move_t <= 0.0 && self.pause <= 0.0 {
                self.pause = self.rng.range(0.8, 3.0);
            }
        } else {
            self.move_t = self.rng.range(0.5, 1.6);
            if self.rng.f() < 0.4 {
                self.side = -self.side;
            }
        }
        moving
    }
}

// ---------------------------------------------------------------- canoes

/// Bobbing and rolling of a floating canoe (all small angles in radians).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoatPose {
    pub y: f32,
    pub pitch: f32,
    pub roll: f32,
    pub yaw: f32,
}

/// Swell pose at time `t`; `phase` de-syncs boats and a stronger wind makes more motion.
pub fn boat_pose(t: f32, phase: f32, wind_speed: f32) -> BoatPose {
    let w = wind_speed.max(0.0);
    let a = (0.03 + 0.012 * w).min(0.12);
    BoatPose {
        y: a * (t * 1.5 + phase).sin() + 0.4 * a * (t * 2.3 + phase * 1.7).sin(),
        roll: (0.05 + 0.02 * w).min(0.2) * (t * 1.2 + phase * 1.3).sin(),
        pitch: (0.035 + 0.012 * w).min(0.12) * (t * 1.5 + phase + 1.0).sin(),
        yaw: 0.05 * (t * 0.5 + phase).sin(),
    }
}

/// Water height under a boat marker at `marker_y`: boats on the sea ride the tide, boats in a
/// lagoon stay at the lagoon's own level.
pub fn boat_level(marker_y: f32, tide_level: f32) -> f32 {
    if (marker_y - SEA_LEVEL).abs() < 0.02 { tide_level } else { marker_y }
}

/// One canoe hull per trim colour, built by the core prop code (so afloat canoes match the
/// beached ones), centred on the origin with the waterline at y = 0 and the bow towards +x.
/// Empty when the terrain has no dry spot to build on.
pub fn canoe_meshes(terrain: &Terrain, seed: u64) -> Vec<MeshData> {
    let Some(at) = land_spot(terrain) else { return Vec::new() };
    (0..TRIM_COLORS)
        .map(|trim| {
            let placed = Placed {
                at,
                angle: 0.0,
                scale: 1.0,
                kind: PropKind::Clutter(ClutterKind::Canoe),
                colors: Colors { trim, ..Colors::default() },
            };
            let (mut m, _) = placed_mesh(&placed, terrain, seed);
            let keel = m.positions.iter().map(|p| p[1]).fold(f32::MAX, f32::min);
            for p in &mut m.positions {
                *p = [p[0] - at.x, p[1] - keel - 0.14, p[2] - at.y];
            }
            m
        })
        .collect()
}

fn land_spot(terrain: &Terrain) -> Option<Vec2> {
    (0..24).flat_map(|r| (0..8).map(move |k| (r, k))).find_map(|(r, k)| {
        let a = k as f32 / 8.0 * std::f32::consts::TAU;
        let p = Vec2::new(a.cos(), a.sin()) * (r as f32 * 1.5);
        (terrain.water_level(p).is_none() && terrain.height(p) > 0.2).then_some(p)
    })
}

// ---------------------------------------------------------------- creature meshes

/// Gull body, facing +z with the origin at the body centre (about 0.6 m long).
pub fn gull_body() -> MeshData {
    let (white, tip, beak) = (tok::gull(), tok::gull_tip(), palette::pigment(3));
    let mut m = MeshData::default();
    let mut bx = |c: Vec3, h: Vec3, col: Vec3| m.oriented_box(c, h, Vec3::X, col, Surface::Petal, 1.0);
    bx(Vec3::ZERO, Vec3::new(0.1, 0.085, 0.24), white);
    bx(Vec3::new(0.0, 0.03, 0.17), Vec3::new(0.085, 0.07, 0.1), white);
    bx(Vec3::new(0.0, 0.1, 0.28), Vec3::new(0.065, 0.065, 0.07), white);
    bx(Vec3::new(0.0, 0.085, 0.37), Vec3::new(0.02, 0.016, 0.06), beak);
    bx(Vec3::new(0.0, 0.0, -0.32), Vec3::new(0.07, 0.014, 0.1), white);
    bx(Vec3::new(0.0, 0.0, -0.4), Vec3::new(0.07, 0.015, 0.025), tip);
    for s in [-1.0f32, 1.0] {
        bx(Vec3::new(0.05 * s, 0.125, 0.3), Vec3::new(0.012, 0.012, 0.012), tip);
    }
    m
}

/// One gull wing hinged at the origin, reaching along +x (about 0.7 m); the tip is indigo.
pub fn gull_wing() -> MeshData {
    let mut m = MeshData::default();
    let grey = tok::gull().lerp(palette::srgb(0xDDE6F2), 0.5);
    m.oriented_box(Vec3::new(0.3, 0.0, 0.0), Vec3::new(0.3, 0.014, 0.13), Vec3::X, grey, Surface::Petal, 1.0);
    m.oriented_box(Vec3::new(0.7, 0.0, -0.03), Vec3::new(0.12, 0.013, 0.09), Vec3::X, tok::gull_tip(), Surface::Petal, 1.0);
    m
}

/// A coral-red crab facing +z, about 0.4 m wide with two raised claws.
pub fn crab_body() -> MeshData {
    let (red, claw, eye) = (tok::crab(), palette::srgb(0xFF7B5E), tok::gull_tip());
    let mut m = MeshData::default();
    let mut bx = |c: Vec3, h: Vec3, col: Vec3| m.oriented_box(c, h, Vec3::X, col, Surface::Petal, 1.0);
    bx(Vec3::new(0.0, 0.07, 0.0), Vec3::new(0.16, 0.05, 0.11), red);
    bx(Vec3::new(0.0, 0.12, -0.01), Vec3::new(0.12, 0.02, 0.08), claw);
    for s in [-1.0f32, 1.0] {
        bx(Vec3::new(0.2 * s, 0.09, 0.16), Vec3::new(0.05, 0.035, 0.05), claw); // claw
        bx(Vec3::new(0.14 * s, 0.08, 0.1), Vec3::new(0.025, 0.02, 0.05), red); // arm
        bx(Vec3::new(0.06 * s, 0.17, 0.09), Vec3::new(0.014, 0.04, 0.014), eye); // eye stalk
        for z in [-0.06f32, 0.0, 0.06] {
            bx(Vec3::new(0.26 * s, 0.035, z), Vec3::new(0.1, 0.012, 0.012), red); // leg
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn world() -> GullWorld {
        GullWorld {
            towers: vec![Vec3::new(5.0, 6.0, 5.0), Vec3::new(-8.0, 4.0, 2.0)],
            perches: vec![Vec3::new(2.0, 3.0, -4.0), Vec3::new(-3.0, 2.5, 6.0), Vec3::new(7.0, 5.0, 7.0)],
            sea: (-6..6).flat_map(|x| (-6..6).map(move |z| Vec2::new(x as f32 * 5.0, z as f32 * 5.0))).collect(),
        }
    }

    fn flock(n: usize, w: &GullWorld) -> Vec<Gull> {
        (0..n).map(|i| Gull::new(Rng(100 + i as u64), w, 0.0)).collect()
    }

    fn tick(gulls: &mut [Gull], w: &GullWorld) {
        for i in 0..gulls.len() {
            let held: HashSet<usize> = gulls.iter().enumerate().filter(|(j, _)| *j != i).flat_map(|(_, g)| (0..w.perches.len()).filter(|&k| g.holds(k))).collect();
            gulls[i].step(0.05, w, 0.0, Vec2::new(1.0, 0.3), &|k| held.contains(&k));
        }
    }

    #[test]
    fn gulls_stay_aloft_and_near_the_island() {
        let w = world();
        let mut gulls = flock(4, &w);
        for _ in 0..8000 {
            tick(&mut gulls, &w);
            for g in &gulls {
                assert!(g.pos.is_finite() && g.heading.is_finite() && g.bank.is_finite());
                assert!(g.pos.x.abs() < 60.0 && g.pos.z.abs() < 60.0 && g.pos.y > -0.1 && g.pos.y < 30.0, "{:?}", g.pos);
            }
        }
    }

    #[test]
    fn gull_lands_on_its_perch() {
        let w = world();
        let mut g = Gull::new(Rng(5), &w, 0.0);
        g.mode = GullMode::Land { perch: 1 };
        g.timer = 40.0;
        let mut perched = false;
        for _ in 0..4000 {
            g.step(0.05, &w, 0.0, Vec2::ZERO, &|_| false);
            if g.is_perched() {
                perched = true;
                break;
            }
        }
        assert!(perched && g.pos == w.perches[1] && g.holds(1));
        assert_eq!(g.wing_angle(1.0, 0.0), -0.9, "wings fold on a perch");
    }

    #[test]
    fn gulls_never_share_a_perch() {
        let w = GullWorld { perches: vec![Vec3::new(1.0, 3.0, 1.0)], ..world() };
        let mut gulls = flock(3, &w);
        let mut ever_perched = false;
        for _ in 0..12000 {
            tick(&mut gulls, &w);
            let on = gulls.iter().filter(|g| g.is_perched()).count();
            ever_perched |= on > 0;
            assert!(on <= 1, "two gulls on one perch");
        }
        assert!(ever_perched);
    }

    #[test]
    fn gulls_are_deterministic() {
        let w = world();
        let run = || {
            let mut g = flock(2, &w);
            for _ in 0..1500 {
                tick(&mut g, &w);
            }
            g.iter().map(|g| g.pos).collect::<Vec<_>>()
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn gulls_without_towers_or_perches_wander_the_sea() {
        let w = GullWorld { sea: vec![Vec2::new(10.0, 10.0)], ..Default::default() };
        let mut gulls = flock(2, &w);
        for _ in 0..2000 {
            tick(&mut gulls, &w);
        }
        assert!(gulls.iter().all(|g| g.pos.is_finite() && matches!(g.mode, GullMode::Roam { .. })));
    }

    /// A beach rising towards +x: sea to the west, x = -3.8 at tide 0 with a 0.12 band.
    fn beach() -> (impl Fn(Vec2) -> f32, impl Fn(Vec2) -> Vec2, impl Fn(Vec2) -> bool) {
        (|p: Vec2| 0.5 + p.x * 0.1, |_p: Vec2| Vec2::NEG_X, |p: Vec2| p.y.abs() < 12.0 && p.x > -20.0 && p.x < 20.0)
    }

    #[test]
    fn crabs_follow_the_waterline_and_scuttle_sideways() {
        let (height, seaward, ok) = beach();
        let shore = Shore { height: &height, seaward: &seaward, ok: &ok };
        let mut c = Crab::new(Rng(9), Vec2::new(2.0, 0.0));
        let mut tide = 0.0;
        let mut travelled = 0.0;
        let mut last = c.pos;
        for i in 0..6000 {
            // Tide rises from 0 to 0.3 over the run.
            tide = (i as f32 / 6000.0 * 0.3).min(0.3);
            c.step(0.05, tide, &shore);
            travelled += (c.pos.y - last.y).abs();
            assert!(c.pos.is_finite() && ok(c.pos));
            if i > 400 {
                let h = height(c.pos);
                assert!(h >= tide + 0.0 && h <= tide + 0.3, "off the waterline: h {h} tide {tide} at {}", c.pos);
            }
            last = c.pos;
        }
        assert!(tide > 0.29 && travelled > 3.0, "crab should scuttle along the shore, moved {travelled}");
        assert!((c.yaw - (-std::f32::consts::FRAC_PI_2)).abs() < 0.1, "faces the sea");
    }

    #[test]
    fn crabs_turn_back_at_obstacles() {
        let (height, seaward, _) = beach();
        let ok = |p: Vec2| p.y.abs() < 1.0;
        let shore = Shore { height: &height, seaward: &seaward, ok: &ok };
        let mut c = Crab::new(Rng(3), Vec2::new(-3.8, 0.0));
        for _ in 0..4000 {
            c.step(0.05, 0.0, &shore);
            assert!(c.pos.y.abs() < 1.0 + 1e-3);
        }
    }

    #[test]
    fn boats_ride_the_swell_within_bounds() {
        for (i, wind) in [0.0f32, 1.2, 9.0, 30.0].into_iter().enumerate() {
            for k in 0..400 {
                let p = boat_pose(k as f32 * 0.1, i as f32 * 1.7, wind);
                assert!(p.y.abs() <= 0.2 && p.roll.abs() <= 0.2 && p.pitch.abs() <= 0.12 && p.yaw.abs() <= 0.05 + 1e-6, "{p:?}");
            }
        }
        // Rougher in a storm than in a calm.
        let amp = |w: f32| (0..400).map(|k| boat_pose(k as f32 * 0.1, 0.0, w).roll.abs()).fold(0.0f32, f32::max);
        assert!(amp(9.0) > amp(1.0));
        assert_eq!(boat_pose(3.0, 1.0, 2.0), boat_pose(3.0, 1.0, 2.0));
    }

    #[test]
    fn sea_boats_ride_the_tide_lagoon_boats_do_not() {
        assert_eq!(boat_level(0.0, 0.3), 0.3);
        assert_eq!(boat_level(0.0, -0.3), -0.3);
        assert_eq!(boat_level(0.45, 0.3), 0.45);
    }

    #[test]
    fn creature_meshes_are_valid_and_finite() {
        for m in [gull_body(), gull_wing(), crab_body()] {
            assert!(!m.is_empty() && m.is_valid());
            assert!(m.positions.iter().flatten().all(|v| v.is_finite()));
        }
        let t = Terrain::build(7, &[], &[]);
        let canoes = canoe_meshes(&t, 7);
        assert_eq!(canoes.len(), TRIM_COLORS as usize);
        for m in &canoes {
            assert!(!m.is_empty() && m.is_valid());
            let (lo, hi) = m.bounds().unwrap();
            assert!(lo.is_finite() && hi.is_finite());
            assert!(lo.y < 0.0 && hi.y > 0.0, "waterline inside the hull: {lo} {hi}");
            assert!(lo.x.abs() < 4.0 && hi.x.abs() < 4.0 && lo.z.abs() < 4.0 && hi.z.abs() < 4.0, "centred: {lo} {hi}");
        }
        // Trim colours differ between variants.
        assert_ne!(canoes[0].colors, canoes[1].colors);
    }
}
