//! Pure document edits behind every tool: `(Document, gesture data, settings) -> Document`.
//! No Bevy state here, so each gesture's outcome is unit-tested.

use super::{HouseStyle, ToolSettings};
use glade_core::geom::{closed, closest_on_polyline, footprint_polygon, is_closed, polyline_length};
use glade_core::{
    Bridge, Colors, Document, Fence, Footprint, GLADE_HALF, House, Material, Opening, OpeningKind, Path, Placed,
    Platform, PropKind, RoofStyle, Stairs, Structure, StructureId, TerrainDab, Wall,
};
use bevy::math::{Vec2, Vec3, Vec3Swizzles};

/// New polyline point is added once the cursor moved this far (m).
pub const POINT_SPACING: f32 = 0.8;
/// A wall stroke ending this close to its start closes into a ring (m).
pub const CLOSE_DIST: f32 = 1.0;
/// Clicking this close to an existing opening edits it instead of adding one (m).
pub const OPENING_SNAP: f32 = 0.6;
/// Minimum stroke length for walls, fences and paths (m).
pub const MIN_STROKE: f32 = 1.5;
/// Brush dabs are laid this far apart along a stroke (m).
pub const DAB_SPACING: f32 = 0.6;
/// Bridge decks sit at least this far above ground / water (m).
pub const BRIDGE_CLEARANCE: f32 = 0.5;

pub const TOO_SHORT: &str = "Too short";
pub const OUTSIDE: &str = "Outside the glade";
pub const NEEDS_WALL: &str = "Needs a wall";

/// Clamps a ground point into the buildable glade (half a metre inside the edge).
pub fn clamp_to_glade(p: Vec2) -> Vec2 {
    p.clamp(Vec2::splat(-GLADE_HALF + 0.5), Vec2::splat(GLADE_HALF - 0.5))
}

pub fn inside_glade(p: Vec2) -> bool {
    p.x.abs() <= GLADE_HALF && p.y.abs() <= GLADE_HALF
}

/// Removes points that are (nearly) collinear with their neighbours or duplicated.
pub fn simplify(pts: &[Vec2]) -> Vec<Vec2> {
    let mut out: Vec<Vec2> = Vec::with_capacity(pts.len());
    for &p in pts {
        if out.last().is_some_and(|l: &Vec2| l.distance(p) < 0.05) {
            continue;
        }
        if out.len() >= 2 {
            let a = out[out.len() - 2];
            let b = out[out.len() - 1];
            let (d0, d1) = ((b - a).normalize_or_zero(), (p - b).normalize_or_zero());
            if d0.dot(d1) > 0.998 {
                out.pop();
            }
        }
        out.push(p);
    }
    out
}

/// Snaps `p` so the segment from `prev` runs at a multiple of 45°.
pub fn snap_45(prev: Vec2, p: Vec2) -> Vec2 {
    let d = p - prev;
    let len = d.length();
    if len < 1e-4 {
        return p;
    }
    let step = std::f32::consts::FRAC_PI_4;
    let a = (d.y.atan2(d.x) / step).round() * step;
    prev + Vec2::from_angle(a) * len
}

/// Simplified stroke; when `close` and the end is within [`CLOSE_DIST`] of the start (and the
/// ring has ≥ 3 corners) the last point is replaced by the first so the polyline is closed.
pub fn finish_polyline(pts: &[Vec2], close: bool) -> Vec<Vec2> {
    let mut v = simplify(pts);
    if close && v.len() >= 3 && v[0].distance(v[v.len() - 1]) <= CLOSE_DIST {
        let first = v[0];
        if v.len() >= 4 {
            *v.last_mut().unwrap() = first;
        } else {
            v.push(first);
        }
        v = simplify(&v);
    }
    v
}

pub fn stroke_error(pts: &[Vec2]) -> Option<&'static str> {
    (pts.len() < 2 || polyline_length(pts) < MIN_STROKE).then_some(TOO_SHORT)
}

