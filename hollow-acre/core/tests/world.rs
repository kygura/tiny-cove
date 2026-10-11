use acre_core::*;

fn count(g: &VoxelGrid, v: Voxel) -> usize {
    let mut n = 0;
    for y in 0..SIZE_Y as i32 {
        for z in 0..SIZE_Z as i32 {
            for x in 0..SIZE_X as i32 {
                n += (g.get(x, y, z) == v) as usize;
            }
        }
    }
    n
}

fn doc(structures: Vec<Structure>) -> Document {
    Document { seed: 42, structures }
}

fn base() -> VoxelGrid {
    generate(&doc(vec![]))
}

/// Count of voxels of kind `v` that the structure added versus bare terrain.
fn added(s: Structure, v: Voxel) -> usize {
    count(&generate(&doc(vec![s])), v).saturating_sub(count(&base(), v))
}

fn wall() -> Structure {
    Structure::Wall { path: vec![[-8.0, 0.0], [8.0, 0.0]], tall: false }
}
fn block() -> Structure {
    Structure::Block { min: [-6.0, -6.0], max: [6.0, 6.0], floors: 5 }
}

#[test]
fn generation_is_deterministic() {
    let d = doc(vec![wall(), block(), Structure::Spire { at: [10.0, 10.0], kind: 1 }]);
    assert!(generate(&d) == generate(&d));
    assert!(generate(&d) != generate(&Document { seed: 43, ..d.clone() }));
}

#[test]
fn terrain_has_expected_features() {
    for seed in [1, 42, 9999] {
        let g = generate(&Document::new(seed));
        for v in [Voxel::Grass, Voxel::Soil, Voxel::Rock, Voxel::Water, Voxel::Sand, Voxel::Asphalt, Voxel::Rust, Voxel::Crystal, Voxel::Wood, Voxel::Leaves, Voxel::Meadow] {
            assert!(count(&g, v) > 0, "seed {seed} missing {v:?}");
        }
        // Surface 6..~22 voxels everywhere; centre is calm.
        let (mut lo, mut hi) = (i32::MAX, 0);
        for z in (0..SIZE_Z as i32).step_by(7) {
            for x in (0..SIZE_X as i32).step_by(7) {
                let t = g.top(x, z);
                assert!(t >= 3, "seed {seed} hole at {x},{z}");
                lo = lo.min(t);
                hi = hi.max(t);
            }
        }
        assert!(hi - lo >= 4, "terrain too flat: {lo}..{hi}");
        let mut cl = (i32::MAX, 0);
        for z in 76..116 {
            for x in 76..116 {
                let h = ground_height(&g, (x as f32 - 96.0) * 0.5, (z as f32 - 96.0) * 0.5);
                let h = (h / VOXEL_SIZE) as i32;
                cl = (cl.0.min(h), cl.1.max(h));
            }
        }
        assert!(cl.1 - cl.0 <= 5, "centre not calm: {cl:?}");
    }
}

#[test]
fn wall_adds_expected_kinds() {
    for v in [Voxel::Concrete, Voxel::ConcreteDark, Voxel::Glass, Voxel::Lamp, Voxel::Ivy] {
        assert!(added(wall(), v) > 0, "wall missing {v:?}");
    }
    let tall = Structure::Wall { path: vec![[-8.0, 0.0], [8.0, 0.0]], tall: true };
    assert!(added(tall.clone(), Voxel::Concrete) > added(wall(), Voxel::Concrete));
    // Tall wall reaches higher than a normal one.
    let top = |s| generate(&doc(vec![s])).top(96, 96);
    assert!(top(tall) > top(wall()) + 5);
}

#[test]
fn block_adds_expected_kinds() {
    let g = generate(&doc(vec![block()]));
    for v in [Voxel::Concrete, Voxel::ConcreteDark, Voxel::Glass, Voxel::WindowLit, Voxel::Solar] {
        assert!(count(&g, v) > count(&base(), v), "block missing {v:?}");
    }
    // Over several footprints we see balconies' planters, tanks and roof gardens.
    let (mut leaves, mut rust, mut grass) = (0, 0, 0);
    for i in 0..12 {
        let o = i as f32 * 3.7;
        let s = Structure::Block { min: [-7.0 + o * 0.1, -9.0], max: [8.0 + o * 0.2, 7.0 + o * 0.05], floors: 6 };
        let g = generate(&doc(vec![s]));
        leaves += count(&g, Voxel::Blossom);
        rust += count(&g, Voxel::Steel);
        grass += count(&g, Voxel::Grass);
    }
    assert!(leaves > 0 && rust > 0 && grass > 0);
    // Taller block is taller; pilotis leave ground floor open.
    let h = |f| generate(&doc(vec![Structure::Block { min: [-6.0, -6.0], max: [6.0, 6.0], floors: f }])).top(96, 96);
    assert!(h(6) > h(2));
    let g = generate(&doc(vec![Structure::Block { min: [-6.0, -6.0], max: [6.0, 6.0], floors: 3 }]));
    let bb = base();
    let mut b0 = 0;
    for z in 84..108 {
        for x in 84..108 {
            b0 = b0.max(bb.top(x, z) + 1);
        }
    }
    let air = (86..106).flat_map(|z| (86..106).map(move |x| (x, z))).filter(|&(x, z)| g.get(x, b0 + 3, z) == Voxel::Air).count();
    assert!(air > 300, "pilotis level should be mostly open: {air}/400");
}

