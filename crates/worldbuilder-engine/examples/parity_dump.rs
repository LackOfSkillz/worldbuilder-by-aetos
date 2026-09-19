//! Dump a native corpus for the native-against-WASM parity harness.
//!
//! Every value crosses the *shipped* `extern "C"` surface -- `wb_world_new`,
//! `wb_world_new_relief`, `wb_world_new_tectonic`, `wb_relief_preset`, `wb_tectonic_preset`,
//! `wb_tectonic_check`, `wb_elevation_m`, `wb_structural_m`, `wb_bottom_at`,
//! `wb_fill_tile_f32`, `wb_erosion_run`, `wb_water_run` -- never an internal function,
//! because the claim under test is about what the browser calls.
//!
//! **Two of those exist so that a module could be compared at all.** `wb_erosion_run` (slice
//! 5a Task 5) is the only door into `erosion.rs`, and `wb_water_run` (slice 5b Task 5) is the
//! only door into `water.rs`: before each existed, that module's own claim of native/WASM
//! agreement was *unfalsifiable* -- not unverified -- because nothing in the export surface
//! touched it. The relief entries are a different shape of gap: the surface *did* reach
//! `detail.rs`'s relief block after the relief slice's Task 4, and nothing here had ever sent
//! one, because every world above is built through `wb_world_new`, which sends `None`.
//!
//! **The tectonic entries are a third shape of gap: three tasks flagged it and none owned
//! it.** Mountains Task 4, Task 3 and Task 5 each reported, correctly, that no value in this
//! corpus went through a tectonic export -- while `TectonicParams::ranges()` is a preset the
//! owner presses on the panel and drives seven of sixteen words across the boundary. Native
//! and WASM are the same Rust over the same pure-Rust `libm`, so the comparison is *strict
//! bit-for-bit* even with transcendentals in the path; what is boundary-only is the DECODE,
//! and nothing exercised it.
//!
//! **Two derivations here are NOT compared values and are labelled as such:** the `WCTL`
//! record carries the water control's predicted divergence, computed from
//! `water::lake_body_surface_areas_m2` rather than from the classifier the control perturbs,
//! and cross-checked against that classifier before it is written; and the `TCTL` record
//! carries the tectonic control's predicted divergence per group, computed through the
//! exports *and* through the library's own `Surface` with blocks read from `tectonics.rs`
//! rather than from the words that crossed the boundary, and cross-checked between the two
//! before it is written. A control gate read off the control's own run is a rubber stamp;
//! both of these are predictions the replaying side has to meet.
//!
//! **The `WC` records are the carve itself** (plan 2b Task 7): a world built through
//! `wb_world_new_water` over a held bake baked *for carving*, sampled at points chosen from the
//! record by category -- in a channel, on a bank, in a body, at a notch, clear of water -- and
//! refused if any category stops being covered (see [`carve_points`]). Every earlier group only
//! proved the carve stays OUT of the canonical path; these compare what it cuts. The `HC` records
//! are the bakes they join, word for word, checked natively to differ from their ordinary twins
//! only where the drain says ([`check_drain`]); `CBANK`/`CBCTL` are the carve's own control.
//!
//! The output is the corpus *and* its answers: every f64 is written as its 16-hex-digit
//! bit pattern, so the replaying side parses no decimal text and the comparison is exact.
//! `parity/parity.mjs` reads this file, replays the identical inputs through the committed
//! `.wasm`, and compares bit patterns. The corpus is therefore defined once, here, and
//! cannot drift between the two sides.
//!
//! Run: `cargo run --release --example parity_dump --features wasm > native.txt`

use worldbuilder_engine::continentality::CoastParams;
use worldbuilder_engine::detail::GullyParams;
use worldbuilder_engine::hydrology;
use worldbuilder_engine::sphere::SpherePoint;
use worldbuilder_engine::stream::{sample_nodes, BuildParams, SamplingKind, StreamGraph};
use worldbuilder_engine::surface::Surface;
use worldbuilder_engine::tectonics::TectonicParams;
use worldbuilder_engine::wasm::*;
use worldbuilder_engine::water;

const SEED: i64 = 20_260_904;
const RADIUS_M: f64 = 6_371_000.0;
const PLATES: u32 = 12;
const LAND: f64 = 0.29;
const RES_M: f64 = 250.0;
const HARBOUR_LAT: f64 = -18.25;
const HARBOUR_LON: f64 = 121.5;

/// The extraction's harbour, as this module's flat f64 records.
fn harbour_records() -> Vec<f64> {
    vec![
        HARBOUR_LAT, HARBOUR_LON, -12.0, 900.0, 260.0, 35.0, WB_COMPOSE_CARVE, WB_SUBSTRATE_DERIVE,
        HARBOUR_LAT, HARBOUR_LON, 4.0, 200.0, 60.0, 35.0, WB_COMPOSE_RAISE, WB_SUBSTRATE_DERIVE,
    ]
}

/// SplitMix64. The scatter has to be reproducible for the dump to be re-derivable, but the
/// replaying side never runs it -- it reads the points back out of the file.
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// A float in [0, 1), from 53 bits.
    fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
    }
}

fn hex(value: f64) -> String {
    format!("{:016x}", value.to_bits())
}

fn hex32(value: f32) -> String {
    format!("{:08x}", value.to_bits())
}

/// One hydro bake through the shipped exports, native side. Returns the status and, on
/// `WB_OK`, the full word vector; on any other status the word vector is empty and the length
/// is 0, since `out_id` was never written and there is nothing to copy.
fn bake_hydro_native(world: u32, params: &[f64]) -> (u32, u32, Vec<f64>) {
    let mut id: u32 = 0;
    let status = wb_hydro_bake(world, params.as_ptr(), params.len() as u32, &mut id); // cast-ok: params is a small, compile-time-bounded local buffer
    if status != WB_OK {
        return (status, 0, Vec::new());
    }
    let len = wb_hydro_len(id);
    let mut words = vec![0.0f64; len as usize]; // cast-ok: a freshly measured record length used to size its own buffer
    assert_eq!(wb_hydro_copy(id, words.as_mut_ptr(), len), WB_OK);
    assert_eq!(wb_hydro_free(id), WB_OK);
    (status, len, words)
}

/// **Ruling Q-21.** The explicit `WP` sample points beside the `WQ` grid, chosen **from the
/// record** rather than by hand, so the parity corpus carries every §8.3 kind the bake actually
/// records -- and a non-sentinel `reach_id` with them.
///
/// The grid covers `none`, `Ocean` and `Lake` densely and reaches nothing else: a river is a few
/// hundred metres wide and a 4-degree box steps about 14 km, so a fixed grid catches `River` and
/// `Pond` only by luck. Those two are the branches the drawing path uses most, and before this
/// neither crossed the boundary at all.
///
/// The rules, all deterministic and all read off the record:
///
/// - **`Lake`, `SaltLake`, `SaltFlat`, `Pond`** -- the **lowest-id** body of that kind, sampled at
///   its own `anchor`. Ruling Q-14's reason applies here too: every recorded outline point is on a
///   shore by construction, so an anchor is the only point that tests the interior.
/// - **`River`** -- the **lowest-id** reach with at least three recorded points whose **middle**
///   recorded point answers `River`. The middle, not an end: a mouth sits at a shore, where Ruling
///   Q-5 hands the answer to the body, and this point exists to carry a real `reach_id`.
/// - **`FineFound`** -- the **lowest-id** body with `shore_member_count == 0`, sampled at its
///   `anchor`. **This is the point that stands in for the pond, and the substitution is measured,
///   not preferred.** Neither parity bake records a body of `BodyKind::Pond`: that kind is an
///   *area* classification (`pond_max_surface_area_m2` against a summed surface area), and at
///   these node counts -- 20,000 on `plain`, 60,000 on `ranges` -- every kept body is above the
///   threshold. What the two bakes DO hold is bodies the **fine pond search** found, which are the
///   ones Ruling Q-16 makes the query read the **detail field** for rather than the landform.
///   That is the branch worth putting on the wire; `BodyKind::Pond` is a label on the same branch
///   that these bakes happen not to apply. `shore_member_count == 0` is the discriminator (Ruling
///   E-8), not `kind`, which is exactly why this point is chosen by it.
///
/// A kind the record has no body of yields no point, is named in the returned report, and is not
/// an error -- a bake with no salt flat cannot be made to produce one. **What IS an error is a
/// chosen point that stops covering what it was chosen for**: `main` asserts that and refuses to
/// write the corpus, which is what keeps this group honest as the bake moves under it.
///
/// Returns the points and, for the dump's own stderr report, what each one was chosen for.
fn water_points_from(world: u32, bake: u32, record: &hydrology::HydroRecord)
    -> Vec<(&'static str, f64, f64, u32, [f64; WP_STRIDE])> {
    let ask = |lat: f64, lon: f64| -> (u32, [f64; WP_STRIDE]) {
        let mut out = [0.0f64; WP_STRIDE];
        let status = wb_water_at(world, bake, lat, lon, out.as_mut_ptr(), WP_STRIDE as u32); // cast-ok: a compile-time stride of five
        (status, out)
    };

    let mut points = Vec::new();
    for (want, kind) in [
        ("Lake", hydrology::BodyKind::Lake),
        ("SaltLake", hydrology::BodyKind::SaltLake),
        ("SaltFlat", hydrology::BodyKind::SaltFlat),
        ("Pond", hydrology::BodyKind::Pond),
    ] {
        // `record.bodies` is written in ascending id order by `record_of`, so `find` IS the
        // lowest-id body of the kind; `min_by_key` would say the same thing less plainly.
        if let Some(body) = record.bodies.iter().find(|b| b.kind == kind) {
            let (status, words) = ask(body.anchor.0, body.anchor.1);
            points.push((want, body.anchor.0, body.anchor.1, status, words));
        }
    }
    if let Some(body) = record.bodies.iter().find(|b| b.shore_member_count == 0) {
        let (status, words) = ask(body.anchor.0, body.anchor.1);
        points.push(("FineFound", body.anchor.0, body.anchor.1, status, words));
    }
    for reach in &record.reaches {
        if reach.points.len() < 3 {
            continue;
        }
        let point = &reach.points[reach.points.len() / 2];
        let (status, words) = ask(point.lat_deg, point.lon_deg);
        if status == WB_OK && words[0] == WATER_KIND_RIVER {
            points.push(("River", point.lat_deg, point.lon_deg, status, words));
            break;
        }
    }
    points
}

/// The lowest-id body with `shore_member_count == 0`, if the record holds one: the body the
/// `FineFound` point of [`water_points_from`] is chosen for, and the id its guard checks against.
fn fine_found_body_id(record: &hydrology::HydroRecord) -> Option<u32> {
    record.bodies.iter().find(|b| b.shore_member_count == 0).map(|b| b.id)
}

/// `wasm::WB_WATER_STRIDE`, restated for an example: an example cannot see a `pub(crate)` and this
/// number is Ruling Q-18's five, not Ruling Q-8's superseded four.
const WP_STRIDE: usize = 5;

/// `wasm::water_kind_code`'s own table, the two values this file names. They are the contract.
const WATER_KIND_NONE: f64 = 0.0;
const WATER_KIND_RIVER: f64 = 6.0;

/// The code `water_kind_code` gives the kind a `WP` point was chosen for, so the guard below can
/// compare the answer against the reason the point is in the corpus at all.
fn wanted_kind_code(want: &str) -> f64 {
    match want {
        "Lake" => 2.0,
        "SaltLake" => 3.0,
        "SaltFlat" => 4.0,
        "Pond" => 5.0,
        "River" => WATER_KIND_RIVER,
        other => panic!("no kind code for {other}"),
    }
}

/// Emits one `WP` line and the stderr report beside it. Ruling Q-21; see [`water_points_from`].
///
/// **This is where the corpus is refused.** Three guards, and each one is a failure mode that has
/// a name: a chosen point that no longer answers its kind (the bake moved under the corpus); a
/// `River` point whose `reach_id` is the sentinel (the reach branch stopped setting it, and the
/// word would be compared 1,024 times without ever being a reach); and an empty point list (a
/// record with no body and no reach, which is not a bake worth comparing). Each fails the dump
/// rather than writing a group that compares agreement it never tested.
fn print_water_points(name: &str, world: u32, params: &[f64])
    -> (Vec<&'static str>, Vec<(&'static str, f64, f64, u32, [f64; WP_STRIDE])>) {
    let mut bake: u32 = 0;
    let status = wb_hydro_bake(world, params.as_ptr(), params.len() as u32, &mut bake); // cast-ok: a small params buffer
    assert_eq!(status, WB_OK, "the {name} bake must succeed for the WP group");
    let len = wb_hydro_len(bake);
    let mut words = vec![0.0f64; len as usize]; // cast-ok: a freshly measured record length sizing its own buffer
    assert_eq!(wb_hydro_copy(bake, words.as_mut_ptr(), len), WB_OK);
    let record = hydrology::record::decode(&words)
        .expect("the record must decode -- this same binary just encoded it");

    let points = water_points_from(world, bake, &record);
    assert!(
        !points.is_empty(),
        "{name}: the record offered no body and no reach, so the WP group would compare nothing"
    );

    let mut covered = Vec::new();
    let mut fields: Vec<String> = Vec::new();
    for (want, lat, lon, point_status, answer) in &points {
        assert_eq!(
            *point_status, WB_OK,
            "{name}: the {want} point at {lat},{lon} was refused with status {point_status}"
        );
        if *want == "FineFound" {
            // Chosen for a BRANCH, not for a kind: `kind` on a fine-found body is whatever the
            // area classifier made it (`Lake` in both parity bakes), so the guard that means
            // something here is that the query still answers THAT BODY. If it stops, the detail
            // field it is read through (Ruling Q-16) has stopped reaching its own anchor, which is
            // precisely the failure this point exists to catch.
            let expected = fine_found_body_id(&record)
                .expect("a FineFound point exists only when a fine-found body does");
            assert_eq!(
                answer[3],
                f64::from(expected),
                "{name}: the fine-found point at {lat},{lon} answers body {} rather than body \
                 {expected} (kind code {}) -- the detail field no longer reaches that body's own \
                 anchor, and Ruling Q-16's branch would be on the wire in name only",
                answer[3],
                answer[0]
            );
        } else {
            assert_eq!(
                answer[0],
                wanted_kind_code(want),
                "{name}: the point chosen for {want} at {lat},{lon} now answers kind {} -- the \
                 bake has moved under this corpus, and a group whose points no longer cover the \
                 kinds they were chosen for proves nothing about those branches",
                answer[0]
            );
        }
        if *want == "River" {
            assert_ne!(
                answer[4],
                f64::from(u32::MAX),
                "{name}: the River point at {lat},{lon} answers River with NO_REACH -- the whole \
                 reason this point is in the corpus is that `reach_id` crosses the boundary as a \
                 real id (Ruling Q-18), and the sentinel would be compared without ever being one"
            );
        }
        covered.push(*want);
        fields.push(hex(*lat));
        fields.push(hex(*lon));
        fields.push(point_status.to_string());
        for word in answer.iter() {
            fields.push(hex(*word));
        }
    }
    println!("WP {name} {} {} {} {}", params.len(),
        params.iter().map(|v| hex(*v)).collect::<Vec<String>>().join(" "),
        points.len(),
        fields.join(" "));
    eprintln!(
        "WP {name}: {} explicit points covering {:?}; bodies {}, reaches {}",
        points.len(), covered, record.bodies.len(), record.reaches.len()
    );
    assert_eq!(wb_hydro_free(bake), WB_OK);
    (covered, points)
}

