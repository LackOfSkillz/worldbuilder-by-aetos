//! Tests for spec §8.1's water layer (plan 2b Task 3), and for the two-phase architecture it rests
//! on (Ruling C-1): a bake never runs on a carved world, and a carved world fingerprints exactly as
//! its bare parent does.
//!
//! `pub(crate)` for [`carved_home`] alone: `wasm.rs` pins Ruling C-9 against the same carved world,
//! and a second copy of the join would be a second thing to keep in step.

use super::*;
use crate::detmath as m;
use crate::hydrology::record::{ground_fingerprint, probe_point};
use crate::hydrology::{
    Body, BodyKind, Downstream, HydroError, HydroRecord, NotchLine, ReachClass, ReachLine,
    ReachPoint,
};
use crate::surface::Surface;
use crate::water::query::{water_at, Detail, Ground, Landform, WaterKind};

const R: f64 = 6_371_000.0;
/// Metres per degree of latitude on `R`.
const M_PER_DEG: f64 = core::f64::consts::PI * R / 180.0;

/// The fixture channel: 100 m wide, bed at 5 m, cut into ground standing at 25 m.
const WIDTH_M: f64 = 100.0;
const BED_M: f64 = 5.0;
const GROUND_M: f64 = 25.0;

fn at(lat: f64, lon: f64) -> SpherePoint {
    SpherePoint::from_latlon(lat, lon)
}

fn reach_point(lat: f64, lon: f64, bed_m: f64, width_m: f64) -> ReachPoint {
    ReachPoint { lat_deg: lat, lon_deg: lon, bed_m, width_m, depth_m: 2.0, flow_m2: 1.0 }
}

fn reach(id: u32, points: Vec<ReachPoint>) -> ReachLine {
    ReachLine {
        id, class: ReachClass::River, order: 1, downstream: Downstream::Ocean, fresh: true, points,
    }
}

fn record(reaches: Vec<ReachLine>, notches: Vec<NotchLine>, bodies: Vec<Body>) -> HydroRecord {
    HydroRecord {
        bodies, reaches, notches, falls: Vec::new(),
        stats: crate::water::query::tests::stats(), ground: [0; 16],
    }
}

fn layer_over(record: HydroRecord) -> WaterLayer {
    WaterLayer::new(WaterParams::canonical(), Arc::new(IndexedRecord::new(record, R)))
}

/// One straight channel along the equator, lon 0.0 -> 0.1 -> 0.2 (two legs of 11.1 km), bed and
/// width constant so a mid-channel point's target is `BED_M` whatever the interpolation does.
fn straight() -> WaterLayer {
    straight_at(BED_M)
}

fn straight_at(bed_m: f64) -> WaterLayer {
    layer_over(record(
        vec![reach(0, vec![reach_point(0.0, 0.0, bed_m, WIDTH_M),
                           reach_point(0.0, 0.1, bed_m, WIDTH_M),
                           reach_point(0.0, 0.2, bed_m, WIDTH_M)])],
        Vec::new(), Vec::new()))
}

/// A point `metres` north of the equator at `lon`: its distance from the equatorial channel's
/// centre line is `metres`, to rounding.
fn north_of(lon: f64, metres: f64) -> SpherePoint {
    at(metres / M_PER_DEG, lon)
}

/// Area-uniform over a latitude/longitude box: stratified evenly in `sin(latitude)` and in
/// longitude, which is equal-area on a sphere, `n` by `n` cells, one sample at each cell's centre.
fn area_uniform(lat0: f64, lat1: f64, lon0: f64, lon1: f64, n: usize) -> Vec<SpherePoint> {
    let to_rad = core::f64::consts::PI / 180.0;
    let (z0, z1) = (m::sin(lat0 * to_rad), m::sin(lat1 * to_rad));
    let mut out = Vec::with_capacity(n * n);
    for i in 0..n {
        let z = z0 + (z1 - z0) * (i as f64 + 0.5) / n as f64;
        let lat = m::asin(z) / to_rad;
        for j in 0..n {
            let lon = lon0 + (lon1 - lon0) * (j as f64 + 0.5) / n as f64;
            out.push(at(lat, lon));
        }
    }
    out
}

// ---- the cross-section ---------------------------------------------------------------------

/// Twice: once on the fixture's round numbers, and once on a bed and a ground for which
/// `ground - (ground - bed)` is **not** `bed` in floating point -- the premise is asserted -- so
/// "exactly" is tested against the arithmetic that would miss it, not only against numbers where
/// any formula lands.
#[test]
fn a_point_in_mid_channel_sits_at_the_bed_exactly() {
    let (awkward_bed, awkward_ground): (f64, f64) = (3.3, 294.98674393933334);
    assert_ne!((awkward_ground - (awkward_ground - awkward_bed)).to_bits(), awkward_bed.to_bits(),
               "fixture is wrong: plain arithmetic already lands on this bed");
    for (bed, ground) in [(BED_M, GROUND_M), (awkward_bed, awkward_ground)] {
        let layer = straight_at(bed);
        // On the centre line mid-leg, on a recorded point, and 40 m off the line -- all inside the
        // 50 m half-width. Every one is the bed to the bit, not to a tolerance.
        for (name, point) in [("mid-leg", at(0.0, 0.05)), ("on a recorded point", at(0.0, 0.1)),
                              ("40 m off the line", north_of(0.15, 40.0))] {
            let (cut, authority) = layer.cut_m(&point, ground);
            assert_eq!(cut.to_bits(), bed.to_bits(), "{name}: cut to {cut}, not the bed {bed}");
            assert_eq!(authority.to_bits(), 1.0f64.to_bits(), "{name}: full authority in the channel");
        }
    }
}