#[test]
fn block_clamps_floors_to_world_height() {
    let g = generate(&doc(vec![Structure::Block { min: [-6.0, -6.0], max: [6.0, 6.0], floors: 255 }]));
    assert!(g.top(96, 96) < SIZE_Y as i32);
}

#[test]
fn path_lays_gravel_and_carves_through_wall() {
    let w = Structure::Wall { path: vec![[0.0, -10.0], [0.0, 10.0]], tall: false };
    let p = Structure::Path { path: vec![[-10.0, 0.0], [10.0, 0.0]] };
    let before = generate(&doc(vec![w.clone()]));
    let after = generate(&doc(vec![w, p.clone()]));
    assert!(count(&after, Voxel::Gravel) > 20);
    let ground = (ground_height(&base(), 0.0, 0.0) / VOXEL_SIZE) as i32;
    // Wall blocks the way at the crossing point before the path; after, an arch is open.
    assert!(before.get(96, ground + 2, 96).is_solid());
    for y in ground..ground + 6 {
        assert_eq!(after.get(96, y, 96), Voxel::Air, "y={y}");
    }
    assert!(after.get(96, ground + 9, 96).is_solid(), "arch top should stay");
    assert!(after.get(96, ground - 1, 96) == Voxel::Gravel);
    // Block crossing gets a passage too.
    let b = generate(&doc(vec![block(), p]));
    assert_eq!(b.get(96, ground + 4, 96), Voxel::Air);
    assert_eq!(b.get(96, ground + 4, 97), Voxel::Air);
}

#[test]
fn field_has_rows_kerb_and_water() {
    let f = Structure::Field { min: [-8.0, -4.0], max: [8.0, 4.0] };
    for v in [Voxel::Crop, Voxel::ConcreteDark, Voxel::Soil] {
        assert!(added(f.clone(), v) > 0, "field missing {v:?}");
    }
    let g = generate(&doc(vec![f]));
    assert!(count(&g, Voxel::Water) > count(&base(), Voxel::Water));
}

#[test]
fn spires_add_expected_kinds() {
    let at = [10.0, -10.0];
    assert!(added(Structure::Spire { at, kind: 0 }, Voxel::Crystal) >= 10);
    let m = Structure::Spire { at, kind: 1 };
    assert!(added(m.clone(), Voxel::Monolith) > 10 && added(m.clone(), Voxel::Rune) > 5 && added(m, Voxel::Concrete) > 20);
    let t = Structure::Spire { at, kind: 2 };
    assert!(added(t.clone(), Voxel::Blossom) > 10 && added(t, Voxel::Wood) > 8);
}

#[test]
fn structures_clear_vegetation_without_floating_leftovers() {
    let g = generate(&doc(vec![block(), Structure::Spire { at: [-12.0, 12.0], kind: 2 }]));
    // No leaf/wood voxel hovers: each has support within its connected blob down to ground.
    for z in 0..SIZE_Z as i32 {
        for x in 0..SIZE_X as i32 {
            for y in 1..SIZE_Y as i32 {
                if g.get(x, y, z) == Voxel::Wood {
                    assert!(g.get(x, y - 1, z) != Voxel::Air, "floating trunk at {x},{y},{z}");
                }
            }
        }
    }
}

#[test]
fn decoration_ignores_structure_order() {
    let a = wall();
    let b = Structure::Spire { at: [20.0, 20.0], kind: 0 };
    assert!(generate(&doc(vec![a.clone(), b.clone()])) == generate(&doc(vec![b, a])));
}

