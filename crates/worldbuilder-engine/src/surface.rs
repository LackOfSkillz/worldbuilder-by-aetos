//! The whole world, assembled.
//!
//! Ported from `worldbuilder/terrain/surface.py`. One object holding every layer, and one
//! question worth asking of it: how high is the ground at this point. Everything else in
//! the engine exists to answer that.
//!
//! ```text
//! continentality      where land is                    structural
//! tectonics           what the plates did to it        structural
//! shelf               what the coast does to the water structural
//! features            what somebody put there          structural
//! detail              roughness                        resolution-aware
//! ```
//!
//! **Only the last of those thins out with zoom.** The rest are geography and answer the
//! same at every scale; a chart drawn at twenty miles shows the same world as one drawn at
//! one, generalised rather than replaced. If structure faded with sampling, zooming out
//! would not simplify the coastline - it would move it.
//!
//! **Features come after the shelf and before detail, and that ordering is the phase.**
//! After the shelf, because a harbour is cut into real bathymetry rather than instead of
//! it. Before detail, because detail then knows to get out of their way: thirty-five
//! metres of coastal roughness would erase a bar standing four metres proud of the bottom,
//! and a bar nobody can find is not a bar.
//!
//! This module carries the constructor and the field layout, and nothing else: it has no
//! constants and no free functions of its own, because the Python has none either -
//! every number it uses is imported from the layer that owns it. `structural_m`,
//! `elevation_m` and `bottom_at` arrive in later tasks, as do the bindings.

use crate::continentality::Continentality;
use crate::detail::Detail;
use crate::features::{Feature, Features};
use crate::generation::plates_for;
use crate::plates::PlateSet;
use crate::shelf::Shelf;
use crate::tectonics::Tectonics;

/// What the caller brought, where Python writes `features=`.
///
/// Python's parameter is one name carrying three cases, told apart at runtime by
/// `features is None` and `isinstance(features, Features)`. Rust has no runtime
/// `isinstance`, so the three cases become `None` and this enum's two variants - which
/// makes the branch the caller is taking explicit at the call site rather than discovered
/// inside the constructor. **The two variants do not converge**, and the difference is
/// visible in the world: see `Surface::new`.
pub enum FeatureInput {
    /// Python's `elif not isinstance(features, Features)` branch: loose `Feature`s, which
    /// this world places itself, at *its own* radius.
    Loose(Vec<Feature>),
    /// Python's `isinstance(features, Features)` branch: already placed, adopted verbatim,
    /// radius and all.
    Built(Features),
}

