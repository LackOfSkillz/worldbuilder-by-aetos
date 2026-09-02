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
