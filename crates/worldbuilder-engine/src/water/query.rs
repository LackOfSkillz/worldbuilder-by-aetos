//! Spec §8.3: **what water is at a point** -- ocean, lake, pond, river or none, with its level,
//! its depth, whether it is fresh, and which recorded body it belongs to.
//!
//! # This module's subject, against the other two in `crate::water`
//!
//! `crate::water`'s own items are slice 5b's *bake* (fill a `StreamGraph`'s basins to their spill
//! points); [`crate::water::index`] is plan 2a Task 1's spatial index over a baked
//! `HydroRecord`; this is plan 2a Task 2's *query* over that same record and that same index.
//! Nothing here touches a `StreamGraph`, and nothing here writes anything. The three share one
//! module path because `water.rs` was already `crate::water` when the query arrived (Ruling Q-9).
//!
//! # The record decides, and the ground only says how deep
//!
//! The query never re-runs connectivity, never re-derives an extent and never guesses at a
//! shoreline. It reads what the bake recorded and asks the caller for the ground at the point.
//!
//! # Two grounds, because the bake wrote its levels against two (Rulings Q-3 and Q-16)
//!
//! [`Ground`] carries **both** surfaces, and which one a comparison uses is decided by which one
//! the bake compared against when it wrote the number down.
//!
//! - **Ruling Q-3, the landform** (`Surface::structural_m`), for a coarse body, a reach and the
//!   ocean. Every level, bed and datum crossing in the record is landform-derived, and a query
//!   that asked the detail field instead would put the shoreline wherever the texture noise
//!   happened to cross the level.
//! - **Ruling Q-16, the detail field** (`Surface::bake_ground_m` at `pond_cell_m` -- exactly
//!   `hydrology::ponds::pond_ground`), for a body the §6.6 fine search found. **Ruling S-9 made
//!   that search read the detail field**, because the landform is smooth at 250 m: the deepest
//!   landform dip on the owner's world is 18 mm, so a pond levelled off `structural_m` would have
//!   no depth to stand on. Such a body's `level_m` is therefore a *detail-field* level, and
//!   comparing it against the landform compares two different surfaces. Measured before this
//!   ruling, on the stock populations: **10 of 41 anchors and 144 of 226 ring vertices stood above
//!   their own recorded level**, so the query answered `None` inside most of every pond, its own
//!   deepest point included.
//!
//! **The branch is `shore_member_count == 0`** -- Ruling E-8's discriminator, the same one that
//! chooses between [`body_claim`] and [`pond_claim`] -- and **not `kind`**, because Ruling S-11
//! records an oversized fine-search find as a `lake` while it still carries a traced ring and a
//! detail-field level.
//!
//! **What it costs, stated plainly**, the same cost Ruling S-9 states for the bake: **a pond moves
//! with a detail slider and a lake does not.** Any change to detail amplitude, detail seed or the
//! roughness a feature authorises moves the ground a pond's level is compared against, so a pond's
//! edge shifts where a lake's stays put. That is the price of a pond having any depth at all.
//!
//! # The order the clauses run in, which is not the order §8.3's table lists them
//!
//! The table reads *ocean, body, river, none*, and Ruling Q-5 keeps that precedence. But **the
//! ocean is decided after the bodies are tested**, because Ruling Q-4 says a recorded body beats
//! it: recorded bodies are exactly what carve lakes out of the below-datum set, and the query has
//! no graph to re-run connectivity on. So the shape is
//!
//! > a body claimed it, **else if** the landform is at or below the datum **and no body's extent
//! > held it**, ocean, **else** a reach, **else** none
//!
//! which yields the table's precedence while letting a body at or under the datum -- the owner's
//! inland sea, every enclosed basin -- answer as itself rather than as sea.
//!
//! # A claim and an extent are two different questions (Ruling Q-12)
//!
//! Ruling Q-4 suppresses the ocean where the point is "inside no recorded body's **extent**".
//! Ruling Q-10 makes a **claim** extent *and* level. They are not the same test, and the query
//! tracks them separately: **extent decides whether the ocean is suppressed; claim decides which
//! body answers.**
//!
//! Where they part company is the dry shore of a body recorded below the datum. A salt flat at
//! level −400 m has a shore band in which the landform stands at −100 m: inside the extent, above
//! the body's level, and still under the sea's. Gating the ocean on the claim would answer `Ocean`
//! 100 m deep there and flood an enclosed basin's dry shore with sea. It is inside the salt flat's
//! extent, so it is not sea; it is above the salt flat's level, so it is not water. It is `none`.
//!
//! # Where each clause of §8.3 lives
//!
//! - [`body_claim`] -- the **shore-point set** clause, `shore_member_count > 0`: `dm <= dc` (the
//!   interior) **or** `dm <= shore_reach_m` (the shore band the level contour crosses).
//! - [`pond_claim`] -- the **traced curve** clause, `shore_member_count == 0` whatever the body's
//!   `kind` (Ruling S-11: a lake the fine search found carries a ring like a pond). The ring
//!   **closes implicitly**: `outline[i]` joins `outline[(i + 1) % len]`, and the first point is
//!   never repeated. Ruling Q-17: a point **on** a vertex or an edge is inside.
//! - [`river_claim`] -- within half a reach's width of its centre line, the width of a leg being
//!   the larger of its two endpoints' (Ruling Q-7).
//!
//! Which of the first two runs is decided by `shore_member_count` **and by nothing else** (Ruling
//! E-8); `kind` says what the water *is*, not how its extent is written down.
//!
//! Ruling T1-3 settles more than one claim: the smaller `dm` wins, ties to the lower body id. A
//! pond's `dm` for that purpose is its distance to its own nearest outline point.
//!
//! **A "claim" is both halves of §8.3's row**, extent *and* level (Ruling Q-10): every body row in
//! the table reads "inside ... **and** at or below its level", so a body whose extent holds the
//! point but whose level is under the landform there does not claim it, and does not stand in the
//! way of a body that does. Ranking by `dm` alone, before the level test, would let a dry extent
//! shadow a wet one wherever two overlap -- which is exactly the ridge case Ruling T1-3 exists
//! for. It still counts as an extent for the ocean's purposes; see Ruling Q-12 below.
//!
//! # An answer names its source (Ruling Q-18)
//!
//! A body answer carries `body_id`; a river answer carries `reach_id`. Neither is recoverable from
//! the rest of `WaterAt` -- §9.1 tints a river by its **class**, which lives on the `ReachLine` --
//! and both are already in hand where the answer is built. `NO_BODY` and `NO_REACH` are separate
//! names for the same sentinel value, because the two fields index different tables.
//!
//! # Notches are not a kind
//!
//! The index carries notches because the *water layer* (plan 2b) cuts them into the ground. §8.3's
//! table has no notch row -- a cut channel is answered by the reach that runs through it -- so
//! this query never reads `Candidates::notches`.

use crate::detmath as m;
use crate::hydrology::{Body, BodyKind, HydroRecord, ReachLine};
use crate::sphere::SpherePoint;
use crate::vectors::{Vec3, DEGENERATE, NORTH_AXIS, POLAR_FALLBACK};
use crate::water::index::WaterIndex;

/// `WaterAt::body_id` when the answer belongs to no recorded body: ocean, river and none.
pub const NO_BODY: u32 = u32::MAX;

/// `WaterAt::reach_id` when the answer belongs to no recorded reach -- everything but a river.
/// The same value as [`NO_BODY`], and deliberately a separate name: the two fields index
/// different tables, and a reader who sees one sentinel doing double duty will eventually pass a
/// body id where a reach id belongs.
pub const NO_REACH: u32 = u32::MAX;

/// Sea level. The record's levels and beds are metres against this same zero, and §8.3's ocean
/// clause is "at or below the datum" -- inclusive, so a landform exactly at zero is sea, not land.
const DATUM_M: f64 = 0.0;

/// How far off the query point's own hemisphere a traced ring's vertex may sit before the ring is
/// refused rather than projected. A traced ring outlines a body under a few km across, so a vertex
/// a quarter of the planet away is not a ring at all; the gnomonic projection [`inside_ring`] uses
/// diverges there, and answering `none` is the honest reading of a record that shape.
const RING_MIN_COS: f64 = 1.0e-6;

/// How near a traced ring's vertex or edge a point must be, in [`inside_ring`]'s projected units,
/// to count as standing *on* the ring -- which Ruling Q-17 makes **inside**. Projected units are
/// `tan` of the angle from the query point, so this is about 6.4 micrometres of ground on Earth's
/// radius: four orders above the floating-point noise a coincident vertex leaves behind, and far
/// below anything a 250 m trace could mean by "somewhere else".
const ON_RING_TOL: f64 = 1.0e-12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaterKind { None, Ocean, Lake, SaltLake, SaltFlat, Pond, River }

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaterAt {
    pub kind: WaterKind,
    pub level_m: f64,
    pub depth_m: f64,
    pub fresh: bool,
    /// The body this answer belongs to, or [`NO_BODY`] for ocean, river and none.
    pub body_id: u32,
    /// **Ruling Q-18.** The reach this answer belongs to, or [`NO_REACH`] for everything that is
    /// not a `River`.
    ///
    /// Set because an answer of `River` is otherwise a dead end: spec §9.1 tints a river **by
    /// class**, and `class` lives on the `ReachLine`, so without this a drawing path would have to
    /// re-run `river_claim` over the candidates to discover which reach had answered -- the whole
    /// query, twice. `river_answer` already holds the reach, so carrying its id costs nothing
    /// there.
    ///
    /// It costs 8 bytes a sample (`WaterAt` measures 24 bytes without it and 32 with), and it
    /// costs Ruling Q-8's tile batch a fifth word per sample. That second cost is why it is here
    /// now rather than later: the stride is Task 4's contract with the relief workers and the
    /// viewer's reader, and widening it before it ships is a decision, while widening it after is
    /// an argument with two other files.
    pub reach_id: u32,
}

