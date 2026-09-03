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
use crate::sphere::SpherePoint;
use crate::substrate::{self, Composition, UnknownSubstrate};
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

    /// The ground before any roughness, which is the same at every scale.
    ///
    /// Args:
    /// point: Anywhere on the planet.
    ///
    /// Returns:
    /// metres: Relative to datum.
    ///
    /// **This one line is three decisions, and every one of them is an ordering.**
    ///
    /// The shelf runs first and its answer is the argument, not a sibling term: the
    /// shelf has already folded continentality and tectonics into one elevation, so
    /// `shelf.elevation_m` *is* the structural ground, and features argue with that
    /// rather than with the macro elevation the shelf was built from. Handing
    /// `land.base_elevation + tectonics.offset_m` here instead compiles, type-checks
    /// and looks like the same physics; at the probe this module tests it moves the
    /// answer by 11.4 m, and over a demo-coast grid by 30.89 m. A harbour is cut into
    /// real bathymetry, not instead of it.
    ///
    /// `Features::apply` then walks its list in construction order, each feature
    /// reading and writing the running result, so the list order is part of the answer
    /// too - reversing this module's two test features moves the probe by 9.4 m.
    ///
    /// And the tuple's **first** element is taken. The second is the authority, a
    /// weight in `[0, 1]`; it is not an elevation and has no business being returned as
    /// one, but it is a plausible `f64` in the same position, so the tests pin the
    /// distance between them (100.9 m at the probe) rather than trusting the index.
    ///
    /// Detail does not appear here at all, and that absence is the method: structure
    /// answers the same at every scale, so nothing resolution-aware may enter. The
    /// authority the second tuple element carries is consumed by `elevation_m`, where
    /// the detail amplitude it damps lives.
    pub fn structural_m(&self, point: &SpherePoint) -> f64 {
        self.features.apply(point, self.shelf.elevation_m(point)).0
    }

    /// How high the ground is.
    ///
    /// Args:
    /// point: Anywhere on the planet.
    /// resolution_m: How far apart the samples being taken are. `None` asks for
    /// canonical ground truth, which is what physics uses; a number lets detail finer
    /// than the sampling drop out, which is both faster and less prone to shimmer.
    ///
    /// Returns:
    /// metres: Relative to datum.
    ///
    /// **Canonical is a defined thing.** `None` evaluates every configured octave down
    /// to the canonical minimum wavelength - not infinite detail. Physics always asks
    /// canonically, so a rock is where it is regardless of how anybody happens to be
    /// looking at the sea around it. A resolution finer than that floor is therefore not
    /// finer than canonical, it *is* canonical: measured, `resolution_m = 25.0` returns
    /// the same bits as `None` at every probe below.
    ///
    /// **One pass, and the intermediates come back with the answer.** `shelf.evaluate`
    /// is called once and its three fields feed everything downstream; asking the shelf
    /// for its weight and the tectonics for their offset separately recomputed the
    /// gradient twice and the plate work three times, for four times the cost.
    ///
    /// **Three orderings live in this method, and none of them is style.** All three
    /// were measured full-pipeline over 625 demo-coast points, and each moves the world.
    ///
    /// The figures below are the **maximum** of `abs(truth - mutant)` over those points -
    /// the mean is three orders smaller - and the points are `Coast.at(offshore, along)`,
    /// so the square is rotated by the demo coast's `SEAWARD_DEG = 296.49` about the
    /// anchor. Both are stated because both were re-derived to recover these numbers: on
    /// the *unrotated* tangent frame, same span and step, the same three maxima are 9.347,
    /// 12.994 and 0.0841 m. A different 625 points is a different set of extrema, and a
    /// budget is a property of its grid as much as of its code.
    ///
    /// - *Dropping the authority multiply* - 11.744 m. `amplitude` is damped by
    ///   `1 - authority` **after** `amplitude_m` sized it and **before** `offset_m`
    ///   spends it. Where somebody stated a shape, roughness defers to it; a harbour
    ///   dredged flat that still carries thirty metres of texture is not dredged.
    /// - *Detail before features* - 5.464 m. Detail is added to `shaped`, so features
    ///   compose against clean structure and roughness lands on top of them. Rough
    ///   first would feed noise into the composition gates and let a `RAISE` argue with
    ///   a texture peak.
    /// - *Sizing detail off pre-feature ground* - 0.045 m. `amplitude_m` is handed
    ///   `shaped`, not `reading.elevation_m`. The smallest of the three and the easiest
    ///   to write by accident, because `reading.elevation_m` is right there in scope.
    ///
    /// **The exact invariant this pins**: `elevation_m` is `structural_m` plus the
    /// detail offset, bit for bit, because `shaped` here and `structural_m`'s answer are
    /// the same computation on the same inputs. Confirmed over a global grid in both
    /// languages. It localises a defect to a stage - a failure here is the detail add,
    /// and a failure of Task 3's invariant is the feature stage - instead of reporting
    /// only that the total is wrong.
    pub fn elevation_m(&self, point: &SpherePoint, resolution_m: Option<f64>) -> f64 {
        let reading = self.shelf.evaluate(point);
        let (shaped, authority) = self.features.apply(point, reading.elevation_m);
        let mut amplitude =
            self.detail
                .amplitude_m(point, shaped, reading.weight, reading.tectonic_m);
        // Where somebody stated a shape, roughness defers to it.
        amplitude *= 1.0 - authority;
        shaped + self.detail.offset_m(point, amplitude, resolution_m)
    }

    /// What the bottom is made of, as fractions of sand, mud and rock.
    ///
    /// Args:
    /// point: Anywhere on the planet.
    ///
    /// Returns:
    /// composition: Fractions summing to one. `Err` where the Python's `PURE[declared]`
    /// raises `KeyError` - see `substrate::UnknownSubstrate`. Python's `bottom_at`
    /// returns a bare `Composition` because a `KeyError` propagates; Rust has no
    /// propagating raise, so the refusal is in the type.
    ///
    /// Costs several times an elevation, because it needs the local slope and a slope is
    /// four probes. Affordable because a ship sounds continuously and anchors once - and
    /// the same intermediates can be handed in when a caller already has them, which is
    /// what `substrate_at` below is for.
    ///
    /// **There is no `substrate` field to reach through.** Python's `__init__` ends with
    /// `self.substrate = Substrate(self)` and this method is `self.substrate.at(point)`;
    /// `substrate.rs` deliberately has no `Substrate` type, so the free `substrate::at`
    /// is called with two callbacks over `&self` instead. Nothing here needs `&mut
    /// self`: both lattices are stateless (see the type docstring), so the callbacks
    /// borrow immutably and a `Surface` can be asked this from several places at once.
    ///
    /// **Six indirect calls, five of them structural.** `structural_m` is called once
    /// for the elevation and four more times inside `slope_at`'s finite difference;
    /// `tectonics.offset_m` is called once. A census that says four is counting
    /// `slope_at`'s probes and forgetting the elevation. The count is pinned by a test,
    /// with counting closures, because it is the cost of this method.
    ///
    /// **And `slope_at` probes through `local_to_sphere`, not the cheap direction.**
    /// Each of its four probes costs a `hypot`, a `cos`, a `sin` and a `sqrt`, so this
    /// method carries five `hypot` calls in all - four in the probes and one in
    /// `slope_at`'s own rise-over-run. A `weight_at`-shaped assumption about the tangent
    /// frame would miss every one of them.
    pub fn bottom_at(&self, point: &SpherePoint) -> Result<Composition, UnknownSubstrate> {
        substrate::at(
            self.radius_m,
            point,
            None,
            None,
            None,
            &|probe| self.structural_m(probe),
            &|probe| self.tectonics.offset_m(probe),
            &self.features,
        )
    }

    // ---- Forwarders, and they are an API decision rather than a transcription -------
    //
    // `surface.py` exposes exactly ONE substrate-facing method, `bottom_at`. The three
    // below have no counterpart in it, and are added deliberately: Python callers reach
    // through the `substrate` attribute - `world.substrate.at(point, **known)`,
    // `world.substrate.slope_at(point, baseline_m)` - and `tests/test_conformance.py`
    // does exactly that in a dozen places. This port has no object to reach through, so
    // without these there is no way for a caller to supply known intermediates or to
    // choose a baseline, and every such caller would have to assemble the callbacks
    // itself and get the resolution order right. They add no behaviour: each is one call
    // to the free function of the same name, with the same callbacks `bottom_at` builds.
    //
    // Labelled here because an unlabelled fifth and sixth method on this type would read
    // later as a transcription error against a Python class that has three.

    /// `substrate::at` with this surface's callbacks, and the intermediates optional.
    ///
    /// The three `Option`s are `is None` **sentinels**: a supplied `0.0` is a value and
    /// is used, not re-derived. Declared in resolution order - elevation, tectonic,
    /// slope - which is Python's *evaluation* order rather than its keyword order; see
    /// `substrate::at`.
    pub fn substrate_at(
        &self,
        point: &SpherePoint,
        elevation_m: Option<f64>,
        tectonic_m: Option<f64>,
        slope: Option<f64>,
    ) -> Result<Composition, UnknownSubstrate> {
        substrate::at(
            self.radius_m,
            point,
            elevation_m,
            tectonic_m,
            slope,
            &|probe| self.structural_m(probe),
            &|probe| self.tectonics.offset_m(probe),
            &self.features,
        )
    }

    /// The one-word answer, which is what the maritime interface asks for.
    pub fn substrate_dominant_at(
        &self,
        point: &SpherePoint,
        elevation_m: Option<f64>,
        tectonic_m: Option<f64>,
        slope: Option<f64>,
    ) -> Result<&'static str, UnknownSubstrate> {
        substrate::dominant_at(
            self.radius_m,
            point,
            elevation_m,
            tectonic_m,
            slope,
            &|probe| self.structural_m(probe),
            &|probe| self.tectonics.offset_m(probe),
            &self.features,
        )
    }

    /// How steep the structural ground is here, over the given baseline.
    ///
    /// Python defaults `baseline_m` to `SLOPE_BASELINE_M`; Rust has no default
    /// arguments, so callers state it - and conformance genuinely varies it.
    pub fn substrate_slope_at(&self, point: &SpherePoint, baseline_m: f64) -> f64 {
        substrate::slope_at(self.radius_m, point, baseline_m, &|probe| {
            self.structural_m(probe)
        })
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
        // **This one cannot fail from within `surface.rs`, and that is not a defect - it is
        // a separation record, not a guard.** `plates_for` takes an `i64` and
        // `generation::joined_key` keys on `world_seed.to_string()`, so while that signature
        // holds the masked *decimal string* `18446744073709551611` is unreachable from Rust
        // at all: no edit to the constructor can make these plates the masked ones. Measured
        // - corrupting the seed outright (`plates_for(SEED ^ 1, ...)`) fails the sibling above
        // and leaves this test passing. What guards against the masked world is that
        // sibling's live-Python comparison; what this records is that the wrong world is far
        // away, which is what makes the sibling's `1e-12` bound worth stating. Count it as a
        // documented separation, not as a discriminating test.
        //
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

    /// A point in the seed -5 world where `Tectonics::offset_m` is sensitive to *the
    /// continentality it holds* - which `shelf_water` is not, and that blindness is the
    /// reason this second probe exists (see the test below).
    ///
    /// Chosen deliberately, not by taking a maximum. Over a 5-degree global grid (2,520
    /// points), a `Tectonics` built on a mis-seeded `Continentality` changes
    /// `offset_m` at **240** of them; of those 240 this is the point whose *whole
    /// neighbourhood* separates furthest - over the 25 points within +-1 degree at a
    /// half-degree step the two fields never come closer than **943.27 m** - so the
    /// sensitivity is a region, not a knife edge a nudged probe would fall off. It is
    /// also the largest single separation on that grid (**1,747.98 m**), and the two
    /// answers have opposite signs: 980 m of down-warp against 768 m of up.
    fn land_sensitive_probe() -> SpherePoint {
        SpherePoint::from_latlon(-75.0, 135.0)
    }

    /// The `Continentality` handed to `Tectonics` is **this world's**, and that link needs
    /// a probe chosen for it.
    ///
    /// `the_layers_are_built_from_each_other_in_the_python_order` above is named for this
    /// link and does not test it: at `shelf_water`, `Tectonics::offset_m` returns
    /// `150.3860222420496` whether its `Continentality` was seeded from this world or from
    /// `noise_seed ^ 1`. Measured - a `Surface` mis-wired that way passed the entire suite.
    /// "Every stage contributes at this probe" was the wrong question; a stage can
    /// contribute a great deal and still be flat in one of its own arguments. The question
    /// is whether corrupting *this argument* moves *this number*.
    ///
    /// So the pairing is load-bearing and both halves are asserted: the catch at
    /// `land_sensitive_probe`, and the blindness at `shelf_water` that makes the first
    /// probe necessary. The mutant is built here rather than described, so the test proves
    /// the wrong wiring is caught rather than asserting that it would be.
    #[test]
    fn the_tectonics_hold_this_worlds_continentality() {
        let surface = plain(None);
        // The mutant differs from the real wiring in the `^ 1` and in nothing else.
        let noise_seed = SEED as u64; // cast-ok: two's-complement, not a truncation
        let mis_seeded = Tectonics::new(
            surface.plates.clone(),
            Continentality::new(noise_seed ^ 1, EARTH_RADIUS_M, LAND_FRACTION),
            EARTH_RADIUS_M,
        );
        // Both figures from the live Python, which was handed `Continentality(-5, ...)` and
        // `Continentality(-6, ...)`; `-5 ^ 1 == -6`, and masking commutes with the xor, so
        // the Python's signed seeds and this `noise_seed ^ 1` are the same two worlds.
        let probe = land_sensitive_probe();
        assert!(close(surface.tectonics.offset_m(&probe), -980.1204136079549, 1e-9));
        assert!(close(mis_seeded.offset_m(&probe), 767.8614639553075, 1e-9));
        // 1,000 m of separation against a 1e-9 relative bound: nothing can satisfy both.
        assert!((surface.tectonics.offset_m(&probe) - mis_seeded.offset_m(&probe)).abs() > 1000.0);

        // And the blindness. Not "the other probe is weaker" but "the other probe cannot
        // see this at all": the two fields agree to the bit there.
        let blind = shelf_water();
        assert_eq!(
            surface.tectonics.offset_m(&blind).to_bits(),
            mis_seeded.offset_m(&blind).to_bits()
        );
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

    // ---- Task 3: `structural_m` and the composition order ----------------------------
    //
    // Two features placed near `shelf_water`, deliberately and not for scenery. They sit
    // where the shelf has real weight (0.34) and the plates have really contributed
    // (+151 m), so shelf, tectonics and continentality are all live at the probes below.
    // A feature stamped at `deep_ocean` would have been useless: the shelf there returns
    // exactly `ABYSS_M` (measured in the live Python - weight 0.0, tectonic 0.0,
    // elevation -4600.0 exactly), and a `Surface` wired to nothing at all reproduces that
    // constant by accident.
    //
    // The pair also exercises both composition gates rather than one: the bank RAISEs
    // (target -50 m against roughly -88 m of ground, so its lift is positive and the
    // RAISE gate passes) and the channel CARVEs (target -120 m, lift negative, the CARVE
    // gate passes). They overlap, so the running `result` the second one reads is the one
    // the first one wrote, which is what makes the list order observable.

    fn bank() -> Feature {
        Feature {
            kind: "bank".to_string(),
            at: SpherePoint::from_latlon(30.0, -65.0),
            target_m: -50.0,
            length_m: 4000.0,
            width_m: 2000.0,
            bearing_deg: 30.0,
            compose: crate::features::RAISE.to_string(),
            marked: false,
            substrate: None,
        }
    }

    fn channel() -> Feature {
        Feature {
            kind: "channel".to_string(),
            at: SpherePoint::from_latlon(30.01, -65.0),
            target_m: -120.0,
            length_m: 6000.0,
            width_m: 1500.0,
            bearing_deg: 75.0,
            compose: crate::features::CARVE.to_string(),
            marked: false,
            substrate: Some("mud".to_string()),
        }
    }

    /// Both features bite hard here - weights 0.873 and 0.800 - so the answer is a long
    /// way from the bare shelf (17.1 m) and a long way from either target. Chosen for
    /// that: it says the feature stage ran, and that both features ran.
    fn deep_probe() -> SpherePoint {
        SpherePoint::from_latlon(30.006, -65.0)
    }

    /// The base-sensitive probe, and the one that carries the ordering claim. Weights
    /// 0.254 and 0.531 - **strictly between zero and one on purpose**: at weight one the
    /// answer is the feature's `target_m` whatever base was handed in, so a saturated
    /// probe cannot tell a shelf base from a macro base from no base at all. Found by
    /// scanning a 61x61 grid of milli-degrees around the bank for both weights in
    /// (0.25, 0.75) and maximising the smaller of (how far the features move the ground,
    /// how far a wrong base would move the answer). Both come out over 11 m.
    fn base_sensitive_probe() -> SpherePoint {
        SpherePoint::from_latlon(30.009, -64.981)
    }

    fn shaped() -> Surface {
        plain(Some(FeatureInput::Loose(vec![bank(), channel()])))
    }

    /// The invariant, and it is a bit comparison rather than a bound: with nothing
    /// placed, `Features::apply` returns its argument untouched, so `structural_m` must
    /// be the *same f64* as `shelf.elevation_m` - not close to it. Checked over a
    /// 625-point global grid, the same grid and the same result as the live Python
    /// (625/625 there too). This localises a defect to a stage: if it fails, the feature
    /// stage is doing something to an empty list; if it passes, everything left to go
    /// wrong is in the composition.
    #[test]
    fn with_nothing_placed_structural_is_the_shelf_bit_for_bit() {
        let surface = plain(None);
        for i in 0..25 {
            for j in 0..25 {
                let lat = -60.0 + f64::from(i) * 5.0;
                let lon = -180.0 + f64::from(j) * 14.4;
                let point = SpherePoint::from_latlon(lat, lon);
                assert_eq!(
                    surface.structural_m(&point).to_bits(),
                    surface.shelf.elevation_m(&point).to_bits(),
                    "no-feature structural drifted from the shelf at {lat}, {lon}"
                );
            }
        }
        // And at the points the later tests place features on, so the invariant is
        // pinned exactly where the composition is about to be exercised.
        for point in [deep_probe(), base_sensitive_probe(), shelf_water(), deep_ocean()] {
            assert_eq!(
                surface.structural_m(&point).to_bits(),
                surface.shelf.elevation_m(&point).to_bits()
            );
        }
        // Rust against Rust proves consistency, not correctness, so one absolute figure
        // from the live Python as well. 1e-9 relative: the path reaches sin, cos, atan2
        // and asin many times over, and every wiring error measured here misses by
        // metres.
        assert!(close(
            surface.structural_m(&deep_probe()),
            -89.95662579145922,
            1e-9
        ));
    }

    /// The composition, against the live Python, at both probes.
    #[test]
    fn features_compose_onto_the_shelf_in_construction_order() {
        let surface = shaped();
        assert!(close(
            surface.structural_m(&deep_probe()),
            -107.01234294485822,
            1e-9
        ));
        assert!(close(
            surface.structural_m(&base_sensitive_probe()),
            -100.34016146516898,
            1e-9
        ));
        // The features really moved the ground, so the stage is not a no-op: 17.1 m at
        // the deep probe, 12.7 m at the base-sensitive one.
        let deep = surface.structural_m(&deep_probe());
        assert!((deep - surface.shelf.elevation_m(&deep_probe())).abs() > 10.0);
        let sensitive = surface.structural_m(&base_sensitive_probe());
        assert!((sensitive - surface.shelf.elevation_m(&base_sensitive_probe())).abs() > 5.0);
    }

    /// The base handed to `apply` is the shelf's answer, not the macro elevation the
    /// shelf was built from, and not the bare continentality underneath it. Both wrong
    /// bases are code somebody could plausibly write; both figures below are measured,
    /// from the same Python run, and both are far enough away that no tolerance reaches
    /// them.
    #[test]
    fn the_base_is_the_shelf_not_the_macro_elevation() {
        let surface = shaped();
        let point = base_sensitive_probe();
        let actual = surface.structural_m(&point);
        // `land.base_elevation + tectonics.offset_m` as the base: -88.90851322503084,
        // which is 11.43 m away. (At the deep probe the same mutation moves the answer by
        // only 0.78 m, because weights of 0.87 and 0.80 leave 2.6% of the base showing -
        // which is exactly why the probe with weights 0.25 and 0.53 is the one asserting
        // this.)
        assert!((actual - (-88.908_513_225_030_84)).abs() > 1.0);
        // `land.base_elevation` alone as the base: -166.40454923699966, 66.06 m away.
        assert!((actual - (-166.404_549_236_999_66)).abs() > 1.0);
        // And the wrong bases are genuinely reachable values, not straw men: the macro
        // elevation here is -54.978 against the shelf's -87.661, a 32.7 m difference in
        // the argument, which the feature weights then damp to 11.43 m in the answer.
        let reading = surface.shelf.evaluate(&point);
        let macro_m = surface.land.base_elevation(&point) + reading.tectonic_m;
        assert!(close(macro_m, -54.97828011320581, 1e-9));
        assert!((macro_m - reading.elevation_m).abs() > 30.0);
    }

    /// Construction order is part of the answer. Same two features, swapped.
    #[test]
    fn swapping_the_two_features_moves_the_answer() {
        let forwards = shaped();
        let backwards = plain(Some(FeatureInput::Loose(vec![channel(), bank()])));
        let point = base_sensitive_probe();
        // From the live Python, the same two features applied in the reverse order onto
        // the same shelf elevation: 9.45 m away at this probe, 48.9 m at the deep one.
        assert!(close(backwards.structural_m(&point), -90.88959541251553, 1e-9));
        assert!((forwards.structural_m(&point) - backwards.structural_m(&point)).abs() > 5.0);
        assert!(
            (forwards.structural_m(&deep_probe()) - backwards.structural_m(&deep_probe())).abs()
                > 40.0
        );
    }

    /// `.0`, not `.1`. The authority is an `f64` in the same tuple and would type-check
    /// in the same place; it is a weight in [0, 1], so returning it puts the seabed a
    /// hundred metres out of position while still looking like a number of metres.
    #[test]
    fn structural_returns_the_elevation_not_the_authority() {
        let surface = shaped();
        for point in [deep_probe(), base_sensitive_probe()] {
            let (elevation, authority) = surface
                .features
                .apply(&point, surface.shelf.elevation_m(&point));
            assert_eq!(surface.structural_m(&point).to_bits(), elevation.to_bits());
            assert!((surface.structural_m(&point) - authority).abs() > 50.0);
        }
        // The authority really is what Python reports, so the distance above is a
        // measured gap and not an artefact of an authority that never got set.
        let (_, deep_authority) = surface
            .features
            .apply(&deep_probe(), surface.shelf.elevation_m(&deep_probe()));
        assert!(close(deep_authority, 0.8734505322907488, 1e-9));
    }

    /// Nothing resolution-aware may enter `structural_m`: structure answers the same at
    /// every scale. There is no `resolution_m` parameter to pass, so what this can check
    /// is that the detail stage is absent from the answer entirely. The detail offset at
    /// this probe is metres in size, so a stray `+ detail.offset_m(...)` could not hide
    /// inside the bit equality below; the test states the amplitude to make the size of
    /// that guard visible rather than assumed.
    // ---- Task 4: `elevation_m`, `bottom_at`, and the callbacks -----------------------
    //
    // Every probe below is `deep_probe` or `base_sensitive_probe`, both chosen in Task 3
    // for a reason that binds harder here. `deep_ocean` is NOT used to check any wiring:
    // the shelf there returns exactly `ABYSS_M` (weight 0.0, tectonic 0.0, elevation
    // -4600.0 exactly), and its bottom composition is exactly `(0.0, 1.0, 0.0)` - both
    // constants a badly wired `Surface` reproduces by accident, and a stage contributing
    // zero cannot show that stage is wired.
    //
    // `base_sensitive_probe` carries the elevation claims because its two feature weights
    // are 0.254 and 0.531, strictly inside (0, 1): at weight one the answer is `target_m`
    // whatever the base, and here the authority is 0.531, so `1 - authority` is neither 0
    // nor 1 and dropping the multiply is visible.
    //
    // For `bottom_at` the two worlds split the work, and NEITHER probe alone is enough:
    //
    // - `plain` at `base_sensitive_probe` - no features, slope 0.0027, tectonic 151 m.
    //   The rock fraction here comes from the TECTONIC term (smooth(151/1200) = 0.0436),
    //   which strictly exceeds the slope term, so this probe is where a dead tectonics
    //   callback shows. The elevation is -87.7 m, between `SETTLED_M` and `SWEPT_M`, so
    //   sand and mud are both live too.
    // - `shaped` at the same point - slope 0.0275, so now the SLOPE term dominates
    //   (0.768 against 0.043) and a dead `slope_at` shows. All three fractions stay
    //   non-zero and the channel's declared `mud` blends in at weight 0.531.
    //
    // `deep_probe` and `shelf_water` in the shaped world are rock-saturated (`natural`
    // returns pure rock, sand exactly 0.0), which hides the depth term entirely. They are
    // asserted for their values but not leaned on for wiring.

    // SIXTEEN MUTATIONS WERE WRITTEN INTO THE SOURCE ON PURPOSE AND RUN. Fifteen fail,
    // named in the tests below. **One survives, and it survives by design rather than by
    // a flat probe**: handing `detail.amplitude_m` the point of somewhere else changes
    // nothing, because `amplitude_m` never reads its `point` - the parameter is vestigial
    // in the Python too (`detail.py:101`, and `detail.rs` carries `#[allow(unused_
    // variables)]` and says so). No probe anywhere on the planet catches that one, so it
    // is recorded here rather than chased.
    //
    // **The class hunted hardest was a probe that is insensitive to one of an active
    // stage's own inputs** - a stage can contribute thirty metres and still be constant
    // in one argument. So each argument was killed separately rather than each stage:
    // `shelf.evaluate`'s weight and tectonic into `amplitude_m` (caught, and caught
    // swapped), the point into `offset_m` (caught), the resolution (caught), the
    // authority (caught), and for `bottom_at` each of the two callbacks and each of the
    // three sentinels on its own (all caught, but only because TWO worlds are probed -
    // see `bottom_at_reads_structure_the_tectonics_and_the_slope`, where each world is
    // provably blind to the mutation the other one catches).

    /// Detail is added under the features instead of on top of them.
    fn mutant_detail_before_features(surface: &Surface, point: &SpherePoint) -> f64 {
        let reading = surface.shelf.evaluate(point);
        let amplitude =
            surface
                .detail
                .amplitude_m(point, reading.elevation_m, reading.weight, reading.tectonic_m);
        let rough = reading.elevation_m + surface.detail.offset_m(point, amplitude, None);
        surface.features.apply(point, rough).0
    }

    /// The authority multiply is left out.
    fn mutant_no_authority(surface: &Surface, point: &SpherePoint) -> f64 {
        let reading = surface.shelf.evaluate(point);
        let (shaped, _) = surface.features.apply(point, reading.elevation_m);
        let amplitude =
            surface
                .detail
                .amplitude_m(point, shaped, reading.weight, reading.tectonic_m);
        shaped + surface.detail.offset_m(point, amplitude, None)
    }

    /// The amplitude is sized off the ground before the features touched it.
    fn mutant_pre_feature_amplitude(surface: &Surface, point: &SpherePoint) -> f64 {
        let reading = surface.shelf.evaluate(point);
        let (shaped, authority) = surface.features.apply(point, reading.elevation_m);
        let amplitude = surface.detail.amplitude_m(
            point,
            reading.elevation_m,
            reading.weight,
            reading.tectonic_m,
        ) * (1.0 - authority);
        shaped + surface.detail.offset_m(point, amplitude, None)
    }

    /// The amplitude `elevation_m` spends, rebuilt from the same intermediates, so the
    /// invariant below can name the detail term exactly rather than approximate it.
    fn damped_amplitude(surface: &Surface, point: &SpherePoint) -> f64 {
        let reading = surface.shelf.evaluate(point);
        let (shaped, authority) = surface.features.apply(point, reading.elevation_m);
        surface
            .detail
            .amplitude_m(point, shaped, reading.weight, reading.tectonic_m)
            * (1.0 - authority)
    }

    /// **The second exact invariant**, and it is a bit comparison rather than a bound:
    /// `elevation_m` is `structural_m` plus the detail offset, the *same f64*, at every
    /// resolution. Checked over a 625-point global grid in both worlds and at both
    /// resolutions, and again on a grid three times as fine over the feature field - the
    /// same test the live Python passes 1250/1250 on.
    ///
    /// This is what localises a defect to a stage. Task 3's invariant says the feature
    /// stage is clean; this one says the detail add is clean; between them a wrong total
    /// has a named owner instead of being "the number is off".
    #[test]
    fn elevation_is_structure_plus_detail_bit_for_bit() {
        let worlds = [plain(None), shaped()];
        for surface in &worlds {
            for i in 0..25 {
                for j in 0..25 {
                    let lat = -60.0 + f64::from(i) * 5.0;
                    let lon = -180.0 + f64::from(j) * 14.4;
                    let point = SpherePoint::from_latlon(lat, lon);
                    let amplitude = damped_amplitude(surface, &point);
                    for resolution in [None, Some(500.0)] {
                        let total = surface.elevation_m(&point, resolution);
                        let parts = surface.structural_m(&point)
                            + surface.detail.offset_m(&point, amplitude, resolution);
                        assert_eq!(
                            total.to_bits(),
                            parts.to_bits(),
                            "elevation drifted from structure + detail at {lat}, {lon}"
                        );
                    }
                }
            }
            // Three times the resolution, over the ground the features actually occupy,
            // where `shaped` and `structural_m` differ by metres and the amplitude is
            // damped hardest. A grid that only samples clean structure would be pinning
            // the easy half of the invariant.
            for i in 0..31 {
                for j in 0..31 {
                    let lat = 30.0 + f64::from(i - 15) * 0.004;
                    let lon = -65.0 + f64::from(j - 15) * 0.004;
                    let point = SpherePoint::from_latlon(lat, lon);
                    let amplitude = damped_amplitude(surface, &point);
                    let total = surface.elevation_m(&point, None);
                    let parts = surface.structural_m(&point)
                        + surface.detail.offset_m(&point, amplitude, None);
                    assert_eq!(total.to_bits(), parts.to_bits());
                }
            }
            // And at the feature centres themselves.
            for point in [bank().at, channel().at, deep_probe(), base_sensitive_probe()] {
                let amplitude = damped_amplitude(surface, &point);
                assert_eq!(
                    surface.elevation_m(&point, None).to_bits(),
                    (surface.structural_m(&point)
                        + surface.detail.offset_m(&point, amplitude, None))
                    .to_bits()
                );
            }
        }
    }

    /// The answer itself, against the live Python. 1e-9 relative: the path reaches sin,
    /// cos, atan2, asin and hypot many times over, while every ordering error measured
    /// here misses by centimetres to metres - two decimal orders clear at the tightest.
    #[test]
    fn elevation_matches_the_live_python() {
        let bare = plain(None);
        assert!(close(bare.elevation_m(&deep_probe(), None), -92.98238988055819, 1e-9));
        assert!(close(
            bare.elevation_m(&deep_probe(), Some(500.0)),
            -92.84942181960578,
            1e-9
        ));
        assert!(close(
            bare.elevation_m(&base_sensitive_probe(), None),
            -92.28764692962568,
            1e-9
        ));
        assert!(close(
            bare.elevation_m(&base_sensitive_probe(), Some(500.0)),
            -91.87773744848144,
            1e-9
        ));
        assert!(close(bare.elevation_m(&shelf_water(), None), -95.72802561529151, 1e-9));

        let surface = shaped();
        assert!(close(
            surface.elevation_m(&deep_probe(), None),
            -107.41556596585484,
            1e-9
        ));
        assert!(close(
            surface.elevation_m(&deep_probe(), Some(500.0)),
            -107.39784621584973,
            1e-9
        ));
        assert!(close(
            surface.elevation_m(&base_sensitive_probe(), None),
            -102.59370837944269,
            1e-9
        ));
        assert!(close(
            surface.elevation_m(&base_sensitive_probe(), Some(500.0)),
            -102.39404836460825,
            1e-9
        ));

        // The intermediates too, so a failure above says which stage moved. All from the
        // same Python run, at the base-sensitive probe in the shaped world.
        let reading = surface.shelf.evaluate(&base_sensitive_probe());
        let (shaped_m, authority) =
            surface.features.apply(&base_sensitive_probe(), reading.elevation_m);
        assert!(close(reading.weight, 0.34395031703563544, 1e-9));
        assert!(close(reading.tectonic_m, 151.11679283543975, 1e-9));
        assert!(close(shaped_m, -100.34016146516898, 1e-9));
        assert!(close(authority, 0.5309604191354713, 1e-9));
        let undamped = surface
            .detail
            .amplitude_m(&base_sensitive_probe(), shaped_m, reading.weight, reading.tectonic_m);
        assert!(close(undamped, 32.971455787700464, 1e-9));
        assert!(close(undamped * (1.0 - authority), 15.464917803156364, 1e-9));
    }

    /// A resolution finer than the canonical floor is not finer than canonical - it *is*
    /// canonical, to the bit. Measured in the live Python, which returns the same repr
    /// for `None` and `25.0` at all three probes. A coarser one genuinely differs, so the
    /// argument is not being ignored.
    #[test]
    fn a_resolution_below_the_canonical_floor_is_canonical() {
        let bare = plain(None);
        for point in [deep_probe(), base_sensitive_probe(), shelf_water()] {
            assert_eq!(
                bare.elevation_m(&point, Some(25.0)).to_bits(),
                bare.elevation_m(&point, None).to_bits()
            );
            assert!(
                (bare.elevation_m(&point, Some(500.0)) - bare.elevation_m(&point, None)).abs()
                    > 0.1
            );
        }
    }

    /// **Where somebody stated a shape, roughness defers to it.** The largest of this
    /// method's three orderings - 11.744 m full-pipeline over the demo coast, 2.55 m and
    /// 2.78 m at these two probes, where the authority is 0.53 and 0.87. The mutant is
    /// built here rather than described, so the test proves the wrong code is caught.
    #[test]
    fn the_authority_damps_the_detail_amplitude() {
        let surface = shaped();
        let point = base_sensitive_probe();
        let wrong = mutant_no_authority(&surface, &point);
        assert!(close(wrong, -105.14476006667721, 1e-9));
        assert!((surface.elevation_m(&point, None) - wrong).abs() > 2.0);
        let deep = deep_probe();
        assert!(close(mutant_no_authority(&surface, &deep), -110.19863071276119, 1e-9));
        assert!(
            (surface.elevation_m(&deep, None) - mutant_no_authority(&surface, &deep)).abs()
                > 2.0
        );
        // And it is a damping, not a switch: the authority here is strictly inside (0, 1),
        // so `1 - authority` is neither a no-op nor a silencer. At authority 1 the mutant
        // and the truth would be a whole amplitude apart and at 0 they would agree, and
        // neither case would say anything about the multiply.
        let reading = surface.shelf.evaluate(&point);
        let (_, authority) = surface.features.apply(&point, reading.elevation_m);
        assert!(authority > 0.01 && authority < 0.99, "authority {authority} cannot test this");
    }

    /// Detail lands on the features, not under them - 5.464 m full-pipeline, 0.64 m and
    /// 0.33 m here. The mutant roughens the shelf first and then composes onto the
    /// roughened ground, which is what "detail before features" actually looks like when
    /// somebody writes it.
    #[test]
    fn detail_comes_after_the_features_not_before_them() {
        let surface = shaped();
        let point = base_sensitive_probe();
        let wrong = mutant_detail_before_features(&surface, &point);
        assert!(close(wrong, -101.95844166218137, 1e-9));
        assert!((surface.elevation_m(&point, None) - wrong).abs() > 0.2);
        let deep = deep_probe();
        assert!(close(
            mutant_detail_before_features(&surface, &deep),
            -107.0889185504868,
            1e-9
        ));
        assert!(
            (surface.elevation_m(&deep, None) - mutant_detail_before_features(&surface, &deep))
                .abs()
                > 0.2
        );
    }

    /// The amplitude is sized off the *shaped* ground, not the shelf's. The smallest of
    /// the three - 0.045 m full-pipeline over the demo coast, 0.083 m and 0.020 m at these
    /// two probes - and the easiest to write by accident, because `reading.elevation_m` is
    /// in scope one line above. **The demo-coast figure is not a ceiling**: the other two
    /// orderings shrink at these probes (11.744 m to 2.55/2.78, 5.464 m to 0.64/0.33) and
    /// this one grows, 0.045 m to 0.083 m. A budget measured on one coast bounds nothing
    /// anywhere else, in either direction. Two
    /// decimal orders above the 1e-9 relative bound the value tests use, so those tests
    /// catch it as well; this one names it.
    #[test]
    fn the_detail_amplitude_is_sized_off_the_shaped_ground() {
        let surface = shaped();
        let point = base_sensitive_probe();
        let wrong = mutant_pre_feature_amplitude(&surface, &point);
        assert!(close(wrong, -102.51022755845966, 1e-9));
        assert!((surface.elevation_m(&point, None) - wrong).abs() > 0.01);
        let deep = deep_probe();
        assert!(close(
            mutant_pre_feature_amplitude(&surface, &deep),
            -107.39525177974745,
            1e-9
        ));
        assert!(
            (surface.elevation_m(&deep, None) - mutant_pre_feature_amplitude(&surface, &deep))
                .abs()
                > 0.01
        );
    }

    /// `bottom_at` against the live Python, in both worlds. 1e-9 relative on fractions of
    /// order one, which is far tighter than any wiring error measured below.
    #[test]
    fn bottom_at_matches_the_live_python() {
        let bare = plain(None);
        let composition = bare.bottom_at(&base_sensitive_probe()).unwrap();
        assert!(close(composition.sand, 0.34250502480173706, 1e-9));
        assert!(close(composition.mud, 0.6139135319375312, 1e-9));
        assert!(close(composition.rock, 0.04358144326073182, 1e-9));
        assert_eq!(composition.dominant(), crate::substrate::MUD);
        let deep = bare.bottom_at(&deep_probe()).unwrap();
        assert!(close(deep.sand, 0.30326208463510135, 1e-9));
        assert!(close(deep.mud, 0.6528911748797973, 1e-9));
        assert!(close(deep.rock, 0.04384674048510139, 1e-9));

        let surface = shaped();
        let composition = surface.bottom_at(&base_sensitive_probe()).unwrap();
        assert!(close(composition.sand, 0.01647523309801734, 1e-9));
        assert!(close(composition.mud, 0.623237083798069, 1e-9));
        assert!(close(composition.rock, 0.36028768310391374, 1e-9));
        // Rock-saturated in `natural`, so this pair is a value check and not a wiring one.
        let rocky = surface.bottom_at(&shelf_water()).unwrap();
        assert!(close(rocky.mud, 0.19479591171649582, 1e-9));
        assert!(close(rocky.rock, 0.8052040882835042, 1e-9));
        assert_eq!(rocky.dominant(), crate::substrate::ROCK);
        // The features moved the bottom, so the placed loop really ran: pure-ish sand and
        // mud in the bare world against a third rock here.
        assert!(composition.rock > 8.0 * bare.bottom_at(&base_sensitive_probe()).unwrap().rock);
    }

    /// **Six indirect calls, five of them structural**, which is what this method costs
    /// and the reason it is not asked per sounding. Counted, not asserted from reading:
    /// the callbacks tally themselves, and the tallying pair must return the same bits as
    /// `bottom_at`'s own. A census that says four has forgotten the elevation.
    #[test]
    fn bottom_at_costs_six_indirect_calls_five_of_them_structural() {
        use std::cell::Cell;
        let surface = shaped();
        let point = base_sensitive_probe();
        let structural = Cell::new(0usize);
        let tectonic = Cell::new(0usize);
        let counted = crate::substrate::at(
            surface.radius_m,
            &point,
            None,
            None,
            None,
            &|probe| {
                structural.set(structural.get() + 1);
                surface.structural_m(probe)
            },
            &|probe| {
                tectonic.set(tectonic.get() + 1);
                surface.tectonics.offset_m(probe)
            },
            &surface.features,
        )
        .unwrap();
        assert_eq!(structural.get(), 5, "one for the elevation and four in slope_at");
        assert_eq!(tectonic.get(), 1);
        assert_eq!(structural.get() + tectonic.get(), 6);
        let direct = surface.bottom_at(&point).unwrap();
        assert_eq!(counted.sand.to_bits(), direct.sand.to_bits());
        assert_eq!(counted.mud.to_bits(), direct.mud.to_bits());
        assert_eq!(counted.rock.to_bits(), direct.rock.to_bits());
    }

    /// The four slope probes are real places, reached through `local_to_sphere` - the
    /// expensive frame direction, `hypot` + `cos` + `sin` + `sqrt` each. A cheap-direction
    /// assumption would put them somewhere else, so the probe geometry is checked rather
    /// than assumed: four distinct points, none of them the centre, and the east pair and
    /// the north pair each `SLOPE_BASELINE_M` apart along the surface.
    #[test]
    fn the_slope_probes_are_four_real_places_around_the_point() {
        use std::cell::RefCell;
        let surface = shaped();
        let point = base_sensitive_probe();
        let seen: RefCell<Vec<SpherePoint>> = RefCell::new(Vec::new());
        crate::substrate::at(
            surface.radius_m,
            &point,
            None,
            None,
            None,
            &|probe| {
                seen.borrow_mut().push(*probe);
                surface.structural_m(probe)
            },
            &|probe| surface.tectonics.offset_m(probe),
            &surface.features,
        )
        .unwrap();
        let seen = seen.into_inner();
        assert_eq!(seen.len(), 5);
        let radius = surface.radius_m;
        // The first is the elevation, at the point itself; the last four are the probes.
        assert_eq!(seen[0].vector.x.to_bits(), point.vector.x.to_bits());
        assert_eq!(seen[0].vector.y.to_bits(), point.vector.y.to_bits());
        assert_eq!(seen[0].vector.z.to_bits(), point.vector.z.to_bits());
        // Half a baseline out from the centre, each of the four, and a whole baseline
        // apart in each pair - a real displacement on a real sphere, which is what
        // `local_to_sphere` gives and a cheap direction would not. Bounds in millimetres
        // over 30 m, because the frame is exact geometry and only the libm differs.
        for probe in &seen[1..] {
            let out = probe.distance_to(&point, radius);
            assert!(
                (out - 30.0).abs() < 1e-3,
                "a slope probe sat {out} m from the point, not half a baseline"
            );
        }
        let east_span = seen[1].distance_to(&seen[2], radius);
        assert!((east_span - 60.0).abs() < 1e-3, "east probes span {east_span} m");
        let north_span = seen[3].distance_to(&seen[4], radius);
        assert!((north_span - 60.0).abs() < 1e-3, "north probes span {north_span} m");
        // The east pair and the north pair are not the same pair: the frame really has
        // two axes, and a `slope_at` that probed one direction twice would read zero
        // slope across the other.
        assert!(seen[1].distance_to(&seen[3], radius) > 30.0);
    }

    /// The callbacks carry what they claim to. Supplying all three intermediates
    /// explicitly must reproduce `bottom_at` to the bit, and each wrong substitution must
    /// not - which is the whole point of choosing two probes rather than one, because no
    /// single point catches all three.
    #[test]
    fn bottom_at_reads_structure_the_tectonics_and_the_slope() {
        let point = base_sensitive_probe();
        for surface in [plain(None), shaped()] {
            let truth = surface.bottom_at(&point).unwrap();
            let explicit = surface
                .substrate_at(
                    &point,
                    Some(surface.structural_m(&point)),
                    Some(surface.tectonics.offset_m(&point)),
                    Some(surface.substrate_slope_at(&point, crate::substrate::SLOPE_BASELINE_M)),
                )
                .unwrap();
            assert_eq!(explicit.sand.to_bits(), truth.sand.to_bits());
            assert_eq!(explicit.mud.to_bits(), truth.mud.to_bits());
            assert_eq!(explicit.rock.to_bits(), truth.rock.to_bits());
        }

        // The elevation callback is `structural_m`, NOT `elevation_m`: detail must not
        // reach the bottom type, or a bar would change what it is made of when somebody
        // zoomed. 0.078 in the sand fraction in the bare world, measured.
        let bare = plain(None);
        let with_detail = bare
            .substrate_at(&point, Some(bare.elevation_m(&point, None)), None, None)
            .unwrap();
        assert!((with_detail.sand - bare.bottom_at(&point).unwrap().sand).abs() > 0.05);

        // The tectonics callback is live, and the BARE world is where that shows: rock
        // there is the tectonic term (0.0436) strictly above the slope term, so zeroing
        // the tectonics moves the answer by 0.030. In the shaped world the same mutation
        // changes nothing at all, because the slope term has overtaken it.
        let flat_plates = bare.substrate_at(&point, None, Some(0.0), None).unwrap();
        assert!((flat_plates.rock - bare.bottom_at(&point).unwrap().rock).abs() > 0.02);

        // And the slope is live, which the SHAPED world is where that shows: the features
        // steepen the ground tenfold, the slope term takes over, and zeroing it moves the
        // answer by 0.34. In the bare world this mutation is invisible.
        let surface = shaped();
        let flat = surface.substrate_at(&point, None, None, Some(0.0)).unwrap();
        assert!((flat.rock - surface.bottom_at(&point).unwrap().rock).abs() > 0.3);
        // The pairing is the claim, so state the halves that DO NOT catch their mutation
        // rather than leaving a reader to assume both probes are equally good.
        let bare_flat = bare.substrate_at(&point, None, None, Some(0.0)).unwrap();
        assert_eq!(bare_flat.rock.to_bits(), bare.bottom_at(&point).unwrap().rock.to_bits());
        let shaped_no_plates = surface.substrate_at(&point, None, Some(0.0), None).unwrap();
        assert_eq!(
            shaped_no_plates.rock.to_bits(),
            surface.bottom_at(&point).unwrap().rock.to_bits()
        );
    }

    /// The forwarders are the free functions and nothing else - no second opinion about
    /// the radius, the callbacks or the resolution order. They exist because Python
    /// callers reach through `world.substrate`, which this port has no object for; see
    /// the label in the source above them.
    #[test]
    fn the_forwarders_are_the_free_functions_with_this_surfaces_callbacks() {
        let surface = shaped();
        let point = base_sensitive_probe();
        let expected = crate::substrate::at(
            surface.radius_m,
            &point,
            None,
            None,
            None,
            &|probe| surface.structural_m(probe),
            &|probe| surface.tectonics.offset_m(probe),
            &surface.features,
        )
        .unwrap();
        let got = surface.substrate_at(&point, None, None, None).unwrap();
        assert_eq!(got.sand.to_bits(), expected.sand.to_bits());
        assert_eq!(got.mud.to_bits(), expected.mud.to_bits());
        assert_eq!(got.rock.to_bits(), expected.rock.to_bits());
        assert_eq!(
            surface.substrate_dominant_at(&point, None, None, None).unwrap(),
            expected.dominant()
        );
        // `slope_at` at the Python default baseline, against the live Python, and at a
        // baseline ten times longer, which `test_conformance.py` genuinely varies. The
        // long one aliases across the channel and reads flatter, which is exactly the
        // aliasing `SLOPE_BASELINE_M` was shortened to avoid.
        let slope = surface.substrate_slope_at(&point, crate::substrate::SLOPE_BASELINE_M);
        assert!(close(slope, 0.027502258572155713, 1e-9));
        assert!((surface.substrate_slope_at(&point, 600.0) - slope).abs() > 1e-3);
        assert!(close(plain(None).substrate_slope_at(&point, 60.0), 0.002738132207272028, 1e-9));
    }

    #[test]
    fn structure_carries_no_detail() {
        let surface = plain(None);
        let point = deep_probe();
        let offset = surface.detail.offset_m(&point, 25.0, None);
        assert!(
            offset.abs() > 1.0,
            "detail here is {offset}, too small to guard with"
        );
        assert_eq!(
            surface.structural_m(&point).to_bits(),
            surface.shelf.elevation_m(&point).to_bits()
        );
    }
}
