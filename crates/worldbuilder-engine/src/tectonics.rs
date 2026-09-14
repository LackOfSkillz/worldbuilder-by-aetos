//! What plate motion does to the ground.
//!
//! Three rules shape this file, and all three are about what it refuses to do.
//!
//! **It returns a contribution, never an elevation.** `offset_m` is a number to *add* to
//! the continental base, so that shelves, erosion, bathymetry and detail can all compose
//! with tectonics later instead of reverse-engineering what tectonics already overwrote.
//! A function that set an absolute height would have made every one of those layers
//! harder to write, and the damage would not have been visible until they were being
//! written.
//!
//! **It does the expensive work only where it matters.** Most of a planet is nowhere near
//! a margin, and the cost measurements from earlier phases are unambiguous: kinematics
//! are 6.6 microseconds a sample and continentality gradients 33, against 5 for finding
//! the margin at all. So a point far from any boundary returns zero having done nothing
//! but the lookup it had to do anyway. Progressive enrichment - cheap context, then a
//! question, then expensive context - rather than assembling everything and discovering
//! later what was needed.
//!
//! **It has no crust model, and does not pretend to.** Whether a margin is oceanic or
//! continental is answered by sampling continentality either side of it, which is crude
//! and sufficient: the same convergent margin can then behave differently along its
//! length, because the land either side of it differs along its length. When a real crust
//! field arrives it replaces the two probes and nothing else changes - the shapers never
//! learn where the answer came from.
//!
//! Ported from `worldbuilder/terrain/tectonics.py`.

use crate::continentality::Continentality;
use crate::detail::smooth;
use crate::detmath as m;
use crate::kinematics::{motion_between, ACROSS_ENOUGH};
use crate::noise::{Noise, LATTICE_LIMIT};
use crate::plates::{Plate, PlateSet};
use crate::sphere::SpherePoint;
use crate::tangent::TangentFrame;
use crate::vectors::{Vec3, DEGENERATE};

/// Beyond this, a margin does nothing at all and no kinematics are evaluated. Every
/// profile below must reach exactly zero by here, or the gate itself becomes a cliff.
///
/// **`TectonicParams` makes the widths a caller's to choose, and this gate does not
/// move with them.** At `canonical()` every profile is inside it by construction; a width
/// chosen beyond it would be truncated here rather than faded, which is the cliff this
/// constant exists to prevent. Validating a caller-supplied block against this bound
/// belongs at the boundary that admits one (the WASM export, a later task), not here --
/// nothing clamps.
pub const MAX_TECTONIC_RANGE_M: f64 = 420_000.0;

/// How far either side of the margin to ask what kind of ground it is. Far enough to be
/// clear of the transition, near enough to describe this stretch of margin rather than
/// the continent behind it.
pub const PROBE_M: f64 = 300_000.0;

/// Continentality at which a side counts as half continental, and how wide the transition
/// from oceanic to continental is. **A width rather than a threshold, and that matters.**
///
/// The first version used a hard test - continental if above zero - and the ground jumped
/// five hundred and fifty metres wherever a margin crossed it, because the two sides of
/// the test run entirely different profiles. It is the same mistake M1.2 made: a hard
/// selection on a continuous quantity. The branches are blended now, and this is how far
/// it takes.
pub const CONTINENTAL_ENOUGH: f64 = 0.0;
pub const CONTINENTAL_BLEND: f64 = 0.45;

/// How sharply the margin picks a side. The profiles are asymmetric - a trench belongs on
/// the ocean side - so they need to know which side is which, and that answer must also
/// arrive continuously. Where the two sides are equally continental it goes smoothly to
/// nothing, which is correct: a symmetric margin has no side to prefer.
pub const SIDE_SHARPNESS: f64 = 6.0;

/// Closing speed that counts as a thoroughly active margin, in metres per million years.
/// Two plates at four centimetres a year approaching head-on. Faster than this does not
/// build higher mountains; it just saturates.
pub const FULL_RATE_M_PER_MYR: f64 = 80_000.0;

/// The profiles. Height in metres, width in metres, and where the feature sits relative
/// to the margin itself - a trench lies out on the oceanic side, an arc a little inboard.
pub const CONTINENT_COLLISION_M: f64 = 1500.0;
pub const CONTINENT_COLLISION_WIDTH_M: f64 = 400_000.0;

pub const COASTAL_UPLIFT_M: f64 = 900.0;
pub const COASTAL_UPLIFT_WIDTH_M: f64 = 260_000.0;
/// How far inboard of the margin the coastal rise is centred.
///
/// **This was a bare literal inside `from_margin`'s `profile` closure, and it is named here
/// because a boundary that admits a caller-chosen `coastal_uplift_width_m` has to know it.**
/// A bump centred `offset` inboard reaches `offset + width` on its near side, and
/// [`MAX_TECTONIC_RANGE_M`] is where margins stop being evaluated at all -- so the widest
/// admissible coastal width is `MAX_TECTONIC_RANGE_M - COASTAL_UPLIFT_OFFSET_M`, and
/// `wasm.rs` derives it from these two constants rather than writing a third number down.
/// The old comment said binding this literal to `RIFT_WIDTH_M` (which it coincidentally
/// equals) would couple two unrelated profiles; a constant of its own does not.
///
/// The value is unchanged, so the canonical path is bit-for-bit what it was.
pub const COASTAL_UPLIFT_OFFSET_M: f64 = 70_000.0;

pub const TRENCH_M: f64 = -2600.0;
pub const TRENCH_WIDTH_M: f64 = 120_000.0;
pub const TRENCH_OFFSET_M: f64 = 90_000.0;

pub const ISLAND_ARC_M: f64 = 700.0;
pub const ISLAND_ARC_WIDTH_M: f64 = 110_000.0;
pub const ISLAND_ARC_OFFSET_M: f64 = 60_000.0;

pub const RIDGE_M: f64 = 900.0;
pub const RIDGE_WIDTH_M: f64 = 380_000.0;
pub const RIFT_M: f64 = -350.0;
pub const RIFT_WIDTH_M: f64 = 70_000.0;

/// The values that decide how high, how wide, how readily and in what SHAPE the plates
/// build mountains, broken out so a caller who wants a different world can ask for one
/// without touching what "canonical" means.
///
/// **Nine of these size the envelope; four give it structure, and the second group is what
/// turns a blade into a range.** The screenshots this slice's Task 2 started from
/// (`shots/wide-steep-6000m-100km.png` against `shots/wide-canonical.png`) show the same
/// landform rescaled: a soft swell at canonical, a smooth sharp-edged slab at 6,000 m over
/// 100 km. The probe measured that slab at a **7.03% flank grade**, and Davis, Suppe &
/// Dahlen (1983) Table 1 gives the Himalaya at **alpha = 4.0 +- 0.5 deg = a 7.0% grade** --
/// so the envelope's grade was already a real orogen's and the picture was still not a
/// range. Steepness was never the missing thing; local shape was.
///

/// `Tectonics::new` takes `Option<TectonicParams>`, following the house pattern already on
/// `Detail::new`'s `relief: Option<ReliefParams>` and `Surface::new`'s
/// `features: Option<FeatureInput>`: **`None` is the canonical path, not an implicit
/// `Default::default()`** -- this codebase deliberately rejects defaults nobody chose (see
/// `stream.rs::BuildParams`). `TectonicParams::canonical()` is the only way to get today's
/// nine constants as a value, and every field names the module constant it was taken from.
///
/// **This block adds the knob. It does not change what it defaults to.**
/// `worldbuilder/terrain/tectonics.py` holds these same constants and is the oracle
/// `tests/test_conformance.py` treats as ground truth, so a changed default here is a
/// change to the reference implementation and therefore the owner's decision, not a commit.
///
/// **The trench and rift constants are deliberately absent.** `TRENCH_M`,
/// `TRENCH_WIDTH_M`, `TRENCH_OFFSET_M`, `RIFT_M` and `RIFT_WIDTH_M` are negative-going
/// sea-floor features; this block exists for the two controls the owner asked for --
/// mountain height and how many margins become ranges -- and widening it to the sea floor
/// would add surface with no request behind it. `ISLAND_ARC_OFFSET_M` is absent for the
/// same reason: it places the arc rather than sizing it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TectonicParams {
    /// `CONTINENT_COLLISION_M`: how high a continent-continent collision lifts the ground.
    pub continent_collision_m: f64,
    /// `CONTINENT_COLLISION_WIDTH_M`: how far from the margin that lift reaches zero.
    /// **Amplitude over this width is the grade**, and today's pair is 1,500 m over
    /// 400 km -- 0.375%, against 3-8% for a real range.
    pub continent_collision_width_m: f64,
    /// `COASTAL_UPLIFT_M`: the subduction-margin coastal rise.
    pub coastal_uplift_m: f64,
    /// `COASTAL_UPLIFT_WIDTH_M`: how far that rise reaches zero.
    pub coastal_uplift_width_m: f64,
    /// `ISLAND_ARC_M`: how high an oceanic-oceanic arc stands.
    pub island_arc_m: f64,
    /// `ISLAND_ARC_WIDTH_M`: how wide it is. Its *offset* from the margin stays a module
    /// constant -- that places the arc, it does not size it.
    pub island_arc_width_m: f64,
    /// `RIDGE_M`: how high a divergent margin's ridge stands.
    pub ridge_m: f64,
    /// `RIDGE_WIDTH_M`: how wide the ridge is.
    pub ridge_width_m: f64,
    /// `CONTINENTAL_BLEND`: how readily a margin counts as continental -- the width of the
    /// oceanic-to-continental transition in `continental_with`, not a threshold. This is
    /// the "how many mountains" knob: it governs how much of a margin's response is the
    /// collision profile at all.
    pub continental_blend: f64,

    // ----------------------------------------------------------------- the structure field
    //
    // **A FOURTH TECHNIQUE WAS BUILT, MEASURED AND REJECTED, AND THIS SAYS SO SO THAT
    // NOBODY RE-PROPOSES IT BLIND.** `crest_warp` warped the signed across-margin distance
    // before its absolute value was taken, bending the crest line off the plate bisector.
    // It worked -- the survey's de-trended crest-wander measurement moved monotonically
    // from 49.2 km to 78.8 km of maximum displacement across six settings -- and it bought
    // NOTHING ELSE: summit count went 2 -> 1, across-range crest count stayed at 1 at every
    // setting, the flank ratio did not move, the peak lost up to 248 m, and
    // `collision_reach_m` grew 150%, pushing configurations at the range gate. And
    // `structure_depth` displaces the crest FURTHER as a side effect (66.2 km at depth 0.5,
    // from a measurement with no unresolved stations) while also adding twelve summits. A
    // curved blade is still a blade. Full tables in task-2-report.md.
    //
    // Every field below is INERT at `canonical()`, by value and not by convention: the
    // canonical setting of each is the arithmetic identity for the expression it enters, so
    // the canonical path performs the same operations on the same numbers in the same order
    // as it did before these existed. That is what `the_structure_fields_are_inert_at_...`
    // asserts by bits, and it is why `worldbuilder/terrain/tectonics.py` did not have to
    // move.
    /// How much narrower the overriding side of a collision is than the subducting side.
    /// **One parameter, not two.**
    ///
    /// A collisional range is asymmetric by construction. Willett, Beaumont & Fullsack
    /// (1993): a wedge grown by accretion at its toe (the **pro**-wedge, on the subducting
    /// side) takes the MINIMUM taper; one grown by material carried across the singularity
    /// (the **retro**-wedge, on the overriding side) takes the MAXIMUM. That paper contains
    /// no numbers and says the problem is scale independent, so it is cited here for the
    /// mechanism only.
    ///
    /// The numbers are Naylor & Sinclair (2008), *Basin Research*, verbatim:
    /// **alpha_pro = 1.5 deg, alpha_retro = 2.5 deg**, giving at H_max = 3 km a **pro-wedge
    /// 115 km wide and a retro-wedge 69 km wide**. 2.5/1.5 = 1.67 and 115/69 = 1.67 -- the
    /// widths are the exact inverse of the tangents at equal height, so ONE ratio gives
    /// both and a second parameter would be a redundant way to disagree with the first.
    ///
    /// **The wide side keeps `continent_collision_width_m` and the narrow side is that
    /// divided by this**, rather than splitting the width about its mean. That choice is
    /// deliberate: it means raising the asymmetry can only ever make a range NARROWER, so no
    /// setting of this field can push a profile past [`MAX_TECTONIC_RANGE_M`] and turn the
    /// range gate into the cliff its own docstring exists to prevent.
    ///
    /// 1.0 is canonical and is exactly symmetric: `width / 1.0` is `width` bit-for-bit.
    /// A value at or below zero is treated as 1.0 rather than producing an infinite or
    /// negative width -- nothing clamps, but nothing divides by zero either.
    pub collision_asymmetry: f64,
    /// How many parallel sutures the collision profile stacks, at hashed inboard offsets.
    ///
    /// **The sleeper, and the only one of these that supplies structure ACROSS the range**
    /// at 50-200 km -- the axis noise does not reach and the axis a real range has. Three
    /// independent lines of evidence arrive here: reading shipped generator code; terrane
    /// accretion (over 70% of the North American Cordillera is accreted terranes); and the
    /// Himalaya carrying at least two sutures of different ages, which is why it has
    /// internal belt structure rather than one crest.
    ///
    /// Pure arithmetic -- a short sum of bumps, no new primitive. 1 is canonical, and the
    /// first suture's weight is exactly 1.0 and its offset exactly zero, so a count of 1 is
    /// the single bump this always was.
    ///
    /// **HAZARD FOR ANY BOUNDARY THAT LATER ADMITS THIS FROM OUTSIDE.** It is a loop bound.
    /// A `u32` near its maximum makes every convergent sample walk four billion iterations,
    /// which is a HANG, not a strange-looking world -- and this project has already found
    /// one ~2,600-second hang by sweeping an export's inputs and zero by spot-checking them.
    /// Nothing here clamps, per `MAX_TECTONIC_RANGE_M`'s note that validation belongs at the
    /// boundary; the survey swept 1 through 4 and nothing above 4 was measured to buy
    /// anything. `wasm.rs::decode_tectonic` deliberately does not carry this field, and says
    /// so at the line where it fills it from `canonical()` instead.
    pub suture_count: u32,
    /// How far apart the stacked sutures sit, inboard, in metres before hashing.
    ///
    /// Ignored when `suture_count` is 1. 0.0 is canonical. **A caller setting both this and
    /// a large count is responsible for the reach**: see [`TectonicParams::collision_reach_m`],
    /// which states how far the profile now carries and is what a boundary admitting these
    /// values should check against [`MAX_TECTONIC_RANGE_M`].
    pub suture_spread_m: f64,
    /// How much of the collision amplitude is handed to the structure field -- a ridged
    /// multifractal times a low-frequency segmentation field, both sampled at the point.
    ///
    /// The ridging supplies ridge-and-valley relief ALONG the belt; the segmentation is what
    /// breaks a continuous welt into separate massifs. It also matches what the field
    /// measurements say about uplift: ordinary orogens run **1-3 mm/yr** (Kishtwar ~3,
    /// Western Alps ~2.5, Southern Alps 1-8) while hotspots run **9-13 mm/yr** (Nanga Parbat
    /// 9-13, Namche Barwa ~9) **and are narrow**. A range's uplift is spiky along its
    /// length, not a smooth dome.
    ///
    /// 0.0 is canonical, and the multiplier is then exactly 1.0 without either noise field
    /// being sampled -- which is what keeps [`crate::noise::Noise::ridged`], a primitive the
    /// Python oracle does not have, off the canonical path entirely.
    pub structure_depth: f64,
    /// The wavelength of the structure field's coarsest ridge octave, in metres. Inert while
    /// `structure_depth` is 0.0; the canonical value is a placeholder that is never read on
    /// that path, and it is stated here rather than left at zero so a caller who turns the
    /// depth up gets a sane field rather than a division by nothing.
    pub structure_wavelength_m: f64,

    // ------------------------------------------------------ the along-margin warp, Task 5
    //
    // **THE STRAIGHT LINE IS A GEOMETRY FACT, NOT A TUNING PROBLEM.** The owner, looking at
    // a range this engine had just produced: *"how do we make them more random? they look
    // like they were drawn with a straight line tool."* They were.
    // `plates.rs::margin_at` computes `asin(|point . bisector_normal|) * radius`, and a
    // bisector normal defines a PLANE THROUGH THE ORIGIN -- a GREAT CIRCLE. Every margin in
    // this engine is a perfect arc, so every range built on one is dead straight by
    // construction, and no setting of any field above changes that. The owner could max
    // every control and get a more detailed straight line.
    //
    // Real plate boundaries are the one shape a great circle never is: transform faults
    // offset spreading ridges into staircases, subduction zones curve into arcs, and
    // collision belts bend around indenters.
    /// How far the collision belt is displaced sideways off the plate bisector, in metres.
    /// **The amplitude of a wander, not of a roughness.**
    ///
    /// The displacement is a function of position **ALONG** the margin only -- see
    /// [`Tectonics::margin_warp_m_at`] -- so the whole belt translates coherently at each
    /// point along its length. That is the difference between a *wandering belt* and a
    /// *noisy edge*, and it is the one thing that separates this from the version Task 2
    /// built and removed, which perturbed by an `fbm` of the 3-D point: that made the crest
    /// wander AND the across-belt profile ragged at the same time, because the noise varied
    /// in both directions at once.
    ///
    /// 0.0 is canonical and the field is then not merely multiplied by zero: `from_margin`
    /// branches on it before sampling, so the canonical path never reaches
    /// [`crate::noise::Noise::fbm`] on this field's account and hands the collision term the
    /// identical `f64` it handed it before this field existed.
    ///
    /// **A caller setting this is responsible for the reach.**
    /// [`TectonicParams::collision_reach_m`] adds this amplitude, because the profile now
    /// reaches this much further on the side the warp pushes toward, and
    /// [`MAX_TECTONIC_RANGE_M`] truncates rather than fades.
    pub margin_warp_m: f64,
    /// The wavelength of the warp's **longest** octave, in metres -- the orocline.
    ///
    /// [`MARGIN_WARP_OCTAVES`] octaves at a gain of 0.5 and a lacunarity of 2.0, so the
    /// schedule is this wavelength, half it and a quarter of it, carrying 4/7, 2/7 and 1/7
    /// of the amplitude. A real margin is ragged at every scale, and the long octave has to
    /// BE long: a wavelength near the belt's own width shreds the belt instead of bending
    /// it, which is the same failure the "function of position along the margin" form exists
    /// to avoid in the other axis.
    ///
    /// Inert while `margin_warp_m` is 0.0. The canonical value is a placeholder never read
    /// on that path, stated rather than left at zero for the reason
    /// `structure_wavelength_m`'s is.
    pub margin_warp_wavelength_m: f64,
}

/// The value of [`TectonicParams::collision_asymmetry`] that means "symmetric", and the
/// canonical setting. Named rather than written as a bare 1.0 because it is the identity
/// this block's inertness argument rests on.
pub const COLLISION_SYMMETRIC: f64 = 1.0;

/// How many octaves the structure field's ridged component takes, and its segmentation
/// component. Both swept by `src/bin/mountain_survey.rs` on this project's own worlds.
pub const STRUCTURE_OCTAVES: u32 = 4;
pub const SEGMENTATION_OCTAVES: u32 = 2;

/// How much longer the segmentation field's wavelength is than the ridge field's. The
/// segmentation is what decides where one massif ends and the next begins, so it has to be
/// coarse relative to the ridges it is gating or it just adds a second layer of ridges.
pub const SEGMENTATION_WAVELENGTH_RATIO: f64 = 6.0;

/// How many octaves the along-margin warp takes, at a gain of 0.5 and a lacunarity of 2.0.
///
/// **Three, and the schedule is the point.** The wavelengths are
/// `margin_warp_wavelength_m`, half it and a quarter of it, carrying 4/7, 2/7 and 1/7 of the
/// amplitude between them. The long octave is the orocline -- the Himalayan arc and the
/// Bolivian orocline are both bends of tens of degrees over their length -- and the two
/// short ones are the kinks a transform-offset margin has at every scale below that. Two
/// octaves gives a bend with no texture; four puts a quarter of the amplitude at a
/// wavelength approaching the belt's own width, which is where a coherent translation starts
/// to read as a torn edge instead.
pub const MARGIN_WARP_OCTAVES: u32 = 3;

/// Salts, so the four fields this module samples are independent of each other and of
/// `continentality`'s. ASCII, in the house style of `stream.rs`'s jitter salts.
pub const STRUCTURE_SALT: u64 = 0x7374_7275_6374_7572; // "structur"
pub const SEGMENTATION_SALT: u64 = 0x7365_676D_656E_7473; // "segments"
pub const MARGIN_WARP_SALT: u64 = 0x7761_6E64_6572_696E; // "wanderin"

/// The scale a 64-bit hash is divided by to land in `[0, 1)`. 2^64, exactly representable.
const HASH_SCALE: f64 = 18_446_744_073_709_551_616.0;

/// A stable fraction in `[0, 1)` for an ordered pair of plate indices and a salt.
///
/// **The ordering is the point.** A suture offset or a warp sense derived from geometry can
/// flip when the same margin is sampled from the other side of it, and a sign that flips
/// across a boundary is a cliff. Sorting the two indices before hashing means both sides of
/// one margin get the same answer by construction, not by luck.
///
/// The avalanche is `stream.rs::node_hash`'s and `noise.rs::lattice`'s, which is this
/// project's one integer-mixing shape.
fn pair_fraction(a: usize, b: usize, salt: u64) -> f64 {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    let mut h = (lo as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) // cast-ok: plate index to unsigned for hashing, no arithmetic meaning
        ^ (hi as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F) // cast-ok: plate index to unsigned for hashing, no arithmetic meaning
        ^ salt.wrapping_mul(0x1656_67B1_9E37_79F9);
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    h = h.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    h ^= h >> 33;
    h as f64 / HASH_SCALE // cast-ok: a u64 hash to f64 over an exact power of two, as `stream.rs::node_fraction` does
}

/// The smallest weight a stacked suture past the first can carry, and how much of the
/// remaining range the hash spans. Sutures of unequal size is the observed shape -- one
/// dominant belt with lesser ones beside it -- rather than a comb of identical crests.
const SUTURE_WEIGHT_FLOOR: f64 = 0.45;

/// How much the hash may stretch or squeeze a suture's nominal offset, either way. Keeps
/// the sutures ordered inboard while stopping them landing on an exact arithmetic comb.
const SUTURE_OFFSET_JITTER: f64 = 0.35;

impl TectonicParams {
    /// Exactly today's nine envelope values, each traceable to the module constant above
    /// it, plus the six structure fields at their inert settings.
    ///
    /// Building a `Tectonics` with `None` and one with `Some(TectonicParams::canonical())`
    /// must produce bit-identical output -- see `surface.rs`'s
    /// `tectonics_none_matches_tectonics_some_canonical_bit_for_bit` -- and, since Task 1
    /// found that test cannot prove the params are READ, the structure fields carry their
    /// own inertness proof in `the_structure_fields_are_inert_at_canonical_settings`.
    pub fn canonical() -> Self {
        Self {
            continent_collision_m: CONTINENT_COLLISION_M,
            continent_collision_width_m: CONTINENT_COLLISION_WIDTH_M,
            coastal_uplift_m: COASTAL_UPLIFT_M,
            coastal_uplift_width_m: COASTAL_UPLIFT_WIDTH_M,
            island_arc_m: ISLAND_ARC_M,
            island_arc_width_m: ISLAND_ARC_WIDTH_M,
            ridge_m: RIDGE_M,
            ridge_width_m: RIDGE_WIDTH_M,
            continental_blend: CONTINENTAL_BLEND,
            collision_asymmetry: COLLISION_SYMMETRIC,
            suture_count: 1,
            suture_spread_m: 0.0,
            structure_depth: 0.0,
            structure_wavelength_m: 120_000.0,
            margin_warp_m: 0.0,
            margin_warp_wavelength_m: 300_000.0,
        }
    }