#[test]
fn diff_chunks_between_documents() {
    let a = generate(&doc(vec![]));
    let b = generate(&doc(vec![wall()]));
    let d = a.diff_chunks(&b);
    assert!(!d.is_empty() && d.len() < 40);
    assert!(a.diff_chunks(&a).is_empty());
    // Every chunk containing a changed voxel is reported.
    for z in 0..SIZE_Z as i32 {
        for x in 0..SIZE_X as i32 {
            for y in 0..SIZE_Y as i32 {
                if a.get(x, y, z) != b.get(x, y, z) {
                    assert!(d.contains(&[x as usize / 16, y as usize / 16, z as usize / 16]));
                }
            }
        }
    }
}

#[test]
fn degenerate_inputs_do_not_panic() {
    let nan = f32::NAN;
    let inf = f32::INFINITY;
    let s = vec![
        Structure::Wall { path: vec![], tall: false },
        Structure::Wall { path: vec![[0.0, 0.0]], tall: true },
        Structure::Wall { path: vec![[1.0, 1.0], [1.0, 1.0], [1.0, 1.0]], tall: false },
        Structure::Wall { path: vec![[nan, 0.0], [inf, 3.0], [0.0, -inf]], tall: true },
        Structure::Wall { path: vec![[-200.0, -200.0], [300.0, 300.0]], tall: true },
        Structure::Wall { path: vec![[1e30, 0.0], [-1e30, 5.0]], tall: false },
        Structure::Wall { path: vec![[47.0, 47.0], [60.0, 60.0]], tall: false },
        Structure::Path { path: vec![] },
        Structure::Path { path: vec![[0.0, 0.0], [0.1, 0.0]] },
        Structure::Path { path: vec![[nan, nan], [1.0, 1.0]] },
        Structure::Block { min: [0.0, 0.0], max: [0.0, 0.0], floors: 3 },
        Structure::Block { min: [0.0, 0.0], max: [1.0, 1.0], floors: 3 },
        Structure::Block { min: [0.0, 0.0], max: [8.0, 8.0], floors: 0 },
        Structure::Block { min: [8.0, 8.0], max: [-8.0, -8.0], floors: 3 },
        Structure::Block { min: [-1e9, -1e9], max: [1e9, 1e9], floors: 12 },
        Structure::Block { min: [nan, 0.0], max: [3.0, 3.0], floors: 2 },
        Structure::Block { min: [40.0, 40.0], max: [60.0, 60.0], floors: 4 },
        Structure::Block { min: [-70.0, -70.0], max: [-45.0, -45.0], floors: 4 },
        Structure::Field { min: [0.0, 0.0], max: [0.2, 9.0] },
        Structure::Field { min: [-1e9, 0.0], max: [1e9, 3.0] },
        Structure::Field { min: [nan, nan], max: [nan, nan] },
        Structure::Field { min: [46.0, 46.0], max: [80.0, 80.0] },
        Structure::Spire { at: [nan, 0.0], kind: 0 },
        Structure::Spire { at: [1000.0, -1000.0], kind: 1 },
        Structure::Spire { at: [-48.0, -48.0], kind: 2 },
        Structure::Spire { at: [47.9, 47.9], kind: 1 },
        Structure::Spire { at: [0.0, 0.0], kind: 255 },
    ];
    for st in &s {
        let _ = generate(&doc(vec![st.clone()]));
    }
    let _ = generate(&doc(s));
    let g = base();
    let _ = ground_height(&g, 1e9, -1e9);
    let _ = ground_height(&g, f32::NAN, 0.0);
    let _ = mesh_chunk(&g, [100, 100, 100]);
}

#[test]
fn tiny_structures_yield_nothing() {
    let b = base();
    for s in [
        Structure::Wall { path: vec![[0.0, 0.0], [0.2, 0.0]], tall: false },
        Structure::Block { min: [0.0, 0.0], max: [1.0, 1.0], floors: 3 },
        Structure::Field { min: [0.0, 0.0], max: [1.0, 1.0] },
        Structure::Path { path: vec![[0.0, 0.0], [0.1, 0.0]] },
    ] {
        assert!(generate(&doc(vec![s])) == b);
    }
}

#[test]
fn ground_height_tracks_terrain_and_structures() {
    let g = base();
    let h = ground_height(&g, 0.0, 0.0);
    assert!((4.0..14.0).contains(&h), "{h}");
    let gb = generate(&doc(vec![block()]));
    assert!(ground_height(&gb, 0.0, 0.0) > h + 10.0);
}