pub fn wall_from(pts: Vec<Vec2>, s: &ToolSettings) -> Wall {
    Wall {
        points: pts,
        height: s.wall_height,
        thickness: 1.0,
        material: s.wall_material,
        battlements: s.wall_battlements,
        ruined: s.wall_ruined,
        foundation: s.wall_foundation,
        roofed: false,
        roof: s.roof_style,
        roof_pitch: s.roof_pitch,
        openings: Vec::new(),
        colors: s.colors,
    }
}

/// Adds the structure a line stroke makes for `tool`-kind strokes. `None` when too short.
pub fn apply_wall_stroke(doc: &Document, pts: &[Vec2], s: &ToolSettings) -> Option<Document> {
    let pts = finish_polyline(pts, true);
    if stroke_error(&pts).is_some() {
        return None;
    }
    let mut d = doc.clone();
    d.add(Structure::Wall(wall_from(pts, s)));
    Some(d)
}

pub fn apply_fence_stroke(doc: &Document, pts: &[Vec2], s: &ToolSettings) -> Option<Document> {
    let pts = finish_polyline(pts, false);
    if stroke_error(&pts).is_some() {
        return None;
    }
    let mut d = doc.clone();
    d.add(Structure::Fence(Fence { points: pts, kind: s.fence_kind, colors: s.colors }));
    Some(d)
}

pub fn apply_path_stroke(doc: &Document, pts: &[Vec2], s: &ToolSettings) -> Option<Document> {
    let pts = finish_polyline(pts, false);
    if stroke_error(&pts).is_some() {
        return None;
    }
    let mut d = doc.clone();
    d.add(Structure::Path(Path { points: pts, width: s.path_width, kind: s.path_kind }));
    Some(d)
}

/// Footprint dragged from `a` to `b`. Rectangles span the drag diagonal in axes rotated by
/// `angle`; circles are centred on `a`. `force` = square / keep circle. `None` = too small.
pub fn footprint_from_drag(a: Vec2, b: Vec2, angle: f32, round: bool, force: bool) -> Option<Footprint> {
    if round {
        let r = a.distance(b);
        return (r >= 1.2).then(|| Footprint::Round { center: a, radius: r.min(12.0) });
    }
    let rot = Vec2::from_angle(angle);
    let local = Vec2::new(rot.dot(b - a), rot.perp().dot(b - a));
    let mut size = local.abs().min(Vec2::splat(24.0));
    if force {
        size = Vec2::splat(size.max_element());
    }
    if size.min_element() < 2.0 {
        return None;
    }
    let local = local.signum() * size;
    let center = a + rot.rotate(local * 0.5);
    Some(Footprint::Rect { center, half: size * 0.5, angle })
}

pub fn house_from(fp: Footprint, st: &HouseStyle, colors: Colors) -> House {
    House {
        footprint: fp,
        wall_height: st.height.clamp(0.6, 24.0),
        roof: st.roof,
        roof_pitch: st.roof_pitch,
        material: st.material,
        half_timber: st.half_timber,
        foundation: st.foundation,
        battlements: st.battlements && st.roof == RoofStyle::Flat,
        openings: Vec::new(),
        colors,
    }
}

pub fn apply_house(doc: &Document, fp: Footprint, st: &HouseStyle, colors: Colors) -> Option<Document> {
    if !footprint_polygon(&fp).iter().all(|p| inside_glade(*p)) {
        return None;
    }
    let mut d = doc.clone();
    d.add(Structure::House(house_from(fp, st, colors)));
    Some(d)
}

pub fn next_roof(r: RoofStyle) -> RoofStyle {
    match r {
        RoofStyle::Gable => RoofStyle::Hip,
        RoofStyle::Hip => RoofStyle::Cone,
        RoofStyle::Cone => RoofStyle::Flat,
        RoofStyle::Flat => RoofStyle::Gable,
    }
}

/// Roof tool click: a closed wall ring toggles its roof (switching on applies the tool's style,
/// pitch and roof colour); a house takes the tool's style, pitch and roof colour. Anything else
/// (or no change) → `None`.
pub fn apply_roof_click(doc: &Document, id: StructureId, style: RoofStyle, pitch: f32, roof_color: u8) -> Option<Document> {
    let mut d = doc.clone();
    match d.get_mut(id)? {
        Structure::Wall(w) if is_closed(&w.points) => {
            w.roofed = !w.roofed;
            if w.roofed {
                w.roof = style;
                w.roof_pitch = pitch;
                w.colors.roof = roof_color;
            }
        }
        Structure::House(h) => {
            h.roof = style;
            h.roof_pitch = pitch;
            h.colors.roof = roof_color;
        }
        _ => return None,
    }
    (d != *doc).then_some(d)
}

