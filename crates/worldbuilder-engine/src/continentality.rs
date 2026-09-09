//! The broad shape of land and sea on a world.
//!
//! Ported from `worldbuilder/terrain/continentality.py`.
//!
//! This takes a seed and nothing else. It cannot consult the plates because it has no way
//! to reach them, which is the point — an architectural claim enforced by the import list
//! rather than by a comment asking people to behave. Do not add a `plates` import here.

use crate::detmath as m;
use crate::noise::Noise;
use crate::sphere::SpherePoint;

/// Cycles per unit of noise space at the first octave. About one and a quarter, which puts
/// the largest features somewhere near five thousand kilometres across — a continent, or
/// an ocean basin, and nothing smaller.
pub const BASE_FREQUENCY: f64 = 1.25;

/// How many octaves. Few, deliberately. Enough to stop the landmasses being simple blobs,
/// not enough to start carving a coast.
pub const OCTAVES: u32 = 4;

/// How high a continental interior stands, and how deep an ocean basin lies, in metres
/// before anything else has its say.
pub const CONTINENT_M: f64 = 700.0;
pub const ABYSS_M: f64 = -4600.0;

/// How much of the surface is dry, unless a world asks otherwise. Earth is about 29 per
/// cent, and it is the single most powerful thing a developer can turn.
pub const LAND_FRACTION: f64 = 0.29;

/// How many points to sample when working out where sea level falls.
pub const CALIBRATION_SAMPLES: usize = 4000;

/// How far apart the probes are when measuring which way the land rises, in metres.
pub const GRADIENT_STEP_M: f64 = 20000.0;

/// Salted so this field is independent of any other on the same world.
pub const NOISE_SALT: u64 = 0x0C0FFEE;

/// Salt for the OPT-IN coastal roughening term below, so it is independent of the field it
/// perturbs. Distinct from `NOISE_SALT` (`0x0C0FFEE`) and from `detail.rs`'s `0x5EABED` --
/// a shared salt would correlate the coast's wobble with the field deciding where the coast
/// is, which is the one correlation this term must not have.
///
/// **Nothing on the canonical path reaches it.** `CoastParams::canonical()` sets `amplitude`
/// to exactly 0.0 and `above_shore` branches on that before sampling, so a canonical world
/// never draws from this lattice at all.
pub const COAST_NOISE_SALT: u64 = 0x0C0A575;

/// The six values that decide how fractal a coastline is, broken out so a caller who wants
/// a rougher coast can ask for one without touching what "canonical" means.
///
/// `Continentality::with_coast` takes `Option<CoastParams>`, following `ReliefParams` on
/// `Detail::new` and `TectonicParams` on `Tectonics::new`: `None` is the canonical path, not
/// an implicit `Default::default()` -- this codebase deliberately rejects defaults nobody
/// chose (see `stream.rs::BuildParams`).
///
/// # Why a separate term rather than a fifth octave
///
/// The obvious way to roughen the land/sea field is to raise `OCTAVES`. Measured, it does
/// not work, for two independent reasons, both recorded in
/// `.superpowers/sdd/notes/research-worldgen.md` §5.3:
///
/// - **`noise.rs::fbm` normalises by the sum of amplitudes.** At `gain = 0.5` a fifth octave
///   carries `0.5^4 / (1 + 0.5 + 0.25 + 0.125 + 0.0625) = 3.23%` of total amplitude and
///   scales the four existing octaves by `30/31`. A coastline term worth 3.23% of the field
///   is a sub-pixel wobble, not a fjord -- while every conformance digest moves.
/// - **`CALIBRATION_SAMPLES = 4000` is already below Nyquist for octave four.** 4,000
///   area-uniform samples over the sphere is a spacing of `sqrt(4*PI/4000)` rad, about 357 km
///   at Earth's radius, against a finest-octave wavelength near 637 km: ~1.8 samples per
///   wavelength. A fifth octave takes that to ~0.9 and degrades the land-fraction order
///   statistic from quasi-Monte-Carlo to plain Monte-Carlo (+-0.72 pp at `land = 0.29`,
///   +-0.58 pp at `land = 0.16`, 1 sigma).
///
/// So this is a **separate term with its own amplitude, windowed by `|above_shore|`**, added
/// after calibration rather than inside it. Three consequences, and the third is the one
/// worth reading twice:
///
/// 1. **Amplitude is a free parameter.** The coast can be roughened to a visible degree
///    without redistributing anything.
/// 2. **`shore` and `spread` do not move at all.** Calibration samples `at`, and `at` is
///    untouched by this block -- so `calibration_reproduces_the_python_reference` holds
///    by construction rather than by luck, and the Nyquist argument above never arises.
/// 3. **Land fraction is preserved to first order for free.** The window is symmetric about
///    the shore and the perturbation is zero-mean, so it moves the coast inland exactly as
///    often as it moves it seaward. **Measured rather than believed** -- see
///    `src/bin/coastline_survey.rs`, and `the_window_is_symmetric_about_the_shore` below.
///
/// **This block does not change what `None` means.** A changed default here is a change to
/// `worldbuilder/terrain/continentality.py`, which `tests/test_conformance.py` treats as
/// ground truth.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoastParams {
    /// How far the coast can be pushed, in multiples of `spread` -- the same unit
    /// `base_elevation` normalises `above_shore` by, so an amplitude of 0.25 moves the
    /// normalised field by at most a quarter of the way from the shore to a saturated
    /// continental interior. **Exactly 0.0 is the inert value and is what `canonical()`
    /// carries**; `above_shore` branches on it before touching the noise.
    pub amplitude: f64,
    /// How wide the coastal band is, in multiples of `spread`. The window is 1.0 at the
    /// shore and exactly 0.0 at `window_spreads * spread` in either direction, so at 1.0
    /// the term lives entirely inside the band where `elevation_from_above` has not yet
    /// saturated -- past that, a perturbation could not move the drawn ground anyway.
    pub window_spreads: f64,
    /// Cycles per unit of noise space at the term's first octave. `BASE_FREQUENCY` is 1.25
    /// and the existing field's finest octave is 10.0; this starts above that, which is the
    /// whole point -- it is the band the four-octave field does not have.
    pub frequency: f64,
    /// How many octaves the term itself spans.
    pub octaves: u32,
    /// Amplitude ratio between successive octaves of the term.
    pub gain: f64,
    /// Frequency ratio between successive octaves of the term.
    pub lacunarity: f64,
}

impl CoastParams {
    /// Today's coastline exactly: **no coastal term at all**. `amplitude` is 0.0, and
    /// `above_shore` returns the unperturbed value without sampling, so
    /// `Some(CoastParams::canonical())` is bit-identical to `None` -- pinned by
    /// `coast_none_matches_coast_some_canonical_bit_for_bit` below and, at the whole-world
    /// level, by `surface.rs::coast_none_matches_coast_some_canonical_bit_for_bit`.
    ///
    /// The other five fields carry the shape a caller would want if they raised the
    /// amplitude, so opting in is one field rather than six. They are inert until then.
    pub fn canonical() -> Self {
        Self {
            amplitude: 0.0,
            window_spreads: 1.0,
            frequency: 20.0,
            octaves: 4,
            gain: 0.5,
            lacunarity: 2.0,
        }
    }

