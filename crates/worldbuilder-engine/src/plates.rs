//! Plates, and the Voronoi lookup that says which one a point is on.
//!
//! Ported from `worldbuilder/plates/model.py` and `worldbuilder/plates/lookup.py`.
//!
//! A point belongs to the plate whose seed is nearest, which is the whole of the Voronoi
//! construction. Everything else here is arithmetic in service of asking that question
//! quickly, and of asking how far the point is from the answer changing.

use crate::detmath as m;
use crate::sphere::SpherePoint;
use crate::vectors::{Vec3, DEGENERATE};

/// One plate: where it is, what it turns about, and how fast.
#[derive(Debug, Clone, Copy)]
pub struct Plate {
    /// Which plate this is. Stable for a given seed and count.
    pub index: usize,
    /// Its centre. A point belongs to the plate whose seed is nearest.
    pub seed: SpherePoint,
    /// The axis it turns about.
    pub euler_pole: SpherePoint,
    /// Radians per million years. **Signed**: the sign and the pole together give the
    /// sense of rotation, so there is no separate clockwise flag to get wrong.
    pub rate_rad_per_myr: f64,
}

/// Where a point stands relative to the edge of its plate.
#[derive(Debug, Clone, Copy)]
pub struct Margin {
    /// The plate the point is on.
    pub nearest: Option<Plate>,
    /// The plate across the nearest stretch of that edge.
    pub neighbour: Option<Plate>,
    /// Metres to it, along the surface.
    pub distance_m: f64,
}

impl Plate {
    /// The rotation vector — the pole, scaled by the rate.
    ///
    /// Combining the two into one vector is what makes surface velocity a single cross
    /// product rather than a special case at the pole itself.
    pub fn angular_velocity(&self) -> Vec3 {
        self.euler_pole.vector.scaled(self.rate_rad_per_myr)
    }
}

/// Every plate on a world, with the arithmetic needed to ask questions about them.
///
/// Holds one precomputed table: for each ordered pair, the normal of the plane bisecting
/// them. Points equidistant from seeds A and B satisfy `dot(P, A) == dot(P, B)`, which
/// rearranges to `dot(P, A - B) == 0` — so the margin between two plates is a great circle
/// whose plane normal is `normalise(A - B)`, and the distance from any point to it is an
/// arc sine away.
///
/// The Python keeps a second copy of this geometry as bare component triples, because a
/// Python method call costs more than the three multiplies inside `Vec3.dot` and a chart
/// redraw makes ninety-nine of them per terrain sample. That duplication is deliberately
/// not ported: in Rust the field access is free, the arithmetic is identical, and one
/// representation cannot fall out of step with itself.
pub struct PlateSet {
    plates: Vec<Plate>,
    /// Row-major, `plates.len()` squared. `None` where the pair cannot define a bisector.
    bisectors: Vec<Option<Vec3>>,
}

impl PlateSet {
    pub fn new(plates: Vec<Plate>) -> Self {
        let n = plates.len();
        let mut bisectors = Vec::with_capacity(n * n);
        for i in 0..n {
            for j in 0..n {
                let difference = plates[i].seed.vector.sub(&plates[j].seed.vector);
                // Python tests `other is plate` first, then the length. Comparing loop
                // positions (i == j) is the faithful translation of that identity check,
                // independent of whether indices are unique within the set.
                let entry = if i == j || difference.length() <= DEGENERATE {
                    None
                } else {
                    difference.normalised()
                };
                bisectors.push(entry);
            }
        }
        Self { plates, bisectors }
    }

    pub fn len(&self) -> usize {
        self.plates.len()
    }

    pub fn is_empty(&self) -> bool {
        self.plates.is_empty()
    }

    pub fn plate(&self, index: usize) -> Plate {
        self.plates[index]
    }

    pub fn plates(&self) -> &[Plate] {
        &self.plates
    }

    /// The bisector normal for an ordered pair, or `None` where none is defined.
    pub fn bisector(&self, a: usize, b: usize) -> Option<Vec3> {
        self.bisectors[a * self.plates.len() + b]
    }