#[test]
fn a_point_one_width_beyond_the_bank_is_untouched_bit_for_bit() {
    let layer = straight();
    // The bank is at half the width, 50 m; one width beyond it is 150 m. The probe is placed a
    // hair past that and its distance is MEASURED rather than assumed, so the premise is checked.
    let probe = north_of(0.05, 1.5 * WIDTH_M + 1.0e-3);
    let (d, _) = leg_foot(&probe, &at(0.0, 0.0), &at(0.0, 0.1), R);
    assert!(d >= 0.5 * WIDTH_M + WIDTH_M, "fixture is wrong: the probe is {d} m out, inside the bank");
    assert!(!layer.bake().index().candidates(&probe).reaches.is_empty(),
            "fixture is wrong: the reach is not even a candidate, so `untouched` proves nothing");
    // Every ground value comes back as the same bits -- including -0.0, which a "zero cut"
    // (`ground + 0.0`) would turn into +0.0, and NaN, which arithmetic would re-quiet.
    for ground in [GROUND_M, -0.0, 0.0, -4_600.0, 1.0e300, f64::NAN] {
        let (cut, authority) = layer.cut_m(&probe, ground);
        assert_eq!(cut.to_bits(), ground.to_bits(), "ground {ground} came back as {cut}");
        assert_eq!(authority.to_bits(), 0.0f64.to_bits(), "no authority past the bank");
    }
    // And a point in a cell the index lists nothing in.
    let far = at(30.0, 30.0);
    assert!(layer.bake().index().candidates(&far).reaches.is_empty());
    assert_eq!(layer.cut_m(&far, -0.0).0.to_bits(), (-0.0f64).to_bits());
}

/// The bank, sampled every half metre from the centre line out past the far edge. The cut must
/// never fall as the point moves out (monotone), and must never move by more than the bank's own
/// gradient allows in one step.
///
/// **The bound is derived here, not written down**: spec §8.1 blends the bank over one width, so
/// the ground climbs `GROUND_M - BED_M` over `WIDTH_M` of distance, and a step of `STEP_M` can
/// move it at most `(GROUND_M - BED_M) * STEP_M / WIDTH_M` -- plus a part in 10^9 for the rounding
/// of the distances themselves. Change the fixture's bed, width or step and the bound follows.
#[test]
fn the_bank_blend_is_monotone_and_never_steeper_than_the_bank() {
    const STEP_M: f64 = 0.5;
    assert_eq!(WaterParams::canonical().bank_widths, 1.0, "spec §8.1: one width either side");
    let bound = (GROUND_M - BED_M) * STEP_M / WIDTH_M * (1.0 + 1.0e-9);
    let layer = straight();
    let far_edge = 0.5 * WIDTH_M + WIDTH_M;
    let steps = ((far_edge + 10.0) / STEP_M) as usize; // cast-ok: a small positive whole count
    let mut previous: Option<(f64, f64)> = None;
    let mut between = 0usize;
    for k in 0..=steps {
        let (cut, authority) = layer.cut_m(&north_of(0.05, k as f64 * STEP_M), GROUND_M);
        if k == 0 {
            assert_eq!(cut.to_bits(), BED_M.to_bits(), "the centre line is the bed");
        }
        if cut > BED_M && cut < GROUND_M {
            between += 1;
        }
        if let Some((was_cut, was_authority)) = previous {
            assert!(cut >= was_cut, "step {k}: the cut fell from {was_cut} to {cut} moving out");
            assert!(authority <= was_authority, "step {k}: authority rose moving out");
            assert!(cut - was_cut <= bound,
                    "step {k}: the cut moved {} m in {STEP_M} m, over the bank's {bound} m",
                    cut - was_cut);
        }
        previous = Some((cut, authority));
    }
    let (last, _) = previous.expect("at least one sample");
    assert_eq!(last.to_bits(), GROUND_M.to_bits(), "past the far edge the ground is untouched");
    // Not vacuous: the blend actually spans the bank rather than jumping across it.
    let expected = (WIDTH_M / STEP_M) as usize - 2; // cast-ok: a small positive whole count
    assert!(between >= expected, "only {between} samples lay on the bank, wanted {expected}");
}