/// Can the roof tool act on this structure?
pub fn roofable(s: &Structure) -> bool {
    match s {
        Structure::Wall(w) => is_closed(&w.points),
        Structure::House(_) => true,
        _ => false,
    }
}

/// Roof tool scroll: change pitch by `dp` (or cycle style when `cycle`) on a house or a roofed ring.
pub fn adjust_roof(doc: &Document, id: StructureId, dp: f32, cycle: bool) -> Option<Document> {
    let mut d = doc.clone();
    let (roof, pitch) = match d.get_mut(id)? {
        Structure::House(h) => (&mut h.roof, &mut h.roof_pitch),
        Structure::Wall(w) if w.roofed => (&mut w.roof, &mut w.roof_pitch),
        _ => return None,
    };
    if cycle {
        *roof = next_roof(*roof);
    } else {
        *pitch = (*pitch + dp).clamp(0.0, 2.5);
    }
    Some(d)
}

pub fn upgrade_opening(k: OpeningKind) -> OpeningKind {
    match k {
        OpeningKind::Window => OpeningKind::Door,
        OpeningKind::Door | OpeningKind::WideDoor => OpeningKind::WideDoor,
    }
}

/// Window/Door click at `along` on a wall or house. Clicking near an existing opening upgrades
/// it (Window → Door → WideDoor) or removes it with `remove`. Houses still on automatic
/// openings switch to manual ones: the clicked window plus a door half the perimeter away.
pub fn apply_opening_click(doc: &Document, id: StructureId, along: f32, remove: bool) -> Option<Document> {
    let mut d = doc.clone();
    let (openings, perimeter) = match d.get_mut(id)? {
        Structure::Wall(w) => {
            let len = polyline_length(&w.points);
            (&mut w.openings, len)
        }
        Structure::House(h) => {
            let len = polyline_length(&closed(&footprint_polygon(&h.footprint)));
            if h.openings.is_empty() {
                if remove {
                    return None;
                }
                h.openings.push(Opening { along, kind: OpeningKind::Window });
                h.openings.push(Opening { along: (along + len * 0.5) % len, kind: OpeningKind::Door });
                return Some(d);
            }
            (&mut h.openings, len)
        }
        _ => return None,
    };
    let along = along.clamp(0.0, perimeter);
    let near = openings
        .iter()
        .enumerate()
        .map(|(i, o)| (i, (o.along - along).abs()))
        .filter(|(_, dist)| *dist <= OPENING_SNAP)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i);
    match (near, remove) {
        (Some(i), true) => {
            openings.remove(i);
        }
        (Some(i), false) => {
            let k = upgrade_opening(openings[i].kind);
            if k == openings[i].kind {
                return None;
            }
            openings[i].kind = k;
        }
        (None, true) => return None,
        (None, false) => openings.push(Opening { along, kind: OpeningKind::Window }),
    }
    Some(d)
}

/// Can windows/doors go into this structure?
pub fn openable(s: &Structure) -> bool {
    matches!(s, Structure::Wall(_) | Structure::House(_))
}

/// Stairs between two picked points, always climbing from the lower one.
pub fn apply_stairs(doc: &Document, a: Vec3, b: Vec3, s: &ToolSettings) -> Option<Document> {
    if a.distance(b) < 1.0 || !inside_glade(a.xz()) || !inside_glade(b.xz()) {
        return None;
    }
    let (from, to) = if a.y <= b.y { (a, b) } else { (b, a) };
    let mut d = doc.clone();
    d.add(Structure::Stairs(Stairs { from, to, width: s.stair_width, kind: s.stair_kind, colors: s.colors }));
    Some(d)
}