    /// **A range rather than a blade.** The envelope Task 4 calibrated, carrying all three
    /// structure techniques Task 2 measured and shipped.
    ///
    /// `ReliefParams::hills()` is the pattern and its bar is the bar: **every field that
    /// moves states the ground it moved on**, and the ground is either a published figure
    /// read verbatim or a column of Task 2's own survey. A preset with no argument behind it
    /// is taste, and this generator has already carried one set of numbers nobody chose.
    ///
    /// **`canonical()` does not move and no default changes.** This is a second constructor
    /// beside it, reachable only by a caller who asks for it by name.
    ///
    /// Eight fields move. The coastal, arc and ridge profiles and `continental_blend` stay at
    /// canonical deliberately: this preset shapes the *collision* profile, which is the one
    /// Task 1's one-ULP perturbation fixtures prove is read live, and `continental_blend`
    /// decides how MANY margins become ranges, which is a different question from what a
    /// range looks like and is the owner's own slider.
    ///
    /// - **`continent_collision_m: 6_000.0` with `continent_collision_width_m: 100_000.0`.**
    ///   The far end of the two sliders Task 4 shipped, and the pair is chosen together
    ///   because the grade is the ratio of the two. `mountain_probe.rs` measured this pair at
    ///   a **7.030% flank grade** on the owner's world, and Davis, Suppe & Dahlen (1983),
    ///   *JGR* 88(B2), Table 1 gives the Himalaya at **alpha = 4.0 +- 0.5 deg** verbatim,
    ///   which is a 7.0% surface slope. The envelope is a real orogen's grade, measured --
    ///   not a large number chosen for being large.
    /// - **`collision_asymmetry: 2.0`.** Naylor & Sinclair (2008), *Basin Research*, verbatim:
    ///   a **115 km pro-wedge against a 69 km retro-wedge**, a ratio of 1.67. **The setting
    ///   that DELIVERS 1.67 on a planet is 2.0, not 1.67**, and that is Task 2's finding and
    ///   the reason this is not the published number: the profile in isolation is exactly
    ///   1.67 at a setting of 1.67 (proved by bisecting [`asymmetric_bump`] itself for its
    ///   half-height crossings), but the survey measures **1.50 on the ground** there and
    ///   **1.64 at 2.0**, because a real margin's two flanks each meet their own base. Chosen
    ///   from the measured column. The sweep is monotone in summit count, grade and flank
    ///   ratio over six settings, and costs 8 m of peak and no reach at all.
    /// - **`suture_count: 2` with `suture_spread_m: 100_000.0`.** The one setting Task 2's
    ///   sutures table found useful: **the across-range crest count doubles, 1 -> 2, and the
    ///   peak does not move at all** (4,540.5 m at 2 x 100 km and at 2 x 150 km, against
    ///   4,540.5 m at one suture). Both sides of that band are measured failures rather than
    ///   supposed ones -- tighter spreads inflate the peak, because overlapping bumps add
    ///   (**7,457.6 m at 4 x 60 km, +64%**), and wider or more numerous ones drive
    ///   [`TectonicParams::collision_reach_m`] past [`MAX_TECTONIC_RANGE_M`] and are truncated
    ///   into the cliff that constant exists to prevent (4 x 150 km reaches 707 km and
    ///   measures a 41.3% grade). Two at 100 km reaches **235 km against the 420 km gate**,
    ///   the largest margin anywhere in the useful band.
    /// - **`structure_depth: 0.7` with `structure_wavelength_m: 80_000.0`.** The largest of
    ///   the three effects: summits **2 -> 12** at 40 km and 6 at 80 km, against 2 for the
    ///   bare blade. 0.7 rather than 0.9 because 0.9 buys three more summits for another 8%
    ///   of the peak. **80 km rather than 40 km is a grade decision, and it is the one place
    ///   this preset does not take the biggest number available:** at 40 km the same depth
    ///   measures a **17.783% grade**, more than twice the steepest surface slope in this
    ///   project's literature notes (Taiwan, alpha = 2.9 +- 0.3 deg = 5.2%; the Himalaya's
    ///   4.0 deg = 7.0%), while 80 km measures **8.420%** -- just past the 3-8% band the
    ///   panel already quotes, beside the Alps. (Both figures are on the bare steep envelope,
    ///   which is the envelope Task 2's whole table is measured on; what this preset itself
    ///   delivers is the table below.) And it has to be one of those two: Task 2
    ///   measured that **120-250 km does nothing at any depth** (summit counts fall back to
    ///   0-3), so this parameter's working band is 40-80 km with no interior to interpolate.
    /// - **`margin_warp_m: 80_000.0` with `margin_warp_wavelength_m: 300_000.0`.** Task 5,
    ///   and the answer to the owner looking at the six fields above and saying *"they look
    ///   like they were drawn with a straight line tool"*. They were: every margin here is a
    ///   great circle and every belt on one is straight by construction. **Chosen against the
    ///   grade and the range gate, not against the sinuosity it maximises**, which is the
    ///   same rule Task 3 used for the asymmetry. On the bare envelope, where the belt is a
    ///   great circle and nothing else is happening, the crest's maximum lateral deviation
    ///   from its own endpoint chord goes **3.6 km -> 15.5 km** at this pair and the crest
    ///   sinuosity **1.0261 -> 1.0470**, with the envelope sinuosity moving with it
    ///   (1.0172/1.0139 -> 1.0250/1.0223) -- **the belt moves, not just the crest inside
    ///   it.** On this preset the deviation goes **21.6 km -> 44.6 km, 0.054 -> 0.124 of the
    ///   belt's length**, and the delivered grade goes **9.811% -> 9.293%**, slightly DOWN.
    ///   120 km buys more bend (0.204 of belt length) and costs a grade of 10.711% and a
    ///   reach of 355 km against the 420 km gate; 80 km reaches **315 km** and keeps the
    ///   grade below the un-warped preset's, which is why it is the one here and the 120 km
    ///   row is published beside it. The wavelength is 300 km for the reason the structure
    ///   wavelength is 80 km: **900 km is measured as doing nothing** (deviation 9.4 km
    ///   against 3.6 km unwarped, on a belt 350 km long -- a bend longer than the belt is a
    ///   tilt), and 300 km is where the measured column peaks.
    ///
    /// # What it DELIVERS, measured, because the request is not the answer
    ///
    /// `structure_at` returns a multiplier of at most 1, so `structure_depth` can only ever
    /// LOWER the peak -- 22% at depth 0.7 -- while a tight `suture_spread_m` would raise it.
    /// So this preset **asks for 6,000 m and delivers 3,323.8 m**, and that is the number the
    /// owner reads off the ground. It is inside the 1,500-6,000 m band Task 4 calibrated the
    /// height slider over, which is the check the amplitude alone cannot pass.
    ///
    /// `src/bin/mountain_survey.rs`'s "THE PRESET" row, same population/method/host as every
    /// table above it (seed 123,925,603, radius 4,500,000 m, 28 plates, land 0.16; peak over a
    /// 0.5-degree global grid refined at 0.05 degrees; grade the steepest single 2 km step on
    /// the flank over 12 bearings; summits P300 in a +/-3 degree box; native release build):
    ///
    /// | | peak | grade | summits | crests | flank ratio | reach |
    /// |---|---|---|---|---|---|---|
    /// | canonical | 1,454.0 m | 1.787% | 0 | 0 | 1.12 | 400 km |
    /// | the blade (6,000 m / 100 km) | 4,540.5 m | 7.030% | 2 | 1 | 1.08 | 100 km |
    /// | **`ranges()`** | **3,323.8 m** | **9.811%** | **8** | 1 | **1.42** | 235 km |
    ///
    /// **THREE THINGS DID NOT COMPOSE, and they are recorded rather than rounded off.** Each
    /// technique was measured alone on the steep envelope; stacked, the numbers move:
    ///
    /// - **The flank ratio is 1.42, not the 1.64 the asymmetry sweep measured at this
    ///   setting.** The structure field carves both flanks, and a carved flank's half-height
    ///   crossing sits closer in on the wide side than a smooth one's. Raising the asymmetry
    ///   to close the gap costs grade the published surface slopes do not support.
    /// - **The across-range crest count is 1, not the 2 the sutures table measured** at this
    ///   count and spread on the bare envelope. What the sutures do buy here is measured and
    ///   is not nothing: dropping to one suture takes the summit count **8 -> 6** with every
    ///   other column identical. The technique still earns its place; the column it earned it
    ///   in on the bare envelope is not the column it earns it in here.
    /// - **The delivered grade is 9.811%, above the 3-8% band this panel quotes.** The band
    ///   comes from Davis, Suppe & Dahlen's *alpha*, a wedge's mean surface taper, and this
    ///   figure is the steepest single 2 km step on a flank the structure field has
    ///   deliberately carved into ridge and valley -- the two are not the same quantity, and
    ///   the 2 km step on a ridged flank must be the larger of them. Stated rather than
    ///   tuned away: the alternative measured at 40 km wavelength reads **17.597%**, which no
    ///   reading of any published figure supports, and avoiding that is what the 80 km choice
    ///   above bought.
    ///
    /// Choosing this preset from the single-technique tables and never measuring the
    /// combination would have been choosing blind, which is Ruling 4 of this slice wearing
    /// different clothes.
    pub fn ranges() -> Self {
        Self {
            continent_collision_m: 6_000.0,
            continent_collision_width_m: 100_000.0,
            collision_asymmetry: 2.0,
            suture_count: 2,
            suture_spread_m: 100_000.0,
            structure_depth: 0.7,
            structure_wavelength_m: 80_000.0,
            margin_warp_m: 80_000.0,
            margin_warp_wavelength_m: 300_000.0,
            ..Self::canonical()
        }
    }

    /// How far from the margin the collision profile still has something to say, in metres.
    ///
    /// The furthest suture's centre plus the collision width -- the wider flank, since
    /// `collision_asymmetry` can only ever narrow the other one.
    /// wider of the two flank half-widths. **This is the number a boundary admitting a
    /// caller-chosen block must check against [`MAX_TECTONIC_RANGE_M`]**, because a profile
    /// that has not reached zero by the gate is truncated there rather than faded, which is
    /// exactly the cliff that constant exists to prevent. Nothing here clamps -- validation
    /// belongs at the boundary that admits a block, per `MAX_TECTONIC_RANGE_M`'s own note.
    ///
    /// At `canonical()` this is `continent_collision_width_m` exactly: one suture at offset
    /// zero, symmetric flanks, and no warp.
    ///
    /// **`margin_warp_m` is added, and that is Task 5's change to this function.** The warp
    /// displaces the whole collision profile sideways off the bisector by up to that
    /// amplitude -- `Noise::fbm` is bounded to `[-1, 1]` by construction, since every lattice
    /// sample is in `[0, 1]` and the sum is divided by the summed amplitude -- so on the side
    /// the warp pushes toward the profile carries weight exactly that much further out than
    /// it did. Leaving it out would report a reach a warped range does not have, and the
    /// range gate would then truncate the far flank into the cliff this function exists to
    /// let a boundary refuse. Task 2's own removed version measured the reach growing 150%
    /// for the same reason.
    pub fn collision_reach_m(&self) -> f64 {
        // `abs`, not a `> 0.0` branch. A negative amplitude is a mirrored warp of the same
        // magnitude and reaches exactly as far, so a branch that floored it at zero would
        // report a reach the profile does not have -- the same "unreported reach" shape a
        // negative `suture_spread_m` has. And `abs` keeps a NaN a NaN, so the boundary's
        // `within` refuses it rather than a branch quietly turning it into a legal zero.
        let warp = self.margin_warp_m.abs();
        let furthest = if self.suture_count > 1 {
            let last = f64::from(self.suture_count - 1); // cast-ok: a small count to float, exact
            let stretched = self.suture_spread_m * (1.0 + SUTURE_OFFSET_JITTER);
            let reach = last * stretched;
            if reach > 0.0 {
                reach
            } else {
                0.0
            }
        } else {
            0.0
        };
        furthest + self.continent_collision_width_m + warp
    }
}

/// Where a seamount stands, hashed per lattice node.
///
/// Distinct from `STRUCTURE_SALT`, `SEGMENTATION_SALT` and `MARGIN_WARP_SALT`, this file's
/// own three, and it has to be: a shared salt would put every island on a margin crest,
/// which is the one place this term is not meant to put them. Also distinct from the
/// cross-module salts these three fields could otherwise echo --
/// `continentality::NOISE_SALT` and `continentality::COAST_NOISE_SALT`, and `detail.rs`'s
/// `0x5EABED`, `0x6011E1` and `0x6011E2` -- named here the way `continentality.rs:41-47`
/// names the salts a new one must differ from, rather than only the ones in this file.
/// Three salts rather than one because existence, position and height must be independent
/// -- drawn from a single field, a tall peak would always sit in the same corner of its
/// cell. [`the_six_salts_in_this_file_are_pairwise_distinct`] guards the six named in this
/// file so a future copy-paste collision fails a test rather than silently correlating two
/// fields.
const PEAK_SALT: u64 = 0x7365_616D_6F75_6E74; // "seamount"
const PEAK_JITTER_SALT: u64 = 0x6A69_7474_6572_6564; // "jittered"
const PEAK_HEIGHT_SALT: u64 = 0x7374_616E_6469_6E67; // "standing"

/// How tall a full-height peak stands, in metres above the seabed it sits on.
///
/// **Confirmed by Task 7's survey and left where Task 2 put it; see [`VOLCANIC_DENSITY`]'s doc
/// for why THIS is the binding constraint, not `reach_m` or `density`, and for the sweep that
/// then chose the density at this height.** At 5,200 m, a node standing exactly
/// on the reference shell clears a 4,600 m abyss by only 600 m -- 88.46% of `height_m` --
/// which the `0.45 + 0.55 * share` envelope only reaches for `share > 0.79`, so barely a
/// fifth of nodes could ever surface at all, and those just past the threshold make
/// vanishingly small islands. At 8,000 m the same abyss needs only `share > 0.0538`, so most
/// nodes (about 77%) can surface, and a full-height summit stands roughly 3,400 m above sea
/// level -- Tenerife is 3,715 m, Reunion 3,070 m, so this is an ordinary volcanic island's
/// scale, not a fitted number chosen to hit a target.
const VOLCANIC_HEIGHT_M: f64 = 8_000.0;
/// What share of lattice cells hold a peak at the named preset.
///
/// **Calibrated by `src/bin/island_survey.rs`, which is the only thing that may change it** -- the survey and the constant must
/// not be allowed to drift apart, the way `CoastParams::fractal()`'s 0.35 and
/// `coastline_survey.rs` must not. Spec §7 question 1 asks for **0.3% to 0.8%** of a planet's
/// surface as islands.
///
/// **What the survey measured** (`cargo run --release --bin island_survey`, section 4, over a
/// 200,000-point Fibonacci spiral per world, `D_added` = ground the block turned to land where
/// the peak-less world had sea, through the full `Surface` pipeline; see
/// `docs/superpowers/reports/2026-09-14-islands-1-peaks-verification.md` for host and rustc):
///
/// **RE-SURVEYED. This value was 0.36, and 0.36 was measured against a field that was
/// suppressed over 77% of the planet.** `Tectonics::offset_m` used to end
/// `margin_offset_m`, which returns early wherever no plate margin is in range -- so the
/// seamount term was never evaluated on most of the world, and every share below was a share
/// of the quarter of the planet where it was. That is fixed (see `Tectonics::offset_m`'s own
/// doc and `the_seamount_term_is_reachable_everywhere_no_matter_where_the_margins_fall`), the
/// field now stands islands everywhere the seabed allows, and the whole sweep was re-run.
///
/// **The correction is 3.41x on the share, not the 4.4x the area change alone suggests.**
/// Measured, not scaled: at the old 0.36 this fixture now reads 1.1400% against the 0.3345% it
/// read before, which is 3.41x. The area the field can stand on grew by about 4.4x, but the
/// ocean coverage and the depth window do not fall uniformly with respect to where margins
/// are, so the two factors are not the same number -- which is exactly why this was re-measured
/// rather than divided.
///
/// | density | island-a (land 0.40) | owner (land 0.16) | earth-a (land 0.29) | margin to the nearer band edge |
/// |---|---|---|---|---|
/// | 0.08 | 0.2665% | 0.3640% | 0.3090% | -0.0335 pp, one BELOW |
/// | 0.09 | 0.3030% | 0.4105% | 0.3430% | +0.0030 pp |
/// | 0.10 | 0.3350% | 0.4590% | 0.3810% | +0.0350 pp |
/// | 0.11 | 0.3650% | 0.5000% | 0.4140% | +0.0650 pp |
/// | 0.12 | 0.3935% | 0.5465% | 0.4495% | +0.0935 pp |
/// | 0.13 | 0.4180% | 0.5915% | 0.4785% | +0.1180 pp |
/// | **0.14** | **0.4480%** | **0.6380%** | **0.5210%** | **+0.1480 pp -- the maximin** |
/// | 0.15 | 0.4795% | 0.6755% | 0.5575% | +0.1245 pp |
/// | 0.16 | 0.5050% | 0.7175% | 0.5940% | +0.0825 pp |
/// | 0.17 | 0.5355% | 0.7625% | 0.6380% | +0.0375 pp |
/// | 0.18 | 0.5640% | 0.8055% | 0.6810% | -0.0055 pp, one ABOVE |
/// | 0.19 | 0.6045% | 0.8540% | 0.7235% | -0.0540 pp, one ABOVE |
/// | 0.20 | 0.6355% | 0.8990% | 0.7615% | -0.0990 pp, one ABOVE |
/// | 0.36 (the suppressed-field pick) | 1.1400% | 1.6180% | 1.3945% | -0.8180 pp, one ABOVE |
///
/// **The islanded share depends on the world's land fraction, so the choice cannot be made on
/// one world.** A world with less land has more deep ocean for the field to stand an island
/// in: at every density the owner's 0.16-land world yields roughly 1.4x the share the
/// 0.40-land fixture does. **Nine admissible hundredths -- 0.09 through 0.17, so eight
/// hundredths of span** -- put all three worlds inside the band at once, and **0.14 is the
/// maximin**: the admissible density whose WORST world sits furthest from a band edge.
///
/// **Both edges are measured, not inferred** -- the 0.08 and 0.18 rows are in the table rather
/// than a gap either side of it. 0.08 misses the floor by 0.0335 pp and 0.18 clears the ceiling
/// by 0.0055 pp, so the nine are exactly nine. (The final whole-branch review's minor 6 found
/// this doc saying "six hundredths wide" while the verification report said "seven hundredths
/// wide" -- two true statements about different quantities, each written as if it were the
/// other. Both now say both, in the corrected units.)
///
/// **The fix widened the safety margin as well as moving the value, which is the useful part.**
/// At 0.36 the maximin margin was +0.0345 pp against a 1-sigma binomial error of about
/// 0.0158 pp -- a little over 2 sigma, and the report flagged that as its first concern. At
/// 0.14 the margin is **+0.1480 pp against about 0.0150 pp**, close to **10 sigma**, and the
/// binding world flips from the floor to the ceiling between 0.14 and 0.15 rather than sitting
/// on top of one edge. `island-a` clears the floor by 0.1480 pp and `owner` clears the ceiling
/// by 0.1620 pp. A fourth world is no longer likely to push an end out of band.
///
/// Hundredths, because `viewer/public/app/peak-params.js`'s density slider carries an integer
/// position and maps it to a value by dividing by 100; a density off that lattice is one the
/// panel cannot reach, which is the defect `panelFieldFaults()` exists for. 0.14 is position
/// 14. It is also **not** `CoastParams::fractal()`'s 0.35, which matters for a reason that is
/// not cosmetic: `viewer/test/peak-params.test.mjs`'s "no peak number is written down twice"
/// scans the viewer's sources for the preset's own distinctive literal, and a density that
/// collided with another channel's would have made that scan pass vacuously. Checked against
/// the four modules that scan covers: `0.14` appears in none of them.
///
/// **The analytic model is not where this came from.** The corrected volumetric model in
/// [`VOLCANIC_REACH_M`]'s doc predicts the FIELD -- what the term would make if the whole
/// planet were abyssal ocean, land included -- not a world's islanded share; the two differ by
/// the ocean-coverage and depth-window factors `island_survey.rs`'s header sets out. The model
/// chose where to sample. The sweep chose the number.
const VOLCANIC_DENSITY: f64 = 0.14;
/// How far a cone reaches from its centre, in metres.
///
/// Must not exceed [`VOLCANIC_LATTICE_M`] -- see `peak_offset_m`'s own doc for why that is
/// the invariant that makes its 3x3x3 scan complete rather than merely adequate.
///
/// **Swept by Task 7 and left where it was, and the survey's finding is worth more than the
/// value: the islanded share is invariant under the PAIR, so `reach_m / lattice_m` is the
/// lever and neither field alone is one.** Holding the ratio at 0.70 and moving the pair over
/// 30 / 45 / 67.5 / 90 km moved the share by less than the spiral's own noise -- measured on
/// `island-a` over 200,000 points, `island_survey.rs` section 3. **Re-measured after the
/// margin-suppression fix and after `density` was re-surveyed to 0.14**: at the shipped
/// density, **0.4305% / 0.4480% / 0.4365% / 0.4475%** -- a spread of 0.0175 pp against the
/// estimator's own 1-sigma binomial error of about 0.0150 pp, so still about one sigma and
/// still the honest word for it is "unmeasurable". At density 0.11: 0.3385% / 0.3650% /
/// 0.3530% / 0.3460%. At density 0.58: 1.8170% / 1.8410% / 1.8380% / 1.8030%. (Before the fix
/// the same sweep at 0.36 read 0.3290% / 0.3345% / 0.3350% / 0.3310%, a 0.006 pp spread; the
/// spread grew with the share, as a binomial spread does, and not relative to it.)
///
/// That is what the model below predicts, now measured rather than supposed: `d(share)` scales
/// with `reach_m`, so `d^3 / lattice_m^3` is scale-free. The pair therefore sets island SIZE
/// and COUNT, not islanded AREA -- and `island_survey.rs` section 6 measures that other half
/// on the same world, over a 5 km raster at the shipped density. **Re-run after the
/// margin-suppression fix and the re-survey**: **13,506 islands of mean 166.9 km2 at 30 km,
/// 6,137 of mean 363.6 km2 at 45 km, 1,558 of mean 1,415.9 km2 at 90 km**, with total island
/// area 0.4419% / 0.4375% / 0.4325% of the sphere. Eightfold in count, eightfold in mean size,
/// and the area does not move. (Before the fix, at 0.36 on a suppressed field, the same three
/// pitches read 10,155 / 4,617 / 1,213 islands and 0.3323% / 0.3337% / 0.3222% of area. The
/// mean sizes are unchanged to within the raster, which is the point: the fix and the re-survey
/// moved how MANY islands there are, not how big one is.) That is why calibration moved `density`
/// and left both of these alone. [`VOLCANIC_HEIGHT_M`] still binds first; read that doc before
/// this one. Clearing the abyss
/// needs `(0.45 + 0.55*share) * smooth(1 - fraction) > 4600/height_m`, so even at `share = 1`
/// the fraction must be below `1 - smooth^-1(4600/height_m)` -- **0.2116** at the old
/// `height_m` of 5,200, the useful radius a node's cone can ever have. The naive model built
/// from that alone (`density * (4/3)*pi*(0.2116*reach_m)^3 / lattice_m^3`) is NOT an
/// approximation of the true yield; it OVERSTATES it by about 12x, because it silently
/// assumes every node can reach `share = 1`'s ceiling, when at `height_m: 5,200` only nodes
/// with `share > 0.79` can surface AT ALL -- the share-weighted mean of `d(share)^3` (see the
/// corrected model below) is only 8.49% of the naive `d(1)^3`. Measured rather than trusted:
/// this generator's own live field found **0.035%** against the naive model's ~0.41% at
/// `height_m: 5,200`, `reach_m` at this value and `density` at 0.30 -- three significant
/// figures away from what the CORRECTED model predicts for that same trio (0.0347%), and the
/// naive model's 12x error accounts for essentially all of the gap.
///
/// **The corrected model**, for Task 7 to start a sweep from and then verify by measurement,
/// never by trusting it outright:
///
/// ```text
/// island area = density * (4/3)*pi * <d(share)^3> / lattice_m^3
///   where d(share) = reach_m * (1 - smooth^-1( (4600/height_m) / (0.45 + 0.55*share) ))
///   and d(share) = 0 for any share where 0.45 + 0.55*share <= 4600/height_m
/// ```
///
/// `<d(share)^3>` is the AVERAGE of `d(share)^3` over `share` uniform on `[0, 1)`, not
/// `d(1)^3` alone -- that average is what the naive model above conflates with the ceiling
/// case, and the whole reason it overstates. At `height_m: 8,000`, `reach_m` at this value:
/// **1.372%** at density 1.0 (77% of nodes can surface, against 21% at 5,200 m) and **~0.50%**
/// at `density: 0.11`, which is why `VOLCANIC_HEIGHT_M` moved rather than either knob here.
/// `reach_m <= lattice_m` holds comfortably at this ratio (0.70) either way.
const VOLCANIC_REACH_M: f64 = 31_500.0;
/// How deep the seabed must be under a peak, in metres below datum.
const VOLCANIC_MIN_DEPTH_M: f64 = 2_500.0;
/// How far apart the candidate nodes are, in metres.
///
/// **Swept by Task 7 and left where it was.** See [`VOLCANIC_REACH_M`]'s doc for the measured
/// reason -- the islanded share is invariant under `(lattice_m, reach_m)` at a fixed ratio, so
/// this field sets island size and count rather than islanded area, and calibration moved
/// `density` instead. [`VOLCANIC_HEIGHT_M`]'s doc says why height binds before either.
/// `island_survey.rs` section 6 reports the island count and the area distribution this value
/// produces, at three pitches, so what it DOES control is on the record too: at this 45 km
/// pitch, **6,137 distinct islands** on `island-a`, mean 363.6 km2, median 327.6 km2, largest
/// 1,732.5 km2 and smallest 7.0 km2 over a 5 km raster -- re-measured after the
/// margin-suppression fix and the density re-survey (it read 4,617 of mean 368.7 km2 before).
const VOLCANIC_LATTICE_M: f64 = 45_000.0;

/// Sparse volcanic peaks rising out of deep ocean.
///
/// **A seamount now stands anywhere in three dimensions, not glued to the reference
/// shell.** Each candidate's node is a jittered lattice point in the same scaled space a
/// query point is measured in, and that node's own distance from the sphere is real: a node
/// that lands slightly outward or inward of the shell is, respectively, a taller island or
/// one that never quite reaches the target depth -- a shoal standing just under the surface.
/// That is not a shortcoming of the geometry; it is the one thing this generator could not
/// produce before this field existed. A submerged shoal is exactly the navigational hazard a
/// real chart carries and open ocean here has never been able to hide one.
///
/// Field order will matter once Tasks 4 and 6 give this struct an ABI -- `wasm.rs` encoding
/// it and `viewer/public/app/peak-params.js` mirroring the encoding, the way `TectonicParams`
/// and `CoastParams` already have. Neither exists yet, so nothing depends on this order
/// today; the fields are kept in a stable, sensible order now so that later wiring does not
/// have to choose one under pressure.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PeakParams {
    /// How tall a full-height peak stands above its seabed, in metres. Must clear the
    /// abyss -- `ABYSS_M` is -4,600, and a 700 m term surfaces nothing, which is exactly
    /// why the island arc never made an island. This is the ceiling a node exactly on the
    /// reference shell reaches; a radially displaced node reaches less.
    pub height_m: f64,
    /// What share of candidate nodes hold a peak, 0 to 1. **This is the opt-in field:** at
    /// zero the term returns before it touches the lattice.
    pub density: f64,
    /// How far a cone reaches from its centre, in metres, measured in real 3-D space rather
    /// than after projecting the candidate onto the sphere. **Must not exceed `lattice_m`**
    /// -- `peak_offset_m` guards this and returns zero rather than scan an incomplete
    /// neighbourhood if it is violated.
    pub reach_m: f64,
    /// How deep the seabed must be for a peak to stand on it, in metres below datum and
    /// stated positive. Keeps islands off the continental shelf.
    pub min_depth_m: f64,
    /// How far apart the candidate nodes are, in metres. With `density`, this sets how many
    /// islands a world gets; `height_m` sets how tall they are. The two are independent,
    /// which is the whole reason this term is built on a lattice rather than a threshold.
    pub lattice_m: f64,
}

impl PeakParams {
    /// Inert. Opting in is one field -- `density` -- rather than five.
    pub fn canonical() -> Self {
        Self {
            height_m: VOLCANIC_HEIGHT_M,
            density: 0.0,
            reach_m: VOLCANIC_REACH_M,
            min_depth_m: VOLCANIC_MIN_DEPTH_M,
            lattice_m: VOLCANIC_LATTICE_M,
        }
    }

    /// Islands, at the density the survey settled on.
    pub fn volcanic() -> Self {
        Self { density: VOLCANIC_DENSITY, ..Self::canonical() }
    }
}

/// How much of a peak stands, given the seabed under it.
///
/// One at the stated depth and below, ramping to zero a quarter again shallower, so the
/// term has no cliff in it. **A NaN closes this window rather than opening it**, which is
/// why the branches are written out: `smooth(NaN)` is 1.0, and a floored metre is
/// indistinguishable from a real one once it is added to an elevation.
fn peak_depth_window(depth_m: f64, min_depth_m: f64) -> f64 {
    let onset = min_depth_m * 0.8;
    let span = min_depth_m - onset;
    if !(span > 0.0) {
        // A zero or negative span, or a NaN threshold. No window.
        return 0.0;
    }
    if depth_m >= min_depth_m {
        1.0
    } else if depth_m > onset {
        let x = (depth_m - onset) / span;
        x * x * (3.0 - 2.0 * x)
    } else {
        // Shallower than the onset, or unanswerable.
        0.0
    }
}

/// Whether a peak block can raise ground **anywhere on any world**, decided from the block
/// alone.
///
/// `Tectonics::with_peaks` stores `None` for a block this answers `false` for, which is what
/// makes `peak_offset_m`'s doc claim -- "a term that is inert ... costs one comparison and no
/// hashing" -- true of every inert block rather than of the zero-density one only. Before the
/// final whole-branch review, four of these five reasons still reached
/// `Continentality::base_elevation` (an fBm) on every sample.
///
/// **Every predicate here is one `peak_offset_m` already applies, written the same way round.**
/// That is the whole safety argument: a block this rejects is a block `peak_offset_m` would
/// return exactly `0.0` for at every point, so deciding it once changes nothing but the cost.
/// The negated comparisons (`!(x > 0.0)` rather than `x <= 0.0`) are kept because a NaN must
/// make a block inert, not live -- the same reason `peak_depth_window` above spells its branches
/// out. `height_m == 0.0` is true of `-0.0` as well, which is deliberate: a `-0.0` height makes
/// every candidate's `standing` a `-0.0` that `peak_offset_m`'s `standing > tallest` never
/// takes, so the term answers `0.0` there too.
///
/// `min_depth_m` is **not** here even though `peak_depth_window`'s `span > 0.0` test is also
/// point-independent. That test lives in one place, inside the window, and copying its arithmetic
/// (`min_depth_m * 0.8`, then a subtraction) into a second place to save a comparison is how two
/// copies of a threshold start to disagree. The four fields that gate before the window is even
/// reached are the ones worth hoisting.
fn peak_block_is_live(params: &PeakParams) -> bool {
    if params.density == 0.0 {
        return false;
    }
    if params.height_m == 0.0 || !params.height_m.is_finite() {
        return false;
    }
    if !(params.lattice_m > 0.0) || !(params.reach_m > 0.0) {
        return false;
    }
    if !(params.reach_m <= params.lattice_m) {
        return false;
    }
    true
}