#[test]
fn a_notch_is_cut_the_same_way_to_its_own_cut_surface() {
    let layer = layer_over(record(
        Vec::new(),
        vec![NotchLine { points: vec![(-20.0, 40.0, 7.0, 60.0), (-20.0, 40.1, 7.0, 60.0)] }],
        Vec::new()));
    let (cut, authority) = layer.cut_m(&at(-20.0, 40.05), GROUND_M);
    assert_eq!(cut.to_bits(), 7.0f64.to_bits(), "mid-notch is the notch's cut surface");
    assert_eq!(authority, 1.0);
    let (bank, bank_authority) = layer.cut_m(&at(-20.0 + 60.0 / M_PER_DEG, 40.05), GROUND_M);
    assert!(bank > 7.0 && bank < GROUND_M, "30 m past the notch's edge is on its bank: {bank}");
    assert!(bank_authority > 0.0 && bank_authority < 1.0);
}

/// Spec §8.1's explicit exception, and the one a layer written by symmetry with reaches gets
/// wrong: **a lake's bed is not cut.** Both kinds of extent -- a shore-point lake and a traced
/// pond -- with the ground inside each standing a metre under its level, as a lake's floor does.
#[test]
fn a_lakes_interior_is_not_cut() {
    let lake = Body {
        id: 0, kind: BodyKind::Lake, fresh: true, enclosed: false, forced: false,
        level_m: 30.0, area_m2: 1.0e8, depth_m: 12.0, outlet_reach: None,
        anchor: (10.0, 10.0),
        outline: vec![(9.9, 10.0), (10.1, 10.0), (10.0, 9.9), (10.0, 10.1),
                      (9.8, 10.0), (10.2, 10.0), (10.0, 9.8), (10.0, 10.2)],
        downstream: Downstream::Ocean, shore_member_count: 4, shore_reach_m: 5_000.0,
    };
    let pond = Body {
        id: 1, kind: BodyKind::Pond, fresh: true, enclosed: false, forced: false,
        level_m: 30.0, area_m2: 1.0e6, depth_m: 3.0, outlet_reach: None,
        anchor: (20.0, 20.0),
        outline: vec![(20.0, 20.0), (20.01, 20.0), (20.01, 20.01), (20.0, 20.01)],
        downstream: Downstream::Sink, shore_member_count: 0, shore_reach_m: 0.0,
    };
    let fixture = record(Vec::new(), Vec::new(), vec![lake, pond]);
    let layer = layer_over(fixture.clone());
    let index = layer.bake().index();
    for (id, probe) in [(0u32, at(10.0, 10.0)), (1u32, at(20.005, 20.005))] {
        assert!(index.candidates(&probe).bodies.contains(&id),
                "fixture is wrong: body {id} is not a candidate, so `not cut` proves nothing");
        // The query agrees this is inside the body, so the probe really is a lake's interior.
        let floor = fixture.bodies[id as usize].level_m - 1.0;
        let flat = move |_: &SpherePoint| floor;
        let answer = water_at(&fixture, index,
                              &Ground { landform_m: Landform(&flat), detail_m: Detail(&flat) }, &probe);
        assert_eq!(answer.body_id, id, "fixture is wrong: the query does not put the probe in body {id}");
        let (cut, authority) = layer.cut_m(&probe, floor);
        assert_eq!(cut.to_bits(), floor.to_bits(), "body {id}'s floor was cut to {cut}");
        assert_eq!(authority.to_bits(), 0.0f64.to_bits(), "a lake is not a channel");
    }
}

/// **The index lists a channel wherever its bank reaches, not only where its water does.** Plan
/// 2a's index listed a reach within half its width, which is all the query needs; the carve
/// reaches a bank further. A channel just south of a cell line has its bank spill across it, and a
/// bank point in the northern cell must still be cut -- or the carve stops dead at the cell line,
/// a cliff along a boundary nobody drew. Both shapes the index lists: a one-point reach (a disc)
/// and a short leg, each 2 km wide, each 2.2 km south of the line, probed 0.6 km north of it.
#[test]
fn a_bank_across_a_cell_line_from_its_channel_is_still_cut() {
    // Find a row boundary by walking north from the equator in 10 m steps.
    let grid = crate::hydrology::buckets::BucketIndex::new(R, DEFAULT_CELL_M);
    let start = grid.cell_of(&at(0.0, 0.0));
    let mut metres = 0.0;
    while grid.cell_of(&north_of(0.0, metres)) == start {
        metres += 10.0;
    }
    // 2.2 km south of the line and 0.6 km north of it: 2.8 km apart, inside a 2 km channel's
    // 1 km half-width plus its 2 km bank, and farther past the line than half the width plus half
    // a short leg's sample gap -- which is all plan 2a's index dilated a leg by.
    let channel_lat = (metres - 2_200.0) / M_PER_DEG;
    let probe = north_of(0.0, metres + 600.0);
    assert_ne!(grid.cell_of(&probe), grid.cell_of(&at(channel_lat, 0.0)),
               "fixture is wrong: the probe is in the channel's own cell");
    for (name, points) in [
        ("a one-point reach", vec![reach_point(channel_lat, 0.0, BED_M, 2_000.0)]),
        ("a short leg", vec![reach_point(channel_lat, -0.005, BED_M, 2_000.0),
                             reach_point(channel_lat, 0.005, BED_M, 2_000.0)]),
    ] {
        let layer = layer_over(record(vec![reach(0, points)], Vec::new(), Vec::new()));
        let (cut, authority) = layer.cut_m(&probe, GROUND_M);
        // 2.8 km from a 2 km channel's centre: past its 1 km half-width, inside its 2 km bank.
        assert!(authority > 0.0 && authority < 1.0, "{name}: authority {authority} on the bank");
        assert!(cut < GROUND_M, "{name}: the bank across the cell line was not cut");
    }
}