/// The native prediction for `--mutate tectonic-warp` on a `WP` group: how many of its
/// `count * (1 + WP_STRIDE)` values move when the same points, with the same params, are asked of
/// a bake on `world` instead.
///
/// **Why this record needed one and `water_at/plain` did not.** `margin_warp_m` reaches the
/// terrain the `ranges` bake runs over -- that is the whole reason `hydro/ranges` moves 16,807
/// words under this control -- so a query on that world must move too, and `parity.mjs` requires
/// every group's movement to equal a number the native side computed rather than a number the run
/// produced. The `plain` groups are on a world with no tectonic block, so their prediction is the
/// zero every unlisted group already gets.
///
/// The points are **replayed, not re-chosen**: the same latitudes and longitudes the corpus
/// records, exactly as the replaying side uses them. Re-choosing from the warp-0 record would
/// compare two different questions and call the difference a divergence.
fn water_points_divergence(
    world: u32,
    params: &[f64],
    recorded: &[(&'static str, f64, f64, u32, [f64; WP_STRIDE])],
) -> usize {
    let mut bake: u32 = 0;
    let status = wb_hydro_bake(world, params.as_ptr(), params.len() as u32, &mut bake); // cast-ok: a small params buffer
    if status != WB_OK {
        // The whole group counts as moved: there is no record to answer from, which is itself a
        // divergence from a corpus recorded off a bake that succeeded.
        return recorded.len() * (1 + WP_STRIDE);
    }
    let mut moved = 0usize;
    for (_, lat, lon, point_status, answer) in recorded {
        let mut out = [0.0f64; WP_STRIDE];
        let got = wb_water_at(world, bake, *lat, *lon, out.as_mut_ptr(), WP_STRIDE as u32); // cast-ok: a compile-time stride of five
        if got != *point_status {
            moved += 1;
        }
        for word in 0..WP_STRIDE {
            // Bit equality, as the replaying side compares: two NaNs of different payloads are
            // different words here, and -0.0 is not 0.0.
            if got != WB_OK || out[word].to_bits() != answer[word].to_bits() {
                moved += 1;
            }
        }
    }
    assert_eq!(wb_hydro_free(bake), WB_OK);
    moved
}

/// I6 (final review ruling, rule (a)): the divergence between a recorded hydro record
/// (`status_on`/`len`/`words_on`) and a freshly measured one under a control
/// (`status_off`/`n`/`words_off`). One tally for the status, one for the length equality, and
/// then bit-for-bit words `i < min(n, len)`; a recorded word at `i >= n` counts as divergent
/// without reading anything at that index, since `words_off` never held that many words in the
/// first place -- this is the native prediction the `--mutate tectonic-warp` control replays.
fn divergent_count(
    status_on: u32,
    len: u32,
    words_on: &[f64],
    status_off: u32,
    n: u32,
    words_off: &[f64],
) -> usize {
    let mut divergent = 0usize;
    if status_on != status_off {
        divergent += 1;
    }
    if len != n {
        divergent += 1;
    }
    let len = len as usize; // cast-ok: a hydro record length, already used to size a Vec above
    let n = n as usize; // cast-ok: as above
    for i in 0..len {
        if i < n {
            if words_on[i].to_bits() != words_off[i].to_bits() {
                divergent += 1;
            }
        } else {
            divergent += 1;
        }
    }
    divergent
}

// --- plan 2b Task 7: the carve across the boundary ------------------------------------------

/// Points per category in a `WC` group: the lowest-id items of each kind that qualify.
const WC_PER_CATEGORY: usize = 6;

/// The five things a carve point can be chosen for, in the order the dump reports them.
const WC_CATEGORIES: [&str; 5] = ["channel", "bank", "body", "notch", "clear"];

/// What a `WC` group is built from on the native side: the world's own arguments, the bake's
/// params (the thirteen-word-or-longer layout, word 12 = 1), and the water block.
#[derive(Clone, Copy)]
struct CarveSpec<'a> {
    name: &'static str,
    /// The handle the bake runs on -- the bare world these same arguments build.
    base: u32,
    tectonic: Option<&'a [f64; WB_TECTONIC_STRIDE]>,
    params: &'a [f64],
    block: [f64; WB_WATER_BLOCK_STRIDE],
}

/// One chosen point: its category, where it is, the carved elevation the door answers, the bare
/// parent's, and -- for a body point -- how far the channel through it would have cut had the
/// lake-bed rule not held (`ground - bed`, positive by the guard).
struct CarvePoint {
    category: &'static str,
    lat: f64,
    lon: f64,
    carved: f64,
    bare: f64,
    would_cut: f64,
}

/// `wb_water_check` and `wb_world_new_water` for a spec over a held bake: the named status, and
/// the handle (0 on refusal). Both halves asked, as the viewer's carve session does.
fn carve_door(spec: &CarveSpec, tectonic: Option<&[f64; WB_TECTONIC_STRIDE]>, bake: u32) -> (u32, u32) {
    let null = core::ptr::null();
    let (t_ptr, t_len) = match tectonic {
        Some(block) => (block.as_ptr(), WB_TECTONIC_STRIDE as u32), // cast-ok: a compile-time sixteen-word stride
        None => (null, 0),
    };
    let block_len = WB_WATER_BLOCK_STRIDE as u32; // cast-ok: a compile-time one-word stride
    let status = wb_water_check(SEED, RADIUS_M, PLATES, LAND, null, 0, null, 0, t_ptr, t_len,
                                null, 0, null, 0, null, 0, spec.block.as_ptr(), block_len, bake);
    let handle = wb_world_new_water(SEED, RADIUS_M, PLATES, LAND, null, 0, null, 0, t_ptr, t_len,
                                    null, 0, null, 0, null, 0, spec.block.as_ptr(), block_len, bake);
    assert_eq!(status == WB_OK, handle != 0, "{}: the checker said {status}, the door {handle}", spec.name);
    (status, handle)
}

/// The canonical water block, read from `wb_water_preset` rather than written here, so no number
/// in this file is a transcription of `water/layer.rs`'s own.
fn water_preset_block() -> [f64; WB_WATER_BLOCK_STRIDE] {
    let mut block = [0.0f64; WB_WATER_BLOCK_STRIDE];
    let len = WB_WATER_BLOCK_STRIDE as u32; // cast-ok: a compile-time one-word stride
    assert_eq!(wb_water_preset(WB_WATER_CANONICAL, block.as_mut_ptr(), len), WB_OK);
    block
}

/// The larger of two widths, written as the comparison (no `f64::max`: NaN-asymmetric).
fn wider(a: f64, b: f64) -> f64 {
    if a > b { a } else { b }
}

/// **The carve's parity group, chosen from the record and REFUSED if it stops testing the carve.**
///
/// Five categories, each the lowest-id items of its kind at recorded geometry (Ruling Q-21's rule,
/// applied to the carve), and each held to what it was chosen for by the layer itself -- asked
/// natively, beside the door -- so a point that has drifted into open country fails here rather
/// than comparing a bare elevation and calling it a carve:
///
/// - **channel**: the middle recorded point of each lowest-id reach with three or more points,
///   where the query answers `River`, the layer's authority is exactly 1, and the carved ground is
///   **that point's own `bed_m`, bit for bit**, below the bare ground.
/// - **bank**: on the same reaches, the point one channel width off the middle leg's midpoint,
///   perpendicular to it -- half a width into the canonical bank -- where the layer's authority is
///   strictly between 0 and 1, the query answers dry ground, and the carve moved the ground.
/// - **body**: the lowest-id bodies with a recorded reach point inside them (the query answers
///   that body there) whose bed stands **below** the landform -- a point the channel WOULD cut but
///   for spec §8.1's lake-bed rule -- and the carved ground is the bare ground, bit for bit.
/// - **notch**: the middle point of each lowest-id notch, outside every body, where the carve
///   lowered the ground.
/// - **clear**: a fixed scatter's first land points that the query calls dry and the index offers
///   no reach or notch at all -- the carved ground must be the bare ground, bit for bit.
///
/// A category with no qualifying point fails the dump. Every carved value is also recomputed
/// through the library's own `Surface::with_water` over the same decoded record, and the two
/// must agree -- a second derivation, as `TCTL`'s counts are.
fn carve_points(spec: &CarveSpec, lib_tectonics: Option<TectonicParams>) -> (u32, Vec<CarvePoint>) {
    use std::sync::Arc;
    use worldbuilder_engine::water::layer::{Carve, IndexedRecord, WaterLayer, WaterParams};

    let (bake_status, bake_len, bake_words) = bake_hydro_native(spec.base, spec.params);
    assert_eq!(bake_status, WB_OK, "{}: the bake for carving must succeed", spec.name);
    assert!(bake_len > 0);
    let record = hydrology::record::decode(&bake_words).expect("the record must decode");
    assert!(record.stats.drained_for_carve, "{}: the WC bake must be baked for carving", spec.name);

    let mut bake: u32 = 0;
    let len = spec.params.len() as u32; // cast-ok: a small params buffer
    assert_eq!(wb_hydro_bake(spec.base, spec.params.as_ptr(), len, &mut bake), WB_OK);
    let (status, carved) = carve_door(spec, spec.tectonic, bake);
    assert_eq!(status, WB_OK, "{}: the door must carve this world with its own bake", spec.name);

    // The library's side: the bare world, the layer over the same record, and the carved world.
    let indexed = Arc::new(IndexedRecord::new(record.clone(), RADIUS_M));
    let params = WaterParams { bank_widths: spec.block[0] };
    let layer = WaterLayer::new(params, Arc::clone(&indexed));
    let plates = PLATES as usize; // cast-ok: a corpus-fixed plate count widened to usize
    let bare_lib = Surface::new(SEED, RADIUS_M, plates, LAND, None, None, lib_tectonics);
    let carved_lib = Surface::with_water(SEED, RADIUS_M, plates, LAND, None, None, lib_tectonics,
                                         None, None, None,
                                         Some(Carve { params, bake: Arc::clone(&indexed) }))
        .expect("the library joins what the door joined");
    let cell_m = record.stats.pond_cell_m;
    // `(cut, authority)` exactly as `Surface::elevation_m` asks it: the landform, and the bare
    // detail field at the record's pond cell.
    let layer_at = |p: &SpherePoint| -> (f64, f64) {
        let bare = |q: &SpherePoint| bare_lib.bake_ground_m(q, Some(cell_m));
        layer.cut_with(p, bare_lib.structural_m(p), &worldbuilder_engine::water::Detail(&bare))
    };
    let query = |lat: f64, lon: f64| -> [f64; WP_STRIDE] {
        let mut out = [0.0f64; WP_STRIDE];
        let got = wb_water_at(spec.base, bake, lat, lon, out.as_mut_ptr(), WP_STRIDE as u32); // cast-ok: a compile-time stride of five
        assert_eq!(got, WB_OK, "{}: the query refused {lat},{lon}", spec.name);
        out
    };
    let sample = |lat: f64, lon: f64| -> (f64, f64) {
        let carved_m = wb_elevation_m(carved, lat, lon, RES_M);
        let point = SpherePoint::from_latlon(lat, lon);
        assert_eq!(carved_m.to_bits(), carved_lib.elevation_m(&point, Some(RES_M)).to_bits(),
                   "{}: the door and the library carve {lat},{lon} differently", spec.name);
        (carved_m, wb_elevation_m(spec.base, lat, lon, RES_M))
    };
    let no_body = f64::from(u32::MAX);

    let mut points: Vec<CarvePoint> = Vec::new();
    let take = |points: &mut Vec<CarvePoint>, category: &'static str, lat: f64, lon: f64, would_cut: f64| {
        let (carved_m, bare_m) = sample(lat, lon);
        points.push(CarvePoint { category, lat, lon, carved: carved_m, bare: bare_m, would_cut });
    };
    let count = |points: &Vec<CarvePoint>, category: &str| points.iter().filter(|p| p.category == category).count();

    // channel and bank, off the same lowest-id reaches.
    for reach in &record.reaches {
        if count(&points, "channel") >= WC_PER_CATEGORY && count(&points, "bank") >= WC_PER_CATEGORY {
            break;
        }
        if reach.points.len() < 3 {
            continue;
        }
        let k = reach.points.len() / 2;
        let mid = &reach.points[k];
        if count(&points, "channel") < WC_PER_CATEGORY {
            let answer = query(mid.lat_deg, mid.lon_deg);
            let (_, authority) = layer_at(&SpherePoint::from_latlon(mid.lat_deg, mid.lon_deg));
            let (carved_m, bare_m) = sample(mid.lat_deg, mid.lon_deg);
            if answer[0] == WATER_KIND_RIVER && authority == 1.0
                && carved_m.to_bits() == mid.bed_m.to_bits() && carved_m < bare_m {
                take(&mut points, "channel", mid.lat_deg, mid.lon_deg, 0.0);
            }
        }
        if count(&points, "bank") < WC_PER_CATEGORY {
            let next = &reach.points[k + 1];
            let a = SpherePoint::from_latlon(mid.lat_deg, mid.lon_deg);
            let b = SpherePoint::from_latlon(next.lat_deg, next.lon_deg);
            let (Some(centre), Some(normal)) =
                (SpherePoint::from_vector(&a.vector.add(&b.vector)), a.vector.cross(&b.vector).normalised())
            else {
                continue;
            };
            // Half a width into a bank one width wide: the channel's half-width plus half a bank.
            let width_m = wider(mid.width_m, next.width_m);
            let off_m = 0.5 * width_m + 0.5 * spec.block[0] * width_m;
            for side in [1.0, -1.0] {
                let Some(p) = SpherePoint::from_vector(&centre.vector.add(&normal.scaled(side * off_m / RADIUS_M))) else {
                    continue;
                };
                let (lat, lon) = p.to_latlon();
                let at = SpherePoint::from_latlon(lat, lon);
                let (cut, authority) = layer_at(&at);
                let answer = query(lat, lon);
                let (carved_m, bare_m) = sample(lat, lon);
                // The layer must have LOWERED the landform here (`cut < structural`), and the
                // carve must have moved the ground the door answers. Not "carved below bare":
                // on a bank detail is damped by `1 - authority`, not removed, so where the bare
                // world's roughness dips, the blended bank can stand above it (measured: it does,
                // at some bank points of both worlds).
                let landform = bare_lib.structural_m(&at);
                if authority > 0.0 && authority < 1.0 && answer[0] == WATER_KIND_NONE
                    && cut < landform && carved_m.to_bits() != bare_m.to_bits() {
                    take(&mut points, "bank", lat, lon, 0.0);
                    break;
                }
            }
        }
    }

    // body: one pass over every recorded reach point, the first qualifying point per body.
    let mut body_first: Vec<(u32, f64, f64, f64)> = Vec::new();
    for reach in &record.reaches {
        for rp in &reach.points {
            let answer = query(rp.lat_deg, rp.lon_deg);
            if answer[3] == no_body {
                continue;
            }
            let body_id = answer[3] as u32; // cast-ok: a body id the query wrote from a u32
            if body_first.iter().any(|(id, ..)| *id == body_id) {
                continue;
            }
            let landform = bare_lib.structural_m(&SpherePoint::from_latlon(rp.lat_deg, rp.lon_deg));
            if rp.bed_m < landform && rp.width_m > 0.0 {
                body_first.push((body_id, rp.lat_deg, rp.lon_deg, landform - rp.bed_m));
            }
        }
    }
    body_first.sort_by_key(|(id, ..)| *id);
    for (_, lat, lon, would_cut) in body_first.into_iter().take(WC_PER_CATEGORY) {
        take(&mut points, "body", lat, lon, would_cut);
    }

    // notch: the middle point of each lowest-position notch, outside every body, lowered.
    let mut notch_at_surface = 0usize;
    for notch in &record.notches {
        if count(&points, "notch") >= WC_PER_CATEGORY {
            break;
        }
        if notch.points.is_empty() {
            continue;
        }
        let (lat, lon, surface_m, _) = notch.points[notch.points.len() / 2];
        if query(lat, lon)[3] != no_body {
            continue;
        }
        let (carved_m, bare_m) = sample(lat, lon);
        if carved_m < bare_m {
            take(&mut points, "notch", lat, lon, 0.0);
            if carved_m.to_bits() == surface_m.to_bits() {
                notch_at_surface += 1;
            }
        }
    }
    // Reported, not required: a notch sits on a recorded river or an outlet cut (Ruling 12b-2),
    // so a reach bed below the notch's own surface can be the deeper cut there -- the layer takes
    // the lowest -- and the point is still in the notch's channel either way.
    eprintln!("WC {}: {notch_at_surface} of the notch points carved to exactly the notch's own \
               surface_m (the rest to a deeper reach bed through the same cut)", spec.name);

    // clear: a fixed scatter, its own generator so no other group's points move.
    let mut rng = Rng(0x5EED_2B_CA4E_0007);
    let mut tries = 0usize;
    while count(&points, "clear") < WC_PER_CATEGORY && tries < 100_000 {
        tries += 1;
        let lat = rng.unit() * 180.0 - 90.0;
        let lon = rng.unit() * 360.0 - 180.0;
        let p = SpherePoint::from_latlon(lat, lon);
        let candidates = indexed.index().candidates(&p);
        if !candidates.reaches.is_empty() || !candidates.notches.is_empty() {
            continue;
        }
        if wb_elevation_m(spec.base, lat, lon, RES_M) <= 0.0 || query(lat, lon)[0] != WATER_KIND_NONE {
            continue;
        }
        take(&mut points, "clear", lat, lon, 0.0);
    }

    // THE GUARD. Every category covered, and every point doing what it was chosen for.
    for category in WC_CATEGORIES {
        assert!(count(&points, category) > 0,
                "{}: no {category} point qualifies -- a carve group without it would compare \
                 elevations that never tested that part of the carve", spec.name);
    }
    for p in &points {
        match p.category {
            "body" | "clear" => assert_eq!(p.carved.to_bits(), p.bare.to_bits(),
                "{}: the {} point at {},{} was cut ({} -> {})", spec.name, p.category, p.lat, p.lon, p.bare, p.carved),
            "bank" => assert_ne!(p.carved.to_bits(), p.bare.to_bits(),
                "{}: the bank point at {},{} was not moved by the carve", spec.name, p.lat, p.lon),
            _ => assert!(p.carved < p.bare,
                "{}: the {} point at {},{} was not lowered", spec.name, p.category, p.lat, p.lon),
        }
    }

    assert_eq!(wb_world_free(carved), WB_OK);
    assert_eq!(wb_hydro_free(bake), WB_OK);
    (status, points)
}

/// Emit one `WC` line and its stderr report.
fn print_carve(spec: &CarveSpec, status: u32, points: &[CarvePoint]) {
    let tectonic: Vec<String> = spec.tectonic.map(|t| t.iter().map(|v| hex(*v)).collect()).unwrap_or_default();
    let params: Vec<String> = spec.params.iter().map(|v| hex(*v)).collect();
    let block: Vec<String> = spec.block.iter().map(|v| hex(*v)).collect();
    let mut fields: Vec<String> = Vec::new();
    for p in points {
        fields.push(hex(p.lat));
        fields.push(hex(p.lon));
        fields.push(hex(p.carved));
    }
    // WC <name> <seed> <radius> <plates> <land> <tlen> <t...> <plen> <p...> <blen> <b...>
    //    <res> <status> <count> [<lat> <lon> <carved elevation>] x count
    //
    // Built as one list of fields and joined once, so an empty tectonic block (the `plain` world)
    // is zero fields rather than an empty string between two spaces.
    let mut line: Vec<String> = vec!["WC".into(), spec.name.into(), SEED.to_string(), hex(RADIUS_M),
                                     PLATES.to_string(), hex(LAND), tectonic.len().to_string()];
    line.extend(tectonic);
    line.push(params.len().to_string());
    line.extend(params);
    line.push(block.len().to_string());
    line.extend(block);
    line.push(hex(RES_M));
    line.push(status.to_string());
    line.push(points.len().to_string());
    line.extend(fields);
    println!("{}", line.join(" "));
    let order: Vec<&str> = points.iter().map(|p| p.category).collect();
    eprintln!("WC {}: point order {order:?}", spec.name);
    for category in WC_CATEGORIES {
        let chosen: Vec<&CarvePoint> = points.iter().filter(|p| p.category == category).collect();
        let depths: Vec<f64> = chosen.iter().map(|p| p.bare - p.carved).collect();
        let deepest = depths.iter().fold(0.0f64, |a, d| if *d > a { *d } else { a });
        let shallowest = depths.iter().fold(deepest, |a, d| if *d < a { *d } else { a });
        let would = chosen.iter().map(|p| p.would_cut).fold(0.0f64, |a, d| if d > a { d } else { a });
        eprintln!(
            "WC {}: {category} {} points, cut (bare - carved) {shallowest:.6} .. {deepest:.6} m{}",
            spec.name, chosen.len(),
            if category == "body" {
                format!("; deepest cut the lake-bed rule refused (landform - bed): {would:.6} m")
            } else {
                String::new()
            }
        );
    }
}