/// A generated planet, ready to be asked about.
///
/// Built from a seed and a handful of parameters, and holding a few tens of kilobytes: the
/// plate records, the continentality calibration, two `Noise` states with the detail band
/// table, and however many features somebody placed. Nothing resembling a map is stored
/// anywhere.
///
/// **The Python's class docstring says this holds "two noise lattices that fill themselves
/// in as they are used". That is true of the Python and false here, so it is not carried
/// over.** `worldbuilder/terrain/noise.py` memoises each cell's eight corners because a
/// Python-level call costs more than the arithmetic it avoids; `noise.rs` deliberately
/// dropped that cache (see the crate README, "The cache is gone, deliberately"), since the
/// memoised value is a pure function of three integers and a seed. Both lattices here are
/// therefore *empty at rest and empty forever* - they fill nothing in. The consequence is
/// structural, not cosmetic: **nothing in this type ever needs `&mut self`**, every
/// callback a later task hands to `substrate::at` closes over `&self`, and there is no
/// cache field to add now or later.
///
/// **The seed's domain is narrower than Python's, and no 64-bit type does better.** Python
/// `int` is unbounded and `Surface(10**30)` is legal today; an `i64` signature represents
/// exactly `[-2^63, 2^63)`. This cannot be fixed by choosing `u64` instead, because
/// `plates_for` keys a *decimal string* (`generation::joined_key`): Task 1 measured
/// `plates_for(2**64 + 7)`, `plates_for(10**30)` and `plates_for(-(2**63) - 1)` all
/// differing from their masked forms, so no 64-bit representation reproduces any of them.
/// A seed outside the range is a world this port cannot build, and that is a stated
/// limitation rather than a rounding.
///
/// **`plates`, `land` and `tectonics` are clones, where Python shares one object.**
/// `Tectonics` owns its `PlateSet` and `Shelf` owns its `Tectonics`, so keeping the
/// Python's field layout means the plate table exists three times over. Every one of those
/// values is immutable and none of them is ever written after construction, so the copies
/// cannot drift and no observation can tell them from Python's shared references; the cost
/// is tens of kilobytes and a handful of memcpys, paid once per world, against a
/// constructor that already runs a 4,000-sample calibration.
///
/// **There is no `substrate` field, where Python has one.** `substrate.rs` deliberately
/// has no `Substrate` type - a `Substrate<'a>` borrowing its host could not be a field of
/// that host, and the Python's own docstring says the thing holds nothing - so `bottom_at`
/// (a later task) calls the free `substrate::at` with callbacks over `&self` instead.
/// Eight fields here answer for Python's nine.
pub struct Surface {
    pub world_seed: i64,
    pub radius_m: f64,
    pub plates: PlateSet,
    pub land: Continentality,
    pub tectonics: Tectonics,
    pub shelf: Shelf,
    pub detail: Detail,
    pub features: Features,
}