    /// The two plates whose seeds are closest.
    ///
    /// Compared by dot product rather than by angle: for unit vectors a larger dot product
    /// *is* a smaller angle, so converting to distances would only be undone by the
    /// comparison — two dozen transcendental calls a sample, to sort numbers that were
    /// already in order.
    ///
    /// Both comparisons are strict, so a tie keeps the earlier plate and the answer does
    /// not depend on iteration order.
    pub fn nearest_two(&self, point: &SpherePoint) -> (Option<Plate>, Option<Plate>) {
        let v = point.vector;
        let (px, py, pz) = (v.x, v.y, v.z);
        let mut best: Option<Plate> = None;
        let mut second: Option<Plate> = None;
        let mut best_dot = -2.0f64;
        let mut second_dot = -2.0f64;
        for plate in &self.plates {
            let s = plate.seed.vector;
            let alignment = px * s.x + py * s.y + pz * s.z;
            if alignment > best_dot {
                second = best;
                second_dot = best_dot;
                best = Some(*plate);
                best_dot = alignment;
            } else if alignment > second_dot {
                second = Some(*plate);
                second_dot = alignment;
            }
        }
        (best, second)
    }

    /// A bisector normal laid flat on the surface at a point.
    pub fn flattened(&self, point: &SpherePoint, normal: &Vec3) -> Option<Vec3> {
        let v = point.vector;
        let flat = normal.sub(&v.scaled(v.dot(normal)));
        if flat.length() <= DEGENERATE {
            return None;
        }
        flat.normalised()
    }

    /// Which way is across the margin, in the tangent plane at this point.
    ///
    /// Wanted by the kinematics, which need to know whether two plates approach each other
    /// *across* their margin or slide *along* it. The bisector's plane normal is already
    /// perpendicular to the margin; this is its component in the tangent plane, which is
    /// what "away from the margin" means to somebody standing there.
    pub fn margin_normal(&self, point: &SpherePoint, margin: &Margin) -> Option<Vec3> {
        let nearest = margin.nearest?;
        let neighbour = margin.neighbour?;

        // The Python indexes the bisector table by margin.nearest.index and
        // margin.neighbour.index unconditionally. The Rust PlateSet::new instead builds
        // the table by loop position, and nothing enforces index == position, so both
        // axes here are resolved from the passed `margin` by looking up each plate's
        // *position* in self.plates rather than trusting its index field. That is a
        // deliberate deviation from the Python, not a bug: for every set generation.py
        // can build, index and position coincide, so the two addressing schemes agree.
        let near_pos = self.plates.iter().position(|p| p.index == nearest.index)?;
        let neighbour_pos = self.plates.iter().position(|p| p.index == neighbour.index)?;

        let normal = self.bisector(near_pos, neighbour_pos)?;
        self.flattened(point, &normal)
    }

    /// How far a point is from the edge of the plate it is on.
    ///
    /// **The minimum over every bisector of the nearest plate**, and it has to be. The
    /// obvious shortcut is to measure only the bisector with the second-nearest seed,
    /// which is nearly always the right one and is a single arc sine. It is also
    /// discontinuous, and the walk-across-a-margin test caught it jumping by five hundred
    /// kilometres: the distance to a bisector is `asin(dot(P, normalise(A - B)))`, and
    /// when the runner-up changes from B to C the numerator is continuous but the
    /// normalisation is not, because `|A - B|` and `|A - C|` differ. Terrain built on that
    /// would have grown a wall wherever a third plate became the runner-up.
    ///
    /// The minimum is taken on the sine rather than the angle: arc sine is monotonic over
    /// the range in question, so the smallest sine is the smallest angle, and one
    /// transcendental call at the end does for the lot.
    pub fn margin_at(&self, point: &SpherePoint, radius_m: f64) -> Margin {
        if self.plates.len() < 2 {
            let (nearest, _) = self.nearest_two(point);
            return Margin { nearest, neighbour: None, distance_m: f64::INFINITY };
        }

        let v = point.vector;
        let (px, py, pz) = (v.x, v.y, v.z);

        // The nearest plate's *position* in `self.plates`, not `Plate::index`.
        // `PlateSet::new` builds the bisector table by loop position and never touches
        // the `index` field it was handed, so the table is only addressable by position.
        // Nothing in this module enforces `index == position`, so recompute the nearest
        // plate here (mirroring `nearest_two`'s tie-breaking) rather than trying to
        // recover a position from `near.index` after the fact.
        let mut near_pos = 0usize;
        let mut best_dot = -2.0f64;
        for (i, plate) in self.plates.iter().enumerate() {
            let s = plate.seed.vector;
            let alignment = px * s.x + py * s.y + pz * s.z;
            if alignment > best_dot {
                near_pos = i;
                best_dot = alignment;
            }
        }
        let near = self.plates[near_pos];

        let mut closest_sine = 2.0f64;
        let mut across: Option<Plate> = None;
        for (other_index, other) in self.plates.iter().enumerate() {
            let normal = match self.bisector(near_pos, other_index) {
                Some(n) => n,
                None => continue,
            };
            let offset = (px * normal.x + py * normal.y + pz * normal.z).abs();
            if offset < closest_sine {
                closest_sine = offset;
                across = Some(*other);
            }
        }

        // Python writes `min(1.0, closest_sine)`; two-argument min keeps 1.0 unless the
        // value is strictly below it, so a NaN would clamp to 1.0 rather than propagate.
        let clamped = if closest_sine < 1.0 { closest_sine } else { 1.0 };
        Margin { nearest: Some(near), neighbour: across, distance_m: m::asin(clamped) * radius_m }
    }

