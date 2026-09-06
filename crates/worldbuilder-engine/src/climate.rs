//! Climate at a point: what the air is like where the ground is.
//!
//! This module is the first half of the climate slice
//! (`docs/superpowers/plans/2026-09-06-slice-climate.md`). It answers **temperature**, and
//! it answers it the way every structural layer in this engine answers: `f(point) -> value`,
//! closed form, no march, no state, no neighbours. Moisture -- which cannot be answered that
//! way, because rain that falls here fell out of air that came from somewhere else -- is
//! Task 2, and lands in this same file with a bounded upwind march.
//!
//! # Why temperature takes elevation and not latitude alone
//!
//! Because **the snow line then costs nothing**. The same tropical latitude is rainforest at
//! sea level and snowcap at altitude, and that is not a special case bolted on afterwards --
//! it is what a lapse rate *is*. Setting `temperature_c(lat, h) = 0` and solving for `h`
//! gives the freezing contour in one line of algebra:
//!
//! ```text
//! h_freeze(lat) = 1000 * (pole_c + (equator_c - pole_c) * cos(lat)) / lapse_c_per_km
//! ```
//!
//! which is a snow line that *falls with latitude and reaches sea level on its own* --
//! at the canonical constants, 4,154 m at the equator, 1,811 m at 45 degrees, and below the
//! datum polewards of 61.3 degrees. `viewer/public/app/relief.js` currently draws its snow
//! from `SNOW_LINE_EQUATOR_M = 4900` falling linearly to zero at 80 degrees latitude, which
//! is a hand-fitted stand-in for exactly this curve. **Task 5 of this slice replaces that
//! basis; this task supplies what it inverts, and changes nothing in the viewer.**
//!
//! Polar caps fall out of the same expression with no special case at all: north of the
//! latitude where the sea-level term itself goes below freezing (61.3 degrees at the
//! canonical constants), *every* elevation is below freezing, ground and sea alike.
//!
//! # Temperature has a unit. That is the whole design.
//!
//! The photoreal slice measured this and it is not a matter of taste
//! (`.superpowers/sdd/2026-09-05-slice-photoreal/task-1-report.md` section 3). With
//! temperature banded by **per-world quantiles**, the owner world's subtropical desert
//! latitudes -- 25 degrees, 22 C -- landed in the *median* temperature band, so the
//! classifier's `hot desert` cell, the brightest land colour it has, became unreachable and
//! the desert belt came out `cold desert`.
//!
//! The reason is structural rather than incidental. A quantile can only ever say "warmer
//! than this fraction of this world's land", and that is not what freezing is. Water freezes
//! at 0 C on every world. **So this function returns degrees Celsius, and Task 3 quantiles
//! moisture -- which is a dimensionless index with no anchor and therefore has nothing else
//! it could mean -- and does not quantile this.**
//!
//! The honest cost of an absolute axis, stated rather than discovered later: a band can go
//! unvisited on a world whose land is all one climate. That is correct rather than dead --
//! a world with no polar land should have no tundra -- and `src/bin/climate_survey.rs`
//! reports the observed span per world so a check can say which bands a world can reach.
//!
//! # What this is not
//!
//! It is a **mean annual surface temperature**, and it has no season, no diurnal cycle, no
//! maritime/continental contrast and no ocean current. Those are all real and all absent, so
//! nothing downstream should read a number from here as "the temperature right now". The
//! viewer adds two fractal terms on top of the profile it computes today (`TEMP_MACRO_C = 4`
//! for the ~3,000 km field, `TEMP_BREAKUP_C = 2.5` for the ~90 km one) precisely to fray the
//! band edges this bare profile would otherwise draw as contour lines; that fraying is a
//! *rendering* term over a mean field and stays where it is. **This module is the mean
//! field.**

use crate::detmath as m;

