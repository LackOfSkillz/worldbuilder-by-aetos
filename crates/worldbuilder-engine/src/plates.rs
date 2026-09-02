//! Plates, and the Voronoi lookup that says which one a point is on.
//!
//! Ported from `worldbuilder/plates/model.py` and `worldbuilder/plates/lookup.py`.
//!
//! A point belongs to the plate whose seed is nearest, which is the whole of the Voronoi
//! construction. Everything else here is arithmetic in service of asking that question
//! quickly, and of asking how far the point is from the answer changing.

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
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(backward.z.to_bits(), (-forward.z).to_bits());
    }

    #[test]
    fn a_zero_rate_is_a_still_plate() {
        let omega = a_plate(0, 0.0).angular_velocity();
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
}