/// Platform from a dragged rectangle (axes rotated by `angle`; `square` forces a square).
pub fn apply_platform(doc: &Document, a: Vec2, b: Vec2, angle: f32, square: bool, s: &ToolSettings) -> Option<Document> {
    let Footprint::Rect { center, half, angle } = footprint_from_drag(a, b, angle, false, square)? else {
        return None;
    };
    let mut d = doc.clone();
    d.add(Structure::Platform(Platform { center, half, angle, height: s.platform_height, colors: s.colors }));
    Some(d)
}

/// Deck height at a bridge end: the picked height, but at least [`BRIDGE_CLEARANCE`] above
/// the ground or water surface below it.
pub fn bridge_deck_height(hit_y: f32, ground_or_water: f32) -> f32 {
    hit_y.max(ground_or_water + BRIDGE_CLEARANCE)
}

/// `base_a` / `base_b`: ground (or water surface) height under each end.
pub fn apply_bridge(doc: &Document, a: Vec3, b: Vec3, base_a: f32, base_b: f32, s: &ToolSettings) -> Option<Document> {
    if a.xz().distance(b.xz()) < 2.0 || !inside_glade(a.xz()) || !inside_glade(b.xz()) {
        return None;
    }
    let a = Vec3::new(a.x, bridge_deck_height(a.y, base_a), a.z);
    let b = Vec3::new(b.x, bridge_deck_height(b.y, base_b), b.z);
    let mut d = doc.clone();
    d.add(Structure::Bridge(Bridge {
        a,
        b,
        width: s.bridge_width,
        kind: s.bridge_kind(),
        hanging: s.hanging,
        colors: s.colors,
    }));
    Some(d)
}

/// Dab centres from `last` (exclusive) towards `cur`, every `spacing` metres. Returns the
/// centres and the new `last`.
pub fn dabs_between(last: Vec2, cur: Vec2, spacing: f32) -> (Vec<Vec2>, Vec2) {
    let dist = last.distance(cur);
    let n = (dist / spacing).floor() as usize;
    if n == 0 {
        return (Vec::new(), last);
    }
    let dir = (cur - last) / dist;
    let pts: Vec<Vec2> = (1..=n).map(|i| last + dir * spacing * i as f32).collect();
    let end = *pts.last().unwrap();
    (pts, end)
}

pub fn apply_dabs(doc: &Document, dabs: &[TerrainDab]) -> Option<Document> {
    if dabs.is_empty() {
        return None;
    }
    let mut d = doc.clone();
    d.terrain.extend_from_slice(dabs);
    Some(d)
}

pub fn apply_placed(doc: &Document, at: Vec2, angle: f32, scale: f32, kind: PropKind, colors: Colors) -> Option<Document> {
    if !inside_glade(at) {
        return None;
    }
    let mut d = doc.clone();
    d.add(Structure::Placed(Placed { at, angle, scale, kind, colors }));
    Some(d)
}

/// A structure's style: colours, material (walls/houses) and half-timbering (houses).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    pub colors: Colors,
    pub material: Option<Material>,
    pub half_timber: Option<bool>,
}

pub fn style_of(s: &Structure) -> Option<Style> {
    let (colors, material, half_timber) = match s {
        Structure::Wall(w) => (w.colors, Some(w.material), None),
        Structure::House(h) => (h.colors, Some(h.material), Some(h.half_timber)),
        Structure::Fence(f) => (f.colors, None, None),
        Structure::Stairs(st) => (st.colors, None, None),
        Structure::Platform(p) => (p.colors, None, None),
        Structure::Bridge(b) => (b.colors, None, None),
        Structure::Placed(p) => (p.colors, None, None),
        Structure::Path(_) => return None,
    };
    Some(Style { colors, material, half_timber })
}

/// Style picker: copies a structure's style into the settings for the next placements
/// (walls, houses and towers; walls have no plaster).
pub fn copy_style(settings: &mut ToolSettings, style: Style) {
    settings.colors = style.colors;
    if let Some(m) = style.material {
        settings.house.material = m;
        settings.tower.material = m;
        settings.wall_material = if m == Material::Plaster { Material::Stone } else { m };
    }
    if let Some(t) = style.half_timber {
        settings.house.half_timber = t;
        settings.tower.half_timber = t;
    }
}