/// The native prediction for `--mutate tectonic-warp` on a `WC` group: the same points, asked
/// of the same door over a bake of the warp-0 world, counted as the replaying side counts them
/// -- one for the checker's status, one per point.
fn carve_divergence(spec: &CarveSpec, control_world: u32, control_tectonic: &[f64; WB_TECTONIC_STRIDE],
                    status: u32, points: &[CarvePoint]) -> usize {
    let mut bake: u32 = 0;
    let len = spec.params.len() as u32; // cast-ok: a small params buffer
    if wb_hydro_bake(control_world, spec.params.as_ptr(), len, &mut bake) != WB_OK {
        return 1 + points.len();
    }
    let (got, handle) = carve_door(spec, Some(control_tectonic), bake);
    let mut moved = usize::from(got != status);
    let mut by_category = [0usize; WC_CATEGORIES.len()];
    for p in points {
        if handle == 0 || wb_elevation_m(handle, p.lat, p.lon, RES_M).to_bits() != p.carved.to_bits() {
            moved += 1;
            if let Some(slot) = WC_CATEGORIES.iter().position(|c| *c == p.category) {
                by_category[slot] += 1;
            }
        }
    }
    eprintln!("WC {}: under the warp control, moved per category {:?} = {by_category:?}, status {}",
              spec.name, WC_CATEGORIES, if got == status { "unmoved" } else { "moved" });
    if handle != 0 {
        assert_eq!(wb_world_free(handle), WB_OK);
    }
    assert_eq!(wb_hydro_free(bake), WB_OK);
    moved
}

/// **The drain, confirmed to be the only difference between the two records** (plan 2b Task 7,
/// fix round). `ordinary` and `carving` are the same world baked with the same params, the second
/// with `drain_for_carve` set. Ruling C-20 says they may differ in exactly three places: word 0
/// (`SCHEMA` 7 against `SCHEMA_CARVE` 8), the fine-found bodies the drain drops -- with a pond
/// that the dropped one's density cell was holding back free to take its place -- and the
/// header's `ponds_kept`, which counts them. Everything else must be equal: every reach, notch
/// and fall, the ground fingerprint, every coarse body with its id, every other stat, and every
/// fine-found body the drain keeps, in the same order.
///
/// **"The drain says" is re-derived, not read off the diff.** Each fine-found body of the ordinary
/// record is put to `hydrology::ponds::drain_deficit_m` against the carving record's own channels,
/// at the bake's own step (`pond_cell_m / 4`) and tolerance (`refine_vertical_m`) -- which is
/// exactly `ponds::is_drained` -- and the set that test drops must be exactly the set that is
/// missing. The channel index is built at `water::index::DEFAULT_CELL_M` rather than the bake's
/// private 200 km: the index's footprint guarantee holds at any cell (`ponds.rs`,
/// `DRAIN_INDEX_CELL_M`'s own doc), so the cell changes the candidates offered, never the answer.
///
/// Refuses the corpus when the drain dropped nothing: a comparison of two records that are equal
/// but for word 0 would show the carving record's words cross the boundary and prove nothing about
/// the drain. Returns (dropped, added, kept) fine-found counts.
fn check_drain(name: &str, ordinary: &[f64], carving: &[f64]) -> (usize, usize, usize) {
    use worldbuilder_engine::hydrology::record::{SCHEMA, SCHEMA_CARVE};
    use worldbuilder_engine::water::index::{WaterIndex, DEFAULT_CELL_M};

    assert_eq!(ordinary[0].to_bits(), SCHEMA.to_bits(), "{name}: the ordinary record's word 0");
    assert_eq!(carving[0].to_bits(), SCHEMA_CARVE.to_bits(), "{name}: the carving record's word 0");
    let o = hydrology::record::decode(ordinary).expect("the ordinary record decodes");
    let c = hydrology::record::decode(carving).expect("the carving record decodes");
    assert!(!o.stats.drained_for_carve && c.stats.drained_for_carve, "{name}: the flags");
    assert!(o.reaches == c.reaches, "{name}: the drain moved a reach");
    assert!(o.notches == c.notches, "{name}: the drain moved a notch");
    assert!(o.falls == c.falls, "{name}: the drain moved a fall");
    assert_eq!(o.ground, c.ground, "{name}: the drain moved the ground fingerprint");

    let coarse = |r: &hydrology::HydroRecord| -> Vec<hydrology::Body> {
        r.bodies.iter().filter(|b| b.shore_member_count != 0).cloned().collect()
    };
    assert!(coarse(&o) == coarse(&c), "{name}: the drain moved a coarse body");
    // A fine-found body with its id set aside: ids are handed out in keep order, so a dropped
    // pond renumbers every find kept after it, and that renumbering is the drain's too.
    let key = |b: &hydrology::Body| hydrology::Body { id: 0, ..b.clone() };
    let fine_o: Vec<hydrology::Body> = o.bodies.iter().filter(|b| b.shore_member_count == 0).map(key).collect();
    let fine_c: Vec<hydrology::Body> = c.bodies.iter().filter(|b| b.shore_member_count == 0).map(key).collect();

    let index = WaterIndex::build(&c, RADIUS_M, DEFAULT_CELL_M);
    let step_m = c.stats.pond_cell_m * 0.25;
    let drained = |b: &hydrology::Body| {
        match hydrology::ponds::drain_deficit_m(b, &c, &index, step_m) {
            Some(deficit) => deficit > c.stats.refine_vertical_m,
            None => false,
        }
    };
    let mut kept_in_order: Vec<&hydrology::Body> = Vec::new();
    let mut dropped = 0usize;
    for body in &fine_o {
        if drained(body) {
            dropped += 1;
            assert!(!fine_c.contains(body),
                    "{name}: a pond the drain drops at {:?} is still in the carving record", body.anchor);
        } else {
            kept_in_order.push(body);
        }
    }
    // Every pond the drain keeps is in the carving record, in the same relative order.
    let mut cursor = 0usize;
    for body in &kept_in_order {
        let found = fine_c[cursor..].iter().position(|b| b == *body);
        let Some(at) = found else {
            panic!("{name}: the pond at {:?} is not drained and not in the carving record, or not in \
                    the ordinary record's order", body.anchor);
        };
        cursor += at + 1;
    }
    // Anything else in the carving record is a find the ordinary bake's density cell turned away,
    // taken because a drained pond freed that cell -- so it must not be drained itself.
    let added: Vec<&hydrology::Body> = fine_c.iter().filter(|b| !fine_o.contains(b)).collect();
    for body in &added {
        assert!(!drained(body), "{name}: the carving record keeps a drained pond at {:?}", body.anchor);
    }
    assert_eq!(fine_c.len(), kept_in_order.len() + added.len(), "{name}: fine-found accounting");

    let mut stats = c.stats.clone();
    stats.drained_for_carve = false;
    stats.ponds_kept = o.stats.ponds_kept;
    assert!(stats == o.stats, "{name}: the drain moved a header field other than ponds_kept");
    assert_eq!(c.stats.ponds_kept as usize, o.stats.ponds_kept as usize - dropped + added.len(), // cast-ok: two pond counts widened to usize
               "{name}: ponds_kept does not count the drain");
    assert!(dropped > 0, "{name}: the drain dropped nothing, so the carving record would prove nothing \
                          about it beyond word 0");
    eprintln!(
        "hydro_carve/{name}: {} words against {} ordinary; drain dropped {dropped} of {} fine-found \
         bodies, {} other finds took freed cells, {} kept; ponds_kept {} -> {}; word 0 {} -> {}; \
         reaches, notches, falls, ground, coarse bodies and every other header field equal",
        carving.len(), ordinary.len(), fine_o.len(), added.len(), kept_in_order.len(),
        o.stats.ponds_kept, c.stats.ponds_kept, ordinary[0], carving[0]
    );
    (dropped, added.len(), kept_in_order.len())
}

/// Emit one `HC` record: the carving bake, word for word, in `H`'s own layout.
fn print_carving_record(name: &str, params: &[f64], status: u32, len: u32, words: &[f64]) {
    let params_hex: Vec<String> = params.iter().map(|v| hex(*v)).collect();
    let words_hex: Vec<String> = words.iter().map(|v| hex(*v)).collect();
    println!("HC {name} {} {} {status} {len} {}", params.len(), params_hex.join(" "), words_hex.join(" "));
}

/// The native prediction for `--mutate carve-bank` on a `WC` group: the same door over the same
/// held bake with `bank_widths` moved to `mutated`, and the recorded points asked again. Returns
/// the count the replaying side will see -- the checker's status and each point -- and asserts
/// the prediction's SHAPE: the bank width reaches a point only through the blended bank, so the
/// points that move must be exactly the bank points. A channel point is at full authority at any
/// bank width, and body and clear points are cut by nothing; a notch point sits on its line.
fn carve_bank_divergence(spec: &CarveSpec, mutated: f64, status: u32, points: &[CarvePoint]) -> usize {
    let mut bake: u32 = 0;
    let len = spec.params.len() as u32; // cast-ok: a small params buffer
    assert_eq!(wb_hydro_bake(spec.base, spec.params.as_ptr(), len, &mut bake), WB_OK);
    let wider_spec = CarveSpec { block: [mutated], ..*spec };
    let (got, handle) = carve_door(&wider_spec, spec.tectonic, bake);
    assert_ne!(handle, 0, "{}: the carve-bank control's block must be admissible", spec.name);
    let mut moved = usize::from(got != status);
    for p in points {
        let changed = wb_elevation_m(handle, p.lat, p.lon, RES_M).to_bits() != p.carved.to_bits();
        assert_eq!(changed, p.category == "bank",
                   "{}: under bank_widths {mutated} the {} point at {},{} {} -- the bank width must move \
                    exactly the bank points", spec.name, p.category, p.lat, p.lon,
                   if changed { "moved" } else { "did not move" });
        if changed {
            moved += 1;
        }
    }
    assert_eq!(wb_world_free(handle), WB_OK);
    assert_eq!(wb_hydro_free(bake), WB_OK);
    moved
}

/// The bank width `--mutate carve-bank` substitutes for the canonical block's: twice it, inside
/// the admissible (0, 4] -- a plausible block a host could send, not a refused one.
const CARVE_BANK_CONTROL: f64 = 2.0;