    /// Every margin of the point's plate that is near enough to matter, not just the
    /// nearest one.
    ///
    /// **Because picking one margin is not continuous, even when its distance is.**
    /// `margin_at` returns a distance that varies smoothly, but the *identity* of the
    /// neighbour it belongs to still jumps: at a point equidistant from two of a plate's
    /// margins, which one is "the" margin flips under a step of a metre, and everything
    /// derived from it -- the normal, the relative motion, what lies either side -- flips
    /// with it. Terrain built on that gained five hundred metres of cliff wherever a plate
    /// had two margins the same distance away.
    ///
    /// The honest answer is that both margins are there. A caller that sums their effects
    /// is continuous, because each contribution depends on its own distance and each fades
    /// out at its own range.
    ///
    /// Ported from `worldbuilder/plates/lookup.py:212-231` (the early exit, the range
    /// threshold, and the candidate loop; the shadow-and-weight logic follows in Task 3).
    pub fn margins_within(
        &self,
        point: &SpherePoint,
        range_m: f64,
        radius_m: f64,
    ) -> (Option<Plate>, Vec<NearbyMargin>) {
        let (nearest, _) = self.nearest_two(point);
        if self.plates.len() < 2 {
            return (nearest, Vec::new());
        }

        // Python writes `min(math.pi / 2, range_m / radius_m)`; two-argument min keeps
        // pi/2 unless the ratio is strictly below it, so a NaN ratio saturates to pi/2
        // rather than propagating. Not f64::min, which would return the ratio.
        let ratio = range_m / radius_m;
        let angle = if ratio < core::f64::consts::FRAC_PI_2 { ratio } else { core::f64::consts::FRAC_PI_2 };
        let limit = m::sin(angle);

        let v = point.vector;
        let (px, py, pz) = (v.x, v.y, v.z);

        // The nearest plate's *position* in `self.plates`, not `Plate::index` -- see the
        // note at `margin_normal` above: `PlateSet::new` addresses its tables by loop
        // position, not by `Plate::index`, and nothing enforces the two coincide.
        let near_pos = match nearest {
            Some(near) => match self.plates.iter().position(|p| p.index == near.index) {
                Some(pos) => pos,
                None => return (nearest, Vec::new()),
            },
            None => return (nearest, Vec::new()),
        };

        let mut found = Vec::new();
        for (other_index, other) in self.plates.iter().enumerate() {
            let normal = match self.bisector(near_pos, other_index) {
                Some(n) => n,
                None => continue,
            };
            let signed = px * normal.x + py * normal.y + pz * normal.z;
            let offset = signed.abs();
            if offset > limit {
                continue;
            }

            // Bug 2: is this bisector actually a margin here, or is a third plate in the
            // way? Two seeds always have a bisector, but it is only part of the cell
            // boundary where those two are genuinely the nearest pair; elsewhere it runs
            // through a third plate's territory, imaginary. The test is to stand at the
            // closest point on the bisector and ask who the neighbours are there.
            let foot_x = px - normal.x * signed;
            let foot_y = py - normal.y * signed;
            let foot_z = pz - normal.z * signed;
            let reach = m::sqrt(foot_x * foot_x + foot_y * foot_y + foot_z * foot_z);
            if reach <= DEGENERATE {
                continue;
            }
            let scale = 1.0 / reach;
            let (stand_x, stand_y, stand_z) = (foot_x * scale, foot_y * scale, foot_z * scale);

            // How far a third plate would have to be for this to be a real margin, against
            // how far the nearest one actually is. Positive means genuine; negative means
            // somebody else's territory.
            //
            // Addressed by loop position, not `Plate::index` -- see the note at
            // `margin_normal` above (plates.rs:161): `PlateSet::new` builds its tables by
            // loop position, not by `Plate::index`, and nothing enforces the two coincide.
            let here = self.plates[near_pos].seed.vector;
            let mine = stand_x * here.x + stand_y * here.y + stand_z * here.z;
            let mut shadow = 2.0f64;
            for (third_pos, third) in self.plates.iter().enumerate() {
                if third_pos == near_pos || third_pos == other_index {
                    continue;
                }
                let seed = third.seed.vector;
                let candidate = mine - (stand_x * seed.x + stand_y * seed.y + stand_z * seed.z);
                // Python writes `shadow = min(shadow, candidate)`; the accumulator is the
                // first operand, so a NaN candidate is ignored and leaves shadow unchanged,
                // while a NaN accumulator would stick permanently. Not f64::min, which is
                // commutative.
                if candidate < shadow {
                    shadow = candidate;
                }
            }

            // **A weight, not a test.** The first version rejected shadowed bisectors with
            // a boolean, and that switched a margin on and off in one step wherever it
            // ended at a triple junction -- a hundred and forty metres of cliff. The third
            // time the same mistake appeared in this phase: a hard decision taken on a
            // continuous quantity. It fades now.
            //
            // Python writes `min(1.0, max(0.0, shadow / SHADOW_BLEND))`. max keeps 0.0
            // unless the value is strictly above it, so NaN clamps to 0.0; min then keeps
            // that.
            let scaled = shadow / SHADOW_BLEND;
            let lifted = if scaled > 0.0 { scaled } else { 0.0 };
            let mut genuine = if lifted < 1.0 { lifted } else { 1.0 };
            if genuine <= 0.0 {
                // A hard exit on a continuous quantity, and deliberately safe: the
                // smoothstep below is exactly zero here, so a skipped candidate and an
                // included one of weight zero are indistinguishable to any summing caller.
                // Not a fourth instance of this module's recurring bug.
                continue;
            }
            genuine = genuine * genuine * (3.0 - 2.0 * genuine);

            // Python writes `min(1.0, offset)`; two-argument min keeps 1.0 unless offset
            // is strictly below it, matching the same clamp used in `margin_at`.
            let clamped = if offset < 1.0 { offset } else { 1.0 };
            found.push(NearbyMargin {
                other: *other,
                distance_m: m::asin(clamped) * radius_m,
                normal,
                weight: genuine,
            });
        }

        (nearest, found)
    }
}