/// Recolours structure `id` with `style` (material only where the structure has one; walls
/// treat plaster as stone).
pub fn apply_style(doc: &Document, id: StructureId, style: Style) -> Option<Document> {
    let mut d = doc.clone();
    match d.get_mut(id)? {
        Structure::Wall(w) => {
            w.colors = style.colors;
            if let Some(m) = style.material {
                w.material = if m == Material::Plaster { Material::Stone } else { m };
            }
        }
        Structure::House(h) => {
            h.colors = style.colors;
            if let Some(m) = style.material {
                h.material = m;
            }
            if let Some(t) = style.half_timber {
                h.half_timber = t;
            }
        }
        Structure::Fence(f) => f.colors = style.colors,
        Structure::Stairs(st) => st.colors = style.colors,
        Structure::Platform(p) => p.colors = style.colors,
        Structure::Bridge(b) => b.colors = style.colors,
        Structure::Placed(p) => p.colors = style.colors,
        Structure::Path(_) => return None,
    }
    (d != *doc).then_some(d)
}

/// Removes every structure in `ids`.
pub fn erase(doc: &Document, ids: &[StructureId]) -> Option<Document> {
    let mut d = doc.clone();
    d.structures.retain(|(id, _)| !ids.contains(id));
    (d.structures.len() != doc.structures.len()).then_some(d)
}

pub fn structure_name(s: &Structure) -> &'static str {
    match s {
        Structure::Wall(w) if is_closed(&w.points) => "Wall ring",
        Structure::Wall(_) => "Wall",
        Structure::House(House { footprint: Footprint::Round { .. }, .. }) => "Tower",
        Structure::House(_) => "House",
        Structure::Fence(_) => "Fence",
        Structure::Path(_) => "Path",
        Structure::Stairs(_) => "Stairs",
        Structure::Platform(_) => "Platform",
        Structure::Bridge(_) => "Bridge",
        Structure::Placed(Placed { kind: PropKind::Tree(_), .. }) => "Tree",
        Structure::Placed(_) => "Clutter",
    }
}

/// Distance from `p` to a structure's outline on the ground (for nearest-wall snapping).
pub fn wall_top_near(doc: &Document, p: Vec2, max_dist: f32) -> Option<(StructureId, f32)> {
    doc.structures
        .iter()
        .filter_map(|(id, s)| match s {
            Structure::Wall(w) => Some((*id, closest_on_polyline(&w.points, p).1, w.height)),
            Structure::House(h) => {
                Some((*id, closest_on_polyline(&closed(&footprint_polygon(&h.footprint)), p).1, h.wall_height))
            }
            _ => None,
        })
        .filter(|(_, d, _)| *d <= max_dist)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _, h)| (id, h))
}

#[cfg(test)]
mod tests {
    use super::*;
    use glade_core::{Hanging, OpeningKind as K, PaintLayer, TerrainOp};

    fn v(x: f32, z: f32) -> Vec2 {
        Vec2::new(x, z)
    }

    fn settings() -> ToolSettings {
        ToolSettings::default()
    }

    #[test]
    fn simplify_drops_collinear_points() {
        let pts = [v(0., 0.), v(1., 0.), v(2., 0.), v(3., 0.), v(3., 1.), v(3., 2.)];
        assert_eq!(simplify(&pts), vec![v(0., 0.), v(3., 0.), v(3., 2.)]);
    }

    #[test]
    fn wall_stroke_takes_settings() {
        let mut s = settings();
        s.wall_height = 7.0;
        s.wall_battlements = false;
        s.colors.roof = 2;
        let doc = Document::new(1);
        let pts = [v(0., 0.), v(1., 0.), v(2., 0.), v(4., 0.), v(4., 3.)];
        let d = apply_wall_stroke(&doc, &pts, &s).expect("wall");
        let Some(Structure::Wall(w)) = d.get(1) else { panic!("no wall") };
        assert_eq!(w.points, vec![v(0., 0.), v(4., 0.), v(4., 3.)]);
        assert_eq!(w.height, 7.0);
        assert!(!w.battlements && !w.roofed);
        assert_eq!(w.colors.roof, 2);
        assert_eq!(d.next_id, 2);
    }