/// Mean annual surface temperature at sea level on the equator, in degrees C.
///
/// # Ground
///
/// Earth's zonal annual-mean surface temperature runs roughly **26 / 20 / 12 / 0 / -25 C at
/// 0 / 30 / 45 / 60 / 90 degrees**. With `POLE_C` below and a `cos(latitude)` profile, this
/// value reproduces those five landmarks to within 1.1 C at every one of them, which is
/// asserted rather than asserted-about in `the_profile_reproduces_earths_zonal_means`.
///
/// It is 27 rather than 26 because the fit is over all five landmarks and not anchored at
/// the equator: 27 costs 1.0 C at the equator and buys the 30- and 45-degree landmarks,
/// which are where most land is.
pub const EQUATOR_C: f64 = 27.0;

/// Mean annual surface temperature at sea level at a pole, in degrees C. See `EQUATOR_C`
/// for the fit; this is the second of its two free parameters and it *is* the 90-degree
/// landmark, exactly.
pub const POLE_C: f64 = -25.0;

/// The environmental lapse rate: how fast the air cools with height, in degrees C per
/// kilometre.
///
/// # Ground
///
/// 6.5 C/km is the standard environmental lapse rate, and is the ICAO Standard Atmosphere's
/// troposphere gradient (6.49 C/km) to the precision anybody quotes it at. It is **not** the
/// dry adiabatic rate (9.8 C/km, which is what a parcel lifted without condensation does)
/// and **not** a saturated adiabatic rate (4-7 C/km, which depends on temperature); the
/// environmental rate is the observed average of a real column and is the right one for a
/// mean annual field.
pub const LAPSE_C_PER_KM: f64 = 6.5;

/// The three values that decide what the air is like, broken out so a caller who wants a
/// colder world, a hotter one, or one whose mountains bite harder can ask for one without
/// touching what "canonical" means.
///
/// `Surface::temperature_c` takes `Option<ClimateParams>`, following `ReliefParams` on
/// `Detail::new`, `TectonicParams` on `Tectonics::new` and `CoastParams` on
/// `Continentality::with_coast`: **`None` is the canonical path, not an implicit
/// `Default::default()`** -- this codebase deliberately rejects defaults nobody chose (see
/// `stream.rs::BuildParams`). This is the fourth parameter block of that kind and it does
/// not invent a fifth convention.
///
/// # What `None` guarantees here, and how that differs from the other three
///
/// This is worth reading carefully, because the guarantee is a *different* one and saying
/// "byte-identical like the others" would be saying nothing.
///
/// `ReliefParams`, `TectonicParams` and `CoastParams` each opt out of a stage that
/// **already existed** and that `worldbuilder/` -- the conformance oracle for 157 tests --
/// already has an opinion about, so for them `None` means "produce the bytes the Python
/// produces". **There is no Python temperature.** Nothing in `worldbuilder/` computes one,
/// no conformance test compares one, and no existing engine output reads one.
///
/// So what `None` means here is the stronger and duller thing: **this stage is not on any
/// existing path at all.** `temperature_c` is a new read-only question asked *of* a
/// `Surface`; it is not a term in `structural_m`, not a term in `elevation_m`, and adds no
/// field to `Surface` (`lib.rs::the_surface_is_not_modified_by_this_slice` pins that
/// struct at eight fields and this task leaves it at eight). Every existing world is
/// bit-identical because no existing code path can reach this module -- which
/// `climate_is_not_reachable_from_any_existing_surface_path` in `surface.rs` asserts by
/// reading the source rather than by believing this paragraph.
///
/// `Some(ClimateParams::canonical())` is nonetheless pinned bit-identical to `None`, at the
/// `Surface` level, because that is the property that stops a future edit from quietly
/// making `canonical()` mean something other than the `None` path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClimateParams {
    /// Sea-level mean annual temperature on the equator, in degrees C.
    pub equator_c: f64,
    /// Sea-level mean annual temperature at a pole, in degrees C.
    pub pole_c: f64,
    /// Degrees C lost per kilometre of elevation above the datum.
    pub lapse_c_per_km: f64,
}

impl ClimateParams {
    /// The fitted Earth-like profile: the three constants above, and nothing else.
    ///
    /// `Some(ClimateParams::canonical())` is bit-identical to `None` -- pinned by
    /// `canonical_matches_none_bit_for_bit` below and, over a real world's elevations, by
    /// `surface.rs::climate_none_matches_climate_some_canonical_bit_for_bit`.
    pub fn canonical() -> Self {
        Self { equator_c: EQUATOR_C, pole_c: POLE_C, lapse_c_per_km: LAPSE_C_PER_KM }
    }
}