impl WaterAt {
    /// Dry ground: no water, no level, no depth, no body, no reach.
    pub fn none() -> WaterAt {
        WaterAt {
            kind: WaterKind::None,
            level_m: 0.0,
            depth_m: 0.0,
            fresh: false,
            body_id: NO_BODY,
            reach_id: NO_REACH,
        }
    }
}

/// `Surface::structural_m` -- the landform, with painted features and **without** the detail
/// field. Ruling Q-3. Used for every coarse body, every reach and the ocean datum.
///
/// A newtype rather than a bare closure so it cannot be passed where [`Detail`] belongs. Both
/// surfaces are `&dyn Fn(&SpherePoint) -> f64`, so as two fields of the same type they could be
/// bound backwards at any call site and nothing -- not the compiler, not the query, not a test
/// with one flat surface -- would say so. The failure is silent and it is not small: ponds read
/// dry and lake edges wander with the detail slider.
pub struct Landform<'a>(pub &'a dyn Fn(&SpherePoint) -> f64);

/// `Surface::bake_ground_m(point, Some(pond_cell_m))` -- the landform **plus** the detail field, at
/// the fine search's own cell size, and **without** the water layer (Ruling C-9). This is exactly
/// `hydrology::ponds::pond_ground`, and passing anything else means comparing a fine-found body's
/// level against a surface it was never levelled from -- `elevation_m` included, which on a carved
/// world has a channel cut into it. Ruling Q-16. Used only for a body with
/// `shore_member_count == 0`.
///
/// A newtype for the same reason as [`Landform`]; see there.
pub struct Detail<'a>(pub &'a dyn Fn(&SpherePoint) -> f64);

impl Landform<'_> {
    pub fn at(&self, point: &SpherePoint) -> f64 {
        (self.0)(point)
    }
}

impl Detail<'_> {
    pub fn at(&self, point: &SpherePoint) -> f64 {
        (self.0)(point)
    }
}

/// The two surfaces the query compares a recorded level against, named rather than positional and
/// typed so a caller **cannot** pass them the wrong way round. `refine::Ground` is the shape this
/// follows.
///
/// See the module header for the whole argument. In short: the bake wrote most of its numbers
/// against the landform (Ruling Q-3) and a fine-search body's level against the detail field
/// (Ruling S-9), so the query has to ask each question of the surface that answered it.
pub struct Ground<'a> {
    pub landform_m: Landform<'a>,
    pub detail_m: Detail<'a>,
}

/// Spec §8.3. `ground` carries **both** surfaces (Rulings Q-3 and Q-16): the landform for coarse
/// bodies, reaches and the ocean, and the detail field at `pond_cell_m` for a body the §6.6 fine
/// search found -- the same field that found it. The module header says why the two differ, and
/// what it costs: a pond moves with a detail slider, and a lake does not.
///
/// `index` must be [`WaterIndex::build`]'s output over this same `record`; it supplies both the
/// candidate lists and the planet radius the record's metres are measured on.
pub fn water_at(
    record: &HydroRecord,
    index: &WaterIndex,
    ground: &Ground,
    point: &SpherePoint,
) -> WaterAt {
    let radius_m = index.radius_m();
    let landform = ground.landform_m.at(point);
    let candidates = index.candidates(point);

    // Bodies first, though the table lists the ocean first: Ruling Q-4, a recorded body's extent
    // beats the ocean's. See [`claim_bodies`] for the claim and for Ruling Q-12's two questions.
    let BodyClaims { best, in_an_extent } =
        claim_bodies(record, candidates.bodies, ground, point, radius_m, landform);
    if let Some((_, body, here)) = best {
        return body_answer(body, here);
    }

    // "a body claimed it, else if the landform is at or below the datum AND no body's extent held
    // it, ocean" (Rulings Q-4 and Q-12). The datum is a landform crossing: the coastline is locked
    // to `structural_m` (spec §11) and never moves with the texture, whatever a pond does.
    if landform <= DATUM_M && !in_an_extent {
        return WaterAt {
            kind: WaterKind::Ocean,
            level_m: DATUM_M,
            depth_m: DATUM_M - landform,
            fresh: false,
            body_id: NO_BODY,
            reach_id: NO_REACH,
        };
    }

    // Then the reaches. Nearest centre line wins, ties to the lower reach id -- the same
    // "smallest distance, then lowest id" shape Ruling T1-3 fixes for bodies, so that a point a
    // confluence puts inside two channels does not answer by the order a cell lists them in.
    let mut best_reach: Option<(f64, &ReachLine)> = None;
    for &id in candidates.reaches {
        let Some(reach) = reach_by_id(record, id) else {
            continue;
        };
        let Some(d) = river_claim(reach, point, radius_m) else {
            continue;
        };
        best_reach = Some(match best_reach {
            None => (d, reach),
            Some((best_d, held)) => {
                if d < best_d || (d == best_d && reach.id < held.id) {
                    (d, reach)
                } else {
                    (best_d, held)
                }
            }
        });
    }
    if let Some((_, reach)) = best_reach {
        return river_answer(reach, point, radius_m);
    }

    WaterAt::none()
}

// ---- the body clauses -------------------------------------------------------------------

/// What the recorded bodies say about `point`: **which body claims it**, if any, and whether it
/// stands **inside any body's extent** at all -- §8.3's body rows, and Ruling Q-12's two separate
/// questions. `bodies` is the index's candidate list here; `landform` is `ground.landform_m` at
/// `point`, already read by the caller.
///
/// **Shared with the water layer**, which asks it whether a point is in a lake before cutting a
/// channel there (spec §8.1, "lake beds: not cut"): "this is a lake" and "this is not a channel"
/// are one test, so the carve can never cut a place the query answers as standing water.
pub(crate) fn claim_bodies<'r>(
    record: &'r HydroRecord,
    bodies: &[u32],
    ground: &Ground,
    point: &SpherePoint,
    radius_m: f64,
    landform: f64,
) -> BodyClaims<'r> {
    // Ruling T1-3 picks between claimants -- smaller `dm`, ties to lower id.
    //
    // Ruling Q-12: **two different questions, tracked separately.** `in_an_extent` is "some
    // candidate body's extent held this point", which is what suppresses the ocean (Q-4's own
    // words: "at or below the datum AND inside no recorded body's extent"). `best` is "some body
    // claimed it", which is extent AND level (Q-10), and is what decides which body answers. Where
    // the two part company -- the dry shore of a body recorded below the datum, where the landform
    // is above the body's level and still under the sea's -- gating the ocean on the *claim* would
    // flood an enclosed basin's dry shore. It is inside the salt flat's extent, so it is not sea;
    // it is above the salt flat's level, so it is not water either. It is `none`.
    //
    // Ruling Q-16's second surface, read only if a ring body is actually a candidate here. Most
    // samples never touch one, and a detail sample is the more expensive of the two.
    let mut detail: Option<f64> = None;
    let mut in_an_extent = false;
    let mut best: Option<(f64, &'r Body, f64)> = None;
    for &id in bodies {
        let Some(body) = body_by_id(record, id) else {
            continue; // an index built over a different record; refuse it, never index blindly
        };
        let Some(dm) = extent_claim(body, point, radius_m) else {
            continue;
        };
        in_an_extent = true;
        // Ruling Q-16: this body's level was written against one of the two surfaces, and the
        // comparison has to use that one. `shore_member_count == 0` is the discriminator (Ruling
        // E-8), and it is the same one `extent_claim` just used above.
        let here = if body.shore_member_count == 0 {
            *detail.get_or_insert_with(|| ground.detail_m.at(point))
        } else {
            landform
        };
        // "and at or below its level" -- the table's own second half, for every body row. A body
        // that fails it is still an extent for Q-4's purposes, and simply does not claim.
        if here > body.level_m {
            continue;
        }
        best = Some(match best {
            None => (dm, body, here),
            Some((best_dm, held, held_here)) => {
                if dm < best_dm || (dm == best_dm && body.id < held.id) {
                    (dm, body, here)
                } else {
                    (best_dm, held, held_here)
                }
            }
        });
    }
    BodyClaims { best, in_an_extent }
}

/// [`claim_bodies`]'s answer: the claiming body as `(dm, body, the ground its level was compared
/// against)`, and whether any candidate's extent held the point.
pub(crate) struct BodyClaims<'r> {
    pub(crate) best: Option<(f64, &'r Body, f64)>,
    pub(crate) in_an_extent: bool,
}