fn main() {
    // Two worlds: open water, and the placed harbour. A scattered corpus never lands
    // inside a feature, and that gap has survived every earlier probe in this project.
    println!("world plain {SEED} {} {PLATES} {}", hex(RADIUS_M), hex(LAND));
    let records = harbour_records();
    let encoded: Vec<String> = records.iter().map(|v| hex(*v)).collect();
    println!("world harbour {SEED} {} {PLATES} {} {}", hex(RADIUS_M), hex(LAND), encoded.join(" "));

    let plain = wb_world_new(SEED, RADIUS_M, PLATES, LAND, core::ptr::null(), 0);
    let harbour = wb_world_new(SEED, RADIUS_M, PLATES, LAND, records.as_ptr(), 2);
    assert!(plain != 0 && harbour != 0, "both worlds must build");

    // --- scattered, open water: 10,000 points, elevation and structural each
    let mut rng = Rng(0x5EED_2B_0000_0001);
    for _ in 0..10_000 {
        let latitude_deg = rng.unit() * 180.0 - 90.0;
        let longitude_deg = rng.unit() * 360.0 - 180.0;
        println!(
            "E plain {} {} {} {}",
            hex(latitude_deg),
            hex(longitude_deg),
            hex(RES_M),
            hex(wb_elevation_m(plain, latitude_deg, longitude_deg, RES_M))
        );
        println!(
            "S plain {} {} {}",
            hex(latitude_deg),
            hex(longitude_deg),
            hex(wb_structural_m(plain, latitude_deg, longitude_deg))
        );
    }

    // --- inside the placed harbour: 10,000 points within +/-0.01 deg of it
    for _ in 0..10_000 {
        let latitude_deg = HARBOUR_LAT + (rng.unit() - 0.5) * 0.02;
        let longitude_deg = HARBOUR_LON + (rng.unit() - 0.5) * 0.02;
        println!(
            "E harbour {} {} {} {}",
            hex(latitude_deg),
            hex(longitude_deg),
            hex(RES_M),
            hex(wb_elevation_m(harbour, latitude_deg, longitude_deg, RES_M))
        );
        println!(
            "S harbour {} {} {}",
            hex(latitude_deg),
            hex(longitude_deg),
            hex(wb_structural_m(harbour, latitude_deg, longitude_deg))
        );
    }

    // --- the resolution sentinel, across the boundary: 200 harbour points x 4 sentinels
    for _ in 0..200 {
        let latitude_deg = HARBOUR_LAT + (rng.unit() - 0.5) * 0.02;
        let longitude_deg = HARBOUR_LON + (rng.unit() - 0.5) * 0.02;
        for sentinel in [0.0f64, -1.0, f64::INFINITY, f64::NAN] {
            println!(
                "E harbour {} {} {} {}",
                hex(latitude_deg),
                hex(longitude_deg),
                hex(sentinel),
                hex(wb_elevation_m(harbour, latitude_deg, longitude_deg, sentinel))
            );
        }
    }

    // --- the inspection tap: 500 points in each world, three fractions each
    for (name, handle, lat_c, lon_c, span) in
        [("plain", plain, 12.0, 34.0, 4.0), ("harbour", harbour, HARBOUR_LAT, HARBOUR_LON, 0.02)]
    {
        for _ in 0..500 {
            let latitude_deg = lat_c + (rng.unit() - 0.5) * span;
            let longitude_deg = lon_c + (rng.unit() - 0.5) * span;
            let mut out = [0.0f64; 3];
            let status = wb_bottom_at(handle, latitude_deg, longitude_deg, out.as_mut_ptr());
            println!(
                "B {name} {} {} {status} {} {} {}",
                hex(latitude_deg),
                hex(longitude_deg),
                hex(out[0]),
                hex(out[1]),
                hex(out[2])
            );
        }
    }

    // --- tiles, because a scalar corpus cannot see the grid: 65x65 in each world
    for (name, handle) in [("plain", plain), ("harbour", harbour)] {
        let (lat0, lat1) = (HARBOUR_LAT + 0.005, HARBOUR_LAT - 0.005);
        let (lon0, lon1) = (HARBOUR_LON - 0.005, HARBOUR_LON + 0.005);
        let (width, height) = (65u32, 65u32);
        let mut tile = vec![0.0f32; 65 * 65];
        let status = wb_fill_tile_f32(
            handle,
            lat0,
            lat1,
            lon0,
            lon1,
            width,
            height,
            RES_M,
            tile.as_mut_ptr(),
            width * height,
        );
        assert_eq!(status, WB_OK, "{name}: the tile must fill");
        let cells: Vec<String> = tile.iter().map(|v| hex32(*v)).collect();
        println!(
            "T {name} {} {} {} {} {width} {height} {} {}",
            hex(lat0),
            hex(lat1),
            hex(lon0),
            hex(lon1),
            hex(RES_M),
            cells.join(" ")
        );
    }

    // --- climate: the two new doors onto climate.rs -----------------------------------
    //
    // Slice `2026-09-06-slice-climate`, Task 4. `wb_climate_tile_f32` and
    // `wb_climate_calibration` are the only exports that reach `climate.rs`, so before they
    // existed that module's native/WASM agreement was **unfalsifiable** in exactly the sense
    // this file's own module doc gives for `wb_erosion_run` and `wb_water_run`.
    //
    // What is genuinely new in the arithmetic, rather than more of what is already compared:
    // the march is an accumulation over up to `march_samples` `elevation_m` probes taken
    // along a `TangentFrame`, so it compounds `detmath::exp` (never previously crossed at
    // all) over a path -- a place where a single-ULP disagreement would grow rather than
    // stay put. The calibration adds a **sort and an order statistic over a 4,000-point
    // Fibonacci spiral**, whose answer depends on the ordering of values that a one-ULP
    // difference could swap.
    //
    // **The control is `--mutate climate-samples`, and it is a control with a shape rather
    // than a size**: one more upwind step changes the rain-out integral and cannot change a
    // closed-form temperature or a quantile of elevation. So it must move every
    // `climate-moist/*` group and leave every `climate-temp/*` and `climate-land/*` group at
    // exactly zero. A control that moved all three would be indistinguishable from a
    // harness bug; that prediction is asserted in `parity.mjs`, not observed.
    //
    // The budgets here are deliberately small (0, 8, 24) except for one canonical-width
    // 16x16 tile: the cost is samples x budget and the replaying side runs in WASM at ~3x
    // native. 16x16 is also the raster the viewer ships, so the compared grid is the grid
    // that is drawn.
    for (name, handle) in [("plain", plain), ("harbour", harbour)] {
        // **The window is over a continent, and that is a finding rather than a preference.**
        // The obvious choice -- the harbour, which every other section of this corpus uses --
        // produced 640 f32 all equal to `3f800000`: the whole 3,200 km upwind path is open
        // water there, so the march recharges to exactly 1.0 and STAYS there whatever the
        // budget is. The first run of `--mutate climate-samples` therefore moved 8 values of
        // 648, all of them calibration edges, and the tile records compared a constant to
        // itself. That is this project's "an assertion that looked load-bearing and was not"
        // arriving in a parity corpus.
        //
        // 6 N to 10 S, 40 E to 56 E is this world's largest dry interior: a global 72 x 36
        // scan through this same export puts its driest land cells at moisture 0.009 against
        // 1.0 offshore, so the window spans nearly the whole range the field has. The
        // assertions below hold the corpus to that rather than trusting this comment.
        let (lat0, lat1) = (6.0, -10.0);
        let (lon0, lon1) = (40.0, 56.0);
        // 16x16 at the shipped budget, then 8x8 at zero -- the identity march, whose answer
        // is exactly 1.0 and which is therefore the one cell of this corpus that a wrong
        // `exp` could not move. It is here so the control has something to be measured
        // against that is NOT sensitive to the same term.
        for (width, height, samples) in [(16u32, 16u32, 160u32), (8, 8, 0)] {
            let values = (width * height) as usize * WB_CLIMATE_STRIDE; // cast-ok: a compile-time grid back to a length
            let mut tile = vec![0.0f32; values];
            let status = wb_climate_tile_f32(
                handle,
                lat0,
                lat1,
                lon0,
                lon1,
                width,
                height,
                RES_M,
                samples,
                tile.as_mut_ptr(),
                values as u32, // cast-ok: a compile-time length back to the ABI's u32
            );
            assert_eq!(status, WB_OK, "{name}: the climate tile must fill");
            // **A corpus of constants compares nothing.** The moisture channel of the
            // canonical-budget tile must actually vary, and it must reach well below
            // saturation, or `--mutate climate-samples` has nothing to move and the parity
            // comparison is a constant against itself. Asserted here, in the generator, so
            // the corpus cannot quietly become uninformative again.
            if samples > 0 {
                let mut distinct: Vec<u32> = tile.iter().skip(1).step_by(2).map(|v| v.to_bits()).collect();
                distinct.sort_unstable();
                distinct.dedup();
                let driest = tile.iter().skip(1).step_by(2).fold(f32::INFINITY, |a, b| if *b < a { *b } else { a });
                assert!(
                    distinct.len() > 100,
                    "{name}: only {} distinct moisture values in the climate tile",
                    distinct.len(),
                );
                assert!(driest < 0.5, "{name}: the driest cell is {driest}; this window is all sea");
            }
            let cells: Vec<String> = tile.iter().map(|v| hex32(*v)).collect();
            println!(
                "CL {name} {} {} {} {} {width} {height} {} {samples} {}",
                hex(lat0),
                hex(lat1),
                hex(lon0),
                hex(lon1),
                hex(RES_M),
                cells.join(" ")
            );
        }
        // The calibration, at a budget small enough to replay in WASM: 4,000 elevations plus
        // one 8-step march at every land point.
        let mut edges = [0.0f64; WB_CLIMATE_CALIBRATION_STRIDE];
        let status = wb_climate_calibration(
            handle,
            RES_M,
            8,
            edges.as_mut_ptr(),
            WB_CLIMATE_CALIBRATION_STRIDE as u32, // cast-ok: a compile-time stride back to the ABI's u32
        );
        assert_eq!(status, WB_OK, "{name}: the calibration must answer");
        assert!(edges[6] > 100.0, "{name}: this corpus needs a world with real land");
        let encoded: Vec<String> = edges.iter().map(|v| hex(*v)).collect();
        println!("CK {name} {} 8 {}", hex(RES_M), encoded.join(" "));
    }

    // --- erosion: one capped bake over a real graph, through wb_erosion_run -----------
    //
    // Task 5's corpus. `erosion.rs`'s module doc claims native and WASM agree bit-for-bit;
    // before this export existed nothing could check that (erosion was unreachable from
    // any WASM export). 3,000 nodes exercises every arithmetic path `erode_step` has --
    // sqrt, atan2 via SpherePoint::distance_to, the implicit receiver update -- the same
    // way a 20,000,000-node planetary bake would, because bit-equality does not depend on
    // size; only the planetary bake's *memory footprint* does (slice 1p: 1.45 GB of arrays,
    // does not fit a 32-bit wasm heap), which is not what this corpus is testing.
    //
    // `EROSION_THRESHOLD_M` is deliberately far tighter than this graph reaches in
    // `EROSION_MAX_ITERATIONS` steps at these constants (c ~ 1.0e-3, see
    // `erosion.rs::erode_step`'s doc): the run is designed to hit the iteration cap on
    // every invocation, native and WASM alike, so the number of `erode_step` calls is fixed
    // by construction rather than a side effect of whichever constant a mutation touches.
    // The two `assert_eq!`s below hold that design to its own claim -- if either ever
    // fires, the corpus's "same step count regardless of perturbation" property (which
    // `parity.mjs --mutate erosion-k` depends on to isolate arithmetic divergence from
    // step-count divergence) no longer holds and the control's own doc is wrong.
    const EROSION_NODES: u32 = 3_000;
    const EROSION_UPLIFT_M_PER_YR: f64 = 1.0e-3;
    const EROSION_ERODIBILITY_PER_YR: f64 = 1.0e-6;
    const EROSION_TIMESTEP_YR: f64 = 1000.0;
    const EROSION_THRESHOLD_M: f64 = 1.0e-9;
    const EROSION_MAX_ITERATIONS: u32 = 20;

    let mut erosion_heights = vec![0.0f64; EROSION_NODES as usize];
    let mut erosion_iterations: u32 = 0;
    let mut erosion_converged: u32 = 0;
    let erosion_status = wb_erosion_run(
        plain,
        EROSION_NODES,
        EROSION_UPLIFT_M_PER_YR,
        EROSION_ERODIBILITY_PER_YR,
        EROSION_TIMESTEP_YR,
        EROSION_THRESHOLD_M,
        EROSION_MAX_ITERATIONS,
        erosion_heights.as_mut_ptr(),
        EROSION_NODES,
        &mut erosion_iterations,
        &mut erosion_converged,
    );
    assert_eq!(erosion_status, WB_OK, "the erosion run must succeed for the parity corpus");
    assert_eq!(
        erosion_iterations, EROSION_MAX_ITERATIONS,
        "the corpus is designed to hit the iteration cap on every run, not converge early -- \
         a different count here means EROSION_THRESHOLD_M is no longer tight enough for this \
         claim, and parity.mjs's erosion-k control can no longer assume a fixed step count"
    );
    assert_eq!(erosion_converged, 0, "see erosion_iterations above");
    let erosion_hex: Vec<String> = erosion_heights.iter().map(|v| hex(*v)).collect();
    println!(
        "R erosion {SEED} {} {PLATES} {} {EROSION_NODES} {} {} {} {} {EROSION_MAX_ITERATIONS} {erosion_status} {erosion_iterations} {erosion_converged} {}",
        hex(RADIUS_M),
        hex(LAND),
        hex(EROSION_UPLIFT_M_PER_YR),
        hex(EROSION_ERODIBILITY_PER_YR),
        hex(EROSION_TIMESTEP_YR),
        hex(EROSION_THRESHOLD_M),
        erosion_hex.join(" ")
    );


    // --- the relief channel: the presets themselves, then a world built from one ---------
    //
    // Relief Task 4 changed the export surface for the first time in that slice and flagged,
    // correctly, that parity had not been re-run against it. The relief block travels to the
    // tile workers, so a native-versus-WASM difference in how it is *decoded* is exactly the
    // class of bug this harness exists to catch, and nothing in the corpus above could have
    // seen one: every world here was built through `wb_world_new`, which sends `None`.
    //
    // The preset's ten fields are compared first, and then used. **They are never retyped**
    // -- `wb_relief_preset` is the only place they come from, on both sides, which is that
    // export's whole reason for existing (its own doc: "so no host ever transcribes a
    // preset"). The replaying side reads them back out of this file rather than calling its
    // own `wb_relief_preset`... and then also calls it, because the comparison of the two is
    // itself a parity claim about the newest export in the surface.
    let mut hills = [0.0f64; WB_RELIEF_STRIDE];
    let mut canonical = [0.0f64; WB_RELIEF_STRIDE];
    let hills_status = wb_relief_preset(WB_RELIEF_HILLS, hills.as_mut_ptr(), WB_RELIEF_STRIDE as u32); // cast-ok: a compile-time stride into the export's u32 length
    let canonical_status =
        wb_relief_preset(WB_RELIEF_CANONICAL, canonical.as_mut_ptr(), WB_RELIEF_STRIDE as u32); // cast-ok: a compile-time stride into the export's u32 length
    assert_eq!(hills_status, WB_OK, "the hills preset must be readable");
    assert_eq!(canonical_status, WB_OK, "the canonical preset must be readable");
    for (selector, status, record) in [
        (WB_RELIEF_CANONICAL, canonical_status, &canonical),
        (WB_RELIEF_HILLS, hills_status, &hills),
    ] {
        let encoded: Vec<String> = record.iter().map(|v| hex(*v)).collect();
        println!("P {selector} {status} {}", encoded.join(" "));
    }

    // A world carrying a NON-canonical relief block. `hills()` is the obvious choice and the
    // brief named it: it is the one preset the viewer's panel can reach with a button, so it
    // is the block most likely to be in flight when a decode differs.
    let relief_encoded: Vec<String> = hills.iter().map(|v| hex(*v)).collect();
    println!(
        "worldr hills {SEED} {} {PLATES} {} {}",
        hex(RADIUS_M),
        hex(LAND),
        relief_encoded.join(" ")
    );
    let hills_world = wb_world_new_relief(
        SEED,
        RADIUS_M,
        PLATES,
        LAND,
        core::ptr::null(),
        0,
        hills.as_ptr(),
        WB_RELIEF_STRIDE as u32, // cast-ok: a compile-time stride into the export's u32 length
    );
    assert!(hills_world != 0, "the hills world must build");

    // 5,000 scattered points on the hills world, elevation and structural each. Structural is
    // included deliberately even though relief cannot move it: a relief block that leaked
    // into the tectonic path would show up here and nowhere else, and an entry that can only
    // agree is still evidence about which paths the block does not reach.
    for _ in 0..5_000 {
        let latitude_deg = rng.unit() * 180.0 - 90.0;
        let longitude_deg = rng.unit() * 360.0 - 180.0;
        println!(
            "E hills {} {} {} {}",
            hex(latitude_deg),
            hex(longitude_deg),
            hex(RES_M),
            hex(wb_elevation_m(hills_world, latitude_deg, longitude_deg, RES_M))
        );
        println!(
            "S hills {} {} {}",
            hex(latitude_deg),
            hex(longitude_deg),
            hex(wb_structural_m(hills_world, latitude_deg, longitude_deg))
        );
    }

    // And a tile, because the tile worker is where the relief block actually lands: the
    // viewer attaches it to the spec before `TilePool.start`, so every worker builds its own
    // world from it. A scalar corpus cannot see the grid, and the grid is the consumer.
    {
        let (lat0, lat1) = (HARBOUR_LAT + 0.005, HARBOUR_LAT - 0.005);
        let (lon0, lon1) = (HARBOUR_LON - 0.005, HARBOUR_LON + 0.005);
        let (width, height) = (65u32, 65u32);
        let mut tile = vec![0.0f32; 65 * 65];
        let status = wb_fill_tile_f32(
            hills_world,
            lat0,
            lat1,
            lon0,
            lon1,
            width,
            height,
            RES_M,
            tile.as_mut_ptr(),
            width * height,
        );
        assert_eq!(status, WB_OK, "hills: the tile must fill");
        let cells: Vec<String> = tile.iter().map(|v| hex32(*v)).collect();
        println!(
            "T hills {} {} {} {} {width} {height} {} {}",
            hex(lat0),
            hex(lat1),
            hex(lon0),
            hex(lon1),
            hex(RES_M),
            cells.join(" ")
        );
    }

    // --- water: the shipped manifest, through the shipped export ------------------------
    //
    // Slice 5b Task 5's own corpus. `water.rs` was unreachable from the export surface until
    // `wb_water_run` existed, exactly as `erosion.rs` was until `wb_erosion_run` did, so its
    // native/WASM claim was unfalsifiable rather than unverified.
    //
    // What is dumped is the SHIPPED manifest -- `water_manifest_from_graph`'s own rows, after
    // fill, overflow resolution, Ruling 7's tied-plateau merge and classification -- and not
    // an intermediate. The pre-flight conflict scan named that trap for this pair of tasks,
    // and 5a's equivalent was `erode_step` being uncapped outside the loop.
    //
    // `WATER_SEA_LEVEL_M = 0.0` is the datum every other record here is already implicitly
    // at. `WATER_POND_MAX_SURFACE_AREA_M2 = 1.0e5` is Task 3's CALIBRATED threshold, on
    // external ground (a shoreline walkable in about fifteen minutes), and it produces ZERO
    // ponds on this mesh -- the smallest body the generator makes is nearly four orders of
    // magnitude larger. That is a recorded finding about the mesh, and the corpus carries the
    // calibrated value rather than a value chosen to make ponds appear.
    const WATER_NODES: u32 = 30_000;
    const WATER_SEA_LEVEL_M: f64 = 0.0;
    const WATER_POND_MAX_SURFACE_AREA_M2: f64 = 1.0e5;
    // The control's threshold. Chosen off the measured distribution rather than by taste: the
    // 5b Task 3 survey put this mesh's body surface areas over roughly one order of magnitude
    // around a median of ~1.9e10 m^2, so a threshold at 2.0e10 splits the population instead
    // of moving all of it or none of it. `--mutate water-pond` replays the record below at
    // this value and at nothing else.
    const WATER_CONTROL_POND_MAX_SURFACE_AREA_M2: f64 = 2.0e10;

    let mut water_rows = vec![0.0f64; WATER_NODES as usize * WB_WATER_BODY_STRIDE];
    let water_capacity = water_rows.len() as u32; // cast-ok: a corpus-fixed buffer length back into the export's u32
    let mut water_body_count: u32 = 0;
    let mut water_sea_level_m: f64 = 0.0;
    let water_status = wb_water_run(
        plain,
        WATER_NODES,
        WATER_SEA_LEVEL_M,
        WATER_POND_MAX_SURFACE_AREA_M2,
        water_rows.as_mut_ptr(),
        water_capacity,
        &mut water_body_count,
        &mut water_sea_level_m,
    );
    assert_eq!(water_status, WB_OK, "the water run must succeed for the parity corpus");
    assert!(water_body_count > 0, "a corpus of zero bodies would compare nothing");
    water_rows.truncate(water_body_count as usize * WB_WATER_BODY_STRIDE);
    assert!(
        water_rows.chunks_exact(WB_WATER_BODY_STRIDE).all(|row| row[1] == WB_BODY_KIND_LAKE),
        "the calibrated threshold produces no ponds on this mesh -- if this ever fires, the \
         corpus has quietly 'fixed' Task 3's recorded finding rather than carrying it",
    );

    // THE CONTROL'S PREDICTION, DERIVED INDEPENDENTLY OF THE CLASSIFIER IT PREDICTS.
    //
    // A control gate that is read off the control's own run is a rubber stamp. So the number
    // below comes from the other side: `water::lake_body_surface_areas_m2` is the summed
    // surface area per physical body, and a body flips to `Pond` exactly when that area is at
    // or below the threshold. Counting the distribution is arithmetic over areas;
    // `classify_lake_kinds` is the thing being predicted. The assertion that the two agree is
    // the cross-check, and it runs here, natively, before the harness ever replays anything.
    let predicted_flips = {
        let sampling = sample_nodes(SEED as u64, WATER_NODES, RADIUS_M) // cast-ok: two's-complement reinterpretation, the same one wb_world_new makes for Noise
            .expect("the corpus node set must sample");
        let surface = Surface::new(SEED, RADIUS_M, PLATES as usize, LAND, None, None, None); // cast-ok: a corpus-fixed plate count widened to usize
        let heights: Vec<f64> =
            sampling.positions.iter().map(|point| surface.elevation_m(point, None)).collect();
        let mut graph = StreamGraph::build(
            &BuildParams {
                world_seed: SEED as u64, // cast-ok: two's-complement reinterpretation, as above
                radius_m: RADIUS_M,
                sea_level_m: WATER_SEA_LEVEL_M,
                sampling_kind: SamplingKind::Spiral,
                pond_max_surface_area_m2: WATER_POND_MAX_SURFACE_AREA_M2,
            },
            &sampling.positions,
            &heights,
            &sampling.area_m2,
            &sampling.neighbours,
        )
        .expect("the corpus graph must build");
        let basins = water::fill_and_resolve_water(&mut graph, WATER_POND_MAX_SURFACE_AREA_M2);
        let areas = water::lake_body_surface_areas_m2(&graph, &basins);
        assert_eq!(
            areas.len(),
            water_body_count as usize,
            "the area distribution and the manifest must describe the same bodies",
        );
        areas.iter().filter(|a| **a <= WATER_CONTROL_POND_MAX_SURFACE_AREA_M2).count()
    };
    assert!(
        predicted_flips > 0 && predicted_flips < water_body_count as usize,
        "the control threshold must split the population -- {predicted_flips} of {water_body_count} \
         is not a control, it is either a no-op or a different corpus",
    );

    // The cross-check: the classifier, run at the control's threshold, must produce exactly
    // the ponds the area distribution predicts. If these two ever disagree, the prediction is
    // wrong and the gate below it is meaningless, and that must fail here rather than be
    // absorbed into a divergent count.
    {
        let mut control_rows = vec![0.0f64; WATER_NODES as usize * WB_WATER_BODY_STRIDE];
        let capacity = control_rows.len() as u32; // cast-ok: as above
        let mut count: u32 = 0;
        let mut datum: f64 = 0.0;
        let status = wb_water_run(
            plain,
            WATER_NODES,
            WATER_SEA_LEVEL_M,
            WATER_CONTROL_POND_MAX_SURFACE_AREA_M2,
            control_rows.as_mut_ptr(),
            capacity,
            &mut count,
            &mut datum,
        );
        assert_eq!(status, WB_OK, "the control's own native run must succeed");
        assert_eq!(count, water_body_count, "the threshold must not move the body count");
        control_rows.truncate(count as usize * WB_WATER_BODY_STRIDE);
        let ponds = control_rows
            .chunks_exact(WB_WATER_BODY_STRIDE)
            .filter(|row| row[1] == WB_BODY_KIND_POND)
            .count();
        assert_eq!(
            ponds, predicted_flips,
            "the classifier and the independently summed surface areas disagree about how many \
             bodies fall under the control threshold",
        );
        // And nothing but `kind` moved, which is what makes this control narrow rather than
        // gross. Asserted here as well as in `tests/wasm_exports.rs` because it is the
        // property the gate's own number depends on.
        for (a, b) in water_rows
            .chunks_exact(WB_WATER_BODY_STRIDE)
            .zip(control_rows.chunks_exact(WB_WATER_BODY_STRIDE))
        {
            for field in [0usize, 2, 3, 4, 5, 6] {
                assert_eq!(
                    a[field].to_bits(),
                    b[field].to_bits(),
                    "field {field} moved with the pond threshold; the control would then be \
                     measuring something other than what it claims",
                );
            }
        }
    }

    println!(
        "WCTL {} {predicted_flips}",
        hex(WATER_CONTROL_POND_MAX_SURFACE_AREA_M2)
    );
    let water_hex: Vec<String> = water_rows.iter().map(|v| hex(*v)).collect();
    println!(
        "W plain {WATER_NODES} {} {} {water_status} {water_body_count} {} {}",
        hex(WATER_SEA_LEVEL_M),
        hex(WATER_POND_MAX_SURFACE_AREA_M2),
        hex(water_sea_level_m),
        water_hex.join(" ")
    );

    // --- the tectonic channel: the presets, the checker, and a world built from one -------
    //
    // Task 4 of the mountains slice flagged this gap, Task 3 flagged it again larger, and
    // Task 5 flagged it a third time two fields larger still. **None of them owned it.** The
    // corpus above compares 71,596 values and not one of them goes through
    // `wb_tectonic_preset`, `wb_tectonic_check` or a tectonic block on
    // `wb_world_new_tectonic` -- while `ranges()` is a preset the owner presses on the panel,
    // and it drives seven of `TectonicParams`' sixteen words across this boundary.
    //
    // The shape is the relief channel's, one row for one row, because the relief channel is
    // the thing this harness already got right: the presets first, field by field; then a
    // world carrying a NON-canonical block; then scalars on it; then a tile, because the tile
    // worker is the block's real consumer in the browser.
    //
    // **Why this is worth doing even though native and WASM are the same Rust.** They are --
    // over the same pure-Rust `libm`, so this comparison is strict bit-for-bit even where
    // transcendentals are in the path, which is a stronger contract than the bounded one
    // Python-versus-Rust conformance holds. What is NOT shared is the *decode*: the block
    // crosses as sixteen f64 in linear memory, is read back through a raw pointer, and is
    // bounds-checked before it becomes a `TectonicParams`. That path exists only on this
    // boundary, and until now nothing exercised it.
    let mut tectonic_canonical = [0.0f64; WB_TECTONIC_STRIDE];
    let mut tectonic_ranges = [0.0f64; WB_TECTONIC_STRIDE];
    let tectonic_canonical_status = wb_tectonic_preset(
        WB_TECTONIC_CANONICAL,
        tectonic_canonical.as_mut_ptr(),
        WB_TECTONIC_STRIDE as u32, // cast-ok: a compile-time stride into the export's u32 length
    );
    let tectonic_ranges_status = wb_tectonic_preset(
        WB_TECTONIC_RANGES,
        tectonic_ranges.as_mut_ptr(),
        WB_TECTONIC_STRIDE as u32, // cast-ok: a compile-time stride into the export's u32 length
    );
    assert_eq!(tectonic_canonical_status, WB_OK, "the canonical tectonic preset must be readable");
    assert_eq!(tectonic_ranges_status, WB_OK, "the ranges tectonic preset must be readable");
    assert_ne!(
        tectonic_ranges, tectonic_canonical,
        "a preset identical to canonical would make every tectonic row below a second copy of \
         the plain world's rows",
    );
    for (selector, status, record) in [
        (WB_TECTONIC_CANONICAL, tectonic_canonical_status, &tectonic_canonical),
        (WB_TECTONIC_RANGES, tectonic_ranges_status, &tectonic_ranges),
    ] {
        let encoded: Vec<String> = record.iter().map(|v| hex(*v)).collect();
        println!("TP {selector} {status} {}", encoded.join(" "));
    }

    // The control's block: `ranges()` with `margin_warp_m` -- word 14, `encode_tectonic`'s
    // own order -- turned off, and nothing else touched. Built from the preset the export
    // just handed back, so the fifteen fields it keeps are not retyped either.
    const TECTONIC_WARP_INDEX: usize = 14;
    const TECTONIC_CONTROL_WARP_M: f64 = 0.0;
    let mut tectonic_control = tectonic_ranges;
    tectonic_control[TECTONIC_WARP_INDEX] = TECTONIC_CONTROL_WARP_M;
    assert_ne!(
        tectonic_ranges[TECTONIC_WARP_INDEX], TECTONIC_CONTROL_WARP_M,
        "the control must actually change the field it names -- a preset that already ships \
         the control's value would make the whole control a no-op wearing a control's name",
    );

    // `wb_tectonic_check`, the third tectonic export and the only one that answers *why* a
    // record was refused. Six records, three accepted and three refused, so the group cannot
    // be trivially uniform in either direction: a checker that refused everything and a
    // checker that accepted everything would both pass a corpus of one kind.
    //
    // The refusals are the three the sweep found matter: the saturating `as u32` cast on a
    // loop bound, a `structure_depth` outside `structure_at`'s documented range, and a warp
    // amplitude that carries the collision profile past `MAX_TECTONIC_RANGE_M` on canonical's
    // 400 km flank -- the reach check, which no per-field ceiling can see.
    let mut tectonic_saturating = tectonic_ranges;
    tectonic_saturating[10] = 1.0e300;
    let mut tectonic_deep = tectonic_ranges;
    tectonic_deep[12] = 1.5;
    let mut tectonic_far = tectonic_canonical;
    tectonic_far[TECTONIC_WARP_INDEX] = 400_000.0;
    let check_records = [
        ("canonical", tectonic_canonical),
        ("ranges", tectonic_ranges),
        ("control", tectonic_control),
        ("saturating", tectonic_saturating),
        ("deep", tectonic_deep),
        ("far", tectonic_far),
    ];
    let mut accepted = 0usize;
    let mut refused = 0usize;
    for (name, record) in &check_records {
        let status = wb_tectonic_check(
            record.as_ptr(),
            WB_TECTONIC_STRIDE as u32, // cast-ok: a compile-time stride into the export's u32 length
        );
        if status == WB_OK {
            accepted += 1;
        } else {
            refused += 1;
        }
        let encoded: Vec<String> = record.iter().map(|v| hex(*v)).collect();
        println!("TC {name} {status} {}", encoded.join(" "));
    }
    assert_eq!(accepted, 3, "three of the six check records are meant to be accepted");
    assert_eq!(refused, 3, "three of the six check records are meant to be refused");

    // A world carrying the non-canonical block, twice under two names. **Two names, one
    // configuration, and that is deliberate**: the scattered points and the concentrated
    // ones then tally as separate groups, so the control's report says in its own output
    // that the belt moved and the rest of the planet did not. One mixed group would have
    // hidden exactly that.
    //
    // `TWARP` goes out first because the replaying side needs it *here*, when it builds
    // these worlds; the prediction it belongs to (`TCTL`) cannot be written until the corpus
    // has been sampled, so the control arrives as two records rather than one.
    println!("TWARP {}", hex(TECTONIC_CONTROL_WARP_M));
    let tectonic_encoded: Vec<String> = tectonic_ranges.iter().map(|v| hex(*v)).collect();
    for name in ["ranges", "belt"] {
        println!(
            "worldt {name} {SEED} {} {PLATES} {} {}",
            hex(RADIUS_M),
            hex(LAND),
            tectonic_encoded.join(" ")
        );
    }
    let tectonic_world = wb_world_new_tectonic(
        SEED,
        RADIUS_M,
        PLATES,
        LAND,
        core::ptr::null(),
        0,
        core::ptr::null(),
        0,
        tectonic_ranges.as_ptr(),
        WB_TECTONIC_STRIDE as u32, // cast-ok: a compile-time stride into the export's u32 length
    );
    assert!(tectonic_world != 0, "the ranges world must build");
    let tectonic_control_world = wb_world_new_tectonic(
        SEED,
        RADIUS_M,
        PLATES,
        LAND,
        core::ptr::null(),
        0,
        core::ptr::null(),
        0,
        tectonic_control.as_ptr(),
        WB_TECTONIC_STRIDE as u32, // cast-ok: a compile-time stride into the export's u32 length
    );
    assert!(tectonic_control_world != 0, "the control world must build");

    // WHERE THE BELT IS, AND WHY IT IS NOT A ROUND NUMBER.
    //
    // `src/bin/mountain_probe.rs`'s `witness_between` scans this exact fixture world on a
    // 0.5-degree global grid for the site where turning `margin_warp_m` off moves
    // `elevation_m` the most: **-5.00, 66.00, where the ground reads 821.955 m with the warp
    // off and 2,432.773 m with it on.** A corpus scattered uniformly over a planet does not
    // land on a 100 km belt -- the same reason this file already carries a second world for
    // the placed harbour -- and a control that moves nothing proves nothing.
    const BELT_LAT: f64 = -5.0;
    const BELT_LON: f64 = 66.0;
    const BELT_SPAN_DEG: f64 = 2.0;
    {
        let point_on = wb_elevation_m(tectonic_world, BELT_LAT, BELT_LON, RES_M);
        let point_off = wb_elevation_m(tectonic_control_world, BELT_LAT, BELT_LON, RES_M);
        let moved = if point_on > point_off { point_on - point_off } else { point_off - point_on };
        assert!(
            moved > 1_000.0,
            "the belt site must be somewhere the control's one field actually moves the \
             ground; it moved {moved} m, so either the witness is stale or the field no \
             longer reaches this world",
        );
    }

    // 5,000 scattered points, exactly as the relief world takes them: global, uniform, and
    // mostly nowhere near a convergent continental margin. That is the point -- they are the
    // corpus's evidence that the block does NOT reach the rest of the planet, and under the
    // control they are the group that mostly stays equal.
    let mut scattered_points = Vec::with_capacity(5_000);
    for _ in 0..5_000 {
        let latitude_deg = rng.unit() * 180.0 - 90.0;
        let longitude_deg = rng.unit() * 360.0 - 180.0;
        scattered_points.push((latitude_deg, longitude_deg));
        println!(
            "E ranges {} {} {} {}",
            hex(latitude_deg),
            hex(longitude_deg),
            hex(RES_M),
            hex(wb_elevation_m(tectonic_world, latitude_deg, longitude_deg, RES_M))
        );
        println!(
            "S ranges {} {} {}",
            hex(latitude_deg),
            hex(longitude_deg),
            hex(wb_structural_m(tectonic_world, latitude_deg, longitude_deg))
        );
    }

    // 2,000 points on the belt itself, in a +/-1 degree box on the witness site.
    let mut belt_points = Vec::with_capacity(2_000);
    for _ in 0..2_000 {
        let latitude_deg = BELT_LAT + (rng.unit() - 0.5) * BELT_SPAN_DEG;
        let longitude_deg = BELT_LON + (rng.unit() - 0.5) * BELT_SPAN_DEG;
        belt_points.push((latitude_deg, longitude_deg));
        println!(
            "E belt {} {} {} {}",
            hex(latitude_deg),
            hex(longitude_deg),
            hex(RES_M),
            hex(wb_elevation_m(tectonic_world, latitude_deg, longitude_deg, RES_M))
        );
        println!(
            "S belt {} {} {}",
            hex(latitude_deg),
            hex(longitude_deg),
            hex(wb_structural_m(tectonic_world, latitude_deg, longitude_deg))
        );
    }

    // And a tile across the belt, because the tile worker is where a tectonic block lands in
    // the browser exactly as a relief block does: the viewer attaches it to the spec before
    // `TilePool.start` and every worker rebuilds the world from it.
    let belt_tile = {
        let (lat0, lat1) = (BELT_LAT + 0.5, BELT_LAT - 0.5);
        let (lon0, lon1) = (BELT_LON - 0.5, BELT_LON + 0.5);
        let (width, height) = (65u32, 65u32);
        let mut tile = vec![0.0f32; 65 * 65];
        let status = wb_fill_tile_f32(
            tectonic_world,
            lat0,
            lat1,
            lon0,
            lon1,
            width,
            height,
            RES_M,
            tile.as_mut_ptr(),
            width * height,
        );
        assert_eq!(status, WB_OK, "belt: the tile must fill");
        let cells: Vec<String> = tile.iter().map(|v| hex32(*v)).collect();
        println!(
            "T belt {} {} {} {} {width} {height} {} {}",
            hex(lat0),
            hex(lat1),
            hex(lon0),
            hex(lon1),
            hex(RES_M),
            cells.join(" ")
        );
        (lat0, lat1, lon0, lon1, width, height, tile)
    };

    // THE TECTONIC CONTROL'S PREDICTION, PER GROUP, MADE HERE AND CHECKED ON THE OTHER SIDE.
    //
    // `--mutate tectonic-warp` replays the `worldt` records with word 14 set to 0.0 and
    // touches nothing else, so `margin_warp_m` is the only thing that differs. Everything
    // the mutation cannot reach must compare EQUAL, and that list is the informative half:
    // the two `TP` preset groups (a world block cannot move an export that hands back
    // `tectonics.rs`' own constants -- the same reason `preset/0` and `version` sit at zero
    // under `--mutate seed`), the `TC` checker group, and every group of every world above.
    //
    // The counts are computed natively, per group, and `parity.mjs` must meet each of them
    // exactly. Three things hold the prediction to something other than its own output:
    //
    //   1. **The library agrees with the exports.** The same counts are recomputed through
    //      `Surface::elevation_m` / `structural_m` directly rather than through
    //      `wb_elevation_m` / `wb_structural_m`, and the two must be equal. A disagreement
    //      means the export layer adds or hides a difference, and it fails HERE rather than
    //      being absorbed into a divergent tally later.
    //   2. **A structural containment.** `margin_warp_m` reaches `elevation_m` only through
    //      the tectonic offset, which is `structural_m`'s own content -- so every point whose
    //      elevation moved must be a point whose structural moved. Asserted as a subset, not
    //      as an equality: the reverse does not hold, and claiming it would be claiming
    //      something false.
    //   3. **Both ends refused.** Every group's count must be strictly between zero and the
    //      group's size. A control that moves everything is as uninformative as one that
    //      moves nothing, and this file will not write a corpus where either is true.
    let (
        control_elevation_ranges,
        control_structural_ranges,
        control_elevation_belt,
        control_structural_belt,
        control_tile_belt,
    ) = {
        // The library side reads its two blocks from `tectonics.rs` rather than from the
        // sixteen words that crossed the boundary, which is what makes this a second
        // derivation instead of the same one twice: if `encode_tectonic` and
        // `decode_tectonic` disagreed anywhere, the two counts below would part company.
        let on = Surface::new(
            SEED,
            RADIUS_M,
            PLATES as usize, // cast-ok: a corpus-fixed plate count widened to usize
            LAND,
            None,
            None,
            Some(TectonicParams::ranges()),
        );
        let off = Surface::new(
            SEED,
            RADIUS_M,
            PLATES as usize, // cast-ok: as above
            LAND,
            None,
            None,
            Some(TectonicParams {
                margin_warp_m: TECTONIC_CONTROL_WARP_M,
                ..TectonicParams::ranges()
            }),
        );

        // Both scalar groups, both ways round, over the points actually dumped.
        let mut moved = [0usize; 4];
        let mut moved_lib = [0usize; 4];
        for (slot, points) in [(0usize, &scattered_points), (2, &belt_points)] {
            for (latitude_deg, longitude_deg) in points {
                let e_on = wb_elevation_m(tectonic_world, *latitude_deg, *longitude_deg, RES_M);
                let e_off =
                    wb_elevation_m(tectonic_control_world, *latitude_deg, *longitude_deg, RES_M);
                let s_on = wb_structural_m(tectonic_world, *latitude_deg, *longitude_deg);
                let s_off = wb_structural_m(tectonic_control_world, *latitude_deg, *longitude_deg);
                let e_moved = e_on.to_bits() != e_off.to_bits();
                let s_moved = s_on.to_bits() != s_off.to_bits();
                if e_moved {
                    moved[slot] += 1;
                }
                if s_moved {
                    moved[slot + 1] += 1;
                }
                assert!(
                    !e_moved || s_moved,
                    "the warp moved elevation at {latitude_deg},{longitude_deg} without moving \
                     structural -- it reaches elevation only through the tectonic offset, so \
                     this would mean it now reaches something else",
                );
                let point = SpherePoint::from_latlon(*latitude_deg, *longitude_deg);
                if on.elevation_m(&point, Some(RES_M)).to_bits()
                    != off.elevation_m(&point, Some(RES_M)).to_bits()
                {
                    moved_lib[slot] += 1;
                }
                if on.structural_m(&point).to_bits() != off.structural_m(&point).to_bits() {
                    moved_lib[slot + 1] += 1;
                }
            }
        }
        assert_eq!(
            moved, moved_lib,
            "the exports and the library disagree about how many values the warp moves; the \
             sixteen words that crossed the boundary and `TectonicParams::ranges()` itself \
             are describing different worlds",
        );

        let (lat0, lat1, lon0, lon1, width, height, on_cells) = belt_tile;
        let mut control_tile = vec![0.0f32; (width * height) as usize]; // cast-ok: a compile-time 65x65 back to a length
        let status = wb_fill_tile_f32(
            tectonic_control_world,
            lat0,
            lat1,
            lon0,
            lon1,
            width,
            height,
            RES_M,
            control_tile.as_mut_ptr(),
            width * height,
        );
        assert_eq!(status, WB_OK, "belt: the control's tile must fill");
        let tile_moved = on_cells
            .iter()
            .zip(control_tile.iter())
            .filter(|(a, b)| a.to_bits() != b.to_bits())
            .count();

        (moved[0], moved[1], moved[2], moved[3], tile_moved)
    };

    for (label, moved, total) in [
        ("elevation/ranges", control_elevation_ranges, 5_000usize),
        ("structural/ranges", control_structural_ranges, 5_000),
        ("elevation/belt", control_elevation_belt, 2_000),
        ("structural/belt", control_structural_belt, 2_000),
        ("tile/belt", control_tile_belt, 65 * 65),
    ] {
        assert!(
            moved > 0 && moved < total,
            "{label}: the control moved {moved} of {total}. A control that moves everything is \
             as uninformative as one that moves nothing, and this corpus refuses to write \
             either",
        );
    }

    // I6 (final review of water 1a): a second `H` record, on this same tectonic `ranges`
    // world, with one forced outlet inside its first enclosed pocket. `earth_like`'s
    // thresholds at 60,000 nodes put the 12b-1 node-area floor in charge of the stream
    // threshold, which is the point -- the plain `H` record above never binds that floor.
    //
    // The forced point is not chosen by hand: an unforced probe bake finds the world's
    // enclosed pockets, and the FIRST one (in bake order) gives its anchor lat/lon, exactly as
    // the ruling asks. That keeps the corpus reproducible from the tectonic block alone,
    // without a hand-picked coordinate this file would otherwise have to justify.
    const HYDRO_TECTONIC_PARAMS_BASE: [f64; 12] =
        [60_000.0, 20_000.0, 8.0, 1.0e6, 1.0e6, 2.5e8, 2.5e9, 1.0e11, 1.0, 1.0, 0.1, 0.0];

    let forced_anchor = {
        let (probe_status, probe_len, probe_words) =
            bake_hydro_native(tectonic_world, &HYDRO_TECTONIC_PARAMS_BASE);
        assert_eq!(probe_status, WB_OK, "the unforced probe bake on the ranges world must succeed");
        assert!(probe_len > 0, "a probe record of zero words has no body to anchor on");
        let record = hydrology::record::decode(&probe_words)
            .expect("the probe record must decode -- it was just encoded by this same binary");
        let body = record
            .bodies
            .iter()
            .find(|b| b.enclosed)
            .expect("the ranges world at 60,000 nodes must have at least one enclosed body");
        body.anchor
    };

    let mut hydro_tectonic_params = HYDRO_TECTONIC_PARAMS_BASE.to_vec();
    hydro_tectonic_params[11] = 1.0; // one forced outlet
    hydro_tectonic_params.push(forced_anchor.0);
    hydro_tectonic_params.push(forced_anchor.1);

    let (h_ranges_status, h_ranges_len, h_ranges_words) =
        bake_hydro_native(tectonic_world, &hydro_tectonic_params);
    assert_eq!(h_ranges_status, WB_OK, "the forced-outlet bake on the ranges world must succeed");
    assert!(h_ranges_len > 0, "a corpus of zero words would compare nothing");

    let h_ranges_params_hex: Vec<String> = hydro_tectonic_params.iter().map(|v| hex(*v)).collect();
    let h_ranges_words_hex: Vec<String> = h_ranges_words.iter().map(|v| hex(*v)).collect();
    println!(
        "H ranges {} {} {h_ranges_status} {h_ranges_len} {}",
        hydro_tectonic_params.len(),
        h_ranges_params_hex.join(" "),
        h_ranges_words_hex.join(" ")
    );

    // The native prediction for `--mutate tectonic-warp`: the same forced params, baked on the
    // warp-0 world instead, compared against the record just printed above under rule (a). This
    // is what lets `parity.mjs` require `hydro/ranges` to move by exactly this many words under
    // that control and by nothing under any other -- the same discipline `TCTL`'s other four
    // counts already hold it to.
    let (h_ranges_off_status, h_ranges_off_len, h_ranges_off_words) =
        bake_hydro_native(tectonic_control_world, &hydro_tectonic_params);
    let hydro_ranges_control = divergent_count(
        h_ranges_status,
        h_ranges_len,
        &h_ranges_words,
        h_ranges_off_status,
        h_ranges_off_len,
        &h_ranges_off_words,
    );
    assert!(
        hydro_ranges_control > 0 && hydro_ranges_control < h_ranges_len as usize + 2, // cast-ok: the record's own length, widened to compare against a divergent count over the same 2+len accounting
        "hydro/ranges: the control moved {hydro_ranges_control} of {}. A control that moves \
         everything is as uninformative as one that moves nothing, and this corpus refuses to \
         write either",
        h_ranges_len + 2,
    );

    // Ruling Q-21's `ranges` points, emitted here rather than beside `WP plain` at the bottom for
    // one reason: this control's prediction for them has to be in the `TCTL` record below, and
    // that record is written here. The replaying side reads records by tag, not by position.
    let (ranges_kinds, ranges_points) =
        print_water_points("ranges", tectonic_world, &hydro_tectonic_params);
    let water_points_ranges_control =
        water_points_divergence(tectonic_control_world, &hydro_tectonic_params, &ranges_points);
    let water_points_ranges_total = ranges_points.len() * (1 + WP_STRIDE);
    assert!(
        water_points_ranges_control > 0 && water_points_ranges_control < water_points_ranges_total,
        "water_point/ranges: the control moved {water_points_ranges_control} of \
         {water_points_ranges_total}. A control that moves everything is as uninformative as one \
         that moves nothing, and this corpus refuses to write either."
    );

    // Plan 2b Task 7: the carve on the `ranges` world, emitted here for the reason the `ranges`
    // `WP` points are -- its prediction under this control belongs in `TCTL` below. The bake is
    // the `H ranges` bake's own params in the thirteen-word layout (word 12, `drain_for_carve`,
    // set; the forced pair after it), because only a record baked for carving can be joined.
    let mut carve_ranges_params = hydro_tectonic_params[..12].to_vec();
    carve_ranges_params.push(1.0);
    carve_ranges_params.push(forced_anchor.0);
    carve_ranges_params.push(forced_anchor.1);
    let carve_ranges = CarveSpec {
        name: "ranges",
        base: tectonic_world,
        tectonic: Some(&tectonic_ranges),
        params: &carve_ranges_params,
        block: water_preset_block(),
    };
    // The carving record itself, word for word (fix round): the drain (Task 4b) runs inside the
    // wasm bake whenever the studio carves, and the carved elevations below see it only at their
    // own points. Compared whole, as `H ranges` is, and checked natively against the ordinary
    // record to differ only where the drain says.
    let (hc_ranges_status, hc_ranges_len, hc_ranges_words) =
        bake_hydro_native(tectonic_world, &carve_ranges_params);
    assert_eq!(hc_ranges_status, WB_OK, "the ranges bake for carving must succeed");
    print_carving_record("ranges", &carve_ranges_params, hc_ranges_status, hc_ranges_len, &hc_ranges_words);
    check_drain("ranges", &h_ranges_words, &hc_ranges_words);
    let (hc_ranges_off_status, hc_ranges_off_len, hc_ranges_off_words) =
        bake_hydro_native(tectonic_control_world, &carve_ranges_params);
    let hydro_carve_ranges_control = divergent_count(
        hc_ranges_status, hc_ranges_len, &hc_ranges_words,
        hc_ranges_off_status, hc_ranges_off_len, &hc_ranges_off_words,
    );
    assert!(
        hydro_carve_ranges_control > 0 && hydro_carve_ranges_control < hc_ranges_len as usize + 2, // cast-ok: a record length widened to compare with a divergent count over the same 2+len accounting
        "hydro_carve/ranges: the control moved {hydro_carve_ranges_control} of {}; this corpus \
         refuses a control that moves nothing or everything",
        hc_ranges_len + 2,
    );

    // `--mutate carve-bank`'s value goes out before the first `WC` record, which is where the
    // replaying side substitutes it; its prediction (`CBCTL`) follows the last.
    println!("CBANK {}", hex(CARVE_BANK_CONTROL));
    let (carve_ranges_status, carve_ranges_points) =
        carve_points(&carve_ranges, Some(TectonicParams::ranges()));
    print_carve(&carve_ranges, carve_ranges_status, &carve_ranges_points);
    let carve_bank_ranges =
        carve_bank_divergence(&carve_ranges, CARVE_BANK_CONTROL, carve_ranges_status, &carve_ranges_points);
    let carve_ranges_control = carve_divergence(&carve_ranges, tectonic_control_world,
        &tectonic_control, carve_ranges_status, &carve_ranges_points);
    let carve_ranges_total = 1 + carve_ranges_points.len();
    assert!(
        carve_ranges_control > 0 && carve_ranges_control < carve_ranges_total,
        "carve/ranges: the control moved {carve_ranges_control} of {carve_ranges_total}. A control \
         that moves everything is as uninformative as one that moves nothing, and this corpus \
         refuses to write either."
    );

    println!(
        "TCTL {control_elevation_ranges} {control_structural_ranges} \
         {control_elevation_belt} {control_structural_belt} {control_tile_belt} \
         {hydro_ranges_control} {water_points_ranges_control} {carve_ranges_control} \
         {hydro_carve_ranges_control}"
    );

    // --- the coast channel: the presets, the checker, and a world built from one -----------
    //
    // Task 5 built `CoastParams` and could not expose it; Task 6 opened
    // `wb_world_new_coast` / `wb_coast_preset` / `wb_coast_check`, and **a new crossing value
    // with no corpus coverage is a crossing value nothing compares.** The tectonic block sat
    // in exactly that position for three tasks before anybody owned it, and this section is
    // written at the same time as the export rather than three tasks later.
    //
    // The shape is the tectonic channel's, one row for one row: the presets first, field by
    // field; then the checker, accepted and refused; then a world carrying a NON-canonical
    // block; then scalars on it; then a tile, because the tile worker is the block's real
    // consumer in the browser.
    //
    // **What is NOT shared between the two sides is the decode.** The block crosses as six
    // f64 in linear memory, is read back through a raw pointer, and is bounds-checked --
    // including one bound that is a *product* of three fields and one that is a loop bound
    // narrowed from an f64 -- before it becomes a `CoastParams`. That path exists only on
    // this boundary.
    let mut coast_canonical = [0.0f64; WB_COAST_STRIDE];
    let mut coast_fractal = [0.0f64; WB_COAST_STRIDE];
    let coast_canonical_status = wb_coast_preset(
        WB_COAST_CANONICAL,
        coast_canonical.as_mut_ptr(),
        WB_COAST_STRIDE as u32, // cast-ok: a compile-time stride into the export's u32 length
    );
    let coast_fractal_status = wb_coast_preset(
        WB_COAST_FRACTAL,
        coast_fractal.as_mut_ptr(),
        WB_COAST_STRIDE as u32, // cast-ok: as above
    );
    assert_eq!(coast_canonical_status, WB_OK, "the canonical coast preset must be readable");
    assert_eq!(coast_fractal_status, WB_OK, "the fractal coast preset must be readable");
    assert_ne!(
        coast_fractal, coast_canonical,
        "a preset identical to canonical would make every coast row below a second copy of \
         the plain world's rows",
    );
    for (selector, status, record) in [
        (WB_COAST_CANONICAL, coast_canonical_status, &coast_canonical),
        (WB_COAST_FRACTAL, coast_fractal_status, &coast_fractal),
    ] {
        let encoded: Vec<String> = record.iter().map(|v| hex(*v)).collect();
        println!("CP {selector} {status} {}", encoded.join(" "));
    }

    // The control's block: `fractal()` with `amplitude` -- word 0, `encode_coast`'s own order
    // -- turned back to canonical's inert value, and nothing else touched. Built from the
    // preset the export just handed back, so the five fields it keeps are not retyped either.
    const COAST_AMPLITUDE_INDEX: usize = 0;
    let coast_control_amplitude = coast_canonical[COAST_AMPLITUDE_INDEX];
    let mut coast_control = coast_fractal;
    coast_control[COAST_AMPLITUDE_INDEX] = coast_control_amplitude;
    assert_ne!(
        coast_fractal[COAST_AMPLITUDE_INDEX], coast_control_amplitude,
        "the control must actually change the field it names -- a preset that already ships \
         the control's value would make the whole control a no-op wearing a control's name",
    );

    // `wb_coast_check`, the third coast export and the only one that answers *why* a record
    // was refused. Six records, three accepted and three refused, so the group cannot be
    // trivially uniform in either direction.
    //
    // The refusals are the three the sweep found matter: the saturating `as u32` on a
    // per-sample loop bound, a **product** of three individually-admissible fields that walks
    // the noise lattice's `i64` index past saturation, and a negative amplitude -- the sign
    // this channel refuses because the lattice is zero-mean and a mirrored field is a second
    // spelling of "how far".
    let mut coast_saturating = coast_fractal;
    coast_saturating[3] = 1.0e300;
    let mut coast_product = coast_fractal;
    coast_product[2] = WB_MAX_COAST_FINEST_FREQUENCY;
    coast_product[3] = f64::from(WB_MAX_COAST_OCTAVES);
    coast_product[5] = WB_MAX_COAST_LACUNARITY;
    let mut coast_mirrored = coast_fractal;
    coast_mirrored[COAST_AMPLITUDE_INDEX] = -coast_fractal[COAST_AMPLITUDE_INDEX];
    let coast_check_records = [
        ("canonical", coast_canonical),
        ("fractal", coast_fractal),
        ("control", coast_control),
        ("saturating", coast_saturating),
        ("product", coast_product),
        ("mirrored", coast_mirrored),
    ];
    let mut coast_accepted = 0usize;
    let mut coast_refused = 0usize;
    for (name, record) in &coast_check_records {
        let status = wb_coast_check(
            record.as_ptr(),
            WB_COAST_STRIDE as u32, // cast-ok: a compile-time stride into the export's u32 length
        );
        if status == WB_OK {
            coast_accepted += 1;
        } else {
            coast_refused += 1;
        }
        let encoded: Vec<String> = record.iter().map(|v| hex(*v)).collect();
        println!("CC {name} {status} {}", encoded.join(" "));
    }
    assert_eq!(coast_accepted, 3, "three of the six coast check records are meant to be accepted");
    assert_eq!(coast_refused, 3, "three of the six coast check records are meant to be refused");

    // A world carrying the non-canonical block, twice under two names, for the reason the
    // tectonic world is: the scattered points and the concentrated ones then tally as
    // separate groups, so the control's own report says which population moved and by how
    // much. One mixed group would have hidden exactly that.
    //
    // `CAMP` goes out first because the replaying side needs it *here*, when it builds these
    // worlds; the prediction it belongs to (`CCTL`) cannot be written until the corpus has
    // been sampled.
    println!("CAMP {}", hex(coast_control_amplitude));
    let coast_encoded: Vec<String> = coast_fractal.iter().map(|v| hex(*v)).collect();
    for name in ["fractal", "shore"] {
        println!(
            "worldc {name} {SEED} {} {PLATES} {} {}",
            hex(RADIUS_M),
            hex(LAND),
            coast_encoded.join(" ")
        );
    }
    let coast_world = wb_world_new_coast(
        SEED,
        RADIUS_M,
        PLATES,
        LAND,
        core::ptr::null(),
        0,
        core::ptr::null(),
        0,
        core::ptr::null(),
        0,
        coast_fractal.as_ptr(),
        WB_COAST_STRIDE as u32, // cast-ok: a compile-time stride into the export's u32 length
    );
    assert!(coast_world != 0, "the fractal coast world must build");
    let coast_control_world = wb_world_new_coast(
        SEED,
        RADIUS_M,
        PLATES,
        LAND,
        core::ptr::null(),
        0,
        core::ptr::null(),
        0,
        core::ptr::null(),
        0,
        coast_control.as_ptr(),
        WB_COAST_STRIDE as u32, // cast-ok: as above
    );
    assert!(coast_control_world != 0, "the coast control world must build");

    // WHERE THE SHORE IS, AND WHY IT IS NOT A ROUND NUMBER.
    //
    // A 0.5-degree global scan of this exact fixture compares the canonical world against
    // `CoastParams::fractal()` and finds **162,159 of 258,480 sites moving**; the largest
    // mover is **-71.50, 38.00, where the ground moves 1,267.34 m**. A corpus scattered
    // uniformly over a sphere that is 71% open water does reach the coastal band -- that band
    // is wide -- but it does not concentrate on it, and the concentrated group is what makes
    // the control's report readable.
    const COAST_LAT: f64 = -71.5;
    const COAST_LON: f64 = 38.0;
    // **Twenty degrees, not two, and the first attempt at two is why.** The coastal window is
    // `|above_shore| <= window_spreads * spread`, which on this world is a band wide enough that
    // a 0.5-degree global scan finds 63% of all sites moving -- so a 2-degree box on the largest
    // mover is entirely INSIDE the band and every one of its 2,000 points moved. This file's own
    // both-ends-refused guard caught that and refused to write the corpus, which is the guard
    // doing exactly what it is for: a group that moves 2,000 of 2,000 is as uninformative as one
    // that moves none. Twenty degrees straddles the band and the ground either side of it.
    const COAST_SPAN_DEG: f64 = 20.0;
    {
        let point_on = wb_elevation_m(coast_world, COAST_LAT, COAST_LON, RES_M);
        let point_off = wb_elevation_m(coast_control_world, COAST_LAT, COAST_LON, RES_M);
        let moved = if point_on > point_off { point_on - point_off } else { point_off - point_on };
        assert!(
            moved > 500.0,
            "the shore site must be somewhere the control's one field actually moves the \
             ground; it moved {moved} m, so either the witness is stale or the field no \
             longer reaches this world",
        );
    }

    // 5,000 scattered points, exactly as the other two worlds take them.
    let mut coast_scattered = Vec::with_capacity(5_000);
    for _ in 0..5_000 {
        let latitude_deg = rng.unit() * 180.0 - 90.0;
        let longitude_deg = rng.unit() * 360.0 - 180.0;
        coast_scattered.push((latitude_deg, longitude_deg));
        println!(
            "E fractal {} {} {} {}",
            hex(latitude_deg),
            hex(longitude_deg),
            hex(RES_M),
            hex(wb_elevation_m(coast_world, latitude_deg, longitude_deg, RES_M))
        );
        println!(
            "S fractal {} {} {}",
            hex(latitude_deg),
            hex(longitude_deg),
            hex(wb_structural_m(coast_world, latitude_deg, longitude_deg))
        );
    }

    // 2,000 points on the shore itself, in a +/-1 degree box on the witness site.
    let mut coast_shore = Vec::with_capacity(2_000);
    for _ in 0..2_000 {
        let latitude_deg = COAST_LAT + (rng.unit() - 0.5) * COAST_SPAN_DEG;
        let longitude_deg = COAST_LON + (rng.unit() - 0.5) * COAST_SPAN_DEG;
        coast_shore.push((latitude_deg, longitude_deg));
        println!(
            "E shore {} {} {} {}",
            hex(latitude_deg),
            hex(longitude_deg),
            hex(RES_M),
            hex(wb_elevation_m(coast_world, latitude_deg, longitude_deg, RES_M))
        );
        println!(
            "S shore {} {} {}",
            hex(latitude_deg),
            hex(longitude_deg),
            hex(wb_structural_m(coast_world, latitude_deg, longitude_deg))
        );
    }

    // And a tile across the shore, because the tile worker is where a coast block lands in
    // the browser: the viewer attaches it to the spec before `TilePool.start` and every
    // worker rebuilds the world from it. A coastline that disagreed between the terrain and
    // the tiles is exactly what a decode difference would look like.
    let shore_tile = {
        // The same box the shore points take, and for the same reason: a 1-degree tile here is
        // entirely inside the coastal band and every one of its 4,225 cells moved under the
        // control. The guard below caught that too.
        let half = COAST_SPAN_DEG / 2.0;
        let (lat0, lat1) = (COAST_LAT + half, COAST_LAT - half);
        let (lon0, lon1) = (COAST_LON - half, COAST_LON + half);
        let (width, height) = (65u32, 65u32);
        let mut tile = vec![0.0f32; 65 * 65];
        let status = wb_fill_tile_f32(
            coast_world,
            lat0,
            lat1,
            lon0,
            lon1,
            width,
            height,
            RES_M,
            tile.as_mut_ptr(),
            width * height,
        );
        assert_eq!(status, WB_OK, "shore: the tile must fill");
        let cells: Vec<String> = tile.iter().map(|v| hex32(*v)).collect();
        println!(
            "T shore {} {} {} {} {width} {height} {} {}",
            hex(lat0),
            hex(lat1),
            hex(lon0),
            hex(lon1),
            hex(RES_M),
            cells.join(" ")
        );
        (lat0, lat1, lon0, lon1, width, height, tile)
    };

    // THE COAST CONTROL'S PREDICTION, PER GROUP, MADE HERE AND CHECKED ON THE OTHER SIDE.
    //
    // `--mutate coast-amplitude` replays the `worldc` records with word 0 set back to
    // canonical's inert value and touches nothing else, so `amplitude` is the only thing that
    // differs. Everything the mutation cannot reach must compare EQUAL, and that list is the
    // informative half: the two `CP` preset groups (a world block cannot move an export that
    // hands back `continentality.rs`' own constants), the `CC` checker group, and every group
    // of every world above -- including both tectonic worlds, whose blocks this mutation does
    // not touch.
    //
    // The counts are computed natively, per group, and `parity.mjs` must meet each of them
    // exactly. Two things hold the prediction to something other than its own output:
    //
    //   1. **The library agrees with the exports.** The same counts are recomputed through
    //      `Surface::elevation_m` / `structural_m` directly rather than through
    //      `wb_elevation_m` / `wb_structural_m`, and the two must be equal. A disagreement
    //      means the export layer adds or hides a difference, and it fails HERE rather than
    //      being absorbed into a divergent tally later.
    //   2. **Both ends refused.** Every group's count must be strictly between zero and the
    //      group's size. A control that moves everything is as uninformative as one that
    //      moves nothing, and this file will not write a corpus where either is true.
    //
    // **No structural-containment claim is made here, unlike the tectonic control's**, and
    // that is deliberate rather than an omission: `CoastParams` reaches `elevation_m` through
    // `Continentality::base_elevation` as well as through the shelf, so an elevation that
    // moves without a structural moving is expected. Claiming the subset the warp satisfies
    // would be claiming something false.
    let (
        control_elevation_fractal,
        control_structural_fractal,
        control_elevation_shore,
        control_structural_shore,
        control_tile_shore,
    ) = {
        // The library side reads its block from `continentality.rs` rather than from the six
        // words that crossed the boundary, which is what makes this a second derivation
        // instead of the same one twice: if `encode_coast` and `decode_coast` disagreed
        // anywhere, the two counts below would part company.
        let on = Surface::with_coast(
            SEED,
            RADIUS_M,
            PLATES as usize, // cast-ok: a corpus-fixed plate count widened to usize
            LAND,
            None,
            None,
            None,
            Some(CoastParams::fractal()),
        );
        let off = Surface::with_coast(
            SEED,
            RADIUS_M,
            PLATES as usize, // cast-ok: as above
            LAND,
            None,
            None,
            None,
            Some(CoastParams { amplitude: CoastParams::canonical().amplitude, ..CoastParams::fractal() }),
        );

        let mut moved = [0usize; 4];
        let mut moved_lib = [0usize; 4];
        for (slot, points) in [(0usize, &coast_scattered), (2, &coast_shore)] {
            for (latitude_deg, longitude_deg) in points {
                let e_on = wb_elevation_m(coast_world, *latitude_deg, *longitude_deg, RES_M);
                let e_off =
                    wb_elevation_m(coast_control_world, *latitude_deg, *longitude_deg, RES_M);
                let s_on = wb_structural_m(coast_world, *latitude_deg, *longitude_deg);
                let s_off = wb_structural_m(coast_control_world, *latitude_deg, *longitude_deg);
                if e_on.to_bits() != e_off.to_bits() {
                    moved[slot] += 1;
                }
                if s_on.to_bits() != s_off.to_bits() {
                    moved[slot + 1] += 1;
                }
                let point = SpherePoint::from_latlon(*latitude_deg, *longitude_deg);
                if on.elevation_m(&point, Some(RES_M)).to_bits()
                    != off.elevation_m(&point, Some(RES_M)).to_bits()
                {
                    moved_lib[slot] += 1;
                }
                if on.structural_m(&point).to_bits() != off.structural_m(&point).to_bits() {
                    moved_lib[slot + 1] += 1;
                }
            }
        }
        assert_eq!(
            moved, moved_lib,
            "the exports and the library disagree about how many values the coast amplitude \
             moves; the six words that crossed the boundary and `CoastParams::fractal()` \
             itself are describing different worlds",
        );

        let (lat0, lat1, lon0, lon1, width, height, on_cells) = shore_tile;
        let mut control_tile = vec![0.0f32; (width * height) as usize]; // cast-ok: a compile-time 65x65 back to a length
        let status = wb_fill_tile_f32(
            coast_control_world,
            lat0,
            lat1,
            lon0,
            lon1,
            width,
            height,
            RES_M,
            control_tile.as_mut_ptr(),
            width * height,
        );
        assert_eq!(status, WB_OK, "shore: the control's tile must fill");
        let tile_moved = on_cells
            .iter()
            .zip(control_tile.iter())
            .filter(|(a, b)| a.to_bits() != b.to_bits())
            .count();

        (moved[0], moved[1], moved[2], moved[3], tile_moved)
    };

    for (label, moved, total) in [
        ("elevation/fractal", control_elevation_fractal, 5_000usize),
        ("structural/fractal", control_structural_fractal, 5_000),
        ("elevation/shore", control_elevation_shore, 2_000),
        ("structural/shore", control_structural_shore, 2_000),
        ("tile/shore", control_tile_shore, 65 * 65),
    ] {
        assert!(
            moved > 0 && moved < total,
            "{label}: the control moved {moved} of {total}. A control that moves everything is \
             as uninformative as one that moves nothing, and this corpus refuses to write \
             either",
        );
    }

    println!(
        "CCTL {control_elevation_fractal} {control_structural_fractal} \
         {control_elevation_shore} {control_structural_shore} {control_tile_shore}"
    );

    // --- the gully channel: presets, checker, a world built from one, and its control ----
    //
    // **A new crossing value with no corpus coverage is a crossing value nothing compares**,
    // which is the sentence the coast rows were written under and is why these rows are in the
    // same commit as the exports they watch, not a slice later.
    //
    // What is not shared between the two sides is the DECODE: ten f64 in linear memory, read
    // back through a raw pointer, bounds-checked field by field, and only then a
    // `GullyParams`. One of those bounds is not politeness -- `WB_MIN_GULLY_SHARPNESS` refuses
    // an exponent at or below zero, and `0^s` at a gully crest is an INFINITE HEIGHT crossing
    // this boundary into a host's vertex buffer. The `GC` records below carry that case.
    let mut gully_canonical = [0.0f64; WB_GULLY_STRIDE];
    let mut gully_drainage = [0.0f64; WB_GULLY_STRIDE];
    let gully_canonical_status = wb_gully_preset(
        WB_GULLY_CANONICAL,
        gully_canonical.as_mut_ptr(),
        WB_GULLY_STRIDE as u32, // cast-ok: a compile-time stride into the export's u32 length
    );
    let gully_drainage_status = wb_gully_preset(
        WB_GULLY_DRAINAGE,
        gully_drainage.as_mut_ptr(),
        WB_GULLY_STRIDE as u32, // cast-ok: as above
    );
    assert_eq!(gully_canonical_status, WB_OK, "the canonical gully preset must be readable");
    assert_eq!(gully_drainage_status, WB_OK, "the drainage gully preset must be readable");
    for (selector, status, record) in [
        (WB_GULLY_CANONICAL, gully_canonical_status, &gully_canonical),
        (WB_GULLY_DRAINAGE, gully_drainage_status, &gully_drainage),
    ] {
        let encoded: Vec<String> = record.iter().map(|v| hex(*v)).collect();
        println!("GP {selector} {status} {}", encoded.join(" "));
    }

    // The control's substitute for word 9, `steer_lattice_m`. **A quarter of the shipped
    // 2,000 m, and inside the domain rather than outside it**: the point of this control is
    // that a plausible value a host could send moves the ground, not that a refused one does.
    // It is carried in the corpus rather than written in `parity.mjs`, so the one number the
    // mutation substitutes arrives like every other input.
    const GULLY_CONTROL_STEER_M: f64 = 500.0;
    const GULLY_STEER_INDEX: usize = 9;

    // Six checker records, three accepted and three refused, asserted here so a checker stuck
    // at either answer cannot pass that group.
    let mut gully_infinite_crest = gully_drainage;
    gully_infinite_crest[5] = -0.5; // the crest-height infinity
    let mut gully_zero_cell = gully_drainage;
    gully_zero_cell[1] = 0.0; // the lattice index the `as i64` saturation lives behind
    let mut gully_negative_amplitude = gully_drainage;
    gully_negative_amplitude[0] = -gully_drainage[0]; // a field that looks configured and inverts the term
    let mut gully_control_record = gully_drainage;
    gully_control_record[GULLY_STEER_INDEX] = GULLY_CONTROL_STEER_M;
    let gully_check_records = [
        ("canonical", gully_canonical),
        ("drainage", gully_drainage),
        ("control", gully_control_record),
        ("infinite-crest", gully_infinite_crest),
        ("zero-cell", gully_zero_cell),
        ("negative-amplitude", gully_negative_amplitude),
    ];
    let mut gully_accepted = 0usize;
    let mut gully_refused = 0usize;
    for (name, record) in &gully_check_records {
        let status = wb_gully_check(
            record.as_ptr(),
            WB_GULLY_STRIDE as u32, // cast-ok: a compile-time stride into the export's u32 length
        );
        if status == WB_OK {
            gully_accepted += 1;
        } else {
            gully_refused += 1;
        }
        let encoded: Vec<String> = record.iter().map(|v| hex(*v)).collect();
        println!("GC {name} {status} {}", encoded.join(" "));
    }
    assert_eq!(gully_accepted, 3, "three of the six gully check records are meant to be accepted");
    assert_eq!(gully_refused, 3, "three of the six gully check records are meant to be refused");

    println!("GSTEER {}", hex(GULLY_CONTROL_STEER_M));
    let gully_encoded: Vec<String> = gully_drainage.iter().map(|v| hex(*v)).collect();
    for name in ["drainage", "flank"] {
        println!(
            "worldg {name} {SEED} {} {PLATES} {} {}",
            hex(RADIUS_M),
            hex(LAND),
            gully_encoded.join(" ")
        );
    }
    let gully_world = wb_world_new_gully(
        SEED,
        RADIUS_M,
        PLATES,
        LAND,
        core::ptr::null(),
        0,
        core::ptr::null(),
        0,
        core::ptr::null(),
        0,
        core::ptr::null(),
        0,
        gully_drainage.as_ptr(),
        WB_GULLY_STRIDE as u32, // cast-ok: a compile-time stride into the export's u32 length
    );
    assert!(gully_world != 0, "the drainage gully world must build");
    let gully_control_world = wb_world_new_gully(
        SEED,
        RADIUS_M,
        PLATES,
        LAND,
        core::ptr::null(),
        0,
        core::ptr::null(),
        0,
        core::ptr::null(),
        0,
        core::ptr::null(),
        0,
        gully_control_record.as_ptr(),
        WB_GULLY_STRIDE as u32, // cast-ok: as above
    );
    assert!(gully_control_world != 0, "the gully control world must build");

    // WHERE THE GATE IS OPEN, AND WHY IT IS NOT A ROUND NUMBER.
    //
    // A uniform scatter over a planet does not land on a flank. `GullyParams::drainage()`'s
    // gate opens over 200-1,100 m of structural ground, and 0.9% of this world is above 800 m.
    // These coordinates are the steepest decile of that -- found by walking 400,000 spiral
    // points on this exact fixture and rounding to a quarter degree, with the rounded site's
    // own `structural_m` re-checked. The concentrated group is what makes the control's report
    // readable; the scattered group is the evidence that the block does NOT reach the rest of
    // the planet.
    const GULLY_LAT: f64 = -8.75;
    const GULLY_LON: f64 = 65.0;
    // **Twenty degrees, not four, and the first attempt at four is why.** A four-degree box on
    // this witness is entirely inside the landmass the flank belongs to, and every one of its
    // 2,000 points moved under the control -- this file's own both-ends-refused guard caught
    // that and refused to write the corpus, exactly as it caught the coast control's first
    // two-degree cut. A group that moves everything is as uninformative as one that moves
    // nothing. Twenty degrees straddles the gate: the high ground, the low ground around it,
    // and the sea beyond that.
    const GULLY_SPAN_DEG: f64 = 20.0;
    {
        let point_on = wb_elevation_m(gully_world, GULLY_LAT, GULLY_LON, RES_M);
        let point_off = wb_elevation_m(plain, GULLY_LAT, GULLY_LON, RES_M);
        let moved = if point_on > point_off { point_on - point_off } else { point_off - point_on };
        assert!(
            moved > 1.0,
            "the flank site must be somewhere the drainage block actually moves the ground; \
             it moved {moved} m, so either the witness is stale or the gate no longer opens \
             there",
        );
    }

    let mut gully_scattered = Vec::with_capacity(5_000);
    for _ in 0..5_000 {
        let latitude_deg = rng.unit() * 180.0 - 90.0;
        let longitude_deg = rng.unit() * 360.0 - 180.0;
        gully_scattered.push((latitude_deg, longitude_deg));
        println!(
            "E drainage {} {} {} {}",
            hex(latitude_deg),
            hex(longitude_deg),
            hex(RES_M),
            hex(wb_elevation_m(gully_world, latitude_deg, longitude_deg, RES_M))
        );
        println!(
            "S drainage {} {} {}",
            hex(latitude_deg),
            hex(longitude_deg),
            hex(wb_structural_m(gully_world, latitude_deg, longitude_deg))
        );
    }

    let mut gully_flank = Vec::with_capacity(2_000);
    for _ in 0..2_000 {
        let latitude_deg = GULLY_LAT + (rng.unit() - 0.5) * GULLY_SPAN_DEG;
        let longitude_deg = GULLY_LON + (rng.unit() - 0.5) * GULLY_SPAN_DEG;
        gully_flank.push((latitude_deg, longitude_deg));
        println!(
            "E flank {} {} {} {}",
            hex(latitude_deg),
            hex(longitude_deg),
            hex(RES_M),
            hex(wb_elevation_m(gully_world, latitude_deg, longitude_deg, RES_M))
        );
        println!(
            "S flank {} {} {}",
            hex(latitude_deg),
            hex(longitude_deg),
            hex(wb_structural_m(gully_world, latitude_deg, longitude_deg))
        );
    }

    // And a tile across the flank, because the tile worker is where a gully block lands in the
    // browser -- and because this is the only block in the corpus whose term is FADED BY
    // RESOLUTION. `Detail::gully_offset_m` drops the whole term when the caller's spacing is
    // coarser than half a stripe wavelength, so a scalar corpus at one resolution cannot see
    // the branch the tiles take.
    let flank_tile = {
        let half = GULLY_SPAN_DEG / 2.0;
        let (lat0, lat1) = (GULLY_LAT + half, GULLY_LAT - half);
        let (lon0, lon1) = (GULLY_LON - half, GULLY_LON + half);
        let (width, height) = (65u32, 65u32);
        let mut tile = vec![0.0f32; 65 * 65];
        let status = wb_fill_tile_f32(
            gully_world,
            lat0,
            lat1,
            lon0,
            lon1,
            width,
            height,
            RES_M,
            tile.as_mut_ptr(),
            width * height,
        );
        assert_eq!(status, WB_OK, "flank: the tile must fill");
        let cells: Vec<String> = tile.iter().map(|v| hex32(*v)).collect();
        println!(
            "T flank {} {} {} {} {width} {height} {} {}",
            hex(lat0),
            hex(lat1),
            hex(lon0),
            hex(lon1),
            hex(RES_M),
            cells.join(" ")
        );
        (lat0, lat1, lon0, lon1, width, height, tile)
    };

    // THE GULLY CONTROL'S PREDICTION, PER GROUP, MADE HERE AND CHECKED ON THE OTHER SIDE.
    //
    // `--mutate gully-steer` replays the `worldg` records with word 9, `steer_lattice_m`, set
    // to 500 m and touches nothing else. That word reaches ONE thing: the world-anchored
    // lattice the gully kernel takes its steering gradient from. It cannot reach
    // `wb_gully_preset` (which hands back `detail.rs`'s own constants), it cannot reach
    // `wb_gully_check` (whose records it does not touch), and it cannot reach any world built
    // without a gully block.
    //
    // **THE STRUCTURAL GROUPS ARE PREDICTED AT EXACTLY ZERO, AND THAT IS THE CLAIM WORTH
    // MAKING.** The gully term is detail; `structural_m` is defined before detail exists and
    // is the very signal the steering lattice reads. If a structural value moved here, the
    // term would have escaped its layer and would be steering on itself -- the recursion
    // `.superpowers/sdd/notes/gradient-probe.md` section 2.4 settled architecturally. So this
    // control's zeros are not an absence of evidence; they are the assertion.
    let (
        control_elevation_drainage,
        control_structural_drainage,
        control_elevation_flank,
        control_structural_flank,
        control_tile_flank,
    ) = {
        // The library side reads its block from `detail.rs` rather than from the ten words
        // that crossed the boundary, which is what makes this a second derivation rather than
        // the same one twice: if `encode_gully` and `decode_gully` disagreed anywhere, the two
        // counts below would part company.
        let on = Surface::with_gully(
            SEED,
            RADIUS_M,
            PLATES as usize, // cast-ok: a corpus-fixed plate count widened to usize
            LAND,
            None,
            None,
            None,
            None,
            Some(GullyParams::drainage()),
        );
        let off = Surface::with_gully(
            SEED,
            RADIUS_M,
            PLATES as usize, // cast-ok: as above
            LAND,
            None,
            None,
            None,
            None,
            Some(GullyParams {
                steer_lattice_m: GULLY_CONTROL_STEER_M,
                ..GullyParams::drainage()
            }),
        );

        let mut moved = [0usize; 4];
        let mut moved_lib = [0usize; 4];
        for (slot, points) in [(0usize, &gully_scattered), (2, &gully_flank)] {
            for (latitude_deg, longitude_deg) in points {
                let e_on = wb_elevation_m(gully_world, *latitude_deg, *longitude_deg, RES_M);
                let e_off =
                    wb_elevation_m(gully_control_world, *latitude_deg, *longitude_deg, RES_M);
                let s_on = wb_structural_m(gully_world, *latitude_deg, *longitude_deg);
                let s_off = wb_structural_m(gully_control_world, *latitude_deg, *longitude_deg);
                if e_on.to_bits() != e_off.to_bits() {
                    moved[slot] += 1;
                }
                if s_on.to_bits() != s_off.to_bits() {
                    moved[slot + 1] += 1;
                }
                let point = SpherePoint::from_latlon(*latitude_deg, *longitude_deg);
                if on.elevation_m(&point, Some(RES_M)).to_bits()
                    != off.elevation_m(&point, Some(RES_M)).to_bits()
                {
                    moved_lib[slot] += 1;
                }
                if on.structural_m(&point).to_bits() != off.structural_m(&point).to_bits() {
                    moved_lib[slot + 1] += 1;
                }
            }
        }
        assert_eq!(
            moved, moved_lib,
            "the exports and the library disagree about how many values the steering lattice \
             moves; the ten words that crossed the boundary and `GullyParams::drainage()` \
             itself are describing different worlds",
        );

        let (lat0, lat1, lon0, lon1, width, height, on_cells) = flank_tile;
        let mut control_tile = vec![0.0f32; (width * height) as usize]; // cast-ok: a compile-time 65x65 back to a length
        let status = wb_fill_tile_f32(
            gully_control_world,
            lat0,
            lat1,
            lon0,
            lon1,
            width,
            height,
            RES_M,
            control_tile.as_mut_ptr(),
            width * height,
        );
        assert_eq!(status, WB_OK, "flank: the control's tile must fill");
        let tile_moved = on_cells
            .iter()
            .zip(control_tile.iter())
            .filter(|(a, b)| a.to_bits() != b.to_bits())
            .count();

        (moved[0], moved[1], moved[2], moved[3], tile_moved)
    };

    for (label, moved, total) in [
        ("elevation/drainage", control_elevation_drainage, 5_000usize),
        ("elevation/flank", control_elevation_flank, 2_000),
        ("tile/flank", control_tile_flank, 65 * 65),
    ] {
        assert!(
            moved > 0 && moved < total,
            "{label}: the control moved {moved} of {total}. A control that moves everything is \
             as uninformative as one that moves nothing, and this corpus refuses to write \
             either",
        );
    }
    // The containment claim, asserted rather than observed. See the comment above.
    assert_eq!(
        control_structural_drainage, 0,
        "a gully block moved structural_m on the scattered points; the term has escaped detail",
    );
    assert_eq!(
        control_structural_flank, 0,
        "a gully block moved structural_m on the flank points; the term has escaped detail",
    );

    println!(
        "GCTL {control_elevation_drainage} {control_structural_drainage} \
         {control_elevation_flank} {control_structural_flank} {control_tile_flank}"
    );

    // --- the hydrology channel: one capped bake, through the shipped export -------------
    //
    // Task 11. `wb_hydro_bake` is the only door onto the hydrology bake across the shipped
    // surface -- before it existed, that bake's native/WASM agreement was unfalsifiable,
    // exactly the sense this file's own doc gives for `wb_erosion_run` and `wb_water_run`.
    //
    // **Controller Ruling C.** This twelve-word params array is a SEPARATE literal from
    // `tests/wasm_exports.rs::hydro_params`, not a shared function: an example cannot see a
    // test module's helpers, so the corpus and that export's own parameter-validation tests
    // each carry their own copy of the fixture. Keep the two equal by inspection if either
    // changes; a silent drift between them would mean the corpus and the unit tests are no
    // longer describing the same bake. Word 0 (the node count) is the one field this array
    // is free to differ from `hydro_params()`'s own -- see the node-count note below.
    //
    // Plan 1b-4 Task 4: raised from 12,000 to 20,000. At 12,000 nodes this world's `H plain`
    // record kept 8 coarse bodies and, after Ruling S-16 raised `earth_like`'s pond density
    // cap, zero ponds -- so `hydro/ranges` was the only parity group ever comparing a body
    // with `shore_member_count == 0` across the native/WASM boundary. `earth_like`'s pond
    // parameters are not wasm params (`hydro_params_from` never sets `pond_search_radius_m`
    // or `pond_density_area_m2`), so every `wb_hydro_bake` call, at any node count, already
    // bakes at the shipped 1,500 m / 1.6e10 pair; only the node count was left to raise.
    // Measured natively at this world's seed (20260904) and land fraction (0.29), same 12
    // middle words: 12,000 nodes -> 8 bodies, 0 ponds, 362 ms; 20,000 -> 13 bodies, 4 ponds,
    // 9 coarse, 526 ms. 20,000 was taken as the smallest of {20,000; 30,000; 50,000} tried
    // (all three keep ponds) because it is closest to the original and the ~164 ms native
    // delta is immaterial against this job's multi-second wall time. See task-4-report.md
    // for the parity job's wall time before and after.
    const HYDRO_PARAMS: [f64; 12] =
        [20_000.0, 500.0, 8.0, 1.0e6, 1.0e6, 3.0e10, 3.0e11, 3.0e12, 1.0, 1.0, 0.1, 0.0];

    let mut hydro_id: u32 = 0;
    let hydro_status =
        wb_hydro_bake(plain, HYDRO_PARAMS.as_ptr(), HYDRO_PARAMS.len() as u32, &mut hydro_id); // cast-ok: a compile-time twelve-word buffer
    assert_eq!(hydro_status, WB_OK, "the hydro bake must succeed for the parity corpus");
    let hydro_len = wb_hydro_len(hydro_id);
    assert!(hydro_len > 0, "a corpus of zero words would compare nothing");
    let mut hydro_words = vec![0.0f64; hydro_len as usize];
    assert_eq!(wb_hydro_copy(hydro_id, hydro_words.as_mut_ptr(), hydro_len), WB_OK);

    let params_hex: Vec<String> = HYDRO_PARAMS.iter().map(|v| hex(*v)).collect();
    let words_hex: Vec<String> = hydro_words.iter().map(|v| hex(*v)).collect();
    println!(
        "H plain {} {} {hydro_status} {hydro_len} {}",
        HYDRO_PARAMS.len(),
        params_hex.join(" "),
        words_hex.join(" ")
    );

    // --- the query channel: §8.3 answered on a fixed grid, through the shipped tile export ----
    //
    // **Plan 2a, Task 6, Step 1.** `wb_water_at` and `wb_water_tile` are the whole of the query
    // across the shipped surface, and before this group their native/WASM agreement was
    // unfalsifiable rather than unverified -- exactly the sense `wb_hydro_bake` above,
    // `wb_erosion_run` and `wb_water_run` are each described in. The tile export is the one used
    // because it is the batch a relief worker actually calls and because its own tests already
    // pin it to `wb_water_at` sample for sample; a scalar group would compare the same arithmetic
    // through a thinner door.
    //
    // **Five words per sample, Ruling Q-18** -- kind, level, depth, body id, reach id -- so the
    // group is `1 + 32 x 32 x 5 = 5,121` values: the tile's own status, then the grid row-major
    // from the north-west with both endpoints included.
    //
    // # How the box was chosen, and why it is a literal
    //
    // It has to contain land, water and at least one recorded body, or the group compares one
    // constant 1,024 times and proves nothing. The box was found by scanning this bake's own
    // record -- every body anchor and every reach midpoint, at seventeen half-widths from 0.05
    // to 8 degrees, 32x32 each -- and scoring the kind histogram by how many kinds appear and how
    // large the smallest of them is. **No box on this bake reaches four kinds**: a river is a few
    // hundred metres wide and a 4-degree box steps about 14 km, so the only boxes that catch one
    // catch a single sample of it, which one re-bake could lose. The most balanced three-kind box
    // was taken instead, on whole degrees:
    //
    //   **31 N, 5 W to 27 N, 1 W -- 692 none, 213 ocean, 119 lake, and recorded body 2.**
    //
    // It is a literal rather than a box derived from the record at run time, deliberately: a
    // derived box would silently follow the bake wherever it went, and the assertions below --
    // which fail the dump rather than writing a corpus that proves nothing -- would never fire.
    const WQ_LAT0: f64 = 31.0;
    const WQ_LON0: f64 = -5.0;
    const WQ_LAT1: f64 = 27.0;
    const WQ_LON1: f64 = -1.0;
    const WQ_ROWS: u32 = 32;
    const WQ_COLUMNS: u32 = 32;
    const WQ_STRIDE: usize = WP_STRIDE; // Ruling Q-18, one statement of the five for both groups
    let wq_samples = (WQ_ROWS as usize) * (WQ_COLUMNS as usize); // cast-ok: two compile-time grid extents
    let mut wq_words = vec![0.0f64; wq_samples * WQ_STRIDE];
    let wq_status = wb_water_tile(
        plain,
        hydro_id,
        WQ_LAT0,
        WQ_LON0,
        WQ_LAT1,
        WQ_LON1,
        WQ_ROWS,
        WQ_COLUMNS,
        wq_words.as_mut_ptr(),
        wq_words.len() as u32, // cast-ok: 5,120, a compile-time bound
    );
    assert_eq!(wq_status, WB_OK, "the query tile must succeed for the parity corpus");

    // The dump refuses to write a corpus that cannot fail, exactly as the coast and gully
    // controls' both-ends-refused guard does -- and that guard has already caught two boxes in
    // this file's history. Here the failure mode is the opposite one: a box entirely inside a
    // lake, or entirely on dry land, compares 1,024 copies of one answer and reports agreement
    // it never tested.
    let mut wq_hist = [0usize; 7];
    let mut wq_bodies = 0usize;
    for sample in 0..wq_samples {
        let kind = wq_words[sample * WQ_STRIDE] as usize; // cast-ok: a kind code, 0..=6 by `water_kind_code`'s own table
        assert!(kind < 7, "the tile wrote a kind code outside §8.3's table: {kind}");
        wq_hist[kind] += 1;
        if wq_words[sample * WQ_STRIDE + 3] != f64::from(u32::MAX) {
            wq_bodies += 1;
        }
    }
    let wq_kinds = wq_hist.iter().filter(|&&n| n > 0).count();
    assert!(
        wq_kinds >= 2,
        "the query box answers one kind for all {wq_samples} samples ({wq_hist:?}); a group that \
         compares one constant proves nothing about the query"
    );
    assert!(
        wq_hist[WATER_KIND_NONE as usize] > 0, // cast-ok: a compile-time kind code, 0
        "the query box holds no dry land ({wq_hist:?}); §8.3's `none` branch would be untested"
    );
    assert!(
        wq_bodies > 0,
        "the query box answers no recorded body ({wq_hist:?}); the body branch, the tie-break and \
         the index's dilation would all be untested"
    );
    println!(
        "WQ plain {} {} {} {} {} {} {} {} {wq_status} {}",
        HYDRO_PARAMS.len(),
        params_hex.join(" "),
        hex(WQ_LAT0),
        hex(WQ_LON0),
        hex(WQ_LAT1),
        hex(WQ_LON1),
        WQ_ROWS,
        WQ_COLUMNS,
        wq_words.iter().map(|v| hex(*v)).collect::<Vec<String>>().join(" ")
    );
    eprintln!(
        "query box {WQ_LAT0},{WQ_LON0} .. {WQ_LAT1},{WQ_LON1}: kinds {wq_kinds}, histogram \
         (none, ocean, lake, salt lake, salt flat, pond, river) {wq_hist:?}, {wq_bodies} samples \
         naming a body"
    );

    assert_eq!(wb_hydro_free(hydro_id), WB_OK);

    // --- Ruling Q-21: the kinds the grid cannot reach, sampled explicitly ---------------------
    //
    // Both bakes are asked, and which one supplies which kind is a measured property of the two
    // records rather than a choice made here. `plain` is asked first because it is the world the
    // `WQ` grid is on; `ranges` is asked because a bake with no pond, no salt lake and no salt
    // flat cannot be made to produce one, and between them the two records cover more.
    let (plain_kinds, _) = print_water_points("plain", plain, &HYDRO_PARAMS);
    // `ranges` was emitted beside `TCTL`, where its own control prediction had to be computed.

    // The ruling's own requirement, asserted across both records rather than within either.
    // `River` and the fine-found branch are the two the drawing path uses most, and they are why
    // this group exists; both bakes hold both, so this is an assertion and not a hope. A kind
    // NEITHER record holds is reported below and is not an error -- there is no bake to take it
    // from, and `BodyKind::Pond` is exactly that case (see `water_points_from`).
    for required in ["River", "FineFound"] {
        assert!(
            plain_kinds.contains(&required) || ranges_kinds.contains(&required),
            "neither parity bake offers a {required} point; Ruling Q-21 exists because that \
             branch does not otherwise cross the native/WASM boundary at all"
        );
    }
    for kind in ["Lake", "SaltLake", "SaltFlat", "Pond", "FineFound", "River"] {
        let plain_has = plain_kinds.contains(&kind);
        let ranges_has = ranges_kinds.contains(&kind);
        if !plain_has && !ranges_has {
            eprintln!(
                "WP coverage: NEITHER bake records a {kind}; §8.3's {kind} branch is not on the \
                 wire in this corpus and is covered by unit tests alone"
            );
        } else {
            eprintln!(
                "WP coverage: {kind} from {}{}{}",
                if plain_has { "plain" } else { "" },
                if plain_has && ranges_has { " and " } else { "" },
                if ranges_has { "ranges" } else { "" }
            );
        }
    }

    // Plan 2b Task 7: the carve on the `plain` world, over `HYDRO_PARAMS` baked for carving.
    let mut carve_plain_params = HYDRO_PARAMS.to_vec();
    carve_plain_params.push(1.0);
    let carve_plain = CarveSpec {
        name: "plain",
        base: plain,
        tectonic: None,
        params: &carve_plain_params,
        block: water_preset_block(),
    };
    let (carve_plain_status, carve_plain_points) = carve_points(&carve_plain, None);
    print_carve(&carve_plain, carve_plain_status, &carve_plain_points);
    let carve_bank_plain =
        carve_bank_divergence(&carve_plain, CARVE_BANK_CONTROL, carve_plain_status, &carve_plain_points);
    for (label, moved, total) in [
        ("carve/ranges", carve_bank_ranges, 1 + carve_ranges_points.len()),
        ("carve/plain", carve_bank_plain, 1 + carve_plain_points.len()),
    ] {
        assert!(moved > 0 && moved < total,
                "{label}: the carve-bank control moved {moved} of {total}; this corpus refuses a \
                 control that moves nothing or everything");
    }
    println!("CBCTL {carve_bank_ranges} {carve_bank_plain}");

    // The `plain` carving record, word for word, beside the carve built from it.
    let (hc_plain_status, hc_plain_len, hc_plain_words) = bake_hydro_native(plain, &carve_plain_params);
    assert_eq!(hc_plain_status, WB_OK, "the plain bake for carving must succeed");
    print_carving_record("plain", &carve_plain_params, hc_plain_status, hc_plain_len, &hc_plain_words);
    check_drain("plain", &hydro_words, &hc_plain_words);

    println!("version {}", wb_generator_version());
}