impl Surface {
    /// Args:
    /// world_seed: The world. See the type docstring for the domain this narrows.
    /// radius_m: The planet's radius. Python defaults this to `EARTH_RADIUS_M`; Rust has
    /// no default arguments, so every caller states it, as `Placed::new` already does.
    /// plate_count: Python's `DEFAULT_PLATE_COUNT`.
    /// land_fraction: Python's `LAND_FRACTION`.
    /// features: `None`, loose features, or a `Features` adopted verbatim.
    ///
    /// **The seed reaches three constructors and they do not agree on what it is.** This
    /// is the one thing in this file that a reviewer should not skim. `plates_for` keys a
    /// decimal string, so `-5` and `18446744073709551611` are different keys and a
    /// different planet - Task 1 measured masking changing the plates in **64 of 64**
    /// seeds, with the `i64` path tracking Python to `2.2e-16` and the masked path to
    /// `0.387`. The two `Noise`-backed constructors mask, so for them the cast is exact.
    /// So the signature stays `i64`, `plates_for` receives it unaltered, and the cast
    /// below is bound once and used at those two sites only. Casting it in one more place
    /// would build a different world while looking like consistency.
    pub fn new(
        world_seed: i64,
        radius_m: f64,
        plate_count: usize,
        land_fraction: f64,
        features: Option<FeatureInput>,
    ) -> Self {
        let plates = plates_for(world_seed, plate_count);
        // `Noise::new` mixes first and masks second (`noise.py:38`, `h = (h ^ (seed * K)) &
        // MASK`), so only the low 64 bits of the mixed value survive and a negative seed's
        // masked result is exactly the wrapping `u64` result. Measured over 2,049 negative
        // seeds through `_lattice`, `Noise.seed` and `Noise.at`: 0 bit mismatches, and not
        // a tautology - all 2,049 give a negative unbounded `Noise.seed` before the mask.
        // See task-1-report.md sections 1a-1c.
        let noise_seed = world_seed as u64; // cast-ok: two's-complement reinterpretation, not a float truncation -- the mask comes AFTER the mixing, so nothing is rounded and nothing is lost
        let land = Continentality::new(noise_seed, radius_m, land_fraction);
        let tectonics = Tectonics::new(plates.clone(), land, radius_m);
        let shelf = Shelf::new(tectonics.clone(), land, radius_m);
        let detail = Detail::new(noise_seed, radius_m);
        // Transcribed from `surface.py`'s three-way branch, and the last arm is the one
        // worth reading twice: a pre-built `Features` is adopted **exactly as it stands,
        // including its own `radius_m`**. Python does not re-place it and does not
        // normalise it, so a `Features` built at 1,234,567 m keeps every tangent frame and
        // every `_cos_reach` at that radius inside a 6,371,000 m world - measured, and
        // pinned by a test below. The branches do not converge, and making them converge
        // would be a fix to a bug the reference implementation does not have.
        let features = match features {
            None => Features::new(Vec::new(), radius_m),
            Some(FeatureInput::Loose(loose)) => Features::new(loose, radius_m),
            Some(FeatureInput::Built(built)) => built,
        };
        Self {
            world_seed,
            radius_m,
            plates,
            land,
            tectonics,
            shelf,
            detail,
            features,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generation::DEFAULT_PLATE_COUNT;
    use crate::sphere::{SpherePoint, EARTH_RADIUS_M};

    // Every expected figure below was taken from the live Python on the host named in
    // `.superpowers/sdd/2026-09-03-slice-1o-surface/task-1-report.md` section 0, by
    // constructing the same objects `Surface.__init__` constructs and printing `repr`.
    const SEED: i64 = -5;
    const LAND_FRACTION: f64 = 0.29;

    fn deep_ocean() -> SpherePoint {
        SpherePoint::from_latlon(41.2, -8.7)
    }

    /// A point in the seed -5 world where the shelf has real weight and the plates have
    /// really contributed - chosen by scanning a 5-degree grid for `weight > 0.2` and
    /// `abs(tectonic_m) > 50`. A point where both are zero would let a badly wired
    /// `Surface` pass by returning a constant.
    fn shelf_water() -> SpherePoint {
        SpherePoint::from_latlon(30.0, -65.0)
    }

    fn bar() -> Feature {
        Feature {
            kind: "bar".to_string(),
            at: SpherePoint::from_latlon(41.2, -8.7),
            target_m: -4.0,
            length_m: 1200.0,
            width_m: 300.0,
            bearing_deg: 30.0,
            compose: crate::features::RAISE.to_string(),
            marked: false,
            substrate: None,
        }
    }

    fn rock() -> Feature {
        Feature {
            kind: "rock".to_string(),
            at: SpherePoint::from_latlon(41.3, -8.6),
            target_m: 2.0,
            length_m: 100.0,
            width_m: 100.0,
            bearing_deg: 0.0,
            compose: crate::features::RAISE.to_string(),
            marked: true,
            substrate: Some("rock".to_string()),
        }
    }

    /// Just off the bar's centre, where its weight is high but not one.
    fn probe() -> SpherePoint {
        SpherePoint::from_latlon(41.201, -8.699)
    }

    fn plain(features: Option<FeatureInput>) -> Surface {
        Surface::new(SEED, EARTH_RADIUS_M, DEFAULT_PLATE_COUNT, LAND_FRACTION, features)
    }

    /// Relative closeness, for the paths that reach a transcendental and therefore compare
    /// pure-Rust `libm` against CPython's platform libm. Every use states its own bound.
    fn close(actual: f64, expected: f64, relative: f64) -> bool {
        let scale = if expected.abs() > 1.0 { expected.abs() } else { 1.0 };
        (actual - expected).abs() <= relative * scale
    }

    #[test]
    fn plates_are_keyed_on_the_signed_seed_not_the_masked_one() {
        let surface = plain(None);
        // Rust against Rust first: whatever the constructor did to the seed on its way to
        // `plates_for`, it must have done nothing. **This half is weaker than it looks and
        // is not the guard** - `world_seed as u64 as i64` round-trips, and
        // `generation::joined_key` stringifies an `i64`, so while `plates_for` keeps that
        // signature the masked *decimal string* is unreachable from Rust at all. What it
        // does catch is a seed altered on the way (measured: `^ 1` fails this test), and it
        // would catch a future signature change that let the masked value through. The real
        // guard against the masked world is the comparison against the live Python below.
        let expected = plates_for(SEED, DEFAULT_PLATE_COUNT);
        for index in 0..DEFAULT_PLATE_COUNT {
            let got = surface.plates.plate(index);
            let want = expected.plate(index);
            assert_eq!(got.seed.vector.x.to_bits(), want.seed.vector.x.to_bits());
            assert_eq!(got.seed.vector.y.to_bits(), want.seed.vector.y.to_bits());
            assert_eq!(got.seed.vector.z.to_bits(), want.seed.vector.z.to_bits());
            assert_eq!(got.rate_rad_per_myr.to_bits(), want.rate_rad_per_myr.to_bits());
        }

        // And against the live Python, which was given the negative seed unaltered.
        // 1e-12 is far above the 2.2e-16 worst distance Task 1 measured and far below the
        // 0.387 the masked seed produces, so the bound cannot be satisfied by both.
        let first = surface.plates.plate(0);
        assert!(close(first.seed.vector.x, 0.22141264197039967, 1e-12));
        assert!(close(first.seed.vector.y, -0.1512200417862282, 1e-12));
        assert!(close(first.seed.vector.z, 0.9633841087218842, 1e-12));
        assert!(close(first.euler_pole.vector.x, 0.012223608390404497, 1e-12));
        assert!(close(first.rate_rad_per_myr, -0.01385465061076852, 1e-12));
        let last = surface.plates.plate(DEFAULT_PLATE_COUNT - 1);
        assert!(close(last.seed.vector.x, 0.3431547394119193, 1e-12));
        assert!(close(last.rate_rad_per_myr, -0.0032799479623228656, 1e-12));
    }

    #[test]
    fn the_masked_seed_really_would_have_built_a_different_world() {
        // The guard above is only worth having if the wrong answer is far away. These are
        // the same components under `plates_for(SEED & (2**64 - 1))`, from the same Python
        // run - a whole different plate, not a rounding difference.
        let surface = plain(None);
        let first = surface.plates.plate(0);
        assert!((first.seed.vector.x - 0.15853435962962845).abs() > 1e-3);
        assert!((first.rate_rad_per_myr - 0.015722720252727022).abs() > 1e-3);
        let last = surface.plates.plate(DEFAULT_PLATE_COUNT - 1);
        assert!((last.seed.vector.x - 0.285921634707757).abs() > 1e-3);
    }

    #[test]
    fn detail_gets_the_masked_seed_and_agrees_with_python_bit_for_bit() {
        let surface = plain(None);
        let point = deep_ocean();
        // `Detail::offset_m` reaches no transcendental: the lattice hash is integer
        // arithmetic and the interpolation is a smoothstep, so this is a bit comparison
        // with no tolerance, the same standard `noise.rs` is held to.
        assert_eq!(
            surface.detail.offset_m(&point, 25.0, None).to_bits(),
            9.694339804183393f64.to_bits()
        );
        assert_eq!(
            surface.detail.offset_m(&point, 25.0, Some(500.0)).to_bits(),
            9.528124443292723f64.to_bits()
        );
    }

    #[test]
    fn continentality_gets_the_masked_seed_and_the_land_fraction() {
        let surface = plain(None);
        assert_eq!(surface.land.land_fraction.to_bits(), LAND_FRACTION.to_bits());
        assert_eq!(surface.land.radius_m.to_bits(), EARTH_RADIUS_M.to_bits());
        // The calibration reaches sqrt/sin/cos, so these are bounded rather than strict.
        assert!(close(surface.land.shore(), -0.005033035864945864, 1e-12));
        assert!(close(surface.land.spread(), 0.21892852176348848, 1e-12));
        assert!(close(surface.land.at(&deep_ocean()), -0.35926831281694593, 1e-12));
    }

    #[test]
    fn the_layers_are_built_from_each_other_in_the_python_order() {
        let surface = plain(None);
        let point = shelf_water();
        // 1e-9 relative: these paths reach sin, cos, atan2 and asin many times over, and
        // the wrong wiring - a shelf built on a differently seeded continentality, or on
        // tectonics that never saw these plates - misses by hundreds of metres, not by
        // parts per billion.
        assert!(close(surface.tectonics.offset_m(&point), 150.3860222420496, 1e-9));
        let reading = surface.shelf.evaluate(&point);
        assert!(close(reading.elevation_m, -91.67475102105001, 1e-9));
        assert!(close(reading.weight, 0.3461125534307774, 1e-9));
        assert!(close(reading.tectonic_m, 150.3860222420496, 1e-9));
        assert!(close(surface.shelf.elevation_m(&point), -91.67475102105001, 1e-9));
        assert!(close(surface.land.at(&point), -0.015084155354279434, 1e-12));
    }

    #[test]
    fn the_scalars_are_kept_exactly_as_given() {
        let surface = plain(None);
        assert_eq!(surface.world_seed, SEED);
        assert_eq!(surface.radius_m.to_bits(), EARTH_RADIUS_M.to_bits());
        assert_eq!(surface.plates.len(), DEFAULT_PLATE_COUNT);
        let odd = Surface::new(7, 1234567.0, 5, 0.5, None);
        assert_eq!(odd.world_seed, 7);
        assert_eq!(odd.radius_m.to_bits(), 1234567.0f64.to_bits());
        assert_eq!(odd.plates.len(), 5);
        assert_eq!(odd.land.land_fraction.to_bits(), 0.5f64.to_bits());
        assert_eq!(odd.land.radius_m.to_bits(), 1234567.0f64.to_bits());
    }

    #[test]
    fn no_features_is_an_empty_features_at_the_world_radius() {
        let surface = plain(None);
        assert_eq!(surface.features.len(), 0);
        assert!(surface.features.is_empty());
        assert_eq!(surface.features.radius_m.to_bits(), EARTH_RADIUS_M.to_bits());
    }

    #[test]
    fn loose_features_are_placed_at_the_world_radius() {
        let surface = plain(Some(FeatureInput::Loose(vec![bar(), rock()])));
        assert_eq!(surface.features.len(), 2);
        assert_eq!(surface.features.radius_m.to_bits(), EARTH_RADIUS_M.to_bits());
        // Order is meaning in `Features::apply`, so check it survived the crossing.
        let kinds: Vec<&str> = surface.features.iter().map(|f| f.kind.as_str()).collect();
        assert_eq!(kinds, vec!["bar", "rock"]);
        assert!(close(
            surface.features.placed[0].weight_at(&probe()),
            0.9545181883460445,
            1e-12
        ));
    }

    #[test]
    fn a_prebuilt_features_is_adopted_verbatim_including_its_own_radius() {
        // A `Features` built for a 1,234,567 m world, handed to a 6,371,000 m one. Python
        // adopts it untouched: `self.features = features`, no re-placing, no radius
        // normalisation. The two branches do not converge.
        let small = Features::new(vec![bar(), rock()], 1234567.0);
        let surface = plain(Some(FeatureInput::Built(small)));
        assert_eq!(surface.radius_m.to_bits(), EARTH_RADIUS_M.to_bits());
        assert_eq!(surface.features.radius_m.to_bits(), 1234567.0f64.to_bits());
        // Every frame and every reach gate stays at the radius it was built at, which is
        // observable through the weight: 0.998 at the small radius against 0.955 at
        // Earth's, both from the live Python.
        let weight = surface.features.placed[0].weight_at(&probe());
        assert!(close(weight, 0.9981770094373194, 1e-12));
        assert!((weight - 0.9545181883460445).abs() > 1e-3);
    }
}