/// One lattice cell's candidate peak, as [`Tectonics::peak_of_cell`] computes it.
///
/// `node` is the cell's own jittered position, in the scaled space `peak_offset_m`'s query
/// point is measured in -- un-normalised, so its own distance from the origin is real. `share`
/// is the cell's height draw, in `[0, 1)`. `min_dist_m` is the real distance, in metres, from
/// `node` to the one point on the sphere closest to it -- already computed by `peak_of_cell`
/// to decide whether this candidate can ever be reached at all, and carried here so a caller
/// (a test, most often) that wants it does not recompute the same closest-point arithmetic a
/// second time. **Deliberately does NOT carry a summit direction or a precomputed crest.**
/// Both are cheap to recover from `node` and `min_dist_m` when something actually needs them
/// (see the test module's `summit_and_crest`), and `peak_offset_m`'s hot loop never does --
/// an earlier version stored them anyway, which cost every live sample a wasted `smooth` call
/// and left `summit`/`crest_m` flagged `dead_code` in a non-test build, for a value nothing
/// outside a test ever read.
#[derive(Debug, Clone, Copy)]
struct PeakCandidate {
    node: Vec3,
    share: f64,
    // Computed unconditionally in `peak_of_cell` regardless (it decides that function's own
    // `None` case), so storing it costs nothing extra on the hot path -- unlike `crest_m`
    // and `summit` before this round, which cost a `smooth` call apiece for a value only a
    // test ever read. This field IS only ever read by a test (see `summit_and_crest` and the
    // ring test), so it is dead code in a non-test build by the same measure; `allow`d rather
    // than left to warn, since the field is intentional and the review that asked for it
    // knew that.
    #[cfg_attr(not(test), allow(dead_code))]
    min_dist_m: f64,
}

/// Nothing for thoroughly oceanic, one for thoroughly continental, and a smooth ramp
/// between.
///
/// Args:
/// value: Continentality on one side of a margin.
pub(crate) fn continental(value: f64) -> f64 {
    continental_with(value, CONTINENTAL_BLEND)
}

/// The same ramp, with the transition width supplied rather than taken from the module
/// constant.
///
/// Split out so `TectonicParams::continental_blend` can reach it without changing
/// `continental`'s signature -- `bindings.rs::tectonics_continental` is a conformance
/// binding whose Python counterpart takes one argument, and the oracle does not move.
/// `continental(v)` is exactly `continental_with(v, CONTINENTAL_BLEND)`, the same four
/// operations in the same order, so the canonical path is unchanged bit-for-bit.
///
/// Args:
/// value: Continentality on one side of a margin.
/// blend: How wide the oceanic-to-continental transition is.
pub(crate) fn continental_with(value: f64, blend: f64) -> f64 {
    let fraction = (value - CONTINENTAL_ENOUGH) / blend * 0.5 + 0.5;
    // Python writes `max(0.0, min(1.0, fraction))`; the two-argument forms are asymmetric
    // under NaN, keeping the first operand unless the second is strictly beyond it. So
    // `min(1.0, fraction)` keeps 1.0 unless `fraction` is strictly less than it, and
    // `max(0.0, ...)` keeps 0.0 unless its argument is strictly greater than it.
    let fraction = if fraction < 1.0 { fraction } else { 1.0 };
    let fraction = if fraction > 0.0 { fraction } else { 0.0 };
    fraction * fraction * (3.0 - 2.0 * fraction)
}

/// A smooth hump: one at the centre, nothing at the edge, and no corner anywhere.
///
/// Args:
/// distance_m: How far from the middle of the feature.
/// width_m: Where it reaches zero.
///
/// Returns a weight between zero and one.
///
/// Notes:
/// Smoothstep rather than a cosine or a straight taper, because it is flat at both
/// ends: the derivative is zero at the centre *and* at the edge. A profile that
/// merely reached zero would still leave a crease where it met the untouched ground,
/// and a crease in terrain is a cliff somebody sails into.
pub(crate) fn bump(distance_m: f64, width_m: f64) -> f64 {
    if width_m <= 0.0 {
        return 0.0;
    }
    let raw = distance_m.abs() / width_m;
    // Python writes `min(1.0, abs(distance_m) / width_m)`; keep the second operand unless
    // it is not strictly less than the first, matching the house form in
    // `plates.rs::margin_at`.
    let away = if raw < 1.0 { raw } else { 1.0 };
    let fade = 1.0 - away;
    fade * fade * (3.0 - 2.0 * fade)
}

/// The same hump with two different half-widths meeting at the crest: `width_m` on the
/// subducting side and `width_m / asymmetry` on the overriding one.
///
/// **A doubly-vergent wedge, which is what a collisional range actually is.** See
/// [`TectonicParams::collision_asymmetry`] for the mechanism and the published angles.
///
/// Positive `distance_m` is the overriding side, because `from_margin` evaluates the
/// profile at `+distance_m` weighted by `toward` -- and `toward` goes to one where the
/// INBOARD side is the more continental, which is the overriding plate. So the narrow,
/// steep retro-wedge lands on the side the geology puts it.
///
/// Continuous, and smoothly so: both branches are [`bump`], which is one at zero and flat
/// there, so the two half-profiles meet at the crest with matching value and matching
/// (zero) derivative. There is no crease at the join, which is the whole reason the
/// canonical `bump` is a smoothstep rather than a cosine.
///
/// Args:
/// distance_m: Signed across-margin distance; positive is the overriding side.
/// width_m: The subducting side's half-width -- the wider one.
/// asymmetry: How many times narrower the overriding side is. At or below zero is treated
/// as symmetric rather than producing an infinite or negative width.
pub(crate) fn asymmetric_bump(distance_m: f64, width_m: f64, asymmetry: f64) -> f64 {
    // `width_m / 1.0` is `width_m` bit-for-bit, so the canonical setting is not merely
    // close to `bump(distance_m, width_m)` -- it is the identical expression.
    let width = if distance_m > 0.0 && asymmetry > 0.0 {
        width_m / asymmetry
    } else {
        width_m
    };
    bump(distance_m, width)
}

/// What kind of ground lies either side of a margin, here.
///
/// Attributes:
/// inboard: Continentality on the nearest plate's side.
/// outboard: Continentality on the neighbour's side.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Setting {
    pub inboard: f64,
    pub outboard: f64,
}

impl Setting {
    /// How continental the near side is, from nothing to one, smoothly, at the canonical
    /// transition width.
    pub fn inboard_continental(&self) -> f64 {
        self.inboard_continental_with(CONTINENTAL_BLEND)
    }

    pub fn outboard_continental(&self) -> f64 {
        self.outboard_continental_with(CONTINENTAL_BLEND)
    }

    /// The same, at a caller-chosen transition width -- what `from_margin` uses so that
    /// `TectonicParams::continental_blend` reaches the profile mix. The two no-argument
    /// forms above delegate here with `CONTINENTAL_BLEND`, so there is one ramp, not two.
    pub fn inboard_continental_with(&self, blend: f64) -> f64 {
        continental_with(self.inboard, blend)
    }

    pub fn outboard_continental_with(&self, blend: f64) -> f64 {
        continental_with(self.outboard, blend)
    }

    /// Which side is the more continental, from -1 to +1, and how decidedly.
    ///
    /// Notes:
    /// Near zero where the two sides are alike, which is what lets an asymmetric
    /// profile fade out rather than flip. A hard comparison here would have put a
    /// trench on one side of a symmetric margin and the other side of it a metre
    /// away.
    pub fn lean(&self) -> f64 {
        m::tanh((self.inboard - self.outboard) * SIDE_SHARPNESS)
    }
}

/// The tectonic contribution to elevation, worked out where it matters and nowhere else.
///
/// Notes:
/// Holds the plates and the continentality field and combines them. It is the first
/// thing in the engine that knows about both, which is deliberate - they were built
/// in ignorance of each other so that continents would not inherit plate shapes, and
/// this is the seam where they are allowed to meet.
#[derive(Clone)]
pub struct Tectonics {
    plates: PlateSet,
    land: Continentality,
    radius_m: f64,
    params: TectonicParams,
    /// The two fields the opt-in structure work samples, built once here so no sample
    /// pays a constructor. **Built unconditionally and sampled conditionally**: constructing
    /// a `Noise` is two multiplies and an XOR, and making the FIELDS optional would put an
    /// `Option` test in the hot path for a saving smaller than the test.
    ///
    /// Salted apart from each other and from `continentality`'s `NOISE_SALT`, so the ridges
    /// and the segmentation are two independent fields on one world rather than one field at
    /// two amplitudes.
    structure: Noise,
    segmentation: Noise,
    /// The along-margin warp's field. Salted apart from the other two for the same reason
    /// they are salted apart from each other, and sampled at a point on the margin's own
    /// great circle rather than at the query point -- see [`Tectonics::margin_warp_m_at`].
    warp: Noise,
    /// The opt-in seamount field. `None` is canonical and is what every existing caller
    /// gets; see [`PeakParams`]. Read by [`Tectonics::offset_m`], which is the only place
    /// it is read.
    peaks: Option<PeakParams>,
    /// The three lattices [`Tectonics::peak_offset_m`] draws from: existence, jitter and
    /// height. Built unconditionally, for the same reason `structure`/`segmentation`/`warp`
    /// are -- a `Noise` is two multiplies and an XOR -- and read only when `peaks` is `Some`
    /// with a non-zero density, so their existence cannot move a canonical world.
    peak_noise: Noise,
    peak_jitter: Noise,
    peak_height: Noise,
}

impl Tectonics {
    /// `params`: `None` for canonical -- today's nine constants, byte-for-byte what
    /// `TectonicParams::canonical()` returns -- or `Some(params)` for a caller-chosen
    /// block. Resolved once here rather than re-checked per sample, so `from_margin` never
    /// sees the `Option` at all, exactly as `Detail::new` resolves `ReliefParams`.
    ///
    /// Delegates to [`Tectonics::with_peaks`] with `None`, exactly as `Continentality::new`
    /// delegates to `with_coast`: a new constructor absorbs the opt-in block rather than
    /// this one growing a fifth parameter, so every one of this signature's existing call
    /// sites is untouched.
    pub fn new(
        plates: PlateSet,
        land: Continentality,
        radius_m: f64,
        params: Option<TectonicParams>,
    ) -> Self {
        Self::with_peaks(plates, land, radius_m, params, None)
    }

    /// The same tectonics, with an opt-in field of standalone seamounts.
    ///
    /// `peaks`: `None` for today's ground, byte-for-byte -- or `Some(params)` for a
    /// caller-chosen [`PeakParams`]. A second constructor rather than a widened `new`, for
    /// the reason `Continentality::with_coast` gives for being one: what Ruling 1 requires
    /// is a property of the parameter, not of where it is spelled, and `new` has ninety-odd
    /// call sites that want none of this.
    ///
    /// `peak_offset_m` is wired into [`Tectonics::offset_m`] (and therefore into
    /// [`Tectonics::elevation_m`]), gated on this field: a stored `None` never calls it or
    /// `base_elevation`, so an absent block costs nothing and moves nothing.
    ///
    /// **An inert block is STORED as `None`, so inertness is decided once per world rather
    /// than once per sample.** See `peak_block_is_live`: `peak_offset_m` has five
    /// point-independent reasons to answer exactly zero everywhere on the planet, and the
    /// final whole-branch review found that `offset_m` gated on only one of them (zero
    /// density), so a block inert for any of the other four still paid
    /// `Continentality::base_elevation` -- a full fBm -- at every sample, only for
    /// `peak_offset_m` to return 0.0 a few lines later. The hydrology bake asks `structural_m`
    /// at about a million nodes, so that was real. The normalisation cannot change an answer:
    /// the predicates are exactly the ones `peak_offset_m` already applied, in the same
    /// NaN-preserving negated form, and each of them made the term return 0.0, which
    /// `offset_m`'s `standing > 0.0` guard already turned back into the untouched `total`.
    /// `the_inert_peak_path_does_not_read_base_elevation` pins all five.
    pub fn with_peaks(
        plates: PlateSet,
        land: Continentality,
        radius_m: f64,
        params: Option<TectonicParams>,
        peaks: Option<PeakParams>,
    ) -> Self {
        let params = params.unwrap_or_else(TectonicParams::canonical);
        // `Continentality` kept the world seed for exactly this -- see its `world_seed`
        // field. Nothing else in the engine gives `Tectonics` a seed, and adding one to
        // this signature would have moved six call sites including two conformance
        // bindings for a value the layer next door already holds.
        let world_seed = land.world_seed();
        let structure = Noise::new(world_seed, STRUCTURE_SALT);
        let segmentation = Noise::new(world_seed, SEGMENTATION_SALT);
        let warp = Noise::new(world_seed, MARGIN_WARP_SALT);
        let peak_noise = Noise::new(world_seed, PEAK_SALT);
        let peak_jitter = Noise::new(world_seed, PEAK_JITTER_SALT);
        let peak_height = Noise::new(world_seed, PEAK_HEIGHT_SALT);
        // Inertness, decided here and not per sample. See this function's own doc.
        let peaks = match peaks {
            Some(params) if peak_block_is_live(&params) => Some(params),
            _ => None,
        };
        Self {
            plates,
            land,
            radius_m,
            params,
            structure,
            segmentation,
            warp,
            peaks,
            peak_noise,
            peak_jitter,
            peak_height,
        }
    }

    /// What this world's uplift profiles are set to. Read-only: nothing writes these after
    /// construction, so no caller can make two samples of one world disagree.
    pub fn params(&self) -> &TectonicParams {
        &self.params
    }

    /// What lies either side of the margin near this point.
    ///
    /// Args:
    /// point: Where.
    /// distance_m: How far the margin is.
    /// normal: Away from the margin, into the nearest plate.
    ///
    /// Returns the continentality on each side.
    ///
    /// Notes:
    /// The probes are placed relative to the *margin*, not to the point, so that two
    /// samples on opposite sides of the same boundary describe the same stretch of it
    /// and agree about what it is. Probing outward from each point instead would have
    /// let a margin be a subduction zone from one side and a collision from the other.
    pub fn setting_at(&self, point: &SpherePoint, distance_m: f64, normal: &Vec3) -> Setting {
        let frame = TangentFrame::at(point, self.radius_m);
        let east = normal.dot(&frame.east);
        let north = normal.dot(&frame.north);

        // Walk back to the margin, then out to either side of it.
        let to_inboard = -distance_m + PROBE_M;
        let to_outboard = -distance_m - PROBE_M;
        Setting {
            inboard: self.land.at(&frame.local_to_sphere(east * to_inboard, north * to_inboard)),
            outboard: self
                .land
                .at(&frame.local_to_sphere(east * to_outboard, north * to_outboard)),
        }
    }

    /// How much the plates raise or lower the ground here.
    ///
    /// Args:
    /// point: Anywhere on the planet.
    ///
    /// Returns metres, to be *added* to the continental base elevation.
    ///
    /// Notes:
    /// **Every margin in range, summed - not the nearest one, chosen.**
    ///
    /// Picking the nearest margin is not continuous even though its distance is. The
    /// identity of the neighbour jumps: at a point equidistant from two of a plate's
    /// margins the choice flips under a step of a metre, and the relative motion, the
    /// normal and what lies either side all flip with it. Measured at five hundred and
    /// sixty metres of cliff, a hundred and thirty kilometres from any boundary, where
    /// one margin was transform and the other divergent.
    ///
    /// Summing is continuous because each term depends only on its own distance and
    /// fades to nothing at its own range. It is also the truer answer: near a triple
    /// junction there really are two margins acting on the ground.
    ///
    /// Costs nothing where nothing is happening. A plate interior fails the distance
    /// test on every bisector, having done one dot product each - and that is 69 per
    /// cent of the planet.
    ///
    /// **Iteration order is load-bearing.** Floating-point addition is not
    /// associative, so the total depends on the order the margins are summed in.
    /// `margins_within` returns them in plate-position order, and this loop must
    /// accumulate in that same order - no sorting, no reversing, no parallel
    /// accumulation.
    ///
    /// **The plate part only.** The seamount term is added by [`Tectonics::offset_m`], which
    /// wraps this. It has to be outside this function rather than at the end of it, because
    /// the two early returns below cover most of the planet -- see `offset_m`'s own doc.
    fn margin_offset_m(&self, point: &SpherePoint) -> f64 {
        let (nearest, margins) =
            self.plates.margins_within(point, MAX_TECTONIC_RANGE_M, self.radius_m);
        if margins.is_empty() {
            return 0.0;
        }
        let near = match nearest {
            Some(plate) => plate,
            None => return 0.0,
        };

        let mut total = 0.0;
        for margin in &margins {
            // `margin.normal` is the bisector's plane normal; `flattened` projects it
            // into the tangent plane at `point` to get the across-margin direction
            // `from_margin` needs. Skipped entirely, not zeroed, when the projection is
            // degenerate - a zero contribution and a skip are different things, and the
            // Python skips.
            let normal = match self.plates.flattened(point, &margin.normal) {
                Some(n) => n,
                None => continue,
            };
            // `margin.normal` goes down as well as `normal`, and they are two different
            // things: `normal` is the across-margin direction in the tangent plane at
            // `point`, and `margin.normal` is the bisector's PLANE normal, which is what the
            // along-margin projection in `margin_warp_m_at` needs. The tangent-plane one
            // cannot stand in for it -- projecting onto a plane that varies with the query
            // point is exactly what would make the warp vary across the belt as well as
            // along it, which is the failure this technique exists to avoid.
            total += margin.weight
                * self.from_margin(
                    point,
                    &near,
                    &margin.other,
                    margin.distance_m,
                    &normal,
                    &margin.normal,
                );
        }
        total
    }

    /// How much the plates AND the seamount field raise or lower the ground here.
    ///
    /// Args:
    /// point: Anywhere on the planet.
    ///
    /// Returns metres, to be *added* to the continental base elevation.
    ///
    /// Notes:
    /// [`Tectonics::margin_offset_m`] is the plate part and carries its own notes -- including
    /// the load-bearing iteration order, which this split does not touch. This function is that
    /// plus the seamount term, and the split exists for one reason.
    ///
    /// **The seamount term cannot live at the end of the margin sum, and that was a real defect
    /// rather than a style point.** `margin_offset_m` returns `0.0` early when no margin is in
    /// range, which its own doc puts at 69 per cent of the planet and which measures **77.16%**
    /// on the shipped fixture. A seamount term written after that early return is not evaluated
    /// on most of the world, and the boundary of the region where it *is* evaluated is a cliff:
    /// measured at **3,460.23 m in one 20 m step**, 454 times `peak_offset_m`'s own analytic
    /// bound and larger than the 1,466 m cliff that made this file grow a continuity test in
    /// the first place. A seamount is a property of the seabed, not of how near a plate
    /// boundary it happens to be, so it belongs at this level -- outside the margin sum
    /// entirely, applied to whatever that sum returned, including nothing.
    /// `the_seamount_term_is_reachable_everywhere_no_matter_where_the_margins_fall` is the pin;
    /// it was written red against the old shape and is green against this one.
    ///
    /// **Gated, not added unconditionally, and that is load-bearing.** `peak_offset_m` returns
    /// exactly `0.0` when the block is absent, inert, or the water here is too shallow -- but
    /// `-0.0 + tectonic` is `+0.0` when `tectonic` is exactly `-0.0`, which would flip a sign
    /// bit `Tectonics::new` never would. Same shape as `Continentality::above_shore`'s
    /// `amplitude == 0.0` guard (`continentality.rs:382-388`): an early return for the
    /// canonical and inert cases, never a `+ 0.0`.
    ///
    /// **One comparison, and it covers every inert block rather than the zero-density one
    /// only.** `with_peaks` stores `None` for any block `peak_block_is_live` refuses, so there
    /// is no second arm here for a `Some` that cannot raise ground -- an inert block takes the
    /// `None` arm and never reaches `base_elevation`'s fBm below.
    pub fn offset_m(&self, point: &SpherePoint) -> f64 {
        let tectonic = self.margin_offset_m(point);
        match self.peaks {
            None => tectonic,
            Some(_) => {
                // "Seabed" here means the ground a seamount would actually stand on: the
                // continental base PLUS everything the plates have already done to it
                // (`tectonic`), not `base_elevation` alone. Using the base alone would let an
                // island erupt on a tectonic ridge that is already shallow, or ignore a trench
                // that has made the water deeper than the base suggests. `Shelf::evaluate`
                // computes exactly this sum as `macro_elevation` from this function's own
                // return value, so this mirrors what the caller will do with the answer.
                //
                // Computed only on this branch -- an fBm, and the whole reason the `None` arm
                // above returns before touching it, since this function is sampled at every
                // node the hydrology bake visits.
                let seabed_m = self.land.base_elevation(point) + tectonic;
                // Precondition carried from `peak_offset_m`'s own doc: `point.vector` must
                // be unit length. Not asserted here, even in debug -- this function's own
                // `point` comes from the same `SpherePoint` every other caller in this file
                // already trusts to be unit length (see `peak_offset_m`'s doc: "every
                // `SpherePoint` this codebase constructs is already unit length"), and nothing
                // upstream of this call site admits a raw external vector the way a `bindings.rs`
                // entry point could. That entry point is this function's other call site
                // (Task 4's), and is where a check belongs if one is ever needed.
                let standing = self.peak_offset_m(point, seabed_m);
                if standing > 0.0 {
                    tectonic + standing
                } else {
                    tectonic
                }
            }
        }
    }

    /// The macro elevation: continental base plus whatever the plates have done to it.
    ///
    /// Args:
    /// point: Anywhere on the planet.
    ///
    /// Returns metres, relative to datum, before shelves or detail.
    pub fn elevation_m(&self, point: &SpherePoint) -> f64 {
        self.land.base_elevation(point) + self.offset_m(point)
    }