    #[test]
    fn wall_stroke_near_start_closes() {
        let doc = Document::new(1);
        let pts = [v(0., 0.), v(5., 0.), v(5., 5.), v(0., 5.), v(0.3, 0.6)];
        let d = apply_wall_stroke(&doc, &pts, &settings()).unwrap();
        let Some(Structure::Wall(w)) = d.get(1) else { panic!() };
        assert!(is_closed(&w.points), "{:?}", w.points);
        assert_eq!(w.points.first(), w.points.last());
        assert_eq!(w.points.len(), 5);
        // Fences never close.
        let d = apply_fence_stroke(&doc, &pts, &settings()).unwrap();
        let Some(Structure::Fence(f)) = d.get(1) else { panic!() };
        assert!(!is_closed(&f.points));
    }

    #[test]
    fn short_strokes_are_rejected() {
        let doc = Document::new(1);
        assert!(apply_path_stroke(&doc, &[v(0., 0.), v(0.5, 0.)], &settings()).is_none());
        assert_eq!(stroke_error(&[v(0., 0.)]), Some(TOO_SHORT));
        let d = apply_path_stroke(&doc, &[v(0., 0.), v(3., 0.)], &settings()).unwrap();
        assert!(matches!(d.get(1), Some(Structure::Path(p)) if p.width == settings().path_width));
    }

    #[test]
    fn snap_45_rounds_direction() {
        let p = snap_45(v(0., 0.), v(10., 1.));
        assert!((p - v(10.05, 0.)).length() < 0.01);
        let p = snap_45(v(0., 0.), v(3., 2.6));
        assert!((p.x - p.y).abs() < 1e-4);
    }

    #[test]
    fn footprints_from_drags() {
        let Some(Footprint::Rect { center, half, .. }) = footprint_from_drag(v(0., 0.), v(4., -6.), 0.0, false, false)
        else {
            panic!()
        };
        assert!((center - v(2., -3.)).length() < 1e-4 && (half - v(2., 3.)).length() < 1e-4);
        let Some(Footprint::Rect { half, .. }) = footprint_from_drag(v(0., 0.), v(4., 6.), 0.0, false, true) else {
            panic!()
        };
        assert_eq!(half, v(3., 3.));
        assert!(footprint_from_drag(v(0., 0.), v(1., 6.), 0.0, false, false).is_none());
        assert!(matches!(
            footprint_from_drag(v(1., 1.), v(4., 5.), 0.0, true, false),
            Some(Footprint::Round { radius, .. }) if (radius - 5.0).abs() < 1e-4
        ));
        // Rotated: corner a stays a corner.
        let fp = footprint_from_drag(v(0., 0.), v(0., 6.), 0.7, false, false).unwrap();
        let poly = footprint_polygon(&fp);
        assert!(poly.iter().any(|p| p.length() < 1e-3));
    }

    #[test]
    fn house_outside_glade_is_invalid() {
        let doc = Document::new(1);
        let fp = Footprint::Round { center: v(31., 0.), radius: 3.0 };
        assert!(apply_house(&doc, fp, &settings().tower, Colors::default()).is_none());
        let fp = Footprint::Round { center: v(0., 0.), radius: 3.0 };
        let d = apply_house(&doc, fp, &settings().tower, Colors::default()).unwrap();
        let Some(Structure::House(h)) = d.get(1) else { panic!() };
        assert_eq!(h.roof, RoofStyle::Cone);
        assert!(h.wall_height >= 6.0);
    }

    fn doc_with_wall(closed_ring: bool) -> Document {
        let mut doc = Document::new(1);
        let mut pts = vec![v(0., 0.), v(6., 0.), v(6., 6.), v(0., 6.)];
        if closed_ring {
            pts.push(v(0., 0.));
        }
        doc.add(Structure::Wall(wall_from(pts, &settings())));
        doc
    }