/// How wide the fade zone is where a third plate shadows a bisector, in the same units
/// as the `shadow` quantity in `margins_within` (a difference of two dot products of unit
/// vectors, i.e. dimensionless). Ported from `worldbuilder/plates/lookup.py`.
pub const SHADOW_BLEND: f64 = 0.02;

/// One margin found near a point, on the way to becoming a weighted contribution.
///
/// See `margins_within` for why there can be more than one, and why each carries its own
/// weight rather than the caller picking a single "the" margin.
#[derive(Debug, Clone, Copy)]
pub struct NearbyMargin {
    /// The plate across this margin.
    pub other: Plate,
    /// Metres to it, along the surface.
    pub distance_m: f64,
    /// The bisector's plane normal.
    pub normal: Vec3,
    /// How much this margin's effect should count, from 0 (shadowed away) to 1 (genuine).
    pub weight: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sphere::EARTH_RADIUS_M;

    fn a_plate(index: usize, rate: f64) -> Plate {
        Plate {
            index,
            seed: SpherePoint::from_latlon(10.0, 20.0),
            euler_pole: SpherePoint::from_latlon(80.0, 5.0),
            rate_rad_per_myr: rate,
        }
    }

    #[test]
    fn angular_velocity_is_the_pole_scaled_by_the_rate() {
        let p = a_plate(0, 0.01);
        let omega = p.angular_velocity();
        let pole = p.euler_pole.vector;
        assert_eq!(omega.x.to_bits(), (pole.x * 0.01).to_bits());
        assert_eq!(omega.y.to_bits(), (pole.y * 0.01).to_bits());
        assert_eq!(omega.z.to_bits(), (pole.z * 0.01).to_bits());
    }

