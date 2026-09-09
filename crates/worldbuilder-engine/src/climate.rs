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
use crate::sphere::SpherePoint;
use crate::tangent::TangentFrame;

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

/// The elevation at which the mean annual temperature reaches freezing, in metres above the
/// datum. **The snow line -- and the point of it is that it is not a global elevation
/// threshold.**
///
/// Args:
/// latitude_deg: Degrees, positive north. `temperature_c`'s profile is `cos`, which is even,
/// so this is even too and no `abs` is needed to make it so.
/// params: `ClimateParams::canonical()` for the fitted profile.
///
/// Returns:
/// Metres above the datum. **A negative answer is meaningful and is not an error**: it says
/// the sea surface itself is below freezing at that latitude, so every elevation there is
/// frozen and a polar cap falls out with no rule of its own. On `canonical()` that begins at
/// 61.26 degrees.
///
/// # It INVERTS `temperature_c` rather than restating it
///
/// The body calls `temperature_c(latitude_deg, 0.0, params)` and divides; it is not a second
/// copy of the profile. That is worth a line of prose because a restated profile is a place
/// for two formulae to drift, and this project has found eight transcription defects across
/// six slices. `the_snow_line_is_where_the_temperature_says_it_is` asserts the **round
/// trip** -- that the temperature at the returned elevation is zero -- rather than asserting
/// the algebra, so a change to `temperature_c` that this function failed to follow is red.
///
/// # What it is NOT, stated because the difference is a metre count and not a nuance
///
/// It is the **mean annual** 0 C isotherm, because a mean annual field is all this module
/// computes. Earth's observed *permanent* snowline sits above its mean annual 0 C isotherm
/// wherever there is a summer to melt in, and the gap widens with latitude: in the tropics
/// the two are within a few hundred metres of each other, while at 60 degrees the annual
/// isotherm is at the datum and the observed snowline is a kilometre or more above it.
/// Closing that needs a seasonal amplitude -- a term this engine does not have, and one this
/// function does not invent. **The consequence is measured rather than argued**:
/// `climate_survey.rs`'s snow section reports what fraction of each world's land this line
/// puts under snow, against the fraction the viewer's previous linear band did and against
/// Earth's roughly 10% permanent ice cover.
///
/// # The NaN and infinity contract, inherited rather than re-decided
///
/// A NaN latitude propagates through `temperature_c`. A `lapse_c_per_km` of zero describes a
/// world where height does not cool, so there is no elevation at which it freezes: the answer
/// is an infinity signed by the sea-level temperature, or NaN at the one latitude where that
/// temperature is itself zero. Loud in every case, which is this module's standing contract.
pub fn freezing_elevation_m(latitude_deg: f64, params: &ClimateParams) -> f64 {
    1000.0 * temperature_c(latitude_deg, 0.0, params) / params.lapse_c_per_km
}

// ===========================================================================================
// MOISTURE -- the bounded upwind march. Task 2 of the climate slice.
// ===========================================================================================
//
// Everything above this line is a closed form. Everything below it is not, and the reason is
// physical rather than architectural: **rain that falls here fell out of air that came from
// somewhere else.** How much moisture is left at a point is the integral of what has already
// rained out along the path the air took to get here, so it cannot be answered from the
// point alone. The design the roadmap chose (§3.4) is to march that path at query time
// rather than to bake a raster, so an edited mountain casts its shadow immediately and the
// studio can never draw from an approximation the game disagrees with.
//
// # The three things the march is, stated before the constants
//
// 1. **A fetch meter.** Walking upwind until open water is reached measures the distance to
//    the sea *along the wind*, which is the term the temperature profile does not have.
//    Task 1's report named the absence: the engine exposes no distance-to-coast, so a west
//    coast and a continental interior get the same temperature. The march does not close
//    that for temperature -- it is still latitude and height -- but **moisture gets a real
//    continentality out of it, for free, because measuring the fetch is what the march
//    already does.**
// 2. **A rain-shadow meter.** Where the ground rises along the path, air is lifted, cools,
//    and sheds moisture; downwind of the crest there is less left. That is the whole
//    mechanism of the Atacama, the Great Basin and the Canterbury Plains.
// 3. **A bounded loop behind a nounwind boundary.** See `MarchBudget`.

/// How far apart the march's samples are, in metres.
///
/// # Ground
///
/// **This is `detail::COARSEST_WAVELENGTH_M`, and that is the argument for it.** Below 20 km
/// the terrain this engine builds is *detail* -- a seven-octave noise stack laid over
/// structure -- and detail is not what casts a rain shadow. Above 20 km the march starts
/// stepping over the structural ridges that do. The two claims are one claim: 20 km is the
/// scale at which this engine's ground stops being texture and starts being terrain.
///
/// It is also the resolution at which `elevation_m`'s `resolution_m` argument has already
/// faded every configured octave, which is why the spike measured `res = 20 km` and
/// `res = 80 km` as the same column. A caller who coarsens the march to buy the spike's
/// measured 27% is coarsening it to exactly this step.
///
/// **Measured, not asserted.** Against a 5 km reference march at a fixed 3,200 km span over
/// every land point of three worlds, a 20 km step differs by a mean of 0.037-0.044 in the
/// moisture index. Halving to 10 km halves that (0.015-0.019) for twice the cost, and
/// doubling to 40 km nearly doubles it (0.068-0.075). **The step error is first order and
/// has no knee either** -- it is a straight purchase, exactly like the sample budget, and
/// the reason the samples are spent on span instead is that the span error at the budget
/// this file rejects is *five times larger*. See `MARCH_SAMPLES`.
pub const MARCH_STEP_M: f64 = 20_000.0;

/// How many steps the march takes, upwind, before it gives up and calls the air saturated.
///
/// **The spike proved there is no performance knee: cost is affine in this number from 1 to
/// 320 samples, natively and in WASM. So this is a physics decision and it is made on
/// measurements of these worlds, not on a cost curve.**
///
/// # Why 160, when the spike benchmarked 40
///
/// Because 40 is not a rain-shadow budget on a planet whose continents are this wide, and
/// that is measured rather than argued.
///
/// **The fetch measurement.** Over 50,000 Fibonacci points per world on the four worlds
/// `climate_survey.rs` uses, marching upwind in 20 km steps until `elevation_m <= 0`, the
/// distance to open water along the wind has a **median of 1,020 to 2,200 km** and a
/// **90th percentile of 2,860 to 4,780 km**. A 40-sample, 800 km march reaches open water
/// for only **20% to 42%** of land points. On the majority of this engine's land, the
/// spike's budget never leaves the continent, so it never measures a fetch at all.
///
/// **The convergence measurement, which is the one that sets the number.** Against a
/// reference march of 8,000 km at the same 20 km step, over every land point of three
/// worlds, the mean absolute error in the moisture index is:
///
/// ```text
///   span   800 km ( 40 samples)   0.178 - 0.270      <- the spike's shape
///   span  1600 km ( 80 samples)   0.041 - 0.085
///   span  2560 km (128 samples)   0.006 - 0.019
///   span  3200 km (160 samples)   0.002 - 0.007      <- chosen
///   span  4800 km (240 samples)   0.0001 - 0.0007
/// ```
///
/// The moisture index runs over `[0, 1]` and Task 3 cuts four or five bands out of it, so a
/// band is order 0.2 wide. **A 40-sample march is wrong by one to one-and-a-half whole
/// bands. A 160-sample march is wrong by three percent of one.** That is the entire
/// argument, and it is why the samples are spent on reach rather than on step: at a fixed
/// 3,200 km span, halving the step from 20 km to 10 km buys 0.02 of accuracy for 160 more
/// samples, while the last 800 km of *span* was worth 0.06 for eighty.
///
/// # What it costs, and what that means for Task 4
///
/// On the spike's own curve, `0.48 + 0.48*N` native and `2.0 + 1.5*N` in WASM: **77 us
/// native and 242 us in WASM per query**, about 4x the spike's headline 40-sample figure.
/// The spike's other finding therefore binds harder than it did: **the raster is the lever
/// and it is quadratic.** At the browser's measured ~2.7 us per elevation, a 160-sample
/// march is ~432 us per moisture query, so a 32x32 moisture raster (1,156 sampled texels)
/// is ~500 ms per tile and a **16x16 one is ~140 ms, which is the parity-with-relief figure
/// the spike put at 32x32 for a 40-sample march.** Task 4 should expect to ship 16x16, not
/// the 32x32 the spike named, and the reason is this constant rather than a regression.
pub const MARCH_SAMPLES: u16 = 160;

/// The ceiling `MarchBudget` refuses to be built above. See that type for why it exists at
/// all; this is where the number comes from.
///
/// The spike measured `count = 2^20` at 0.643 s and extrapolated `u32::MAX` to **~2,600
/// seconds inside one uninterruptible call** -- and `extern "C"` is nounwind, so that is a
/// hang and not an abort. A bare `u16` would already cap the damage at 65,535 samples, about
/// 31 ms native and 100 ms in WASM, which is survivable but is a frame budget's worth of
/// freeze for a single point.
///
/// **1,024 is 6.4x the canonical budget and 246 us native / 2.8 ms in the browser at the
/// worst.** It is drawn at a round number rather than at the exact point where the cost
/// becomes objectionable, for the reason `wasm.rs` gives for `land_fraction`'s bound: an
/// exact boundary is an accident of the host it was measured on and would move if the host
/// did. What matters is that the loop bound is **finite by construction and small**, and
/// that no caller -- Rust or C -- can reach past it.
pub const MAX_MARCH_SAMPLES: u16 = 1_024;

/// Metres of orographic lift that remove `1 - 1/e` of the air's remaining moisture.
///
/// # Ground -- three real rain shadows, fitted
///
/// Each of these is a crest height and the ratio of leeward to windward precipitation, and
/// each implies a scale through `ratio = exp(-crest / scale)`:
///
/// | range | crest | lee / windward | implied scale |
/// | --- | --- | --- | --- |
/// | Sierra Nevada, Great Basin behind it | ~2,500 m | ~0.20 | 1,553 m |
/// | Southern Alps, Canterbury behind them | ~2,000 m | ~0.06 | 712 m |
/// | Andes, Atacama behind them | ~4,000 m | ~0.01 | 868 m |
///
/// The three do not agree with each other -- they span a factor of 2.2 -- because real rain
/// shadows also depend on wind speed, sea temperature and how far the lee station is from
/// the crest, none of which this model has. **Their geometric mean is 986 m and the constant
/// is 1,000 m**, which reproduces the three ratios as 0.082 / 0.135 / 0.018 against the
/// observed 0.20 / 0.06 / 0.01: worst factor 2.4, in a quantity whose observations disagree
/// by 2.2 among themselves. `a_thousand_metres_of_lift_shed_the_fitted_fraction` pins it.
///
/// The alternative -- fitting one shadow exactly -- would be a number with a smaller stated
/// error and a larger real one, which is the shape this project has already found four
/// times in transcribed figures.
pub const LIFT_SCALE_M: f64 = 1_000.0;

/// Metres of overland travel that remove `1 - 1/e` of the air's remaining moisture, with no
/// lifting at all.
///
/// This is the term that makes a continental interior drier than its coast on flat ground,
/// and it is the honest half of what a distance-to-coast field would give: **the march is
/// already walking the fetch, so charging for it costs nothing extra.**
///
/// # Ground -- two continental transects
///
/// | transect | inland distance | coastal / interior rainfall | implied scale |
/// | --- | --- | --- | --- |
/// | Atlantic coast to central Kazakhstan, along the westerlies | ~4,000 km | 800 / 200 mm | 2,885 km |
/// | Queensland coast to central Australia | ~2,000 km | 1,200 / 280 mm | 1,372 km |
///
/// Geometric mean 1,990 km; the constant is **2,000 km**. Both transects cross some relief,
/// so both implied scales are if anything too short (they attribute orographic loss to
/// fetch), which makes 2,000 km a conservative -- wetter -- choice. Stated because it is a
/// bias with a known sign rather than an error bar.
pub const FETCH_SCALE_M: f64 = 2_000_000.0;