// ---- a cut never raises -----------------------------------------------------------------------

/// **A raising cut is a dam.** A reach whose bed rises above the ground at its last step -- Ruling
/// R-4's mouth, stood up deliberately here -- and a notch whose cut surface stands 40 m above the
/// ground it crosses, under ground that is not flat, swept area-uniformly over the whole neighbourhood.
#[test]
fn the_layer_never_raises_ground_across_a_rising_mouth() {
    let layer = layer_over(record(
        vec![reach(0, vec![reach_point(0.0, 0.0, 5.0, 150.0),
                           reach_point(0.0, 0.01, 4.0, 150.0),
                           reach_point(0.0, 0.02, 30.0, 150.0)])],
        vec![NotchLine { points: vec![(0.004, 0.0, 50.0, 80.0), (0.004, 0.02, 50.0, 80.0)] }],
        Vec::new()));
    // Ground rising gently eastward from 8 m to about 12 m: under the mouth's 30 m bed, and
    // under the notch's 50 m surface everywhere.
    let ground = |p: &SpherePoint| 8.0 + 200.0 * p.to_latlon().1;
    let (mut lowered, mut held_in_channel, mut on_bank) = (0usize, 0usize, 0usize);
    for p in area_uniform(-0.006, 0.008, -0.004, 0.024, 160) {
        let g = ground(&p);
        let (cut, authority) = layer.cut_m(&p, g);
        assert!(cut <= g, "raised {} m at {:?}", cut - g, p.to_latlon());
        if cut < g {
            lowered += 1;
        }
        if authority == 1.0 && cut == g {
            held_in_channel += 1; // in a channel whose target stands above the ground: the clamp
        }
        if authority > 0.0 && authority < 1.0 {
            on_bank += 1;
        }
    }
    assert!(lowered > 0 && on_bank > 0, "vacuous sweep: {lowered} lowered, {on_bank} on a bank");
    assert!(held_in_channel > 0, "the sweep never stood in the rising mouth or the notch");
}

/// The same property on a real bake: every reach and notch of `bake_tests::world()`, its own
/// ground at canonical resolution, swept area-uniformly around every recorded leg -- and over the
/// whole planet, where nearly every point is out of reach and must come back bit for bit.
#[test]
fn the_layer_never_raises_ground_on_a_real_bake() {
    let world = crate::hydrology::bake_tests::world();
    let baked = crate::hydrology::bake(&world, &crate::hydrology::bake_tests::params())
        .expect("bake");
    let layer = layer_over(baked.clone());
    let mut lowered = 0usize;
    let mut legs = 0usize;
    let mut sweep = |p: &SpherePoint| {
        let g = world.bake_ground_m(p, None);
        let (cut, authority) = layer.cut_m(p, g);
        assert!(cut <= g, "raised {} m at {:?}", cut - g, p.to_latlon());
        if authority == 0.0 {
            assert_eq!(cut.to_bits(), g.to_bits(), "no authority, yet the ground moved");
        }
        if cut < g {
            lowered += 1;
        }
    };
    let lines = baked.reaches.iter()
        .map(|r| r.points.iter().map(|p| (p.lat_deg, p.lon_deg, p.width_m)).collect::<Vec<_>>())
        .chain(baked.notches.iter()
            .map(|n| n.points.iter().map(|&(la, lo, _, w)| (la, lo, w)).collect::<Vec<_>>()));
    for line in lines {
        for pair in line.windows(2) {
            let ((la, lo, wa), (lb, lob, wb)) = (pair[0], pair[1]);
            if (lo - lob) > 180.0 || (lob - lo) > 180.0 {
                continue; // a leg across the seam: a lat/lon box around it would span the planet
            }
            legs += 1;
            let pad = footprint_m(leg_width_m(wa, wb)) / M_PER_DEG;
            let (lat0, lat1) = if la < lb { (la, lb) } else { (lb, la) };
            let (lon0, lon1) = if lo < lob { (lo, lob) } else { (lob, lo) };
            for p in area_uniform(lat0 - pad, lat1 + pad, lon0 - pad, lon1 + pad, 6) {
                sweep(&p);
            }
        }
    }
    // The whole planet, area-uniformly.
    for p in area_uniform(-90.0, 90.0, -180.0, 180.0, 100) {
        sweep(&p);
    }
    assert!(legs > 100, "vacuous: only {legs} recorded legs were swept");
    assert!(lowered > 0, "vacuous: nothing on {legs} legs was cut at all");
}

// ---- the carve and the query agree ------------------------------------------------------------