    /// A named, opt-in preset: a coastline with bays, offshore islands and peninsulas at
    /// several scales. **`canonical()` above is untouched by this constructor and stays the
    /// `None` path.**
    ///
    /// One field moves, and it was swept rather than chosen: `src/bin/coastline_survey.rs`
    /// measures land fraction, coastline length against the unroughened coast at three
    /// sample spacings, island count and largest-landmass share across
    /// `amplitude = 0.0 .. 1.5`. The numbers and the reasoning for this value are in
    /// `.superpowers/sdd/2026-09-05-slice-photoreal/task-5-report.md`.
    ///
    /// The frequency schedule is deliberately stated in `canonical()` rather than here, so
    /// this preset is one number and a reader can see that it is.
    pub fn fractal() -> Self {
        Self { amplitude: FRACTAL_AMPLITUDE, ..Self::canonical() }
    }
}

/// `CoastParams::fractal()`'s amplitude, named so the survey binary and the preset cannot
/// drift apart. Swept, not chosen -- see `fractal()`.
pub const FRACTAL_AMPLITUDE: f64 = 0.35;

/// The coastal window: one at the shore, zero at and beyond the far edge of the band, a
/// smoothstep between.
///
/// `reach_fraction` is `|above_shore| / (window_spreads * spread)`.
///
/// **Written as explicit branches, and that is not a style preference.** `f64::min`,
/// `f64::max` and `.clamp` are NaN-asymmetric and this repository's own guard does not
/// catch them; a window function is a clamp waiting to be written. The branch order here
/// makes the NaN case a decision rather than an accident: NaN satisfies none of the three
/// comparisons, so it falls to the final arm and the window closes. A NaN that reached the
/// noise multiply instead would put a NaN into `above_shore` and from there into every
/// elevation on the world.
pub fn coast_window(reach_fraction: f64) -> f64 {
    if reach_fraction > 0.0 && reach_fraction < 1.0 {
        // The same smoothstep `detail.rs::smooth` applies, on `1 - x`, and pinned against
        // it by `the_window_is_the_house_smoothstep` below rather than merely resembling it.
        let x = 1.0 - reach_fraction;
        x * x * (3.0 - 2.0 * x)
    } else if reach_fraction <= 0.0 {
        // At the shore, and on the negative side a caller cannot reach through `abs`.
        1.0
    } else {
        // `reach_fraction >= 1.0`, and NaN -- outside the band, or unanswerable.
        0.0
    }
}

/// Which way continentality increases, here, and how sharply. Change per metre.
#[derive(Debug, Clone, Copy)]
pub struct Gradient {
    pub east: f64,
    pub north: f64,
}