    #[test]
    fn opening_upgrade_cycle_and_remove() {
        let doc = doc_with_wall(false);
        let d1 = apply_opening_click(&doc, 1, 3.0, false).unwrap();
        let ks = |d: &Document| match d.get(1) {
            Some(Structure::Wall(w)) => w.openings.iter().map(|o| o.kind).collect::<Vec<_>>(),
            _ => panic!(),
        };
        assert_eq!(ks(&d1), vec![K::Window]);
        let d2 = apply_opening_click(&d1, 1, 3.4, false).unwrap();
        assert_eq!(ks(&d2), vec![K::Door]);
        let d3 = apply_opening_click(&d2, 1, 2.7, false).unwrap();
        assert_eq!(ks(&d3), vec![K::WideDoor]);
        assert!(apply_opening_click(&d3, 1, 3.0, false).is_none(), "wide door is the last step");
        let d4 = apply_opening_click(&d3, 1, 9.0, false).unwrap();
        assert_eq!(ks(&d4), vec![K::WideDoor, K::Window]);
        let d5 = apply_opening_click(&d4, 1, 3.1, true).unwrap();
        assert_eq!(ks(&d5), vec![K::Window]);
        assert!(apply_opening_click(&d5, 1, 15.0, true).is_none());
    }

    #[test]
    fn first_opening_on_auto_house_goes_manual() {
        let mut doc = Document::new(1);
        let fp = Footprint::Rect { center: v(0., 0.), half: v(3., 2.), angle: 0.0 };
        doc.add(Structure::House(house_from(fp, &settings().house, Colors::default())));
        let d = apply_opening_click(&doc, 1, 2.0, false).unwrap();
        let Some(Structure::House(h)) = d.get(1) else { panic!() };
        assert_eq!(h.openings.len(), 2);
        assert_eq!(h.openings[0], Opening { along: 2.0, kind: K::Window });
        assert_eq!(h.openings[1], Opening { along: 12.0, kind: K::Door });
    }

    #[test]
    fn roof_tool_needs_closed_ring() {
        assert!(apply_roof_click(&doc_with_wall(false), 1, RoofStyle::Hip, 0.5, 0).is_none());
        let d = apply_roof_click(&doc_with_wall(true), 1, RoofStyle::Hip, 0.5, 3).unwrap();
        let Some(Structure::Wall(w)) = d.get(1) else { panic!() };
        assert!(w.roofed && w.roof == RoofStyle::Hip && w.colors.roof == 3);
        let d = apply_roof_click(&d, 1, RoofStyle::Hip, 0.5, 3).unwrap();
        assert!(matches!(d.get(1), Some(Structure::Wall(w)) if !w.roofed));
        let d2 = adjust_roof(&apply_roof_click(&d, 1, RoofStyle::Gable, 0.5, 0).unwrap(), 1, 0.25, false).unwrap();
        assert!(matches!(d2.get(1), Some(Structure::Wall(w)) if (w.roof_pitch - 0.75).abs() < 1e-5));
    }

    #[test]
    fn roof_click_applies_strip_roof_to_house() {
        let mut doc = Document::new(1);
        doc.add(Structure::House(house_from(
            Footprint::Round { center: Vec2::ZERO, radius: 2.0 },
            &settings().house,
            Colors::default(),
        )));
        let d = apply_roof_click(&doc, 1, RoofStyle::Flat, 0.3, 4).unwrap();
        let Some(Structure::House(h)) = d.get(1) else { panic!() };
        assert_eq!((h.roof, h.roof_pitch, h.colors.roof), (RoofStyle::Flat, 0.3, 4));
        assert!(apply_roof_click(&d, 1, RoofStyle::Flat, 0.3, 4).is_none(), "no change, no edit");
    }

    #[test]
    fn platform_shift_forces_square() {
        let doc = Document::new(1);
        let d = apply_platform(&doc, v(0., 0.), v(3., 5.), 0.0, true, &settings()).unwrap();
        let Some(Structure::Platform(p)) = d.get(1) else { panic!() };
        assert_eq!(p.half, v(2.5, 2.5));
        let d = apply_platform(&doc, v(0., 0.), v(3., 5.), 0.0, false, &settings()).unwrap();
        assert!(matches!(d.get(1), Some(Structure::Platform(p)) if p.half == v(1.5, 2.5)));
    }