/// **"This is a river" and "this is a channel" are one test.** A tapering reach (Ruling Q-7's
/// wider-endpoint width is only visible on a taper), a bend, and a one-point reach, swept
/// area-uniformly: the query answers `River` exactly where the layer's authority is 1.
#[test]
fn the_carve_and_the_query_agree_about_where_the_channel_is() {
    let fixture = record(
        vec![reach(0, vec![reach_point(0.0, 0.0, 5.0, 80.0),
                           reach_point(0.0, 0.004, 4.0, 240.0),
                           reach_point(0.003, 0.007, 3.0, 120.0)]),
             reach(1, vec![reach_point(-0.002, 0.002, 6.0, 90.0)])],
        Vec::new(), Vec::new());
    let layer = layer_over(fixture.clone());
    let index = layer.bake().index();
    let high = |_: &SpherePoint| 100.0;
    let ground = Ground { landform_m: Landform(&high), detail_m: Detail(&high) };
    let (mut rivers, mut banks) = (0usize, 0usize);
    for p in area_uniform(-0.004, 0.005, -0.002, 0.009, 180) {
        let is_river = water_at(&fixture, index, &ground, &p).kind == WaterKind::River;
        let (_, authority) = layer.cut_m(&p, 100.0);
        assert_eq!(is_river, authority == 1.0,
                   "at {:?} the query says river={is_river} and the carve's authority is {authority}",
                   p.to_latlon());
        if is_river {
            rivers += 1;
        } else if authority > 0.0 {
            banks += 1;
        }
    }
    assert!(rivers > 0 && banks > 0, "vacuous sweep: {rivers} river points, {banks} bank points");
}

// ---- the two phases (Ruling C-1) --------------------------------------------------------------

/// The world `bake_tests` bakes: seed 20,260,904, 6,371 km, 12 plates, 0.29 land.
pub(crate) fn home() -> Surface {
    Surface::new(20_260_904, R, 12, 0.29, None, None, None)
}

/// `home()`, carved by a hand-written record whose one reach runs straight across fingerprint
/// probe 0 with its bed 50 m under the ground there -- so if the carve reached the fingerprint,
/// the digest would move. Returns `(bare, carved, probe 0)`.
pub(crate) fn carved_home() -> (Surface, Surface, SpherePoint) {
    let bare = home();
    let probe = probe_point(0);
    let (lat, lon) = probe.to_latlon();
    let bed = bare.elevation_m(&probe, None) - 50.0;
    let mut joined = record(
        vec![reach(0, vec![reach_point(lat, lon - 0.01, bed, 200.0),
                           reach_point(lat, lon + 0.01, bed, 200.0)])],
        Vec::new(), Vec::new());
    joined.ground = ground_fingerprint(&bare);
    let carve = Carve { params: WaterParams::canonical(), bake: Arc::new(IndexedRecord::new(joined, R)) };
    let carved = Surface::with_water(20_260_904, R, 12, 0.29, None, None, None, None, None, None,
                                     Some(carve))
        .expect("a record joined to its own world");
    (bare, carved, probe)
}

#[test]
fn an_absent_block_leaves_structural_and_elevation_bit_identical() {
    let plain = home();
    let absent = Surface::with_water(20_260_904, R, 12, 0.29, None, None, None, None, None, None, None)
        .expect("None never refuses");
    assert!(!absent.is_carved());
    for p in area_uniform(-90.0, 90.0, -180.0, 180.0, 40) {
        assert_eq!(absent.structural_m(&p).to_bits(), plain.structural_m(&p).to_bits());
        for resolution in [None, Some(250.0), Some(20_000.0)] {
            assert_eq!(absent.elevation_m(&p, resolution).to_bits(),
                       plain.elevation_m(&p, resolution).to_bits(),
                       "elevation at {:?}, resolution {resolution:?}", p.to_latlon());
            assert_eq!(absent.bake_ground_m(&p, resolution).to_bits(),
                       plain.elevation_m(&p, resolution).to_bits());
        }
    }
}

/// **The bake cannot be computed on a carved surface** -- refused, not merely documented against.
#[test]
fn a_carved_world_is_refused_by_the_bake() {
    let (bare, carved, _) = carved_home();
    assert!(carved.is_carved() && !bare.is_carved());
    let params = crate::hydrology::bake_tests::params();
    assert_eq!(crate::hydrology::bake(&carved, &params).err(), Some(HydroError::Carved));
    assert_eq!(crate::hydrology::bake_stages(&carved, &params).err(), Some(HydroError::Carved));
    // The land graph is a bake's input and a public door of its own: it reads carved ground
    // through `moisture_index`, so it refuses too -- and the bare parent still samples.
    let graph = |s: &Surface| {
        crate::hydrology::landgraph::LandGraph::sample(s, params.total_nodes, params.wetness_nodes)
    };
    assert!(graph(&carved).is_none(), "LandGraph::sample sampled a carved world");
    assert!(graph(&bare).is_some(), "the bare parent must still sample, or the refusal proves nothing");
}