/// Metres of travel over open water that close `1 - 1/e` of the gap back to saturation.
///
/// # Ground
///
/// Air crossing open water re-moistens from below, fast. The sharpest everyday evidence is
/// lake-effect snow, which needs roughly **100 km of open-water fetch** to organise and is
/// fully developed across the ~400 km of the Sea of Japan -- an air mass that arrives
/// continental-dry and leaves saturated. **300 km** puts 28% of the recovery inside the
/// 100 km threshold and 74% inside 400 km, which is the shape those two landmarks describe.
///
/// **And it barely matters, which is measured rather than hoped.** Any scale short against
/// the span saturates the air long before it makes landfall, so the constant's exact value
/// is nearly invisible: moved over a **factor of ten**, 100 km to 1,000 km, on terrain built
/// specifically to expose it (a far continent, then a sea, then a coastal range), it shifts
/// the moisture index by **0.0113** -- about a twentieth of one of Task 3's bands.
/// `the_recharge_scale_is_not_a_sensitive_parameter` pins that, and pins that it is not
/// zero: a constant nothing reads and a constant nothing depends on are different things,
/// and only the first is a defect.
///
/// It is fitted to two landmarks rather than three for that reason. A third would be effort
/// spent on a digit the answer cannot see.
pub const RECHARGE_SCALE_M: f64 = 300_000.0;

/// Where the trade-wind belt gives way to the westerlies, in degrees of latitude.
///
/// Earth's three-cell circulation, and the only piece of atmospheric dynamics in this file:
/// **easterlies from the equator to 30, westerlies from 30 to 60, polar easterlies beyond**.
/// The two boundaries are the Hadley and Ferrel cell edges and they are the standard
/// idealisation, which is what this model wants -- a real jet stream meanders and a real
/// intertropical convergence zone migrates with the season, and this field has no season.
pub const TRADE_WIND_EDGE_DEG: f64 = 30.0;

/// Where the westerlies give way to the polar easterlies. See `TRADE_WIND_EDGE_DEG`.
pub const POLAR_EASTERLY_EDGE_DEG: f64 = 60.0;

/// A march length that **cannot be constructed out of range**, which is the whole of its
/// job.
///
/// # Why a type and not a bounds check
///
/// The plan's instruction is *bound the loop in the type, not only in the export*, and the
/// reason is reach rather than tidiness. The spike measured a `count` of `2^20` at 0.643 s
/// and extrapolated `u32::MAX` to about **2,600 seconds inside one uninterruptible call**;
/// `extern "C"` is nounwind, so what a host sees is a **hang**, not an abort it can catch.
/// Refusing at the export closes the C ABI and leaves every Rust caller of
/// `climate::moisture_index` and `Surface::moisture_index` holding the same unbounded loop
/// -- and this crate is a library whose Rust surface is `pub`.
///
/// So the bound lives where the value does. `MarchBudget::new` is the only constructor,
/// there is no public field and no `Default`, and it returns `None` above
/// `MAX_MARCH_SAMPLES`. **A `MoistureParams` therefore cannot be built with an unbounded
/// march, which means the loop in `moisture_index` has a finite bound that no caller and no
/// export can widen.** That is the same first-line/second-line split `nan-abyss-fix.md`
/// records for `WB_MAX_COAST_GAIN`: the type refuses, and Task 4's export will still name
/// the offending field in a status code, because a refusal that says which argument was
/// wrong is worth more than one that only says no.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarchBudget(u16);

impl MarchBudget {
    /// `None` above `MAX_MARCH_SAMPLES`. There is no other way to make one.
    ///
    /// Zero is admitted deliberately: a zero-step march is a well-defined question with a
    /// well-defined answer -- the air has travelled nowhere, so it is still saturated -- and
    /// refusing it would be refusing the identity element rather than an error.
    pub fn new(samples: u16) -> Option<Self> {
        if samples > MAX_MARCH_SAMPLES {
            None
        } else {
            Some(Self(samples))
        }
    }

    /// `MARCH_SAMPLES` steps: the budget the convergence measurement chose.
    pub fn canonical() -> Self {
        Self(MARCH_SAMPLES)
    }

    pub fn samples(self) -> u16 {
        self.0
    }
}

/// What the air does on its way here, broken out the way `ClimateParams` breaks out what it
/// is like when it arrives.
///
/// `Surface::moisture_index` takes `Option<MoistureParams>` with `None` canonical -- the
/// fifth opt-in block of that kind in this crate and the second in this file. It is a
/// **method** argument for the same reason `ClimateParams` is: the march holds no state, so
/// there is nothing for a constructor to build and nothing for `Surface` to store, and
/// `lib.rs::the_surface_is_not_modified_by_this_slice` still pins that struct at eight
/// fields by name.
///
/// The budget is a `MarchBudget` rather than a `u16` so that a `MoistureParams` cannot
/// carry an unbounded loop; see that type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MoistureParams {
    /// How many upwind steps to take. Bounded by construction.
    pub budget: MarchBudget,
    /// How far apart those steps are, in metres.
    pub step_m: f64,
    /// Metres of orographic lift that shed `1 - 1/e` of the remaining moisture.
    pub lift_scale_m: f64,
    /// Metres of overland fetch that shed `1 - 1/e` of the remaining moisture.
    pub fetch_scale_m: f64,
    /// Metres of over-water fetch that close `1 - 1/e` of the gap back to saturation.
    pub recharge_scale_m: f64,
}

impl MoistureParams {
    /// The measured march: 160 steps of 20 km, 3,200 km of reach, and the three fitted
    /// scales. `Some(MoistureParams::canonical())` is bit-identical to `None`.
    pub fn canonical() -> Self {
        Self {
            budget: MarchBudget::canonical(),
            step_m: MARCH_STEP_M,
            lift_scale_m: LIFT_SCALE_M,
            fetch_scale_m: FETCH_SCALE_M,
            recharge_scale_m: RECHARGE_SCALE_M,
        }
    }
}

/// Which way the air came from, as a component along the local east axis: `+1` if it came
/// from the east, `-1` if it came from the west.
///
/// Earth's three-cell zonal circulation, and nothing else -- see `TRADE_WIND_EDGE_DEG`.
///
/// # Two approximations, stated rather than discovered later
///
/// **The wind is purely zonal.** Real trades blow from the north-east in the northern
/// hemisphere and the south-east in the southern, and the westerlies have a matching
/// poleward tilt. Dropping the meridional component means a north-south coastline collects
/// all of the rain on this model and an east-west one collects none, which is what a purely
/// zonal flow would genuinely do; it is the band structure, not the tilt, that produces the
/// deserts, and the tilt is the cheaper thing to add later.
///
/// **A NaN latitude takes the easterly arm, and that is not a swallow.** Both comparisons
/// against a NaN are false, so an unanswerable latitude falls through to `+1`. It is left
/// that way on purpose, for the reason Task 1's report gives at length: a NaN latitude can
/// only arise from a point whose vector is non-finite, `noise.rs`'s lattice guard already
/// makes `elevation_m` answer NaN for every such point, and the march multiplies by those
/// elevations -- so the NaN arrives through the elevation whatever the wind says. Task 1
/// wrote exactly this guard one level up, mutation-tested it, found the whole suite green
/// without it and **deleted it**. A second dead guard is not defence in depth.
pub fn upwind_east(latitude_deg: f64) -> f64 {
    let from_equator = latitude_deg.abs();
    if from_equator >= TRADE_WIND_EDGE_DEG && from_equator < POLAR_EASTERLY_EDGE_DEG {
        -1.0
    } else {
        1.0
    }
}