/// What a claiming body answers: its own kind and level, its own `fresh`, and a depth that is the
/// level minus the ground, floored at zero (Ruling Q-6 -- a *ground* depth, not a bathymetric one,
/// so it varies across the body rather than reporting one number for the whole thing).
///
/// `ground_m` is whichever of [`Ground`]'s two surfaces this body's level was written against
/// (Ruling Q-16): the landform for a coarse body, the detail field for a fine-search one. Passing
/// the other would report a depth measured from a surface the level never met.
fn body_answer(body: &Body, ground_m: f64) -> WaterAt {
    let drop = body.level_m - ground_m;
    let depth_m = if drop > 0.0 { drop } else { 0.0 };
    let kind = match body.kind {
        BodyKind::Lake => WaterKind::Lake,
        BodyKind::Pond => WaterKind::Pond,
        BodyKind::SaltLake => WaterKind::SaltLake,
        BodyKind::SaltFlat => WaterKind::SaltFlat,
    };
    WaterAt {
        kind,
        level_m: body.level_m,
        depth_m,
        fresh: body.fresh,
        body_id: body.id,
        reach_id: NO_REACH, // Ruling Q-18: standing water belongs to no reach
    }
}

/// Ruling E-8: `shore_member_count`, and nothing else, says which clause a body's extent takes.
/// Returns the `dm` Ruling T1-3 ranks claimants by, or `None` if the point is outside.
fn extent_claim(body: &Body, point: &SpherePoint, radius_m: f64) -> Option<f64> {
    if body.shore_member_count == 0 {
        pond_claim(body, point, radius_m)
    } else {
        body_claim(body, point, radius_m)
    }
}

/// §8.3's **shore-point set** clause. `dm` is the distance to the nearest shore *member*, `dc` to
/// the nearest *collar* point (infinite when a body has no collar); the point is inside if
/// `dm <= dc` **or** `dm <= shore_reach_m`.
///
/// The first clause holds the interior. The second holds the shore band: the level contour crosses
/// every member-to-collar step whose collar end stands above the level, and `shore_reach_m` is the
/// longest such step, so a band that wide holds the whole contour inside the extent. 26 of 1,042
/// measured bodies carry a band of exactly zero, and clause 1 answers those alone.
///
/// Ties go to the lower outline index, which a strict `<` in each running minimum gives: the first
/// point scanned at a given distance keeps it.
fn body_claim(body: &Body, point: &SpherePoint, radius_m: f64) -> Option<f64> {
    let members = body.shore_member_count as usize; // cast-ok: a recorded count, not a float
    let mut dm = f64::INFINITY;
    let mut dc = f64::INFINITY;
    for (i, &(lat, lon)) in body.outline.iter().enumerate() {
        let d = point.distance_to(&SpherePoint::from_latlon(lat, lon), radius_m);
        if i < members {
            if d < dm {
                dm = d;
            }
        } else if d < dc {
            dc = d;
        }
    }
    if !dm.is_finite() {
        return None; // no member to measure from: this clause has nothing to say
    }
    if dm <= dc || dm <= body.shore_reach_m {
        Some(dm)
    } else {
        None
    }
}

/// §8.3's **traced curve** clause, for a pond and for any body with `shore_member_count == 0`.
/// The outline's points are joined in order and **the ring closes implicitly** -- `outline[i]` to
/// `outline[(i + 1) % len]`, with the first point never repeated. `shore_reach_m` is not consulted.
///
/// The `dm` handed back for Ruling T1-3's tie-break is the distance to the ring's nearest point,
/// as §8.3 says in as many words.
fn pond_claim(body: &Body, point: &SpherePoint, radius_m: f64) -> Option<f64> {
    if !inside_ring(&body.outline, point) {
        return None;
    }
    let mut dm = f64::INFINITY;
    for &(lat, lon) in &body.outline {
        let d = point.distance_to(&SpherePoint::from_latlon(lat, lon), radius_m);
        if d < dm {
            dm = d;
        }
    }
    if dm.is_finite() { Some(dm) } else { None }
}

/// Is `point` inside the closed ring `outline`?
///
/// **Gnomonic about the query point, then an even-odd crossing count.** Every vertex is projected
/// along its own direction onto the plane tangent at `point`, in that point's own east/north
/// frame; the projection carries great-circle arcs to straight lines exactly, so the ring's edges
/// stay edges and the query point sits at the origin. A ray along +east from the origin then
/// crosses the boundary an odd number of times iff the point is inside -- which is the test that
/// gets a **concave** ring right, where a bounding box or a convex hull does not.
///
/// **The boundary is inside** (Ruling Q-17): a point on a vertex or on an edge is tested for
/// explicitly and answers `true` before the ray runs, because a vertex projects onto the ray's own
/// origin, where a crossing count is at its least decisive.
///
/// A vertex at or behind the horizon (`RING_MIN_COS`) has no gnomonic image, and the whole ring is
/// refused rather than half-projected. A traced ring outlines a body under a few km across, so
/// that cannot arise from a record this query is meant to answer.
fn inside_ring(outline: &[(f64, f64)], point: &SpherePoint) -> bool {
    if outline.len() < 3 {
        return false; // fewer than three points bound no area
    }
    let centre = point.vector;
    let mut axis = NORTH_AXIS.cross(&centre);
    if axis.length() < DEGENERATE {
        axis = POLAR_FALLBACK.cross(&centre); // at a pole, east has no meaning; pick the same one
    }
    let Some(east) = axis.normalised() else {
        return false;
    };
    let north = centre.cross(&east);

    let mut planar: Vec<(f64, f64)> = Vec::with_capacity(outline.len());
    for &(lat, lon) in outline {
        let v: Vec3 = SpherePoint::from_latlon(lat, lon).vector;
        let towards = v.dot(&centre);
        if towards < RING_MIN_COS {
            return false;
        }
        let scale = 1.0 / towards;
        planar.push((v.dot(&east) * scale, v.dot(&north) * scale));
    }

    // Ruling Q-17: **the boundary is inside, and it is decided here rather than left to the ray.**
    // A point projected onto a vertex lands on the ray's own origin, where `(ay > 0.0) != (by > 0.0)`
    // reads it as below the ray and the crossing count answers arbitrarily -- 88 recorded ring
    // vertices were claimed by nothing on the stock populations, which is deterministic but is not
    // a decision anybody made. So: on a vertex or on an edge, inside, before the ray runs at all.
    //
    // The tolerance is in projected units, which are `tan` of the angle from the query point, so
    // 1e-12 is about 6.4 micrometres of ground on Earth's radius -- four orders above the ~1e-16
    // noise a coincident vertex leaves in the dot products, and far below anything a 250 m trace
    // could mean by "a different place".
    for &(x, y) in &planar {
        if x * x + y * y <= ON_RING_TOL * ON_RING_TOL {
            return true; // standing on a recorded vertex
        }
    }
    for i in 0..planar.len() {
        let (ax, ay) = planar[i];
        let (bx, by) = planar[(i + 1) % planar.len()];
        let (ex, ey) = (bx - ax, by - ay);
        let span = ex * ex + ey * ey;
        if span <= 0.0 {
            continue; // a repeated vertex: no edge to stand on, and the vertex test covered it
        }
        // Where along the edge the perpendicular from the origin falls. Off either end, the
        // nearest point of the edge is an endpoint, which the vertex loop already tested.
        let t = -(ax * ex + ay * ey) / span;
        if t < 0.0 || t > 1.0 {
            continue;
        }
        let (fx, fy) = (ax + t * ex, ay + t * ey);
        if fx * fx + fy * fy <= ON_RING_TOL * ON_RING_TOL {
            return true; // standing on an edge
        }
    }

    let mut inside = false;
    for i in 0..planar.len() {
        let (ax, ay) = planar[i];
        let (bx, by) = planar[(i + 1) % planar.len()]; // the ring closes implicitly
        if (ay > 0.0) != (by > 0.0) {
            // The edge straddles the ray's line. Where does it cross, east or west of the origin?
            let crossing = ax + (0.0 - ay) / (by - ay) * (bx - ax);
            if crossing > 0.0 {
                inside = !inside;
            }
        }
    }
    inside
}

// ---- the river clause ---------------------------------------------------------------------

/// §8.3's **river** clause: within half a reach's width of its centre line. Ruling Q-7 -- a leg's
/// width is the **larger** of its two endpoints', because a leg tapers between recorded points and
/// the smaller value would answer `none` inside a channel the carve will cut.
///
/// Returns the distance to the nearest claiming leg's centre line, or `None`.
fn river_claim(reach: &ReachLine, point: &SpherePoint, radius_m: f64) -> Option<f64> {
    let points = &reach.points;
    if points.is_empty() {
        return None;
    }
    if points.len() == 1 {
        let d = point.distance_to(&reach_at(reach, 0), radius_m);
        return if d <= half_of(points[0].width_m) { Some(d) } else { None };
    }
    let mut best: Option<f64> = None;
    for leg in 0..points.len() - 1 {
        let wider = leg_width_m(points[leg].width_m, points[leg + 1].width_m);
        let d = distance_to_leg_m(point, &reach_at(reach, leg), &reach_at(reach, leg + 1), radius_m);
        if d <= half_of(wider) {
            best = Some(match best {
                None => d,
                Some(held) => if d < held { d } else { held },
            });
        }
    }
    best
}