/// **A carved world and its bare parent fingerprint identically** -- which is what lets a record be
/// checked against the carved world at all (plan 2b Task 2). Not vacuous: the carve moves the
/// ground at probe 0 by tens of metres, so a fingerprint that read the carved ground would differ.
#[test]
fn a_carved_world_fingerprints_exactly_as_its_bare_parent() {
    let (bare, carved, probe) = carved_home();
    let moved = bare.elevation_m(&probe, None) - carved.elevation_m(&probe, None);
    assert!(moved > 1.0, "fixture is wrong: the carve moved probe 0 by only {moved} m");
    assert_eq!(ground_fingerprint(&carved), ground_fingerprint(&bare));
    // Why: `structural_m` does not see the layer, and `bake_ground_m` is the bare parent's
    // `elevation_m` bit for bit -- in the channel, on its bank and far from it.
    let (lat, lon) = probe.to_latlon();
    let mut points = area_uniform(lat - 0.01, lat + 0.01, lon - 0.012, lon + 0.012, 30);
    points.push(probe);
    for p in points {
        assert_eq!(carved.structural_m(&p).to_bits(), bare.structural_m(&p).to_bits());
        for resolution in [None, Some(250.0)] {
            assert_eq!(carved.bake_ground_m(&p, resolution).to_bits(),
                       bare.elevation_m(&p, resolution).to_bits(),
                       "bake ground at {:?}, resolution {resolution:?}", p.to_latlon());
        }
    }
}

/// The join is where the refusals live: a record from other ground, an index built at another
/// radius, and a block that is not admissible are each refused, and each by name.
#[test]
fn the_join_refuses_a_foreign_record_a_foreign_radius_and_a_bad_block() {
    let bare = home();
    let good = {
        let mut r = record(Vec::new(), Vec::new(), Vec::new());
        r.ground = ground_fingerprint(&bare);
        r
    };
    let join = |params: WaterParams, record: HydroRecord, radius_m: f64| {
        Surface::with_water(20_260_904, R, 12, 0.29, None, None, None, None, None, None,
                            Some(Carve { params, bake: Arc::new(IndexedRecord::new(record, radius_m)) }))
            .err()
    };
    assert_eq!(join(WaterParams::canonical(), good.clone(), R), None, "its own world joins");
    let mut foreign = good.clone();
    foreign.ground[0] ^= 1;
    assert!(matches!(join(WaterParams::canonical(), foreign, R), Some(CarveRefused::Foreign(_))));
    assert_eq!(join(WaterParams::canonical(), good.clone(), 6_000_000.0), Some(CarveRefused::Radius));
    for bank_widths in [0.0, -1.0, MAX_BANK_WIDTHS * 1.01, f64::NAN, f64::INFINITY] {
        assert_eq!(join(WaterParams { bank_widths }, good.clone(), R), Some(CarveRefused::Params),
                   "bank_widths {bank_widths}");
    }
    assert_eq!(join(WaterParams { bank_widths: MAX_BANK_WIDTHS }, good, R), None);
}

// ---- a lake's bed is not a river's channel (Task 3 review blocker) ------------------------------

/// Every sample of a real bake that the query answers as a body -- a lake, a pond, a salt lake or a
/// salt flat -- together with the id that answered: an area-uniform 40 by 40 grid over every body's
/// outline box (padded by its shore band), plus 35 points in every reach's channel, which is where a
/// body a reach flows into or through meets its channel.
fn body_samples(bare: &Surface, bake: &IndexedRecord) -> Vec<(SpherePoint, u32)> {
    let record = bake.record();
    let mut points = channel_samples(bake);
    for body in &record.bodies {
        let (mut lat0, mut lat1, mut lon0, mut lon1) = (90.0f64, -90.0f64, 180.0f64, -180.0f64);
        for &(la, lo) in &body.outline {
            if la < lat0 { lat0 = la; }
            if la > lat1 { lat1 = la; }
            if lo < lon0 { lon0 = lo; }
            if lo > lon1 { lon1 = lo; }
        }
        if body.outline.is_empty() || lon1 - lon0 > 180.0 {
            continue; // no outline, or a box across the seam that would span the planet
        }
        let pad = body.shore_reach_m / M_PER_DEG;
        let (lat0, lat1) = (if lat0 - pad < -90.0 { -90.0 } else { lat0 - pad },
                            if lat1 + pad > 90.0 { 90.0 } else { lat1 + pad });
        points.extend(area_uniform(lat0, lat1, lon0 - pad, lon1 + pad, 40));
    }
    let cell_m = record.stats.pond_cell_m;
    let landform = |q: &SpherePoint| bare.structural_m(q);
    let detail = |q: &SpherePoint| bare.bake_ground_m(q, Some(cell_m));
    let ground = Ground { landform_m: Landform(&landform), detail_m: Detail(&detail) };
    points.into_iter()
        .filter_map(|p| {
            let q = water_at(record, bake.index(), &ground, &p);
            match q.kind {
                WaterKind::Lake | WaterKind::Pond | WaterKind::SaltLake | WaterKind::SaltFlat =>
                    Some((p, q.body_id)),
                _ => None,
            }
        })
        .collect()
}