    #[test]
    fn a_negative_rate_reverses_the_rotation() {
        // The sign and the pole together give the sense of rotation; there is no
        // separate clockwise flag to get wrong.
        let forward = a_plate(0, 0.01).angular_velocity();
        let backward = a_plate(0, -0.01).angular_velocity();
        assert_eq!(backward.x.to_bits(), (-forward.x).to_bits());
        assert_eq!(backward.y.to_bits(), (-forward.y).to_bits());
        assert_eq!(backward.z.to_bits(), (-forward.z).to_bits());
    }

    #[test]
    fn a_zero_rate_is_a_still_plate() {
        let omega = a_plate(0, 0.0).angular_velocity();
        // +0.0, not -0.0, because this fixture's pole has all-positive components:
        // from_latlon(80.0, 5.0) is about (+0.1729, +0.01514, +0.9848), both angles
        // lying strictly between 0 and 90 degrees. `negative * 0.0` is `-0.0` and
        // to_bits() tells the two apart, so a southern or western pole would need
        // different expectations here.
        assert_eq!(omega.x.to_bits(), 0.0f64.to_bits());
        assert_eq!(omega.y.to_bits(), 0.0f64.to_bits());
        assert_eq!(omega.z.to_bits(), 0.0f64.to_bits());
    }

    /// No prior helper in this file is parametrised by lat/lon (the closest, `a_plate`,
    /// takes only an index and a rate against a fixed seed). Added to match the brief's
    /// `test_plate(index, lat, lon)` name and signature, with a pole and rate that are
    /// non-degenerate and differ from the seed, per the binding contract fixed in 1f.
    fn test_plate(index: usize, lat: f64, lon: f64) -> Plate {
        Plate {
            index,
            seed: SpherePoint::from_latlon(lat, lon),
            euler_pole: SpherePoint::from_latlon(80.0, 5.0),
            rate_rad_per_myr: 0.01,
        }
    }

    /// Two seeds on the equator and one lifted off it. The third seed must NOT lie on
    /// the great circle bisecting the other two -- see `a_wide_range_admits_every_candidate_bisector`.
    fn three_plate_set() -> PlateSet {
        PlateSet::new(vec![
            test_plate(0, 0.0, 0.0),
            test_plate(1, 0.0, 90.0),
            test_plate(2, 60.0, 45.0),
        ])
    }

    #[test]
    fn a_single_plate_set_has_no_margins() {
        let set = PlateSet::new(vec![test_plate(0, 0.0, 0.0)]);
        let (nearest, found) = set.margins_within(
            &SpherePoint::from_latlon(10.0, 10.0), 1.0e6, EARTH_RADIUS_M);
        assert!(nearest.is_some(), "one plate still owns every point");
        assert!(found.is_empty(), "fewer than two plates means no margin can exist");
    }

    #[test]
    fn a_plate_interior_finds_nothing_in_a_short_range() {
        // Standing on seed 0, the nearer bisector is the one with seed 2, roughly
        // 35 degrees away - about 3,900 km. A 1,000 km range must reject both on
        // the range test alone, before any shadow work is done.
        let set = three_plate_set();
        let (_, found) = set.margins_within(
            &SpherePoint::from_latlon(0.0, 0.0), 1.0e6, EARTH_RADIUS_M);
        assert!(found.is_empty(), "every candidate is beyond the range limit");
    }

    #[test]
    fn a_wide_range_admits_every_candidate_bisector() {
        // A range spanning the planet admits both of plate 0's candidate bisectors.
        // This is the pre-shadow count; Task 3 re-verifies it once shadowing exists.
        let set = three_plate_set();
        let (_, found) = set.margins_within(
            &SpherePoint::from_latlon(0.0, 0.0), 2.0e7, EARTH_RADIUS_M);
        assert_eq!(found.len(), 2, "both bisectors are in range before shadowing");
    }