/// Mean annual surface temperature at a latitude and an elevation, in degrees C.
///
/// Args:
/// latitude_deg: Degrees, positive north. The profile is `cos`, which is even, so the
/// sign does not matter and no `abs` is needed to make it not matter.
/// elevation_m: Height above the datum, in metres, as `Surface::elevation_m` reports it.
/// **Only elevation above the datum cools.** Below it there is ocean, whose surface is at
/// the datum; giving a 4,000 m trench 26 C of lapse warming would be an artifact of the
/// arithmetic rather than a fact about the world. Land below the datum (a rift basin, a
/// dry sea floor) is therefore very slightly under-warmed, by at most 6.5 C per kilometre
/// of depth, and that is a stated approximation rather than an oversight.
/// params: `ClimateParams::canonical()` for the fitted profile.
///
/// Returns:
/// Degrees Celsius. **An absolute unit, deliberately** -- see the module docstring.
///
/// # The NaN contract is PROPAGATE, and one line of it is load-bearing
///
/// The elevation floor is written as an explicit NaN test *before* the comparison, and that
/// is not decoration. The obvious form is `if elevation_m > 0.0 { elevation_m } else { 0.0 }`,
/// which is what `max(0, h)` compiles to in every language -- and **every comparison against
/// NaN is false**, so a NaN elevation would take the `else` arm, lapse by exactly zero, and
/// return the perfectly plausible sea-level temperature for that latitude.
///
/// That is the failure family this project has now found four times: the abyss guard, the
/// NaN sea level that produced a world bit-identical to a legitimate all-land one, the
/// unnameable lattice cell, and -- in this very slice's own spike -- a NaN march step that
/// **returned full moisture rather than NaN**. The plan says in terms: *decide deliberately;
/// do not inherit it*. Decided: a temperature nobody can answer comes back as NaN.
///
/// The same reasoning applies one level up, at `Surface::temperature_c`, where the entrant
/// is a point rather than a number and the swallowing is done by
/// `SpherePoint::to_latlon`'s Python-compatible clamp; see that function.
pub fn temperature_c(latitude_deg: f64, elevation_m: f64, params: &ClimateParams) -> f64 {
    // NaN FIRST, comparison second. See the docstring: the comparison alone would return a
    // plausible sea-level temperature for an unanswerable elevation.
    let above_datum_m = if elevation_m.is_nan() {
        elevation_m
    } else if elevation_m > 0.0 {
        elevation_m
    } else {
        0.0
    };
    let sea_level_c =
        params.pole_c + (params.equator_c - params.pole_c) * m::cos(m::to_radians(latitude_deg));
    sea_level_c - params.lapse_c_per_km * above_datum_m / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The five landmarks the two constants were fitted to, and the fit's own error bar.
    ///
    /// **Population:** Earth's zonal annual-mean surface temperature at 0, 30, 45, 60 and 90
    /// degrees, quoted as 26 / 20 / 12 / 0 / -25 C. **Method:** `temperature_c(lat, 0.0,
    /// canonical())` at each, compared against the landmark. **Host:** this crate's test
    /// runner, and the arithmetic is `detmath` so it is the same on both targets.
    #[test]
    fn the_profile_reproduces_earths_zonal_means() {
        let params = ClimateParams::canonical();
        let landmarks = [(0.0, 26.0), (30.0, 20.0), (45.0, 12.0), (60.0, 0.0), (90.0, -25.0)];
        let mut worst: f64 = 0.0;
        for (latitude, observed) in landmarks {
            let error = (temperature_c(latitude, 0.0, &params) - observed).abs();
            assert!(error <= 1.1, "at {latitude} deg the profile is off by {error} C");
            if error > worst {
                worst = error;
            }
        }
        // Pinned so the fit cannot silently loosen. The worst error is 1.0 C and TWO
        // landmarks are at it -- the equator (27 against 26) and 60 degrees (1.0 against
        // 0.0). The other three are 0.033, 0.230 and 0.000.
        assert!(worst > 0.99 && worst < 1.01, "worst landmark error was {worst} C");
    }

    /// The alternative profile, and why it is not the one here.
    ///
    /// `cos^2(latitude)` -- equivalently `1 - sin^2` -- is the other one-parameter shape a
    /// zonal profile is usually written with, and it is **6.0 C too cold at 30 degrees**,
    /// which is the entire width of a temperature band. The photoreal slice measured the
    /// consequence downstream (every desert latitude leaves the hot band); this measures the
    /// cause, so the choice of `cos` is a number rather than a preference.
    #[test]
    fn a_squared_cosine_profile_is_six_degrees_too_cold_in_the_subtropics() {
        let params = ClimateParams::canonical();
        let cosine = m::cos(m::to_radians(30.0));
        let squared = params.pole_c + (params.equator_c - params.pole_c) * cosine * cosine;
        let plain = temperature_c(30.0, 0.0, &params);
        let gap = plain - squared;
        assert!(gap > 5.9 && gap < 6.1, "the two profiles differ by {gap} C at 30 deg");
        // And the squared form misses the landmark it is being judged against, badly.
        assert!((squared - 20.0).abs() > 5.0, "squared profile was {squared} C against 20 C");
    }

    /// The lapse rate, at the unit it is stated in.
    #[test]
    fn a_kilometre_of_height_costs_the_lapse_rate() {
        let params = ClimateParams::canonical();
        let sea = temperature_c(0.0, 0.0, &params);
        let high = temperature_c(0.0, 1000.0, &params);
        assert!((sea - high - LAPSE_C_PER_KM).abs() < 1e-12, "{sea} - {high}");
    }

    /// The claim the module docstring makes about the snow line, checked as arithmetic
    /// rather than left as prose: the freezing contour is where this closed form crosses
    /// zero, it falls with latitude, and it reaches the datum at a latitude this world can
    /// actually have land at. **Task 5 consumes this; Task 1 only proves it is there.**
    #[test]
    fn the_freezing_contour_falls_with_latitude_and_reaches_the_datum() {
        let params = ClimateParams::canonical();
        let freeze_m = |latitude: f64| {
            1000.0 * (params.pole_c + (params.equator_c - params.pole_c)
                * m::cos(m::to_radians(latitude)))
                / params.lapse_c_per_km
        };
        let equator = freeze_m(0.0);
        let mid = freeze_m(45.0);
        assert!(equator > mid, "the snow line must fall with latitude: {equator} then {mid}");
        assert!((equator - 4153.8).abs() < 0.5, "equatorial freezing contour was {equator} m");
        assert!((mid - 1810.7).abs() < 0.5, "45-degree freezing contour was {mid} m");
        // It crosses the datum between 61 and 62 degrees, so a polar cap needs no rule.
        assert!(freeze_m(61.0) > 0.0 && freeze_m(62.0) < 0.0);
        // And every elevation is frozen beyond it, which is what "polar cap with no special
        // case" means: at 70 degrees even the sea surface is below zero.
        assert!(temperature_c(70.0, 0.0, &params) < 0.0);
    }

    /// `cos` is even, so the hemispheres agree bit-for-bit rather than approximately.
    #[test]
    fn the_two_hemispheres_are_bit_identical() {
        let params = ClimateParams::canonical();
        for latitude in [0.0, 12.5, 37.0, 61.3, 89.0] {
            let north = temperature_c(latitude, 250.0, &params);
            let south = temperature_c(-latitude, 250.0, &params);
            assert_eq!(north.to_bits(), south.to_bits(), "at {latitude} deg");
        }
    }

    /// Below the datum there is ocean, and the ocean surface is at the datum. A trench does
    /// not get lapse *warming*, which is what an unfloored `elevation_m` would give it.
    #[test]
    fn nothing_below_the_datum_warms() {
        let params = ClimateParams::canonical();
        let surface = temperature_c(20.0, 0.0, &params);
        for depth in [-1.0, -250.0, -4000.0, -11000.0] {
            assert_eq!(
                temperature_c(20.0, depth, &params).to_bits(),
                surface.to_bits(),
                "depth {depth} m moved the temperature"
            );
        }
        // -0.0 is a different bit pattern from 0.0 and must not take a different branch.
        assert_eq!(temperature_c(20.0, -0.0, &params).to_bits(), surface.to_bits());
    }

    /// THE ONE THAT MATTERS. A NaN elevation must not come back as a plausible sea-level
    /// temperature -- which is exactly what the obvious `max(0, h)` would return, because
    /// every comparison against NaN is false.
    ///
    /// Proven red by mutation: replacing the guarded floor with
    /// `if elevation_m > 0.0 { elevation_m } else { 0.0 }` makes this the only failing test
    /// in the crate. Recorded in the task report.
    #[test]
    fn a_nan_elevation_is_not_answered_with_sea_level() {
        let params = ClimateParams::canonical();
        let answer = temperature_c(20.0, f64::NAN, &params);
        assert!(answer.is_nan(), "a NaN elevation was answered with {answer} C");
        // The blindness itself, asserted rather than described: the value the unguarded
        // form would have returned is a real, ordinary, believable temperature, which is
        // what would have made it invisible.
        let plausible = temperature_c(20.0, 0.0, &params);
        assert!(plausible > 15.0 && plausible < 25.0, "the swallowed value was {plausible} C");
    }

    /// The other three entrants, each propagating rather than saturating.
    #[test]
    fn every_nan_entrant_propagates() {
        let canonical = ClimateParams::canonical();
        assert!(temperature_c(f64::NAN, 100.0, &canonical).is_nan());
        for field in 0..3 {
            let mut params = canonical;
            match field {
                0 => params.equator_c = f64::NAN,
                1 => params.pole_c = f64::NAN,
                _ => params.lapse_c_per_km = f64::NAN,
            }
            // The lapse rate only reaches the answer through the elevation term, so it is
            // probed at an elevation. That is a real hole in NaN coverage if the probe is
            // at sea level, and it is why this loop uses 800 m rather than 0.
            let answer = temperature_c(20.0, 800.0, &params);
            assert!(answer.is_nan(), "field {field} gave {answer} C");
        }
    }

    /// Infinities do not become NaN by cancellation and do not saturate to something
    /// ordinary. An infinite elevation is infinitely cold; an infinite latitude has no
    /// cosine, so it is unanswerable.
    #[test]
    fn infinite_entrants_stay_loud() {
        let params = ClimateParams::canonical();
        assert_eq!(temperature_c(0.0, f64::INFINITY, &params), f64::NEG_INFINITY);
        // Below the datum is floored, so a negative infinity is the datum and not a
        // temperature of positive infinity.
        assert_eq!(
            temperature_c(0.0, f64::NEG_INFINITY, &params).to_bits(),
            temperature_c(0.0, 0.0, &params).to_bits()
        );
        assert!(temperature_c(f64::INFINITY, 0.0, &params).is_nan());
    }

    /// The parameter block is inert at `canonical()`: passing it explicitly is the same
    /// bits as the constants it is made of.
    #[test]
    fn canonical_matches_none_bit_for_bit() {
        let params = ClimateParams::canonical();
        assert_eq!(params.equator_c.to_bits(), EQUATOR_C.to_bits());
        assert_eq!(params.pole_c.to_bits(), POLE_C.to_bits());
        assert_eq!(params.lapse_c_per_km.to_bits(), LAPSE_C_PER_KM.to_bits());
    }

    /// The parameters are READ, not decorative. `CoastParams` needed its own inertness
    /// proof for exactly this reason: a bit-identity test between `None` and `canonical()`
    /// passes just as well when the block is ignored entirely.
    #[test]
    fn every_parameter_moves_the_answer() {
        let canonical = ClimateParams::canonical();
        let probe = |params: &ClimateParams| temperature_c(35.0, 1500.0, params);
        let base = probe(&canonical);
        for field in 0..3 {
            let mut params = canonical;
            match field {
                0 => params.equator_c += 5.0,
                1 => params.pole_c += 5.0,
                _ => params.lapse_c_per_km += 1.0,
            }
            assert_ne!(probe(&params).to_bits(), base.to_bits(), "field {field} was not read");
        }
    }
}