    #[test]
    fn style_picker_copies_into_house_tower_and_wall() {
        let mut s = settings();
        let st = Style { colors: Colors { roof: 5, wall: 4, trim: 3 }, material: Some(Material::Plaster), half_timber: Some(false) };
        copy_style(&mut s, st);
        assert_eq!(s.colors, st.colors);
        assert_eq!((s.house.material, s.tower.material, s.wall_material), (Material::Plaster, Material::Plaster, Material::Stone));
        assert!(!s.house.half_timber && !s.tower.half_timber);
    }

    #[test]
    fn bridge_decks_clear_ground_and_water() {
        assert_eq!(bridge_deck_height(0.2, 0.0), BRIDGE_CLEARANCE);
        assert_eq!(bridge_deck_height(4.0, 0.0), 4.0);
        let doc = Document::new(1);
        let mut s = settings();
        s.bridge_kind = 1;
        s.hanging = Hanging::Flags;
        let d = apply_bridge(&doc, Vec3::new(0., 0.1, 0.), Vec3::new(6., 3.0, 0.), -0.5, 1.0, &s).unwrap();
        let Some(Structure::Bridge(b)) = d.get(1) else { panic!() };
        assert_eq!(b.a.y, 0.1f32.max(-0.5 + BRIDGE_CLEARANCE));
        assert_eq!(b.b.y, 3.0);
        assert_eq!(b.kind, glade_core::BridgeKind::Stone);
        assert!(apply_bridge(&doc, Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), 0.0, 0.0, &s).is_none());
    }

    #[test]
    fn stairs_climb_from_low_end() {
        let d = apply_stairs(&Document::new(1), Vec3::new(0., 4., 0.), Vec3::new(3., 0., 0.), &settings()).unwrap();
        let Some(Structure::Stairs(st)) = d.get(1) else { panic!() };
        assert!(st.from.y < st.to.y);
    }

    #[test]
    fn dabs_are_evenly_spaced() {
        let (pts, last) = dabs_between(v(0., 0.), v(2.0, 0.), 0.6);
        assert_eq!(pts.len(), 3);
        assert!((last - v(1.8, 0.)).length() < 1e-5);
        let (pts, last2) = dabs_between(last, v(2.0, 0.), 0.6);
        assert!(pts.is_empty() && last2 == last);
        let dab = TerrainDab { center: v(0., 0.), radius: 2.0, op: TerrainOp::Paint { layer: PaintLayer::Flowers, strength: 1.0 } };
        let d = apply_dabs(&Document::new(1), &[dab, dab]).unwrap();
        assert_eq!(d.terrain.len(), 2);
    }

    #[test]
    fn eraser_removes_touched_structures() {
        let mut doc = doc_with_wall(false);
        let t = apply_placed(&doc, v(1., 1.), 0.0, 1.0, PropKind::Tree(glade_core::TreeKind::Oak), Colors::default());
        doc = t.unwrap();
        let d = erase(&doc, &[1]).unwrap();
        assert_eq!(d.structures.len(), 1);
        assert!(d.get(2).is_some());
        assert!(erase(&d, &[1]).is_none());
        assert_eq!(erase(&doc, &[1, 2]).unwrap().structures.len(), 0);
    }

    #[test]
    fn style_round_trip() {
        let mut doc = doc_with_wall(false);
        let fp = Footprint::Rect { center: v(10., 0.), half: v(3., 2.), angle: 0.0 };
        doc.add(Structure::House(house_from(fp, &settings().house, Colors { roof: 3, wall: 2, trim: 1 })));
        let st = style_of(doc.get(2).unwrap()).unwrap();
        assert_eq!(st.material, Some(Material::Plaster));
        let d = apply_style(&doc, 1, st).unwrap();
        let Some(Structure::Wall(w)) = d.get(1) else { panic!() };
        assert_eq!(w.colors, Colors { roof: 3, wall: 2, trim: 1 });
        assert_eq!(w.material, Material::Stone, "walls have no plaster");
        assert!(apply_style(&d, 1, st).is_none(), "no-op is not an edit");
    }
}