    #[test]
    fn a_bisector_running_through_a_third_plate_is_not_a_margin() {
        // Bug 2. Standing well north on plate 0, the bisector of plates 0 and 1 has
        // its foot in territory that plate 2 owns, so that bisector is imaginary
        // there and must not be returned. Derive the latitude from the set rather
        // than trusting this comment: find a point whose nearest plate is 0 and
        // where the 0-1 shadow is negative, and assert plate 1 is absent from the
        // result while the margin against plate 2 is present.
        let set = three_plate_set();
        let (nearest, found) = set.margins_within(
            &SpherePoint::from_latlon(20.0, 20.0), 2.0e7, EARTH_RADIUS_M);
        assert_eq!(nearest.expect("a plate owns this point").index, 0);
        let others: Vec<usize> = found.iter().map(|m| m.other.index).collect();
        assert!(
            !others.contains(&1),
            "the 0-1 bisector is shadowed by plate 2 here, so it is not a margin; got {others:?}",
        );
    }

    #[test]
    fn a_shadowed_margin_fades_rather_than_switching_off() {
        // Bug 3, and the test that would catch a reversion to the boolean. Walking
        // north along longitude 20, the shadow that plate 2 casts on the 0-1 margin
        // goes from about +0.207 at the equator to about -0.214 by 27 degrees, so
        // the crossing lies inside this path. A boolean would step from 1.0 to
        // absent in a single sample; the fade must not.
        //
        // SHADOW_BLEND is 0.02, so the fade occupies a narrow band: the shadow moves
        // roughly 0.0021 per step here, which puts about ten samples inside it. If
        // your measured numbers differ, add samples rather than relaxing the bound.
        let set = three_plate_set();
        let mut weights = Vec::new();
        for step in 0..200 {
            let lat = (step as f64) * 0.125; // cast-ok: loop counter to f64
            let (_, found) = set.margins_within(
                &SpherePoint::from_latlon(lat, 20.0), 2.0e7, EARTH_RADIUS_M);
            weights.push(found.iter().find(|m| m.other.index == 1).map_or(0.0, |m| m.weight));
        }
        let biggest = weights.windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f64, |a, b| if b > a { b } else { a });
        assert!(
            biggest < 0.25,
            "the shadow weight must fade across neighbouring samples, not step; \
             largest single-step change was {biggest}",
        );
        assert!(
            weights.iter().any(|&w| w > 0.0) && weights.iter().any(|&w| w == 0.0),
            "the path must actually cross the shadow boundary, or the test proves nothing",
        );
    }

    #[test]
    fn the_weight_never_leaves_zero_to_one() {
        // The smoothstep of a [0,1] clamp cannot leave [0,1]. Weak on its own, which
        // is why it is not the test that guards bug 3.
        let set = three_plate_set();
        let (_, found) = set.margins_within(
            &SpherePoint::from_latlon(0.0, 20.0), 2.0e7, EARTH_RADIUS_M);
        for margin in &found {
            assert!(margin.weight > 0.0 && margin.weight <= 1.0, "weight {} out of range", margin.weight);
        }
    }

    fn two_plates() -> PlateSet {
        PlateSet::new(vec![
            Plate { index: 0, seed: SpherePoint::from_latlon(0.0, 0.0),
                    euler_pole: SpherePoint::from_latlon(90.0, 0.0), rate_rad_per_myr: 0.01 },
            Plate { index: 1, seed: SpherePoint::from_latlon(0.0, 90.0),
                    euler_pole: SpherePoint::from_latlon(90.0, 0.0), rate_rad_per_myr: 0.01 },
        ])
    }

    #[test]
    fn the_set_reports_its_plates() {
        let set = two_plates();
        assert_eq!(set.len(), 2);
        assert!(!set.is_empty());
        assert_eq!(set.plate(1).index, 1);
    }

    #[test]
    fn a_plate_has_no_bisector_with_itself() {
        assert!(two_plates().bisector(0, 0).is_none());
        assert!(two_plates().bisector(1, 1).is_none());
    }

    #[test]
    fn a_bisector_is_the_normalised_difference_of_the_seeds() {
        let set = two_plates();
        let a = set.plate(0).seed.vector;
        let b = set.plate(1).seed.vector;
        let want = a.sub(&b).normalised().expect("distinct seeds");
        let got = set.bisector(0, 1).expect("distinct seeds");
        assert_eq!(got.x.to_bits(), want.x.to_bits());
        assert_eq!(got.y.to_bits(), want.y.to_bits());
        assert_eq!(got.z.to_bits(), want.z.to_bits());
    }

    #[test]
    fn the_bisector_reverses_with_the_pair() {
        let set = two_plates();
        let forward = set.bisector(0, 1).expect("distinct");
        let backward = set.bisector(1, 0).expect("distinct");
        // x and y are non-zero in this fixture and do reverse exactly.
        assert_eq!(backward.x.to_bits(), (-forward.x).to_bits());
        assert_eq!(backward.y.to_bits(), (-forward.y).to_bits());
    }

    #[test]
    fn a_zero_component_does_not_reverse() {
        // Both seeds lie on the equator, so both have z = 0 and each direction's
        // subtraction gives 0.0 - 0.0 = +0.0 under round-to-nearest. The bisector is
        // computed as its own subtraction in each direction rather than as one negated,
        // so the zero component is +0.0 both ways -- it does not become -0.0. Asserting
        // a sign flip here would be asserting something untrue.
        let set = two_plates();
        let forward = set.bisector(0, 1).expect("distinct");
        let backward = set.bisector(1, 0).expect("distinct");
        assert_eq!(forward.z.to_bits(), 0.0f64.to_bits());
        assert_eq!(backward.z.to_bits(), 0.0f64.to_bits());
    }

    #[test]
    fn coincident_seeds_have_no_bisector() {
        // Closer together than DEGENERATE, so the difference has no trustworthy
        // direction and the table records that rather than inventing one.
        let here = SpherePoint::from_latlon(12.0, 34.0);
        let set = PlateSet::new(vec![
            Plate { index: 0, seed: here, euler_pole: here, rate_rad_per_myr: 0.0 },
            Plate { index: 1, seed: here, euler_pole: here, rate_rad_per_myr: 0.0 },
        ]);
        assert!(set.bisector(0, 1).is_none());
    }

    #[test]
    fn a_point_on_a_seed_belongs_to_that_plate() {
        let set = two_plates();
        let (best, second) = set.nearest_two(&set.plate(0).seed);
        assert_eq!(best.expect("a nearest").index, 0);
        assert_eq!(second.expect("a second").index, 1);
    }

    #[test]
    fn the_second_is_the_other_one() {
        let set = two_plates();
        let (best, second) = set.nearest_two(&set.plate(1).seed);
        assert_eq!(best.expect("a nearest").index, 1);
        assert_eq!(second.expect("a second").index, 0);
    }

    #[test]
    fn a_tie_keeps_the_earlier_plate() {
        // Equidistant from both seeds. Both comparisons are strict, so the lower index
        // wins and the answer does not depend on iteration order.
        // The point (0, 45) is not exactly equidistant in floating point (off by one ULP),
        // so we construct the tie directly: take the two seed vectors, add them, normalise.
        let set = two_plates();
        let s0 = set.plate(0).seed.vector;
        let s1 = set.plate(1).seed.vector;
        let sum = s0.add(&s1);
        let tie_point = SpherePoint { vector: sum.normalised().expect("distinct non-opposite seeds") };
        let (best, _) = set.nearest_two(&tie_point);
        assert_eq!(best.expect("a nearest").index, 0);
    }

    #[test]
    fn an_empty_set_has_no_nearest() {
        let set = PlateSet::new(vec![]);
        let (best, second) = set.nearest_two(&SpherePoint::from_latlon(0.0, 0.0));
        assert!(best.is_none() && second.is_none());
    }

    #[test]
    fn a_single_plate_has_no_second() {
        let only = Plate {
            index: 0,
            seed: SpherePoint::from_latlon(5.0, 5.0),
            euler_pole: SpherePoint::from_latlon(90.0, 0.0),
            rate_rad_per_myr: 0.0,
        };
        let set = PlateSet::new(vec![only]);
        let (best, second) = set.nearest_two(&SpherePoint::from_latlon(0.0, 0.0));
        assert_eq!(best.expect("a nearest").index, 0);
        assert!(second.is_none());
    }

    #[test]
    fn a_single_plate_has_no_margin() {
        let only = Plate {
            index: 0,
            seed: SpherePoint::from_latlon(5.0, 5.0),
            euler_pole: SpherePoint::from_latlon(90.0, 0.0),
            rate_rad_per_myr: 0.0,
        };
        let set = PlateSet::new(vec![only]);
        let m = set.margin_at(&SpherePoint::from_latlon(0.0, 0.0), EARTH_RADIUS_M);
        assert_eq!(m.nearest.expect("a nearest").index, 0);
        assert!(m.neighbour.is_none());
        assert!(m.distance_m.is_infinite());
    }

    #[test]
    fn a_point_on_the_bisector_is_at_zero_distance() {
        // Equidistant from both seeds, so it stands on their shared edge.
        let set = two_plates();
        let a = set.plate(0).seed.vector;
        let b = set.plate(1).seed.vector;
        let midpoint = SpherePoint {
            vector: a.add(&b).normalised().expect("distinct seeds"),
        };
        let m = set.margin_at(&midpoint, EARTH_RADIUS_M);
        assert!(m.distance_m.abs() < 1e-6, "distance was {}", m.distance_m);
        assert!(m.neighbour.is_some());
    }

    #[test]
    fn a_point_at_a_seed_is_a_quarter_turn_from_the_edge() {
        // With two seeds ninety degrees apart, standing on one puts the shared edge
        // forty-five degrees away.
        let set = two_plates();
        let m = set.margin_at(&set.plate(0).seed, EARTH_RADIUS_M);
        let expected = (std::f64::consts::PI / 4.0) * EARTH_RADIUS_M;
        assert!((m.distance_m - expected).abs() < 1.0, "distance was {}", m.distance_m);
        assert_eq!(m.neighbour.expect("a neighbour").index, 1);
    }

    #[test]
    fn the_distance_never_exceeds_a_quarter_turn() {
        // asin caps at pi/2, and the sine is clamped to 1.0 before it.
        let set = two_plates();
        for lat in (-80..81).step_by(20) {
            for lon in (-180..181).step_by(45) {
                let m = set.margin_at(&SpherePoint::from_latlon(lat as f64, lon as f64), EARTH_RADIUS_M);
                assert!(m.distance_m <= (std::f64::consts::PI / 2.0) * EARTH_RADIUS_M + 1.0);
            }
        }
    }

    #[test]
    fn a_flattened_normal_lies_in_the_tangent_plane() {
        let set = two_plates();
        let p = SpherePoint::from_latlon(20.0, 40.0);
        let normal = set.bisector(0, 1).expect("distinct seeds");
        let flat = set.flattened(&p, &normal).expect("not degenerate here");
        // Perpendicular to up, and unit length.
        assert!(flat.dot(&p.vector).abs() < 1e-12, "dot was {}", flat.dot(&p.vector));
        assert!((flat.length() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn a_normal_parallel_to_up_has_no_flat_direction() {
        // Standing exactly where the bisector's normal points straight up leaves no
        // component in the tangent plane, so there is no "across" to report.
        let set = two_plates();
        let normal = set.bisector(0, 1).expect("distinct seeds");
        let straight_up = SpherePoint { vector: normal };
        assert!(set.flattened(&straight_up, &normal).is_none());
    }

    #[test]
    fn a_margin_with_no_neighbour_has_no_normal() {
        let only = Plate {
            index: 0,
            seed: SpherePoint::from_latlon(5.0, 5.0),
            euler_pole: SpherePoint::from_latlon(90.0, 0.0),
            rate_rad_per_myr: 0.0,
        };
        let set = PlateSet::new(vec![only]);
        let p = SpherePoint::from_latlon(0.0, 0.0);
        let margin = set.margin_at(&p, EARTH_RADIUS_M);
        assert!(set.margin_normal(&p, &margin).is_none());
    }

    #[test]
    fn the_margin_normal_points_across_the_margin() {
        let set = two_plates();
        let p = SpherePoint::from_latlon(10.0, 30.0);
        let margin = set.margin_at(&p, EARTH_RADIUS_M);
        let across = set.margin_normal(&p, &margin).expect("a neighbour exists here");
        assert!(across.dot(&p.vector).abs() < 1e-12);
        assert!((across.length() - 1.0).abs() < 1e-12);
    }

}