/// What a claiming reach answers. Ruling Q-6: the depth is the reach's own `depth_m` **at the
/// nearest recorded point**, and the level is that same point's `bed_m + depth_m` -- not an
/// interpolation along the leg, which §8.3 does not ask for and the tile batch (Ruling Q-8) is
/// explicit about not doing. A river belongs to no body.
fn river_answer(reach: &ReachLine, point: &SpherePoint, radius_m: f64) -> WaterAt {
    let mut nearest = 0usize;
    let mut best = f64::INFINITY;
    for i in 0..reach.points.len() {
        let d = point.distance_to(&reach_at(reach, i), radius_m);
        if d < best {
            best = d;
            nearest = i; // ties to the lower index: a strict `<` keeps the first one scanned
        }
    }
    let rp = &reach.points[nearest];
    WaterAt {
        kind: WaterKind::River,
        level_m: rp.bed_m + rp.depth_m,
        depth_m: rp.depth_m,
        fresh: reach.fresh,
        body_id: NO_BODY,
        // Ruling Q-18: the one branch that names a reach. Which reach answered is not recoverable
        // from anything else in `WaterAt`, and §9.1's drawing needs it to reach `ReachLine::class`.
        reach_id: reach.id,
    }
}

fn reach_at(reach: &ReachLine, i: usize) -> SpherePoint {
    SpherePoint::from_latlon(reach.points[i].lat_deg, reach.points[i].lon_deg)
}

// ---- leg geometry, shared with the water layer ------------------------------------------------
//
// **The query's "this is a river" and the carve's "this is a channel" are one test.** Plan 2b's
// water layer (`water::layer`) cuts a channel wherever this module would answer `River`, and it
// finds that place with the functions below rather than with a copy of them: the same Ruling Q-7
// width, the same half of it, the same distance to the same arc. A second version would be a
// second place for the channel to be.

/// Ruling Q-7: a leg's width is the **larger** of its two endpoints', because a leg tapers between
/// recorded points and the smaller value would answer `none` inside a channel the carve cuts.
pub(crate) fn leg_width_m(width_a_m: f64, width_b_m: f64) -> f64 {
    if width_b_m > width_a_m { width_b_m } else { width_a_m }
}

/// Half a width, and zero for a width that is negative or not a number -- `index.rs`'s own rule,
/// so the index and the query agree on what a malformed width means.
pub(crate) fn half_of(width_m: f64) -> f64 {
    if width_m > 0.0 { width_m * 0.5 } else { 0.0 }
}

/// The great-circle distance from `point` to the arc `a`-`b`: the cross-track distance when the
/// foot of the perpendicular falls between the ends, and the nearer end otherwise.
///
/// Whether the foot falls between them is decided by two sign tests against the arc's own normal
/// -- `(a x f) . n >= 0` and `(f x b) . n >= 0` -- rather than by comparing angles, so no
/// transcendental is spent deciding it. Two coincident or antipodal ends have no arc to be
/// perpendicular to and fall back to the ends.
fn distance_to_leg_m(point: &SpherePoint, a: &SpherePoint, b: &SpherePoint, radius_m: f64) -> f64 {
    leg_foot(point, a, b, radius_m).0
}

/// [`distance_to_leg_m`], and **how far along the leg its answer was measured**: `0` at `a`, `1`
/// at `b`. The distance is computed here and nowhere else, so the query and the carve cannot
/// disagree about it.
///
/// The fraction needs no transcendental either. `before` and `after` are the two sign tests'
/// own values, and on unit vectors they are `|a x b|` times the sines of the angles `a`-foot and
/// foot-`b`; their ratio is therefore the foot's position by the sines of its two angles, which
/// is exact at both ends and monotone along the leg, and it departs from the angular fraction
/// only by the difference between an angle and its sine -- second order in a leg a few hundred
/// metres long on a planet thousands of kilometres round. Where the foot falls off the leg, the
/// distance is to the nearer end and so is the fraction: `0` for `a`, `1` for `b`, and `b` on a
/// tie, because `ends` below keeps `db` when `da == db`.
pub(crate) fn leg_foot(point: &SpherePoint, a: &SpherePoint, b: &SpherePoint, radius_m: f64)
                       -> (f64, f64) {
    let da = point.distance_to(a, radius_m);
    let db = point.distance_to(b, radius_m);
    let ends = if da < db { (da, 0.0) } else { (db, 1.0) };

    let across = a.vector.cross(&b.vector);
    if across.length() < DEGENERATE {
        return ends;
    }
    let Some(normal) = across.normalised() else {
        return ends;
    };
    let off_plane = point.vector.dot(&normal);
    let Some(foot) = point.vector.sub(&normal.scaled(off_plane)).normalised() else {
        return ends; // the point is a pole of this arc; every point on it is equally far
    };
    let before = a.vector.cross(&foot).dot(&across);
    let after = foot.cross(&b.vector).dot(&across);
    if before < 0.0 || after < 0.0 {
        return ends;
    }
    // `asin` of the sine of the angle off the plane. Written as an explicit branch rather than
    // `.abs()`, and capped rather than `.clamp(`ed, per the house rules.
    let magnitude = if off_plane < 0.0 { -off_plane } else { off_plane };
    let capped = if magnitude > 1.0 { 1.0 } else { magnitude };
    let span = before + after;
    let along = if span > 0.0 { before / span } else { 0.0 };
    (m::asin(capped) * radius_m, along)
}

// ---- resolving what the index handed back ----------------------------------------------------

/// The body with this **id**. `Candidates::bodies` carries `Body::id`, and this query resolves it
/// as an id, not as a position: the position is tried first only as a fast path, and is used only
/// when the body sitting there actually carries that id. A record whose ids and positions disagree
/// -- or an index built over a different record entirely -- answers correctly or answers nothing,
/// and never indexes blindly.
fn body_by_id(record: &HydroRecord, id: u32) -> Option<&Body> {
    let position = id as usize; // cast-ok: a recorded id used as a position, verified on the line below
    if let Some(body) = record.bodies.get(position) {
        if body.id == id {
            return Some(body);
        }
    }
    record.bodies.iter().find(|body| body.id == id)
}

/// The reach with this **id**, on the same terms as [`body_by_id`].
fn reach_by_id(record: &HydroRecord, id: u32) -> Option<&ReachLine> {
    reach_position(record, id).map(|position| &record.reaches[position])
}

/// Where in `record.reaches` the reach with this **id** sits, on [`reach_by_id`]'s terms -- the
/// position tried first as a fast path, then a scan for the first reach carrying the id. Shared
/// with the water layer, which keeps each reach's points pre-projected by position and must
/// resolve an index candidate to the same reach the query would.
pub(crate) fn reach_position(record: &HydroRecord, id: u32) -> Option<usize> {
    let position = id as usize; // cast-ok: a recorded id used as a position, verified on the line below
    if let Some(reach) = record.reaches.get(position) {
        if reach.id == id {
            return Some(position);
        }
    }
    record.reaches.iter().position(|reach| reach.id == id)
}

// ---- tests -----------------------------------------------------------------------------