/// How much moisture the air still has when it gets here, as a dimensionless index.
///
/// Args:
/// latitude_deg: The query point's latitude, which picks the prevailing wind band.
/// frame: A `TangentFrame` at the query point. The march walks its local east axis, so the
/// path is a **geodesic launched due east or due west** -- not a parallel of latitude,
/// which a straight line on a sphere is not. Over 3,200 km from 45 degrees that path drifts
/// polewards, and that is the physically right thing for a parcel travelling in a straight
/// line to do.
/// params: `MoistureParams::canonical()` for the measured march.
/// elevation_at: How high the ground is at a probe point. This is the `substrate::at`
/// shape -- a stateless stage in this crate is a free function reached through a sampler
/// rather than a method on a layer -- and it is what lets the rain-out arithmetic be tested
/// against a synthetic ridge with no `Surface` at all.
///
/// Returns:
/// A number in `[0, 1]`, or **NaN for params outside the domain** -- a negative `step_m` or
/// a scale that is not strictly positive; see the guard at the top of the body, which was
/// written because a sweep found 536 out-of-range answers and not because the code looked
/// wrong. 1 is saturated marine air,
/// 0 is air that has rained out completely. **It has no unit and no anchor**, which is
/// exactly why Task 3 quantiles it and does not quantile temperature. There is no moisture
/// equivalent of water freezing at zero.
///
/// # How it walks, and why downwind
///
/// The samples are laid out from `budget` steps upwind down to the query point, and the
/// accumulator runs **downwind**, in the direction the air actually travels, because
/// rain-out is causal: what falls on the second ridge depends on what the first one already
/// took. Walking the other way and reversing the arithmetic would need the elevations kept
/// in an array, and an array sized by a runtime budget is either a heap allocation on a
/// path that must run in WASM or a fixed buffer sized by the ceiling. Marching downwind
/// needs neither: the far end is just an offset, so the loop counts down and carries two
/// floats.
///
/// At each step:
///
/// - **over water** (`elevation <= 0`) the air relaxes back towards saturation,
///   `m <- 1 - (1 - m) * exp(-step / recharge_scale)`;
/// - **over land** it loses `exp(-lift / lift_scale)` for whatever the ground rose since the
///   last sample, and then `exp(-step / fetch_scale)` for having travelled over land at all.
///
/// **Every factor is an `exp` of a non-positive number, so the result stays in `[0, 1]` by
/// arithmetic and there is no clamp anywhere in this function.** That is not a stylistic
/// point: `f64::min`, `f64::max` and `.clamp(` are NaN-asymmetric and banned in this crate,
/// and a march written with a clamp is a march that would have swallowed exactly the NaN the
/// spike found.
///
/// # The NaN contract is PROPAGATE, and the spike found the trap
///
/// The spike's own note, verbatim: *"A `NaN` step silently returns full moisture rather than
/// `NaN`. `lift_m` is `NaN`, `if lift_m > 0.0` is false, and the accumulator is never
/// touched -- so a host that passes garbage gets a plausible answer instead of an obviously
/// wrong one."* That is the fifth instance of this project's recurring failure -- the abyss
/// that read as the deepest ocean, the NaN land fraction that produced a legitimate all-land
/// world, the lattice cell that could not name itself, and Task 1's own NaN elevation that
/// would have read as sea level.
///
/// **So the lift test is a NaN test first and a comparison second**, the same shape as
/// `temperature_c`'s elevation floor. The house decision on the three NaN entrants that
/// converged on `elevation_from_above` was propagate, for three reasons that all still hold
/// here: there is no Python oracle to contradict, a panic across a nounwind boundary is an
/// abort rather than a loud failure, and refusing at the boundary cannot cover callers that
/// pass bare scalars. A moisture nobody can compute comes back NaN.
///
/// **What is deliberately NOT guarded**, because guarding it would be the dead code Task 1
/// deleted: a non-finite `step_m` makes every probe offset non-finite, `local_to_sphere`
/// carries that into the point, and `elevation_m`'s lattice guard answers NaN -- so the NaN
/// arrives through the sampler. `a_nan_step_length_does_not_return_full_moisture` asserts
/// that it arrives, rather than asserting that a guard here would catch it.
///
/// # What a zero step means
///
/// `step_m = 0.0` marches nowhere: every probe is the query point, nothing rises, and no
/// fetch is travelled, so the answer is exactly `1.0`. That is arithmetically correct rather
/// than a fall-through -- a path of zero length loses nothing -- and it is pinned so that a
/// future edit cannot make it mean "the loop did not run".
pub fn moisture_index(
    latitude_deg: f64,
    frame: &TangentFrame,
    params: &MoistureParams,
    elevation_at: &dyn Fn(&SpherePoint) -> f64,
) -> f64 {
    // THE DOMAIN, REFUSED AT THE DOOR RATHER THAN HONOURED, and found by sweeping rather
    // than by reading the code. Each comparison is NEGATED so that a NaN takes the same
    // door -- a NaN fails `>=` and `>` alike and the `!` makes that `true`, which is the
    // `noise.rs::LATTICE_LIMIT` form and the reason `f64::min`/`max`/`clamp` are banned.
    //
    // What the sweep found: over 37,818 calls across three cross products, **536 returned
    // a value outside `[0, 1]`** -- infinities and numbers of order 1e25 -- and every one
    // came from this band rather than from a cliff:
    //
    // - a **negative `step_m`** makes `exp(-step / fetch_scale)` greater than one, so the
    //   fetch term *adds* moisture every step and the index runs away. A negative length is
    //   not a shorter march, it is the wind blowing backwards through an amplifier.
    // - a **zero scale** divides by zero, so `exp` of an infinity is an infinity or a zero,
    //   and the index leaves the interval in one step. `0.0` is non-negative, which is why
    //   this bound is `> 0.0` and not `>= 0.0`; that distinction is the whole guard.
    // - a **negative scale** flips the sign of every exponent, turning rain-out into
    //   rain-in.
    //
    // None of those is a plausible answer a caller could act on, and an out-of-range index
    // would silently break every band Task 3 cuts out of `[0, 1]`. So the answer is the one
    // this ABI already speaks: **NaN for a question that cannot be answered.** Infinite
    // scales are deliberately ADMITTED -- `exp(-x / inf)` is `1`, meaning "this term never
    // fires", which is a coherent request and stays in range.
    if !(params.step_m >= 0.0)
        || !(params.lift_scale_m > 0.0)
        || !(params.fetch_scale_m > 0.0)
        || !(params.recharge_scale_m > 0.0)
    {
        return f64::NAN;
    }
    let east = upwind_east(latitude_deg);
    // Land height above the datum at `steps` steps upwind. Below the datum is open water and
    // reads as exactly zero, matching `temperature_c`'s floor and for the same reason: the
    // sea surface is at the datum, so a trench is not a valley the air has to climb out of.
    // NaN first, comparison second -- an unanswerable elevation must not read as sea.
    let land_height_m = |steps: u16| -> f64 {
        let probe = frame.local_to_sphere(east * params.step_m * f64::from(steps), 0.0);
        let height = elevation_at(&probe);
        if height.is_nan() {
            height
        } else if height > 0.0 {
            height
        } else {
            0.0
        }
    };

    let budget = params.budget.samples();
    let mut moisture = 1.0f64;
    let mut previous_m = land_height_m(budget);
    // The seed sample is read only as the *previous* height, so a NaN there is invisible to
    // every arm below if the first step downwind is over water: the recharge arm does not
    // look at `previous_m` at all, and the air would come back saturated from a path whose
    // far end could not be answered. That is the spike's defect wearing a third branch, and
    // it is the one arm no mutation of the loop body can reach.
    if previous_m.is_nan() {
        moisture = f64::NAN;
    }
    let mut remaining = budget;
    while remaining > 0 {
        remaining -= 1;
        let height_m = land_height_m(remaining);
        if height_m > 0.0 {
            let lift_m = height_m - previous_m;
            // NO NaN GUARD HERE, AND THAT IS MEASURED RATHER THAN ASSUMED. The obvious
            // reading of the spike's defect is that this line needs one: `lift_m > 0.0` is
            // false for a NaN, so an unanswerable step would leave the accumulator
            // untouched. **One was written here, and mutation M1 deleted it with the whole
            // 682-test suite still green.** It was dead, and the proof is structural rather
            // than empirical: `lift_m` can only be NaN if `height_m` or `previous_m` is, and
            // `previous_m` is last iteration's `height_m`. A NaN `height_m` fails
            // `height_m > 0.0` and lands in the water arm below, whose first branch poisons
            // the accumulator -- so by the time a NaN can reach this subtraction, `moisture`
            // is already NaN and the multiply carries it regardless. The two guards that ARE
            // here (the water arm, and the seed before the loop) are each proven red on
            // their own by M2 and M3. **Dead code looks like a feature**, so this is a
            // comment and not a branch.
            if lift_m > 0.0 {
                moisture *= m::exp(-lift_m / params.lift_scale_m);
            }
            moisture *= m::exp(-params.step_m / params.fetch_scale_m);
        } else if height_m.is_nan() {
            // Open water is `height_m <= 0.0`, and a NaN fails that comparison as surely as
            // it fails the land one. Without this arm a NaN elevation would be recharged
            // towards saturation and come back as marine air -- the spike's defect wearing
            // the other branch.
            moisture = f64::NAN;
        } else {
            moisture = 1.0 - (1.0 - moisture) * m::exp(-params.step_m / params.recharge_scale_m);
        }
        previous_m = height_m;
    }
    moisture
}

// ============================================================================================
// Bands: three axes, and the non-uniform spacing that beats even spacing
// ============================================================================================

/// The absolute temperature band edges, in degrees C: polar / boreal / temperate /
/// subtropical / tropical.
///
/// **This is the one axis of the three that is not quantiled, and that is a measurement
/// rather than a preference.** The photoreal slice quantiled it, and the owner world's
/// subtropical desert latitudes -- 25 degrees, 22 C -- fell into the *median* temperature
/// band, so `hot desert`, the brightest land colour in that palette, became unreachable and
/// the desert belt came out `cold desert`. The cause is structural: a quantile can only say
/// "warmer than this fraction of *this* world's land", and water freezes at 0 C on every
/// world. Holdridge -- the model WorldEngine says it implements -- puts its temperature axis
/// in degrees for the same reason, and WorldEngine quantiles it only because its temperature
/// layer is unitless noise. **This engine's is degrees**, from a profile fitted to Earth's
/// zonal means and a standard lapse rate (see `temperature_c`).
///
/// The edges themselves are physical: 0 is freezing, 8 C is roughly the boreal/temperate
/// annual-mean transition, 18 C is Koppen's own A/C boundary, and 24 C separates a
/// subtropical mean from a tropical one.
///
/// **The honest cost, which is the reason this constant carries a survey and the two
/// quantiled axes do not:** an absolute band can go unvisited on a world whose land is all
/// one climate. That is correct rather than dead -- a world with no polar land should have
/// no tundra -- and it is measured in `src/bin/climate_survey.rs` rather than hoped for.
/// Task 1 found all five reached on all four survey worlds, the owner's thinnest at 2.7% of
/// its land.
///
/// The value is byte-identical to `viewer/public/app/biome.js::TEMP_BAND_EDGES_C`, which
/// Task 4 replaces with this one. That is checked by reading, not by a test: a test here
/// that reads a viewer file would go red the moment Task 4 deletes the constant it asserts
/// against, which is a gate that fires on the work it exists to admit.
pub const TEMP_BAND_EDGES_C: [f64; 4] = [0.0, 8.0, 18.0, 24.0];

/// How many temperature bands `TEMP_BAND_EDGES_C` cuts.
pub const TEMPERATURE_BANDS: usize = TEMP_BAND_EDGES_C.len() + 1;

/// **The bell, as equally spaced z-scores rather than as transcribed percentages.**
///
/// WorldEngine's `humidity.py` carries the finding and not the reason: *"These were
/// originally evenly spaced at 12.5% each but changing them to a bell curve produced better
/// results."* Its shipped humidity quantiles
/// `[0.941, 0.778, 0.507, 0.236, 0.073, 0.014, 0.002]` are visibly that bell -- band widths
/// 5.9 / 16.3 / 27.1 / 27.1 / 16.3 / 5.9 / 1.2 / 0.2 percent of land, narrow tails and a
/// wide middle.
///
/// Rather than copy seven magic numbers, this derives the same shape from its generator:
/// **the normal CDF of equally spaced z-scores**. At `+-0.5` and `+-1.5` that gives
/// quantiles 0.0668 / 0.3085 / 0.6915 / 0.9332, i.e. band widths of
/// **6.7 / 24.2 / 38.3 / 24.2 / 6.7 percent** of a world's land -- arid, dry, moist, wet,
/// perhumid.
///
/// **Five bands, not the roadmap's four.** The roadmap wrote four evenly-spaced bands and the
/// research note said to revise that on the evidence. Five is what the consumer needs:
/// `biome.js`'s interior table is a 5 x 5 rectangle against the five temperature bands, so a
/// fourth moisture band would leave a whole column of it unreachable -- the same defect this
/// project has already shipped nine times in one palette. WorldEngine's eight would leave
/// three columns empty.
///
/// **Why the tails are not narrower.** WorldEngine's own extremes are 1.2% and 0.2% of land,
/// which at this engine's 4,000-point calibration would be read from about the 14th and 2nd
/// order statistic of some 1,200 land samples -- an edge placed on a handful of points. 6.7%
/// is about 80 samples, which is a quantile rather than an anecdote.
pub const MOISTURE_BELL_Z: [f64; 4] = [-1.5, -0.5, 0.5, 1.5];

/// How many moisture bands `MOISTURE_BELL_Z` cuts.
pub const MOISTURE_BANDS: usize = MOISTURE_BELL_Z.len() + 1;

/// The landform axis, as quantiles of a world's own land elevation: lowland / interior /
/// montane.
///
/// **Placed against Earth's land hypsometry rather than picked**: roughly a quarter of
/// Earth's land is coastal plain, and the ground that carries bare rock, alpine vegetation
/// and permanent snow is the top 15% or so. Reading them as quantiles rather than as metres
/// is what stops the family of dead bands this project has shipped three of -- a quantile is
/// reached on every world by construction, however low that world's mountains.
///
/// **The bottom band is named `lowland` and not `coastal`, and that word is a finding.**
/// `biome.js` calls it coastal, because the bottom quartile of land elevation is the best
/// stand-in available to it, and the plan for this slice says Task 4 should replace it with
/// "a real coastal term". **This task cannot supply one.** The engine exposes no
/// distance-to-coast, the plan's brief for this task says that is a finding to report and not
/// a field to invent, and a hypsometric quartile is a *lowland* test that correlates with
/// coast rather than a coastal one -- an inland basin at 40 m is in it and a cliff coast at
/// 400 m is not. The band is real and useful, and the name says what it measures rather than
/// what a consumer would like it to mean.
pub const LANDFORM_QUANTILES: [f64; 2] = [0.25, 0.85];

/// How many landform bands `LANDFORM_QUANTILES` cuts.
pub const LANDFORM_BANDS: usize = LANDFORM_QUANTILES.len() + 1;