    /// How high the seamount field stands at a point, in metres, never negative.
    ///
    /// One candidate per lattice node, its existence hashed against `density`, its position
    /// jittered inside its own cell and its height drawn from a third salt. The 27 cells
    /// around the sample are examined because a jittered node can fall in any neighbour.
    ///
    /// **The 3x3x3 scan is complete, not merely adequate, because `reach_m <= lattice_m` is
    /// enforced below -- and that is a proof, not an assertion.** In the scaled space this
    /// function measures in, one lattice unit is `lattice_m` of real distance and the query's
    /// own cell coordinate on any axis is `b = floor(q)`, so `q` itself lies in `[b, b+1)`.
    /// A candidate cell outside the 3x3x3 block differs from `b` by at least 2 on SOME axis,
    /// so that candidate's coordinate on that axis lies in `[b+2, b+3)` or further --
    /// whatever its jitter, at least `1.0` scaled unit (`lattice_m` of real distance) away
    /// from `q` on that axis alone, and Euclidean distance can only be at least that large.
    /// `reach_m <= lattice_m` therefore means such a candidate's `fraction` is at least 1.0,
    /// which the `!(fraction < 1.0)` guard below already excludes -- so nothing outside the
    /// 3x3x3 block could ever have contributed regardless of whether the scan reached it.
    /// **The boundary itself has no seam**: at exact equality (`fraction == 1.0`),
    /// `smooth(1.0 - 1.0)` is `smooth(0.0)`, which is exactly 0 by its own formula, so a
    /// candidate at the farthest distance this guard still admits contributes nothing, and
    /// there is no discontinuity to paper over at the cutoff. Task 4 enforces the invariant
    /// again at the ABI boundary; here it is a guard, returning zero, not a clamp.
    ///
    /// **Distance is measured in the scaled space the lattice itself lives in, before any
    /// projection onto the sphere.** An earlier version of this function normalised each
    /// candidate onto the unit sphere and measured chord distance there, which collapses a
    /// node's radial position to nothing: a node one full lattice cell further from the
    /// planet's centre than another, in the same direction, reported the same chord to a
    /// surface query -- a mistake independent of scan radius, since no `N x N x N` widening
    /// fixes a distance that was never being measured. See [`Tectonics::peak_of_cell`] for
    /// where the corrected distance is computed and why a node's radial position is real
    /// rather than a defect to normalise away.
    ///
    /// **The window is evaluated before the lattice is touched**, so a term that is inert, or
    /// a point over shallow water, costs one comparison and no hashing. That is the same
    /// ordering `coast_offset` uses (`continentality.rs:409-411`) and for the same reason.
    /// **"Inert" means inert for any reason, not only for a zero density**: the five
    /// point-independent ways a block can answer zero everywhere are decided once per world by
    /// `peak_block_is_live`, which `Tectonics::with_peaks` applies before storing the block, so
    /// every one of them arrives here as a `None` and stops at the match below. Until the final
    /// whole-branch review only the zero-density case did; the other four reached
    /// `Tectonics::offset_m`'s `base_elevation` call -- a full fBm -- on every sample.
    ///
    /// `seabed_m` is taken as an argument rather than read from `self.land` here, even
    /// though `Tectonics` holds the `Continentality` this world's seabed comes from.
    /// `Continentality::base_elevation` costs an fBm, and the whole point of evaluating the
    /// depth window first is that an inert block or a shallow point costs nothing -- reaching
    /// into `self.land` unconditionally would pay that fBm on every call, including the ones
    /// this function exists to make free. It also keeps this function directly testable at a
    /// stated depth, which is what most of the tests below do, and leaves a caller that
    /// already has an elevation in hand (Task 2's `offset_m`) free to hand it over rather than
    /// have a second one computed on its behalf.
    ///
    /// **Precondition: `point.vector` is a unit vector.** Neither this function nor
    /// [`Tectonics::peak_of_cell`] normalises it. Every `SpherePoint` this codebase
    /// constructs is already unit length, but `bindings.rs` does not itself re-check one
    /// coming from outside, so a caller admitting a raw vector from a boundary is responsible
    /// for that guarantee -- carried to Tasks 2 and 4, where those call sites live.
    pub fn peak_offset_m(&self, point: &SpherePoint, seabed_m: f64) -> f64 {
        // One comparison for an inert block. `Tectonics::with_peaks` has already refused,
        // once for the whole world, every block `peak_block_is_live` calls dead -- zero
        // density, a zero or non-finite `height_m`, a non-positive `lattice_m` or `reach_m`,
        // and a `reach_m` past `lattice_m`, the invariant that makes the 3x3x3 scan below
        // complete (see this function's own doc; Task 4 restates it at the ABI boundary).
        // Nothing writes `peaks` after construction, so a `Some` reaching here is live and
        // those five conditions need not be re-asked at every sample.
        let params = match self.peaks {
            None => return 0.0,
            Some(params) => params,
        };
        let window = peak_depth_window(-seabed_m, params.min_depth_m);
        if !(window > 0.0) {
            return 0.0;
        }

        // Lattice cells about `lattice_m` across on this planet's surface. `q` is the query
        // point in that SAME scaled space -- one unit is `lattice_m` of real ground distance
        // -- and is what every candidate's distance below is measured against, before any
        // renormalisation onto the sphere.
        let frequency = self.radius_m / params.lattice_m;
        let v = point.vector;
        let q = Vec3 { x: v.x * frequency, y: v.y * frequency, z: v.z * frequency };

        // Lattice coordinates are never derived by a bare cast. `noise.rs:166-168` is the
        // pattern: floor, bound with a negated pair so a NaN takes the branch, then cast.
        let (fx, fy, fz) = (m::floor(q.x), m::floor(q.y), m::floor(q.z));
        if !(fx >= -LATTICE_LIMIT && fx <= LATTICE_LIMIT)
            || !(fy >= -LATTICE_LIMIT && fy <= LATTICE_LIMIT)
            || !(fz >= -LATTICE_LIMIT && fz <= LATTICE_LIMIT)
        {
            return 0.0;
        }
        let (bx, by, bz) = (
            fx as i64, // cast-ok: floored and bounded against LATTICE_LIMIT on the lines above
            fy as i64, // cast-ok: floored and bounded against LATTICE_LIMIT on the lines above
            fz as i64, // cast-ok: floored and bounded against LATTICE_LIMIT on the lines above
        );

        let mut tallest = 0.0f64;
        for dz in -1..=1 {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (cx, cy, cz) = (bx + dx, by + dy, bz + dz);
                    let candidate = match self.peak_of_cell(cx, cy, cz, params) {
                        Some(found) => found,
                        None => continue,
                    };
                    // The corrected distance: real 3-D separation between the query point
                    // and the candidate's own node, in the scaled space they are both
                    // already expressed in, converted to metres by `lattice_m` -- one unit
                    // of this space IS `lattice_m` of ground distance. This is exactly the
                    // distance a node's radial position was previously erased from.
                    let d = Vec3 {
                        x: q.x - candidate.node.x,
                        y: q.y - candidate.node.y,
                        z: q.z - candidate.node.z,
                    };
                    let dist_m = m::sqrt(d.x * d.x + d.y * d.y + d.z * d.z) * params.lattice_m;
                    let fraction = dist_m / params.reach_m;
                    if !(fraction < 1.0) {
                        continue;
                    }
                    // A cone with a smoothed flank, so nothing downstream differences a corner.
                    let standing = params.height_m * (0.45 + 0.55 * candidate.share)
                        * smooth(1.0 - fraction)
                        * window;
                    if standing > tallest {
                        tallest = standing;
                    }
                }
            }
        }
        tallest
    }

    /// One lattice cell's candidate peak: its own node, and how far that node stands from
    /// the one point on the sphere closest to it.
    ///
    /// **Precondition: `self` was built at the radius `point.vector` is a unit vector for.**
    /// This function (and `peak_offset_m` above it) assumes every query direction it is
    /// handed is already unit length; neither normalises. Nothing at this layer enforces
    /// that -- `bindings.rs` does not either -- so a caller that can hand this a non-unit
    /// vector is responsible for the check. Tasks 2 and 4 carry that enforcement at their own
    /// call sites, which is where the boundary that actually admits an outside vector lives.
    ///
    /// `None` if the cell holds no peak at this density, if its jittered node lands exactly
    /// on the coordinate origin (not reachable by any real query, but guarded rather than
    /// divided into a NaN), or if -- even at the single point on the sphere closest to this
    /// node -- the node is too far from the sphere for any query to ever be in reach. The
    /// last case is new: a node's own existence no longer guarantees it can be seen from
    /// anywhere.
    ///
    /// **This is the one statement of where a candidate's node stands, in the same scaled
    /// space `peak_offset_m`'s query point is measured in.** `peak_offset_m`'s 3x3x3 scan
    /// calls this once per neighbour and measures its own distance to `node` directly; a
    /// test that wants to know a candidate's own geometry calls this same function rather
    /// than re-deriving the jitter maths, which is what makes the summit and ring tests below
    /// proofs about this code and not about a second copy of it.
    ///
    /// **A node is a point in three dimensions, not a point pinned to the sphere.** The
    /// jittered node's own distance from the origin need not equal `radius_m / lattice_m`,
    /// the radius every query point sits at -- and that radial difference is real: a node
    /// that lands further out than the shell stands taller than `height_m` would suggest at
    /// a query directly below it were the shell not itself the limit, while a node that
    /// lands further in can never reach `height_m` at all, no matter how directly a query
    /// stands over it. `min_dist_m` is exactly the shortfall that ceiling is built from --
    /// the tallest crest this node can ever produce is
    /// `height_m * (0.45 + 0.55 * share) * smooth(1 - min_dist_m / reach_m)`, achieved only
    /// by a query at the node's own summit direction with a fully-saturated depth window, and
    /// it is frequently below `height_m * (0.45 + 0.55 * share)` alone, which was this
    /// function's entire (wrong) answer before a node's radial position was measured. Nothing
    /// on the live path ever needs that crest value, so nothing here computes it -- see
    /// `PeakCandidate`'s own doc.
    ///
    /// **This is also the feature the fixed distance buys, not merely the bug it fixes.** A
    /// node that falls short of the shell produces a real seamount that never breaks the
    /// surface -- a submerged shoal, which is exactly the navigational hazard a chart wants
    /// and which a generator that flattened every candidate onto the shell could never
    /// produce.
    fn peak_of_cell(&self, cx: i64, cy: i64, cz: i64, params: PeakParams) -> Option<PeakCandidate> {
        if self.peak_noise.lattice_at(cx, cy, cz) >= params.density {
            return None;
        }
        // Jitter inside the cell, from a salt of its own so height and position are
        // uncorrelated. This node lives in the SAME scaled space `peak_offset_m`'s query
        // point does -- one unit is `lattice_m` of real ground distance -- and is not
        // renormalised onto the sphere: its own distance from the origin is left exactly as
        // the lattice produced it.
        let jx = self.peak_jitter.lattice_at(cx, cy, cz);
        let jy = self.peak_jitter.lattice_at(cy, cz, cx);
        let jz = self.peak_jitter.lattice_at(cz, cx, cy);
        // `cx`, `cy` and `cz` are `bx + dx` etc. in `peak_offset_m` -- an i64 within one of
        // `bx`/`by`/`bz`, each already an exact round trip of a floored, LATTICE_LIMIT-bounded
        // f64 (see `noise.rs:166-168`'s pattern). i64-to-f64 is exact up to 2^53 (about
        // 9.007e15); LATTICE_LIMIT (9.0e18) is the earlier float-to-int cast's overflow bound
        // and is far past that, not a precision guarantee for this cast on its own -- what
        // actually keeps this exact is that every coordinate this crate's own frequency
        // schedules can produce is of order 10 to 1e6, many orders of magnitude below 2^53.
        let node = Vec3 {
            x: (cx as f64) + jx, // cast-ok: see the note above this block
            y: (cy as f64) + jy, // cast-ok: see the note above this block
            z: (cz as f64) + jz, // cast-ok: see the note above this block
        };
        let radial = m::sqrt(node.x * node.x + node.y * node.y + node.z * node.z);
        if !(radial > 0.0) {
            return None;
        }
        // The one point on the sphere closest to `node`: `node`'s own direction, scaled back
        // up to the query radius. A local value, not stored -- nothing outside this function
        // needs the DIRECTION, only the DISTANCE it produces below, so keeping it local is
        // what stops that distance's own arithmetic (and, before this round, a wasted
        // `smooth` call besides) from following it onto the hot path as a dead field.
        let frequency = self.radius_m / params.lattice_m;
        let closest = Vec3 {
            x: node.x / radial * frequency,
            y: node.y / radial * frequency,
            z: node.z / radial * frequency,
        };
        let d = Vec3 { x: closest.x - node.x, y: closest.y - node.y, z: closest.z - node.z };
        let min_dist_m = m::sqrt(d.x * d.x + d.y * d.y + d.z * d.z) * params.lattice_m;
        if !(min_dist_m / params.reach_m < 1.0) {
            // Too far from the shell, in either direction, for any query to ever reach it.
            return None;
        }
        let share = self.peak_height.lattice_at(cx, cy, cz);
        Some(PeakCandidate { node, share, min_dist_m })
    }

    /// The stacked-suture collision shape at a signed across-margin distance.
    ///
    /// **A range is not one crest.** Over 70% of the North American Cordillera is accreted
    /// terranes and one of its margins took four separate accretion events in 60 Myr; the
    /// Himalaya carries at least two sutures of different ages, which is why it has internal
    /// belt structure rather than a single ridge. So the collision profile is a short sum of
    /// asymmetric bumps at hashed inboard offsets, not a single symmetric one -- and that
    /// supplies structure ACROSS the range at 50-200 km, the axis a noise field cannot
    /// reach because noise has no idea where the margin is.
    ///
    /// Weights and offsets are hashed from the ORDERED plate pair, so both sides of one
    /// margin agree about where its sutures are. The first suture is special-cased to weight
    /// exactly 1.0 at offset exactly 0.0, which is what makes `suture_count == 1` the single
    /// [`asymmetric_bump`] this profile has always been -- identical expression, not merely
    /// an equal value.
    ///
    /// **Overlapping sutures ADD.** With `suture_spread_m` below the flank width the sum
    /// is a plateau rather than a comb, and above it the crests separate with saddles
    /// between. That interaction is real, it means the height knob and the count knob are
    /// not independent, and `src/bin/mountain_survey.rs` measures the peak at every pair
    /// rather than hiding it behind a normalisation nobody could justify.
    ///
    /// Args:
    /// across_m: Signed across-margin distance, already warped; positive is inboard.
    /// near: The plate the point is on.
    /// far: The plate across the margin.
    fn sutures(&self, across_m: f64, near: &Plate, far: &Plate) -> f64 {
        let params = self.params;
        let mut total = asymmetric_bump(
            across_m,
            params.continent_collision_width_m,
            params.collision_asymmetry,
        );
        let mut index = 1u32;
        while index < params.suture_count {
            // In [-1, 1), so a suture sits either side of its nominal offset and the belt
            // does not read as an arithmetic comb.
            let jitter =
                2.0 * pair_fraction(near.index, far.index, STRUCTURE_SALT ^ u64::from(index)) - 1.0;
            let centre = f64::from(index) // cast-ok: a small suture index to float, exact
                * params.suture_spread_m
                * (1.0 + SUTURE_OFFSET_JITTER * jitter);
            let weight = SUTURE_WEIGHT_FLOOR
                + (1.0 - SUTURE_WEIGHT_FLOOR)
                    * pair_fraction(near.index, far.index, SEGMENTATION_SALT ^ u64::from(index));
            total += weight
                * asymmetric_bump(
                    across_m - centre,
                    params.continent_collision_width_m,
                    params.collision_asymmetry,
                );
            index += 1;
        }
        total
    }

    /// The structure multiplier at a point: a ridged multifractal gated by a coarse
    /// segmentation field, mixed in by `structure_depth`. In `[1 - depth, 1]`.
    ///
    /// The ridging is what gives relief ALONG the belt; the segmentation is what breaks a
    /// continuous welt into separate massifs, which is the difference between a range and a
    /// wall. Both match what the uplift measurements say: an ordinary orogen runs 1-3 mm/yr
    /// and its hotspots 9-13 mm/yr, and the hotspots are narrow -- a spiky field, not a dome.
    ///
    /// **Multiplied into the collision amplitude, never added.** An added field would move
    /// ground that is nowhere near a margin and would not fade with the envelope; a
    /// multiplied one can only carve the range that is already there, which is the whole
    /// envelope-times-structure argument this task exists for.
    fn structure_at(&self, point: &SpherePoint) -> f64 {
        let params = self.params;
        if params.structure_wavelength_m <= 0.0 {
            return 1.0;
        }
        let frequency = self.radius_m / params.structure_wavelength_m;
        let v = point.vector;
        let ridges = self.structure.ridged(
            v.x * frequency,
            v.y * frequency,
            v.z * frequency,
            1.0,
            STRUCTURE_OCTAVES,
            0.5,
            2.0,
        );
        // `fbm` is centred on zero with roughly unit range; recentre to [0, 1] and
        // smoothstep so a massif's edge is a soft boundary rather than a step. Bounded by
        // explicit branch: the house form, and `clamp` is NaN-asymmetric.
        let coarse = self.segmentation.fbm(
            v.x * frequency / SEGMENTATION_WAVELENGTH_RATIO,
            v.y * frequency / SEGMENTATION_WAVELENGTH_RATIO,
            v.z * frequency / SEGMENTATION_WAVELENGTH_RATIO,
            1.0,
            SEGMENTATION_OCTAVES,
            0.5,
            2.0,
        );
        let raw = coarse * 0.5 + 0.5;
        let capped = if raw < 1.0 { raw } else { 1.0 };
        let bounded = if capped > 0.0 { capped } else { 0.0 };
        let segments = bounded * bounded * (3.0 - 2.0 * bounded);

        1.0 - params.structure_depth + params.structure_depth * ridges * segments
    }

    /// **How far this stretch of margin has wandered off its great circle, in metres.**
    ///
    /// This is the whole of Task 5 and the whole of the answer to *"they look like they were
    /// drawn with a straight line tool"*. Every margin in this engine IS a great circle --
    /// `plates.rs::margin_at` measures `asin(|p . n|) * radius`, and a plane through the
    /// origin cuts a sphere in a circle of its full radius, exactly once, with no other
    /// shape available. So a belt anchored on that distance is straight by construction and
    /// no amplitude, width, asymmetry, suture or structure setting can bend it.
    ///
    /// **The perturbation is a function of position ALONG the margin, and nothing else.**
    /// With `n` the bisector's plane normal and `p` the query point,
    ///
    /// ```text
    /// signed = p . n                      // the signed sine-distance across the margin
    /// along  = normalise(p - signed * n)  // the nearest point ON the great circle
    /// ```
    ///
    /// `along` is **constant across the belt and varies only along it**: every point on one
    /// perpendicular through the margin projects to the same place. Sampling the noise there
    /// therefore translates the entire belt sideways, coherently, at each station along its
    /// length -- a *wandering belt*. Task 2's removed version sampled an `fbm` of `p` itself,
    /// which varies in both directions at once and so produced a *noisy edge*: the crest
    /// moved, and the profile across the belt was chewed up while it moved. That difference
    /// is the difference between the Andes and a torn strip of paper, and it is why this is
    /// a new technique rather than a revival.
    ///
    /// **The projection is orientation-blind and that is load-bearing.** `bisector(i, j)` is
    /// `normalise(seed_i - seed_j)`, so the SAME margin sampled from its two sides is handed
    /// `n` and `-n`. Under that flip `signed` negates and `signed * n` does not, so `along`
    /// -- and every metre of warp derived from it -- is identical on both sides by
    /// construction rather than by luck. That is `pair_fraction`'s argument about ordered
    /// pairs, made about a projection instead of a hash, and it is what stops the warp
    /// putting a seam down the middle of every belt it bends.
    ///
    /// **The seam that IS here is the bisector's own poles**, where `p` is parallel to `n`,
    /// `p - signed * n` is the zero vector, and there is no direction to keep. Guarded
    /// explicitly against [`DEGENERATE`] in `flattened`'s form, not by trusting
    /// `Vec3::normalised`, which only refuses an exactly-zero length -- and answered with
    /// zero warp rather than by skipping the margin, because a skip is a discontinuity and
    /// zero is the continuous limit. Those poles sit 90 degrees from the margin, which on
    /// any world is far beyond [`MAX_TECTONIC_RANGE_M`], so this is a guard against
    /// arithmetic rather than against a place terrain is drawn.
    ///
    /// Returns metres, signed, bounded in magnitude by `margin_warp_m`: `Noise::fbm` divides
    /// its octave sum by the summed amplitude and every lattice sample is in `[0, 1]`, so it
    /// is in `[-1, 1]` by construction. [`TectonicParams::collision_reach_m`] relies on that
    /// bound.
    fn margin_warp_m_at(&self, point: &SpherePoint, bisector: &Vec3) -> f64 {
        let params = self.params;
        // A silence rather than a division by nothing, matching `structure_at`'s opening
        // line. `wasm.rs` floors this field for the same reason it floors that one.
        if params.margin_warp_wavelength_m <= 0.0 {
            return 0.0;
        }
        let v = point.vector;
        let signed = v.dot(bisector);
        let flat = v.sub(&bisector.scaled(signed));
        if flat.length() <= DEGENERATE {
            return 0.0;
        }
        let along = match flat.normalised() {
            Some(a) => a,
            None => return 0.0,
        };
        let frequency = self.radius_m / params.margin_warp_wavelength_m;
        params.margin_warp_m
            * self.warp.fbm(
                along.x * frequency,
                along.y * frequency,
                along.z * frequency,
                1.0,
                MARGIN_WARP_OCTAVES,
                0.5,
                2.0,
            )
    }

    /// One margin's contribution to the ground here.
    ///
    /// Args:
    /// point: Where.
    /// near: The plate the point is on.
    /// far: The plate across this margin.
    /// distance_m: How far the margin is.
    /// normal: Across it, tangent to the surface, pointing towards `near`.
    /// bisector: The margin's great-circle plane normal, for the along-margin warp only.
    ///
    /// Returns metres, which may be zero, and usually is.
    fn from_margin(
        &self,
        point: &SpherePoint,
        near: &Plate,
        far: &Plate,
        distance_m: f64,
        normal: &Vec3,
        bisector: &Vec3,
    ) -> f64 {
        let motion = motion_between(near, far, point, normal, self.radius_m);

        // How much of the relative motion is across the margin rather than along it, from
        // -1 (pulling apart) through 0 (pure sliding) to +1 (head on).
        //
        // **Weighed, not classified.** `motion.kind` is a name given by a threshold, and
        // using the name to pick a profile meant a margin drifting from convergent to
        // transform went from a full mountain belt to nothing in one step. The name
        // survives for diagnostics; the terrain uses the number.
        let speed = m::hypot(motion.closing_m_per_myr, motion.sliding_m_per_myr);
        // Safe regardless of `hypot`'s algorithm: Task 1 measured that `math.hypot` and
        // `libm::hypot` differ by at most 1 ULP but both are exactly zero only when both
        // arguments are exactly zero. So this comparison decides identically either way.
        if speed <= 0.0 {
            return 0.0;
        }
        let across = motion.closing_m_per_myr / speed;

        // A transform margin still leaves no mark. It arrives at no mark smoothly.
        let mut engagement = (across.abs() - ACROSS_ENOUGH) / (1.0 - ACROSS_ENOUGH);
        // This is the one branch that genuinely depends on `hypot`'s precision: `across`
        // is built from `speed`, and a 1-ULP disagreement between `math.hypot` and
        // `libm::hypot` propagates into it. Task 1 measured the margin here at 1.19e-4 --
        // about 1.07e12 ULP -- twelve orders of magnitude clear of where a 1-ULP `hypot`
        // disagreement could flip this comparison.
        if engagement <= 0.0 {
            return 0.0;
        }
        // Python writes `min(1.0, engagement)`; house form keeps the first operand unless
        // the second is strictly less than it.
        engagement = if engagement < 1.0 { engagement } else { 1.0 };
        engagement = engagement * engagement * (3.0 - 2.0 * engagement);

        // Python writes `min(1.0, speed / FULL_RATE_M_PER_MYR)`.
        let rate_fraction = speed / FULL_RATE_M_PER_MYR;
        let capped_rate = if rate_fraction < 1.0 { rate_fraction } else { 1.0 };
        let strength = capped_rate * engagement;
        // Nearly unreachable, not dead: this can only fire if `engagement`'s smoothstep
        // above has underflowed to exactly zero, which needs the pre-smoothstep
        // `engagement` below roughly 1e-162. Ported and kept rather than deleted as
        // unreachable.
        if strength <= 0.0 {
            return 0.0;
        }

        if across < 0.0 {
            // Pulling apart. Symmetric about the axis, so it needs no sense of side.
            //
            // Safe regardless of `hypot`'s precision, even though it looks like the most
            // dangerous of these branches: `hypot` is never negative, and the zero case
            // already returned above, so `speed` is strictly positive here and dividing
            // by it cannot change the sign of `motion.closing_m_per_myr`. This branch is
            // decided by that sign, which is algebraic, not measured through `hypot`.
            return strength
                * (self.params.ridge_m * bump(distance_m, self.params.ridge_width_m)
                    + RIFT_M * bump(distance_m, RIFT_WIDTH_M));
        }

        let setting = self.setting_at(point, distance_m, normal);
        let inboard = setting.inboard_continental_with(self.params.continental_blend);
        let outboard = setting.outboard_continental_with(self.params.continental_blend);
        let collision = inboard * outboard;
        let oceanic = (1.0 - inboard) * (1.0 - outboard);
        // Python writes `max(0.0, 1.0 - collision - oceanic)`; house form keeps the first
        // operand unless the second is strictly greater than it.
        let remainder = 1.0 - collision - oceanic;
        let subduction = if remainder > 0.0 { remainder } else { 0.0 };

        // The convergent response at a signed distance across the margin. A closure
        // rather than a free function: it exists only to capture `collision`, `oceanic`
        // and `subduction`, which are local to this call, and a free function would need
        // all three threaded through as extra parameters for no benefit.
        //
        // `params` is a copy rather than a borrow of `self.params` so the hot fields are a
        // plain value in the closure. The closure DOES now hold a shared borrow of `self`,
        // for `sutures`, which is sound and was not true before this task: nothing here is
        // mutable and `Tectonics` has no interior mutability -- `noise.rs` dropped the
        // Python's corner cache precisely so it would not.
        let params = self.params;

        // ------------------------------------------------------- the structure field
        //
        // Sampled ONCE per call, outside the closure, for two reasons. It costs: `profile`
        // is evaluated twice, at `+distance_m` and `-distance_m`, and both are the same
        // point, so a sample inside would be paid twice for one answer. And it is CORRECT:
        // it describes where on the belt this point sits, which is a property of the point
        // and not of which side of the blend is being evaluated.
        //
        // Skipped entirely at its canonical setting, which is what keeps `Noise::ridged` --
        // a primitive `worldbuilder/terrain/noise.py` does not have -- off the canonical
        // path rather than merely multiplied by zero on it.
        let structure = if params.structure_depth != 0.0 {
            self.structure_at(point)
        } else {
            1.0
        };

        // ---------------------------------------------------- the along-margin warp
        //
        // Sampled ONCE, outside the closure, for `structure`'s two reasons and a third of
        // its own: the closure is evaluated at both `+distance_m` and `-distance_m` for the
        // same point, and the warp is a property of WHERE ON THE MARGIN this point sits, not
        // of which side of the blend is being evaluated. A sample inside would be paid twice
        // for one answer and would be describing the wrong thing while it did.
        //
        // Skipped entirely at the canonical setting -- the same shape as `structure`, and
        // for the same reason: a canonical world must perform the identical operations it
        // performed before this field existed, not merely reach the same number.
        //
        // **THE SIGNED AXIS IS THE PROFILE'S OWN, AND THE FIRST VERSION'S WAS NOT.** The
        // warp needs a signed across-margin coordinate to displace along, and the obvious
        // source -- `p . bisector` -- carries no side information at all: for a point on
        // plate A the table hands back `normalise(A - B)` and the dot product is positive;
        // for a point on B it hands back `normalise(B - A)` and the dot product is positive
        // AGAIN. So the first version took the side from the ORDERED PLATE PAIR, the way
        // `pair_fraction` takes its suture offsets, and **that produced a 913 m cliff**,
        // measured as a single 100 m step on a transect across the belt (against 11.6 m for
        // the same configuration unwarped).
        //
        // The reason is worth writing down, because the ordered pair genuinely IS stable
        // across the margin it belongs to. It is not stable across a THIRD plate's boundary:
        // crossing from plate A into plate C replaces the whole `(A, *)` margin set with
        // `(C, *)`, so the belt that was `(A, B)` becomes `(C, B)` -- a different ordered
        // pair, whose index comparison can come out the other way and flip the displacement
        // from `+w` to `-w` in one step. `pair_fraction`'s own doc comment names this exact
        // hazard -- *"a sign that flips across a boundary is a cliff"* -- about the sutures,
        // and it applies with more force to a term that moves the whole belt.
        //
        // So the sign comes from the axis `from_margin` ALREADY has. The profile is
        // evaluated at `+distance_m` and `-distance_m` and blended by `toward`, which is the
        // smoothed lean -- **positive is the overriding, more continental side**, a
        // geological quantity that varies continuously and belongs to no plate's index. Write
        // `x` for that signed coordinate; on the overriding side the point has `x = +d` with
        // `toward` near one, and on the subducting side `x = -d` with `toward` near zero, so
        // subtracting the warp from BOTH branch arguments makes the collision profile
        // `P(x - w)` on both sides of the margin -- one expression, one belt, translated by
        // `w`. Where the lean is near zero the two branches simply blend, which is the
        // behaviour that was already there.
        let (collision_across_m, mirrored_collision_across_m) = if params.margin_warp_m != 0.0 {
            let warp_m = self.margin_warp_m_at(point, bisector);
            (distance_m - warp_m, -distance_m - warp_m)
        } else {
            // The identical `f64`s the profile was handed before this field existed, not
            // merely equal ones: no warp is sampled, no arithmetic is performed, and the
            // canonical path evaluates the same expression on the same numbers in the same
            // order. That is what `the_margin_warp_is_inert_at_its_canonical_setting`
            // asserts by bits.
            (distance_m, -distance_m)
        };

        let profile = |across_m: f64, collision_across_m: f64| -> f64 {
            let collided = params.continent_collision_m
                * structure
                * self.sutures(collision_across_m, near, far);
            // Only the collision term is stacked, modulated AND WARPED. The trench, the arc
            // and the coastal rise stay anchored to the margin itself, which is where they
            // belong: a trench IS the plate boundary. That is why the warp arrives as a
            // second argument rather than by moving `across_m` -- the three terms below read
            // the unwarped distance and the collision term above reads the warped one, from
            // the same call.
            let trench = TRENCH_M * bump(across_m + TRENCH_OFFSET_M, TRENCH_WIDTH_M);
            let arc =
                params.island_arc_m * bump(across_m - ISLAND_ARC_OFFSET_M, params.island_arc_width_m);
            // `COASTAL_UPLIFT_OFFSET_M`, which is 70,000 and coincidentally equals
            // `RIFT_WIDTH_M`. It was a bare literal here for exactly that reason -- binding
            // it to `RIFT_WIDTH_M` would couple two profiles that must vary independently --
            // and it now has a constant of its own instead, because the WASM boundary has to
            // derive this profile's width ceiling from it. Same value, same arithmetic.
            let uplift = params.coastal_uplift_m
                * bump(across_m - COASTAL_UPLIFT_OFFSET_M, params.coastal_uplift_width_m);
            collision * collided + oceanic * (arc + trench) + subduction * (uplift + trench)
        };

        // Which side of the margin this point is on, weighed rather than decided.
        //
        // The obvious form is `signed = distance * lean`, and it is wrong in a way that
        // took a diagnostic to see: scaling the axis *compresses* distance, so with a
        // lean of -0.22 a point four hundred and nineteen kilometres out mapped to -90
        // km, which is exactly where the trench sits. The trench fired at four hundred
        // kilometres and the range gate then cut it off mid-profile.
        //
        // The distance stays true. The profile is evaluated on both sides and blended by
        // the lean, which keeps every feature at its intended range and reaches zero by
        // the gate because each profile does.
        let toward = (1.0 + setting.lean()) * 0.5;
        strength
            * (toward * profile(distance_m, collision_across_m)
                + (1.0 - toward) * profile(-distance_m, mirrored_collision_across_m))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::continentality::{ABYSS_M, LAND_FRACTION};
    use crate::plates::tests::three_plate_set;
    use crate::sphere::EARTH_RADIUS_M;

    impl Tectonics {
        /// Test-only window onto the continental base, so `elevation_is_the_base_...`
        /// can compute the same sum `elevation_m` computes internally and compare
        /// bit-for-bit, without duplicating `Continentality`'s internals in the test.
        fn land_base_elevation_for_test(&self, point: &SpherePoint) -> f64 {
            self.land.base_elevation(point)
        }
    }

    /// A `Tectonics` over `three_plate_set()`, for the `offset_m`/`elevation_m` tests
    /// that only need *some* world, not a particular geometry.
    fn test_world() -> Tectonics {
        let land = Continentality::new(20260902, EARTH_RADIUS_M, LAND_FRACTION);
        Tectonics::new(three_plate_set(), land, EARTH_RADIUS_M, None)
    }

    #[test]
    fn a_plate_interior_contributes_exactly_nothing() {
        // three_plate_set's nearest bisector to seed 0 is about 3,900 km away, an order
        // of magnitude beyond MAX_TECTONIC_RANGE_M, so margins_within returns an empty
        // list and offset_m exits before doing any arithmetic at all. Exactly zero, not
        // approximately -- it is a literal early return.
        //
        // Confirmed rather than assumed: querying three_plate_set() directly at this
        // point returns zero margins (measured), so the assertion below exercises the
        // early return and not a sum that happens to cancel.
        let set = three_plate_set();
        let point = SpherePoint::from_latlon(0.0, 0.0);
        let (_, found) = set.margins_within(&point, MAX_TECTONIC_RANGE_M, EARTH_RADIUS_M);
        assert!(found.is_empty(), "expected no margins in range, found {}", found.len());

        let world = test_world();
        assert_eq!(world.offset_m(&SpherePoint::from_latlon(0.0, 0.0)), 0.0);
    }

    #[test]
    fn elevation_is_the_base_plus_the_offset_bit_for_bit() {
        // elevation_m is defined as exactly that sum, in that order. Anything less than
        // bit-equality means the composition was rewritten.
        let world = test_world();
        let point = SpherePoint::from_latlon(12.0, 20.0);
        let expected = world.land_base_elevation_for_test(&point) + world.offset_m(&point);
        assert_eq!(world.elevation_m(&point).to_bits(), expected.to_bits());
    }

    #[test]
    fn a_point_near_two_margins_sums_both_contributions() {
        // The reason offset_m exists. Find a point with more than one margin in range
        // and assert the total differs from either margin's contribution alone -- that
        // is what distinguishes summing from choosing, and choosing was worth 560 m of
        // cliff.
        //
        // three_plate_set() has no such point: its three seeds sit tens of thousands of
        // kilometres apart (two 90 degrees apart on the equator, one lifted to 60N),
        // so no two margins ever fall within MAX_TECTONIC_RANGE_M (420 km) of the same
        // spot -- confirmed by sampling margins_within across that set and finding at
        // most one margin in range anywhere.
        //
        // A dedicated three-plate set with seeds a few degrees apart puts a genuine
        // near-triple-junction inside MAX_TECTONIC_RANGE_M. Found by brute-force
        // sampling of margins_within over a small lat/lon grid around the seeds' rough
        // midpoint: with seeds at (0,0), (0,4) and (3,2), the point (0.5N, 1.5E) has
        // two margins in range, at measured distances of about 55,595 m and 61,682 m --
        // neither equal to the other, so the point is not an artefact of symmetry.
        // Distinct rates about a shared pole, as in `lopsided_world` above: relative
        // motion is the *difference* of two plates' angular velocities, so equal rates
        // would leave every margin here motionless (speed == 0.0, from_margin's first
        // early return) regardless of geometry.
        let plate = |index: usize, lat: f64, lon: f64, rate: f64| Plate {
            index,
            seed: SpherePoint::from_latlon(lat, lon),
            euler_pole: SpherePoint::from_latlon(80.0, 5.0),
            rate_rad_per_myr: rate,
        };
        let set = PlateSet::new(vec![
            plate(0, 0.0, 0.0, 0.02),
            plate(1, 0.0, 4.0, -0.015),
            plate(2, 3.0, 2.0, 0.01),
        ]);
        let point = SpherePoint::from_latlon(0.5, 1.5);

        let (nearest, margins) = set.margins_within(&point, MAX_TECTONIC_RANGE_M, EARTH_RADIUS_M);
        assert_eq!(margins.len(), 2, "expected exactly two margins in range at this point");
        let near = nearest.expect("a nearest plate when margins were found");

        let land = Continentality::new(20260902, EARTH_RADIUS_M, LAND_FRACTION);
        let world = Tectonics::new(set, land, EARTH_RADIUS_M, None);

        let total = world.offset_m(&point);

        // Each margin's own contribution, computed the same way offset_m computes it,
        // so the comparison is against what "choosing just this one" would have given.
        let solo: Vec<f64> = margins
            .iter()
            .filter_map(|margin| {
                let normal = world.plates.flattened(&point, &margin.normal)?;
                let contribution = world.from_margin(
                    &point,
                    &near,
                    &margin.other,
                    margin.distance_m,
                    &normal,
                    &margin.normal,
                );
                Some(margin.weight * contribution)
            })
            .collect();
        assert_eq!(solo.len(), 2, "both margins must survive the flattened() projection");

        assert_ne!(total, solo[0], "the sum must not collapse to just the first margin");
        assert_ne!(total, solo[1], "the sum must not collapse to just the second margin");
        assert_eq!(
            total.to_bits(),
            (solo[0] + solo[1]).to_bits(),
            "the total must be exactly the two contributions summed in plate-position order"
        );
    }

    #[test]
    fn continental_saturates_at_both_ends_and_is_smooth_between() {
        // Thoroughly oceanic and thoroughly continental clamp to exactly 0 and 1;
        // the midpoint is exactly 0.5 because the smoothstep is symmetric about it.
        assert_eq!(continental(-10.0), 0.0);
        assert_eq!(continental(10.0), 1.0);
        assert_eq!(continental(CONTINENTAL_ENOUGH), 0.5);
    }

    #[test]
    fn canonical_params_are_the_module_constants_bit_for_bit() {
        // `canonical()` is not "about today's values", it IS today's values. Compared as
        // bit patterns, because the claim is exactness -- `worldbuilder/terrain/
        // tectonics.py` holds the same nine numbers and is the conformance oracle.
        let p = TectonicParams::canonical();
        assert_eq!(p.continent_collision_m.to_bits(), CONTINENT_COLLISION_M.to_bits());
        assert_eq!(p.continent_collision_width_m.to_bits(), CONTINENT_COLLISION_WIDTH_M.to_bits());
        assert_eq!(p.coastal_uplift_m.to_bits(), COASTAL_UPLIFT_M.to_bits());
        assert_eq!(p.coastal_uplift_width_m.to_bits(), COASTAL_UPLIFT_WIDTH_M.to_bits());
        assert_eq!(p.island_arc_m.to_bits(), ISLAND_ARC_M.to_bits());
        assert_eq!(p.island_arc_width_m.to_bits(), ISLAND_ARC_WIDTH_M.to_bits());
        assert_eq!(p.ridge_m.to_bits(), RIDGE_M.to_bits());
        assert_eq!(p.ridge_width_m.to_bits(), RIDGE_WIDTH_M.to_bits());
        assert_eq!(p.continental_blend.to_bits(), CONTINENTAL_BLEND.to_bits());
    }

    #[test]
    fn continental_is_exactly_continental_with_at_the_canonical_blend() {
        // The no-argument form is kept because `bindings.rs::tectonics_continental` is a
        // conformance binding and its Python counterpart takes one argument. It must
        // therefore be the SAME ramp as the parameterised one, not a second copy that can
        // drift: bit-equality, over both sides of the clamp and the interior.
        for value in [-10.0, -1.0, -0.45, -0.1, 0.0, 0.1, 0.45, 1.0, 10.0] {
            assert_eq!(
                continental(value).to_bits(),
                continental_with(value, CONTINENTAL_BLEND).to_bits(),
                "the two ramps disagree at {value}"
            );
            let setting = Setting { inboard: value, outboard: -value };
            assert_eq!(
                setting.inboard_continental().to_bits(),
                setting.inboard_continental_with(CONTINENTAL_BLEND).to_bits()
            );
            assert_eq!(
                setting.outboard_continental().to_bits(),
                setting.outboard_continental_with(CONTINENTAL_BLEND).to_bits()
            );
        }
    }

    #[test]
    fn a_narrower_continental_blend_is_a_different_ramp() {
        // The discrimination half: if `continental_with` ignored its `blend` argument the
        // test above would pass vacuously. A blend nobody would choose by accident, at a
        // value inside the ramp rather than out on either clamped shoulder.
        assert_ne!(continental_with(0.1, 0.45).to_bits(), continental_with(0.1, 0.9).to_bits());
    }

    #[test]
    fn bump_is_one_at_the_centre_and_zero_at_the_edge() {
        assert_eq!(bump(0.0, 100_000.0), 1.0);
        assert_eq!(bump(100_000.0, 100_000.0), 0.0);
        assert_eq!(bump(-100_000.0, 100_000.0), 0.0);
        assert_eq!(bump(200_000.0, 100_000.0), 0.0);
    }

    #[test]
    fn bump_has_zero_derivative_at_both_ends() {
        // The reason it is a smoothstep and not a taper: no crease where it meets
        // untouched ground. Sample either side of centre and edge; the change per
        // step must shrink towards both, not stay linear.
        let w = 100_000.0;
        let near_centre = bump(0.0, w) - bump(1_000.0, w);
        let mid_slope = bump(40_000.0, w) - bump(41_000.0, w);
        let near_edge = bump(99_000.0, w) - bump(100_000.0, w);
        assert!(near_centre < mid_slope, "must flatten towards the centre");
        assert!(near_edge < mid_slope, "must flatten towards the edge");
    }

    #[test]
    fn a_zero_width_bump_is_nothing_rather_than_a_division_by_zero() {
        assert_eq!(bump(0.0, 0.0), 0.0);
        assert_eq!(bump(50.0, -1.0), 0.0);
    }

    #[test]
    fn lean_is_zero_for_a_symmetric_margin_and_saturates_when_lopsided() {
        // Exactly zero when the two sides are alike -- tanh(0) is exactly 0.0 -- which
        // is what lets an asymmetric profile fade out rather than flip.
        assert_eq!(Setting { inboard: 0.3, outboard: 0.3 }.lean(), 0.0);
        assert!(Setting { inboard: 1.0, outboard: -1.0 }.lean() > 0.99);
        assert!(Setting { inboard: -1.0, outboard: 1.0 }.lean() < -0.99);
    }

    #[test]
    fn setting_at_agrees_from_either_side_of_the_same_margin() {
        // Derivation: `three_plate_set()` puts plate 0 at (0N,0E) and plate 1 at
        // (0N,90E), ninety degrees apart on the equator, with plate 2 lifted off to
        // (60N,45E) far enough away that it plays no part here. Their margin is the
        // great circle bisecting seeds 0 and 1; its midpoint, `normalised(seed0 +
        // seed1)`, is the same construction `plates.rs`'s
        // `a_point_on_the_bisector_is_at_zero_distance` test uses to land exactly on
        // a margin.
        //
        // `flattened(mid, bisector(0, 1))` is the bisector's plane normal projected
        // into the tangent plane at `mid` -- a unit tangent vector perpendicular to
        // the margin's local direction there, i.e. the axis "across" it. Walking a
        // fixed distance `D` either way along that axis from `mid`, in `mid`'s own
        // frame, places two points symmetric about the same margin on the same
        // local straight line: one on plate 0's side, one on plate 1's.
        //
        // Each point's own `margin_at`/`margin_normal` (the same calls `offset_m`
        // makes in real use) then supply the `distance_m` and `normal` `setting_at`
        // expects, and because the two points sit on opposite sides of one margin,
        // one point's "inboard" plate is the other's "outboard" plate.
        let set = three_plate_set();
        let seed0 = set.plate(0).seed.vector;
        let seed1 = set.plate(1).seed.vector;
        let mid = SpherePoint { vector: seed0.add(&seed1).normalised().expect("distinct seeds") };
        let bisector = set.bisector(0, 1).expect("distinct seeds");
        let across = set.flattened(&mid, &bisector).expect("not degenerate at the midpoint");

        let frame = TangentFrame::at(&mid, EARTH_RADIUS_M);
        let east = across.dot(&frame.east);
        let north = across.dot(&frame.north);

        let d = 200_000.0;
        let point_a = frame.local_to_sphere(east * d, north * d);
        let point_b = frame.local_to_sphere(east * -d, north * -d);

        let margin_a = set.margin_at(&point_a, EARTH_RADIUS_M);
        let margin_b = set.margin_at(&point_b, EARTH_RADIUS_M);
        // The two points must actually straddle the same margin -- nearest and
        // neighbour swapped -- or this test would not be exercising the property
        // it claims to.
        assert_eq!(
            margin_a.nearest.expect("a nearest plate").index,
            margin_b.neighbour.expect("a neighbour plate").index
        );
        assert_eq!(
            margin_a.neighbour.expect("a neighbour plate").index,
            margin_b.nearest.expect("a nearest plate").index
        );

        let normal_a = set.margin_normal(&point_a, &margin_a).expect("not degenerate");
        let normal_b = set.margin_normal(&point_b, &margin_b).expect("not degenerate");

        let land = Continentality::new(12345, EARTH_RADIUS_M, LAND_FRACTION);
        let tectonics = Tectonics::new(set, land, EARTH_RADIUS_M, None);

        let setting_a = tectonics.setting_at(&point_a, margin_a.distance_m, &normal_a);
        let setting_b = tectonics.setting_at(&point_b, margin_b.distance_m, &normal_b);

        // The property the design exists for: two samples on opposite sides of the
        // same margin describe the same stretch of it. `inboard` and `outboard`
        // swap roles between the two points -- point A's near plate is point B's
        // far plate -- so it is A's inboard against B's outboard, and A's outboard
        // against B's inboard, not the two `Setting`s being equal outright.
        // Measured: the two sides agree to within a couple of ULPs (~3e-16), not merely
        // within some loose tolerance -- the residual is rounding noise from projecting
        // into two different tangent frames, not a modelling error.
        let tolerance = 1e-9;
        assert!(
            (setting_a.inboard - setting_b.outboard).abs() < tolerance,
            "a.inboard={} b.outboard={}",
            setting_a.inboard,
            setting_b.outboard
        );
        assert!(
            (setting_a.outboard - setting_b.inboard).abs() < tolerance,
            "a.outboard={} b.inboard={}",
            setting_a.outboard,
            setting_b.inboard
        );
    }

    /// A fixed point, margin geometry and pair of plates for exercising `from_margin`
    /// directly, bypassing `offset_m`'s margin lookup.
    ///
    /// Both Euler poles sit at the north pole, so both plates' angular velocities are
    /// `(0, 0, rate)` and the point is on the equator -- the same construction
    /// `kinematics.rs`'s `two_plates_driving_into_each_other_are_convergent` uses. The
    /// relative velocity is then due east-west and the normal `(0, 1, 0)` points due
    /// east, straight along it, so the margin is fully engaged (no sliding component at
    /// all) regardless of distance -- `from_margin`'s `strength` does not depend on
    /// `distance_m`, only on `near`, `far`, `point` and `normal`, which this struct holds
    /// fixed.
    struct LopsidedWorld {
        tectonics: Tectonics,
        point: SpherePoint,
        near: Plate,
        far: Plate,
        normal: Vec3,
        /// The bisector's PLANE normal, `normalise(near.seed - far.seed)` -- the same vector
        /// `PlateSet::new` puts in its table for this ordered pair, built here directly
        /// because this fixture calls `from_margin` without going through `margins_within`.
        /// The along-margin warp is the only thing that reads it.
        bisector: Vec3,
    }

    impl LopsidedWorld {
        fn from_margin_for_test(&self, distance_m: f64) -> f64 {
            self.tectonics.from_margin(
                &self.point,
                &self.near,
                &self.far,
                distance_m,
                &self.normal,
                &self.bisector,
            )
        }
    }

    fn lopsided_world_with(params: Option<TectonicParams>) -> LopsidedWorld {
        let near = Plate {
            index: 0,
            seed: SpherePoint::from_latlon(0.0, 0.0),
            euler_pole: SpherePoint::from_latlon(90.0, 0.0),
            rate_rad_per_myr: 0.01,
        };
        let far = Plate {
            index: 1,
            seed: SpherePoint::from_latlon(0.0, 10.0),
            euler_pole: SpherePoint::from_latlon(90.0, 0.0),
            rate_rad_per_myr: 0.02,
        };
        let point = SpherePoint::from_latlon(0.0, 0.0);
        let normal = Vec3::new(0.0, 1.0, 0.0);

        // A world seed chosen only because it happens to give the two probe points --
        // 300 km either side of the margin, per `PROBE_M` -- genuinely different
        // continentality. Confirmed by the `lean_is_genuinely_non_zero` test below,
        // which is what makes the 419 km test meaningful rather than vacuous.
        let land = Continentality::new(20260902, EARTH_RADIUS_M, LAND_FRACTION);
        let plates = PlateSet::new(vec![near, far]);
        let tectonics = Tectonics::new(plates, land, EARTH_RADIUS_M, params);

        let bisector = near
            .seed
            .vector
            .sub(&far.seed.vector)
            .normalised()
            .expect("two distinct seeds have a bisector");
        LopsidedWorld { tectonics, point, near, far, normal, bisector }
    }

    fn lopsided_world() -> LopsidedWorld {
        lopsided_world_with(None)
    }

    /// One ULP up, by bits, so a perturbation is the smallest change that still is one.
    fn flip_last_bit(x: f64) -> f64 {
        f64::from_bits(x.to_bits() ^ 1)
    }

    /// Every field of `TectonicParams`, and the one-line accessor that perturbs it.
    #[allow(clippy::type_complexity)]
    const FIELDS: [(&str, fn(&mut TectonicParams)); 15] = [
        ("continent_collision_m", |p| p.continent_collision_m = flip_last_bit(p.continent_collision_m)),
        ("continent_collision_width_m", |p| {
            p.continent_collision_width_m = flip_last_bit(p.continent_collision_width_m)
        }),
        ("coastal_uplift_m", |p| p.coastal_uplift_m = flip_last_bit(p.coastal_uplift_m)),
        ("coastal_uplift_width_m", |p| {
            p.coastal_uplift_width_m = flip_last_bit(p.coastal_uplift_width_m)
        }),
        ("island_arc_m", |p| p.island_arc_m = flip_last_bit(p.island_arc_m)),
        ("island_arc_width_m", |p| p.island_arc_width_m = flip_last_bit(p.island_arc_width_m)),
        ("ridge_m", |p| p.ridge_m = flip_last_bit(p.ridge_m)),
        ("ridge_width_m", |p| p.ridge_width_m = flip_last_bit(p.ridge_width_m)),
        ("continental_blend", |p| p.continental_blend = flip_last_bit(p.continental_blend)),
        // The four structure fields. `collision_asymmetry` is canonically 1.0, so a
        // one-ULP flip is a real change to a real width and it must show up. The other
        // three are canonically ZERO or canonically unread, so a one-ULP flip of them is
        // NOT a meaningful perturbation and they are expected to come back BLIND here --
        // that is the design (they are inert at canonical) and not a hole. What proves
        // they are wired is `each_structure_field_moves_the_answer_at_a_stated_setting`
        // below, which perturbs them by an amount the field actually has, and this array
        // carries them so a reader comparing it against the struct sees no field missing.
        // `suture_count` is a `u32` and cannot take an ULP flip at all; it is covered by
        // the stated-setting test only.
        ("collision_asymmetry", |p| p.collision_asymmetry = flip_last_bit(p.collision_asymmetry)),
        ("suture_spread_m", |p| p.suture_spread_m = flip_last_bit(p.suture_spread_m)),
        ("structure_depth", |p| p.structure_depth = flip_last_bit(p.structure_depth)),
        ("structure_wavelength_m", |p| {
            p.structure_wavelength_m = flip_last_bit(p.structure_wavelength_m)
        }),
        // Task 5's two, carried here for the same reason and expected BLIND for the same
        // reason: `margin_warp_m` is canonically 0.0, so a one-ULP flip of it is 5e-324 of
        // sideways displacement on a 4,500 km planet, and `margin_warp_wavelength_m` is
        // unread while the amplitude is zero. Both are proved live by
        // `each_structure_field_moves_the_answer_at_a_stated_setting` and
        // `the_margin_warp_wavelength_is_unreachable_until_the_amplitude_is_turned_up`.
        ("margin_warp_m", |p| p.margin_warp_m = flip_last_bit(p.margin_warp_m)),
        ("margin_warp_wavelength_m", |p| {
            p.margin_warp_wavelength_m = flip_last_bit(p.margin_warp_wavelength_m)
        }),
    ];

    /// **The test that makes `TectonicParams` mean anything.**
    ///
    /// Task 1's bit-identity test cannot catch a field the uplift path never reads: `None`
    /// resolves through `unwrap_or_else(TectonicParams::canonical)`, so both of its arms
    /// call the same function and agree no matter what the path ignores. That is a real
    /// hole and it was found by mutation, not by reading -- the sixth assertion in this
    /// project to look load-bearing and not be.
    ///
    /// So this asserts the complement: perturb ONE field by ONE ULP and the answer must
    /// move somewhere. Population: `lopsided_world()`, swept at 1 km steps from 0 to
    /// `MAX_TECTONIC_RANGE_M`, comparing `from_margin` by bits. A field that survives the
    /// whole sweep unchanged is a field the path does not read, and a slider bound to it
    /// would be a control that does nothing.
    fn fields_that_move(baseline: &LopsidedWorld) -> Vec<&'static str> {
        let mut live = Vec::new();
        for (name, perturb) in FIELDS {
            let mut params = TectonicParams::canonical();
            perturb(&mut params);
            let probe = lopsided_world_with(Some(params));
            let mut distance_m = 0.0;
            while distance_m <= MAX_TECTONIC_RANGE_M {
                if baseline.from_margin_for_test(distance_m).to_bits()
                    != probe.from_margin_for_test(distance_m).to_bits()
                {
                    live.push(name);
                    break;
                }
                distance_m += 1_000.0;
            }
        }
        live
    }

    /// A margin the convergent fixture cannot be: plates pulling APART, over ocean.
    /// `far`'s rotation is reversed, which makes the relative motion extensional and sends
    /// `from_margin` down the divergent early-return; the land fraction is driven to 0.02
    /// so both 300 km probes read oceanic and `oceanic` is not ~0.
    fn spreading_world_with(params: Option<TectonicParams>) -> LopsidedWorld {
        let near = Plate {
            index: 0,
            seed: SpherePoint::from_latlon(0.0, 0.0),
            euler_pole: SpherePoint::from_latlon(90.0, 0.0),
            rate_rad_per_myr: 0.01,
        };
        let far = Plate {
            index: 1,
            seed: SpherePoint::from_latlon(0.0, 10.0),
            euler_pole: SpherePoint::from_latlon(90.0, 0.0),
            rate_rad_per_myr: -0.02,
        };
        let point = SpherePoint::from_latlon(0.0, 0.0);
        let normal = Vec3::new(0.0, 1.0, 0.0);
        let land = Continentality::new(20260902, EARTH_RADIUS_M, 0.02);
        let plates = PlateSet::new(vec![near, far]);
        let tectonics = Tectonics::new(plates, land, EARTH_RADIUS_M, params);
        let bisector = near
            .seed
            .vector
            .sub(&far.seed.vector)
            .normalised()
            .expect("two distinct seeds have a bisector");
        LopsidedWorld { tectonics, point, near, far, normal, bisector }
    }

    #[test]
    fn the_divergent_params_move_the_answer() {
        // The cover the convergent test names for `ridge_*`, which is dead on a
        // continental convergent margin for structural reasons. Note this fixture cannot
        // cover `island_arc_*` either: the divergent early-return fires before the arc
        // term is ever formed.
        let baseline = spreading_world_with(None);
        let mut live = Vec::new();
        for (name, perturb) in FIELDS {
            let mut params = TectonicParams::canonical();
            perturb(&mut params);
            let probe = spreading_world_with(Some(params));
            let mut distance_m = 0.0;
            while distance_m <= MAX_TECTONIC_RANGE_M {
                if baseline.from_margin_for_test(distance_m).to_bits()
                    != probe.from_margin_for_test(distance_m).to_bits()
                {
                    live.push(name);
                    break;
                }
                distance_m += 1_000.0;
            }
        }
        for wanted in ["ridge_m", "ridge_width_m"] {
            assert!(live.contains(&wanted), "{wanted} is not read on a divergent margin: {live:?}");
        }
    }

    #[test]
    fn the_convergent_continental_params_move_the_answer() {
        // Every field this fixture's margin can reach must move the answer, and the four
        // it structurally cannot are asserted BLIND rather than quietly skipped -- a probe
        // that exercises a stage can be blind to that stage's arguments, and the blindness
        // is worth an assertion of its own.
        //
        // Why those four: `island_arc_*` is multiplied by `oceanic`, which is
        // `(1 - inboard) * (1 - outboard)` and therefore ~0 on a continental margin; and
        // `ridge_*` lives on the divergent early-return, which a convergent margin never
        // takes. `ridge_*` is covered by `the_divergent_params_move_the_answer` below.
        //
        // **`island_arc_m` and `island_arc_width_m` HAVE NO COVERAGE, and this is a stated
        // gap rather than an oversight.** The arc term is multiplied by `oceanic`, so it
        // needs a convergent margin whose BOTH probe points read oceanic; a synthetic
        // two-plate fixture at land fraction 0.02 still did not produce one, and the
        // attempt is recorded rather than deleted. Task 2's survey runs over a real planet
        // where oceanic convergent margins exist, and is where those two fields should be
        // proven live. A slider bound to either would today have no evidence behind it --
        // which is exactly why this slice binds its sliders to the collision fields.
        let live = fields_that_move(&lopsided_world());
        assert_eq!(
            live,
            vec![
                "continent_collision_m",
                "continent_collision_width_m",
                "coastal_uplift_m",
                "coastal_uplift_width_m",
                "continental_blend",
                "collision_asymmetry",
            ],
            "a field that stops moving the answer is a slider that does nothing"
        );
    }

    #[test]
    fn lopsided_world_has_a_genuinely_non_zero_lean() {
        // Guards against the exact failure mode that made an earlier fade test in this
        // port vacuous: a symmetric margin would make the 419 km test below pass for the
        // wrong reason, by making `toward` exactly 0.5 rather than by making both sides
        // of the blend genuinely zero.
        let world = lopsided_world();
        let setting = world.tectonics.setting_at(&world.point, 419_000.0, &world.normal);
        let lean = setting.lean();
        // Measured: 0.679.
        assert!(lean.abs() > 1e-6, "expected a genuinely non-zero lean, got {lean}");
    }

    #[test]
    fn a_close_convergent_margin_contributes_something_non_zero() {
        // Confirms the 419 km test below cannot pass merely because `from_margin`
        // returns zero everywhere -- close in, at least one profile term must be within
        // its width on the near side of the blend.
        let world = lopsided_world();
        let contribution = world.from_margin_for_test(10_000.0);
        assert_ne!(contribution, 0.0, "a margin 10 km away must contribute something");
    }

    #[test]
    fn every_profile_reaches_zero_before_the_range_gate() {
        // MAX_TECTONIC_RANGE_M's own docstring: "Every profile below must reach exactly
        // zero by here, or the gate itself becomes a cliff." At 419 km every bump
        // argument is outside its width, on both sides of the blend, so the sum is
        // exactly zero -- not merely small.
        //
        // This is the regression test for the 419 km mismapping. The obvious form,
        // `signed = distance * lean`, compresses the axis: that is the documented bug --
        // with the historical lean of -0.22, a point 419 km out maps to about -92 km,
        // which is the trench centre, and returns roughly -2597 m instead of zero. It is
        // the reason the code below is shaped the way it is, not this test's own numbers.
        //
        // This fixture's lean is +0.679 (see `lopsided_world_has_a_genuinely_non_zero_lean`
        // above), so the buggy form maps 419 km to about +284,649 m instead and returns
        // about +220.234 m instead of zero -- still wrong, just a different wrong answer.
        let world = lopsided_world();
        let contribution = world.from_margin_for_test(419_000.0);
        assert_eq!(contribution, 0.0, "a margin 419 km away must contribute exactly nothing");
    }

    // ------------------------------------------------------------- the structure field

    /// **The inertness proof the whole slice's safety argument rests on.**
    ///
    /// Ruling 1 says `worldbuilder/terrain/tectonics.py` is the conformance oracle for 157
    /// tests and no default may move. This task added a stacked, asymmetric and
    /// noise-modulated collision term and a ridged-multifractal primitive the Python does
    /// not have -- all of which is safe only if the canonical settings reduce to the exact
    /// expression that was there before, operation for operation.
    ///
    /// So this asserts the reduction directly rather than inferring it from a green suite:
    /// at `canonical()`, `sutures` IS `bump(across_m, continent_collision_width_m)`,
    /// bit-for-bit, over a sweep from -420 km to +420 km at 500 m. Not "close" -- equal by
    /// bits, on both sides of zero, because the asymmetric branch is taken on one side and
    /// not the other and both must reduce.
    #[test]
    fn the_structure_fields_are_inert_at_canonical_settings() {
        let world = lopsided_world();
        let params = TectonicParams::canonical();
        let mut across_m = -MAX_TECTONIC_RANGE_M;
        while across_m <= MAX_TECTONIC_RANGE_M {
            let structured = world.tectonics.sutures(across_m, &world.near, &world.far);
            let plain = bump(across_m, params.continent_collision_width_m);
            assert_eq!(
                structured.to_bits(),
                plain.to_bits(),
                "the canonical collision shape moved at {across_m} m"
            );
            across_m += 500.0;
        }
        // And the fields whose canonical value IS the arithmetic identity say so.
        assert_eq!(params.collision_asymmetry.to_bits(), 1.0f64.to_bits());
        assert_eq!(params.structure_depth.to_bits(), 0.0f64.to_bits());
        assert_eq!(params.suture_count, 1);
        assert_eq!(params.suture_spread_m.to_bits(), 0.0f64.to_bits());
    }

    /// **The along-margin warp is inert at canonical, and this asserts the ARGUMENT rather
    /// than the answer.**
    ///
    /// `the_structure_fields_are_inert_at_canonical_settings` above asserts `sutures`
    /// reduces to `bump`, which is a statement about the profile. The warp does not touch
    /// the profile at all -- it changes the DISTANCE the profile is asked about, which is
    /// the argument, and a probe that exercises a stage can be blind to that stage's
    /// arguments. So this sweeps `from_margin` itself, at 1 km steps to the range gate, on
    /// the `None` path against an explicit `canonical()`, and compares by bits.
    ///
    /// The second half is the discrimination, and without it the first half proves nothing:
    /// at 80 km of warp on the SAME fixture the answer must move, or this test would pass
    /// on a warp that was never wired in at all.
    #[test]
    fn the_margin_warp_is_inert_at_its_canonical_setting() {
        let none = lopsided_world_with(None);
        let explicit = lopsided_world_with(Some(TectonicParams::canonical()));
        let mut warped_params = TectonicParams::canonical();
        warped_params.margin_warp_m = 80_000.0;
        let warped = lopsided_world_with(Some(warped_params));

        let mut moved = false;
        let mut distance_m = 0.0;
        while distance_m <= MAX_TECTONIC_RANGE_M {
            assert_eq!(
                none.from_margin_for_test(distance_m).to_bits(),
                explicit.from_margin_for_test(distance_m).to_bits(),
                "the canonical path moved at {distance_m} m"
            );
            if none.from_margin_for_test(distance_m).to_bits()
                != warped.from_margin_for_test(distance_m).to_bits()
            {
                moved = true;
            }
            distance_m += 1_000.0;
        }
        assert!(moved, "an 80 km warp that changed nothing would make the sweep above vacuous");
        assert_eq!(TectonicParams::canonical().margin_warp_m.to_bits(), 0.0f64.to_bits());
    }

    /// `margin_warp_wavelength_m` is unreachable while the amplitude is zero, and it is
    /// asserted BLIND rather than left out -- the same house rule and the same shape as
    /// `the_structure_wavelength_is_unreachable_until_the_depth_is_turned_up`.
    #[test]
    fn the_margin_warp_wavelength_is_unreachable_until_the_amplitude_is_turned_up() {
        let baseline = lopsided_world();
        let mut alone = TectonicParams::canonical();
        alone.margin_warp_wavelength_m = 250_000.0;
        let blind = lopsided_world_with(Some(alone));

        let mut together = TectonicParams::canonical();
        together.margin_warp_m = 80_000.0;
        together.margin_warp_wavelength_m = 250_000.0;
        let mut other = together;
        other.margin_warp_wavelength_m = 900_000.0;
        let a = lopsided_world_with(Some(together));
        let b = lopsided_world_with(Some(other));

        let mut blind_moved = false;
        let mut wavelength_moved = false;
        let mut distance_m = 0.0;
        while distance_m <= MAX_TECTONIC_RANGE_M {
            if baseline.from_margin_for_test(distance_m).to_bits()
                != blind.from_margin_for_test(distance_m).to_bits()
            {
                blind_moved = true;
            }
            if a.from_margin_for_test(distance_m).to_bits()
                != b.from_margin_for_test(distance_m).to_bits()
            {
                wavelength_moved = true;
            }
            distance_m += 1_000.0;
        }
        assert!(!blind_moved, "the warp wavelength moved the answer with the amplitude at zero");
        assert!(wavelength_moved, "the warp wavelength does nothing with the amplitude turned up");
    }

    /// **The warp offset is the same on both sides of one margin, and that is what stops it
    /// putting a seam down the middle of every belt it bends.**
    ///
    /// `PlateSet::new` stores `normalise(seed_i - seed_j)` for the ordered pair, so the same
    /// margin sampled from its two sides is handed `n` and `-n`. Under that flip `signed`
    /// negates and `signed * n` does not, so the along-margin projection is invariant -- by
    /// construction, not by luck. Asserted by bits rather than argued, and the last two
    /// lines stop it holding for the vacuous reason of the warp being zero everywhere.
    #[test]
    fn the_warp_offset_is_identical_from_either_side_of_the_margin() {
        let mut params = TectonicParams::canonical();
        params.margin_warp_m = 80_000.0;
        let world = lopsided_world_with(Some(params));
        let flipped = Vec3::new(-world.bisector.x, -world.bisector.y, -world.bisector.z);
        for (lat, lon) in [(0.0, 0.0), (12.0, 3.0), (-40.0, 61.0), (70.0, -120.0)] {
            let point = SpherePoint::from_latlon(lat, lon);
            let a = world.tectonics.margin_warp_m_at(&point, &world.bisector);
            let b = world.tectonics.margin_warp_m_at(&point, &flipped);
            assert_eq!(
                a.to_bits(),
                b.to_bits(),
                "the warp disagreed across the margin at {lat},{lon}"
            );
        }
        let live = world
            .tectonics
            .margin_warp_m_at(&SpherePoint::from_latlon(12.0, 3.0), &world.bisector);
        assert_ne!(live, 0.0, "a warp of exactly zero would make this test vacuous");
    }

    /// **The bisector's poles, where `p - (p . n) n` is the zero vector and there is no
    /// direction to keep.** Answered with zero warp rather than a NaN or a skip, and reached
    /// by construction rather than by hoping a sweep lands on it: the poles of the plane
    /// whose normal is `n` ARE `+n` and `-n`.
    #[test]
    fn the_warp_is_zero_at_the_bisectors_poles_rather_than_a_nan() {
        let mut params = TectonicParams::canonical();
        params.margin_warp_m = 80_000.0;
        let world = lopsided_world_with(Some(params));
        let n = world.bisector;
        for pole in [n, Vec3::new(-n.x, -n.y, -n.z)] {
            let point = SpherePoint { vector: pole };
            let warp = world.tectonics.margin_warp_m_at(&point, &n);
            assert_eq!(warp.to_bits(), 0.0f64.to_bits(), "the bisector's pole produced {warp}");
        }
    }

    /// **`collision_reach_m` accounts for the warp, because the range gate truncates rather
    /// than fades.**
    ///
    /// The profile half is asserted against the PROFILE rather than against the formula that
    /// produced it -- how Task 2 wrote this check and for the same reason: past the stated
    /// reach the collision term must be exactly zero on both sides, or a boundary admitting
    /// the block has been told a number the ground does not have.
    #[test]
    fn the_reach_carries_the_warp_amplitude() {
        let mut params = TectonicParams::canonical();
        params.margin_warp_m = 80_000.0;
        assert_eq!(
            params.collision_reach_m().to_bits(),
            (params.continent_collision_width_m + 80_000.0).to_bits()
        );
        // A negative amplitude is a mirrored warp of the same magnitude and reaches exactly
        // as far, so the reach must not read it as zero.
        let mut mirrored = TectonicParams::canonical();
        mirrored.margin_warp_m = -80_000.0;
        assert_eq!(
            mirrored.collision_reach_m().to_bits(),
            params.collision_reach_m().to_bits(),
            "a mirrored warp reported a different reach"
        );
        // The unwarped reach is unchanged, so this is an addition and not a rewrite.
        assert_eq!(
            TectonicParams::canonical().collision_reach_m().to_bits(),
            CONTINENT_COLLISION_WIDTH_M.to_bits()
        );

        // `sutures` at or beyond the reach is exactly zero on both sides, because `bump` is
        // exactly zero at and beyond its width and the warp only shifts where the profile is
        // centred. The warped ARGUMENT is bounded by the reach, which is the claim.
        let world = lopsided_world_with(Some(params));
        let reach = params.collision_reach_m();
        for side in [-1.0f64, 1.0] {
            for extra in [0.0, 1_000.0, 50_000.0] {
                let across = side * (reach + extra);
                let value = world.tectonics.sutures(across, &world.near, &world.far);
                assert_eq!(value, 0.0, "the collision profile carries weight at {across} m");
            }
        }
    }

    /// `asymmetric_bump` at a symmetric ratio is `bump`, by bits, on both sides of the sign
    /// boundary where its branch changes. The discrimination is the second half: at 1.67 it
    /// must NOT be `bump` inboard, or the first half would pass for a vacuous reason.
    #[test]
    fn a_symmetric_asymmetric_bump_is_the_plain_bump() {
        for i in -1000..=1000 {
            let d = f64::from(i) * 500.0;
            assert_eq!(
                asymmetric_bump(d, 400_000.0, COLLISION_SYMMETRIC).to_bits(),
                bump(d, 400_000.0).to_bits(),
                "symmetric asymmetric_bump differs at {d} m"
            );
        }
        let inboard = 100_000.0;
        assert_ne!(
            asymmetric_bump(inboard, 400_000.0, 1.67).to_bits(),
            bump(inboard, 400_000.0).to_bits(),
            "a 1.67 ratio that changed nothing inboard would make the sweep above vacuous"
        );
        assert_eq!(
            asymmetric_bump(-inboard, 400_000.0, 1.67).to_bits(),
            bump(-inboard, 400_000.0).to_bits(),
            "the OUTBOARD side keeps the full width -- that is what bounds the reach"
        );
    }

    /// The published shape, checked as a shape and not as arithmetic.
    ///
    /// Naylor & Sinclair (2008) give alpha_pro = 1.5 deg and alpha_retro = 2.5 deg, a ratio
    /// of 1.67, and a pro-wedge of 115 km against a retro-wedge of 69 km -- 115/69 = 1.67,
    /// the exact inverse. So a profile built from ONE asymmetry ratio must reproduce that
    /// width ratio at half height.
    ///
    /// Measured by bisecting the actual `asymmetric_bump` for its half-height crossing on
    /// each side to 1 m, not by evaluating the closed form -- an algebraic check here would
    /// only prove the test author can rearrange a smoothstep.
    #[test]
    fn the_asymmetry_ratio_reproduces_the_published_flank_width_ratio() {
        let width_m = 115_000.0;
        let ratio = 1.67;
        let half_width = |sign: f64| -> f64 {
            let (mut low, mut high) = (0.0f64, width_m);
            while high - low > 1.0 {
                let middle = 0.5 * (low + high);
                if asymmetric_bump(sign * middle, width_m, ratio) > 0.5 {
                    low = middle;
                } else {
                    high = middle;
                }
            }
            0.5 * (low + high)
        };
        let pro = half_width(-1.0);
        let retro = half_width(1.0);
        let measured = pro / retro;
        assert!(
            (measured - ratio).abs() < 0.01,
            "flank width ratio {measured} is not the published 1.67 (pro {pro} m, retro {retro} m)"
        );
        // The widths themselves follow from whatever `continent_collision_width_m` a caller
        // chooses; only the RATIO is what the paper pins. Half height of a 115 km flank is
        // 57.5 km, and 115/1.67 = 68.9 km gives 34.4 km.
        assert!((pro - 57_500.0).abs() < 100.0, "pro half-width {pro} m");
        assert!((retro - 34_431.0).abs() < 100.0, "retro half-width {retro} m");
    }

    /// **The complement of `FIELDS`, and the test that actually proves the structure is
    /// wired.** A one-ULP flip of a field whose canonical value is 0.0 is not a perturbation
    /// of anything, so those fields come back BLIND from the ULP sweep by design. This
    /// perturbs each by an amount the field genuinely has and requires the answer to move --
    /// same population as the ULP sweep (`lopsided_world`, `from_margin` at 1 km steps from
    /// 0 to `MAX_TECTONIC_RANGE_M`, compared by bits).
    #[test]
    fn each_structure_field_moves_the_answer_at_a_stated_setting() {
        let baseline = lopsided_world();
        let settings: [(&str, fn(&mut TectonicParams)); 5] = [
            ("collision_asymmetry = 1.67", |p| p.collision_asymmetry = 1.67),
            ("suture_count = 3, spread 150 km", |p| {
                p.suture_count = 3;
                p.suture_spread_m = 150_000.0;
            }),
            ("suture_count = 2, spread 90 km", |p| {
                p.suture_count = 2;
                p.suture_spread_m = 90_000.0;
            }),
            ("structure_depth = 0.6", |p| p.structure_depth = 0.6),
            ("margin_warp_m = 80 km", |p| p.margin_warp_m = 80_000.0),
        ];
        for (label, apply) in settings {
            let mut params = TectonicParams::canonical();
            apply(&mut params);
            let probe = lopsided_world_with(Some(params));
            let mut moved = false;
            let mut distance_m = 0.0;
            while distance_m <= MAX_TECTONIC_RANGE_M {
                if baseline.from_margin_for_test(distance_m).to_bits()
                    != probe.from_margin_for_test(distance_m).to_bits()
                {
                    moved = true;
                    break;
                }
                distance_m += 1_000.0;
            }
            assert!(moved, "{label} changed nothing -- a structure knob that does nothing");
        }
    }

    /// `structure_wavelength_m` is the one structure field that is unreachable while
    /// `structure_depth` is zero, and it is asserted BLIND rather than quietly left out --
    /// the house rule that a probe blind to a stage's arguments should assert the blindness.
    #[test]
    fn the_structure_wavelength_is_unreachable_until_the_depth_is_turned_up() {
        let baseline = lopsided_world();
        let mut alone = TectonicParams::canonical();
        alone.structure_wavelength_m = 40_000.0;
        let blind = lopsided_world_with(Some(alone));

        let mut together = TectonicParams::canonical();
        together.structure_depth = 0.6;
        together.structure_wavelength_m = 40_000.0;
        let mut other = together;
        other.structure_wavelength_m = 250_000.0;
        let a = lopsided_world_with(Some(together));
        let b = lopsided_world_with(Some(other));

        let mut blind_moved = false;
        let mut wavelength_moved = false;
        let mut distance_m = 0.0;
        while distance_m <= MAX_TECTONIC_RANGE_M {
            if baseline.from_margin_for_test(distance_m).to_bits()
                != blind.from_margin_for_test(distance_m).to_bits()
            {
                blind_moved = true;
            }
            if a.from_margin_for_test(distance_m).to_bits()
                != b.from_margin_for_test(distance_m).to_bits()
            {
                wavelength_moved = true;
            }
            distance_m += 1_000.0;
        }
        assert!(!blind_moved, "the wavelength must be inert while the depth is zero");
        assert!(wavelength_moved, "the wavelength must bite once the depth is not zero");
    }

    /// The structure multiplier's stated range, swept rather than spot-checked, and its
    /// canonical short-circuit asserted as an exact 1.0.
    #[test]
    fn the_structure_multiplier_stays_within_one_minus_depth_and_one() {
        let world = lopsided_world();
        assert_eq!(
            world.tectonics.structure_at(&world.point).to_bits(),
            1.0f64.to_bits(),
            "at canonical depth the multiplier must be exactly one"
        );
        for depth in [0.2, 0.5, 0.8, 1.0] {
            let mut params = TectonicParams::canonical();
            params.structure_depth = depth;
            let probe = lopsided_world_with(Some(params));
            let mut lat = -85.0;
            while lat <= 85.0 {
                let mut lon = -180.0;
                while lon < 180.0 {
                    let v = probe.tectonics.structure_at(&SpherePoint::from_latlon(lat, lon));
                    assert!(
                        v >= 1.0 - depth - 1e-12 && v <= 1.0 + 1e-12,
                        "structure {v} outside [1-{depth}, 1] at {lat},{lon}"
                    );
                    lon += 11.0;
                }
                lat += 7.0;
            }
        }
    }

    /// A margin's sutures must be the same from both sides of it. The hash takes an ORDERED
    /// pair for exactly this reason, and an unordered one would put a cliff down the middle
    /// of every stacked range.
    #[test]
    fn the_suture_hash_does_not_depend_on_which_plate_is_near() {
        for salt in [STRUCTURE_SALT, SEGMENTATION_SALT] {
            for (a, b) in [(0usize, 1usize), (3, 17), (17, 3), (9, 9)] {
                assert_eq!(
                    pair_fraction(a, b, salt).to_bits(),
                    pair_fraction(b, a, salt).to_bits(),
                    "pair_fraction({a},{b}) flipped with the argument order"
                );
            }
        }
        // Discrimination: different pairs and different salts must actually differ, or the
        // loop above passes because the hash is a constant.
        assert_ne!(
            pair_fraction(0, 1, STRUCTURE_SALT).to_bits(),
            pair_fraction(0, 2, STRUCTURE_SALT).to_bits()
        );
        assert_ne!(
            pair_fraction(0, 1, STRUCTURE_SALT).to_bits(),
            pair_fraction(0, 1, SEGMENTATION_SALT).to_bits()
        );
    }

    /// `collision_reach_m` must be the truth about how far the profile carries, because it
    /// is what a boundary admitting a caller-chosen block has to check against
    /// `MAX_TECTONIC_RANGE_M`. Verified against the profile itself rather than against the
    /// formula: past the stated reach, `sutures` is exactly zero on both sides.
    #[test]
    fn the_stated_collision_reach_is_where_the_profile_actually_stops() {
        assert_eq!(
            TectonicParams::canonical().collision_reach_m().to_bits(),
            CONTINENT_COLLISION_WIDTH_M.to_bits(),
            "canonical reach is the canonical width, exactly"
        );
        for (count, spread_m, width_m, asymmetry) in [
            (1u32, 0.0, 400_000.0, 1.0),
            (2, 90_000.0, 120_000.0, 1.0),
            (3, 150_000.0, 100_000.0, 1.67),
            (4, 60_000.0, 80_000.0, 2.5),
        ] {
            let mut params = TectonicParams::canonical();
            params.suture_count = count;
            params.suture_spread_m = spread_m;
            params.continent_collision_width_m = width_m;
            params.collision_asymmetry = asymmetry;
            let reach = params.collision_reach_m();
            let world = lopsided_world_with(Some(params));
            let mut across_m = reach;
            while across_m <= reach + 200_000.0 {
                assert_eq!(
                    world.tectonics.sutures(across_m, &world.near, &world.far),
                    0.0,
                    "count {count} spread {spread_m} still has a profile at {across_m} m, \
                     past its stated reach of {reach} m"
                );
                assert_eq!(
                    world.tectonics.sutures(-across_m, &world.near, &world.far),
                    0.0,
                    "count {count} spread {spread_m} reaches past {reach} m outboard"
                );
                across_m += 1_000.0;
            }
        }
    }

    /// The one thing a caller can do that turns the range gate into the cliff its own
    /// docstring exists to prevent: choose a block whose reach exceeds it. Stated here as a
    /// measurement rather than enforced, because nothing in this layer clamps -- but a
    /// reader should be able to see that the boundary is real and where it is.
    #[test]
    fn a_stacked_block_can_reach_past_the_range_gate_and_this_says_where() {
        let mut params = TectonicParams::canonical();
        params.suture_count = 4;
        params.suture_spread_m = 150_000.0;
        params.continent_collision_width_m = 100_000.0;
        assert!(
            params.collision_reach_m() > MAX_TECTONIC_RANGE_M,
            "reach {} should exceed the gate at {MAX_TECTONIC_RANGE_M}",
            params.collision_reach_m()
        );
        let mut safe = params;
        safe.suture_count = 3;
        safe.suture_spread_m = 100_000.0;
        assert!(
            safe.collision_reach_m() <= MAX_TECTONIC_RANGE_M,
            "reach {} should sit inside the gate",
            safe.collision_reach_m()
        );
    }

    // ---------------------------------------------- Task 3: the `ranges()` preset ---------

    /// `ranges()` must not be `canonical()` with the serial numbers filed off -- every field
    /// it moves is named in its doc comment with its ground, and every field it does NOT move
    /// must still be canonical. `ReliefParams::hills()`'s own first test is this one.
    #[test]
    fn ranges_only_moves_the_six_named_fields() {
        let ranges = TectonicParams::ranges();
        let canonical = TectonicParams::canonical();

        // Untouched: the coastal, arc and ridge profiles, and the blend. This preset shapes
        // the COLLISION profile; how many margins become ranges is the owner's own slider.
        assert_eq!(ranges.coastal_uplift_m, canonical.coastal_uplift_m);
        assert_eq!(ranges.coastal_uplift_width_m, canonical.coastal_uplift_width_m);
        assert_eq!(ranges.island_arc_m, canonical.island_arc_m);
        assert_eq!(ranges.island_arc_width_m, canonical.island_arc_width_m);
        assert_eq!(ranges.ridge_m, canonical.ridge_m);
        assert_eq!(ranges.ridge_width_m, canonical.ridge_width_m);
        assert_eq!(ranges.continental_blend, canonical.continental_blend);

        // Moved, and each is the value its doc comment argues for.
        assert_eq!(ranges.continent_collision_m, 6_000.0);
        assert_eq!(ranges.continent_collision_width_m, 100_000.0);
        assert_eq!(ranges.collision_asymmetry, 2.0);
        assert_eq!(ranges.suture_count, 2);
        assert_eq!(ranges.suture_spread_m, 100_000.0);
        assert_eq!(ranges.structure_depth, 0.7);
        assert_eq!(ranges.structure_wavelength_m, 80_000.0);

        // And every one of them is genuinely a move -- a "preset" field that happened to
        // equal canonical would be a field this preset does not actually choose.
        for (label, moved, base) in [
            ("continent_collision_m", ranges.continent_collision_m, canonical.continent_collision_m),
            (
                "continent_collision_width_m",
                ranges.continent_collision_width_m,
                canonical.continent_collision_width_m,
            ),
            ("collision_asymmetry", ranges.collision_asymmetry, canonical.collision_asymmetry),
            ("suture_spread_m", ranges.suture_spread_m, canonical.suture_spread_m),
            ("structure_depth", ranges.structure_depth, canonical.structure_depth),
            (
                "structure_wavelength_m",
                ranges.structure_wavelength_m,
                canonical.structure_wavelength_m,
            ),
        ] {
            assert_ne!(moved, base, "{label} is not actually moved by the preset");
        }
        assert_ne!(ranges.suture_count, canonical.suture_count);
    }

    /// **RULING 1: adding a second constructor must not perturb what `None` means.**
    /// `canonical()` is the `None` path's exact equivalent and this preset sits beside it, so
    /// this asserts the neighbour changed nothing -- the same check
    /// `canonical_is_still_bit_identical_to_none_after_adding_hills` makes in `detail.rs`.
    #[test]
    fn canonical_is_still_bit_identical_to_none_after_adding_ranges() {
        let none = lopsided_world();
        let explicit = lopsided_world_with(Some(TectonicParams::canonical()));
        let mut distance_m = -MAX_TECTONIC_RANGE_M;
        while distance_m <= MAX_TECTONIC_RANGE_M {
            assert_eq!(
                none.from_margin_for_test(distance_m).to_bits(),
                explicit.from_margin_for_test(distance_m).to_bits(),
                "None and canonical() disagree at {distance_m} m"
            );
            distance_m += 1_000.0;
        }
    }

    /// **The preset must sit inside the range gate, and by a stated margin.**
    ///
    /// This is the check Task 2's `collision_reach_m` was added for, made of the one block
    /// that ships with sutures turned on. 315 km against a 420 km gate: one suture past the
    /// first, at 100 km, stretched by the `SUTURE_OFFSET_JITTER` ceiling, plus the 100 km
    /// flank, plus **80 km of `margin_warp_m`**. Asserted against the function rather than
    /// the arithmetic, and then the arithmetic is stated so a reader can check the function.
    ///
    /// **Task 5's warp spends 80 km of the preset's gate headroom, and that cost is asserted
    /// rather than mentioned.** The preset was 235 km before it. One of the two neighbours
    /// this test used to walk is now PAST the gate -- a third suture takes the reach to
    /// 450 km -- and it is asserted past rather than quietly dropped, because the fact that
    /// matters is what happens next: `wasm.rs::tectonic_is_admissible` asks
    /// `collision_reach_m` and REFUSES the record, so the boundary turns it away at the door
    /// instead of the range gate truncating it into a cliff mid-profile. `suture_count` is
    /// not on a slider (Task 3 made it a readout precisely because it is jointly constrained),
    /// so nothing the owner can move reaches it; a hand-written query string can, and is
    /// refused.
    #[test]
    fn the_ranges_preset_sits_inside_the_range_gate_with_room() {
        let reach = TectonicParams::ranges().collision_reach_m();
        assert!(
            reach <= MAX_TECTONIC_RANGE_M,
            "the preset reaches {reach} m past the {MAX_TECTONIC_RANGE_M} m gate"
        );
        assert_eq!(
            reach, 315_000.0,
            "one suture at 100 km x 1.35, plus a 100 km flank, plus an 80 km warp"
        );
        // The same preset with the warp switched off is what it was before Task 5, so the
        // 80 km is visibly the warp's and not a change of definition.
        let unwarped = TectonicParams { margin_warp_m: 0.0, ..TectonicParams::ranges() };
        assert_eq!(unwarped.collision_reach_m(), 235_000.0);

        // A wider spread is still inside.
        let mut wider = TectonicParams::ranges();
        wider.suture_spread_m = 150_000.0;
        assert!(wider.collision_reach_m() <= MAX_TECTONIC_RANGE_M);

        // A third suture is not, and that is the warp's cost stated as a number.
        let mut deeper = TectonicParams::ranges();
        deeper.suture_count = 3;
        assert_eq!(deeper.collision_reach_m(), 450_000.0);
        assert!(
            deeper.collision_reach_m() > MAX_TECTONIC_RANGE_M,
            "a third suture used to fit and no longer does -- the boundary must refuse it"
        );
        assert!(
            TectonicParams { margin_warp_m: 0.0, ..deeper }.collision_reach_m()
                <= MAX_TECTONIC_RANGE_M,
            "and it is the warp that spent the headroom, not the sutures"
        );
    }

    /// **The preset must actually move the ground**, and be different from the bare envelope
    /// it sits on -- otherwise the three structure techniques would be a story about a block
    /// nobody can see. Both directions asserted, because "different from canonical" alone
    /// would pass on the envelope change and prove nothing about the structure at all.
    #[test]
    fn the_ranges_preset_differs_from_both_canonical_and_its_own_bare_envelope() {
        let canonical = lopsided_world();
        let preset = lopsided_world_with(Some(TectonicParams::ranges()));
        // The same 6,000 m / 100 km envelope with every structure field back at its inert
        // setting: the blade Task 2's screenshots start from.
        let blade = lopsided_world_with(Some(TectonicParams {
            continent_collision_m: 6_000.0,
            continent_collision_width_m: 100_000.0,
            ..TectonicParams::canonical()
        }));

        let (mut off_canonical, mut off_blade) = (false, false);
        let mut distance_m = -MAX_TECTONIC_RANGE_M;
        while distance_m <= MAX_TECTONIC_RANGE_M {
            let here = preset.from_margin_for_test(distance_m).to_bits();
            if here != canonical.from_margin_for_test(distance_m).to_bits() {
                off_canonical = true;
            }
            if here != blade.from_margin_for_test(distance_m).to_bits() {
                off_blade = true;
            }
            distance_m += 1_000.0;
        }
        assert!(off_canonical, "the preset builds the canonical world");
        assert!(off_blade, "the preset is the bare envelope -- the structure fields do nothing");
    }

    /// A `Tectonics` built with an opt-in [`PeakParams`] block, over the same
    /// `three_plate_set()` the rest of this module's plate-agnostic tests use -- the peak
    /// field never reads `self.plates` or `self.land`, so any world will do.
    fn peaked(params: PeakParams) -> Tectonics {
        let land = Continentality::new(7788, EARTH_RADIUS_M, 0.4);
        Tectonics::with_peaks(three_plate_set(), land, EARTH_RADIUS_M, None, Some(params))
    }

    /// A fixed area-uniform spiral (golden-angle Fibonacci), the same construction
    /// `continentality.rs::spiral` uses, so a measurement over it samples the surface rather
    /// than over-sampling the poles the way an evenly-stepped lat/lon grid does.
    fn area_uniform_spiral(count: usize) -> Vec<SpherePoint> {
        let golden = core::f64::consts::PI * (3.0 - m::sqrt(5.0));
        let n = count as f64; // cast-ok: sample count to float, exact far below 2^53
        (0..count)
            .map(|index| {
                let i = index as f64; // cast-ok: loop counter to float, exact far below 2^53
                let z = 1.0 - 2.0 * (i + 0.5) / n;
                let inner = 1.0 - z * z;
                let ring = m::sqrt(if inner > 0.0 { inner } else { 0.0 });
                let angle = golden * i;
                SpherePoint { vector: Vec3::new(m::cos(angle) * ring, m::sin(angle) * ring, z) }
            })
            .collect()
    }

    /// Real probes' own floor cells that hold a peak at `params`, together with the
    /// [`PeakCandidate`] found there.
    ///
    /// **Why real probes, not an arbitrary lattice coordinate.** A lattice cell reachable
    /// from nowhere on the unit sphere -- near the coordinate origin, where `|cx|,|cy|,|cz|`
    /// are all far below `frequency` -- has a jittered node whose distance from the origin
    /// has nothing to do with `frequency`; it is now filtered out by `peak_of_cell`'s own
    /// `min_fraction < 1.0` check (such a node is always too far from the shell for any
    /// query to reach), but walking real probes' own floor cells is still the direct way to
    /// enumerate the only kind of cell `peak_offset_m` ever actually visits, and is shared by
    /// every test below so there is one way to do it rather than one per test.
    fn peak_candidates_from_real_probes(
        tectonics: &Tectonics,
        params: PeakParams,
        probe_count: i32,
    ) -> Vec<(i64, i64, i64, PeakCandidate)> {
        let frequency = EARTH_RADIUS_M / params.lattice_m;
        let mut found = Vec::new();
        let lat_step = 178.0 / f64::from(probe_count);
        let lon_step = 0.71;
        for i in 0..probe_count {
            let lat = -89.0 + lat_step * f64::from(i); // cast-ok: loop counter, 0..probe_count
            let lon = lon_step * f64::from(i) - 180.0; // cast-ok: loop counter
            let v = SpherePoint::from_latlon(lat, lon).vector;
            // The identical floor-bound-cast `peak_offset_m` itself takes, negated pair and
            // all -- `constraints.md` makes that pattern unconditional, and it is not relaxed
            // for a test helper. In practice `|v.x| <= 1` and `frequency` is a small constant
            // (of order 10-100 at any `lattice_m` this crate ships), so the bound is never
            // actually the branch taken here; the guard exists so this helper cannot silently
            // drift from the real one's safety property if that ever stopped being true.
            let (fx, fy, fz) = (m::floor(v.x * frequency), m::floor(v.y * frequency), m::floor(v.z * frequency));
            if !(fx >= -LATTICE_LIMIT && fx <= LATTICE_LIMIT)
                || !(fy >= -LATTICE_LIMIT && fy <= LATTICE_LIMIT)
                || !(fz >= -LATTICE_LIMIT && fz <= LATTICE_LIMIT)
            {
                continue;
            }
            let (cx, cy, cz) = (
                fx as i64, // cast-ok: floored and bounded against LATTICE_LIMIT on the lines above
                fy as i64, // cast-ok: floored and bounded against LATTICE_LIMIT on the lines above
                fz as i64, // cast-ok: floored and bounded against LATTICE_LIMIT on the lines above
            );
            if let Some(candidate) = tectonics.peak_of_cell(cx, cy, cz, params) {
                found.push((cx, cy, cz, candidate));
            }
        }
        found
    }

    /// A candidate's summit direction and the tallest crest it can ever produce, recomputed
    /// from the geometry `peak_of_cell` already returned in `candidate`.
    ///
    /// **Not on any production path, and deliberately so.** `crest_m` costs a `smooth` call
    /// and an envelope multiply that `peak_offset_m`'s hot loop has no use for -- which is
    /// why `PeakCandidate` itself does not carry either value; see its own doc and the
    /// second review round's minor 5, which found both being computed unconditionally on
    /// every live sample. A test that wants them recomputes from `node`, `share` and
    /// `min_dist_m`, all already exposed, rather than paying for them on the hot path or
    /// re-deriving the jitter maths that produced `node` in the first place.
    fn summit_and_crest(candidate: &PeakCandidate, params: PeakParams) -> (Vec3, f64) {
        let radial = m::sqrt(
            candidate.node.x * candidate.node.x
                + candidate.node.y * candidate.node.y
                + candidate.node.z * candidate.node.z,
        );
        let summit = Vec3 {
            x: candidate.node.x / radial,
            y: candidate.node.y / radial,
            z: candidate.node.z / radial,
        };
        let fraction = candidate.min_dist_m / params.reach_m;
        let crest_m = if fraction < 1.0 {
            params.height_m * (0.45 + 0.55 * candidate.share) * smooth(1.0 - fraction)
        } else {
            0.0
        };
        (summit, crest_m)
    }

    #[test]
    fn a_peak_field_with_no_density_is_exactly_zero() {
        // The inert arm must be an EARLY RETURN, not `+ 0.0`. Adding an exactly-zero
        // offset to a -0.0 seabed yields +0.0 and flips the sign bit, which is how a
        // block that is supposed to change nothing changes something.
        let params = PeakParams { density: 0.0, ..PeakParams::volcanic() };
        let tectonics = peaked(params);
        for (lat, lon) in [(0.0, 0.0), (12.5, -47.5), (-63.25, 128.75), (89.0, 180.0)] {
            let point = SpherePoint::from_latlon(lat, lon);
            let got = tectonics.peak_offset_m(&point, -4600.0);
            assert_eq!(got.to_bits(), 0.0f64.to_bits(), "at {lat},{lon}");
        }
    }

    #[test]
    fn a_peak_needs_deep_water_under_it() {
        // The window is a depth, so it can be stated as one. Shallow seabed gets nothing
        // however dense the field, which is what stops an island erupting on a shelf.
        let tectonics = peaked(PeakParams { density: 1.0, ..PeakParams::volcanic() });
        let mut shallow = 0usize;
        for i in 0..400 {
            let point = SpherePoint::from_latlon(-80.0 + 0.4 * f64::from(i), 17.0); // cast-ok: loop counter, 0..400
            if tectonics.peak_offset_m(&point, -100.0) != 0.0 {
                shallow += 1;
            }
        }
        assert_eq!(shallow, 0, "a 100 m seabed is not deep enough for any peak");
    }

    #[test]
    fn a_dense_field_puts_peaks_in_deep_water_and_a_sparse_one_puts_fewer() {
        // Density and height are independent knobs. This is the property a thresholded
        // fbm cannot give, and the reason this term is built on the lattice.
        let dense = peaked(PeakParams { density: 0.50, ..PeakParams::volcanic() });
        let sparse = peaked(PeakParams { density: 0.05, ..PeakParams::volcanic() });
        let (mut d, mut s) = (0usize, 0usize);
        for i in 0..2_000 {
            let lat = -70.0 + 0.07 * f64::from(i); // cast-ok: loop counter, 0..2000
            let point = SpherePoint::from_latlon(lat, 0.37 * f64::from(i) - 180.0); // cast-ok: loop counter
            if dense.peak_offset_m(&point, -4600.0) > 0.0 { d += 1; }
            if sparse.peak_offset_m(&point, -4600.0) > 0.0 { s += 1; }
        }
        assert!(d > 0, "a half-dense field found no peaks in 2,000 deep probes");
        assert!(d > s, "denser must mean more: dense {d}, sparse {s}");
    }

    #[test]
    fn a_peak_stands_exactly_at_its_own_summit() {
        // Deterministic, not sampled: summits are enumerable, so this needs no luck. An
        // earlier version of this test sampled a spiral of query points and asserted the
        // TALLEST cleared 4,600 m; that shape of test is exactly what let the radial
        // distance bug ship, because "the tallest sample clears the bound" says nothing
        // about whether the FORMULA is right at the points the spiral happens to land on.
        //
        // This calls [`Tectonics::peak_of_cell`], the one statement of a candidate's
        // geometry, rather than re-deriving the jitter maths here -- a test that re-derived
        // it would pass against a field that computed a summit differently.
        //
        // **The crest asserted is `summit_and_crest`'s `crest_m`, not
        // `height_m * (0.45 + 0.55 * share)`.** Those two coincide only when a node sits
        // exactly on the reference shell. In general a node's own radial position falls
        // short of or beyond the shell, and `crest_m` -- built from `min_dist_m`, the real
        // distance between the node and the one point on the sphere closest to it -- is the
        // honest ceiling. Asserting the naive formula here would silently reintroduce the
        // bug this round of review found: it is bit-identical to `crest_m` only when the
        // radial shortfall happens to be zero.
        //
        // **Density is 0.0025 here, not the shipping preset's 0.30.** At 0.30, with
        // `reach_m` now 70% of `lattice_m` (31,500 of 45,000, up from 31% before the ratio
        // was recalibrated), a second candidate is often within reach of the cell under
        // test's own summit, and legitimately outscores it -- correct behaviour
        // (`peak_offset_m` is a max over the whole neighbourhood), and not what this test is
        // pinning down. The useful volume around a node scales with `reach_m^3`, so going
        // from a ratio of 0.31 to 0.70 (a factor of 2.25) raised collision odds at a fixed
        // density by about `2.25^3 =~ 11.4`x -- the density that isolated a candidate before
        // no longer does, which is why this is 0.03 / 11.4, rounded, rather than the old
        // value carried over unchanged. At 0.0025 a competing neighbour is rare enough again
        // that the cell under test is almost always the only one present; the probe count
        // below is raised to compensate for fewer cells passing the existence hash at all.
        // The shipping preset's own height-clearing property is asserted separately, in
        // `the_shipping_preset_still_clears_the_abyss`, using a comparison competition
        // cannot break.
        let params = PeakParams { density: 0.0025, ..PeakParams::volcanic() };
        let tectonics = peaked(params);
        let candidates = peak_candidates_from_real_probes(&tectonics, params, 60_000);
        let mut tallest = 0.0f64;
        for (cx, cy, cz, candidate) in &candidates {
            let (summit, crest_m) = summit_and_crest(candidate, params);
            // `SpherePoint { vector: summit }` directly, not `SpherePoint::from_vector`
            // -- `summit` is already exactly unit length (`node / |node|`), and
            // `from_vector` would renormalise it, introducing a sub-ULP rounding difference
            // between this query's `v` and the `summit` `crest_m` was computed from. The
            // claim here is bit-exactness, so the query must be the identical value.
            let point = SpherePoint { vector: summit };
            let got = tectonics.peak_offset_m(&point, -4600.0);
            assert_eq!(
                got.to_bits(),
                crest_m.to_bits(),
                "at cell ({cx},{cy},{cz}): got {got}, crest_m {crest_m}"
            );
            if got > tallest {
                tallest = got;
            }
        }
        assert!(!candidates.is_empty(), "density 0.0025 found no summit under 60,000 probes");
        assert!(tallest > 4_600.0, "tallest summit {tallest} m does not clear the abyss");
    }

    #[test]
    fn the_shipping_preset_still_clears_the_abyss() {
        // **Pinned at the shipping preset, not only at density 1.0.** The reviewer's sweep
        // found 5,135 m at density 1.0 but only 4,354 m at the OLD preset density (0.06) --
        // the shipping preset never actually cleared 4,600 m over 120,000 km of transect,
        // and a one-sided `> 4_600.0` test at density 1.0 alone would never have shown that,
        // because comparing a lattice hash against 1.0 is trivially true regardless of
        // whether the comparison operator is even correct. Testing at the real preset
        // density exercises that comparison honestly.
        //
        // This does not use `peak_offset_m` at each candidate's own summit for the
        // comparison, because at the shipping density a competing neighbour can
        // legitimately win there (see `a_peak_stands_exactly_at_its_own_summit`). Instead it
        // uses the inequality that holds regardless of competition: the actual field,
        // queried at the tallest candidate's own summit, can only be AT LEAST that
        // candidate's own `crest_m` (every other candidate in range only adds to the max,
        // never subtracts from it), so `crest_m > 4_600.0` on its own already proves the
        // real generator clears the abyss somewhere -- and the live query below confirms it
        // rather than trusting the arithmetic alone.
        let params = PeakParams::volcanic();
        let tectonics = peaked(params);
        let candidates = peak_candidates_from_real_probes(&tectonics, params, 12_000);
        assert!(!candidates.is_empty(), "the shipping preset found no peak under 12,000 probes");

        let mut tallest_crest_m = 0.0f64;
        let mut tallest_cell = (0i64, 0i64, 0i64);
        let mut tallest_summit = Vec3::new(0.0, 0.0, 0.0);
        for (cx, cy, cz, candidate) in &candidates {
            let (summit, crest_m) = summit_and_crest(candidate, params);
            if crest_m > tallest_crest_m {
                tallest_crest_m = crest_m;
                tallest_cell = (*cx, *cy, *cz);
                tallest_summit = summit;
            }
        }
        assert!(
            tallest_crest_m > 4_600.0,
            "the tallest of {} shipping-preset summits crests at {tallest_crest_m} m, short of \
             the abyss",
            candidates.len()
        );

        // See `a_peak_stands_exactly_at_its_own_summit` for why this is a direct field
        // construction rather than `SpherePoint::from_vector`: `tallest_summit` is already
        // exactly unit length, and renormalising it again could round `got` a sub-ULP below
        // `tallest_crest_m`, which would break the `>=` below for a reason that has nothing
        // to do with the property it is checking.
        let point = SpherePoint { vector: tallest_summit };
        let got = tectonics.peak_offset_m(&point, -4600.0);
        let (cx, cy, cz) = tallest_cell;
        assert!(
            got >= tallest_crest_m,
            "at cell ({cx},{cy},{cz}): the live field ({got} m) undercuts its own candidate's \
             crest ({tallest_crest_m} m) -- some OTHER accounting must be wrong, since a \
             neighbour can only ever raise this maximum"
        );
        assert!(got > 4_600.0, "the live field at the tallest summit is {got} m, not > 4,600 m");
    }

    #[test]
    fn a_20_000_point_area_uniform_sweep_measures_the_islanded_share() {
        // **Measured, not predicted.** `VOLCANIC_REACH_M`'s doc derives an island area from
        // the CORRECTED model -- `density * (4/3)*pi * <d(share)^3> / lattice_m^3`, averaging
        // `d(share)^3` over `share` rather than assuming every node reaches the `share = 1`
        // ceiling -- which predicts about 0.50% at the shipping preset (`height_m: 8,000`,
        // `density: 0.11`). This test is the check against the LIVE field, over a population
        // that samples the surface uniformly rather than over-sampling the poles the way a
        // lat/lon grid does.
        //
        // A first version of this test, at the round-4 constants (`height_m: 5,200`,
        // `density: 0.30`), measured 0.035% against a naive (uncorrected) model's ~0.41% --
        // an apparent 12x gap that I first (and wrongly) attributed to the model
        // over-counting a candidate's useful volume geometrically. It does not: the
        // controller re-derived the volume-ratio arithmetic exactly and found no
        // approximation error in it at all. **The missing term was the height envelope.**
        // `0.45 + 0.55*share` means a node clears the abyss only once
        // `share > (4600/height_m - 0.45) / 0.55`, which at `height_m: 5,200` is `share >
        // 0.79` -- so only 21% of nodes could ever surface, and those just past the
        // threshold made vanishingly small islands. The corrected, share-averaged model
        // predicts 0.0347% for that same old trio, which is what the 0.035% actually
        // measured -- three significant figures, not a coincidence. `VOLCANIC_HEIGHT_M`'s
        // doc has the full corrected model and the qualifying-node percentages at both
        // heights.
        //
        // "Standing above the datum offshore" is `peak_offset_m(point, ABYSS_M) > -ABYSS_M`
        // -- the seabed is the standard abyss (-4,600 m) and the surface stands above 0 only
        // once the offset exceeds 4,600 m, exactly the bar every other test in this file
        // calls "clearing the abyss".
        //
        // The band asserted (0.005%-2.0%) is deliberately wide of both the model's ~0.50%
        // and the 0.3-0.8% design target -- hitting the target precisely is Task 7's job, and
        // the constants are stated as provisional in their own doc comments -- but tight
        // enough that a badly broken field (zero islands anywhere, or most of the ocean
        // standing above datum) still fails here rather than only in a later survey.
        let params = PeakParams::volcanic();
        let tectonics = peaked(params);
        let points = area_uniform_spiral(20_000);
        let mut islanded = 0usize;
        for point in &points {
            if tectonics.peak_offset_m(point, ABYSS_M) > -ABYSS_M {
                islanded += 1;
            }
        }
        let islanded_f64 = islanded as f64; // cast-ok: a count of at most 20,000, exact far below 2^53
        let total_f64 = points.len() as f64; // cast-ok: a count of at most 20,000, exact far below 2^53
        let share = islanded_f64 / total_f64 * 100.0;
        assert!(
            share > 0.005 && share < 2.0,
            "{islanded} of {} points ({share:.4}%) stood above datum offshore -- outside the \
             0.005%-2.0% sanity band",
            points.len()
        );
    }

    #[test]
    fn a_ring_off_the_summit_matches_the_stated_profile() {
        // **The catch this test exists for.** Evaluating exactly AT a summit can never
        // exercise the radial-distance bug: a summit's own cell is always inside its own
        // 3x3x3 neighbourhood, chord-to-self is zero either way, and the pre-fix code was
        // wrong about every OTHER point, not that one. This samples a ring at 0.2-0.9 of
        // `reach_m` off each enumerated summit and checks the profile the fixed geometry
        // predicts, which the pre-fix geometry could not have produced except by accident.
        //
        // `min_dist_m` is read straight off `candidate` -- `peak_of_cell` already computed
        // it (to decide whether the candidate is reachable at all) and now returns it for
        // exactly this, so there is one statement of that arithmetic rather than two. Only
        // the summit DIRECTION is recomputed here, by normalising `candidate.node` -- cheap,
        // and not a second copy of the distance arithmetic that produced `min_dist_m` itself.
        //
        // The expected distance from a ring point to the node is then the flat, local
        // approximation `sqrt(min_dist_m^2 + t_m^2)` -- Pythagoras in the plane tangent to
        // the sphere at the summit -- valid because `reach_m` (at most 31,500 m here) is
        // still more than four orders of magnitude below `radius_m` (6,371,000 m), so the
        // curvature correction is negligible against the metre-scale tolerance below.
        //
        // Density is sparse (0.0003) for the reason `a_peak_stands_exactly_at_its_own_summit`
        // gives: a competing neighbour would break the comparison for a reason that has
        // nothing to do with the property under test, and at this preset's now-larger
        // reach-to-lattice ratio (0.70, up from 0.31) a competing neighbour is around 11x
        // likelier at any fixed density. This test samples a RING, not only the summit
        // itself, at up to 0.9 of `reach_m` off it -- closer to a neighbour's own reach than
        // the exact-summit test ever gets -- so it needed a lower density still to stay
        // clean at 40 enumerated summits.
        let params = PeakParams { density: 0.0003, ..PeakParams::volcanic() };
        let tectonics = peaked(params);
        let candidates = peak_candidates_from_real_probes(&tectonics, params, 300_000);
        assert!(!candidates.is_empty(), "density 0.0003 found no summit under 300,000 probes");

        let mut rings_checked = 0usize;
        for (_, _, _, candidate) in candidates.iter().take(40) {
            let min_dist_m = candidate.min_dist_m;
            let radial = m::sqrt(
                candidate.node.x * candidate.node.x
                    + candidate.node.y * candidate.node.y
                    + candidate.node.z * candidate.node.z,
            );
            let summit = Vec3 {
                x: candidate.node.x / radial,
                y: candidate.node.y / radial,
                z: candidate.node.z / radial,
            };
            let summit_point = SpherePoint::from_vector(&summit)
                .expect("a summit direction is a unit vector, never the zero one");
            let frame = TangentFrame::at(&summit_point, EARTH_RADIUS_M);
            for fraction in [0.2, 0.5, 0.9] {
                let t_m = fraction * params.reach_m;
                for bearing_deg in [0.0, 90.0, 180.0, 270.0] {
                    let bearing = m::to_radians(bearing_deg);
                    let ring_point =
                        frame.local_to_sphere(t_m * m::cos(bearing), t_m * m::sin(bearing));
                    let got = tectonics.peak_offset_m(&ring_point, -4600.0);
                    let dist_m = m::sqrt(min_dist_m * min_dist_m + t_m * t_m);
                    let expected = if dist_m < params.reach_m {
                        params.height_m
                            * (0.45 + 0.55 * candidate.share)
                            * smooth(1.0 - dist_m / params.reach_m)
                    } else {
                        0.0
                    };
                    let tolerance = 5.0 + 0.02 * expected;
                    // `.abs()` is banned in this crate; write the comparison out.
                    let diff = if got > expected { got - expected } else { expected - got };
                    assert!(
                        diff <= tolerance,
                        "ring at {fraction}*reach, bearing {bearing_deg}: got {got} m, \
                         expected {expected} m (tolerance {tolerance} m)"
                    );
                    rings_checked += 1;
                }
            }
        }
        assert!(rings_checked > 0, "no ring was actually checked");
    }

    #[test]
    fn no_step_along_a_transect_exceeds_the_analytic_bound() {
        // **The continuity test that would have caught the radial-distance bug.** The
        // reviewer measured a single 20 m step producing a 1,466 m jump in the pre-fix code,
        // against an analytic ceiling of `height_m * 1.5 / reach_m * step_m` -- `1.5` because
        // `d(smooth)/du` peaks at 1.5 at `u = 0.5`, so no correctly-computed profile can ever
        // move faster than that over one step. Derived from the params here, not
        // hard-coded, so a change to `height_m` or `reach_m` keeps this test honest about
        // what bound it is actually checking -- at today's `height_m: 8,000` that bound is
        // 7.62 m at a 20 m step, moved automatically from the 11.14 m it was before
        // `VOLCANIC_HEIGHT_M` was raised.
        //
        // **Three cases, not two.** Density 1.0 maximises how many cell boundaries a single
        // transect crosses, which is where the pre-fix bug fired loudest (a 3,393.8 m worst
        // step at the round-3 constants, since re-measured worse below). The shipping
        // preset is what ships, and its own violations are rarer -- the third review round
        // found the round-2 continuity claim was measured against RETIRED constants and
        // asked for ten times the coverage here, so the shipping arm below walks about
        // 240 transects of 60 km each (roughly 14,400 km) rather than 24. The third case,
        // `reach_m == lattice_m`, sits exactly at the boundary the completeness proof in
        // `peak_offset_m`'s own doc is tightest at -- a brute-force 9x9x9-versus-3x3x3 check
        // over 280,000 probes already found no disagreement there, so this is a guard
        // against a future recalibration landing on that edge, not a suspected bug today.
        //
        // **Two bearings per transect (0 degrees and 45 degrees), not one.** A transect due
        // east crosses lattice cell planes at one fixed angle; a diagonal transect crosses
        // them at a different one, which is what actually varies which axis's floor flips
        // first as the query moves -- the mechanism the whole bug lived in.
        //
        // **What this test deliberately does NOT measure, and where that is measured.** Every
        // step below is taken at a fixed `seabed_m` of -4,600 m, so `peak_depth_window`
        // returns exactly 1.0 throughout and the bound above is purely GEOMETRIC -- the
        // window's own gradient, which is the steeper of the two, is held out on purpose so
        // that a violation here can only mean the profile moved too fast in space.
        // `no_composed_step_exceeds_the_geometric_and_window_bounds_together` below walks the
        // same transects with the seabed the world really has and asserts the composed step of
        // `Tectonics::offset_m` against both terms at once.
        let cases: [(&str, PeakParams, i32, i32); 3] = [
            ("density 1.0", PeakParams { density: 1.0, ..PeakParams::volcanic() }, 24, 3_000),
            ("shipping preset", PeakParams::volcanic(), 240, 3_000),
            (
                "reach_m == lattice_m",
                PeakParams { reach_m: VOLCANIC_LATTICE_M, density: 1.0, ..PeakParams::volcanic() },
                24,
                3_000,
            ),
        ];
        for (label, params, transect_count, steps_per_transect) in cases {
            let tectonics = peaked(params);
            let step_m = 20.0;
            let max_step_m = params.height_m * 1.5 / params.reach_m * step_m;
            let mut transects_walked = 0usize;
            let lat_step = 178.0 / f64::from(transect_count);
            for i in 0..transect_count {
                let lat = -89.0 + lat_step * f64::from(i); // cast-ok: loop counter, 0..transect_count
                let lon = 0.83 * f64::from(i) - 180.0; // cast-ok: loop counter
                for bearing_deg in [0.0, 45.0] {
                    let frame = TangentFrame::at_latlon(lat, lon, EARTH_RADIUS_M);
                    let bearing = m::to_radians(bearing_deg);
                    let (east, north) = (m::cos(bearing), m::sin(bearing));
                    transects_walked += 1;
                    let mut previous = tectonics.peak_offset_m(&frame.origin, -4600.0);
                    for step in 1..steps_per_transect {
                        let t_m = step_m * f64::from(step); // cast-ok: loop counter
                        let point = frame.local_to_sphere(t_m * east, t_m * north);
                        let here = tectonics.peak_offset_m(&point, -4600.0);
                        let delta = if here > previous { here - previous } else { previous - here };
                        assert!(
                            delta <= max_step_m + 1e-6,
                            "{label}: step {step} at lat {lat}, bearing {bearing_deg}: jumped \
                             {delta} m, over the {max_step_m} m analytic bound \
                             ({previous} m -> {here} m)"
                        );
                        previous = here;
                    }
                }
            }
            assert!(transects_walked > 0, "{label}: no transect was actually walked");
        }
    }

    /// The other half of continuity, and the half the final whole-branch review found missing:
    /// **the test above holds `seabed_m` at -4,600 m, so `peak_depth_window` returns exactly
    /// 1.0 at every step and its own gradient is never in the measurement** -- and the window
    /// is the steeper of the two factors. Its ramp spans `min_depth_m - min_depth_m * 0.8`
    /// (500 m at the preset), so its slope reaches `1.5 / span` per metre of depth, which is
    /// `height_m * 1.5 / span` = 24 m of peak per metre of seabed: over three times the 7.62 m
    /// the geometric bound allows for a 20 m step.
    ///
    /// So this walks the same transects with the seabed the world actually has under them --
    /// `base_elevation + offset_m`, exactly the sum `Tectonics::offset_m` hands
    /// `peak_offset_m` and exactly what `Shelf::evaluate` recomputes -- and asserts the
    /// **composed** step of `Tectonics::offset_m` itself, not the peak term against a fiction.
    ///
    /// **The bound, derived from the parameters and from the measured seabed move, never
    /// written down.** Over one step the composed offset moves by
    /// `delta(total) + delta(standing)`, where `total` is bit-for-bit the offset the bare
    /// `Tectonics::new` field produces (the peak term is added, never fed back), so
    ///
    /// ```text
    /// |delta(offset_m)| <= |delta(bare offset_m)|                     <- measured, per step
    ///                    + height_m * 1.5 / reach_m * step_m          <- geometric, per step
    ///                    + height_m * 1.5 / span    * |delta(seabed)| <- the window's own ramp
    /// ```
    ///
    /// Each term is a Lipschitz product of factors that are themselves at most 1: the height
    /// envelope `0.45 + 0.55 * share` never exceeds 1, `smooth`'s derivative peaks at 1.5, and
    /// the window is at most 1, so holding one factor and moving the other gives each line.
    /// The first line is taken from the field itself rather than bounded, because the margin
    /// terms `total` is made of have no analytic Lipschitz constant on this branch and are not
    /// what this test is about -- what is asserted is that **the seamount term adds no more
    /// than its own two-part bound to whatever the pre-existing field already did.**
    ///
    /// **Measured, on this host (rustc 1.98.0, `--release`), at the shipping preset over
    /// 240 transects x 2 bearings x 3,000 steps of 20 m:** the largest composed step is
    /// 26.9865 m against a worst per-step bound of 4,492.2 m, and the largest seamount-only
    /// step is 7.2768 m -- inside the 7.6190 m geometric bound the fixed-seabed test enforces,
    /// so the window's extra allowance is headroom here rather than a cliff being admitted.
    /// The composed bound is genuinely larger than the geometric one (4,492.2 m against
    /// 7.6190 m at its worst step) because a single 20 m step can move the seabed by metres
    /// where the margin terms are steep; that is the number, stated rather than a widening
    /// waved through. 41,131 steps land strictly inside the window's ramp, which is the
    /// coverage claim the fixed-seabed test cannot make at all.
    #[test]
    fn no_composed_step_exceeds_the_geometric_and_window_bounds_together() {
        let cases: [(&str, PeakParams, i32, i32); 2] = [
            ("shipping preset", PeakParams::volcanic(), 240, 3_000),
            ("density 1.0", PeakParams { density: 1.0, ..PeakParams::volcanic() }, 240, 3_000),
        ];
        for (label, params, transect_count, steps_per_transect) in cases {
            // The same fixture `peaked` builds, plus the bare field it differs from by one
            // term. Both must share the `Continentality`, or the baseline would not be the
            // same `total`.
            let land = Continentality::new(7788, EARTH_RADIUS_M, 0.4);
            let bare = Tectonics::new(three_plate_set(), land, EARTH_RADIUS_M, None);
            let peaked =
                Tectonics::with_peaks(three_plate_set(), land, EARTH_RADIUS_M, None, Some(params));

            let step_m = 20.0;
            // Both halves of the bound, from `params` and from `peak_depth_window`'s own
            // arithmetic -- the 0.8 onset factor is read off that function, not guessed.
            let geometric_m = params.height_m * 1.5 / params.reach_m * step_m;
            let span_m = params.min_depth_m - params.min_depth_m * 0.8;
            let per_metre_of_seabed = params.height_m * 1.5 / span_m;

            let mut worst_composed_m = 0.0f64;
            let mut worst_bound_m = 0.0f64;
            let mut worst_peak_step_m = 0.0f64;
            let mut worst_seabed_step_m = 0.0f64;
            let mut steps_on_the_ramp = 0usize;
            let mut steps_walked = 0usize;
            let lat_step = 178.0 / f64::from(transect_count);
            for i in 0..transect_count {
                let lat = -89.0 + lat_step * f64::from(i); // cast-ok: loop counter, 0..transect_count
                let lon = 0.83 * f64::from(i) - 180.0; // cast-ok: loop counter
                for bearing_deg in [0.0, 45.0] {
                    let frame = TangentFrame::at_latlon(lat, lon, EARTH_RADIUS_M);
                    let bearing = m::to_radians(bearing_deg);
                    let (east, north) = (m::cos(bearing), m::sin(bearing));
                    let sample = |point: &SpherePoint| {
                        let plain = bare.offset_m(point);
                        let seabed_m = bare.land.base_elevation(point) + plain;
                        (plain, peaked.offset_m(point), seabed_m)
                    };
                    let mut previous = sample(&frame.origin);
                    for step in 1..steps_per_transect {
                        let t_m = step_m * f64::from(step); // cast-ok: loop counter
                        let point = frame.local_to_sphere(t_m * east, t_m * north);
                        let here = sample(&point);
                        let gap = |a: f64, b: f64| if a > b { a - b } else { b - a };
                        // **Every step is bounded, with nothing skipped.** An earlier draft of
                        // this test skipped steps that crossed the frontier of margin range,
                        // because on one side of it `offset_m` returned before the seamount
                        // term ever ran -- the defect
                        // `the_seamount_term_is_reachable_everywhere_no_matter_where_the_margins_fall`
                        // now forbids. With the term hoisted out of the margin sum there is no
                        // such frontier and no such exemption: 1,439,520 steps per arm, all asserted.
                        let bare_step_m = gap(here.0, previous.0);
                        let composed_step_m = gap(here.1, previous.1);
                        let seabed_step_m = gap(here.2, previous.2);
                        let bound_m = bare_step_m
                            + geometric_m
                            + per_metre_of_seabed * seabed_step_m;
                        assert!(
                            composed_step_m <= bound_m + 1e-6,
                            "{label}: step {step} at lat {lat}, bearing {bearing_deg}: the \
                             composed offset jumped {composed_step_m} m, over the {bound_m} m \
                             bound (bare step {bare_step_m} m + geometric {geometric_m} m + \
                             {per_metre_of_seabed} m/m x {seabed_step_m} m of seabed)"
                        );
                        if composed_step_m > worst_composed_m {
                            worst_composed_m = composed_step_m;
                        }
                        if bound_m > worst_bound_m {
                            worst_bound_m = bound_m;
                        }
                        // The seamount term on its own, against the geometric-only bound the
                        // fixed-seabed test enforces -- reported, not asserted, because with
                        // the seabed moving it is the window that may legitimately add more.
                        let peak_step_m = gap(here.1 - here.0, previous.1 - previous.0);
                        if peak_step_m > worst_peak_step_m {
                            worst_peak_step_m = peak_step_m;
                        }
                        if seabed_step_m > worst_seabed_step_m {
                            worst_seabed_step_m = seabed_step_m;
                        }
                        // Coverage: the window must actually be somewhere on its ramp, or this
                        // test would be the fixed-seabed one again with extra arithmetic.
                        let window = peak_depth_window(-here.2, params.min_depth_m);
                        if window > 0.0 && window < 1.0 {
                            steps_on_the_ramp += 1;
                        }
                        steps_walked += 1;
                        previous = here;
                    }
                }
            }
            println!(
                "{label}: {steps_walked} bounded steps, none skipped; worst composed \
                 {worst_composed_m:.4} m against a worst \
                 bound of {worst_bound_m:.4} m; worst seamount-only step \
                 {worst_peak_step_m:.4} m against the {geometric_m:.4} m geometric bound; worst \
                 seabed move {worst_seabed_step_m:.4} m at {per_metre_of_seabed:.4} m/m; \
                 {steps_on_the_ramp} steps strictly inside the window's ramp"
            );
            assert!(steps_walked > 0, "{label}: no step was actually walked");
            // **The claim that makes this test the one minor 3 asked for.** Without this the
            // test could pass with the window saturated at every step, which is exactly the
            // hole it exists to close.
            assert!(
                steps_on_the_ramp > 1_000,
                "{label}: only {steps_on_the_ramp} steps landed on the window's ramp, so this \
                 walk measured the saturated window the fixed-seabed test already covers"
            );
        }
    }

    /// **A seamount is a property of the seabed, not of how near a plate boundary it happens to
    /// be.** This is the pin for the defect the composed-continuity measurement above uncovered,
    /// and it was written RED against the shape that had the bug.
    ///
    /// `Tectonics::margin_offset_m` returns `0.0` early when `margins_within` comes back empty
    /// or `nearest` is `None` -- its own doc puts that at 69 per cent of the planet, and it
    /// measures 77.16% on this fixture. The seamount term was originally written at the *end* of
    /// that function, after those early returns, so on most of the world it was never evaluated,
    /// and the boundary of the region where it was evaluated was a cliff. Measured against the
    /// old shape, on this fixture (`plates_for(20_260_904, 12)`,
    /// `Continentality::new(20_260_904, 6_371_000, 0.29)`, `PeakParams::volcanic()`, rustc
    /// 1.98.0 `--release`) -- the same world `tests/wasm_exports.rs` uses:
    ///
    /// - **154,314 of 200,000** area-uniform points (77.16%) never reached the term.
    /// - The field would have stood up to **7,824.3 m** at those points; **28,942** of the
    ///   200,000 (14.5%) suppressed more than 100 m.
    /// - Worst single-step jump in `offset_m` at a frontier crossing, over 600 transects x 2
    ///   bearings x 3,000 steps of 20 m: **3,460.23 m** (lat -25.81, bearing 45 degrees, step
    ///   560) -- **454x** the 7.62 m analytic bound, and larger than the 1,466 m cliff whose
    ///   discovery is why this file has a continuity test at all.
    /// - With the term live on both sides of a step the worst step was **6.13 m**, inside the
    ///   bound: the field was continuous and the wiring was not. On the three-plate fixture the
    ///   same measurement read 90.24% suppressed and a 2,749.07 m worst cliff.
    ///
    /// `Tectonics::offset_m` now wraps `margin_offset_m` instead of ending it, so both counts
    /// below are zero. **This test also fixes the island population in place:** hoisting the
    /// term multiplied the area the field can stand on by about 4.4x, which is why
    /// `VOLCANIC_DENSITY` had to be re-surveyed down from 0.36 -- see that constant's own doc
    /// for the re-run sweep and why the maximin margin is now near 10 sigma rather than 2.
    #[test]
    fn the_seamount_term_is_reachable_everywhere_no_matter_where_the_margins_fall() {
        let params = PeakParams::volcanic();
        let plates = crate::generation::plates_for(20_260_904, 12);
        let land = Continentality::new(20_260_904, EARTH_RADIUS_M, 0.29);
        let bare = Tectonics::new(plates.clone(), land, EARTH_RADIUS_M, None);
        let peaked = Tectonics::with_peaks(plates, land, EARTH_RADIUS_M, None, Some(params));

        let live = |point: &SpherePoint| {
            let (nearest, margins) =
                bare.plates.margins_within(point, MAX_TECTONIC_RANGE_M, EARTH_RADIUS_M);
            !margins.is_empty() && nearest.is_some()
        };

        // 1. The term must be reachable everywhere. `peak_offset_m` answers what the field
        //    wants to stand at a point; `offset_m` must not silently decline to ask it.
        //    Asserted as a count of points where the field wants something and the composed
        //    answer does not carry it, which is the observable form of "the term was skipped".
        let points = area_uniform_spiral(200_000);
        let mut suppressed = 0usize;
        let mut worst_suppressed_m = 0.0f64;
        let mut interior_points = 0usize;
        let mut interior_islands = 0usize;
        for point in &points {
            let interior = !live(point);
            if interior {
                interior_points += 1;
            }
            let tectonic = bare.offset_m(point);
            let wanted = peaked.peak_offset_m(point, bare.land.base_elevation(point) + tectonic);
            let carried = peaked.offset_m(point);
            // The field wants something here, so the composed answer must be exactly the sum
            // `offset_m` is specified to return. Compared on the SUM rather than on a
            // difference: `carried - tectonic` is not `standing` once `tectonic` is large
            // enough to round the addition, and 1,615 of these points hit that -- an artefact
            // of the check, not of the field.
            if wanted > 0.0 {
                if interior {
                    interior_islands += 1;
                }
                if carried.to_bits() != (tectonic + wanted).to_bits() {
                    suppressed += 1;
                    if wanted > worst_suppressed_m {
                        worst_suppressed_m = wanted;
                    }
                }
            }
        }
        assert_eq!(
            suppressed,
            0,
            "{suppressed} of {} points never reach the seamount term, suppressing up to \
             {worst_suppressed_m} m of island",
            points.len()
        );
        // And the claim is not vacuous: most of this fixture IS plate interior, and the field
        // really does want to stand seamounts there. Without these two the assertion above
        // would pass on a world with no plate interiors, or with no islands in them -- which is
        // precisely the shape the old code was mistaken for.
        assert!(
            interior_points > points.len() / 2,
            "only {interior_points} of {} points are plate interior; this fixture no longer \
             exercises the early return the defect lived behind",
            points.len()
        );
        assert!(
            interior_islands > 100,
            "the field wants a seamount at only {interior_islands} plate-interior points, so \
             the assertion above could pass without the term being reachable there"
        );

        // 2. And therefore no step ACROSS THE FRONTIER is a cliff -- the 3,460.23 m jump the
        //    old shape produced is now unreproducible. Only frontier-crossing steps are
        //    asserted here, because the ones with margins on both sides are
        //    `no_composed_step_exceeds_the_geometric_and_window_bounds_together`'s job, and it
        //    walks a different fixture. The bound is the same three-part one that test derives:
        //    the bare field's own measured step, plus the geometric term, plus the window's ramp
        //    allowance against the measured seabed move. `crossings` is asserted non-zero, or
        //    this half would be an empty loop.
        let step_m = 20.0;
        let geometric_m = params.height_m * 1.5 / params.reach_m * step_m;
        let span_m = params.min_depth_m - params.min_depth_m * 0.8;
        let per_metre_of_seabed = params.height_m * 1.5 / span_m;
        let mut crossings = 0usize;
        for i in 0..600 {
            let lat = -89.0 + (178.0 / 600.0) * f64::from(i); // cast-ok: loop counter, 0..600
            let lon = 0.611 * f64::from(i) - 180.0; // cast-ok: loop counter
            for bearing_deg in [0.0, 45.0] {
                let frame = TangentFrame::at_latlon(lat, lon, EARTH_RADIUS_M);
                let bearing = m::to_radians(bearing_deg);
                let (east, north) = (m::cos(bearing), m::sin(bearing));
                let sample = |point: &SpherePoint| {
                    let plain = bare.offset_m(point);
                    (
                        peaked.offset_m(point),
                        bare.land.base_elevation(point) + plain,
                        plain,
                        live(point),
                    )
                };
                let mut previous = sample(&frame.origin);
                for step in 1..3_000 {
                    let t_m = step_m * f64::from(step); // cast-ok: loop counter
                    let point = frame.local_to_sphere(t_m * east, t_m * north);
                    let here = sample(&point);
                    if here.3 == previous.3 {
                        previous = here;
                        continue;
                    }
                    crossings += 1;
                    let gap = |a: f64, b: f64| if a > b { a - b } else { b - a };
                    let bound_m = gap(here.2, previous.2)
                        + geometric_m
                        + per_metre_of_seabed * gap(here.1, previous.1);
                    let delta_m = gap(here.0, previous.0);
                    assert!(
                        delta_m <= bound_m + 1e-6,
                        "step {step} at lat {lat}, bearing {bearing_deg}: offset_m jumped \
                         {delta_m} m across the margin-range frontier, over a {bound_m} m \
                         bound ({} m -> {} m)",
                        previous.0,
                        here.0
                    );
                    previous = here;
                }
            }
        }
        assert!(crossings > 0, "no transect crossed the frontier, so nothing was asserted");
    }

    #[test]
    fn ordinary_sampling_finds_a_peak_without_needing_a_summit_hit() {
        // Reachability by ordinary sampling, which the deterministic tests above cannot
        // show on their own: the term must actually turn up for a caller sampling
        // `peak_offset_m` at everyday points, not merely be computable at an enumerated
        // summit. **3,000 m, not the original 1,000** -- this bound went slack when
        // `reach_m`/`lattice_m` was recalibrated for island area (Task 1, round 4): the same
        // 2,000-point spiral that used to find 1,000-and-a-bit now finds 4,582.6 m, because
        // a larger reach-to-lattice ratio makes ordinary sampling land near a peak far more
        // often. 3,000 m keeps real margin below that measured figure without needing a
        // near-summit hit, so it stays robust to an unrelated change in the field or the
        // sampling pattern while still meaning something.
        let tectonics = peaked(PeakParams { density: 1.0, ..PeakParams::volcanic() });
        let mut tallest = 0.0f64;
        for i in 0..2_000 {
            let point = SpherePoint::from_latlon(
                -70.0 + 0.07 * f64::from(i), // cast-ok: loop counter, 0..2000
                0.37 * f64::from(i) - 180.0, // cast-ok: loop counter
            );
            let got = tectonics.peak_offset_m(&point, -4600.0);
            if got > tallest {
                tallest = got;
            }
        }
        assert!(tallest > 3_000.0, "ordinary sampling found nothing over 3,000 m: {tallest} m");
    }

    #[test]
    fn the_peak_field_never_answers_a_nan_or_an_infinity() {
        let params = PeakParams { density: 1.0, ..PeakParams::volcanic() };
        let tectonics = peaked(params);
        for i in 0..3_000 {
            let point = SpherePoint::from_latlon(-89.0 + 0.06 * f64::from(i), 0.0); // cast-ok: loop counter
            for seabed in [-4600.0, -150.0, 0.0, 700.0, f64::NAN] {
                let got = tectonics.peak_offset_m(&point, seabed);
                assert!(got.is_finite(), "peak_offset_m({seabed}) = {got}");
                assert!(got >= 0.0, "a peak may only ever raise ground, got {got}");
                // Minor 3 from the third review round: "pin both ends" was only half done --
                // nothing asserted the field could never exceed its own ceiling. `height_m`
                // is what a node exactly on the reference shell, at share 1, with a
                // saturated window reaches; nothing can stand taller than that.
                assert!(
                    got <= params.height_m,
                    "peak_offset_m({seabed}) = {got}, above its own height_m ({})",
                    params.height_m
                );
            }
        }
    }

    #[test]
    fn height_m_as_infinity_does_not_escape_as_infinity() {
        // Minor 4 from the second review round: `height_m = +-INFINITY` used to escape
        // `peak_offset_m` as `+INFINITY` rather than being refused with the other cheap
        // guards.
        let tectonics = peaked(PeakParams { height_m: f64::INFINITY, ..PeakParams::volcanic() });
        let got = tectonics.peak_offset_m(&SpherePoint::from_latlon(10.0, 20.0), -4600.0);
        assert!(got.is_finite(), "an infinite height_m escaped as {got}");
        let tectonics = peaked(PeakParams { height_m: f64::NEG_INFINITY, ..PeakParams::volcanic() });
        let got = tectonics.peak_offset_m(&SpherePoint::from_latlon(10.0, 20.0), -4600.0);
        assert!(got.is_finite(), "a negative-infinite height_m escaped as {got}");
    }

    #[test]
    fn the_six_salts_in_this_file_are_pairwise_distinct() {
        // A future copy-paste collision fails here rather than silently correlating two
        // fields, in the style of `continentality.rs:863-864`.
        let salts = [
            ("STRUCTURE_SALT", STRUCTURE_SALT),
            ("SEGMENTATION_SALT", SEGMENTATION_SALT),
            ("MARGIN_WARP_SALT", MARGIN_WARP_SALT),
            ("PEAK_SALT", PEAK_SALT),
            ("PEAK_JITTER_SALT", PEAK_JITTER_SALT),
            ("PEAK_HEIGHT_SALT", PEAK_HEIGHT_SALT),
        ];
        for i in 0..salts.len() {
            for j in (i + 1)..salts.len() {
                assert_ne!(
                    salts[i].1, salts[j].1,
                    "{} and {} collide",
                    salts[i].0, salts[j].0
                );
            }
        }
    }

    #[test]
    fn the_six_salts_are_also_distinct_from_the_five_cross_module_ones_the_doc_names() {
        // Minor 4 from the third review round: the doc comment beside `PEAK_SALT` names five
        // cross-module salts a new one must differ from, but the guard above only checked
        // the six in this file. `continentality.rs:863-864` asserts its own cross-module
        // case explicitly rather than folding it into a loop over a single module's
        // constants; this does the same for the three peak salts against all five.
        use crate::continentality::{COAST_NOISE_SALT, NOISE_SALT};
        let peak_salts = [
            ("PEAK_SALT", PEAK_SALT),
            ("PEAK_JITTER_SALT", PEAK_JITTER_SALT),
            ("PEAK_HEIGHT_SALT", PEAK_HEIGHT_SALT),
        ];
        // `detail.rs`'s three are bare literals there (`0x5EABED`, `0x6011E1`, `0x6011E2`),
        // not named constants -- named here instead, the same way
        // `fractal_only_moves_the_amplitude` names `0x5EABED` as a literal rather than
        // inventing a constant `detail.rs` itself does not have.
        let cross_module = [
            ("continentality::NOISE_SALT", NOISE_SALT),
            ("continentality::COAST_NOISE_SALT", COAST_NOISE_SALT),
            ("detail.rs's Detail::with_gully noise salt", 0x5EABEDu64),
            ("detail.rs's Detail::with_gully jitter_x salt", 0x6011E1u64),
            ("detail.rs's Detail::with_gully jitter_y salt", 0x6011E2u64),
        ];
        for (peak_name, peak_salt) in peak_salts {
            for (other_name, other_salt) in cross_module {
                assert_ne!(peak_salt, other_salt, "{peak_name} and {other_name} collide");
            }
        }
    }

    /// The plan's first global constraint, and the whole point of this task: an absent
    /// peak block must produce a BIT-identical world, not merely a close one. `None` and
    /// `Some(PeakParams::canonical())` (density `0.0`) both take the early-return arms in
    /// `offset_m`'s new `match`, so neither should differ from `Tectonics::new` by so much
    /// as a sign bit.
    #[test]
    fn the_tectonic_offset_is_bit_identical_without_a_peak_block() {
        let land = Continentality::new(4242, EARTH_RADIUS_M, LAND_FRACTION);
        let bare = Tectonics::new(three_plate_set(), land, EARTH_RADIUS_M, Some(TectonicParams::canonical()));
        let with_none =
            Tectonics::with_peaks(three_plate_set(), land, EARTH_RADIUS_M, Some(TectonicParams::canonical()), None);
        let inert = Tectonics::with_peaks(
            three_plate_set(),
            land,
            EARTH_RADIUS_M,
            Some(TectonicParams::canonical()),
            Some(PeakParams::canonical()),
        );
        for i in 0..4_000 {
            let point = SpherePoint::from_latlon(
                -89.0 + 0.0445 * f64::from(i), // cast-ok: loop counter, 0..4000
                0.19 * f64::from(i) - 180.0,   // cast-ok: loop counter, 0..4000
            );
            let want = bare.offset_m(&point);
            assert_eq!(with_none.offset_m(&point).to_bits(), want.to_bits(), "None at probe {i}");
            assert_eq!(inert.offset_m(&point).to_bits(), want.to_bits(), "canonical at probe {i}");
        }
    }

    /// The other half of the plan's claim: an active peak block must raise ground and never
    /// lower it. `offset_m` only ever adds `standing` when it is strictly positive, so this
    /// is really a test that the wiring in `offset_m` did not accidentally let a peak
    /// subtract or that the seabed it is evaluated against was assembled wrong.
    #[test]
    fn a_peak_block_raises_the_offset_where_the_water_is_deep() {
        let land = Continentality::new(4242, EARTH_RADIUS_M, LAND_FRACTION);
        let bare = Tectonics::new(three_plate_set(), land, EARTH_RADIUS_M, Some(TectonicParams::canonical()));
        let peaked = Tectonics::with_peaks(
            three_plate_set(),
            land,
            EARTH_RADIUS_M,
            Some(TectonicParams::canonical()),
            Some(PeakParams { density: 0.35, ..PeakParams::volcanic() }),
        );
        let mut raised = 0usize;
        let mut lowered = 0usize;
        for i in 0..6_000 {
            let point = SpherePoint::from_latlon(
                -89.0 + 0.0297 * f64::from(i), // cast-ok: loop counter, 0..6000
                0.41 * f64::from(i) - 180.0,   // cast-ok: loop counter, 0..6000
            );
            let before = bare.offset_m(&point);
            let after = peaked.offset_m(&point);
            if after > before {
                raised += 1;
            }
            if after < before {
                lowered += 1;
            }
        }
        assert!(raised > 0, "a peak block raised nothing over 6,000 probes");
        assert_eq!(lowered, 0, "a peak may only ever raise ground; {lowered} probes fell");
    }

    /// Task 2's own addition to the bit-identity claim above: not just that the INERT path
    /// produces the same number, but that it does not pay for `base_elevation`'s fBm to get
    /// there. Can't observe a call directly, so this follows
    /// `continentality.rs`'s `the_coast_lattice_is_not_read_on_the_canonical_path`
    /// (`continentality.rs:774`): swap in a `Continentality` whose `base_elevation` answers
    /// differently -- here, a different `land_fraction`, which moves `shore`/`spread` in
    /// calibration but leaves `world_seed` (and therefore every `Noise` field `Tectonics`
    /// salts from it) untouched -- and show the inert path does not notice, while an active
    /// one does.
    ///
    /// **Widened by the final whole-branch review's minor 9 to all five inert cases.** It
    /// originally covered `None` and a zero-density `Some`, which were the only two
    /// `offset_m` gated on; a block inert because of `height_m`, `reach_m` or a violated
    /// `reach_m <= lattice_m` still paid the fBm. `Tectonics::with_peaks` now decides
    /// inertness once, through `peak_block_is_live`, and the loop below is what keeps it
    /// decided: five blocks, each inert for a different reason, each required to be blind to
    /// the land under it *and* bit-identical to no block at all.
    #[test]
    fn the_inert_peak_path_does_not_read_base_elevation() {
        let plates = three_plate_set();
        let land_a = Continentality::new(4242, EARTH_RADIUS_M, 0.2);
        let land_b = Continentality::new(4242, EARTH_RADIUS_M, 0.8);

        let points = area_uniform_spiral(1_000);

        // Inert (no block at all): the two lands must not be distinguishable through
        // `offset_m`, because `base_elevation` is never reached to tell them apart.
        let none_a = Tectonics::new(plates.clone(), land_a, EARTH_RADIUS_M, Some(TectonicParams::canonical()));
        let none_b = Tectonics::new(plates.clone(), land_b, EARTH_RADIUS_M, Some(TectonicParams::canonical()));
        for (index, point) in points.iter().enumerate() {
            assert_eq!(
                none_a.offset_m(point).to_bits(),
                none_b.offset_m(point).to_bits(),
                "inert path differed at spiral index {index}"
            );
        }

        // Inert with a `Some` block, on **every** reason a block can be inert, not only the
        // zero-density one. Minor 9 of the final whole-branch review: `offset_m` gated on
        // `density == 0.0` alone, so a block made inert any other way still paid
        // `base_elevation`'s fBm at every sample. `with_peaks` now decides all five through
        // `peak_block_is_live`, and this is what holds it there -- each of these five blocks
        // must be as blind to the land under it as no block at all is.
        //
        // `height_m: 0.0` is admissible on purpose (`WB_MIN_PEAK_HEIGHT_M` is 0.0) and so is
        // the whole of `PeakParams::canonical()`; the other three are blocks the ABI would
        // refuse but the engine must still survive, since `Tectonics::with_peaks` is public
        // and `peak_offset_m`'s guards are the engine's own, not the boundary's.
        let inert_blocks: [(&str, PeakParams); 5] = [
            ("density 0.0", PeakParams::canonical()),
            ("height_m 0.0", PeakParams { height_m: 0.0, ..PeakParams::volcanic() }),
            ("height_m NaN", PeakParams { height_m: f64::NAN, ..PeakParams::volcanic() }),
            ("reach_m 0.0", PeakParams { reach_m: 0.0, ..PeakParams::volcanic() }),
            (
                "reach_m past lattice_m",
                PeakParams {
                    reach_m: VOLCANIC_LATTICE_M + 1.0,
                    ..PeakParams::volcanic()
                },
            ),
        ];
        for (label, block) in inert_blocks {
            let zero_a = Tectonics::with_peaks(
                plates.clone(),
                land_a,
                EARTH_RADIUS_M,
                Some(TectonicParams::canonical()),
                Some(block),
            );
            let zero_b = Tectonics::with_peaks(
                plates.clone(),
                land_b,
                EARTH_RADIUS_M,
                Some(TectonicParams::canonical()),
                Some(block),
            );
            for (index, point) in points.iter().enumerate() {
                assert_eq!(
                    zero_a.offset_m(point).to_bits(),
                    zero_b.offset_m(point).to_bits(),
                    "{label}: inert path differed at spiral index {index}"
                );
                // And the same block is bit-identical to no block at all, which is what says
                // the two lands agree because nothing was read rather than because something
                // was read consistently.
                assert_eq!(
                    zero_a.offset_m(point).to_bits(),
                    none_a.offset_m(point).to_bits(),
                    "{label}: differed from an absent block at spiral index {index}"
                );
            }
        }

        // And the difference between the two lands is real: on an ACTIVE peak block, the
        // same swap does move the answer at some of these points, so the two assertions
        // above pass because the inert path never reads `base_elevation`, not because
        // swapping `land_fraction` is inert.
        let active_a = Tectonics::with_peaks(
            plates.clone(),
            land_a,
            EARTH_RADIUS_M,
            Some(TectonicParams::canonical()),
            Some(PeakParams { density: 1.0, ..PeakParams::volcanic() }),
        );
        let active_b = Tectonics::with_peaks(
            plates,
            land_b,
            EARTH_RADIUS_M,
            Some(TectonicParams::canonical()),
            Some(PeakParams { density: 1.0, ..PeakParams::volcanic() }),
        );
        let moved = points
            .iter()
            .filter(|p| active_a.offset_m(p).to_bits() != active_b.offset_m(p).to_bits())
            .count();
        assert!(moved > 10, "the land_fraction swap moved only {moved} of 1,000 active points");
    }
}
