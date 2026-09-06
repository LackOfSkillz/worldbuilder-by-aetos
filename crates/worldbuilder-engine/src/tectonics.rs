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
use crate::detmath as m;
use crate::kinematics::{motion_between, ACROSS_ENOUGH};
use crate::noise::Noise;
use crate::plates::{Plate, PlateSet};
use crate::sphere::SpherePoint;
use crate::tangent::TangentFrame;
use crate::vectors::Vec3;

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

/// Salts, so the three fields this module samples are independent of each other and of
/// `continentality`'s. ASCII, in the house style of `stream.rs`'s jitter salts.
pub const STRUCTURE_SALT: u64 = 0x7374_7275_6374_7572; // "structur"
pub const SEGMENTATION_SALT: u64 = 0x7365_676D_656E_7473; // "segments"

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
    /// Six fields move. The coastal, arc and ridge profiles and `continental_blend` stay at
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
    /// zero and symmetric flanks.
    pub fn collision_reach_m(&self) -> f64 {
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
        furthest + self.continent_collision_width_m
    }
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
}

impl Tectonics {
    /// `params`: `None` for canonical -- today's nine constants, byte-for-byte what
    /// `TectonicParams::canonical()` returns -- or `Some(params)` for a caller-chosen
    /// block. Resolved once here rather than re-checked per sample, so `from_margin` never
    /// sees the `Option` at all, exactly as `Detail::new` resolves `ReliefParams`.
    pub fn new(
        plates: PlateSet,
        land: Continentality,
        radius_m: f64,
        params: Option<TectonicParams>,
    ) -> Self {
        let params = params.unwrap_or_else(TectonicParams::canonical);
        // `Continentality` kept the world seed for exactly this -- see its `world_seed`
        // field. Nothing else in the engine gives `Tectonics` a seed, and adding one to
        // this signature would have moved six call sites including two conformance
        // bindings for a value the layer next door already holds.
        let world_seed = land.world_seed();
        let structure = Noise::new(world_seed, STRUCTURE_SALT);
        let segmentation = Noise::new(world_seed, SEGMENTATION_SALT);
        Self { plates, land, radius_m, params, structure, segmentation }
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
    pub fn offset_m(&self, point: &SpherePoint) -> f64 {
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
            total += margin.weight
                * self.from_margin(point, &near, &margin.other, margin.distance_m, &normal);
        }
        total
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

    /// One margin's contribution to the ground here.
    ///
    /// Args:
    /// point: Where.
    /// near: The plate the point is on.
    /// far: The plate across this margin.
    /// distance_m: How far the margin is.
    /// normal: Across it, tangent to the surface, pointing towards `near`.
    ///
    /// Returns metres, which may be zero, and usually is.
    fn from_margin(
        &self,
        point: &SpherePoint,
        near: &Plate,
        far: &Plate,
        distance_m: f64,
        normal: &Vec3,
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

        let profile = |across_m: f64| -> f64 {
            let collided = params.continent_collision_m
                * structure
                * self.sutures(across_m, near, far);
            // Only the collision term is stacked and modulated. The trench, the arc and
            // the coastal rise stay anchored to the margin itself, which is where they
            // belong: a trench IS the plate boundary.
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
        strength * (toward * profile(distance_m) + (1.0 - toward) * profile(-distance_m))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::continentality::LAND_FRACTION;
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
    }

    impl LopsidedWorld {
        fn from_margin_for_test(&self, distance_m: f64) -> f64 {
            self.tectonics.from_margin(&self.point, &self.near, &self.far, distance_m, &self.normal)
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

        LopsidedWorld { tectonics, point, near, far, normal }
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
    const FIELDS: [(&str, fn(&mut TectonicParams)); 13] = [
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
        LopsidedWorld { tectonics, point, near, far, normal }
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
        let settings: [(&str, fn(&mut TectonicParams)); 4] = [
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
    /// that ships with sutures turned on. 235 km against a 420 km gate: one suture past the
    /// first, at 100 km, stretched by the `SUTURE_OFFSET_JITTER` ceiling, plus the 100 km
    /// flank. Asserted against the function rather than the arithmetic, and then the
    /// arithmetic is stated so a reader can check the function.
    #[test]
    fn the_ranges_preset_sits_inside_the_range_gate_with_room() {
        let reach = TectonicParams::ranges().collision_reach_m();
        assert!(
            reach <= MAX_TECTONIC_RANGE_M,
            "the preset reaches {reach} m past the {MAX_TECTONIC_RANGE_M} m gate"
        );
        assert_eq!(reach, 235_000.0, "one suture at 100 km x 1.35, plus a 100 km flank");
        // The measured neighbours this preset was chosen over, both still inside -- so the
        // owner can move any slider off the preset without walking into the cliff.
        let mut wider = TectonicParams::ranges();
        wider.suture_spread_m = 150_000.0;
        assert!(wider.collision_reach_m() <= MAX_TECTONIC_RANGE_M);
        let mut deeper = TectonicParams::ranges();
        deeper.suture_count = 3;
        assert!(deeper.collision_reach_m() <= MAX_TECTONIC_RANGE_M);
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
}