#[test]
fn full_world_meshes() {
    let g = generate(&doc(vec![wall(), block(), Structure::Field { min: [10.0, 10.0], max: [20.0, 18.0] }, Structure::Spire { at: [-14.0, 3.0], kind: 1 }]));
    let (mut verts, mut tris) = (0, 0);
    for cy in 0..CHUNKS_Y {
        for cz in 0..CHUNKS_Z {
            for cx in 0..CHUNKS_X {
                let m = mesh_chunk(&g, [cx, cy, cz]);
                assert_eq!(m.positions.len(), m.normals.len());
                assert_eq!(m.positions.len(), m.voxels.len());
                assert_eq!(m.positions.len(), m.ao.len());
                assert!(m.indices.iter().all(|&i| (i as usize) < m.positions.len()));
                verts += m.positions.len();
                tris += m.indices.len() / 3;
            }
        }
    }
    eprintln!("verts {verts} tris {tris}");
    assert!(tris > 10_000);
}

#[test]
fn generate_perf_sanity() {
    let mut s = vec![];
    for i in 0..20 {
        let a = (i as f32) * 1.3;
        let (x, z) = (a.cos() * 18.0, a.sin() * 18.0);
        s.push(match i % 5 {
            0 => Structure::Wall { path: vec![[x, z], [x + 10.0, z + 3.0], [x + 14.0, z + 12.0]], tall: i % 2 == 0 },
            1 => Structure::Block { min: [x, z], max: [x + 8.0, z + 7.0], floors: 4 },
            2 => Structure::Path { path: vec![[x - 15.0, z], [x, z + 4.0], [x + 15.0, z]] },
            3 => Structure::Field { min: [x, z], max: [x + 9.0, z + 6.0] },
            _ => Structure::Spire { at: [x, z], kind: (i % 3) as u8 },
        });
    }
    let d = doc(s);
    generate(&d); // warm terrain cache
    let t = std::time::Instant::now();
    for _ in 0..5 {
        generate(&d);
    }
    let per = t.elapsed() / 5;
    eprintln!("generate (20 structures): {per:?}");
    assert!(per.as_millis() < if cfg!(debug_assertions) { 2000 } else { 100 });
}

#[test]
fn path_does_not_wipe_spire_tree() {
    let tree = Structure::Spire { at: [0.0, 0.0], kind: 2 };
    let p = Structure::Path { path: vec![[-10.0, 2.0], [10.0, 2.0]] };
    let a = generate(&doc(vec![tree.clone()]));
    let b = generate(&doc(vec![tree, p]));
    assert_eq!(count(&a, Voxel::Blossom), count(&b, Voxel::Blossom));
    assert_eq!(count(&a, Voxel::Leaves), count(&b, Voxel::Leaves));
}

#[test]
fn huge_block_is_fast() {
    let t = std::time::Instant::now();
    let _ = generate(&doc(vec![Structure::Block { min: [-1e9, -1e9], max: [1e9, 1e9], floors: 12 }]));
    assert!(t.elapsed().as_millis() < if cfg!(debug_assertions) { 1500 } else { 60 }, "{:?}", t.elapsed());
}

#[test]
fn history_ignores_non_finite_duplicates() {
    let mut h = History::new(Document::new(1));
    let bad = Document { seed: 1, structures: vec![Structure::Spire { at: [f32::NAN, 0.0], kind: 0 }] };
    h.push(bad.clone());
    h.push(bad);
    assert!(!h.can_undo());
}

#[test]
fn spire_tree_trunk_has_no_floating_wood() {
    for at in [[-30.0, 30.0], [30.0, 20.0], [-20.0, -35.0], [35.0, -30.0]] {
        let g = generate(&doc(vec![Structure::Spire { at, kind: 2 }]));
        for z in 0..SIZE_Z as i32 {
            for x in 0..SIZE_X as i32 {
                for y in 1..SIZE_Y as i32 {
                    if g.get(x, y, z) == Voxel::Wood {
                        assert!(g.get(x, y - 1, z) != Voxel::Air, "floating wood {x},{y},{z}");
                    }
                }
            }
        }
    }
}

#[test]
fn wall_decoration_depends_on_position() {
    let lit = |dz: f32| {
        let s = Structure::Wall { path: vec![[-20.0, dz], [20.0, dz]], tall: false };
        let g = generate(&doc(vec![s]));
        (0..SIZE_Z as i32).flat_map(|z| (0..SIZE_X as i32).map(move |x| (x, z))).filter(|&(x, z)| (0..SIZE_Y as i32).any(|y| g.get(x, y, z) == Voxel::WindowLit)).collect::<Vec<_>>()
    };
    let (a, b) = (lit(-20.0), lit(20.0));
    let norm = |v: &[(i32, i32)]| v.iter().map(|p| p.0).collect::<Vec<_>>();
    assert_ne!(norm(&a), norm(&b));
}