/// **Spec §8.1: lake beds are not cut -- on real bakes where reaches actually enter bodies.** The
/// fine pond search looks for ponds along reach lines, so a pond sitting on a river is the ordinary
/// case, and a coarse lake a reach flows into is another. `bake_tests::world()` at
/// `earth_like(30_000)` and `earth_like(60_000)`: every sample the query answers as a body keeps
/// its ground to the bit, carved world against bare, and the layer claims no authority there.
///
/// Not vacuous: each bake must put channel samples inside bodies -- the population the first lake
/// test could not reach, because no reach came near its lakes.
#[test]
fn no_body_is_cut_where_reaches_enter_it() {
    let mut failures: Vec<String> = Vec::new();
    for nodes in [30_000u32, 60_000] {
        let bare = home();
        let baked = crate::hydrology::bake(&bare, &crate::hydrology::HydroParams::earth_like(nodes))
            .expect("the bare world bakes");
        let bake = Arc::new(IndexedRecord::new(baked, R));
        let carved = Surface::with_water(20_260_904, R, 12, 0.29, None, None, None, None, None, None,
                                         Some(Carve { params: WaterParams::canonical(), bake: bake.clone() }))
            .expect("joins its own world");
        let mut cut_bodies: Vec<u32> = Vec::new();
        let (mut samples, mut lowered, mut in_channel, mut worst) = (0usize, 0usize, 0usize, 0.0f64);
        let channel = channel_samples(&bake);
        let samples_in = body_samples(&bare, &bake);
        for (p, id) in &samples_in {
            samples += 1;
            if channel.iter().any(|c| c.vector == p.vector) {
                in_channel += 1;
            }
            let was = bare.elevation_m(p, None);
            let now = carved.elevation_m(p, None);
            if now.to_bits() != was.to_bits() {
                if !cut_bodies.contains(id) {
                    cut_bodies.push(*id);
                }
                if was - now > 0.5 {
                    lowered += 1;
                }
                if was - now > worst {
                    worst = was - now;
                }
            }
        }
        eprintln!("earth_like({nodes}): {} bodies, {samples} body samples ({in_channel} in a channel), \
                   {} bodies cut, {lowered} samples lowered > 0.5 m, deepest {worst} m",
                  bake.record().bodies.len(), cut_bodies.len());
        assert!(in_channel > 0, "earth_like({nodes}): vacuous, no reach's channel enters a body");
        if !cut_bodies.is_empty() {
            failures.push(format!(
                "earth_like({nodes}): {} of {} bodies cut ({cut_bodies:?}), {lowered} samples \
                 lowered by more than 0.5 m, deepest {worst} m",
                cut_bodies.len(), bake.record().bodies.len()));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("; "));
}

// ---- texture defers to a channel (plan 2b Task 4; Rulings C-13 and C-14) -----------------------

/// `bake_tests::world()`, with or without a gully block, baked bare at `bake_tests::params()` and
/// then carved by its own record: `(bare, carved, bake)`.
fn carved_by_its_own_bake(gully: Option<crate::detail::GullyParams>)
                          -> (Surface, Surface, Arc<IndexedRecord>) {
    let bare = Surface::with_gully(20_260_904, R, 12, 0.29, None, None, None, None, gully);
    let baked = crate::hydrology::bake(&bare, &crate::hydrology::bake_tests::params())
        .expect("the bare world bakes");
    let bake = Arc::new(IndexedRecord::new(baked, R));
    let carved = Surface::with_water(20_260_904, R, 12, 0.29, None, None, None, None, gully, None,
                                     Some(Carve { params: WaterParams::canonical(), bake: bake.clone() }))
        .expect("a record joined to the world it was baked from");
    (bare, carved, bake)
}

/// Points **in** every recorded reach's channel: along every leg at seven interior fractions, each
/// on the centre line and at 50% and 90% of the half-width to either side -- 35 a leg. Placed on
/// the leg's own great circle (a normalised blend of its ends) and pushed off it along the arc's
/// normal, so "inside the half-width" is by construction rather than by luck of a scatter.
fn channel_samples(bake: &IndexedRecord) -> Vec<SpherePoint> {
    let mut out = Vec::new();
    for reach in &bake.record().reaches {
        for pair in reach.points.windows(2) {
            let (a, b) = (at(pair[0].lat_deg, pair[0].lon_deg), at(pair[1].lat_deg, pair[1].lon_deg));
            let Some(normal) = a.vector.cross(&b.vector).normalised() else { continue };
            let half = half_of(leg_width_m(pair[0].width_m, pair[1].width_m));
            for k in 1..8 {
                let t = f64::from(k) / 8.0;
                let blend = a.vector.scaled(1.0 - t).add(&b.vector.scaled(t));
                let Some(on) = SpherePoint::from_vector(&blend) else { continue };
                for share in [-0.9, -0.5, 0.0, 0.5, 0.9] {
                    let pushed = on.vector.add(&normal.scaled(share * half / R));
                    if let Some(p) = SpherePoint::from_vector(&pushed) {
                        out.push(p);
                    }
                }
            }
        }
    }
    out
}

/// The query over a carved world, grounded exactly as `wasm.rs::with_ground` grounds it: the
/// landform for coarse bodies and reaches, the bare ground at `pond_cell_m` for fine-found ones.
fn ask_carved(carved: &Surface, bake: &IndexedRecord, p: &SpherePoint) -> crate::water::WaterAt {
    let cell_m = bake.record().stats.pond_cell_m;
    let landform = |q: &SpherePoint| carved.structural_m(q);
    let detail = |q: &SpherePoint| carved.bake_ground_m(q, Some(cell_m));
    water_at(bake.record(), bake.index(),
             &Ground { landform_m: Landform(&landform), detail_m: Detail(&detail) }, p)
}

/// **Ruling C-13: the query's water surface and the carve's bed are one geometry.** Wherever the
/// query answers `River`, the channel the layer cuts there -- into ground as high as you like --
/// stands at or under the level the query reports. Before C-13 the query read the nearest recorded
/// point's level, a step along each reach, and the interpolated bed stood above it: a river that
/// reads dry in its own channel.
#[test]
fn the_querys_water_surface_never_stands_below_the_carved_bed() {
    let (_, carved, bake) = carved_by_its_own_bake(None);
    let layer = WaterLayer::new(WaterParams::canonical(), bake.clone());
    let mut rivers = 0usize;
    for p in channel_samples(&bake) {
        let q = ask_carved(&carved, &bake, &p);
        if q.kind != WaterKind::River {
            continue; // a mouth handed to a body or the sea: not this reach's surface to report
        }
        rivers += 1;
        let (bed, authority) = layer.cut_m(&p, 1.0e6);
        assert_eq!(authority, 1.0, "the query says river and the carve says bank at {:?}", p.to_latlon());
        assert!(bed <= q.level_m, "the bed {bed} m stands above the water {} m at {:?}",
                q.level_m, p.to_latlon());
    }
    assert!(rivers > 1_000, "vacuous: only {rivers} channel samples answered river");
}

/// **Texture cannot dam a river.** Over the whole length of every channel of a real bake, no
/// sample of the carved world's ground -- detail and all, canonical and at a viewer's 250 m --
/// stands above the water level the query reports there. That is the property; "the amplitude
/// was multiplied by `1 - authority`" is only how it is kept.
///
/// Run on two worlds: the plain one, and the same world with the gully block on, because the
/// gully term is a second texture with its own damping and a channel with a gully across it is
/// dammed too.
///
/// **Not vacuous**, and asserted rather than hoped: at a good share of these very samples the bare
/// world's own texture (its `elevation_m` less its `structural_m`) stands taller than the water
/// is deep, so the same texture laid on the carved bed undamped would break the surface.
#[test]
fn no_detail_sample_rises_above_the_water_along_a_channel() {
    for (name, gully) in [("plain", None), ("gullied", Some(crate::detail::GullyParams::drainage()))] {
        let (bare, carved, bake) = carved_by_its_own_bake(gully);
        let layer = WaterLayer::new(WaterParams::canonical(), bake.clone());
        let (mut rivers, mut would_dam) = (0usize, 0usize);
        for p in channel_samples(&bake) {
            let q = ask_carved(&carved, &bake, &p);
            if q.kind != WaterKind::River {
                continue;
            }
            rivers += 1;
            for resolution in [None, Some(250.0)] {
                let ground = carved.elevation_m(&p, resolution);
                assert!(ground <= q.level_m,
                        "{name}: ground {ground} m over water {} m at {:?}, resolution {resolution:?}",
                        q.level_m, p.to_latlon());
            }
            let texture = bare.elevation_m(&p, None) - bare.structural_m(&p);
            let bed = layer.cut_m(&p, bare.structural_m(&p)).0;
            if bed + texture > q.level_m {
                would_dam += 1;
            }
        }
        assert!(rivers > 1_000, "{name}: vacuous, only {rivers} channel samples answered river");
        assert!(would_dam * 10 > rivers,
                "{name}: the premise is weak -- undamped texture would break the surface at only \
                 {would_dam} of {rivers} samples");
    }
}

/// A carve with nothing to cut is its bare parent, bit for bit -- elevation and the gully term
/// alike -- because an uncut point's damping factor is `(1 - a) * 1.0`, which is `1 - a` exactly.
/// What lets a carved world differ from its parent only where water runs.
#[test]
fn a_carve_with_nothing_to_cut_is_its_bare_parent_bit_for_bit() {
    for gully in [None, Some(crate::detail::GullyParams::drainage())] {
        let bare = Surface::with_gully(20_260_904, R, 12, 0.29, None, None, None, None, gully);
        let mut empty = record(Vec::new(), Vec::new(), Vec::new());
        empty.ground = ground_fingerprint(&bare);
        let carve = Carve { params: WaterParams::canonical(), bake: Arc::new(IndexedRecord::new(empty, R)) };
        let carved = Surface::with_water(20_260_904, R, 12, 0.29, None, None, None, None, gully, None,
                                         Some(carve))
            .expect("joins");
        for p in area_uniform(-90.0, 90.0, -180.0, 180.0, 30) {
            for resolution in [None, Some(250.0)] {
                assert_eq!(carved.elevation_m(&p, resolution).to_bits(),
                           bare.elevation_m(&p, resolution).to_bits(),
                           "gully {}: {:?}", gully.is_some(), p.to_latlon());
            }
        }
    }
}