/// `pub(crate)` for [`stats`] alone: `water::layer`'s tests drive hand-written records too, and
/// a second copy of that forty-field fixture would be a second thing to keep in step.
#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::hydrology::{
        BakeStats, Body, BodyKind, Downstream, HydroParams, HydroRecord, ReachClass, ReachLine,
        ReachPoint,
    };
    use crate::sphere::SpherePoint;
    use crate::water::index::{WaterIndex, DEFAULT_CELL_M};

    const R: f64 = 6_371_000.0;
    /// Metres per degree of latitude on `R`: 111,194.93 m.
    const M_PER_DEG: f64 = core::f64::consts::PI * R / 180.0;

    /// The band body 0 and the tie-break bodies carry. Wide enough that the probes below sit
    /// well inside or well outside it, never within rounding of its edge.
    const BAND_M: f64 = 25_000.0;
    const WIDE_BAND_M: f64 = 60_000.0;

    fn at(lat: f64, lon: f64) -> SpherePoint {
        SpherePoint::from_latlon(lat, lon)
    }

    /// A constant surface, for building either half of a [`Ground`].
    fn flat(height_m: f64) -> impl Fn(&SpherePoint) -> f64 {
        move |_: &SpherePoint| height_m
    }

    /// Drive a query with one height standing for **both** surfaces. Right for every test that
    /// does not care which of the two a body reads -- which is every one but
    /// `a_ring_bodys_level_is_read_off_the_detail_field`, where they deliberately differ.
    fn ask_with(record: &HydroRecord, index: &WaterIndex, height_m: f64, point: &SpherePoint)
                -> WaterAt {
        let same = flat(height_m);
        water_at(record, index, &Ground { landform_m: Landform(&same), detail_m: Detail(&same) }, point)
    }

    fn body(id: u32, kind: BodyKind, fresh: bool, level_m: f64, shore_member_count: u32,
            shore_reach_m: f64, outline: Vec<(f64, f64)>) -> Body {
        Body {
            id,
            kind,
            fresh,
            enclosed: false,
            forced: false,
            level_m,
            area_m2: 1.0e6,
            depth_m: 5.0,
            outlet_reach: None,
            anchor: outline[0],
            outline,
            downstream: Downstream::Ocean,
            shore_member_count,
            shore_reach_m,
        }
    }

    fn reach_point(lat: f64, lon: f64, bed_m: f64, width_m: f64, depth_m: f64) -> ReachPoint {
        ReachPoint { lat_deg: lat, lon_deg: lon, bed_m, width_m, depth_m, flow_m2: 1.0 }
    }

    /// Every count zero: a `HydroRecord` driven directly, with no bake behind it. The query
    /// reads nothing from `stats`.
    pub(crate) fn stats() -> BakeStats {
        let p = HydroParams::earth_like(1_000);
        BakeStats {
            nodes: 0, land_nodes: 0, hollows: 0, kept: 0, notched: 0, closed: 0,
            streams: 0, rivers: 0, great: 0, max_order: 0,
            bifurcation_min: 0.0, bifurcation_max: 0.0,
            stream_flow_m2: p.stream_flow_m2, river_flow_m2: p.river_flow_m2,
            great_flow_m2: p.great_flow_m2,
            total_nodes: p.total_nodes, wetness_nodes: p.wetness_nodes,
            keep_depth_m: p.keep_depth_m, keep_area_m2: p.keep_area_m2,
            pond_max_area_m2: p.pond_max_area_m2, keep_max_area_m2: p.keep_max_area_m2,
            min_stream_nodes: p.min_stream_nodes, notch_fall_m: p.notch_fall_m,
            evaporation_factor: p.evaporation_factor, salt_flat_share: p.salt_flat_share,
            forced_requested: 0, forced_matched: 0,
            capped_basins: 0, capped_inner: 0, capped_inner_kept: 0,
            refine_step_m: p.refine_step_m, refine_simplify_m: p.refine_simplify_m,
            refine_vertical_m: p.refine_vertical_m,
            fall_min_drop_m: p.fall_min_drop_m, fall_max_run_m: p.fall_max_run_m,
            meander_wavelength_widths: p.meander_wavelength_widths,
            meander_amplitude_widths: p.meander_amplitude_widths,
            meander_max_slope: p.meander_max_slope,
            crossings_coarse: 0, crossings_left: 0,
            ponds_found: 0, ponds_kept: 0,
            pond_cell_m: p.pond_cell_m, pond_search_radius_m: p.pond_search_radius_m,
            pond_keep_depth_m: p.pond_keep_depth_m, pond_keep_area_m2: p.pond_keep_area_m2,
            pond_wetness_share: p.pond_wetness_share, pond_max_slope: p.pond_max_slope,
            pond_density_area_m2: p.pond_density_area_m2,
            shore_members: 0, collar_points: 0,
        }
    }

    /// A pond's traced ring, **concave**: a square with a V notched up into its southern edge,
    /// apex at the middle. Written as (lat, lon), so read the second word as x and the first as
    /// y. The ring closes implicitly -- the first point is not repeated.
    ///
    /// ```text
    ///   10.04  V1----------------V2
    ///          |                  |
    ///   10.02  |        V4        |
    ///          |       /  \       |
    ///   10.00  V0----'      '----V3
    ///        10.00     10.02    10.04
    /// ```
    ///
    /// A point just north of the southern edge and west of the apex -- (10.005, 10.015) -- is
    /// **outside**, because the boundary there is the V's western arm, which at longitude
    /// 10.015 stands at latitude 10.015. A bounding box, a convex hull, or a crossing count
    /// that misses the implicit closing edge all put it inside.
    fn pond_ring() -> Vec<(f64, f64)> {
        vec![(10.00, 10.00), (10.04, 10.00), (10.04, 10.04), (10.00, 10.04), (10.02, 10.02)]
    }

    /// Body 0's shore-point extent: two members on the equator, and a collar 0.4 deg (44,478 m)
    /// north of each, against a 25,000 m band.
    ///
    /// **The collar sits beyond the band on purpose.** Ruling E-3 measures the band from steps
    /// whose collar end stands *above* the level, and a body has plenty of steps that do not
    /// qualify, so a collar point further out than `shore_reach_m` is the ordinary case rather
    /// than a contrived one. It also keeps the extent narrower than the candidate set, so a probe can
    /// be **outside the extent and still a candidate** and brief case 3 has somewhere to stand --
    /// the index offers every body whose bounding circle reaches the cell (Ruling Q-13), and that
    /// circle is the whole outline plus the band, wider than the extent by construction.
    fn shore_body() -> Body {
        body(0, BodyKind::Lake, true, 100.0, 2, BAND_M,
             vec![(0.0, 0.0), (0.0, 0.1), (0.4, 0.0), (0.4, 0.1)])
    }

    /// The whole fixture. Nine bodies and two reaches, each far enough from the rest that no
    /// probe can be answered by the wrong one; ids are assigned to match positions here, and
    /// `a_record_whose_ids_are_not_positions_still_answers_by_id` proves the query does not
    /// rely on that.
    fn fixture() -> HydroRecord {
        let bodies = vec![
            // 0: the shore-point lake -- tests 1 to 4.
            shore_body(),
            // 1: the pond's traced ring -- test 7.
            body(1, BodyKind::Pond, true, 50.0, 0, 0.0, pond_ring()),
            // 2, 3: the salt bodies -- test 11.
            body(2, BodyKind::SaltLake, false, 100.0, 1, BAND_M, vec![(60.0, 0.0), (60.3, 0.0)]),
            body(3, BodyKind::SaltFlat, false, 100.0, 1, BAND_M, vec![(70.0, 0.0), (70.3, 0.0)]),
            // 4: the zero band -- test 5. Its collar is 0.02 deg (2,224 m) from its member, so a
            // point between them is close enough that any band at all would admit it.
            body(4, BodyKind::Lake, true, 100.0, 1, 0.0, vec![(30.0, 0.0), (30.02, 0.0)]),
            // 5, 6: two bodies whose extents both hold one point -- test 6. The NEARER member
            // belongs to the HIGHER id on purpose, so "lower id wins" is not the same answer.
            body(5, BodyKind::Lake, true, 100.0, 1, WIDE_BAND_M, vec![(40.0, 0.2), (40.5, 0.2)]),
            body(6, BodyKind::Lake, true, 100.0, 1, WIDE_BAND_M, vec![(40.0, 0.0), (40.5, 0.0)]),
            // 7, 8: the same member position twice, so `dm` ties bit for bit -- test 6's tie.
            body(7, BodyKind::Lake, true, 100.0, 1, WIDE_BAND_M, vec![(50.0, 0.0), (50.5, 0.0)]),
            body(8, BodyKind::Lake, true, 100.0, 1, WIDE_BAND_M, vec![(50.0, 0.0), (50.5, 0.0)]),
            // 9: an enclosed body AT the datum -- test 10. Ruling Q-4: its claim beats the ocean.
            body(9, BodyKind::Lake, false, 0.0, 1, BAND_M, vec![(80.0, 0.0), (80.3, 0.0)]),
            // 10: an enclosed salt flat recorded 400 m BELOW the datum -- Ruling Q-12. Its shore
            // band holds landform that is above its own level and still under the sea's, which is
            // the one place "inside an extent" and "claimed by a body" part company.
            body(10, BodyKind::SaltFlat, false, -400.0, 1, BAND_M,
                 vec![(-60.0, 0.0), (-60.3, 0.0)]),
        ];
        let reaches = vec![
            // Reach 0: 1,000 m wide, so the band is 500 m either side of the line.
            ReachLine {
                id: 0, class: ReachClass::River, order: 2, downstream: Downstream::Ocean,
                fresh: true,
                points: vec![reach_point(0.0, 120.0, 20.0, 1_000.0, 3.0),
                             reach_point(0.0, 120.1, 19.0, 1_000.0, 3.0),
                             reach_point(0.0, 120.2, 18.0, 1_000.0, 3.0)],
            },
            // Reach 1: a salt-free stream nobody probes, present so the reach loop is not
            // driven with a single reach in it.
            ReachLine {
                id: 1, class: ReachClass::Stream, order: 1, downstream: Downstream::Sink,
                fresh: false,
                points: vec![reach_point(-20.0, 130.0, 5.0, 200.0, 1.0),
                             reach_point(-20.0, 130.1, 4.0, 200.0, 1.0)],
            },
            // Reaches 2 and 3: two 2,000 m channels whose bands overlap, so one point is inside
            // both. Reach 3 -- the HIGHER id -- runs the nearer of the two to the shared probe on
            // purpose, so "lower id wins" is not the same answer as "nearest centre line wins".
            // Their beds differ, so which one answered is legible in `level_m` alone.
            ReachLine {
                id: 2, class: ReachClass::River, order: 2, downstream: Downstream::Ocean,
                fresh: true,
                points: vec![reach_point(800.0 / M_PER_DEG, 140.0, 50.0, 2_000.0, 4.0),
                             reach_point(800.0 / M_PER_DEG, 140.2, 50.0, 2_000.0, 4.0)],
            },
            ReachLine {
                id: 3, class: ReachClass::River, order: 2, downstream: Downstream::Sink,
                fresh: false,
                points: vec![reach_point(-200.0 / M_PER_DEG, 140.0, 60.0, 2_000.0, 5.0),
                             reach_point(-200.0 / M_PER_DEG, 140.2, 60.0, 2_000.0, 5.0)],
            },
        ];
        HydroRecord { bodies, reaches, notches: Vec::new(), falls: Vec::new(), stats: stats(), ground: [0; 16] }
    }

    fn built(record: &HydroRecord) -> WaterIndex {
        WaterIndex::build(record, R, DEFAULT_CELL_M)
    }

    /// Ask the fixture, with a constant landform.
    fn ask(lat: f64, lon: f64, ground_m: f64) -> WaterAt {
        let record = fixture();
        let index = built(&record);
        ask_with(&record, &index, ground_m, &at(lat, lon))
    }

    fn close(got: f64, want: f64, tol: f64, what: &str) {
        let off = if got > want { got - want } else { want - got };
        assert!(off <= tol, "{what}: got {got}, wanted {want} (within {tol})");
    }

    // ---- 1. A lake's interior ----------------------------------------------------------

    #[test]
    fn a_lake_interior_answers_the_body_its_level_and_the_landform_depth() {
        // Midway between body 0's two members: dm is 5,560 m, dc 44,824 m, so clause 1 admits
        // it without the band being consulted at all.
        let got = ask(0.0, 0.05, 40.0);
        assert_eq!(got.kind, WaterKind::Lake);
        assert_eq!(got.body_id, 0);
        assert_eq!(got.level_m.to_bits(), 100.0f64.to_bits(), "the body's own level");
        assert_eq!(got.depth_m.to_bits(), 60.0f64.to_bits(), "the level minus the landform");
        assert!(got.fresh, "body 0 is fresh");
    }

    // ---- 2. Beyond the shore ------------------------------------------------------------

    #[test]
    fn ground_above_the_level_inside_the_extent_is_none() {
        let got = ask(0.0, 0.05, 150.0);
        assert_eq!(got, WaterAt::none(), "inside the extent but 50 m above the water surface");
    }

    // ---- 3. Outside the extent ----------------------------------------------------------

    #[test]
    fn nearer_a_collar_than_a_member_and_past_the_band_is_none() {
        // Standing on body 0's own collar point: dm 44,478 m, dc 0 m. dm > dc, and dm is well
        // past the 25,000 m band. The premises are asserted, not merely commented -- the first
        // draft of this test probed a point the index does not offer body 0 at all, so its
        // `none` said nothing about the extent clause and it passed against a stub query.
        let probe = at(0.4, 0.0);
        let record = fixture();
        let index = built(&record);
        assert!(index.candidates(&probe).bodies.contains(&0),
                "fixture is wrong: body 0 is not even a candidate here, so `none` proves nothing");
        let dm = probe.distance_to(&at(0.0, 0.0), R);
        let dc = probe.distance_to(&at(0.4, 0.0), R);
        assert!(dm > dc, "fixture is wrong: dm {dm} m is not past the collar at dc {dc} m");
        assert!(dm > record.bodies[0].shore_reach_m,
                "fixture is wrong: dm {dm} m is inside the band, so clause 2 would admit it");

        let got = ask_with(&record, &index, 40.0, &probe);
        assert_eq!(got, WaterAt::none(), "outside the extent, even though 40 m is below 100 m");
    }

    // ---- 4. The band ---------------------------------------------------------------------

    #[test]
    fn nearer_a_collar_than_a_member_but_inside_the_band_is_the_body() {
        // 0.21 deg north: dm 23,351 m -- past the collar (dc 21,127 m) but inside the 25,000 m
        // band. Clause 1 refuses it; clause 2 is the only thing that can admit it.
        let probe = at(0.21, 0.0);
        let record = fixture();
        let dm = probe.distance_to(&at(0.0, 0.0), R);
        let dc = probe.distance_to(&at(0.4, 0.0), R);
        assert!(dm > dc, "fixture is wrong: the probe is nearer a member ({dm} m) than a collar ({dc} m)");
        assert!(dm <= record.bodies[0].shore_reach_m, "fixture is wrong: {dm} m is past the band");

        let got = ask(0.21, 0.0, 40.0);
        assert_eq!(got.kind, WaterKind::Lake, "the shore band is what holds the level contour");
        assert_eq!(got.body_id, 0);
        assert_eq!(got.depth_m.to_bits(), 60.0f64.to_bits());
    }

    // ---- 5. A zero band --------------------------------------------------------------------

    /// 26 of 1,042 measured bodies carry `shore_reach_m == 0.0`, and §8.3 answers them by the
    /// nearest-point clause alone. Both sides of that zero are asserted: the same probe, the
    /// same body, differing only in the band -- refused at zero, admitted at 25,000 m.
    #[test]
    fn a_zero_band_admits_only_what_the_nearest_point_clause_admits() {
        let interior = at(29.99, 0.0); // dm 1,112 m, dc 3,336 m -- clause 1's business
        let banded = at(30.015, 0.0);  // dm 1,668 m, dc 556 m -- only a band can admit it

        let mut record = fixture();
        assert_eq!(record.bodies[4].shore_reach_m.to_bits(), 0.0f64.to_bits());
        let index = built(&record);
        let same = flat(40.0);
        let g = Ground { landform_m: Landform(&same), detail_m: Detail(&same) };
        assert_eq!(water_at(&record, &index, &g, &interior).kind, WaterKind::Lake,
                   "clause 1 still admits the interior with no band at all");
        assert_eq!(water_at(&record, &index, &g, &banded).body_id, NO_BODY,
                   "with a zero band nothing but clause 1 can admit, and clause 1 refuses this");

        // The other side of the zero: give that same body a band and the same probe is inside.
        record.bodies[4].shore_reach_m = BAND_M;
        let index = built(&record);
        let got = water_at(&record, &index, &g, &banded);
        assert_eq!(got.kind, WaterKind::Lake, "the probe is 1,668 m from a member");
        assert_eq!(got.body_id, 4);
    }

    // ---- 6. Two bodies claiming one point (Ruling T1-3) ------------------------------------

    #[test]
    fn where_two_bodies_claim_a_point_the_smaller_dm_wins() {
        // Body 6's member is 4,259 m away, body 5's 12,777 m. Both extents hold the point.
        let probe = at(40.0, 0.05);
        let record = fixture();
        let index = built(&record);
        let candidates = index.candidates(&probe);
        assert!(candidates.bodies.contains(&5) && candidates.bodies.contains(&6),
                "fixture is wrong: the index must offer both bodies, got {:?}", candidates.bodies);
        let got = ask_with(&record, &index, 40.0, &probe);
        assert_eq!(got.body_id, 6, "the nearer member belongs to the HIGHER id here on purpose");
    }

    #[test]
    fn where_two_bodies_tie_on_dm_the_lower_id_wins() {
        // Bodies 7 and 8 carry the same member position, so `dm` is bit-identical.
        let probe = at(50.01, 0.0);
        let record = fixture();
        let index = built(&record);
        let dm7 = probe.distance_to(&at(record.bodies[7].outline[0].0, record.bodies[7].outline[0].1), R);
        let dm8 = probe.distance_to(&at(record.bodies[8].outline[0].0, record.bodies[8].outline[0].1), R);
        assert_eq!(dm7.to_bits(), dm8.to_bits(), "fixture is wrong: the two dm are not a tie");
        let got = ask_with(&record, &index, 40.0, &probe);
        assert_eq!(got.body_id, 7, "a tie goes to the lower body id");
    }

    // ---- 7. A pond ---------------------------------------------------------------------------

    #[test]
    fn a_pond_answers_inside_its_traced_ring_and_none_outside_it() {
        let record = fixture();
        let index = built(&record);
        let same = flat(20.0);
        let g = Ground { landform_m: Landform(&same), detail_m: Detail(&same) };

        let inside = at(10.03, 10.01);
        let got = water_at(&record, &index, &g, &inside);
        assert_eq!(got.kind, WaterKind::Pond);
        assert_eq!(got.body_id, 1);
        assert_eq!(got.level_m.to_bits(), 50.0f64.to_bits());
        assert_eq!(got.depth_m.to_bits(), 30.0f64.to_bits());
        assert!(got.fresh);

        // In the concave notch: inside the bounding box and inside the convex hull, outside
        // the ring. The candidate assertion is what stops this passing vacuously.
        let notch = at(10.005, 10.015);
        assert!(index.candidates(&notch).bodies.contains(&1),
                "fixture is wrong: the pond is not even a candidate at the notch probe");
        assert_eq!(water_at(&record, &index, &g, &notch), WaterAt::none(),
                   "the V notched into the southern edge puts this point outside the ring");

        // And well away from the ring entirely.
        assert_eq!(water_at(&record, &index, &g, &at(10.10, 10.10)), WaterAt::none());
    }

    /// **Ruling Q-16.** A body with `shore_member_count == 0` was levelled off the detail field
    /// (Ruling S-9), so its level is compared against the detail field; everything else was
    /// levelled off the landform (Ruling Q-3) and is compared against that. Driven with the two
    /// surfaces deliberately disagreeing, and driven **both ways round**, so passing them the
    /// wrong way round fails rather than merely looking odd.
    #[test]
    fn a_ring_bodys_level_is_read_off_the_detail_field_and_a_shore_bodys_off_the_landform() {
        let record = fixture();
        let index = built(&record);
        assert_eq!(record.bodies[1].shore_member_count, 0, "body 1 is the traced ring");
        assert!(record.bodies[0].shore_member_count > 0, "body 0 is a shore-point set");

        let in_the_pond = at(10.03, 10.01);   // inside the ring; the pond's level is 50 m
        let in_the_lake = at(0.0, 0.05);      // inside body 0's extent; its level is 100 m

        // Detail wet, landform dry: 20 m is under the pond's 50 m level, 150 m is over the lake's
        // 100 m. Only the body that reads the detail field can answer.
        let detail_is_wet = Ground { landform_m: Landform(&flat(150.0)), detail_m: Detail(&flat(20.0)) };
        let pond = water_at(&record, &index, &detail_is_wet, &in_the_pond);
        assert_eq!(pond.kind, WaterKind::Pond, "a ring body reads the detail field, where it is wet");
        assert_eq!(pond.body_id, 1);
        assert_eq!(pond.depth_m.to_bits(), 30.0f64.to_bits(), "level 50 m over detail 20 m");
        assert_eq!(water_at(&record, &index, &detail_is_wet, &in_the_lake), WaterAt::none(),
                   "a shore-point body reads the landform, which stands 50 m over its own level");

        // The other way round: landform wet, detail dry. 40 m is under the lake's 100 m level,
        // 80 m is over the pond's 50 m. Now only the body that reads the landform can answer.
        let landform_is_wet = Ground { landform_m: Landform(&flat(40.0)), detail_m: Detail(&flat(80.0)) };
        assert_eq!(water_at(&record, &index, &landform_is_wet, &in_the_pond), WaterAt::none(),
                   "the pond's own level is 50 m and the detail field stands at 80 m");
        let lake = water_at(&record, &index, &landform_is_wet, &in_the_lake);
        assert_eq!(lake.kind, WaterKind::Lake, "the lake reads the landform, at 40 m");
        assert_eq!(lake.body_id, 0);
        assert_eq!(lake.depth_m.to_bits(), 60.0f64.to_bits(), "level 100 m over landform 40 m");
    }

    /// **Ruling Q-17.** A point standing on a traced ring -- on a vertex, or anywhere along an
    /// edge -- is inside it. Every vertex and every edge midpoint of the fixture's ring is
    /// probed, the concave apex and the implicit closing edge included, because a vertex projects
    /// onto the crossing count's own ray origin and the count answers arbitrarily there.
    #[test]
    fn a_point_on_a_rings_vertex_or_edge_is_inside_it() {
        let record = fixture();
        let index = built(&record);
        let ring = pond_ring();

        for (i, &(lat, lon)) in ring.iter().enumerate() {
            let vertex = at(lat, lon);
            assert!(index.candidates(&vertex).bodies.contains(&1),
                    "fixture is wrong: the pond is not a candidate at its own vertex {i}");
            let got = ask_with(&record, &index, 20.0, &vertex);
            assert_eq!(got.kind, WaterKind::Pond, "vertex {i} at {lat},{lon} is on the ring");
            assert_eq!(got.body_id, 1);

            // The edge from this vertex to the next, closing implicitly at the last one. The
            // midpoint is the spherical one -- the normalised sum -- because an edge is a
            // great-circle arc and the average of two lat/lon pairs is not on it.
            let (next_lat, next_lon) = ring[(i + 1) % ring.len()];
            let next = at(next_lat, next_lon);
            let mid = SpherePoint::from_vector(&vertex.vector.add(&next.vector))
                .expect("two distinct ring vertices are not antipodal");
            let got = ask_with(&record, &index, 20.0, &mid);
            assert_eq!(got.kind, WaterKind::Pond,
                       "the midpoint of edge {i} ({lat},{lon} to {next_lat},{next_lon}) is on the \
                        ring");
            assert_eq!(got.body_id, 1);
        }
    }

    // ---- 8. A river ----------------------------------------------------------------------------

    #[test]
    fn a_river_answers_within_half_its_width_of_the_line_and_none_outside() {
        let record = fixture();
        let index = built(&record);
        let same = flat(40.0);
        let g = Ground { landform_m: Landform(&same), detail_m: Detail(&same) };

        // On the line, halfway along the first leg. The nearest recorded point there is the one
        // at lon 120.1 (bed 19 m), 5,560 m away against 5,560 m for lon 120.0 -- so probe a
        // little east of centre where the nearest point is unambiguous.
        let on = at(0.0, 120.12);
        let got = water_at(&record, &index, &g, &on);
        assert_eq!(got.kind, WaterKind::River);
        assert_eq!(got.body_id, NO_BODY, "a river belongs to no body");
        assert_eq!(got.reach_id, 0, "Ruling Q-18: a river names the reach that answered");
        assert_eq!(got.depth_m.to_bits(), 3.0f64.to_bits(), "the reach's own depth");
        assert_eq!(got.level_m.to_bits(), 22.0f64.to_bits(), "bed 19 m plus depth 3 m");
        assert!(got.fresh, "reach 0's chain reaches the ocean");

        // 400 m north of the line: inside the 500 m half width.
        let inside = at(400.0 / M_PER_DEG, 120.12);
        assert_eq!(water_at(&record, &index, &g, &inside).kind, WaterKind::River);

        // 700 m north of the line: outside it.
        let outside = at(700.0 / M_PER_DEG, 120.12);
        assert_eq!(water_at(&record, &index, &g, &outside), WaterAt::none(),
                   "700 m from the centre line of a 1,000 m channel");
    }

    /// Ruling Q-11: where two reaches both hold a point, the nearer centre line wins. §8.3 fixes
    /// an order for bodies and says nothing about reaches, so this pins the rule the query chose
    /// -- and pins it *discriminatingly*, because reach 3 runs 200 m from the probe and reach 2
    /// runs 800 m, so the nearer channel is the **higher** id and "lower id wins" answers reach 2.
    #[test]
    fn where_two_reaches_hold_a_point_the_nearer_centre_line_wins() {
        let probe = at(0.0, 140.1);
        let record = fixture();
        let index = built(&record);
        let candidates = index.candidates(&probe);
        assert!(candidates.reaches.contains(&2) && candidates.reaches.contains(&3),
                "fixture is wrong: the index must offer both reaches, got {:?}", candidates.reaches);

        let got = ask_with(&record, &index, 40.0, &probe);
        assert_eq!(got.kind, WaterKind::River);
        assert_eq!(got.level_m.to_bits(), 65.0f64.to_bits(),
                   "reach 3's bed 60 m plus depth 5 m -- reach 2 would read 54 m");
        assert_eq!(got.depth_m.to_bits(), 5.0f64.to_bits());
        assert!(!got.fresh, "reach 3 ends in a sink");
        assert_eq!(got.reach_id, 3, "Ruling Q-18: and it names the one that actually answered");
    }

    /// A reach's influence stops at its last recorded point, not at the end of the line it lies
    /// on. Past the end the distance is measured to the endpoint, which is the branch every
    /// probe beside the middle of a leg leaves untouched.
    #[test]
    fn a_reachs_influence_stops_at_its_last_recorded_point() {
        let record = fixture();
        let index = built(&record);
        let same = flat(40.0);
        let g = Ground { landform_m: Landform(&same), detail_m: Detail(&same) };

        // Reach 0 starts at lon 120.0. 334 m short of it, along the very line it runs on: inside
        // the 500 m half width of the endpoint, so still river.
        let near_end = at(0.0, 120.0 - 334.0 / M_PER_DEG);
        assert_eq!(water_at(&record, &index, &g, &near_end).kind, WaterKind::River);

        // 556 m short of it, on the same line. The cross-track distance to that line is zero, so
        // a query that measured to the infinite great circle instead of to the arc would answer
        // `River` here. It must not.
        let past_end = at(0.0, 120.0 - 556.0 / M_PER_DEG);
        assert!(index.candidates(&past_end).reaches.contains(&0),
                "fixture is wrong: reach 0 is not a candidate, so `none` proves nothing");
        assert_eq!(water_at(&record, &index, &g, &past_end), WaterAt::none(),
                   "556 m beyond the last recorded point of a 1,000 m channel");
    }

    // ---- 9. Ocean --------------------------------------------------------------------------------

    #[test]
    fn ground_at_or_below_the_datum_in_no_bodys_extent_is_ocean() {
        let deep = ask(-45.0, 150.0, -1_200.0);
        assert_eq!(deep.kind, WaterKind::Ocean);
        assert_eq!(deep.body_id, NO_BODY);
        assert_eq!(deep.level_m.to_bits(), 0.0f64.to_bits(), "the datum");
        assert_eq!(deep.depth_m.to_bits(), 1_200.0f64.to_bits());
        assert!(!deep.fresh, "the sea is salt");

        // "At or below": exactly at the datum is ocean, at a hand's breadth above it is not.
        assert_eq!(ask(-45.0, 150.0, 0.0).kind, WaterKind::Ocean);
        assert_eq!(ask(-45.0, 150.0, 0.1), WaterAt::none());
    }

    // ---- 10. Ocean loses to a recorded body (Ruling Q-4) -----------------------------------------

    #[test]
    fn a_recorded_bodys_claim_beats_the_ocean_below_the_datum() {
        // Body 9 sits at the datum; the landform under the probe is 50 m below it. Both the
        // ocean clause and the body clause hold, and the record is what decides.
        let got = ask(80.01, 0.0, -50.0);
        assert_eq!(got.kind, WaterKind::Lake, "the record says this below-datum water is a lake");
        assert_eq!(got.body_id, 9);
        assert_eq!(got.level_m.to_bits(), 0.0f64.to_bits());
        assert_eq!(got.depth_m.to_bits(), 50.0f64.to_bits());
        assert!(!got.fresh, "body 9 is closed");

        // And the same landform a degree away, outside every extent, is the ocean again.
        let away = ask(85.0, 0.0, -50.0);
        assert_eq!(away.kind, WaterKind::Ocean);
        assert_eq!(away.body_id, NO_BODY);
    }

    /// **Ruling Q-12.** The ocean is suppressed by a body's *extent*, not by its *claim*: the two
    /// part company on the dry shore of a body recorded below the datum, and gating the ocean on
    /// the claim floods that shore with sea.
    ///
    /// Body 10 is a salt flat at level −400 m. Its shore band holds a probe where the landform is
    /// −100 m: inside the extent, 300 m **above** the flat's own level, and 100 m **below** the
    /// sea's. Neither clause should answer it.
    #[test]
    fn a_bodys_extent_suppresses_the_ocean_even_where_the_body_does_not_claim() {
        let record = fixture();
        let index = built(&record);
        let flat = &record.bodies[10];
        assert_eq!(flat.id, 10);
        assert!(flat.level_m < DATUM_M, "fixture is wrong: this body is not below the datum");

        // In the band: dm 23,351 m (inside the 25,000 m band), dc 1,112 m. Clause 2's case.
        let band = at(-60.21, 0.0);
        let dm = band.distance_to(&at(-60.0, 0.0), R);
        let dc = band.distance_to(&at(-60.3, 0.0), R);
        assert!(dm > dc && dm <= flat.shore_reach_m,
                "fixture is wrong: dm {dm} m, dc {dc} m, band {} m", flat.shore_reach_m);
        assert!(index.candidates(&band).bodies.contains(&10), "fixture is wrong: not a candidate");

        // The landform is above the flat's level and below the datum. This is dry ground.
        let dry = ask_with(&record, &index, -100.0, &band);
        assert_eq!(dry, WaterAt::none(),
                   "an enclosed basin's dry shore is not sea, however far under the datum it lies");

        // Drop the landform under the flat's own level and the same probe is the flat.
        let wet = ask_with(&record, &index, -500.0, &band);
        assert_eq!(wet.kind, WaterKind::SaltFlat);
        assert_eq!(wet.body_id, 10);
        assert_eq!(wet.level_m.to_bits(), (-400.0f64).to_bits());
        assert_eq!(wet.depth_m.to_bits(), 100.0f64.to_bits());
        assert!(!wet.fresh);

        // And the same dry landform outside every extent is the ocean, so the suppression above
        // is the extent's doing and not the landform's.
        let outside = at(-61.0, 0.0);
        let dm_out = outside.distance_to(&at(-60.0, 0.0), R);
        let dc_out = outside.distance_to(&at(-60.3, 0.0), R);
        assert!(dm_out > dc_out && dm_out > flat.shore_reach_m, "fixture is wrong: still inside");
        let sea = ask_with(&record, &index, -100.0, &outside);
        assert_eq!(sea.kind, WaterKind::Ocean);
        assert_eq!(sea.depth_m.to_bits(), 100.0f64.to_bits());
    }

    // ---- 11. A salt body ---------------------------------------------------------------------------

    #[test]
    fn a_salt_body_answers_by_its_own_kind_and_is_never_fresh() {
        let lake = ask(60.005, 0.0, 40.0);
        assert_eq!(lake.kind, WaterKind::SaltLake);
        assert_eq!(lake.body_id, 2);
        assert!(!lake.fresh);
        assert_eq!(lake.depth_m.to_bits(), 60.0f64.to_bits());

        let flat = ask(70.005, 0.0, 40.0);
        assert_eq!(flat.kind, WaterKind::SaltFlat);
        assert_eq!(flat.body_id, 3);
        assert!(!flat.fresh);
    }

    // ---- the assumptions the clauses rest on --------------------------------------------------------

    /// The index carries `Body::id`, not a position in `record.bodies`, and this query resolves
    /// it as an id. Proved by shuffling the record so the two disagree everywhere.
    #[test]
    fn a_record_whose_ids_are_not_positions_still_answers_by_id() {
        let mut record = fixture();
        record.bodies.reverse();
        let index = built(&record);
        let got = ask_with(&record, &index, 40.0, &at(0.0, 0.05));
        assert_eq!(got.kind, WaterKind::Lake);
        assert_eq!(got.body_id, 0, "body 0 now sits last in the vector");
        assert_eq!(got.level_m.to_bits(), 100.0f64.to_bits());
    }

    /// **Ruling Q-18.** A river names its reach; nothing else does. Every one of §8.3's other
    /// answers -- a shore-point body, a traced-ring body, the ocean and dry ground -- leaves
    /// `reach_id` at `NO_REACH`, and a body answer never borrows a reach's id even where a reach
    /// runs through the same place.
    #[test]
    fn only_a_river_names_a_reach() {
        let record = fixture();
        let index = built(&record);

        for (what, lat, lon, ground_m, want) in [
            ("a shore-point lake", 0.0, 0.05, 40.0, WaterKind::Lake),
            ("a traced-ring pond", 10.03, 10.01, 20.0, WaterKind::Pond),
            ("a salt flat", 70.005, 0.0, 40.0, WaterKind::SaltFlat),
            ("the ocean", -45.0, 150.0, -1_200.0, WaterKind::Ocean),
            ("dry ground", -45.0, 150.0, 500.0, WaterKind::None),
        ] {
            let got = ask_with(&record, &index, ground_m, &at(lat, lon));
            assert_eq!(got.kind, want, "fixture is wrong: {what} does not answer {want:?}");
            assert_eq!(got.reach_id, NO_REACH, "{what} named a reach");
        }

        // And the case that could plausibly leak one: a point a reach covers, inside a body's
        // extent, where the body wins by Ruling Q-5. The body answer must still say NO_REACH.
        let on_a_reach = at(0.0, 120.12);
        assert_eq!(ask_with(&record, &index, 40.0, &on_a_reach).reach_id, 0, "premise: a river here");
        let mut with_a_lake = fixture();
        with_a_lake.bodies.push(body(11, BodyKind::Lake, true, 100.0, 1, BAND_M,
                                     vec![(0.0, 120.12), (0.3, 120.12)]));
        let index = built(&with_a_lake);
        let got = ask_with(&with_a_lake, &index, 40.0, &on_a_reach);
        assert_eq!(got.kind, WaterKind::Lake, "Ruling Q-5: a body beats a river");
        assert_eq!(got.body_id, 11);
        assert_eq!(got.reach_id, NO_REACH,
                   "a body answered, so no reach did -- the reach's id must not leak through");
    }

    /// A body's depth is the level minus the landform and never below zero (Ruling Q-6).
    #[test]
    fn a_depth_is_never_negative() {
        // Exactly at the level: still water, zero deep, not `none`.
        let got = ask(0.0, 0.05, 100.0);
        assert_eq!(got.kind, WaterKind::Lake, "\"at or below its level\" includes at");
        assert_eq!(got.depth_m.to_bits(), 0.0f64.to_bits());
    }

    /// Nothing about the answer depends on the order the index happens to list candidates in,
    /// and two calls agree.
    #[test]
    fn the_same_question_gets_the_same_answer_twice() {
        let record = fixture();
        let index = built(&record);
        let same = flat(40.0);
        let g = Ground { landform_m: Landform(&same), detail_m: Detail(&same) };
        for (lat, lon) in [(0.0, 0.05), (0.21, 0.0), (40.0, 0.05), (50.01, 0.0),
                           (10.03, 10.01), (0.0, 120.12), (-45.0, 150.0), (89.9, 12.0)] {
            let p = at(lat, lon);
            assert_eq!(water_at(&record, &index, &g, &p), water_at(&record, &index, &g, &p),
                       "at {lat},{lon}");
        }
    }

    /// An empty record answers `none` above the datum and `ocean` below it, and panics at
    /// neither pole nor seam.
    #[test]
    fn an_empty_record_answers_the_landform_alone() {
        let mut record = fixture();
        record.bodies.clear();
        record.reaches.clear();
        let index = built(&record);
        for (lat, lon) in [(0.0, 0.0), (89.9, 0.0), (-89.9, 0.0), (0.0, -180.0), (0.0, 180.0)] {
            assert_eq!(ask_with(&record, &index, 10.0, &at(lat, lon)), WaterAt::none());
            assert_eq!(ask_with(&record, &index, -10.0, &at(lat, lon)).kind,
                       WaterKind::Ocean);
        }
        close(M_PER_DEG, 111_194.93, 0.1, "metres per degree");
    }
}