/// The calibration population, and it is `continentality::CALIBRATION_SAMPLES` on purpose.
///
/// **This is the grid-free order statistic this project already owns.** WorldEngine finds
/// its band edges by bisecting over a global masked array; a global array is the one thing
/// the spec rules out here, because the studio must not draw from a separately baked
/// approximation that could disagree with the game. `continentality.rs::calibrate` has
/// answered the same question -- "where does the q-th quantile of this field over this
/// planet fall" -- since slice 0: a fixed area-uniform Fibonacci spiral, sorted, read at an
/// order statistic. **No second mechanism was built.**
///
/// 4,000 points is not arbitrary there and is not arbitrary here. The spiral is area-uniform,
/// so on a 29%-land world about 1,160 samples survive the land test, and the standard error
/// of the q-th order statistic in probability is `sqrt(q(1-q)/n)` -- 0.7 percentage points at
/// the 6.7% edge and 1.4 at the 30.9% edge. That is finer than the bands are wide.
/// `the_band_calibration_uses_continentalitys_own_population` asserts the two constants are
/// the same bits, so if that layer ever moves its population this says so rather than leaving
/// the argument quietly false.
pub const BAND_CALIBRATION_SAMPLES: usize = crate::continentality::CALIBRATION_SAMPLES;

/// The standard normal CDF, Abramowitz & Stegun 26.2.17.
///
/// Maximum absolute error 7.5e-8, which is five orders of magnitude finer than the sampling
/// error of the order statistic it feeds (see `BAND_CALIBRATION_SAMPLES`), so the
/// approximation is invisible at the resolution these quantiles are read at.
///
/// It exists so `MOISTURE_BELL_Z` can be a bell rather than a table of transcribed
/// percentages: **the spacing is derived from its generator here, and a reader changes the
/// shape by moving one z-score.** `the_bell_quantiles_are_the_normal_cdf_of_the_z_scores`
/// pins it against the textbook values.
pub fn normal_cdf(z: f64) -> f64 {
    let t = 1.0 / (1.0 + 0.231_641_9 * z.abs());
    let d = 0.398_942_280_401_432_7 * m::exp(-0.5 * z * z);
    let p = d
        * t
        * (0.319_381_530
            + t * (-0.356_563_782
                + t * (1.781_477_937 + t * (-1.821_255_978 + t * 1.330_274_429))));
    if z >= 0.0 {
        1.0 - p
    } else {
        p
    }
}

/// `MOISTURE_BELL_Z` as quantiles: 0.0668 / 0.3085 / 0.6915 / 0.9332.
pub fn moisture_quantiles() -> [f64; MOISTURE_BELL_Z.len()] {
    let mut out = [0.0f64; MOISTURE_BELL_Z.len()];
    let mut index = 0;
    while index < MOISTURE_BELL_Z.len() {
        out[index] = normal_cdf(MOISTURE_BELL_Z[index]);
        index += 1;
    }
    out
}

/// The `q`-th order statistic of an ascending sample, by truncation.
///
/// `continentality::calibrate`'s own form -- `values[(q * last) as usize]` -- rather than a
/// linear interpolation between neighbours. Truncating returns a value the field actually
/// produced somewhere on the planet, which is what a band edge should be; interpolating
/// invents one. On a thousand-odd samples the two differ by far less than the sampling error
/// either way.
fn order_statistic(sorted: &[f64], q: f64) -> f64 {
    let last = (sorted.len() - 1) as f64; // cast-ok: count to float, exact far below 2^53
    let index = (q * last) as usize; // cast-ok: truncation, matching continentality::calibrate
    sorted[index]
}

/// One world's band edges: the two axes that are quantiles of its own land.
///
/// Temperature is not here, because it is not calibrated -- see `TEMP_BAND_EDGES_C`.
///
/// # What the quantile buys, stated as the defect it prevents
///
/// A band edge in absolute units is a band that can be empty. This project has shipped
/// **nine of thirty-three palette colours unreachable** because a noise field's real standard
/// deviation was a fifth of its nominal one, and **three colour blends that were never once
/// selected**. Because a moisture or landform edge here is an order statistic of this world's
/// own land, **every band it cuts is occupied by construction** -- and the only way that can
/// fail is a *tie*, which is why `calibrate` reports `land_samples` and why
/// `src/bin/climate_survey.rs` measures occupancy rather than assuming it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BandEdges {
    moisture: [f64; MOISTURE_BELL_Z.len()],
    landform: [f64; LANDFORM_QUANTILES.len()],
    land_samples: usize,
}

impl BandEdges {
    /// Calibrate against a world, by the same Fibonacci order statistic
    /// `continentality::calibrate` uses.
    ///
    /// Args:
    /// elevation_at: How high the ground is at a probe point. Sampled at every one of
    /// `BAND_CALIBRATION_SAMPLES` points; the land test is `> 0.0`, matching every other
    /// land test in this crate.
    /// moisture_at: The moisture index at a probe point. Sampled **only on land**, which is
    /// what makes this affordable: a moisture query is `MARCH_SAMPLES + 1` elevation
    /// queries, so calibrating over the whole sphere would be three times the work for a
    /// distribution nothing bands.
    ///
    /// Returns:
    /// Edges for both quantiled axes, or **NaN edges** if any sample could not be answered,
    /// or if the world has no land at all. Both cases are real: a NaN elevation makes a NaN
    /// moisture by `moisture_index`'s own contract, and an all-ocean world has no land
    /// distribution to take a quantile of. **A world with no land must not report the edges
    /// of a world that has some** -- that is the shape of the NaN land fraction that produced
    /// a world bit-identical to a legitimate all-land one, and `band_index` refuses a NaN
    /// edge rather than banding against it.
    pub fn calibrate(
        elevation_at: &dyn Fn(&SpherePoint) -> f64,
        moisture_at: &dyn Fn(&SpherePoint) -> f64,
    ) -> Self {
        let golden = core::f64::consts::PI * (3.0 - m::sqrt(5.0));
        let n = BAND_CALIBRATION_SAMPLES;
        let mut heights: Vec<f64> = Vec::new();
        let mut wetness: Vec<f64> = Vec::new();
        let mut unanswerable = false;

        for index in 0..n {
            // The spiral, in `continentality::calibrate`'s own form: cell centres rather
            // than edges, so neither pole is sampled twice, and the vector is deliberately
            // NOT normalised -- that is what the Python oracle hands to `SpherePoint`, and
            // this construction has to stay the same one for the claim in the doc comment to
            // be true.
            let z = 1.0 - 2.0 * (index as f64 + 0.5) / (n as f64); // cast-ok: loop counter to float, no truncation
            let inner = 1.0 - z * z;
            let ring = m::sqrt(if inner > 0.0 { inner } else { 0.0 });
            let angle = golden * index as f64; // cast-ok: loop counter to float, no truncation
            let point = SpherePoint {
                vector: crate::vectors::Vec3::new(m::cos(angle) * ring, m::sin(angle) * ring, z),
            };
            let height = elevation_at(&point);
            if height.is_nan() {
                unanswerable = true;
                continue;
            }
            if !(height > 0.0) {
                continue;
            }
            let wet = moisture_at(&point);
            if wet.is_nan() {
                unanswerable = true;
                continue;
            }
            heights.push(height);
            wetness.push(wet);
        }

        let land_samples = heights.len();
        if unanswerable || land_samples == 0 {
            return Self {
                moisture: [f64::NAN; MOISTURE_BELL_Z.len()],
                landform: [f64::NAN; LANDFORM_QUANTILES.len()],
                land_samples,
            };
        }

        // `total_cmp` rather than `partial_cmp(..).expect(..)`: this crate is reached through
        // `extern "C"`, where a panic is an abort rather than something a host can catch, and
        // an ordering that panics on a value the guard above has already refused would be a
        // second mechanism for the same fact -- which is how fourteen assertions in this
        // project came to look load-bearing and not be. The guard is the load-bearing line;
        // this is only a total order.
        heights.sort_by(f64::total_cmp);
        wetness.sort_by(f64::total_cmp);

        let mut moisture = [0.0f64; MOISTURE_BELL_Z.len()];
        let quantiles = moisture_quantiles();
        let mut index = 0;
        while index < quantiles.len() {
            moisture[index] = order_statistic(&wetness, quantiles[index]);
            index += 1;
        }
        let mut landform = [0.0f64; LANDFORM_QUANTILES.len()];
        let mut index = 0;
        while index < LANDFORM_QUANTILES.len() {
            landform[index] = order_statistic(&heights, LANDFORM_QUANTILES[index]);
            index += 1;
        }

        Self {
            moisture,
            landform,
            land_samples,
        }
    }

    /// The four moisture edges, ascending.
    pub fn moisture(&self) -> &[f64] {
        &self.moisture
    }

    /// The two landform edges, ascending, in metres.
    pub fn landform(&self) -> &[f64] {
        &self.landform
    }

    /// How many of `BAND_CALIBRATION_SAMPLES` were land. **Zero means the edges are NaN**,
    /// and it is reported rather than inferred so a caller can tell an all-ocean world from
    /// one whose samples could not be answered.
    pub fn land_samples(&self) -> usize {
        self.land_samples
    }
}

/// Which band `value` falls in, given ascending `edges`. `edges.len() + 1` outcomes.
///
/// Returns `None` if `value` or any edge is NaN. **That is the whole reason this returns an
/// `Option`**: a band index is a small non-negative integer with no way to spell "I could not
/// tell", so an unanswerable moisture would otherwise land in band 0 and read as the driest
/// ground on the planet -- the fifth appearance in this project of a NaN producing a
/// plausible answer rather than an error. `BandEdges::calibrate` returns NaN edges for a
/// world it could not calibrate, and this is what stops those being read as a real banding.
///
/// The comparison is `value >= edge`, so a value sitting exactly on an edge takes the
/// **upper** band, matching `viewer/public/app/biome.js::bandIndex`.
pub fn band_index(edges: &[f64], value: f64) -> Option<usize> {
    if value.is_nan() {
        return None;
    }
    let mut band = 0;
    for edge in edges {
        if edge.is_nan() {
            return None;
        }
        if value >= *edge {
            band += 1;
        }
    }
    Some(band)
}

/// Which temperature band a temperature in degrees C falls in. Absolute, not calibrated.
pub fn temperature_band(temperature_c: f64) -> Option<usize> {
    band_index(&TEMP_BAND_EDGES_C, temperature_c)
}

/// Which moisture band a moisture index falls in, against this world's own edges.
pub fn moisture_band(moisture_index: f64, edges: &BandEdges) -> Option<usize> {
    band_index(edges.moisture(), moisture_index)
}

/// Which landform band an elevation in metres falls in, against this world's own edges.
pub fn landform_band(elevation_m: f64, edges: &BandEdges) -> Option<usize> {
    band_index(edges.landform(), elevation_m)
}