impl Gradient {
    pub fn magnitude(&self) -> f64 {
        m::hypot(self.east, self.north)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Continentality {
    pub radius_m: f64,
    pub land_fraction: f64,
    /// The world seed this field was built from, kept so a LATER layer can salt its own
    /// noise from the same world.
    ///
    /// **Recorded, and never read by anything in this module.** `Tectonics` holds a
    /// `Continentality` and its opt-in structure field needs a world seed; `Tectonics::new`
    /// has no seed parameter, and adding one would have moved six call sites including two
    /// conformance bindings, for a value this struct already receives and today discards.
    /// Nothing in `continentality.rs` consults it, so no continentality output can depend
    /// on it -- pinned by `the_recorded_seed_is_not_read_by_this_module` below.
    world_seed: u64,
    noise: Noise,
    shore: f64,
    spread: f64,
    /// The opt-in coastal roughening block. `None` is canonical and is what every existing
    /// caller gets; see `CoastParams`.
    coast: Option<CoastParams>,
    /// The lattice the coastal term draws from, salted with `COAST_NOISE_SALT`. Built
    /// unconditionally -- `Noise::new` is a hash of two integers and stores nothing else
    /// (see `surface.rs`: "empty at rest and empty forever") -- and **read only when
    /// `coast` is `Some` with a non-zero amplitude**, so its existence cannot move a
    /// canonical world. Pinned by `the_coast_lattice_is_not_read_on_the_canonical_path`.
    coast_noise: Noise,
}

impl Continentality {
    /// What world this field was built for. See the field's own note: it is a record kept
    /// for other layers, and this module never reads it.
    pub fn world_seed(&self) -> u64 {
        self.world_seed
    }

    /// The canonical field: exactly `worldbuilder/terrain/continentality.py`, no coastal
    /// term. Delegates to `with_coast` with `None`.
    ///
    /// **Why this signature did not grow an eighth parameter.** `ReliefParams` and
    /// `TectonicParams` each widened the constructor they attach to, and each had a handful
    /// of call sites. `Continentality::new` has twenty-odd and `Surface::new` seventy;
    /// widening both would put a mechanical `, None` at ninety sites in one commit whose
    /// point is a coastline. The C ABI this crate already ships made the same call for the
    /// same reason -- `wb_world_new`, `wb_world_new_relief` and `wb_world_new_tectonic` are
    /// three entry points, not one widened three times -- so a delegating constructor is
    /// the house form here rather than a fourth convention. What the pattern actually
    /// requires is unchanged and is enforced below: the parameter is opt-in, `None` is
    /// canonical, `canonical()` is inert, and `None` is bit-identical to
    /// `Some(CoastParams::canonical())`.
    pub fn new(world_seed: u64, radius_m: f64, land_fraction: f64) -> Self {
        Self::with_coast(world_seed, radius_m, land_fraction, None)
    }

    /// The same field, with an opt-in coastal roughening block.
    ///
    /// `coast`: `None` for today's coastline, byte-for-byte -- or `Some(params)` for a
    /// caller-chosen `CoastParams`.
    pub fn with_coast(
        world_seed: u64,
        radius_m: f64,
        land_fraction: f64,
        coast: Option<CoastParams>,
    ) -> Self {
        // Calibration runs before the struct is built, so no partially-built value with
        // placeholder shore/spread can ever exist — mirroring the Python, where
        // `_calibrate()` runs inside `__init__` and there is no window to observe an
        // uncalibrated instance.
        //
        // **And `coast` is not one of its arguments, deliberately.** The window the coastal
        // term is scaled by needs `shore` and `spread`, which calibration is what produces;
        // feeding the perturbed field back into its own calibration would be circular. So
        // the term is applied AFTER, in `above_shore`, and the calibration pair is
        // identical on every path -- which is why `shore()` and `spread()`, both part of the
        // conformance surface, cannot move no matter what a caller passes here.
        let noise = Noise::new(world_seed, NOISE_SALT);
        let (shore, spread) = Self::calibrate(&noise, land_fraction);
        Self {
            radius_m,
            land_fraction,
            world_seed,
            noise,
            shore,
            spread,
            coast,
            coast_noise: Noise::new(world_seed, COAST_NOISE_SALT),
        }
    }

    /// Where sea level falls, and how varied the field is.
    ///
    /// The sample points are a fixed Fibonacci spiral and the field is a pure function, so
    /// this is generated-and-stored and still perfectly deterministic.
    fn calibrate(noise: &Noise, land_fraction: f64) -> (f64, f64) {
        let golden = core::f64::consts::PI * (3.0 - m::sqrt(5.0));
        let n = CALIBRATION_SAMPLES;
        let mut values: Vec<f64> = Vec::with_capacity(n);

        for index in 0..n {
            let z = 1.0 - 2.0 * (index as f64 + 0.5) / (n as f64); // cast-ok: loop counter to float, no truncation
            let inner = 1.0 - z * z;
            // Python writes max(0.0, 1.0 - z*z); two-argument max returns the second
            // argument when the comparison is false, so a NaN inner would yield 0.0.
            let ring = m::sqrt(if inner > 0.0 { inner } else { 0.0 });
            let angle = golden * index as f64; // cast-ok: loop counter to float, no truncation
            let point = SpherePoint {
                vector: crate::vectors::Vec3::new(m::cos(angle) * ring, m::sin(angle) * ring, z),
            };
            // The sample point is deliberately NOT normalised — the Python builds the
            // vector directly and hands it to SpherePoint, and the spiral already lies on
            // the unit sphere to within rounding.
            let v = point.vector;
            values.push(noise.fbm(v.x, v.y, v.z, BASE_FREQUENCY, OCTAVES, 0.5, 2.0));
        }

        // Python's list.sort() on floats and Rust's stable sort_by agree: no NaN is
        // produced here, and -0.0 compares equal to 0.0 in both, with stability keeping
        // the original order in that case.
        values.sort_by(|a, b| a.partial_cmp(b).expect("the field produces no NaN"));

        let last = (n - 1) as f64; // cast-ok: count to float, exact for n far below 2^53
        let spread_index = (0.84 * last) as usize; // cast-ok: truncation, matching Python's int()

        // THE SHORE INDEX IS A SATURATING CAST, AND A NaN SATURATES TO ZERO.
        //
        // `values` is sorted ascending, so `values[0]` is the field's GLOBAL MINIMUM: a NaN
        // `land_fraction` would put sea level below every sample on the planet and hand back
        // a world that is entirely land. Measured, seed 12345: shore -0.6889 against a
        // canonical 0.0956, and 2000 of 2000 spiral points above the shore against 578.
        //
        // That is not a wrong-looking number, it is a WORLD -- and it is bit-identical to the
        // world `land_fraction = 1.0` legitimately produces, which is exactly what makes it
        // invisible. This is `elevation_from_above`'s silent abyss one function up, with a
        // cast doing the swallowing instead of a pair of comparisons.
        //
        // The contract is the same one, for the same reasons: propagate. Python's `int()`
        // RAISES on a NaN, so there is no oracle value to match and no canonical sample that
        // can reach here; a panic is unavailable, because this constructor is reached through
        // `extern "C"` exports where nounwind makes a panic an abort; and refusing at the
        // boundary cannot cover the reach, since `wb_world_new` already refuses this while
        // `bindings::continentality_*` -- which take a bare `f64` -- do not. A NaN shore makes
        // `above_shore` NaN, which `elevation_from_above` carries out as a NaN, so the world
        // reads as unanswerable rather than as dry land.
        let shore = if land_fraction.is_nan() {
            f64::NAN
        } else {
            // The marker below is true only BECAUSE of the branch above. Within `[0, 1]`, the
            // documented domain, this is Python's `int()` exactly. Outside it the languages
            // already disagree and both ends are loud enough to find: below 0 the product
            // runs past `last` and the index panics (measured -- it is why `wb_world_new`
            // bounds this parameter), and above 1 it saturates to 0, which is the answer
            // `land_fraction = 1.0` gives and is the monotone continuation of the curve
            // rather than a surprise.
            let shore_index = ((1.0 - land_fraction) * last) as usize; // cast-ok: truncation, matching Python's int(); NaN excluded above
            values[shore_index]
        };
        let middle = values[n / 2];
        let difference = values[spread_index] - middle;
        // Python writes `... or 1e-6`, and both 0.0 and -0.0 are falsy there, so either
        // becomes 1e-6. NaN is truthy and passes through unchanged.
        let spread = if difference == 0.0 { 1e-6 } else { difference };
        (shore, spread)
    }

    /// The raw field, before sea level has been decided.
    pub fn at(&self, point: &SpherePoint) -> f64 {
        let v = point.vector;
        self.noise.fbm(v.x, v.y, v.z, BASE_FREQUENCY, OCTAVES, 0.5, 2.0)
    }

    /// How far above the shoreline this point stands, in field units. Zero exactly at the
    /// coast, positive inland.
    ///
    /// **This is where the opt-in coastal term lives, and it is the only place it lives.**
    /// `at` above stays the raw four-octave field: calibration reads it, `gradient` below
    /// reads it, and `tectonics.rs` probes it inboard along a margin. Perturbing `at`
    /// instead would push the roughening into all three, and into a calibration that
    /// produced the very `shore` the perturbation is measured from.
    ///
    /// Everything that decides land from sea goes through this method or through
    /// `base_elevation`, which calls it -- so the roughened boundary reaches `Shelf` and
    /// `Surface` without either of them knowing this block exists.
    pub fn above_shore(&self, point: &SpherePoint) -> f64 {
        let raw = self.at(point) - self.shore;
        match self.coast {
            None => raw,
            // The zero-amplitude guard is an early return rather than a `+ 0.0`, and that
            // is load-bearing: `-0.0 + 0.0` is `+0.0`, so adding an exactly-zero offset
            // would flip the sign bit of a point sitting exactly on the calibrated shore
            // and `Some(canonical())` would not be BIT-identical to `None`. Same shape as
            // `detail.rs::offset_m`'s `amplitude_m <= 0.0` guard.
            Some(params) if params.amplitude == 0.0 => raw,
            Some(params) => raw + self.coast_offset(point, raw, &params),
        }
    }

    /// The coastal term itself: a zero-mean fBm, scaled by `amplitude * spread`, windowed
    /// by how far this point is from the calibrated shore.
    ///
    /// `raw_above` is the UNPERTURBED `above_shore`, and the window is a function of that
    /// rather than of the answer -- the alternative is an implicit equation with no reason
    /// to have a solution. It also keeps the window symmetric about the shore, which is the
    /// property that holds land fraction: the perturbation is drawn from the same
    /// distribution on both sides, so it moves the coast inland exactly as often as it
    /// moves it seaward.
    fn coast_offset(&self, point: &SpherePoint, raw_above: f64, params: &CoastParams) -> f64 {
        let reach = self.spread * params.window_spreads;
        // `spread` is a positive quantile difference by construction (`calibrate` falls
        // back to 1e-6), but `window_spreads` is a caller's number. A non-positive or
        // unanswerable reach closes the window rather than dividing by it.
        let window = if reach > 0.0 { coast_window(raw_above.abs() / reach) } else { 0.0 };
        if window > 0.0 {
            let v = point.vector;
            let field = self.coast_noise.fbm(
                v.x,
                v.y,
                v.z,
                params.frequency,
                params.octaves,
                params.gain,
                params.lacunarity,
            );
            // `spread`, not `reach`: the amplitude is stated in the same unit
            // `base_elevation` normalises by, so widening the window does not silently
            // deepen the bays.
            params.amplitude * self.spread * window * field
        } else {
            0.0
        }
    }

    /// The coastal block this field was built with, if any. Exposed so a caller can see
    /// what it asked for, and so the survey binary reports the configuration it measured
    /// rather than the one it believes it passed.
    pub fn coast(&self) -> Option<CoastParams> {
        self.coast
    }

    /// Elevation relative to datum, before tectonics or detail.
    pub fn base_elevation(&self, point: &SpherePoint) -> f64 {
        self.elevation_from_above(self.above_shore(point) / self.spread)
    }

    /// The curve itself, separated so it can be exercised without hunting for a point that
    /// happens to land at a given height.
    ///
    /// # A NaN leaves here as a NaN, and that is the contract
    ///
    /// **Both arms below are NaN-asymmetric, and both of them are false for a NaN.** Before
    /// the guard existed a NaN `above` fell through `above >= 0.0`, then through
    /// `depth < 1.0`, and came out as `ABYSS_M * 1.0` -- a plausible abyssal metre rather
    /// than an error. Every affected point silently became the deepest ocean on the planet,
    /// and every `is_finite` assertion in this crate stayed green while it happened. That is
    /// the worst failure shape this codebase has: wrong, quiet, and self-consistent.
    ///
    /// **The guard is here rather than at any one entrant because three of them converge on
    /// this function**, and it is the only place all three meet:
    ///
    /// 1. **A non-finite `latitude_deg` or `longitude_deg` through the C ABI.**
    ///    `wb_elevation_m`, `wb_structural_m`, `wb_tile_*` and `wb_bottom_at` take two bare
    ///    `f64` and validate neither; `SpherePoint::from_latlon` turns a NaN *or an infinity*
    ///    into an all-NaN vector, and `Noise::at` carries that straight through `at`.
    ///    Measured: `base_elevation` at a NaN latitude returned exactly `-4600.0`.
    /// 2. **A non-finite `x`, `y` or `z` through the Python bindings.**
    ///    `bindings::continentality_base_elevation` builds a `SpherePoint` from the caller's
    ///    three components and normalises nothing.
    /// 3. **The opt-in coastal term.** `Noise::fbm` can reach `inf / inf` when the octave
    ///    schedule overflows; refused at the C ABI by `WB_MAX_COAST_GAIN`, but reachable from
    ///    any Rust caller of [`Continentality::with_coast`], and inherited by any future term
    ///    that lands in `above_shore`.
    ///
    /// **Propagate rather than refuse, and the reason is `extern "C"`.** Every export that
    /// reaches here is nounwind, so a refusal expressed as a panic is an abort; and
    /// `wb_elevation_m` returns one scalar with no status channel, already documenting NaN as
    /// the value it gives for a question it cannot answer. Propagation makes that answer
    /// honest, and it makes every finiteness assertion downstream -- `sample_coast`'s among
    /// them -- load-bearing instead of merely looking it.
    ///
    /// **Invisible on the canonical path, and that is checked rather than argued.**
    /// `worldbuilder/terrain/continentality.py` is the oracle for 157 conformance tests, and
    /// its `min(1.0, above)` would hand back `1.0` for a NaN exactly as the arms below did --
    /// but no canonical input can produce one. `at` is finite at every finite point, `shore`
    /// and `spread` come from a fixed spiral over the raw field (`calibrate`'s own sort
    /// asserts no NaN reaches it), and `continentality_corpus()` is normalised sphere points.
    /// See `a_nan_above_the_shore_surfaces_as_a_nan_rather_than_as_the_abyss` below.
    pub fn elevation_from_above(&self, above: f64) -> f64 {
        // Explicit, and FIRST. This is `plates.rs::margin_at`'s house form turned around:
        // there a NaN is floored deliberately because the value being guarded is a weight,
        // and a floored weight is visible in the product. Here the value is a METRE, and a
        // floored metre is indistinguishable from a real one.
        if above.is_nan() {
            return f64::NAN;
        }
        if above >= 0.0 {
            // Python: CONTINENT_M * min(1.0, above) ** 0.75
            let capped = if above < 1.0 { above } else { 1.0 };
            CONTINENT_M * m::powf(capped, 0.75)
        } else {
            // Linear on the seaward side, and that number was measured rather than chosen.
            // Python: ABYSS_M * min(1.0, -above)
            let depth = -above;
            let capped = if depth < 1.0 { depth } else { 1.0 };
            ABYSS_M * capped
        }
    }

    /// Which way continentality rises, measured along the surface.
    pub fn gradient(&self, point: &SpherePoint) -> Gradient {
        let frame = crate::tangent::TangentFrame::at(point, self.radius_m);
        let step = GRADIENT_STEP_M;
        let east = self.at(&frame.local_to_sphere(step, 0.0));
        let west = self.at(&frame.local_to_sphere(-step, 0.0));
        let north = self.at(&frame.local_to_sphere(0.0, step));
        let south = self.at(&frame.local_to_sphere(0.0, -step));
        Gradient {
            east: (east - west) / (2.0 * step),
            north: (north - south) / (2.0 * step),
        }
    }

    /// Where sea level fell for this calibration. Exposed for bindings and tests alike --
    /// the calibration pair is itself part of the conformance surface, not an internal.
    pub fn shore(&self) -> f64 {
        self.shore
    }

    /// The spread used to normalise `above_shore` before the elevation curve.
    pub fn spread(&self) -> f64 {
        self.spread
    }

    #[cfg(test)]
    pub fn shore_for_test(&self) -> f64 {
        self.shore
    }

    #[cfg(test)]
    pub fn spread_for_test(&self) -> f64 {
        self.spread
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // EARTH_RADIUS_M is a test-only need here, so it is imported in this module rather
    // than at file scope, where a non-test build reports it unused.
    use crate::sphere::{SpherePoint, EARTH_RADIUS_M};

    #[test]
    fn the_constants_match_the_python() {
        assert_eq!(BASE_FREQUENCY.to_bits(), 1.25f64.to_bits());
        assert_eq!(OCTAVES, 4);
        assert_eq!(CONTINENT_M.to_bits(), 700.0f64.to_bits());
        assert_eq!(ABYSS_M.to_bits(), (-4600.0f64).to_bits());
        assert_eq!(LAND_FRACTION.to_bits(), 0.29f64.to_bits());
        assert_eq!(CALIBRATION_SAMPLES, 4000);
        assert_eq!(GRADIENT_STEP_M.to_bits(), 20000.0f64.to_bits());
        assert_eq!(NOISE_SALT, 0x0C0FFEE);
    }

    // ---- Task 5: the opt-in coastal roughening block -----------------------------------

    /// A fixed area-uniform spiral, the same construction `calibrate` uses, so every test
    /// below has a population it can name rather than a handful of latitudes somebody liked.
    fn spiral(count: usize) -> Vec<SpherePoint> {
        let golden = core::f64::consts::PI * (3.0 - m::sqrt(5.0));
        let n = count as f64; // cast-ok: sample count to float, exact far below 2^53
        (0..count)
            .map(|index| {
                let i = index as f64; // cast-ok: loop counter to float, exact far below 2^53
                let z = 1.0 - 2.0 * (i + 0.5) / n;
                let inner = 1.0 - z * z;
                let ring = m::sqrt(if inner > 0.0 { inner } else { 0.0 });
                let angle = golden * i;
                SpherePoint {
                    vector: crate::vectors::Vec3::new(m::cos(angle) * ring, m::sin(angle) * ring, z),
                }
            })
            .collect()
    }

    /// Ruling 1's central claim at the point this task touches: adding `CoastParams` beside
    /// the existing constructor must not perturb what `None` means, and `canonical()` must
    /// be the inert value rather than merely a small one.
    ///
    /// **The last two assertions are what stop this passing vacuously.** A population where
    /// nothing ever differs would satisfy the first loop under any implementation at all, so
    /// the same population is re-measured under `fractal()` and required to differ at a
    /// substantial number of points -- the field is demonstrably capable of moving, and it
    /// did not move.
    #[test]
    fn coast_none_matches_coast_some_canonical_bit_for_bit() {
        let none = Continentality::with_coast(20_260_905, EARTH_RADIUS_M, LAND_FRACTION, None);
        let canonical = Continentality::with_coast(
            20_260_905,
            EARTH_RADIUS_M,
            LAND_FRACTION,
            Some(CoastParams::canonical()),
        );
        let fractal = Continentality::with_coast(
            20_260_905,
            EARTH_RADIUS_M,
            LAND_FRACTION,
            Some(CoastParams::fractal()),
        );

        let points = spiral(2000);
        let mut moved = 0usize;
        for (index, point) in points.iter().enumerate() {
            assert_eq!(
                none.above_shore(point).to_bits(),
                canonical.above_shore(point).to_bits(),
                "above_shore at spiral index {index}"
            );
            assert_eq!(
                none.base_elevation(point).to_bits(),
                canonical.base_elevation(point).to_bits(),
                "base_elevation at spiral index {index}"
            );
            if none.above_shore(point).to_bits() != fractal.above_shore(point).to_bits() {
                moved += 1;
            }
        }
        assert!(
            moved > 100,
            "fractal() moved only {moved} of {} points -- this test cannot prove canonical() \
             is inert if the mechanism it is inert AGAINST does nothing either",
            points.len()
        );
        // And the shore itself is untouched on every path, because calibration never sees
        // this block: `shore()` and `spread()` are part of the conformance surface.
        assert_eq!(none.shore().to_bits(), fractal.shore().to_bits());
        assert_eq!(none.spread().to_bits(), fractal.spread().to_bits());
    }

    /// The window is the same smoothstep the rest of the engine uses, on `1 - x`. Asserted
    /// against `detail::smooth` by bits rather than described as resembling it, so a later
    /// edit to either one shows up here.
    #[test]
    fn the_window_is_the_house_smoothstep() {
        let mut fraction = -0.5f64;
        while fraction <= 1.5 {
            assert_eq!(
                coast_window(fraction).to_bits(),
                crate::detail::smooth(1.0 - fraction).to_bits(),
                "at reach fraction {fraction}"
            );
            fraction += 0.01;
        }
        assert_eq!(coast_window(0.0).to_bits(), 1.0f64.to_bits());
        assert_eq!(coast_window(1.0).to_bits(), 0.0f64.to_bits());
        // The branch order's stated purpose: an unanswerable reach fraction CLOSES the
        // window rather than putting a NaN into every elevation on the world. `smooth` gets
        // there by its clamp order; this gets there by an explicit final arm, and the two
        // agreeing is the point of the loop above.
        assert_eq!(coast_window(f64::NAN).to_bits(), 0.0f64.to_bits());
    }

    /// The claim the whole technique rests on: the term acts at the coast and nowhere else.
    /// Deep interiors and abyssal plains must come back BIT-identical.
    ///
    /// Discriminated by its own last assertion: the same sweep counts points inside the band
    /// that DID move, so "nothing moved anywhere" cannot pass this.
    #[test]
    fn the_coastal_term_acts_only_inside_its_own_window() {
        let none = Continentality::with_coast(20_260_905, EARTH_RADIUS_M, LAND_FRACTION, None);
        let fractal = Continentality::with_coast(
            20_260_905,
            EARTH_RADIUS_M,
            LAND_FRACTION,
            Some(CoastParams::fractal()),
        );
        let reach = none.spread() * CoastParams::fractal().window_spreads;
        let ceiling = CoastParams::fractal().amplitude * none.spread();

        let mut outside = 0usize;
        let mut inside_moved = 0usize;
        for point in spiral(4000).iter() {
            let raw = none.above_shore(point);
            let got = fractal.above_shore(point);
            if raw.abs() >= reach {
                outside += 1;
                assert_eq!(
                    got.to_bits(),
                    raw.to_bits(),
                    "a point {} spreads from the shore must be untouched",
                    raw.abs() / none.spread()
                );
            } else {
                if got.to_bits() != raw.to_bits() {
                    inside_moved += 1;
                }
                // `fbm` returns [-1, 1] and the window returns [0, 1], so the offset can
                // never exceed `amplitude * spread`. A term that escaped its own stated
                // amplitude would be a different technique from the one measured.
                assert!(
                    (got - raw).abs() <= ceiling,
                    "offset {} exceeds the stated ceiling {ceiling}",
                    (got - raw).abs()
                );
            }
        }
        assert!(outside > 1000, "only {outside} of 4000 points lay outside the band");
        assert!(
            inside_moved > 100,
            "only {inside_moved} points inside the band moved -- the window cannot be shown \
             to be selective if the term does nothing anywhere"
        );
    }

    /// **The claim the brief said to measure rather than believe.** A window symmetric about
    /// the shore, applied to a zero-mean field, moves the coast inland exactly as often as it
    /// moves it seaward, so land fraction is preserved to first order without recalibration.
    ///
    /// Population: a 20,000-point area-uniform spiral -- 1 sigma binomial standard error at
    /// `p = 0.29` is `sqrt(0.29*0.71/20000) = 0.32 pp`. The bound below is 1.0 pp, about
    /// three sigma of that estimator, and the survey binary measures the same quantity at
    /// 200,000 points (+-0.10 pp) on three worlds where the largest movement seen was
    /// +0.146 pp. This is the cheap version of that check, kept in the suite so a later edit
    /// that breaks the symmetry cannot reach a commit.
    ///
    /// **Discriminated, and this is the assertion that matters**: the same loop counts the
    /// points whose CLASSIFICATION flipped. A term that did nothing would hold land fraction
    /// perfectly and prove nothing, so the flips are required to be numerous.
    #[test]
    fn the_coastal_term_holds_land_fraction_while_moving_the_coast() {
        let none = Continentality::with_coast(20_260_905, EARTH_RADIUS_M, LAND_FRACTION, None);
        let fractal = Continentality::with_coast(
            20_260_905,
            EARTH_RADIUS_M,
            LAND_FRACTION,
            Some(CoastParams::fractal()),
        );
        let points = spiral(20_000);
        let mut land_off = 0usize;
        let mut land_on = 0usize;
        let mut to_land = 0usize;
        let mut to_sea = 0usize;
        for point in points.iter() {
            let was = none.above_shore(point) > 0.0;
            let now = fractal.above_shore(point) > 0.0;
            if was {
                land_off += 1;
            }
            if now {
                land_on += 1;
            }
            if !was && now {
                to_land += 1;
            }
            if was && !now {
                to_sea += 1;
            }
        }
        let total = points.len() as f64; // cast-ok: count to float, exact far below 2^53
        let off = land_off as f64; // cast-ok: count to float, exact far below 2^53
        let on = land_on as f64; // cast-ok: count to float, exact far below 2^53
        let shift_pp = (on - off) / total * 100.0;
        assert!(
            shift_pp.abs() < 1.0,
            "land fraction moved {shift_pp} pp ({land_off} -> {land_on} of {}) -- the window \
             is no longer symmetric about the shore",
            points.len()
        );
        assert!(
            to_land > 50 && to_sea > 50,
            "the coast moved seaward at {to_sea} points and inland at {to_land} -- a term \
             that moved nothing would hold land fraction perfectly and mean nothing"
        );
    }

    /// The sibling of `the_recorded_seed_is_not_read_by_this_module`, for the second lattice:
    /// a canonical world must not draw from `coast_noise` at all. The field is private, so
    /// this child module is the only place that can nudge it after construction and watch the
    /// outputs not move.
    #[test]
    fn the_coast_lattice_is_not_read_on_the_canonical_path() {
        let canonical = Continentality::new(20_260_905, EARTH_RADIUS_M, LAND_FRACTION);
        let mut nudged = canonical;
        nudged.coast_noise = Noise::new(99, 99);

        let points = spiral(400);
        for (index, point) in points.iter().enumerate() {
            assert_eq!(
                canonical.above_shore(point).to_bits(),
                nudged.above_shore(point).to_bits(),
                "above_shore at spiral index {index}"
            );
        }

        // And the nudge is real: the SAME nudge on a world that HAS opted in does move the
        // answer, so this passes because the canonical path never reads the lattice, not
        // because replacing a lattice is inert.
        let opted_in = Continentality::with_coast(
            20_260_905,
            EARTH_RADIUS_M,
            LAND_FRACTION,
            Some(CoastParams::fractal()),
        );
        let mut opted_in_nudged = opted_in;
        opted_in_nudged.coast_noise = Noise::new(99, 99);
        let moved = points
            .iter()
            .filter(|p| opted_in.above_shore(p).to_bits() != opted_in_nudged.above_shore(p).to_bits())
            .count();
        assert!(moved > 10, "the nudge moved only {moved} points on an opted-in world");
    }

    /// The trap the constraints name by name: a window function is a clamp waiting to be
    /// written, and a clamp is NaN-asymmetric. A caller's `window_spreads` is the one field
    /// that reaches a division, so every degenerate value of it must close the window rather
    /// than poison the world.
    #[test]
    fn a_degenerate_window_width_closes_the_window_instead_of_poisoning_the_field() {
        let none = Continentality::with_coast(20_260_905, EARTH_RADIUS_M, LAND_FRACTION, None);
        let points = spiral(200);
        for width in [0.0, -1.0, f64::NAN] {
            let odd = Continentality::with_coast(
                20_260_905,
                EARTH_RADIUS_M,
                LAND_FRACTION,
                Some(CoastParams { window_spreads: width, ..CoastParams::fractal() }),
            );
            for point in points.iter() {
                let got = odd.above_shore(point);
                assert!(got.is_finite(), "above_shore was {got} at window_spreads {width}");
                assert_eq!(
                    got.to_bits(),
                    none.above_shore(point).to_bits(),
                    "a window of width {width} must be closed, not merely finite"
                );
            }
        }
        // An infinite width is NOT degenerate -- it is a window that never closes -- so it
        // must be admitted rather than swept into the branch above. Stated here so the
        // assertion above cannot quietly grow to cover it.
        let endless = Continentality::with_coast(
            20_260_905,
            EARTH_RADIUS_M,
            LAND_FRACTION,
            Some(CoastParams { window_spreads: f64::INFINITY, ..CoastParams::fractal() }),
        );
        let moved = points
            .iter()
            .filter(|p| endless.above_shore(p).to_bits() != none.above_shore(p).to_bits())
            .count();
        assert!(moved > 100, "an endless window moved only {moved} of 200 points");
    }

    /// `fractal()` must not be `canonical()` with the serial numbers filed off: exactly one
    /// field moves, and it is the amplitude.
    #[test]
    fn fractal_only_moves_the_amplitude() {
        let fractal = CoastParams::fractal();
        let canonical = CoastParams::canonical();
        assert_eq!(fractal.window_spreads, canonical.window_spreads);
        assert_eq!(fractal.frequency, canonical.frequency);
        assert_eq!(fractal.octaves, canonical.octaves);
        assert_eq!(fractal.gain, canonical.gain);
        assert_eq!(fractal.lacunarity, canonical.lacunarity);
        assert_eq!(canonical.amplitude.to_bits(), 0.0f64.to_bits());
        assert_eq!(fractal.amplitude.to_bits(), FRACTAL_AMPLITUDE.to_bits());
        assert!(fractal.amplitude > 0.0);
        // The salt must differ from the field it perturbs and from `detail.rs`'s, or the
        // wobble correlates with the thing deciding where the coast is.
        assert_ne!(COAST_NOISE_SALT, NOISE_SALT);
        assert_ne!(COAST_NOISE_SALT, 0x5EABED);
        // The term must start above the base field's own finest octave, or it is not the
        // band the four-octave field is missing: BASE_FREQUENCY * lacunarity^(OCTAVES-1).
        assert!(fractal.frequency > BASE_FREQUENCY * 8.0);
    }

    #[test]
    fn a_gradient_reports_its_magnitude() {
        let g = Gradient { east: 3.0, north: 4.0 };
        assert_eq!(g.magnitude().to_bits(), 5.0f64.to_bits());
    }

    #[test]
    fn the_field_varies_across_the_planet() {
        let c = Continentality::new(12345, EARTH_RADIUS_M, LAND_FRACTION);
        let a = c.at(&SpherePoint::from_latlon(0.0, 0.0));
        let b = c.at(&SpherePoint::from_latlon(45.0, 90.0));
        assert_ne!(a.to_bits(), b.to_bits());
        assert!(a.is_finite() && b.is_finite());
    }

    #[test]
    fn the_field_is_reproducible() {
        let a = Continentality::new(12345, EARTH_RADIUS_M, LAND_FRACTION);
        let b = Continentality::new(12345, EARTH_RADIUS_M, LAND_FRACTION);
        let p = SpherePoint::from_latlon(31.0, 7.0);
        assert_eq!(a.at(&p).to_bits(), b.at(&p).to_bits());
    }

    #[test]
    fn calibration_reproduces_the_python_reference() {
        // Measured from the Python on seed 12345 at the default land fraction.
        let c = Continentality::new(12345, EARTH_RADIUS_M, LAND_FRACTION);
        assert!((c.shore_for_test() - 0.09556581019557257).abs() < 1e-12,
                "shore was {}", c.shore_for_test());
        assert!((c.spread_for_test() - 0.1984287160252961).abs() < 1e-12,
                "spread was {}", c.spread_for_test());
    }

    #[test]
    fn a_higher_land_fraction_lowers_the_shore() {
        // More land means sea level sits at a lower quantile of the same field.
        let less = Continentality::new(12345, EARTH_RADIUS_M, 0.2);
        let more = Continentality::new(12345, EARTH_RADIUS_M, 0.5);
        assert!(more.shore_for_test() < less.shore_for_test());
    }

    #[test]
    fn the_spread_is_never_zero() {
        let c = Continentality::new(12345, EARTH_RADIUS_M, LAND_FRACTION);
        assert!(c.spread_for_test() != 0.0);
    }

    #[test]
    fn above_shore_is_zero_at_the_calibrated_shoreline() {
        let c = Continentality::new(12345, EARTH_RADIUS_M, LAND_FRACTION);
        // A point whose raw field equals the shore has above_shore exactly zero.
        let p = SpherePoint::from_latlon(17.0, 43.0);
        let expected = c.at(&p) - c.shore_for_test();
        assert_eq!(c.above_shore(&p).to_bits(), expected.to_bits());
    }

    #[test]
    fn elevation_is_bounded_by_the_continent_and_the_abyss() {
        let c = Continentality::new(12345, EARTH_RADIUS_M, LAND_FRACTION);
        for lat in (-80..81).step_by(10) {
            for lon in (-180..181).step_by(20) {
                let e = c.base_elevation(&SpherePoint::from_latlon(lat as f64, lon as f64));
                assert!(e <= CONTINENT_M, "{} at {},{}", e, lat, lon);
                assert!(e >= ABYSS_M, "{} at {},{}", e, lat, lon);
            }
        }
    }

    #[test]
    fn the_gradient_points_uphill() {
        let c = Continentality::new(12345, EARTH_RADIUS_M, LAND_FRACTION);
        let p = SpherePoint::from_latlon(20.0, 30.0);
        let g = c.gradient(&p);
        let frame = crate::tangent::TangentFrame::at(&p, EARTH_RADIUS_M);
        // Stepping a little way along the gradient should raise the field.
        let step = 5000.0;
        let scale = step / g.magnitude();
        let uphill = frame.local_to_sphere(g.east * scale, g.north * scale);
        assert!(c.at(&uphill) > c.at(&p), "gradient did not point uphill");
    }

    #[test]
    fn the_gradient_is_finite_everywhere_including_the_poles() {
        let c = Continentality::new(12345, EARTH_RADIUS_M, LAND_FRACTION);
        for (lat, lon) in [(90.0, 0.0), (-90.0, 0.0), (0.0, 0.0), (45.0, -170.0)] {
            let g = c.gradient(&SpherePoint::from_latlon(lat, lon));
            assert!(g.east.is_finite() && g.north.is_finite(), "at {},{}", lat, lon);
        }
    }

    #[test]
    fn the_seaward_side_is_linear() {
        // Twice as far below the shore is twice as deep, until the abyss clamps it.
        let c = Continentality::new(12345, EARTH_RADIUS_M, LAND_FRACTION);
        let quarter = c.elevation_from_above(-0.25);
        let half = c.elevation_from_above(-0.5);
        assert!((half - 2.0 * quarter).abs() < 1e-9, "{} vs {}", half, quarter);
    }

    /// The test that would have caught the silent abyss.
    ///
    /// `elevation_from_above`'s two arms are `above >= 0.0` and `depth < 1.0`, and **a NaN
    /// is false for both**, so before the guard a NaN fell through to `ABYSS_M * 1.0`. The
    /// planet drowned and every `is_finite` assertion in this crate stayed green -- there was
    /// no non-finite value left anywhere for one to find.
    ///
    /// **Why `assert!(is_nan)` here is not one of this project's decorative assertions.**
    /// The value it displaces, `ABYSS_M`, is a GENUINE output of this same function at a
    /// legitimate input -- asserted below before anything else, so "the drowned answer is
    /// indistinguishable from a real one" is measured rather than asserted about. A test that
    /// only checked `elevation != ABYSS_M` at a hostile point would be checking a bound the
    /// function reaches on its own; this checks the *class* of the number instead.
    ///
    /// All three entrants named in `elevation_from_above`'s doc are exercised, because a
    /// guard proved on one of them is a guard proved on the arithmetic and not on the reach.
    #[test]
    fn a_nan_above_the_shore_surfaces_as_a_nan_rather_than_as_the_abyss() {
        let c = Continentality::new(12345, EARTH_RADIUS_M, LAND_FRACTION);

        // FIRST: the value the old failure produced is a real answer this function gives at
        // a real input. Without this line the assertions below are asserting against a
        // number nothing produces, which is the shape of every check this project has found
        // to be load-bearing in name only.
        assert_eq!(c.elevation_from_above(-2.0).to_bits(), ABYSS_M.to_bits());
        assert_eq!(c.elevation_from_above(-1.0).to_bits(), ABYSS_M.to_bits());

        // The curve itself, both NaN sign bits.
        for above in [f64::NAN, -f64::NAN] {
            let e = c.elevation_from_above(above);
            assert!(e.is_nan(), "elevation_from_above({above}) was {e}, not a NaN");
        }

        // ENTRANT 1: a non-finite latitude or longitude through the C ABI. Nothing between
        // `wb_elevation_m` and here validates either, and `from_latlon` turns an INFINITY
        // into a NaN vector just as readily as a NaN.
        for (lat, lon) in [
            (f64::NAN, 0.0),
            (0.0, f64::NAN),
            (f64::INFINITY, 0.0),
            (f64::NEG_INFINITY, 0.0),
            (0.0, f64::INFINITY),
        ] {
            let p = SpherePoint::from_latlon(lat, lon);
            let e = c.base_elevation(&p);
            assert!(e.is_nan(), "base_elevation at lat {lat} lon {lon} was {e}, not a NaN");
        }

        // ENTRANT 2: a non-finite vector component through the Python bindings, which build
        // a `SpherePoint` from three caller floats and normalise nothing.
        for v in [
            crate::vectors::Vec3::new(f64::NAN, 0.0, 0.0),
            crate::vectors::Vec3::new(0.0, f64::NAN, 0.0),
            crate::vectors::Vec3::new(0.0, 0.0, f64::NAN),
        ] {
            let e = c.base_elevation(&SpherePoint { vector: v });
            assert!(e.is_nan(), "base_elevation at ({},{},{}) was {e}", v.x, v.y, v.z);
        }

        // ENTRANT 3: the opt-in coastal term's own NaN. `Noise::fbm` overflows its running
        // amplitude to `+inf`, `loudest` with it, and `2*total/loudest` is `inf/inf`. The C
        // ABI refuses this record; a Rust caller of `with_coast` is not held by that.
        let drowning = Continentality::with_coast(
            12345,
            EARTH_RADIUS_M,
            LAND_FRACTION,
            Some(CoastParams { gain: 1.0e300, ..CoastParams::fractal() }),
        );
        let mut nan_above = 0usize;
        for point in spiral(200).iter() {
            if drowning.above_shore(point).is_nan() {
                nan_above += 1;
                let e = drowning.base_elevation(point);
                assert!(e.is_nan(), "a NaN above_shore came out of the curve as {e}");
            }
        }
        assert!(nan_above > 50, "only {nan_above} of 200 points had a NaN above_shore; the \
                                 coastal entrant is no longer being exercised");

        // AND THE OTHER HALF: the guard is invisible to every finite input. The two arms are
        // transcribed here as they stood before it, and compared BY BITS -- so a guard that
        // moved any real elevation, on either side of the shore or at either saturation,
        // fails here rather than in the conformance suite an hour later.
        let mut land = 0usize;
        let mut sea = 0usize;
        for above in [
            -1.0e300, -3.0, -1.0, -0.9999999999999999, -0.5, -1.0e-300, -0.0, 0.0, 1.0e-300,
            0.5, 0.9999999999999999, 1.0, 3.0, 1.0e300, f64::INFINITY, f64::NEG_INFINITY,
        ] {
            let want = if above >= 0.0 {
                land += 1;
                let capped = if above < 1.0 { above } else { 1.0 };
                CONTINENT_M * m::powf(capped, 0.75)
            } else {
                sea += 1;
                let depth = -above;
                let capped = if depth < 1.0 { depth } else { 1.0 };
                ABYSS_M * capped
            };
            assert_eq!(
                c.elevation_from_above(above).to_bits(),
                want.to_bits(),
                "the guard moved a finite answer at above = {above}"
            );
        }
        // Both arms were actually walked, including both infinities and both zeros -- an
        // invisibility claim that only ever took one branch would be worth nothing.
        assert_eq!((land, sea), (9, 7));
    }

    /// The calibration sample, sorted, rebuilt here from the spiral rather than read out of
    /// `calibrate` -- which is private and returns only the two numbers it picked. This is
    /// the same construction `tests/test_conformance.py` transcribes from the Python, so
    /// the test below indexes it exactly as `int((1 - land_fraction) * (n - 1))` does.
    fn calibration_values() -> Vec<f64> {
        let noise = Noise::new(12345, NOISE_SALT);
        let golden = core::f64::consts::PI * (3.0 - m::sqrt(5.0));
        let n = CALIBRATION_SAMPLES;
        let mut values: Vec<f64> = Vec::with_capacity(n);
        for index in 0..n {
            let z = 1.0 - 2.0 * (index as f64 + 0.5) / (n as f64); // cast-ok: loop counter to float, no truncation
            let inner = 1.0 - z * z;
            let ring = m::sqrt(if inner > 0.0 { inner } else { 0.0 });
            let angle = golden * index as f64; // cast-ok: loop counter to float, no truncation
            let v = crate::vectors::Vec3::new(m::cos(angle) * ring, m::sin(angle) * ring, z);
            values.push(noise.fbm(v.x, v.y, v.z, BASE_FREQUENCY, OCTAVES, 0.5, 2.0));
        }
        values.sort_by(|a, b| a.partial_cmp(b).expect("the field produces no NaN"));
        values
    }

    /// The test that would have caught the all-land world.
    ///
    /// `calibrate` picks sea level with `values[((1.0 - land_fraction) * last) as usize]`.
    /// **`as usize` saturates, and a NaN saturates to 0** -- so a NaN `land_fraction` chose
    /// the sorted sample's first element, the field's global minimum, and every point on the
    /// planet stood above the shore.
    ///
    /// **Why the assertions here are not decorative.** The number the defect produced is a
    /// number this same function GIVES at a real input: `land_fraction = 1.0` produces the
    /// identical shore, bit for bit, and it is supposed to. So a test that asserted "the NaN
    /// world's shore is not -0.6889" would be asserting against a legitimate answer. The
    /// three things asserted instead are the *class* of the number (NaN, not a height), the
    /// fact that the displaced value is genuinely reachable (pinned below, first), and the
    /// fact that the canonical world is NOT all land (pinned below, second) -- without which
    /// "2000 of 2000 points are land" discriminates nothing.
    #[test]
    fn a_nan_land_fraction_surfaces_as_a_nan_shore_rather_than_as_a_world_of_pure_land() {
        let values = calibration_values();
        let points = spiral(2000);

        // FIRST: the value the defect produced is the field's global minimum, and it is the
        // answer a legitimate `land_fraction` of 1.0 gives. Without this line every
        // assertion below is discriminating against a number nothing produces.
        let all_land = Continentality::new(12345, EARTH_RADIUS_M, 1.0);
        assert_eq!(all_land.shore().to_bits(), values[0].to_bits());
        let all_land_count = points.iter().filter(|p| all_land.above_shore(p) >= 0.0).count();
        assert_eq!(all_land_count, 2000, "land_fraction = 1.0 must legitimately be all land");

        // SECOND: the canonical world is NOT all land, so "every point is land" is a real
        // discriminator rather than a property of this fixture.
        let canonical = Continentality::new(12345, EARTH_RADIUS_M, LAND_FRACTION);
        let canonical_land = points.iter().filter(|p| canonical.above_shore(p) >= 0.0).count();
        assert_eq!(canonical_land, 578, "the canonical fixture's land count moved; re-derive it");
        assert_eq!(canonical.shore().to_bits(), values[((1.0 - LAND_FRACTION) * 3999.0) as usize].to_bits()); // cast-ok: constant 0.71 * 3999, truncation as in Python

        // THE GUARD, both NaN sign bits, and all the way out to a metre.
        for land_fraction in [f64::NAN, -f64::NAN] {
            let c = Continentality::new(12345, EARTH_RADIUS_M, land_fraction);
            assert!(c.shore().is_nan(), "shore was {} for a NaN land fraction", c.shore());
            // `spread` does not read `land_fraction` at all and must not have moved.
            assert_eq!(c.spread().to_bits(), canonical.spread().to_bits());

            let mut nan_elevations = 0usize;
            for point in points.iter() {
                assert!(c.above_shore(point).is_nan());
                if c.base_elevation(point).is_nan() {
                    nan_elevations += 1;
                }
            }
            // Exact, not a threshold: an unanswerable world is unanswerable everywhere.
            assert_eq!(nan_elevations, 2000, "sign bit {}", land_fraction.is_sign_negative());
        }

        // AND THE OTHER HALF: the guard is invisible to every land fraction the domain
        // admits. The pre-guard arithmetic is transcribed inline and compared BY BITS.
        let mut distinct = std::collections::BTreeSet::new();
        for land_fraction in [0.0, 0.05, 0.2, LAND_FRACTION, 0.5, 0.71, 0.95, 1.0] {
            let index = ((1.0 - land_fraction) * 3999.0) as usize; // cast-ok: truncation, the pre-guard expression transcribed
            let c = Continentality::new(12345, EARTH_RADIUS_M, land_fraction);
            assert_eq!(
                c.shore().to_bits(),
                values[index].to_bits(),
                "the guard moved the shore at land_fraction = {land_fraction}"
            );
            distinct.insert(c.shore().to_bits());
        }
        // Eight land fractions, eight different shores -- an invisibility claim compared
        // against a constant would be worth nothing.
        assert_eq!(distinct.len(), 8);
    }

    #[test]
    fn the_recorded_seed_is_not_read_by_this_module() {
        // The claim `world_seed`'s own doc makes -- that it is a record for other layers
        // and nothing here consults it -- asserted rather than trusted. The field is
        // private, so this test, a child module, is the only place that CAN nudge it after
        // construction and watch the outputs not move.
        //
        // Population: a 400-point Fibonacci spiral, `at` and `base_elevation` at every
        // point, compared BY BITS. Discriminated by the two assertions at the end: the
        // nudge is real, and the field is genuinely capable of producing different values
        // when it is fed in at CONSTRUCTION -- so this passes because the field is unread,
        // not because the outputs are constant.
        let a = Continentality::new(20_260_904, EARTH_RADIUS_M, LAND_FRACTION);
        let mut b = a;
        b.world_seed = b.world_seed.wrapping_add(1);

        let golden = core::f64::consts::PI * (3.0 - m::sqrt(5.0));
        for index in 0..400u32 {
            let z = 1.0 - 2.0 * (f64::from(index) + 0.5) / 400.0;
            let inner = 1.0 - z * z;
            let ring = m::sqrt(if inner > 0.0 { inner } else { 0.0 });
            let angle = golden * f64::from(index);
            let point = SpherePoint {
                vector: crate::vectors::Vec3::new(m::cos(angle) * ring, m::sin(angle) * ring, z),
            };
            assert_eq!(a.at(&point).to_bits(), b.at(&point).to_bits(), "at, index {index}");
            assert_eq!(
                a.base_elevation(&point).to_bits(),
                b.base_elevation(&point).to_bits(),
                "base_elevation, index {index}"
            );
        }

        assert_ne!(a.world_seed(), b.world_seed(), "the nudge must be a real change");
        let elsewhere = Continentality::new(20_260_905, EARTH_RADIUS_M, LAND_FRACTION);
        let probe = SpherePoint::from_latlon(12.0, 34.0);
        assert_ne!(
            a.at(&probe).to_bits(),
            elsewhere.at(&probe).to_bits(),
            "a seed passed at construction MUST change the field, or this test proves nothing"
        );
    }
}