/// **Three axes at a point.** Landform x temperature x moisture, as band indices.
///
/// # Why three and not two
///
/// WorldEngine's classifier is `temperature x humidity`, and its manual says so outright: it
/// implements the two axes of Holdridge it can compute. That is adequate for a picture and it
/// is a regression for a game. **A two-axis model has no landform axis and therefore no
/// coastal band at all**, so a wet tropical lowland gets the same answer whether or not it is
/// on a shore -- and for a MUD, "tropical coastal" and "boreal coastal" are different places
/// to stand and want different room descriptions. WorldEngine handles high ground as a
/// draw-time colour modifier rather than as a classification input, which cannot be read back
/// as metadata at all.
///
/// The cost is stated rather than hidden: 3 x 5 x 5 = 75 cells against WorldEngine's 56,
/// before any collapsing. `biome.js` collapses them to 33 colours today, and this returns the
/// bands rather than a colour precisely so the palette stays where the measured work already
/// is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bands {
    /// 0 lowland, 1 interior, 2 montane. See `LANDFORM_QUANTILES` for why it is not
    /// "coastal".
    pub landform: usize,
    /// 0 polar .. 4 tropical, by `TEMP_BAND_EDGES_C`.
    pub temperature: usize,
    /// 0 arid .. 4 perhumid, by this world's `MOISTURE_BELL_Z` quantiles.
    pub moisture: usize,
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
        // Task 5 promoted this closure into `climate::freezing_elevation_m`, so this test
        // now measures the shipped function rather than a copy of its algebra that could
        // drift from it. The numbers below are unchanged from Task 1's run.
        let freeze_m = |latitude: f64| freezing_elevation_m(latitude, &params);
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

    /// **The round trip is the assertion, and it is the whole reason this function inverts
    /// `temperature_c` instead of restating the profile.** Asserting the algebra would only
    /// say the copy matches the copy. Asserting that the temperature AT the returned
    /// elevation is zero says the two agree, so a change to the profile that this function
    /// failed to follow is red here rather than discovered in a picture.
    #[test]
    fn the_snow_line_is_where_the_temperature_says_it_is() {
        let params = ClimateParams::canonical();
        for latitude in [0.0, 12.5, 20.0, 45.0, 55.0, 61.0, -33.0, -45.0] {
            let line = freezing_elevation_m(latitude, &params);
            assert!(line > 0.0, "at {latitude} deg the line should be above the datum, was {line}");
            let at_the_line = temperature_c(latitude, line, &params);
            assert!(
                at_the_line.abs() < 1.0e-9,
                "at {latitude} deg the line is {line} m and the temperature there is {at_the_line} C",
            );
        }
        // **Past the crossing the round trip DOES NOT hold, and that is the datum floor
        // doing its job rather than a failure.** `temperature_c` refuses to warm anything
        // below the datum, so feeding it a negative line returns the sea-level temperature
        // -- which is itself already below freezing, which is exactly what a negative line
        // means. Pinned so a future removal of the floor is red in two places.
        for latitude in [70.0, 89.0] {
            let line = freezing_elevation_m(latitude, &params);
            assert!(line < 0.0, "at {latitude} deg the sea surface should be frozen");
            let at_the_line = temperature_c(latitude, line, &params);
            assert_eq!(at_the_line.to_bits(), temperature_c(latitude, 0.0, &params).to_bits());
            assert!(at_the_line < 0.0, "at {latitude} deg even the datum is above freezing");
        }
    }

    /// **The shape claim, made checkable.** The band this replaces in `relief.js` was a
    /// straight line in latitude; this is a cosine, and the two disagree by far more than
    /// their endpoints do. A straight line drawn through THIS curve's own two ends -- 4,154 m
    /// at the equator and the datum at its crossing -- sits 708 m below it at 45 degrees, so
    /// "linear where the real thing is a cosine" is a metre count rather than a description.
    #[test]
    fn the_snow_line_falls_as_a_cosine_and_not_as_a_line() {
        let params = ClimateParams::canonical();
        let at = |latitude: f64| freezing_elevation_m(latitude, &params);

        // The crossing, bisected rather than asserted, so the 61.26 in the docstrings is
        // this run's number and not a transcription.
        let (mut lo, mut hi) = (61.0, 62.0);
        for _ in 0..40 {
            let mid = 0.5 * (lo + hi);
            if at(mid) > 0.0 {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        assert!(
            (lo - 61.26).abs() < 0.01,
            "the freezing contour reaches the datum at {lo} deg, not 61.26",
        );

        let linear_through_the_same_ends = at(0.0) * (1.0 - 45.0 / lo);
        let gap = at(45.0) - linear_through_the_same_ends;
        assert!(
            gap > 700.0 && gap < 720.0,
            "a line through the same two ends is {gap:.1} m below the cosine at 45 deg",
        );
    }

    /// The unanswerable entrants stay unanswerable, and a world where height does not cool
    /// says so loudly instead of returning a plausible altitude.
    #[test]
    fn a_snow_line_nobody_can_answer_is_not_a_number() {
        let mut params = ClimateParams::canonical();
        assert!(freezing_elevation_m(f64::NAN, &params).is_nan(), "a NaN latitude");

        // Height does not cool: there is no elevation at which this world freezes.
        params.lapse_c_per_km = 0.0;
        assert_eq!(freezing_elevation_m(0.0, &params), f64::INFINITY, "a warm zero-lapse world");
        assert_eq!(freezing_elevation_m(89.0, &params), f64::NEG_INFINITY, "a cold zero-lapse world");
        // And where the sea-level temperature is ITSELF exactly zero, `0/0`: a world that is
        // everywhere at freezing and where height does not cool has no snow line anywhere,
        // and the answer is NaN rather than a plausible zero.
        params.equator_c = 0.0;
        params.pole_c = 0.0;
        assert_eq!(temperature_c(10.0, 0.0, &params).to_bits(), 0.0_f64.to_bits());
        assert!(freezing_elevation_m(10.0, &params).is_nan(), "a world already at freezing");
    }

    /// The parameters are READ. A bit-identity test between `None` and `canonical()` cannot
    /// say this, which is the `CoastParams` lesson and the reason `temperature_c` has the
    /// same test.
    #[test]
    fn every_parameter_moves_the_snow_line() {
        let canonical = ClimateParams::canonical();
        let probe = |params: &ClimateParams| freezing_elevation_m(35.0, params);
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

    // =======================================================================================
    // MOISTURE -- Task 2. Everything below marches against a SYNTHETIC ground, so the
    // arithmetic is judged on terrain the test states rather than on terrain a world
    // happens to have. `surface.rs` carries the tests that need a real world.
    // =======================================================================================

    use crate::sphere::EARTH_RADIUS_M;

    /// The equator's band is easterly, so upwind is +x on the local chart and a test can
    /// write its terrain as "height at this many metres upwind" and read left to right.
    const EASTERLY_LAT: f64 = 0.0;
    /// A westerly band: upwind is -x, so the *same* terrain function shadows nothing.
    const WESTERLY_LAT: f64 = 45.0;

    fn frame_at(latitude_deg: f64) -> TangentFrame {
        TangentFrame::at_latlon(latitude_deg, 0.0, EARTH_RADIUS_M)
    }

    /// Turns a "height at this local x" function into the sampler `moisture_index` wants,
    /// by asking the frame where the probe fell. The round trip is exact to about a
    /// micrometre (`tangent.rs::the_round_trip_returns_where_it_started`), which is eleven
    /// orders below the 20 km step.
    fn ground(frame: &TangentFrame, height_of_x: impl Fn(f64) -> f64 + 'static)
        -> impl Fn(&SpherePoint) -> f64
    {
        let frame = *frame;
        move |point: &SpherePoint| {
            let (x_m, _y_m) = frame.sphere_to_local(point);
            height_of_x(x_m)
        }
    }

    /// A coast with a range on it: open water beyond `sea_from_m` upwind, a ridge of
    /// `crest_m` between `ridge_from_m` and `sea_from_m`, and low land in from there.
    fn coast_with_a_ridge(sea_from_m: f64, ridge_from_m: f64, crest_m: f64)
        -> impl Fn(f64) -> f64 + Copy
    {
        move |x_m: f64| {
            if x_m >= sea_from_m {
                -3_000.0
            } else if x_m >= ridge_from_m {
                crest_m
            } else {
                1.0
            }
        }
    }

    /// A far continent, then a sea, then a coastal range, then low land to the query point.
    ///
    /// **This is the terrain the parameter tests need and the simple coast is not.** On a
    /// coast whose water runs all the way to the far end of the march, the air starts
    /// saturated and stays saturated until landfall, so `recharge_scale_m` never has a gap
    /// to close and a shorter budget that still reaches the same water gives the same
    /// answer -- both parameters read as unread. Two of the tests below were written against
    /// that terrain, failed, and are the reason this fixture exists: **a fixture that cannot
    /// exercise a parameter makes the parameter look dead.**
    fn a_continent_a_sea_and_a_range() -> impl Fn(f64) -> f64 + Copy {
        |x_m: f64| {
            if x_m >= 1_000_000.0 {
                1.0
            } else if x_m >= 300_000.0 {
                -3_000.0
            } else if x_m >= 200_000.0 {
                3_000.0
            } else {
                1.0
            }
        }
    }

    /// The three cells, at their edges and inside them, in both hemispheres.
    ///
    /// The edges are half-open the way the constants read: 30 degrees is already the
    /// westerlies and 60 is already the polar easterlies. Pinning the *boundary* samples is
    /// the point -- an off-by-one in a band table is invisible at the band centres.
    #[test]
    fn the_wind_bands_are_earths_three_cells() {
        for sign in [1.0f64, -1.0] {
            assert_eq!(upwind_east(sign * 0.0), 1.0, "the equator is in the trades");
            assert_eq!(upwind_east(sign * 29.999), 1.0, "just inside the trades");
            assert_eq!(upwind_east(sign * 30.0), -1.0, "30 deg is already westerly");
            assert_eq!(upwind_east(sign * 45.0), -1.0, "mid-latitudes are westerly");
            assert_eq!(upwind_east(sign * 59.999), -1.0, "just inside the westerlies");
            assert_eq!(upwind_east(sign * 60.0), 1.0, "60 deg is already polar easterly");
            assert_eq!(upwind_east(sign * 90.0), 1.0, "the pole is polar easterly");
        }
        // Even in latitude, so the two hemispheres get mirror-image winds rather than the
        // same one -- which is what `abs` buys and what its removal would cost.
        assert_eq!(upwind_east(-45.0).to_bits(), upwind_east(45.0).to_bits());
    }

    /// THE POINT OF THE WHOLE TASK. A range between the query point and the sea leaves the
    /// lee dry, and removing the range leaves it wet, on identical terrain otherwise.
    #[test]
    fn a_ridge_casts_a_rain_shadow_and_flat_ground_does_not() {
        let frame = frame_at(EASTERLY_LAT);
        let params = MoistureParams::canonical();
        let shadowed = moisture_index(EASTERLY_LAT, &frame, &params,
            &ground(&frame, coast_with_a_ridge(600_000.0, 500_000.0, 3_000.0)));
        let open = moisture_index(EASTERLY_LAT, &frame, &params,
            &ground(&frame, coast_with_a_ridge(600_000.0, 500_000.0, 1.0)));
        assert!(shadowed < 0.10, "the lee of a 3,000 m range read {shadowed}");
        assert!(open > 0.60, "the same coast without the range read {open}");
        // And the gap is a factor, not a rounding difference: the whole claim of the slice
        // is that a mountain makes a desert, so the ratio is pinned rather than the order.
        assert!(open / shadowed > 5.0, "the range only cost a factor of {}", open / shadowed);
    }

    /// The band steers the march, so the SAME terrain shadows the equator and not the
    /// mid-latitudes. Without this, a march that ignored `upwind_east` and always walked
    /// `+x` would pass every other test in this file.
    #[test]
    fn the_wind_band_decides_which_side_of_a_range_is_dry() {
        let params = MoistureParams::canonical();
        let terrain = coast_with_a_ridge(600_000.0, 500_000.0, 3_000.0);
        let easterly = frame_at(EASTERLY_LAT);
        let lee = moisture_index(EASTERLY_LAT, &easterly, &params, &ground(&easterly, terrain));
        // At 45 degrees the air comes from the west, so the march walks -x, where this
        // terrain function is low land for ever -- no range, and no sea either.
        let westerly = frame_at(WESTERLY_LAT);
        let windward = moisture_index(WESTERLY_LAT, &westerly, &params,
            &ground(&westerly, coast_with_a_ridge(600_000.0, 500_000.0, 3_000.0)));
        assert!(lee < 0.10, "the easterly band's lee read {lee}");
        assert!(windward > lee * 3.0, "the westerly band saw the range anyway: {windward}");
    }

    /// Recharge over water erases history, so a range 2,000 km out to sea shadows far less
    /// than the same range at the coast. This is the test that the accumulator runs
    /// **downwind**: a march that summed the lifts without ordering them against the water
    /// in between could not tell these two apart.
    #[test]
    fn a_distant_range_across_water_shadows_less_than_a_near_one() {
        let frame = frame_at(EASTERLY_LAT);
        let params = MoistureParams::canonical();
        // Near: the range is the coast itself.
        let near = moisture_index(EASTERLY_LAT, &frame, &params,
            &ground(&frame, coast_with_a_ridge(200_000.0, 100_000.0, 3_000.0)));
        // Far: an identical range, then 1,900 km of open water, then the query point.
        let far = moisture_index(EASTERLY_LAT, &frame, &params, &ground(&frame, |x_m: f64| {
            if x_m >= 2_100_000.0 {
                -3_000.0
            } else if x_m >= 2_000_000.0 {
                3_000.0
            } else {
                -3_000.0
            }
        }));
        assert!(far > 0.95, "an island range 2,000 km upwind still cost {}", 1.0 - far);
        assert!(near < 0.30, "the same range at the coast read {near}");
    }

    /// The fetch term alone, checked against its own closed form. Flat land all the way out
    /// means no lift and no water, so the march reduces to `exp(-span / fetch_scale)` and
    /// the accumulated product must equal it.
    #[test]
    fn a_flat_continent_dries_at_the_fetch_scale() {
        let frame = frame_at(EASTERLY_LAT);
        let params = MoistureParams::canonical();
        let measured = moisture_index(EASTERLY_LAT, &frame, &params, &ground(&frame, |_x| 1.0));
        let span_m = params.step_m * f64::from(params.budget.samples());
        let expected = m::exp(-span_m / params.fetch_scale_m);
        assert!((measured - expected).abs() < 1e-12, "{measured} against {expected}");
        // 3,200 km of the canonical fetch scale is a fifth of saturation left, which is what
        // makes a continental interior a different place from its coast.
        assert!(measured > 0.19 && measured < 0.21, "a 3,200 km flat interior read {measured}");
    }

    /// Open water is saturated, exactly, not nearly.
    #[test]
    fn open_ocean_stays_saturated_bit_for_bit() {
        let frame = frame_at(EASTERLY_LAT);
        let answer = moisture_index(EASTERLY_LAT, &frame, &MoistureParams::canonical(),
            &ground(&frame, |_x| -4_000.0));
        assert_eq!(answer.to_bits(), 1.0f64.to_bits(), "open ocean read {answer}");
        // The datum itself is water, not land: `elevation_m == 0.0` is the shoreline and
        // must take the recharge arm, not the fetch one.
        let at_datum = moisture_index(EASTERLY_LAT, &frame, &MoistureParams::canonical(),
            &ground(&frame, |_x| 0.0));
        assert_eq!(at_datum.to_bits(), 1.0f64.to_bits(), "the datum read {at_datum}");
    }

    /// The three rain shadows `LIFT_SCALE_M` was fitted to, reproduced as ratios.
    ///
    /// **Population:** Sierra Nevada / Great Basin (~2,500 m crest, lee about 0.20 of
    /// windward), Southern Alps / Canterbury (~2,000 m, ~0.06), Andes / Atacama (~4,000 m,
    /// ~0.01). **Method:** the shed factor `exp(-crest / LIFT_SCALE_M)` this march applies
    /// to a single climb, compared against the observed ratio. **Host:** arithmetic, so
    /// any.
    ///
    /// The three landmarks disagree with each other by a factor of 2.2 -- 1,553 / 712 / 868
    /// metres of implied scale -- so the constant is their geometric mean and the assertion
    /// is a factor band rather than a percentage. Naming the disagreement is the point: a
    /// tighter assertion here would be a tighter fit to three numbers that do not agree.
    #[test]
    fn a_thousand_metres_of_lift_sheds_the_fitted_fraction() {
        let landmarks = [(2_500.0f64, 0.20f64), (2_000.0, 0.06), (4_000.0, 0.01)];
        let mut worst = 1.0f64;
        for (crest_m, observed) in landmarks {
            let modelled = m::exp(-crest_m / LIFT_SCALE_M);
            let factor = if modelled > observed { modelled / observed } else { observed / modelled };
            assert!(factor < 2.5, "a {crest_m} m crest modelled {modelled} against {observed}");
            if factor > worst {
                worst = factor;
            }
        }
        // Pinned so the fit cannot silently loosen, and so the honest number is on record:
        // the worst landmark is out by a factor of 2.4, in a quantity whose three
        // observations imply scales that differ by 2.2 among themselves.
        assert!(worst > 2.3 && worst < 2.5, "worst landmark factor was {worst}");
    }

    /// The march produces an index, and an index that left `[0, 1]` would break every band
    /// Task 3 cuts out of it. Checked on ground designed to break it: a sawtooth of
    /// eight-kilometre peaks alternating with trenches, at a step that lands between them.
    #[test]
    fn moisture_stays_inside_the_unit_interval_on_hostile_ground() {
        let frame = frame_at(EASTERLY_LAT);
        let params = MoistureParams::canonical();
        let answer = moisture_index(EASTERLY_LAT, &frame, &params, &ground(&frame, |x_m: f64| {
            // Alternates every step, so every land sample is a fresh 8,000 m climb.
            let bucket = m::floor(x_m / 20_000.0);
            if (bucket / 2.0) == m::floor(bucket / 2.0) { 8_000.0 } else { -6_000.0 }
        }));
        assert!(answer >= 0.0 && answer <= 1.0, "a sawtooth planet read {answer}");
        // And it is not zero by underflow: it must be a real number in the interval, which
        // is what says the assertion above is discriminating rather than vacuous.
        assert!(answer.is_finite(), "the sawtooth read {answer}");
    }

    /// THE SPIKE'S OWN DEFECT, PINNED. Its report: *"A `NaN` step silently returns full
    /// moisture rather than `NaN`. `lift_m` is `NaN`, `if lift_m > 0.0` is false, and the
    /// accumulator is never touched."*
    ///
    /// Three entrants, because a guard proved on one is a guard proved on the arithmetic
    /// and not on the reach: a NaN in the middle of a land path, a NaN over what would
    /// otherwise be water, and a NaN at the far upwind seed of an all-water path -- which is
    /// the one arm the loop body cannot see, because the recharge arm never reads the
    /// previous height.
    #[test]
    fn a_nan_elevation_is_not_answered_with_marine_air() {
        let frame = frame_at(EASTERLY_LAT);
        let params = MoistureParams::canonical();

        // 1. Mid-path, over land.
        let mid_land = moisture_index(EASTERLY_LAT, &frame, &params, &ground(&frame, |x_m: f64| {
            if x_m > 1_000_000.0 && x_m < 1_100_000.0 { f64::NAN } else { 1.0 }
        }));
        assert!(mid_land.is_nan(), "a NaN over land read {mid_land}");

        // 2. Mid-path, over water -- the branch where a NaN fails the `> 0.0` test and
        // would otherwise be recharged towards saturation.
        let mid_water = moisture_index(EASTERLY_LAT, &frame, &params, &ground(&frame, |x_m: f64| {
            if x_m > 1_000_000.0 && x_m < 1_100_000.0 { f64::NAN } else { -4_000.0 }
        }));
        assert!(mid_water.is_nan(), "a NaN over water read {mid_water}");

        // 3. The seed alone, on an otherwise all-water path. Nothing in the loop body reads
        // it, so only the guard before the loop can catch this.
        let span_m = params.step_m * f64::from(params.budget.samples());
        let seed_only = moisture_index(EASTERLY_LAT, &frame, &params, &ground(&frame, move |x_m: f64| {
            if x_m > span_m - 1_000.0 { f64::NAN } else { -4_000.0 }
        }));
        assert!(seed_only.is_nan(), "a NaN at the far end read {seed_only}");

        // The blindness itself, asserted rather than described: the value the unguarded
        // form returns is 1.0 -- fully saturated marine air, the most ordinary answer this
        // function has. That is what would have made it invisible.
        let plausible = moisture_index(EASTERLY_LAT, &frame, &params,
            &ground(&frame, |_x| -4_000.0));
        assert_eq!(plausible.to_bits(), 1.0f64.to_bits(),
            "the swallowed value is meant to be exactly saturated air");
    }

    /// A march of no length loses nothing, and a march of no steps loses nothing, and both
    /// are exactly saturated rather than approximately so.
    ///
    /// These are the identity elements rather than errors -- a path of zero length cannot
    /// rain -- and they are pinned so that a future edit cannot make an empty answer mean
    /// "the loop did not run".
    #[test]
    fn a_march_of_no_length_and_a_march_of_no_steps_both_stay_saturated() {
        let frame = frame_at(EASTERLY_LAT);
        let dry_land = ground(&frame, |_x| 4_000.0);

        let mut still = MoistureParams::canonical();
        still.step_m = 0.0;
        assert_eq!(moisture_index(EASTERLY_LAT, &frame, &still, &dry_land).to_bits(),
            1.0f64.to_bits());

        let mut none = MoistureParams::canonical();
        none.budget = MarchBudget::new(0).expect("zero is an admissible budget");
        assert_eq!(moisture_index(EASTERLY_LAT, &frame, &none, &dry_land).to_bits(),
            1.0f64.to_bits());
    }

    /// THE LOOP BOUND. `MarchBudget` is the only way to set the march's length and it
    /// cannot be built above the ceiling, so no caller -- Rust or, once Task 4 adds one, C
    /// -- can reach the ~2,600-second hang the spike extrapolated.
    #[test]
    fn the_march_length_cannot_be_built_above_the_ceiling() {
        assert!(MarchBudget::new(MAX_MARCH_SAMPLES).is_some(), "the ceiling itself is admitted");
        assert!(MarchBudget::new(MAX_MARCH_SAMPLES + 1).is_none(), "one past the ceiling");
        assert!(MarchBudget::new(u16::MAX).is_none(), "the widest a u16 can hold");
        assert!(MarchBudget::new(0).is_some(), "zero is the identity, not an error");
        assert_eq!(MarchBudget::canonical().samples(), MARCH_SAMPLES);
        // The ceiling is a real bound and not a formality: it is 6.4x the canonical budget,
        // so it leaves room to experiment and still bounds one call to a fraction of a
        // millisecond natively.
        assert!(u32::from(MAX_MARCH_SAMPLES) > 4 * u32::from(MARCH_SAMPLES));
        assert!(MAX_MARCH_SAMPLES < u16::MAX / 8);
    }

    /// `canonical()` is the constants, and `None` at the `Surface` level is `canonical()`
    /// (pinned over a real world in `surface.rs`).
    #[test]
    fn canonical_moisture_params_match_the_constants() {
        let params = MoistureParams::canonical();
        assert_eq!(params.budget.samples(), MARCH_SAMPLES);
        assert_eq!(params.step_m.to_bits(), MARCH_STEP_M.to_bits());
        assert_eq!(params.lift_scale_m.to_bits(), LIFT_SCALE_M.to_bits());
        assert_eq!(params.fetch_scale_m.to_bits(), FETCH_SCALE_M.to_bits());
        assert_eq!(params.recharge_scale_m.to_bits(), RECHARGE_SCALE_M.to_bits());
        // The step is the coarsest detail octave, which is the whole argument for it. If
        // `detail.rs` ever moves that constant, this says so rather than leaving the
        // docstring quietly wrong.
        assert_eq!(MARCH_STEP_M.to_bits(), crate::detail::COARSEST_WAVELENGTH_M.to_bits());
    }

    /// All five parameters are READ. A bit-identity test between `None` and `canonical()`
    /// passes just as well when the whole block is ignored -- the `CoastParams` lesson, and
    /// the reason this test exists separately from that one.
    #[test]
    fn every_moisture_parameter_moves_the_answer() {
        let frame = frame_at(EASTERLY_LAT);
        let terrain = a_continent_a_sea_and_a_range();
        let canonical = MoistureParams::canonical();
        let base = moisture_index(EASTERLY_LAT, &frame, &canonical, &ground(&frame, terrain));
        for field in 0..5 {
            let mut params = canonical;
            match field {
                0 => params.budget = MarchBudget::new(40).expect("in range"),
                1 => params.step_m = 30_000.0,
                2 => params.lift_scale_m = 2_000.0,
                3 => params.fetch_scale_m = 1_000_000.0,
                _ => params.recharge_scale_m = 30_000.0,
            }
            let moved = moisture_index(EASTERLY_LAT, &frame, &params, &ground(&frame, terrain));
            assert_ne!(moved.to_bits(), base.to_bits(), "field {field} was not read");
        }
    }

    /// THE DOMAIN, AND IT WAS FOUND BY SWEEPING RATHER THAN BY READING.
    ///
    /// A cross-product sweep of 37,818 calls -- budget x step x lift scale, step x fetch x
    /// recharge, and resolution x budget x step, each over a set of probe points including
    /// both band edges and three non-finite vectors -- returned **536 answers outside
    /// `[0, 1]`**: infinities, and numbers of order 1e25. Every one came from a *band* of
    /// admissible-looking arguments and not from a cliff, which is the shape this project
    /// has now found six times and never once by spot-checking.
    ///
    /// Three entrants, each asserted here at the value that produced it:
    ///
    /// - a **negative `step_m`**, which turns `exp(-step / fetch)` into a number greater
    ///   than one, so the fetch term adds moisture at every step;
    /// - a **zero scale**, which divides by zero -- and `0.0` is *non-negative*, so a guard
    ///   written `>= 0.0` would have admitted it. That distinction is the guard;
    /// - a **negative scale**, which flips every exponent and turns rain-out into rain-in.
    ///
    /// And the other half, which is what stops this from being a guard that refuses too
    /// much: an **infinite** scale is admitted, because `exp(-x / inf)` is `1` and means
    /// "this term never fires", which is a coherent request with an in-range answer.
    #[test]
    fn the_march_refuses_the_domain_it_cannot_answer() {
        let frame = frame_at(EASTERLY_LAT);
        let terrain = a_continent_a_sea_and_a_range();
        let probe = |params: &MoistureParams| {
            moisture_index(EASTERLY_LAT, &frame, params, &ground(&frame, terrain))
        };

        // First, that the value being discriminated against is real: the canonical answer
        // on this terrain is an ordinary number strictly inside the interval. Without this
        // line every assertion below could be asserting against nothing.
        let canonical = probe(&MoistureParams::canonical());
        assert!(canonical > 0.0 && canonical < 1.0, "the canonical answer was {canonical}");

        let mut refused = 0usize;
        for (label, params) in [
            ("negative step", MoistureParams { step_m: -20_000.0, ..MoistureParams::canonical() }),
            ("zero lift scale", MoistureParams { lift_scale_m: 0.0, ..MoistureParams::canonical() }),
            ("negative zero lift scale", MoistureParams { lift_scale_m: -0.0, ..MoistureParams::canonical() }),
            ("negative lift scale", MoistureParams { lift_scale_m: -1_000.0, ..MoistureParams::canonical() }),
            ("zero fetch scale", MoistureParams { fetch_scale_m: 0.0, ..MoistureParams::canonical() }),
            ("negative fetch scale", MoistureParams { fetch_scale_m: -2e6, ..MoistureParams::canonical() }),
            ("zero recharge scale", MoistureParams { recharge_scale_m: 0.0, ..MoistureParams::canonical() }),
            ("negative recharge scale", MoistureParams { recharge_scale_m: -3e5, ..MoistureParams::canonical() }),
            // A NaN in any of the four fields is refused by the SAME guard, because every
            // comparison is negated and a NaN fails `>=` and `>` alike. A separate test
            // asserted this through the arithmetic instead and was deleted when the guard
            // landed: with the guard in place no mutation could turn it red, because a NaN
            // parameter reached NaN by two independent routes and breaking either left the
            // other. **An assertion two mechanisms both satisfy is not load-bearing**, and
            // this project has now found fourteen of those.
            ("nan step", MoistureParams { step_m: f64::NAN, ..MoistureParams::canonical() }),
            ("nan lift scale", MoistureParams { lift_scale_m: f64::NAN, ..MoistureParams::canonical() }),
            ("nan fetch scale", MoistureParams { fetch_scale_m: f64::NAN, ..MoistureParams::canonical() }),
            ("nan recharge scale", MoistureParams { recharge_scale_m: f64::NAN, ..MoistureParams::canonical() }),
        ] {
            let answer = probe(&params);
            assert!(answer.is_nan(), "{label} was answered with {answer}");
            refused += 1;
        }
        assert_eq!(refused, 12, "the loop refused {refused} configurations");

        // `-0.0` is a different bit pattern from `0.0` and the two ends of the guard treat
        // it differently ON PURPOSE, which is asserted rather than left to be discovered:
        // `-0.0 >= 0.0` is TRUE, so a negative-zero STEP is admitted and is the identity
        // march; `-0.0 > 0.0` is FALSE, so a negative-zero SCALE is refused with the other
        // zero. This test was written asserting the opposite for the step and went red
        // saying so, which is the line that earned this paragraph.
        let negative_zero_step = probe(&MoistureParams { step_m: -0.0, ..MoistureParams::canonical() });
        assert_eq!(negative_zero_step.to_bits(), 1.0f64.to_bits(),
            "a negative-zero step is a march of no length, not an error");

        // And the admitted half. An infinite scale means "this term never fires", which is
        // answerable and in range -- a guard that refused it would be refusing a question
        // rather than an error.
        for params in [
            MoistureParams { lift_scale_m: f64::INFINITY, ..MoistureParams::canonical() },
            MoistureParams { fetch_scale_m: f64::INFINITY, ..MoistureParams::canonical() },
            MoistureParams { recharge_scale_m: f64::INFINITY, ..MoistureParams::canonical() },
        ] {
            let answer = probe(&params);
            assert!(answer >= 0.0 && answer <= 1.0, "an infinite scale was answered with {answer}");
        }
        // An infinite fetch scale means no continental drying at all, so the same terrain
        // must come back WETTER than canonical -- which says the admitted arm is reaching
        // the arithmetic rather than merely not panicking.
        let no_drying = probe(&MoistureParams { fetch_scale_m: f64::INFINITY, ..MoistureParams::canonical() });
        assert!(no_drying > canonical, "an infinite fetch scale read {no_drying} against {canonical}");
    }

    /// `RECHARGE_SCALE_M`'s docstring claims the answer barely depends on it. That is a
    /// claim about this model and it is measured here rather than asserted, because an
    /// insensitive parameter is one nobody should spend effort fitting -- and saying so
    /// without a number is how a guess becomes a constant.
    ///
    /// **Population:** the coast-with-a-range terrain above, at the canonical march.
    /// **Method:** the recharge scale moved over a factor of ten, 100 km to 1,000 km,
    /// against the 300 km constant. **Host:** arithmetic.
    #[test]
    fn the_recharge_scale_is_not_a_sensitive_parameter() {
        let frame = frame_at(EASTERLY_LAT);
        let terrain = a_continent_a_sea_and_a_range();
        let base = moisture_index(EASTERLY_LAT, &frame, &MoistureParams::canonical(),
            &ground(&frame, terrain));
        let mut worst = 0.0f64;
        for scale_m in [100_000.0f64, 1_000_000.0] {
            let mut params = MoistureParams::canonical();
            params.recharge_scale_m = scale_m;
            let moved = moisture_index(EASTERLY_LAT, &frame, &params, &ground(&frame, terrain));
            let shift = (moved - base).abs();
            if shift > worst {
                worst = shift;
            }
        }
        // MEASURED: a factor of ten in this constant moves the index by 0.0113 on the
        // terrain built to expose it -- about a **twentieth of a band**, against the roughly
        // 0.2-wide bands Task 3 will cut. The lift scale, for contrast, moves the same
        // terrain by more than a whole band for a factor of two, which is why that one is
        // fitted to three landmarks and this one to two. The band is 0.015 rather than
        // 0.0113 so the pin is a ceiling and not a transcription of one run.
        assert!(worst < 0.015, "a factor of ten in the recharge scale moved it {worst}");
        assert!(worst > 0.0, "the recharge scale did not reach the answer at all");
    }

    // ----------------------------------------------------------------------------------------
    // Task 3: bands
    // ----------------------------------------------------------------------------------------

    /// The textbook values of the standard normal CDF at the four shipped z-scores.
    ///
    /// The point of `normal_cdf` is that the bell spacing is DERIVED rather than transcribed,
    /// which is only worth anything if the derivation is right. A&S 26.2.17 claims 7.5e-8;
    /// this asserts 1e-7 against values read from the normal table, and asserts the symmetry
    /// `F(-z) = 1 - F(z)` separately because the function's two arms are written separately.
    #[test]
    fn the_bell_quantiles_are_the_normal_cdf_of_the_z_scores() {
        let expected = [0.066_807_2, 0.308_537_5, 0.691_462_5, 0.933_192_8];
        let got = moisture_quantiles();
        for (index, (a, b)) in got.iter().zip(expected.iter()).enumerate() {
            assert!(
                (a - b).abs() < 1e-7,
                "quantile {index}: {a} against the tabulated {b}"
            );
        }
        assert!((normal_cdf(0.0) - 0.5).abs() < 1e-7, "F(0) = {}", normal_cdf(0.0));
        for z in [-2.5, -1.5, -0.5, 0.5, 1.5, 2.5] {
            let sum = normal_cdf(z) + normal_cdf(-z);
            assert!((sum - 1.0).abs() < 1e-7, "F({z}) + F(-{z}) = {sum}");
        }
    }

    /// **The spacing is a bell and not a ruler, and the ratio is the assertion.**
    ///
    /// Evenly spaced quantiles would make all five band widths 20% and every ratio 1. The
    /// shipped spacing makes them 6.7 / 24.2 / 38.3 / 24.2 / 6.7, so the middle band is 5.7
    /// times the tails. This pins the tails below 10%, the middle above 30%, and the widths
    /// symmetric -- three statements a ruler fails and a bell passes.
    #[test]
    fn the_moisture_bands_are_bell_spaced_and_not_evenly_spaced() {
        let quantiles = moisture_quantiles();
        let mut widths = [0.0f64; MOISTURE_BANDS];
        let mut previous = 0.0;
        for (index, q) in quantiles.iter().enumerate() {
            assert!(*q > previous, "quantiles must ascend: {q} after {previous}");
            widths[index] = q - previous;
            previous = *q;
        }
        widths[MOISTURE_BANDS - 1] = 1.0 - previous;

        assert!(widths[0] < 0.10, "the arid band is {} of land", widths[0]);
        assert!(
            widths[MOISTURE_BANDS - 1] < 0.10,
            "the perhumid band is {} of land",
            widths[MOISTURE_BANDS - 1]
        );
        assert!(widths[2] > 0.30, "the middle band is {} of land", widths[2]);
        assert!(
            widths[2] / widths[0] > 4.0,
            "middle/tail ratio {} -- evenly spaced bands give 1",
            widths[2] / widths[0]
        );
        assert!(
            (widths[0] - widths[MOISTURE_BANDS - 1]).abs() < 1e-9
                && (widths[1] - widths[MOISTURE_BANDS - 2]).abs() < 1e-9,
            "the bell must be symmetric: {widths:?}"
        );
    }

    /// The calibration population is `continentality`'s, by bits, not by resemblance.
    ///
    /// The doc comment claims this is the same grid-free order statistic that layer already
    /// uses. If `continentality` ever moves its sample count, this says so rather than
    /// leaving the claim quietly false -- the same shape as
    /// `canonical_moisture_params_match_the_constants` for the march step.
    #[test]
    fn the_band_calibration_uses_continentalitys_own_population() {
        assert_eq!(
            BAND_CALIBRATION_SAMPLES,
            crate::continentality::CALIBRATION_SAMPLES,
            "the band edges claim to use continentality's own spiral"
        );
    }

    /// A field whose value is a strictly increasing function of the spiral's own `z`, so the
    /// sample is uniform by construction and every edge has a known place.
    fn ramp(point: &SpherePoint) -> f64 {
        1000.0 * (point.vector.z + 1.0) + 1.0
    }

    /// **The edges are quantiles of the sample, and this counts rather than trusts.**
    ///
    /// For each edge, the fraction of land samples strictly below it must be the quantile it
    /// was asked for, to within one sample. A calibration that returned the mean, the median
    /// of everything, or the quantiles in the wrong order fails this.
    #[test]
    fn the_edges_are_the_quantiles_they_were_asked_for() {
        let edges = BandEdges::calibrate(&ramp, &|p| 2.0 * ramp(p));
        assert_eq!(
            edges.land_samples(),
            BAND_CALIBRATION_SAMPLES,
            "the ramp is positive everywhere, so every sample is land"
        );

        // Rebuild the sample the same way `calibrate` does, so the fractions below are read
        // off the same population and not a similar one.
        let golden = core::f64::consts::PI * (3.0 - m::sqrt(5.0));
        let n = BAND_CALIBRATION_SAMPLES;
        let mut heights = Vec::with_capacity(n);
        for index in 0..n {
            let z = 1.0 - 2.0 * (index as f64 + 0.5) / (n as f64); // cast-ok: loop counter to float
            let inner = 1.0 - z * z;
            let ring = m::sqrt(if inner > 0.0 { inner } else { 0.0 });
            let angle = golden * index as f64; // cast-ok: loop counter to float
            heights.push(ramp(&SpherePoint {
                vector: crate::vectors::Vec3::new(m::cos(angle) * ring, m::sin(angle) * ring, z),
            }));
        }
        let tolerance = 2.0 / n as f64; // cast-ok: count to float

        for (edge, q) in edges.moisture().iter().zip(moisture_quantiles().iter()) {
            let below = heights.iter().filter(|h| 2.0 * **h < *edge).count();
            let fraction = below as f64 / n as f64; // cast-ok: counts to float
            assert!(
                (fraction - q).abs() < tolerance,
                "moisture edge {edge} sits at quantile {fraction}, asked for {q}"
            );
        }
        for (edge, q) in edges.landform().iter().zip(LANDFORM_QUANTILES.iter()) {
            let below = heights.iter().filter(|h| **h < *edge).count();
            let fraction = below as f64 / n as f64; // cast-ok: counts to float
            assert!(
                (fraction - q).abs() < tolerance,
                "landform edge {edge} sits at quantile {fraction}, asked for {q}"
            );
        }
    }

    /// A value sitting exactly on an edge takes the upper band, and the count of outcomes is
    /// `edges + 1`. `viewer/public/app/biome.js::bandIndex` does the same, and a classifier
    /// whose two ends disagree about the boundary draws a one-texel seam along every edge.
    #[test]
    fn a_value_on_an_edge_takes_the_upper_band() {
        let edges = [1.0, 2.0, 3.0];
        assert_eq!(band_index(&edges, 0.5), Some(0));
        assert_eq!(band_index(&edges, 1.0), Some(1), "exactly on the first edge");
        assert_eq!(band_index(&edges, 1.5), Some(1));
        assert_eq!(band_index(&edges, 3.0), Some(3), "exactly on the last edge");
        assert_eq!(band_index(&edges, 1e300), Some(3));
        assert_eq!(band_index(&edges, f64::INFINITY), Some(3));
        assert_eq!(band_index(&edges, f64::NEG_INFINITY), Some(0));
        assert_eq!(band_index(&[], 7.0), Some(0), "no edges is one band");
    }

    /// **The datum is not land, and the calibration's land test says so.**
    ///
    /// `> 0.0` and `>= 0.0` differ on exactly one value, and on a real world that value has
    /// measure zero -- so the distinction is invisible to every test that uses real terrain,
    /// which is how it stayed green under mutation until this fixture existed. It matters
    /// anyway: the sea surface is AT the datum, so admitting it would put open water in the
    /// land distribution both quantiled axes are taken over, and the landform axis's bottom
    /// band would then be calibrated partly on ocean.
    ///
    /// Half this spiral is at exactly `0.0` and half is above it, so the land count is the
    /// assertion.
    #[test]
    fn the_datum_is_not_land_to_the_calibration() {
        let half = BandEdges::calibrate(
            &|p| if p.vector.z > 0.0 { 100.0 } else { 0.0 },
            &|_| 0.5,
        );
        assert_eq!(
            half.land_samples(),
            BAND_CALIBRATION_SAMPLES / 2,
            "the datum itself must not count as land"
        );
    }

    /// **An unanswerable value must not read as the driest ground on the planet.**
    ///
    /// A band index has no way to spell "I could not tell", so without the `Option` a NaN
    /// moisture takes band 0 -- the fifth appearance in this project of a NaN producing a
    /// plausible answer. It is asserted on all three axes because all three can see one.
    #[test]
    fn an_unanswerable_value_is_not_banded_as_the_driest_ground() {
        let edges = BandEdges::calibrate(&ramp, &|p| 2.0 * ramp(p));
        assert_eq!(band_index(&[1.0, 2.0], f64::NAN), None);
        assert_eq!(temperature_band(f64::NAN), None);
        assert_eq!(moisture_band(f64::NAN, &edges), None);
        assert_eq!(landform_band(f64::NAN, &edges), None);
        // And the answerable neighbours still answer, so the guard is a door and not a wall.
        assert_eq!(temperature_band(-40.0), Some(0));
        assert_eq!(temperature_band(30.0), Some(TEMPERATURE_BANDS - 1));
        assert!(moisture_band(0.0, &edges).is_some());
    }

    /// **A world with no land must not report the edges of a world that has some.**
    ///
    /// The shape of the NaN land fraction that produced a world bit-identical to a legitimate
    /// all-land one: a calibration with nothing to calibrate on has to say so, and the way it
    /// says so is edges that refuse to band.
    #[test]
    fn a_world_that_cannot_be_calibrated_does_not_band() {
        let ocean = BandEdges::calibrate(&|_| -1.0, &|_| 0.5);
        assert_eq!(ocean.land_samples(), 0);
        assert!(
            ocean.moisture().iter().all(|e| e.is_nan())
                && ocean.landform().iter().all(|e| e.is_nan()),
            "an all-ocean world has no land distribution: {:?} {:?}",
            ocean.moisture(),
            ocean.landform()
        );
        assert_eq!(moisture_band(0.5, &ocean), None, "NaN edges must not band");
        assert_eq!(landform_band(100.0, &ocean), None);
    }

    /// One unanswerable sample poisons the calibration, on either sampler.
    ///
    /// **A quantile taken over the samples that happened to answer is a quantile of a
    /// different population**, silently. The march can answer NaN -- that is its stated
    /// contract for a non-finite point or an out-of-domain parameter -- so this is reachable
    /// rather than hypothetical, and dropping those samples would be the same swallow one
    /// level up.
    #[test]
    fn one_unanswerable_sample_poisons_the_calibration() {
        // The spiral's first point sits at z = 1 - 1/n, so exactly one sample is above 0.999.
        let elevation_nan = BandEdges::calibrate(
            &|p| if p.vector.z > 0.999 { f64::NAN } else { ramp(p) },
            &|p| 2.0 * ramp(p),
        );
        assert!(
            elevation_nan.moisture().iter().all(|e| e.is_nan()),
            "a NaN elevation must not be quietly skipped: {:?}",
            elevation_nan.moisture()
        );
        let moisture_nan = BandEdges::calibrate(&ramp, &|p| {
            if p.vector.z > 0.999 {
                f64::NAN
            } else {
                2.0 * ramp(p)
            }
        });
        assert!(
            moisture_nan.moisture().iter().all(|e| e.is_nan())
                && moisture_nan.landform().iter().all(|e| e.is_nan()),
            "a NaN moisture must not be quietly skipped: {:?}",
            moisture_nan.moisture()
        );
        assert!(
            moisture_nan.land_samples() > 0,
            "and the land count still reports what was seen, so a caller can tell the two \
             refusals apart"
        );
    }

    /// **The acceptance bar of this task, asserted rather than described: every band on all
    /// three axes is reached on two worlds.**
    ///
    /// Population: 1,200 Fibonacci-spiral points per world, the same construction the
    /// calibration uses, over the two worlds `src/bin/climate_survey.rs` calls `default` and
    /// `seed 424242`; land is `elevation_m > 0.0`. The edges come from
    /// `Surface::band_edges(None, None)`, so this is the shipped calibration and not a
    /// re-implementation of it -- and the occupancy population is deliberately NOT the
    /// calibration population, because an edge that is only occupied on the points it was
    /// fitted to is not an edge.
    ///
    /// **Two of the three axes are occupied by construction and the third is not**, which is
    /// exactly why this asserts all three: moisture and landform are quantiles of the world's
    /// own land and can only fail by TYING, and temperature is absolute and can fail by a
    /// world simply not having that climate. The release survey measures the same property
    /// over 20,000 points on four worlds; this is the part CI can afford.
    #[test]
    fn every_band_on_all_three_axes_is_reached_on_two_worlds() {
        const POPULATION: usize = 1_200;
        let golden = core::f64::consts::PI * (3.0 - m::sqrt(5.0));
        for (label, seed, plates, land_fraction) in
            [("default", 20_260_904i64, 12usize, 0.29f64), ("seed 424242", 424_242, 18, 0.40)]
        {
            let surface = crate::surface::Surface::new(
                seed,
                crate::sphere::EARTH_RADIUS_M,
                plates,
                land_fraction,
                None,
                None,
                None,
            );
            let edges = surface.band_edges(None, None);
            assert!(
                edges.land_samples() > 0,
                "{label}: nothing to calibrate on"
            );

            let mut landform = [0usize; LANDFORM_BANDS];
            let mut temperature = [0usize; TEMPERATURE_BANDS];
            let mut moisture = [0usize; MOISTURE_BANDS];
            let mut land = 0usize;
            for index in 0..POPULATION {
                let z = 1.0 - 2.0 * (index as f64 + 0.5) / (POPULATION as f64); // cast-ok: loop counter to float
                let inner = 1.0 - z * z;
                let ring = m::sqrt(if inner > 0.0 { inner } else { 0.0 });
                let angle = golden * index as f64; // cast-ok: loop counter to float
                let point = SpherePoint {
                    vector: crate::vectors::Vec3::new(
                        m::cos(angle) * ring,
                        m::sin(angle) * ring,
                        z,
                    ),
                };
                if !(surface.elevation_m(&point, None) > 0.0) {
                    continue;
                }
                land += 1;
                let bands = surface
                    .bands_at(&point, None, None, None, &edges)
                    .expect("real land on a calibrated world is answerable");
                landform[bands.landform] += 1;
                temperature[bands.temperature] += 1;
                moisture[bands.moisture] += 1;
            }
            assert!(land > 200, "{label}: only {land} land points to band");
            for (axis, counts) in [
                ("landform", &landform[..]),
                ("temperature", &temperature[..]),
                ("moisture", &moisture[..]),
            ] {
                for (band, count) in counts.iter().enumerate() {
                    assert!(
                        *count > 0,
                        "{label}: {axis} band {band} is UNREACHED over {land} land points \
                         ({counts:?}) -- a band nobody can enter is a dead palette entry"
                    );
                }
            }
        }
    }
}
