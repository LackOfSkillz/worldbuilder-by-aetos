//! Texture, and only texture.
//!
//! Ported from `worldbuilder/terrain/detail.py`. Detail roughens ground that structure has
//! already decided; it does not decide anything itself. This module carries the module
//! constants, the `smooth` helper, and the band table that plans the octaves — the noise
//! sampling and evaluation come in a later task.

use crate::detmath as m;
use crate::noise::Noise;
use crate::sphere::SpherePoint;
use crate::steer::Steer;
use crate::tangent::TangentFrame;
use crate::vectors::Vec3;

/// The finest ground truth this generator has. Physics sees detail down to here and no
/// further; there is no finer octave to add without changing what canonical means.
pub const CANONICAL_WAVELENGTH_M: f64 = 250.0;

/// The coarsest detail band. Above this, structure has the say.
pub const COARSEST_WAVELENGTH_M: f64 = 20_000.0;

/// How many multiples of the sample spacing an octave's wavelength must be before it is
/// worth drawing, and where it has faded out entirely.
///
/// Nyquist puts the floor at two, but barely representable is not usefully representable -
/// an octave at twice the sample spacing is four points a cycle, which reads as noise
/// rather than as landform and aliases while doing it. It fades between two and four.
pub const BARELY_M: f64 = 2.0;
pub const CLEARLY_M: f64 = 4.0;

/// How rough the ground is, in metres, in each setting. Every one of these is far below
/// the structural relief it decorates: a shelf falls a hundred and fifty metres over
/// eighty kilometres, so fifteen metres of roughness on it is texture and not topography.
pub const ABYSSAL_M: f64 = 55.0;
pub const SHELF_M: f64 = 15.0;
pub const COAST_M: f64 = 35.0;
pub const INTERIOR_M: f64 = 80.0;
pub const MOUNTAIN_M: f64 = 150.0;

/// The ten values that decide how rough a world is, broken out so a caller who wants a
/// different world can ask for one without touching what "canonical" means.
///
/// `Detail::new` takes `Option<ReliefParams>`, following the house pattern already on
/// `Surface::new`'s `features: Option<FeatureInput>`: `None` is the canonical path, not an
/// implicit `Default::default()` -- this codebase deliberately rejects defaults nobody
/// chose (see `stream.rs::BuildParams`). `ReliefParams::canonical()` is the only way to
/// get today's nine constants (plus the coarsest wavelength, kept as the other end of the
/// same schedule) as a value, and every field is commented with the constant or literal it
/// was measured from.
///
/// **This task adds the block. It does not change what it defaults to.** A changed
/// default here is a change to `worldbuilder/terrain/detail.py`'s constants, which the
/// conformance suite in `tests/test_conformance.py` treats as ground truth.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReliefParams {
    /// `CANONICAL_WAVELENGTH_M`: the finest octave; the loop bound.
    pub canonical_wavelength_m: f64,
    /// `COARSEST_WAVELENGTH_M`: the coarsest detail band.
    pub coarsest_wavelength_m: f64,
    /// `ABYSSAL_M`: roughness in deep water.
    pub abyssal_m: f64,
    /// `SHELF_M`: roughness over the continental shelf.
    pub shelf_m: f64,
    /// `COAST_M`: roughness right at the shoreline.
    pub coast_m: f64,
    /// `INTERIOR_M`: roughness on ordinary land.
    pub interior_m: f64,
    /// `MOUNTAIN_M`: roughness at the tops.
    pub mountain_m: f64,
    /// The `0.7` in `amplitude_m`'s quieting term: how much deliberate deep structure
    /// can suppress roughness, at its strongest.
    pub quieting_strength: f64,
    /// The `1200.0` in `amplitude_m`'s quieting term: the tectonic-offset scale over
    /// which the quieting ramps in.
    pub quieting_scale_m: f64,
    /// The `0.5` in `plan`'s `share *= 0.5`: how much amplitude each octave keeps of the
    /// one before it.
    pub octave_persistence: f64,
}

impl ReliefParams {
    /// Exactly today's nine values (plus the coarsest wavelength), each traceable to the
    /// module constant or literal it was measured from. Building a `Detail` with `None`
    /// and one with `Some(ReliefParams::canonical())` must produce bit-identical output --
    /// see `surface.rs`'s `relief_none_matches_relief_some_canonical`.
    pub fn canonical() -> Self {
        Self {
            canonical_wavelength_m: CANONICAL_WAVELENGTH_M,
            coarsest_wavelength_m: COARSEST_WAVELENGTH_M,
            abyssal_m: ABYSSAL_M,
            shelf_m: SHELF_M,
            coast_m: COAST_M,
            interior_m: INTERIOR_M,
            mountain_m: MOUNTAIN_M,
            quieting_strength: 0.7,
            quieting_scale_m: 1200.0,
            octave_persistence: 0.5,
        }
    }

    /// A named, opt-in preset -- Task 3 of the relief-amplitude slice
    /// (`.superpowers/sdd/2026-09-05-slice-relief-amplitude/progress.md`), chosen from
    /// Task 2's measured tables, not a new default. **`canonical()` above is untouched by
    /// this constructor and stays the `None` path -- Ruling 1.**
    ///
    /// Three fields move, each on stated, external ground:
    ///
    /// - **`octave_persistence: 0.65`.** Real terrain's measured Hurst exponent is
    ///   H ~= 0.6-0.71 (Gagnon, Lovejoy & Schertzer, over four DEMs). At this schedule's own
    ///   lacunarity of 2.0, `H = ln(1/persistence) / ln(2)`, so persistence 0.65 gives
    ///   H = 0.6215 -- inside that band. This is the only swept persistence value that
    ///   lands there; 0.5 (H=1.0) and 0.71/0.75 (H=0.49/0.42) both fall outside it. Task
    ///   2 also found persistence is not the scale-neutral knob an infinite-series argument
    ///   would suggest -- this schedule is a finite seven octaves -- so 0.65 is chosen for
    ///   matching real terrain's measured roughness, not for being scale-invariant.
    /// - **`quieting_strength: -0.7`.** The exact negation of `canonical()`'s `0.7`, not a
    ///   new magnitude picked from the sweep's extreme corner. Two grounds for the sign
    ///   flip: Task 2 measured that on the peak population (real uplift, large
    ///   `tectonic_m`) inverting the sign more than doubles relief (median 4.02 -> 5.19 m,
    ///   max 10.32 -> 19.67 m at today's other settings) while on generic land it is
    ///   inert -- so the flip is choosing where tectonics are already large, not
    ///   manufacturing relief from nothing. And Outerra's published terrain generator does
    ///   the same thing in spirit: its amplitude *rises* with slope and curvature instead
    ///   of being suppressed by them ("the amplitude of noise is modulated by slope --
    ///   flat areas have less noise, while the steeper get more"; curvature raises it too,
    ///   "depending also on elevation", to fix flat mountaintops). This engine keys on
    ///   tectonic magnitude rather than slope or curvature, so the match is an
    ///   approximation of a shipped mechanism, not a reproduction of it -- matching
    ///   Outerra's actual inputs would be a mechanism change, out of scope here.
    /// - **`mountain_m: MOUNTAIN_M * 4.0` (600.0).** Grounded on Hammond's landform
    ///   classification: hills are 80-160 m of local relief over a 2 km run; low mountains
    ///   start at 300 m. Combined with the two choices above, Task 2's sweep measured this
    ///   exact combination (`mountain_m` x4, `quieting_strength=-0.7`,
    ///   `octave_persistence=0.65`) giving the peak population a relief max of 124.25 m --
    ///   inside the hills band, with headroom to the 160 m ceiling and nowhere near the
    ///   300 m mountain floor. `mountain_m` x3 (450.0) also reaches the hills band (93.37 m
    ///   max) but with less of the band filled; x4 was chosen as the multiplier, of the
    ///   ones Task 2 swept, whose measured result best fills a published target band rather
    ///   than for being the largest number tried.
    ///
    /// **Ruling 6, and what this preset does not attempt:** the roughness spectrum alone
    /// cannot make mountains -- Task 2's most extreme corner (this same `mountain_m` x4,
    /// `quieting_strength=-0.7`, but `octave_persistence=0.75`, outside real terrain's
    /// Hurst range) topped out at 161.34 m relief on the peak population (82.10 m on land;
    /// `progress.md`'s own Ruling 6 prose has these two swapped, see task-3-report.md) and
    /// never reached Hammond's low-mountains band. Mountain height is tectonic (Ruling 4, a
    /// separate slice); this preset is the best hills the roughness spectrum can produce,
    /// not mountains it cannot.
    pub fn hills() -> Self {
        Self { mountain_m: MOUNTAIN_M * 4.0, quieting_strength: -0.7, octave_persistence: 0.65, ..Self::canonical() }
    }
}

/// How far a pivot may be nudged off its lattice node, in cells.
///
/// A module constant rather than an eleventh field of [`GullyParams`], because it is not a
/// dial: it is what stops the pivot lattice reading as a lattice. At zero the kernel is a
/// regular grid of wave sources and the eye finds the grid immediately; at half a cell the
/// sources are anywhere in their own cell and the grid is gone. The published kernel this
/// reimplements uses the same half-cell figure for the same reason, and it is one of the few
/// of its numbers that is a statement about lattices rather than about its own terrain.
///
/// **The jitter moves the PHASE, never the WEIGHT.** A jittered pivot could leave the
/// eight-corner window, and then the window would truncate a non-zero contribution and the
/// height field would step along a lattice plane. The weight is therefore taken from the
/// unjittered node, which is what makes
/// `the_gully_term_is_continuous_across_a_pivot_cell_boundary` hold exactly rather than
/// approximately.
pub const GULLY_PIVOT_JITTER_CELLS: f64 = 0.5;

/// Two turns of a circle, for the plane-wave phase.
const TAU: f64 = 2.0 * std::f64::consts::PI;

/// The drainage texture: what turns a smooth flank into a branching set of gullies.
///
/// **Opt-in, and `None` is canonical -- Ruling 1.** The fifth block of this kind, after
/// `ReliefParams` here, `TectonicParams`, `CoastParams` and `ClimateParams`; the convention
/// is theirs and nothing new is invented. [`GullyParams::canonical()`] carries an
/// `amplitude_m` of exactly zero, and `Detail::gully_offset_m` returns before it can add
/// anything -- so a world built with `Some(canonical())` is bit-identical to one built with
/// `None`, held by an early return rather than by trusting `x + 0.0 == x`, which is false
/// for `x = -0.0`.
///
/// # The mechanism, and the one number without which it is a no-op
///
/// Stripe direction is the steering gradient **rotated ninety degrees**, so the wave's
/// phase varies along the contour and its crests run down the fall line: gullies, not
/// terraces. Stripe frequency scales with slope, which is what makes steep faces finely
/// dissected and leaves flat ground alone.
///
/// The published form leaves that direction vector *unnormalised* and lets the slope itself
/// set the frequency. **On this generator that is a planet-wide no-op**, and it was measured
/// before any of this was written rather than discovered in a render:
/// `.superpowers/sdd/notes/gradient-probe.md` section 3 puts the steepest decile of high
/// ground at **0.0052 m/m -- 0.30 degrees**, against real mountain flanks at 20-35 degrees,
/// and `viewer/public/app/relief.js:75` independently records "the steepest texel found
/// anywhere in the probe set is 1.9 deg". Handed 0.005 where the technique assumes ~0.5,
/// every cosine of a direction-dot-displacement is the cosine of nearly nothing -- a
/// constant, everywhere, which is the kernel's documented flat-ground fade applied to the
/// whole planet.
///
/// So [`GullyParams::slope_reference`] exists, and it is a property of this generator rather
/// than of the technique. See its own doc for the population it was measured on. This
/// project has shipped a very careful no-op before -- three colour blends in `relief.js` were
/// dead against this terrain until a slice measured them, and nine of thirty-three palette
/// colours were unreachable because a noise field's real standard deviation was a fifth of
/// its nominal one -- which is why the scale is a named, bounded field and not a literal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GullyParams {
    /// How deep the drainage texture cuts, in metres, at full gate. Zero is off, and off is
    /// canonical.
    pub amplitude_m: f64,
    /// The pivot lattice's pitch in metres -- the spacing of the wave sources, and the
    /// stripe wavelength at exactly one stripe per cell.
    pub cell_m: f64,
    /// **The slope scale.** The steering slope, in m/m, at which the kernel reaches its
    /// nominal [`stripes_per_cell`](Self::stripes_per_cell) and its full erosion energy.
    /// Below it the pattern coarsens and fades; above it the frequency keeps rising until
    /// [`max_stripes_per_cell`](Self::max_stripes_per_cell) caps it.
    ///
    /// Measured, not chosen: see `.superpowers/sdd/notes/gully-kernel.md` for the three
    /// worlds, the population and the quantiles.
    pub slope_reference: f64,
    /// Stripes across one cell at exactly the reference slope.
    pub stripes_per_cell: f64,
    /// The ceiling on stripes per cell, so that a steep site cannot ask for a wavelength
    /// finer than the field can carry.
    pub max_stripes_per_cell: f64,
    /// The crest-versus-floor asymmetry, as the exponent of the edge-shaping curve. Below
    /// one it sharpens crests and broadens floors, which is what leaves snow on ridge lines
    /// instead of blanketing the flank; at exactly one the term is symmetric and the
    /// asymmetry is off.
    pub crest_sharpness: f64,
    /// Where the gate opens, in metres of structural ground.
    pub gate_elevation_m: f64,
    /// Over how many metres it opens.
    pub gate_elevation_span_m: f64,
    /// The fraction of full energy kept on dead-flat gated ground, so that a valley floor
    /// inside the gate is quieted rather than switched off, which would put a visible edge
    /// along a contour.
    pub flat_energy_floor: f64,
    /// The steering lattice's spacing **and** its central-differencing step, in metres. Both
    /// at once on purpose: `gradient-probe.md` section 2.4 measured `grad(structural_m)`'s
    /// direction invariant to the step to a p95 of 0.02 degrees across a 26x range, so a step
    /// finer than the lattice buys nothing and costs four `structural_m` calls a node.
    pub steer_lattice_m: f64,
    /// **The second harmonic's weight at the top of its band -- the field that makes channels
    /// merge.** Zero is off, and off reproduces the single-cosine shaping bit for bit -- held
    /// by a pinned table of eighteen pre-harmonic `f64` bit patterns, not by an argument.
    ///
    /// See [`harmonic_band_m`](Self::harmonic_band_m) for the mechanism and
    /// `.superpowers/sdd/notes/gully-merging.md` for the measurement.
    pub harmonic_weight: f64,
    /// Over how many metres of **local relief** the harmonic weight climbs from zero to
    /// [`harmonic_weight`](Self::harmonic_weight), through the same `smooth` the gate uses.
    ///
    /// # Local relief, not world elevation -- and that is the whole of this field's history
    ///
    /// `a = harmonic_weight * smooth((shaped - reference) / harmonic_band_m)`, where
    /// `reference` is [`steer::Steer::reference_m`](crate::steer::Steer::reference_m): the
    /// containing steer cell's own mean `structural_m`. **The expression is the one that
    /// shipped with `gate_elevation_m` in the reference's place; only the datum moved.** So
    /// this band is still "the metres over which the weight climbs from zero", and they are
    /// now metres of height above the ground's own local mean rather than above sea level.
    ///
    /// **The zero of that datum is flat ground, and that is why a world constant can aim at
    /// it.** `shaped - reference` is `-(h^2/4) * laplacian(structural)` over the lattice's
    /// own stencil, so it is positive on a convex spur, negative in a concave hollow, and
    /// crosses zero on every flank on every planet regardless of how high that flank is. Sea
    /// level has no such property, which is the whole of why the old form fired on one
    /// contour. Its SCALE is small -- a p10-to-p90 of a few tenths of a metre on this
    /// generator's flanks, measured -- so this band is a fraction of a metre where the old
    /// one was thousands, and [`crate::wasm::WB_MIN_GULLY_HARMONIC_BAND_M`] moved down to
    /// admit that.
    ///
    /// **It used to be `shaped - gate_elevation_m`, and that was a real defect rather than a
    /// coarser version of this.** With a world reference, `a = 1/4` is ONE contour at ONE
    /// world elevation -- 825 m at the shipped preset -- so only the flanks that cross that
    /// elevation branch at all, and a measured flank at 1,353-1,417 m got nothing at any
    /// setting in a 5x6 cross product. With a local reference the locus is a contour of local
    /// relief, which every flank has at every elevation. Measured both ways on the same three
    /// sites: `.superpowers/sdd/notes/gully-local-reference.md`.
    ///
    /// # Why a second harmonic makes a confluence, and why the weight must VARY
    ///
    /// The shipped shaping is a single-minimum, 2pi-periodic profile of the phase: **one
    /// channel per period, always.** The number of channels crossing a contour of length `L`
    /// is then the phase winding `L * |k|`, and `|k|` is tied to the structural slope, which
    /// has no downhill trend on a flank. So the count cannot fall and nothing can merge --
    /// measured, not argued: `.superpowers/sdd/notes/ridge-merging-spike.md` counts **0
    /// confluences and 0 divergences in 319 contour-row pairs** on the steepest flank this
    /// generator has, and shows that none of the ten fields above makes `|k|` fall.
    ///
    /// Add a second harmonic and the shaping is `cos t + a*cos 2t`, whose derivative is
    /// `-sin t * (1 + 4a*cos t)`. The critical points are `sin t = 0` and `cos t = -1/(4a)`,
    /// and the second pair **exists only for `a >= 1/4`**. Below that there is one minimum per
    /// period; above it `t = pi` turns into a local maximum with two minima straddling it.
    /// **The level set `a = 1/4` is therefore a locus of downhill Y-junctions**: two channels
    /// above it, one below, joined continuously. Every merge is a Y; no defect, no state and
    /// no neighbour lookup is involved, and the term stays `f(SpherePoint) -> value`.
    ///
    /// Making `a` a function of the query point's height above its own cell mean is what puts
    /// that level set on the ground: it is a contour of local relief, and a flank crosses it
    /// wherever the ground turns from spur into hollow. **A fixed `a` merges nothing**,
    /// however large -- the spike's control pins `a` at 0.90, gets twice as many channels as
    /// the shipped kernel, and still scores 0 confluences and 0 divergences. The merging
    /// comes from the weight FALLING, not from the harmonic being present, which is why this
    /// is a band and not a scalar.
    ///
    /// # What this delivers, and what it does not -- read this before extending it
    ///
    /// **This is branching keyed to LOCAL RELIEF. It is not a network keyed to CATCHMENT, and
    /// no point-evaluable term will ever be one.** The reference moved from sea level to the
    /// cell's own mean, and that changed WHICH ground branches -- all of it, rather than one
    /// contour -- without moving that line one inch. Contributing area `A(p)` is the measure
    /// of the set of points whose flow path reaches `p`: a functional of the field over an
    /// upslope domain of unbounded extent, not computable in bounded work from data local to
    /// `p`, no matter how this expression is written. The cell mean is bounded local work
    /// over a fixed 2 km stencil and is exactly the kind of thing that IS reachable;
    /// catchment is not, and the distinction is the size of the domain rather than the choice
    /// of datum. Two adjacent points at one local relief can drain wildly different areas, so
    /// a term keyed to relief correlates with catchment only loosely.
    ///
    /// **That division is deliberate, not a gap.** Texture -- the branching V-notches at
    /// 0.5-2 km that `erosion-architecture-spike.md` measured the stream graph cannot reach --
    /// comes from this detail field. Real hydrology comes from the stream graph in `stream.rs`
    /// and `water.rs`, which already resolves basins, spill levels and lake bodies over the
    /// whole planet because it is allowed the traversal this term is not. **Do not try to make
    /// this field do hydrology**: widening it towards accumulation is re-deriving the solver
    /// that already exists, at a per-texel cost, and it cannot converge on one.
    ///
    /// Two honest limits survive the change: the Y's are **symmetric** (both tributaries the
    /// same width, where real ones are lopsided), and the hierarchy is **shallow** (one
    /// harmonic is one halving; a fifth or sixth Strahler order would want a fifth or sixth
    /// harmonic). The third -- that the confluences on a flank are **synchronised** onto one
    /// contour -- is the one this field's local reference was built to remove, and the
    /// measurement that says whether it did is in
    /// `.superpowers/sdd/notes/gully-local-reference.md`.
    pub harmonic_band_m: f64,
}

impl GullyParams {
    /// The block, switched off. `amplitude_m` is exactly zero and every other field carries
    /// [`drainage()`](Self::drainage)'s value, so `canonical()` reads as "the drainage
    /// kernel, not running" rather than as a second set of numbers to keep in step.
    ///
    /// **This is the `None` path's exact equivalent and it must stay that way** -- see
    /// `surface.rs`'s `gully_none_matches_gully_some_canonical_bit_for_bit`.
    pub fn canonical() -> Self {
        Self { amplitude_m: 0.0, ..Self::drainage() }
    }

    /// The measured preset: the drainage texture, on.
    ///
    /// Every value here is either measured on this generator or derived from a constant this
    /// crate already owns. None of them is transcribed from the published kernel, whose own
    /// constants are calibrated against a display surface two orders of magnitude steeper
    /// than this one.
    ///
    /// - **`slope_reference: 0.005`** m/m (0.286 degrees). Measured over 500,000 spiral
    ///   points on each of three worlds at a 2 km step. It is the p90 of high ground
    ///   (>800 m) on the default world (0.005206), the p95 of high ground on the owner's
    ///   4,500 km world (0.005290), and sits between the p95 and p99 of high ground on the
    ///   erosion-sweep world (0.003787 / 0.005323). On all three it lies between the p99 of
    ///   *all* land (0.00375-0.0066) and its maximum. So the steepest tenth of high ground
    ///   is at or above the reference on the world the viewer draws, and no world in the set
    ///   is either saturated or dead.
    /// - **`cell_m: 1000.0`**. The branching V-notches in the reference photographs are a
    ///   0.5-2 km feature -- `.superpowers/sdd/notes/erosion-architecture-spike.md` section 1
    ///   is what measured that the stream graph cannot reach them, and it is the whole reason
    ///   this kernel exists. One kilometre is the middle of that band.
    /// - **`max_stripes_per_cell: 4.0`**, which is not a taste: `cell_m / 4` is **250 m**,
    ///   exactly [`CANONICAL_WAVELENGTH_M`]. The cap is set so the finest stripe this kernel
    ///   can produce anywhere is the generator's own resolution floor -- a stripe below it
    ///   is a stripe that aliases in every grid, which is the argument `offset_m` already
    ///   makes for the octaves.
    /// - **`stripes_per_cell: 1.0`**. One stripe per cell at the reference slope, so the
    ///   reference wavelength is `cell_m` and the two numbers mean the same thing there.
    /// - **`gate_elevation_m: 200.0` / `gate_elevation_span_m: 900.0`.** Not new numbers:
    ///   this is `amplitude_m`'s own `high` curve, `smooth((elevation - 200) / 900)`, which
    ///   is where this file already draws the line between ordinary land and the tops. A
    ///   second, differently-calibrated definition of "high" would be two answers to one
    ///   question.
    /// - **`flat_energy_floor: 0.25`.** The published kernel keeps half. A quarter is chosen
    ///   because this generator's flat ground is a much larger share of its gated land than
    ///   that kernel's is -- the median land slope is 0.0004-0.0008 m/m, a tenth of the
    ///   reference -- so half would spread a visible texture over ground with no fall line
    ///   to organise it. It is deliberately not zero: a hard cut-off would draw an edge along
    ///   a contour line, which is the artefact the smooth gates in this file exist to avoid.
    /// - **`crest_sharpness: 0.7`.** Below one, so crests sharpen and floors round.
    /// - **`amplitude_m: 60.0`.** Chosen against a published band rather than for being the
    ///   largest number that renders: Hammond's landform classification puts hills at
    ///   80-160 m of local relief over a 2 km run and low mountains at 300 m. Measured on the
    ///   gated flank population, this amplitude delivers local relief inside the hills band --
    ///   the same target and the same posture `ReliefParams::hills()` took, and for the same
    ///   reason: this is texture on a generator whose mountains are tectonic.
    /// - **`steer_lattice_m: 2000.0`.** `gradient-probe.md` section 1.3's measured
    ///   recommendation.
    /// - **`harmonic_weight: 2.4` and `harmonic_band_m: 0.12`.** Swept as a CROSS PRODUCT
    ///   (5 weights x 6 bands, three flank sites, two channel masks) by
    ///   `src/bin/gully_merging_survey.rs`, because the pitchfork's locus is
    ///   `weight * smooth((shaped - reference) / band) = 1/4` and moves when EITHER field
    ///   moves -- a one-at-a-time sweep would be measuring two different loci and calling it
    ///   two axes.
    ///
    ///   **The band is a fraction of a metre because the quantity it divides is.** It is
    ///   metres of LOCAL relief -- `shaped - steer.reference_m` -- whose measured p10-to-p90
    ///   over the gated texels of the steepest surveyed flank is **+0.022 m to +0.163 m**.
    ///   The pair puts the locus at **+0.024 m**, inside that distribution on all three sites,
    ///   and the share of texels above it FALLS downhill on all three, which is the merge.
    ///
    ///   Measured at mask 30%, channels per contour Q1 -> Q4 against the single-cosine
    ///   kernel's own baseline on the same ground:
    ///
    ///   - site 0: **70.4 -> 46.2 (-34%)**, width **51.6 -> 97.9 m (+90%)**, 28 confluences
    ///     against the comb's +4% / 0% / 0 confluences
    ///   - site 1: **51.1 -> 35.4 (-31%)**, width **66.9 -> 116.5 m (+74%)**, 39 confluences
    ///     against the comb's +2% / +4% / 0
    ///   - site 200: **15.8 -> 15.1 (-4%)**, width **195.7 -> 294.5 m (+50%)**, 33 confluences
    ///     against the comb's +20% / +3% / 5. **This is the site that got nothing at any
    ///     setting under the world-keyed form**, and it is the one the change was made for.
    ///
    ///   **Two honest things about this pair.** It is still a RIDGE, not a plateau: at the
    ///   same weight, a band of 0.10 or 0.15 m takes site 1's channel count from +4% to +19%
    ///   and +20% at mask 20%. And **the picture at 25 km is not better for it** -- the extra
    ///   branching reads as a finer stipple from that distance, which is a finding about what
    ///   this term can be rather than about these two numbers. See
    ///   `.superpowers/sdd/notes/gully-local-reference.md`.
    pub fn drainage() -> Self {
        Self {
            amplitude_m: 60.0,
            cell_m: 1000.0,
            slope_reference: 0.005,
            stripes_per_cell: 1.0,
            max_stripes_per_cell: 4.0,
            crest_sharpness: 0.7,
            gate_elevation_m: 200.0,
            gate_elevation_span_m: 900.0,
            flat_energy_floor: 0.25,
            steer_lattice_m: 2_000.0,
            harmonic_weight: 2.4,
            harmonic_band_m: 0.12,
        }
    }
}

/// `max(0.0, min(1.0, fraction))` then the smoothstep `x * x * (3.0 - 2.0 * x)`, in the
/// Python's operand order.
pub fn smooth(fraction: f64) -> f64 {
    let upper = if fraction < 1.0 { fraction } else { 1.0 };
    let clamped = if upper > 0.0 { upper } else { 0.0 };
    clamped * clamped * (3.0 - 2.0 * clamped)
}

/// One octave: a wavelength in metres, the frequency it maps to in noise space, and the
/// share of total amplitude it carries once normalised.
#[derive(Debug, Clone, Copy)]
pub struct Band {
    pub wavelength_m: f64,
    pub frequency: f64,
    pub share: f64,
}

/// Roughness, scaled to what is being roughened and to what can be seen.
pub struct Detail {
    radius_m: f64,
    noise: Noise,
    bands: Vec<Band>,
    relief: ReliefParams,
    /// `None` is the canonical path -- no drainage term at all. See [`GullyParams`].
    gully: Option<GullyParams>,
    /// The two pivot-jitter fields. Two rather than one because a pivot needs two
    /// independent offsets and this crate has exactly one hash; salting it twice is how
    /// `Noise::new`'s own doc says to get two independent fields from one seed, and is what
    /// `Continentality` and `Detail` already do to each other.
    jitter_x: Noise,
    jitter_y: Noise,
}

impl Detail {
    /// `relief`: `None` for canonical -- today's nine values (plus the coarsest
    /// wavelength), byte-for-byte what `ReliefParams::canonical()` returns. `Some(params)`
    /// for a caller-chosen block. Resolved once here rather than re-checked on every call,
    /// so `amplitude_m` and `plan` never see the `Option` at all.
    pub fn new(world_seed: u64, radius_m: f64, relief: Option<ReliefParams>) -> Self {
        Self::with_gully(world_seed, radius_m, relief, None)
    }

    /// The same roughness, plus an opt-in drainage texture.
    ///
    /// `gully`: `None` for today's ground, byte-for-byte -- or `Some(params)` for a
    /// caller-chosen [`GullyParams`]. A second constructor rather than a fourth parameter on
    /// [`Detail::new`], for the reason `Surface::with_coast` gives for being a second
    /// constructor: what Ruling 1 requires is a property of the parameter, not of where it
    /// is spelled, and `new` has call sites that want none of this.
    pub fn with_gully(
        world_seed: u64,
        radius_m: f64,
        relief: Option<ReliefParams>,
        gully: Option<GullyParams>,
    ) -> Self {
        let relief = relief.unwrap_or_else(ReliefParams::canonical);
        let noise = Noise::new(world_seed, 0x5EABED);
        let bands = Self::plan(radius_m, &relief);
        Self {
            radius_m,
            noise,
            bands,
            relief,
            gully,
            jitter_x: Noise::new(world_seed, 0x6011E1),
            jitter_y: Noise::new(world_seed, 0x6011E2),
        }
    }

    /// The drainage block this `Detail` was built with, or `None` for the canonical path.
    pub fn gully(&self) -> Option<GullyParams> {
        self.gully
    }

    pub fn bands(&self) -> &[Band] {
        &self.bands
    }

    /// The octaves, as wavelengths in metres with the share of amplitude each carries.
    ///
    /// Worked out once. Each octave is half the wavelength and half the amplitude of the
    /// one before, and the shares are normalised so that the total amplitude is what the
    /// caller asked for however many bands there happen to be - otherwise adding an octave
    /// would quietly make every world rougher.
    fn plan(radius_m: f64, relief: &ReliefParams) -> Vec<Band> {
        let mut raw: Vec<(f64, f64, f64)> = Vec::new();
        let mut wavelength = relief.coarsest_wavelength_m;
        let mut share = 1.0;
        while wavelength >= relief.canonical_wavelength_m {
            // Wavelength in metres to cycles per unit of noise space on the unit sphere.
            // Transcribed as the Python's four operations, in order -- not simplified to
            // radius_m / wavelength. The two forms agree at Earth's radius for every
            // configured wavelength, but diverge at other radii, and radius_m is a
            // constructor parameter here.
            let frequency = 2.0 * std::f64::consts::PI * radius_m / wavelength
                / (2.0 * std::f64::consts::PI);
            raw.push((wavelength, frequency, share));
            wavelength *= 0.5;
            share *= relief.octave_persistence;
        }
        let sum: f64 = raw.iter().map(|(_, _, s)| *s).sum();
        // `sum(...) or 1.0` in Python: 0.0 and -0.0 are falsy, NaN is truthy. `== 0.0`
        // matches both (since -0.0 == 0.0) and lets NaN pass through unchanged.
        let total = if sum == 0.0 { 1.0 } else { sum };
        raw.into_iter()
            // Shares are normalised so the total amplitude is what the caller asked for
            // however many bands there happen to be -- otherwise adding an octave would
            // quietly make every world rougher.
            .map(|(w, f, s)| Band { wavelength_m: w, frequency: f, share: s / total })
            .collect()
    }

    /// How rough the ground should be here.
    ///
    /// `point` is accepted but not used in the Python either -- kept here for signature
    /// fidelity with the reference and so the binding in a later task doesn't need to
    /// special-case this method.
    ///
    /// Blended from smooth weights rather than chosen from a category, for the same
    /// reason as everything else in this engine. And the trench term is deliberate: a
    /// deep, deliberate piece of structure stays legible instead of being buried under
    /// texture that has no idea it is there.
    #[allow(unused_variables)]
    pub fn amplitude_m(
        &self,
        point: &SpherePoint,
        elevation_m: f64,
        shelf_weight: f64,
        tectonic_m: f64,
    ) -> f64 {
        // How high, from deep water through the shelf to the tops.
        let deep = 1.0 - smooth((elevation_m + 3000.0) / 2500.0);
        let high = smooth((elevation_m - 200.0) / 900.0);
        let near_shore = smooth(1.0 - elevation_m.abs() / 350.0);

        let mut rough = deep * self.relief.abyssal_m
            + (1.0 - deep) * (1.0 - high) * self.relief.interior_m
            + high * self.relief.mountain_m;
        rough = rough * (1.0 - near_shore) + self.relief.coast_m * near_shore;
        rough = rough * (1.0 - shelf_weight) + self.relief.shelf_m * shelf_weight;

        // Deliberate deep structure keeps its shape.
        let quieted = 1.0
            - self.relief.quieting_strength * smooth(tectonic_m.abs() / self.relief.quieting_scale_m);
        rough * quieted
    }

    /// The roughness itself.
    ///
    /// `resolution_m` is `None` for canonical ground truth -- every configured octave,
    /// down to `CANONICAL_WAVELENGTH_M`. Python's `if resolution_m:` is false for both
    /// `None` and `0.0` (and `-0.0`, also falsy), so a caller passing zero gets every
    /// octave at full strength, exactly as if nothing had been passed -- it must not
    /// divide by zero. The `match Some(r) if r != 0.0 => Some(r), _ => None` below makes
    /// `Some(0.0)` and `Some(-0.0)` take the same canonical path as `None`.
    ///
    /// **Octaves fade rather than switch off.** Dropping one the instant it becomes
    /// unrepresentable would be a cliff in *resolution* rather than in position -- the
    /// ground would jump as somebody zoomed, which is the same bug M1.4 kept producing,
    /// in a different axis. Each octave dims between twice and four times the sample
    /// spacing and is gone by the far end.
    ///
    /// Sub-sample frequencies are not merely wasted work. They alias: an octave shorter
    /// than the spacing lands somewhere different in every grid, so a chart would
    /// shimmer as a ship moved rather than showing generalised ground.
    pub fn offset_m(&self, point: &SpherePoint, amplitude_m: f64, resolution_m: Option<f64>) -> f64 {
        if amplitude_m <= 0.0 {
            return 0.0;
        }

        // Python's `if resolution_m:` is false for None, 0.0 and -0.0. `r != 0.0`
        // matches both zeros (since -0.0 == 0.0 in IEEE 754), collapsing them to `None`
        // here.
        //
        // A NaN resolution is different: `NaN != 0.0` is true, so `Some(NaN)` takes the
        // *resolution* branch below, not this canonical one -- Python's `if resolution_m`
        // is also true for NaN (NaN is truthy), so both languages agree on which branch
        // runs. They still produce the same total, but not because NaN reaches this arm:
        // inside the loop, `wavelength / NaN` is NaN, and `smooth(NaN)` clamps to `1.0`
        // in both languages (the comparisons `fraction < 1.0` / `min(1.0, fraction)` and
        // `upper > 0.0` / `max(0.0, ...)` are false against NaN, so the upper-clamp value
        // wins on both sides), giving `visible = 1.0` for every band -- identical to the
        // canonical arm's literal `1.0`. The equivalence comes from `smooth`'s clamp
        // order, not from `Some(NaN)` reaching the `None` path.
        let resolution = match resolution_m {
            Some(r) if r != 0.0 => Some(r),
            _ => None,
        };

        let vector = point.vector;
        let mut total = 0.0;
        for band in &self.bands {
            let visible = match resolution {
                Some(r) => {
                    let v = smooth((band.wavelength_m / r - BARELY_M) / (CLEARLY_M - BARELY_M));
                    if v <= 0.0 {
                        // Everything finer is finer still, so nothing below can be
                        // visible.
                        break;
                    }
                    v
                }
                None => 1.0,
            };
            total += (self.noise.at(
                vector.x * band.frequency,
                vector.y * band.frequency,
                vector.z * band.frequency,
            ) - 0.5)
                * 2.0
                * band.share
                * visible;
        }
        total * amplitude_m
    }

    /// The drainage texture, in metres, at one point.
    ///
    /// Returns exactly `0.0` -- and, more to the point, is never reached at all by
    /// `Surface::elevation_m` -- on the canonical path. See [`GullyParams`] for why the
    /// off switch is an early return rather than an addition of zero.
    ///
    /// # Arguments
    ///
    /// - `frame`: the tangent frame at `point`. Passed in rather than built here because
    ///   `Surface::elevation_m` already has one and building a second is the same six
    ///   transcendentals twice.
    /// - `steer`: the steering reading at `point` -- `grad(structural_m)` in `frame`'s
    ///   basis in m/m, **and the containing lattice cell's own mean `structural_m`**. From
    ///   `steer::SteerLattice`, which is where every word about *why* it is the structural
    ///   gradient lives, and where the reference costs no extra `structural_m` call because
    ///   it is the mean of the four samples the gradient already differences.
    /// - `shaped`: the structural, feature-composed ground at `point`. What the gate reads.
    /// - `resolution_m`: the caller's sample spacing, on the same contract as
    ///   [`Detail::offset_m`]'s.
    ///
    /// # The kernel
    ///
    /// A sum of plane waves, one per pivot, over the eight corners of the containing cell of
    /// a cubic lattice in unit-sphere space -- the same lattice geometry `noise.rs` uses, and
    /// for its stated reason: a two-dimensional field cannot be wrapped onto a sphere without
    /// a seam down one meridian and a pinch at each pole.
    ///
    /// **The window is exactly the eight corners and that is not an approximation.** The
    /// weight is `smooth(1 - r)` in lattice units, which is zero for `r >= 1` and has zero
    /// derivative there. Any lattice node that is *not* a corner of the containing cell
    /// differs from the query point by at least one whole unit in some coordinate, so it is
    /// at a distance of at least 1 and its weight is exactly zero. The eight-corner sum is
    /// therefore the sum over every node with a non-zero weight, and the field is continuous
    /// everywhere rather than to within a truncated tail. The published kernel this
    /// reimplements uses a Gaussian over a 4x4 window instead, which leaves a real (if small)
    /// step at the window edge; a compact, C1 weight costs half the pivots and none of the
    /// continuity.
    ///
    /// **The phase is what makes it drainage.** `dir` is the steering gradient rotated
    /// ninety degrees and divided by [`GullyParams::slope_reference`], so the phase varies
    /// along the contour, the crests run down the fall line, and the frequency rises with
    /// slope until the cap. Divided, not left unnormalised: see [`GullyParams`] for the
    /// measurement that makes the difference between a texture and a planet-wide constant.
    ///
    /// **The asymmetry is what puts snow on ridges.** A symmetric wave blankets a flank
    /// evenly. Raising the folded signal to a power below one sharpens the crests into
    /// narrow spines and broadens the floors, so the mean of the term is negative -- the
    /// kernel carves valleys out of the flank rather than piling ridges on top of it, which
    /// is also what keeps it from raising the summit it is decorating.
    pub fn gully_offset_m(
        &self,
        point: &SpherePoint,
        frame: &TangentFrame,
        steer: Steer,
        shaped: f64,
        resolution_m: Option<f64>,
    ) -> f64 {
        let gully = match self.gully {
            Some(gully) => gully,
            None => return 0.0,
        };
        // Ruling 1's hinge: `Some(canonical())` leaves here, having touched nothing.
        if gully.amplitude_m == 0.0 {
            return 0.0;
        }

        // The gate. `smooth` clamps, so a point below the gate elevation gives exactly zero
        // and a NaN steer gives exactly zero too -- `smooth`'s clamp order sends a NaN to
        // the upper bound, and the `<= 0.0` tests below are written as they are so a NaN
        // leaves by the refusing door rather than the accepting one.
        let land = smooth((shaped - gully.gate_elevation_m) / gully.gate_elevation_span_m);
        if !(land > 0.0) {
            return 0.0;
        }
        let slope = m::hypot(steer.grad_x, steer.grad_y);
        let energy = gully.flat_energy_floor
            + (1.0 - gully.flat_energy_floor) * smooth(slope / gully.slope_reference);
        let gate = land * energy;
        if !(gate > 0.0) {
            return 0.0;
        }

        // Stripes per cell, scaled by the measured slope reference and capped so the finest
        // wavelength this kernel can ask for is the generator's own resolution floor.
        let mut stripes = slope / gully.slope_reference * gully.stripes_per_cell;
        if stripes > gully.max_stripes_per_cell {
            stripes = gully.max_stripes_per_cell;
        }

        // **The caller's sampling caps the frequency too, and the first cut of this got it
        // wrong in a way the parity corpus caught.**
        //
        // The obvious rule -- fade the whole term out once one stripe wavelength is finer
        // than twice the sample spacing -- deletes the kernel entirely on exactly the ground
        // it is for: at `resolution_m = 250`, a flank steep enough to ask for 2.7 stripes has
        // a 270 m wavelength, which is below that floor, so a steep flank rendered at the
        // scalar exports' own resolution got NOTHING. The corpus's witness assertion refused
        // to write a corpus in which the drainage block moved the ground by 0 m, which is that
        // guard doing what it exists for.
        //
        // The rule that is actually the module's own is the one `offset_m` uses: **an octave
        // finer than the sampling is DROPPED, and the ones above it are still drawn.** So a
        // coarse caller does not lose the gully, it gets a coarser one -- generalised ground
        // rather than shimmer, which is the sentence `offset_m`'s doc already makes. The
        // ceiling is `cell_m / (CLEARLY_M * resolution_m)`: the finest stripe that is still
        // *clearly* representable at the caller's spacing.
        let ceiling = match resolution_m {
            Some(r) if r != 0.0 => gully.cell_m / (CLEARLY_M * r),
            _ => f64::INFINITY,
        };
        if stripes > ceiling {
            stripes = ceiling;
        }
        // Negated so a NaN takes this door too, and lands on a frequency of zero rather than
        // poisoning every phase below. A NaN `resolution_m` leaves `stripes` untouched here
        // (`stripes > NaN` is false) and gives `visible = 1.0` below, so it behaves exactly
        // as the canonical path does -- the same equivalence `offset_m`'s doc works through.
        if !(stripes > 0.0) {
            stripes = 0.0;
        }

        // What remains once the stripes have been capped is the pivot lattice itself, at
        // `cell_m`, and that is what the two-to-four-samples fade is applied to. Below two
        // samples per cell there is no structure left to draw and the term goes to zero.
        let visible = match resolution_m {
            Some(r) if r != 0.0 => smooth((gully.cell_m / r - BARELY_M) / (CLEARLY_M - BARELY_M)),
            _ => 1.0,
        };
        if !(visible > 0.0) {
            return 0.0;
        }

        // The stripe direction: the steering gradient rotated ninety degrees, in stripes per
        // cell. At zero slope `stripes` is zero, so this vanishes continuously and the whole
        // pattern degenerates to the pivot lattice's own blobs rather than to a discontinuity
        // in an undefined direction.
        let (dir_x, dir_y) = if slope > 0.0 {
            (steer.grad_y / slope * stripes, -steer.grad_x / slope * stripes)
        } else {
            (0.0, 0.0)
        };

        let cell = gully.cell_m / self.radius_m;
        let v = point.vector;
        let (fx, fy, fz) = (v.x / cell, v.y / cell, v.z / cell);
        let (bx, by, bz) = (m::floor(fx), m::floor(fy), m::floor(fz));
        // The same saturation `Noise::at` documents and refuses: `as i64` saturates and the
        // very next line asks for `+ 1`. Unreachable on any record the C ABI admits, and
        // guarded anyway, because behind `extern "C"` an overflow is an abort.
        if !(bx.abs() < GULLY_LATTICE_LIMIT
            && by.abs() < GULLY_LATTICE_LIMIT
            && bz.abs() < GULLY_LATTICE_LIMIT)
        {
            return 0.0;
        }
        let (ix, iy, iz) = (bx as i64, by as i64, bz as i64); // cast-ok: guarded on the line above against the saturation `Noise::at` documents; each is finite and below 9e18

        let mut accumulated = 0.0;
        let mut harmonic = 0.0;
        let mut weight_total = 0.0;
        for corner in 0..8u32 {
            let (sx, sy, sz) = (corner & 1, (corner >> 1) & 1, (corner >> 2) & 1);
            let node = (ix + sx as i64, iy + sy as i64, iz + sz as i64); // cast-ok: a 0-or-1 corner selector widened for lattice arithmetic
            // Distance from the query point to the UNJITTERED node, in lattice units. This
            // is what decides the weight, and it is why the eight-corner window is exact.
            let (ox, oy, oz) = (
                node.0 as f64 - fx, // cast-ok: a lattice coordinate to a float; bounded by GULLY_LATTICE_LIMIT above, far below 2^53
                node.1 as f64 - fy, // cast-ok: as above
                node.2 as f64 - fz, // cast-ok: as above
            );
            let weight = smooth(1.0 - m::sqrt(ox * ox + oy * oy + oz * oz));
            if !(weight > 0.0) {
                continue;
            }
            // The pivot's displacement from the query point, projected onto the query
            // point's own tangent plane and measured in cells. Projected rather than
            // walked along a geodesic: a pivot is at most one cell away, and over a
            // kilometre on a planet of thousands the two differ by less than a millimetre,
            // for four transcendentals a pivot.
            let pivot = Vec3::new(
                node.0 as f64 * cell, // cast-ok: as above
                node.1 as f64 * cell, // cast-ok: as above
                node.2 as f64 * cell, // cast-ok: as above
            );
            let offset = pivot.sub(&v);
            let along = offset.dot(&frame.east) / cell
                + (self.jitter_x.lattice_at(node.0, node.1, node.2) - 0.5)
                    * GULLY_PIVOT_JITTER_CELLS;
            let across = offset.dot(&frame.north) / cell
                + (self.jitter_y.lattice_at(node.0, node.1, node.2) - 0.5)
                    * GULLY_PIVOT_JITTER_CELLS;
            let first = m::cos(TAU * (along * dir_x + across * dir_y));
            accumulated += weight * first;
            // **`cos 2t` by the double-angle identity, so the harmonic costs no transcendental
            // at all.** The spike priced this change at eight more `cos` per texel per level;
            // `2c^2 - 1` is exact arithmetic on a value already computed, so the real price is
            // three flops a pivot. It also cannot introduce a native-against-WASM divergence:
            // multiply and subtract are correctly rounded by IEEE-754 on every conforming
            // implementation, which is the same argument `detmath::sqrt` makes for calling the
            // hardware instruction -- and it is exactly the argument `cos` itself cannot make,
            // which is why `cos` stays on libm and this does not add a second call to it.
            harmonic += weight * (2.0 * first * first - 1.0);
            weight_total += weight;
        }
        // The nearest corner of a containing cell is at most sqrt(3)/2 lattice units away,
        // so at least one weight is always positive and this cannot divide by zero. Guarded
        // rather than argued, because a NaN here would reach a height.
        if !(weight_total > 0.0) {
            return 0.0;
        }
        // **The pitchfork, keyed to the LOCAL ground.** `a` is the second harmonic's weight at
        // this point; the shaping is `(cos t + a*cos 2t) / (1 + a)`, which has one minimum per
        // period below `a = 1/4` and two above it.
        //
        // **`shaped - steer.reference_m`, not `shaped - gate_elevation_m`, and that is this
        // slice.** The first form is height above the containing steer cell's own mean
        // structural ground -- positive on a spur, negative in a hollow, at EVERY world
        // elevation. The second was height above sea level, so `a = 1/4` was one contour on
        // the whole planet and only the flanks that happened to cross it branched at all: a
        // flank measured at 1,353-1,417 m got nothing at any setting. See
        // `harmonic_band_m`'s doc, and `.superpowers/sdd/notes/gully-local-reference.md`.
        //
        // The `0.5 +` centres the ramp on the reference itself, so the band is the FULL width
        // of the ramp in metres of local relief and the locus sits a fixed fraction of it
        // below the cell mean. `gate_elevation_m` no longer appears here at all -- it gates,
        // and it no longer also sets where channels merge, which is two jobs one number was
        // doing.
        //
        // **`a > 0.0` IS THE NaN DOOR, and that is the honest description of it.** The first
        // draft of this line claimed the branch was what makes `harmonic_weight: 0.0`
        // bit-identical to the kernel that had no harmonic, on the `x + 0.0 == x` argument
        // `canonical()`'s early return makes. **A mutation showed that was false here**:
        // making this unconditional left all eighteen pinned pre-harmonic bit patterns
        // unmoved, because `a` is then exactly `0.0`, `a * harmonic` is `+0.0`, and
        // `accumulated` is a sum of cosines that is never negative zero. The claim was
        // decoration and the test that "proved" it proved nothing.
        //
        // What the branch does carry is the same thing every other negated test in this
        // function carries: a NaN leaves by the refusing door. A NaN `harmonic_weight` or
        // `harmonic_band_m` makes `a` NaN, `a > 0.0` is false, and the point gets the
        // single-cosine shaping rather than a NaN height. That is what the pinned test
        // asserts now, and it is what turns red when this branch is removed.
        let a = gully.harmonic_weight
            * smooth((shaped - steer.reference_m) / gully.harmonic_band_m);
        let signal = if a > 0.0 {
            (accumulated + a * harmonic) / (weight_total * (1.0 + a))
        } else {
            accumulated / weight_total
        };

        // Crest against floor. `folded` is 0 at a crest and 1 at a floor; an exponent below
        // one pushes the bulk of the field towards the floor, so crests survive as narrow
        // spines and the mean of the term is negative.
        let mut folded = (1.0 - signal) * 0.5;
        if !(folded > 0.0) {
            folded = 0.0;
        }
        if folded > 1.0 {
            folded = 1.0;
        }
        let shaped_signal = 1.0 - 2.0 * m::powf(folded, gully.crest_sharpness);

        gully.amplitude_m * gate * visible * shaped_signal
    }
}

/// The largest pivot-lattice coordinate this kernel will name, mirroring `noise.rs`'s
/// `LATTICE_LIMIT` and drawn at the same round number for the same reason.
const GULLY_LATTICE_LIMIT: f64 = 9.0e18;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sphere::EARTH_RADIUS_M;

    /// **The local relief the tests below hand the kernel, and it is a measurement.** The
    /// median of `shaped - steer.reference_m` over the gated texels of the steepest flank
    /// `src/bin/gully_merging_survey.rs` finds on `DEFAULT_WORLD` is **+0.092 m**. Every test
    /// that wants the kernel running as it runs on the ground uses this, rather than a zero
    /// that would silently switch the harmonic off and leave the test measuring the
    /// single-cosine kernel while claiming to measure the shipped one.
    const MEASURED_LOCAL_RELIEF_M: f64 = 0.092;

    /// A steer at a chosen gradient, on ground sitting [`MEASURED_LOCAL_RELIEF_M`] above its
    /// own cell mean.
    fn steer_at(grad_x: f64, grad_y: f64, shaped: f64) -> Steer {
        Steer::new(grad_x, grad_y, shaped - MEASURED_LOCAL_RELIEF_M)
    }

    #[test]
    fn the_band_table_is_seven_octaves_from_twenty_kilometres_down() {
        // Measured from the Python, not computed here: the loop halves the wavelength
        // from COARSEST_WAVELENGTH_M while it stays at or above CANONICAL_WAVELENGTH_M,
        // and 312.5 is the last that qualifies -- 156.25 is below 250.
        let d = Detail::new(20260831, EARTH_RADIUS_M, None);
        let want: [(f64, f64); 7] = [
            (20000.0, 318.55),
            (10000.0, 637.1),
            (5000.0, 1274.2),
            (2500.0, 2548.4),
            (1250.0, 5096.8),
            (625.0, 10193.6),
            (312.5, 20387.2),
        ];
        assert_eq!(d.bands().len(), 7);
        for (i, (w, f)) in want.iter().enumerate() {
            assert_eq!(d.bands()[i].wavelength_m.to_bits(), w.to_bits(), "band {i} wavelength");
            assert_eq!(d.bands()[i].frequency.to_bits(), f.to_bits(), "band {i} frequency");
        }
    }

    #[test]
    fn the_shares_are_normalised_to_exactly_one() {
        // "otherwise adding an octave would quietly make every world rougher". The raw
        // shares halve from 1.0, so they sum to 2 - 0.5^6; dividing through gives 1.0,
        // and it lands exactly on 1.0 for this table -- measured, not assumed.
        let d = Detail::new(20260831, EARTH_RADIUS_M, None);
        let total: f64 = d.bands().iter().map(|b| b.share).sum();
        assert_eq!(total, 1.0, "shares must normalise to exactly one, got {total}");
    }

    #[test]
    fn smooth_saturates_at_both_ends() {
        assert_eq!(smooth(-10.0), 0.0);
        assert_eq!(smooth(10.0), 1.0);
        assert_eq!(smooth(0.5), 0.5);
    }

    // `point` is unused by the formula (see the doc comment on `amplitude_m`), so any
    // point will do for these tests.
    fn anywhere() -> SpherePoint {
        SpherePoint::from_latlon(0.0, 0.0)
    }


    /// **The assertion this whole slice turns on, and it is written as a comparison because
    /// an absolute would not fail.**
    ///
    /// The published kernel leaves its direction vector unnormalised, so stripe frequency is
    /// the slope itself. This generator's steepest flanks are 0.005 m/m -- about a hundredth
    /// of what that assumes -- so handing the kernel the raw slope makes every cosine the
    /// cosine of nearly nothing, and the term becomes a constant multiple of the gate: a very
    /// careful no-op, planet-wide.
    ///
    /// This walks 5 km across a flank at a slope this generator actually produces, once with
    /// the measured `slope_reference` and once at `1.0` -- which is the unnormalised form,
    /// spelled as a parameter. The scaled kernel must vary by a large fraction of its
    /// amplitude; the unscaled one must be flat.
    ///
    /// **Proved red by mutation**: setting `slope_reference` in `drainage()` to `1.0` turns
    /// this red on the first assertion, and only this one -- which is the point, because it
    /// is the mutation that produces a render nobody can tell from a broken build.
    #[test]
    fn the_slope_scale_is_what_stops_the_kernel_being_a_constant() {
        let spread_at = |reference: f64| -> f64 {
            let detail = Detail::with_gully(
                20260831,
                EARTH_RADIUS_M,
                None,
                Some(GullyParams { slope_reference: reference, ..GullyParams::drainage() }),
            );
            let origin = SpherePoint::from_latlon(24.0, 71.0);
            let frame = TangentFrame::at(&origin, EARTH_RADIUS_M);
            // 0.005 m/m: the steepest decile of high ground on the default world, measured.
            let steer = steer_at(0.005, 0.0, 1_200.0);
            let mut low = f64::INFINITY;
            let mut high = f64::NEG_INFINITY;
            for step in 0..100 {
                // cast-ok: a loop counter to a float for a distance in metres
                let point = frame.local_to_sphere(step as f64 * 50.0, 0.0);
                let value = detail.gully_offset_m(&point, &frame, steer, 1_200.0, None);
                if value < low {
                    low = value;
                }
                if value > high {
                    high = value;
                }
            }
            high - low
        };
        let scaled = spread_at(GullyParams::drainage().slope_reference);
        let unscaled = spread_at(1.0);
        assert!(
            scaled > 20.0,
            "at the measured slope reference the kernel must vary by a large fraction of its \
             60 m amplitude across a flank; spread was {scaled} m"
        );
        assert!(
            unscaled < 0.5,
            "the unnormalised form is the no-op this slice exists to avoid and must be shown \
             to be one; spread was {unscaled} m"
        );
        assert!(
            scaled > 40.0 * unscaled,
            "the scaled kernel must be at least a factor of forty livelier than the \
             unnormalised one; {scaled} against {unscaled}"
        );
    }

    /// **The mechanism, tested on the shipped function rather than on the algebra -- and it
    /// is keyed to LOCAL relief now, which is the whole of this slice.**
    ///
    /// `cos t + a*cos 2t` has one minimum per period below `a = 1/4` and two above it, so the
    /// number of channels a contour crosses must RISE with the point's height above its own
    /// steer cell's mean -- and therefore fall going downhill, where the measured local
    /// relief falls (+0.159 m to +0.027 m over the steepest surveyed flank). This walks 6 km
    /// along the contour (the steer is due east, so the stripes run east and the phase varies
    /// north) and counts local minima at two LOCAL reliefs straddling the preset's locus at
    /// +0.023 m: `a` is 0.045 at +0.010 m and 2.03 at +0.090 m.
    ///
    /// **The `shaped` argument is IDENTICAL in the two walks, and that is the assertion this
    /// test could not make before.** Under the old world-keyed form the two walks had to sit
    /// at 700 m and 2,000 m of world elevation, so the gate, the amplitude curve and the
    /// harmonic all moved together. Here the only thing that differs is the local datum,
    /// which is exactly the argument this slice added -- the probe mutates the ARGUMENT and
    /// not the stage.
    ///
    /// **The control is in the same test and is what makes it a finding.** At
    /// `harmonic_weight: 0.0` the shaping does not read the reference at all, so the two
    /// walks must find EXACTLY the same number of minima.
    ///
    /// **Proved red by mutation, siblings neutralised.** Four mutations, each reverted:
    /// dropping `* smooth(...)` so `a` is the flat `harmonic_weight` turns the first
    /// assertion red and leaves the control green; replacing `2c^2 - 1` with `c` turns it red
    /// the same way; pinning `a` at 0.9 turns it red; and **reading `gully.gate_elevation_m`
    /// instead of `steer.reference_m`** -- the pre-slice expression, and the one mutation
    /// that matters here -- makes the two walks identical and turns it red, because `shaped`
    /// is the same in both.
    #[test]
    fn the_second_harmonic_puts_more_channels_on_a_spur_than_in_a_hollow() {
        let minima_at = |harmonic_weight: f64, local_m: f64| -> usize {
            let detail = Detail::with_gully(
                20260831,
                EARTH_RADIUS_M,
                None,
                Some(GullyParams { harmonic_weight, ..GullyParams::drainage() }),
            );
            let origin = SpherePoint::from_latlon(24.0, 71.0);
            let frame = TangentFrame::at(&origin, EARTH_RADIUS_M);
            // Due east, so the fall line is east, the contour is north, and walking north
            // sweeps the phase. 0.005 m/m is the measured steepest decile of high ground.
            let shaped = 1_200.0;
            let steer = Steer::new(0.005, 0.0, shaped - local_m);
            let walk: Vec<f64> = (0..300)
                .map(|step| {
                    // cast-ok: a loop counter to a float for a distance in metres
                    let point = frame.local_to_sphere(0.0, step as f64 * 20.0);
                    detail.gully_offset_m(&point, &frame, steer, shaped, None)
                })
                .collect();
            (1..walk.len() - 1)
                .filter(|&k| walk[k] < walk[k - 1] && walk[k] < walk[k + 1])
                .count()
        };
        let preset = GullyParams::drainage().harmonic_weight;
        let hollow = minima_at(preset, 0.010);
        let spur = minima_at(preset, 0.090);
        assert!(
            // cast-ok: two counts to floats for a ratio
            spur as f64 >= 1.4 * hollow as f64,
            "the second harmonic must put substantially more channels on ground above its own \
             cell mean than on ground level with it; found {spur} minima at +0.090 m of local \
             relief against {hollow} at +0.010 m"
        );
        let flat_hollow = minima_at(0.0, 0.010);
        let flat_spur = minima_at(0.0, 0.090);
        assert_eq!(
            flat_hollow, flat_spur,
            "CONTROL: with the harmonic off the shaping does not read the local reference at \
             all, so the two walks must find exactly the same minima; found {flat_hollow} \
             against {flat_spur}"
        );
    }

    /// **`harmonic_weight: 0.0` is the kernel exactly as it was before the harmonic existed,
    /// bit for bit -- and this is the only test in the file that can say so.**
    ///
    /// The eighteen values below were produced by running `Detail::gully_offset_m` at
    /// `GullyParams::drainage()` on the commit before this change (`910b69e`), through a
    /// throwaway `[[bin]]` that printed `f64::to_bits`, and they are pasted here as hex. So
    /// this is a genuine before-and-after oracle rather than a restatement of the code under
    /// test: nothing in the expected column is derived from the expression it checks.
    ///
    /// **The second half of this test exists because the first half did not catch what it
    /// was written to catch.** It originally claimed to prove the `a > 0.0` branch load-bearing
    /// on the `x + 0.0 == x` argument. Making the branch unconditional left all eighteen
    /// patterns unmoved -- `a` is exactly `0.0`, `a * harmonic` is `+0.0`, and `accumulated`
    /// is a sum of cosines that is never negative zero -- so the assertion was green for a
    /// reason unrelated to the claim. The branch's real job is the NaN door every other
    /// negated test in `gully_offset_m` carries, so the same eighteen records are replayed
    /// with a NaN in each of the two new fields and must give the SAME bits: a NaN weight is
    /// a point that gets the single-cosine shaping, not a NaN height.
    ///
    /// **Proved red by mutation, siblings neutralised.** Removing the `a > 0.0` test turns the
    /// NaN half red and leaves the first half green (that is the finding above). Removing the
    /// `harmonic_weight` factor from `a` turns the first half red and leaves the NaN half
    /// green. Neither mutation is caught by both, which is why both are here.
    #[test]
    fn the_kernel_with_the_harmonic_off_is_bit_for_bit_what_it_was_before_the_harmonic() {
        let detail = Detail::with_gully(
            20260831,
            EARTH_RADIUS_M,
            None,
            Some(GullyParams { harmonic_weight: 0.0, ..GullyParams::drainage() }),
        );
        let origin = SpherePoint::from_latlon(24.0, 71.0);
        let frame = TangentFrame::at(&origin, EARTH_RADIUS_M);
        //
        // **The steer carries a third word now and the pins did not move**, which is worth
        // one sentence: with `harmonic_weight: 0.0` the reference is multiplied by a zero
        // weight, so the pre-harmonic bits are still the right answer and this table did not
        // have to be regenerated. If a future change makes the reference reach the height on
        // the harmonic-off path, this table goes red, which is what it is for.
        let records: [((f64, f64), f64, Option<f64>); 6] = [
            ((0.005, 0.0), 1_200.0, None),
            ((0.005, 0.0), 1_200.0, Some(76.35)),
            ((0.002, 0.004), 900.0, None),
            ((-0.003, 0.008), 2_400.0, Some(250.0)),
            ((0.012, -0.001), 1_500.0, None),
            ((0.0004, 0.0003), 305.0, None),
        ];
        let want: [u64; 18] = [
            0xc047e8c3b4f4a194,
            0xc04898d43fffab1b,
            0xc04623a805fde74a,
            0xc047e8c3b4f4a194,
            0xc04898d43fffab1b,
            0xc04623a805fde74a,
            0x4026f6fd9818530e,
            0x4008d772ebdefb59,
            0xc012219cc208e12e,
            0xc02ce9d0289e1f25,
            0xc042e9040e66364f,
            0xc03dd836ae8d13a2,
            0x4034fcfca68458c0,
            0xc03953685eac7728,
            0xc045a7c6ffe7e966,
            0x3fe157218572f3e6,
            0x3fe114c277f2958f,
            0x3fe101767d760961,
        ];
        let mut seen = 0usize;
        for (steer, shaped, resolution_m) in records {
            for step in 0..3 {
                // cast-ok: a loop counter to a float for a distance in metres
                let point = frame.local_to_sphere(step as f64 * 137.0, step as f64 * 61.0);
                let value = detail.gully_offset_m(
                    &point,
                    &frame,
                    steer_at(steer.0, steer.1, shaped),
                    shaped,
                    resolution_m,
                );
                assert_eq!(
                    value.to_bits(),
                    want[seen],
                    "record {seen} moved: {value} m. The kernel with the harmonic switched off \
                     must be byte-identical to the kernel that had no harmonic at all."
                );
                seen += 1;
            }
        }
        assert_eq!(seen, want.len(), "every pinned record must have been compared");

        // The NaN door. A NaN in either new field must land on the same single-cosine value,
        // not on a NaN height -- and not on a different finite one either.
        //
        // Only the WEIGHT is replayed against the pins, and that asymmetry is `smooth`'s own
        // documented clamp order rather than an omission: a NaN band makes the fraction NaN,
        // `smooth` sends a NaN to its UPPER bound, and `a` comes out as a finite
        // `harmonic_weight` -- the pinned-weight control, which is a texture and not a NaN
        // height. That is asserted separately below rather than papered over.
        for poisoned in [GullyParams { harmonic_weight: f64::NAN, ..GullyParams::drainage() }] {
            let detail = Detail::with_gully(20260831, EARTH_RADIUS_M, None, Some(poisoned));
            let mut seen = 0usize;
            for (steer, shaped, resolution_m) in records {
                for step in 0..3 {
                    // cast-ok: a loop counter to a float for a distance in metres
                    let point = frame.local_to_sphere(step as f64 * 137.0, step as f64 * 61.0);
                    let value = detail.gully_offset_m(
                        &point,
                        &frame,
                        steer_at(steer.0, steer.1, shaped),
                        shaped,
                        resolution_m,
                    );
                    assert_eq!(
                        value.to_bits(),
                        want[seen],
                        "record {seen} with a NaN harmonic field gave {value} m; a NaN must                          leave by the refusing door and land on the single-cosine shaping"
                    );
                    seen += 1;
                }
            }
            assert_eq!(seen, want.len(), "every pinned record must have been replayed");
        }

        let nan_band = Detail::with_gully(
            20260831,
            EARTH_RADIUS_M,
            None,
            Some(GullyParams { harmonic_band_m: f64::NAN, ..GullyParams::drainage() }),
        );
        let value = nan_band.gully_offset_m(
            &frame.origin.clone(),
            &frame,
            steer_at(0.005, 0.0, 1_200.0),
            1_200.0,
            None,
        );
        assert!(
            value.is_finite(),
            "a NaN band must not reach a height; `smooth` sends it to the upper bound and the              point gets the pinned-weight shaping, but it got {value}"
        );

        // **The third NaN, and it does NOT take the refusing door -- measured, and recorded
        // as the asymmetry it is rather than asserted into the shape the other two have.**
        //
        // The reference is an ARGUMENT rather than a field, so no `gully_is_admissible` bound
        // stands between a caller and a NaN in it. It reaches `smooth`, whose clamp order
        // sends a NaN to its UPPER bound -- exactly as it does for a NaN `harmonic_band_m` --
        // so `a` comes out as a finite `harmonic_weight` and the point gets the SATURATED
        // shaping, not the single-cosine one. The first draft of this test asserted the pins
        // here and went red; the pins were right and the claim was wrong.
        //
        // What is asserted instead is the true statement, and it is still load-bearing: a NaN
        // reference must be bit-for-bit a reference sitting far enough below the point to
        // saturate the ramp. Swapping `smooth`'s two clamps turns this red.
        let live =
            Detail::with_gully(20260831, EARTH_RADIUS_M, None, Some(GullyParams::drainage()));
        let band = GullyParams::drainage().harmonic_band_m;
        for (steer, shaped, resolution_m) in records {
            for step in 0..3 {
                // cast-ok: a loop counter to a float for a distance in metres
                let point = frame.local_to_sphere(step as f64 * 137.0, step as f64 * 61.0);
                let poisoned = live.gully_offset_m(
                    &point,
                    &frame,
                    Steer::new(steer.0, steer.1, f64::NAN),
                    shaped,
                    resolution_m,
                );
                let saturated = live.gully_offset_m(
                    &point,
                    &frame,
                    Steer::new(steer.0, steer.1, shaped - 10.0 * band),
                    shaped,
                    resolution_m,
                );
                assert!(
                    poisoned.is_finite(),
                    "a NaN local reference must not reach a height; it gave {poisoned}"
                );
                assert_eq!(
                    poisoned.to_bits(),
                    saturated.to_bits(),
                    "a NaN local reference must land exactly on the saturated shaping \
                     `smooth`'s clamp order sends it to; {poisoned} against {saturated}"
                );
            }
        }
    }

    /// **The preset's pitchfork must sit inside the local relief this generator's flanks
    /// actually have**, or the harmonic is the same careful no-op that
    /// `the_slope_scale_is_what_stops_the_kernel_being_a_constant` exists to refuse: a weight
    /// that never crosses 1/4 on real ground is exactly the pinned-weight control, which
    /// measures 0 confluences and 0 divergences over 319 contour-row pairs.
    ///
    /// **This is the test the world-keyed form could not have.** It used to ask whether the
    /// locus lay inside the GATE's elevation band -- a question about two constants, which
    /// says nothing about whether any flank crosses it, and which passed on a preset that did
    /// nothing at all on a measured flank 500 m above the locus. This asks the ground.
    ///
    /// - **Population:** the 225 texels of a 15 x 15 grid at 400 m spacing centred on the
    ///   steepest flank `src/bin/gully_merging_survey.rs` finds on `DEFAULT_WORLD` (rank 0,
    ///   lat -8.9780, lon 65.0630), all of them above the gate.
    /// - **Method:** `structural_m` at each texel minus the shipped `SteerLattice`'s own
    ///   `reference_m` there, at `drainage().steer_lattice_m`. The lattice under test, not a
    ///   reimplementation of it.
    ///
    /// **Proved red by mutation**: `harmonic_band_m: 6.0` -- fifty times the preset and still
    /// a plausible-looking number -- puts the locus at +1.14 m, above every texel on the
    /// flank, and turns the share assertion to 0%.
    #[test]
    fn the_preset_crosses_the_pitchfork_on_real_ground() {
        let gully = GullyParams::drainage();
        let a_at = |local_m: f64| gully.harmonic_weight * smooth(local_m / gully.harmonic_band_m);
        assert_eq!(a_at(0.0), 0.0, "the weight starts at zero on the cell's own mean");
        assert!(
            a_at(gully.harmonic_band_m) >= 0.25,
            "the preset must reach the pitchfork somewhere inside its own band, or it merges \
             nothing anywhere; a at the top of the band is {}",
            a_at(gully.harmonic_band_m)
        );
        let mut lo = 0.0;
        let mut hi = gully.harmonic_band_m;
        for _ in 0..80 {
            let mid = 0.5 * (lo + hi);
            if a_at(mid) < 0.25 {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let locus = 0.5 * (lo + hi);

        // The ground. `Surface::new` here is `DEFAULT_WORLD`, the world the survey measured
        // and the viewer draws.
        let world = crate::surface::Surface::new(
            20_260_904,
            EARTH_RADIUS_M,
            12,
            0.29,
            None,
            None,
            None,
        );
        let lattice = crate::steer::SteerLattice::new(EARTH_RADIUS_M, gully.steer_lattice_m);
        let structural = |p: &SpherePoint| world.structural_m(p);
        let origin = SpherePoint::from_latlon(-8.9780, 65.0630);
        let frame = TangentFrame::at(&origin, EARTH_RADIUS_M);
        // The grid is oriented on the fall line, exactly as the survey's is: `+v` is the
        // negated steering gradient, so a row is a contour and walking `+v` is walking
        // downhill. Taken from the lattice rather than assumed.
        let here0 = lattice.at(&origin, &frame, &structural);
        let slope = m::hypot(here0.grad_x, here0.grad_y);
        let down = (-here0.grad_x / slope, -here0.grad_y / slope);
        let across = (-down.1, down.0);
        let share_over = |rows: std::ops::Range<i64>| -> (f64, f64) {
            let mut above = 0.0;
            let mut total = 0.0;
            for row in rows {
                for column in -10..=10 {
                    // cast-ok: small loop counters to floats for distances in metres
                    let (u, v) = (column as f64 * 640.0, row as f64 * 640.0);
                    let point = frame
                        .local_to_sphere(u * across.0 + v * down.0, u * across.1 + v * down.1);
                    let at = TangentFrame::at(&point, EARTH_RADIUS_M);
                    let shaped = world.structural_m(&point);
                    if shaped <= gully.gate_elevation_m {
                        continue;
                    }
                    total += 1.0;
                    if shaped - lattice.at(&point, &at, &structural).reference_m > locus {
                        above += 1.0;
                    }
                }
            }
            (above / total, total)
        };
        let (uphill, uphill_n) = share_over(-10..-4);
        let (downhill, downhill_n) = share_over(4..11);
        assert!(
            uphill_n > 100.0 && downhill_n > 100.0,
            "the surveyed flank must be gated ground; {uphill_n} and {downhill_n} texels"
        );
        // **The mechanism, on the ground, in one number.** Two channels per period above the
        // locus and one below it, so a share that FALLS downhill is the merge itself.
        // Measured at this preset on this flank: **100% uphill against 64% downhill.**
        //
        // Under the pre-slice expression -- `shaped - gate_elevation_m` in the reference's
        // place -- both thirds sit around 1,170 m of world elevation, `a` saturates at the
        // weight on all of it, and the two shares are 100% and 100%. That mutation is what
        // this assertion is aimed at, and the old test could not have been aimed at it.
        assert!(
            uphill > 0.90 && downhill < 0.75 && uphill - downhill > 0.25,
            "the pitchfork locus is at {locus:+.4} m of local relief; {:.0}% of the uphill \
             third of this flank sits above it and {:.0}% of the downhill third does. A locus \
             every texel or no texel crosses is a locus nothing merges at, and a share that \
             does not fall downhill is a locus nothing merges ACROSS.",
            uphill * 100.0,
            downhill * 100.0
        );
    }

    /// The eight-corner pivot window is exact rather than truncated, so the term is continuous
    /// everywhere -- including exactly on a lattice plane, which is where a truncated window
    /// would leave a step and the eye would read a cliff.
    ///
    /// **Proved red by mutation**: taking the weight from the JITTERED displacement instead of
    /// the unjittered node -- which is what the published kernel does -- turns this red.
    #[test]
    fn the_gully_term_is_continuous_across_a_pivot_cell_boundary() {
        let detail =
            Detail::with_gully(20260831, EARTH_RADIUS_M, None, Some(GullyParams::drainage()));
        let origin = SpherePoint::from_latlon(-13.0, 155.0);
        let frame = TangentFrame::at(&origin, EARTH_RADIUS_M);
        let steer = steer_at(0.004, 0.002, 1_200.0);
        let mut previous: Option<f64> = None;
        let mut worst: f64 = 0.0;
        // Half-metre steps over 3 km: several 1,000 m pivot cells, sampled two thousand times
        // finer than the cell, so any step at a cell face is far larger than the smooth
        // variation either side of it.
        for step in 0..6_000 {
            // cast-ok: a loop counter to a float for a distance in metres
            let point = frame.local_to_sphere(step as f64 * 0.5, 0.0);
            let value = detail.gully_offset_m(&point, &frame, steer, 1_200.0, None);
            if let Some(before) = previous {
                let jump = (value - before).abs();
                if jump > worst {
                    worst = jump;
                }
            }
            previous = Some(value);
        }
        assert!(
            worst < 0.5,
            "a half-metre step must never move a 60 m amplitude term by half a metre; worst \
             jump {worst} m"
        );
    }

    /// `canonical()` is off, and off means the function returns before it computes anything --
    /// not that it computes zero. Ruling 1's hinge at this level; `surface.rs` holds the same
    /// claim over the whole pipeline.
    #[test]
    fn the_canonical_gully_block_is_exactly_off() {
        let detail =
            Detail::with_gully(20260831, EARTH_RADIUS_M, None, Some(GullyParams::canonical()));
        let origin = SpherePoint::from_latlon(7.0, -33.0);
        let frame = TangentFrame::at(&origin, EARTH_RADIUS_M);
        for step in 0..50 {
            // cast-ok: a loop counter to a float for a distance in metres
            let point = frame.local_to_sphere(step as f64 * 137.0, 0.0);
            let value =
                detail.gully_offset_m(&point, &frame, steer_at(0.01, -0.004, 1_500.0), 1_500.0, None);
            assert_eq!(value.to_bits(), 0.0f64.to_bits(), "canonical must be exactly +0.0");
        }
        assert_eq!(GullyParams::canonical().amplitude_m, 0.0);
        // Every other field is `drainage()`'s, so there is one set of numbers and not two.
        assert_eq!(
            GullyParams { amplitude_m: GullyParams::drainage().amplitude_m, ..GullyParams::canonical() },
            GullyParams::drainage()
        );
    }

    /// The crest-versus-floor asymmetry is what puts snow on ridge lines rather than
    /// blanketing a flank, and it is a property of `crest_sharpness` being below one. At
    /// exactly one the kernel is symmetric and its mean is nearly zero; below one the mean
    /// must be negative, because the term carves valleys rather than piling ridges.
    ///
    /// Measured on the flank population (see `.superpowers/sdd/notes/gully-kernel.md` section
    /// 5): mean -20.4 / -10.8 / -4.8 / +0.3 m at 0.5 / 0.7 / 0.85 / 1.0.
    ///
    /// **Proved red by mutation**: `crest_sharpness: 1.0` in `drainage()` turns this red.
    #[test]
    fn the_crest_asymmetry_carves_rather_than_blankets() {
        let mean_at = |sharpness: f64| -> f64 {
            let detail = Detail::with_gully(
                20260831,
                EARTH_RADIUS_M,
                None,
                Some(GullyParams { crest_sharpness: sharpness, ..GullyParams::drainage() }),
            );
            let origin = SpherePoint::from_latlon(41.0, -119.0);
            let frame = TangentFrame::at(&origin, EARTH_RADIUS_M);
            let mut total = 0.0;
            let mut count = 0.0;
            for row in 0..40 {
                for column in 0..40 {
                    // cast-ok: loop counters to floats for distances in metres
                    let point =
                        frame.local_to_sphere(column as f64 * 97.0, row as f64 * 97.0);
                    total += detail.gully_offset_m(
                        &point,
                        &frame,
                        steer_at(0.005, 0.0, 1_200.0),
                        1_200.0,
                        None,
                    );
                    count += 1.0;
                }
            }
            total / count
        };
        let symmetric = mean_at(1.0);
        let sharpened = mean_at(GullyParams::drainage().crest_sharpness);
        assert!(
            symmetric.abs() < 2.0,
            "at an exponent of one the term must be near-symmetric; mean {symmetric} m"
        );
        assert!(
            sharpened < -4.0,
            "the shipped exponent must carve: mean {sharpened} m against a 60 m amplitude"
        );
    }

    #[test]
    fn deep_abyssal_ground_gives_exactly_abyssal_m() {
        // elevation_m = -6000.0, shelf_weight = 0.0, tectonic_m = 0.0.
        //   deep:  (elevation + 3000) / 2500 = -3000 / 2500 = -1.2, clamped to 0.0,
        //          smooth(0.0) = 0.0, so deep = 1.0 - 0.0 = 1.0 exactly.
        //   high:  (elevation - 200) / 900 = -6200 / 900, negative, clamped to 0.0,
        //          smooth(0.0) = 0.0.
        //   near_shore: 1.0 - abs(elevation) / 350 = 1.0 - 6000/350, deeply negative,
        //          clamped to 0.0, smooth(0.0) = 0.0.
        //   rough = 1.0*ABYSSAL_M + (1.0-1.0)*(1.0-0.0)*INTERIOR_M + 0.0*MOUNTAIN_M
        //         = ABYSSAL_M + 0.0 + 0.0 = ABYSSAL_M exactly.
        //   rough = rough*(1.0-0.0) + COAST_M*0.0 = rough (near_shore term drops out).
        //   rough = rough*(1.0-0.0) + SHELF_M*0.0 = rough (shelf_weight is 0.0).
        //   quieted: abs(0.0)/1200 = 0.0, smooth(0.0) = 0.0, quieted = 1.0 - 0.0 = 1.0.
        //   result = ABYSSAL_M * 1.0 = ABYSSAL_M exactly.
        let d = Detail::new(20260831, EARTH_RADIUS_M, None);
        let got = d.amplitude_m(&anywhere(), -6000.0, 0.0, 0.0);
        assert_eq!(got, ABYSSAL_M);
    }

    #[test]
    fn a_mountain_gives_exactly_mountain_m() {
        // elevation_m = 2000.0, shelf_weight = 0.0, tectonic_m = 0.0.
        //   deep:  (2000+3000)/2500 = 2.0, clamped to 1.0, smooth(1.0) = 1.0,
        //          so deep = 1.0 - 1.0 = 0.0 exactly.
        //   high:  (2000-200)/900 = 1800/900 = 2.0, clamped to 1.0, smooth(1.0) = 1.0
        //          exactly.
        //   near_shore: 1.0 - abs(2000)/350 = 1.0 - 5.714... , negative, clamped to 0.0,
        //          smooth(0.0) = 0.0.
        //   rough = 0.0*ABYSSAL_M + (1.0-0.0)*(1.0-1.0)*INTERIOR_M + 1.0*MOUNTAIN_M
        //         = 0.0 + 0.0 + MOUNTAIN_M = MOUNTAIN_M exactly.
        //   rough = rough*(1.0-0.0) + COAST_M*0.0 = rough.
        //   rough = rough*(1.0-0.0) + SHELF_M*0.0 = rough (shelf_weight is 0.0).
        //   quieted = 1.0 - 0.7*smooth(0.0) = 1.0.
        //   result = MOUNTAIN_M * 1.0 = MOUNTAIN_M exactly.
        let d = Detail::new(20260831, EARTH_RADIUS_M, None);
        let got = d.amplitude_m(&anywhere(), 2000.0, 0.0, 0.0);
        assert_eq!(got, MOUNTAIN_M);
    }

    #[test]
    fn full_shelf_weight_pulls_the_answer_to_exactly_shelf_m() {
        // Same elevation as the deep-abyssal case (-6000.0), so by that derivation
        // `rough` reaches SHELF_M's blend step as ABYSSAL_M exactly, i.e. 55.0.
        // shelf_weight = 1.0:
        //   rough = rough*(1.0-1.0) + SHELF_M*1.0 = 0.0 + SHELF_M = SHELF_M exactly,
        //   independent of what `rough` was going in.
        //   tectonic_m = 0.0, so quieted = 1.0 as before.
        //   result = SHELF_M * 1.0 = SHELF_M exactly.
        let d = Detail::new(20260831, EARTH_RADIUS_M, None);
        let got = d.amplitude_m(&anywhere(), -6000.0, 1.0, 0.0);
        assert_eq!(got, SHELF_M);
    }

    #[test]
    fn a_large_tectonic_m_quiets_the_result_to_thirty_percent() {
        // Same elevation/shelf_weight as the deep-abyssal case, so `rough` reaches the
        // quieting step as ABYSSAL_M exactly, i.e. 55.0.
        // tectonic_m = 5000.0: abs(5000)/1200 = 4.1666..., clamped to 1.0,
        //   smooth(1.0) = 1.0 exactly, so quieted = 1.0 - 0.7*1.0 = 1.0 - 0.7.
        //   In f64, 1.0 - 0.7 does not land on 0.3 -- it rounds to 0.30000000000000004
        //   (0x1.3333333333334p-2). result = 55.0 * (1.0 - 0.7), computed here the same
        //   way the formula computes it, not read back from the implementation.
        let d = Detail::new(20260831, EARTH_RADIUS_M, None);
        let got = d.amplitude_m(&anywhere(), -6000.0, 0.0, 5000.0);
        let expected: f64 = ABYSSAL_M * (1.0 - 0.7);
        assert_eq!(got, expected);
    }

    #[test]
    fn a_resolution_of_zero_behaves_exactly_like_canonical() {
        // Python's `if resolution_m:` is false for BOTH None and 0.0, so a caller
        // passing zero gets every octave, not a division by zero. A Rust Option port
        // diverges here unless Some(0.0) is special-cased -- this is the test that
        // catches it, and it must be bit-exact rather than approximate.
        let d = Detail::new(20260831, EARTH_RADIUS_M, None);
        let p = SpherePoint::from_latlon(17.0, 43.0);
        let canonical = d.offset_m(&p, 100.0, None);
        let zero = d.offset_m(&p, 100.0, Some(0.0));
        assert_eq!(zero.to_bits(), canonical.to_bits(), "Some(0.0) must equal None");
    }

    #[test]
    fn a_resolution_of_negative_zero_behaves_exactly_like_canonical() {
        // -0.0 is falsy in Python too, so Some(-0.0) must take the canonical path
        // exactly as Some(0.0) and None do.
        let d = Detail::new(20260831, EARTH_RADIUS_M, None);
        let p = SpherePoint::from_latlon(17.0, 43.0);
        let canonical = d.offset_m(&p, 100.0, None);
        let neg_zero = d.offset_m(&p, 100.0, Some(-0.0));
        assert_eq!(neg_zero.to_bits(), canonical.to_bits(), "Some(-0.0) must equal None");
    }

    #[test]
    fn a_nan_resolution_behaves_exactly_like_canonical() {
        // Some(NaN) takes the *resolution* branch (NaN != 0.0), not the canonical one --
        // unlike the zero cases above. But every band's `visible` still comes out to
        // exactly 1.0: `wavelength / NaN` is NaN, and `smooth(NaN)` clamps to 1.0 in both
        // languages because the comparisons that drive the clamp are false against NaN.
        // So the result matches canonical bit-for-bit, for a different reason than the
        // zero cases -- guarded here rather than merely asserted in a comment.
        let d = Detail::new(20260831, EARTH_RADIUS_M, None);
        let p = SpherePoint::from_latlon(17.0, 43.0);
        let canonical = d.offset_m(&p, 100.0, None);
        let nan_res = d.offset_m(&p, 100.0, Some(f64::NAN));
        assert_eq!(nan_res.to_bits(), canonical.to_bits(), "Some(f64::NAN) must equal None");
    }

    #[test]
    fn zero_amplitude_returns_exactly_zero() {
        let d = Detail::new(20260831, EARTH_RADIUS_M, None);
        let p = SpherePoint::from_latlon(17.0, 43.0);
        assert_eq!(d.offset_m(&p, 0.0, None), 0.0);
        assert_eq!(d.offset_m(&p, -1.0, None), 0.0);
    }

    #[test]
    fn a_coarse_resolution_drops_the_fine_octaves() {
        // At a sample spacing of 5 km, an octave of 312.5 m is far below Nyquist and
        // must contribute nothing, so the coarse answer differs from the canonical one.
        let d = Detail::new(20260831, EARTH_RADIUS_M, None);
        let p = SpherePoint::from_latlon(17.0, 43.0);
        let canonical = d.offset_m(&p, 100.0, None);
        let coarse = d.offset_m(&p, 100.0, Some(5000.0));
        assert!(canonical != coarse, "a coarse resolution must drop fine octaves");
    }

    #[test]
    fn the_fade_is_gradual_rather_than_a_step() {
        // The docstring's claim is that octaves fade rather than switch off: "a cliff in
        // resolution rather than in position - the ground would jump as somebody
        // zoomed". A port that dropped an octave abruptly at its Nyquist-ish threshold
        // would pass every test above (they only check that *some* difference exists)
        // but fail this one.
        //
        // The coarsest band is 20000.0 m. It fades as `wavelength / resolution_m` moves
        // from BARELY_M (2.0) to CLEARLY_M (4.0), i.e. resolution_m from
        // 20000/4 = 5000.0 up to 20000/2 = 10000.0. Sampling resolution_m across
        // [4000.0, 11000.0] in fixed steps crosses that whole fade window on both sides,
        // so the range is guaranteed not to be vacuous (unlike a range that stayed
        // entirely above or below the window, which would report a max step of 0.0 and
        // pass without testing anything -- that vacuity has hit this port three times).
        //
        // Deriving the bound, from the actual shape of `visible`, not a round number:
        //
        // `visible = smooth(x)` where `x = (wavelength / r - BARELY_M) / (CLEARLY_M -
        // BARELY_M)`, so `dx/dr = -wavelength / (2 * r^2)` (the 2 is `CLEARLY_M -
        // BARELY_M`), and `smooth`'s derivative peaks at 1.5 (at x = 0.5). Over one
        // sample step `d_res` the largest possible change in `visible` is therefore
        // `1.5 * wavelength * d_res / (2 * r_min^2)`, evaluated at the smallest `r` in
        // the sampled range that still lies in the fade window -- the slope is steepest
        // there. For the coarsest band (wavelength 20000.0) with d_res = 100.0 and
        // r_min = 5000.0 (the low edge of its fade window, also the low edge of the
        // sampled range):
        //   1.5 * 20000.0 * 100.0 / (2 * 5000.0^2) = 1.5 * 2_000_000.0 / 50_000_000.0
        //     = 0.06
        // A single band's contribution to `total` is `noise_factor * share * visible`
        // with `noise_factor` (`(noise - 0.5) * 2.0`) in [-1, 1], so one step can move
        // that band's term by at most `0.06 * share_of_coarsest_band`, times
        // `amplitude_m` once the final multiply is applied: a genuine fade cannot move
        // the total by more than `0.06 * share_of_coarsest_band * amplitude_m` per
        // sample -- call this bound `gradual_ceiling`, about 3.02 for this table
        // (share_of_coarsest_band ~= 0.504, amplitude_m = 100.0). Measured empirically
        // against the real implementation below, the actual max step is ~0.96, comfortably
        // under that analytic ceiling, which confirms the derivation rather than just
        // asserting it.
        //
        // An abrupt cutoff (`visible` jumping 1 -> 0 instead of fading) moves the total
        // by up to `share_of_coarsest_band * amplitude_m` in one step (`noise_factor`'s
        // full swing) -- about 50.4 here, and measured empirically at ~25.7 for the
        // actual noise value at this crossing. That is over 8x `gradual_ceiling`, so a
        // test bound placed between the two discriminates: pick a factor of 0.2 rather
        // than 0.06, i.e. `0.2 * share_of_coarsest_band * amplitude_m` (~10.1 here) --
        // above `gradual_ceiling` by more than 3x so a real fade never trips it, and
        // below the measured abrupt-step magnitude by more than 2x so a hard cutoff
        // does. This was proven, not assumed: mutating `visible`'s computation to a hard
        // step (`if frac > 0.0 { 1.0 } else { 0.0 }`) and rerunning this test failed it
        // (max_step ~25.7 against this bound of ~10.1); reverting the mutation passed it
        // again (max_step ~0.96) -- see task-3-report.md for the numbers from that run.
        let d = Detail::new(20260831, EARTH_RADIUS_M, None);
        let p = SpherePoint::from_latlon(17.0, 43.0);
        let share_of_coarsest_band = d.bands()[0].share;
        let bound = 0.2 * share_of_coarsest_band * 100.0; // amplitude_m = 100.0

        let mut resolution_m = 4000.0_f64;
        let mut previous = d.offset_m(&p, 100.0, Some(resolution_m));
        let mut max_step: f64 = 0.0;
        let mut low_seen = false;
        let mut high_seen = false;
        while resolution_m <= 11000.0 {
            if resolution_m < 5000.0 {
                low_seen = true;
            }
            if resolution_m > 10000.0 {
                high_seen = true;
            }
            let current = d.offset_m(&p, 100.0, Some(resolution_m));
            let step = (current - previous).abs();
            if step > max_step {
                max_step = step;
            }
            previous = current;
            resolution_m += 100.0;
        }

        // The range must actually cross the fade window (below BARELY_M's threshold and
        // above CLEARLY_M's), or this test would pass vacuously.
        assert!(low_seen, "the sampled range must dip below the fade window");
        assert!(high_seen, "the sampled range must rise above the fade window");
        assert!(max_step > 0.0, "the range must actually show the octave fading");
        assert!(
            max_step < bound,
            "a step of {max_step} between adjacent samples exceeds the derived bound \
             of {bound}, suggesting a cliff rather than a fade"
        );
    }

    // ---- Task 3: the `hills()` preset -------------------------------------------------

    /// `hills()` must not be `canonical()` with the serial numbers filed off -- every
    /// field it does not deliberately move must still equal `canonical()`'s.
    #[test]
    fn hills_only_moves_the_three_named_fields() {
        let hills = ReliefParams::hills();
        let canonical = ReliefParams::canonical();
        assert_eq!(hills.canonical_wavelength_m, canonical.canonical_wavelength_m);
        assert_eq!(hills.coarsest_wavelength_m, canonical.coarsest_wavelength_m);
        assert_eq!(hills.abyssal_m, canonical.abyssal_m);
        assert_eq!(hills.shelf_m, canonical.shelf_m);
        assert_eq!(hills.coast_m, canonical.coast_m);
        assert_eq!(hills.interior_m, canonical.interior_m);
        assert_eq!(hills.quieting_scale_m, canonical.quieting_scale_m);
        assert_eq!(hills.mountain_m, canonical.mountain_m * 4.0);
        assert_eq!(hills.quieting_strength, -canonical.quieting_strength);
        assert_eq!(hills.octave_persistence, 0.65);
    }

    /// Ruling 1's central claim, restated at the point this task touches: adding
    /// `hills()` beside `canonical()` must not perturb what `None` means.
    /// `surface.rs::relief_none_matches_relief_some_canonical_bit_for_bit` already checks
    /// this at the full `Surface::elevation_m` level over 5,402 comparisons; this is the
    /// same claim checked directly at `Detail::offset_m`, so a reviewer of this file does
    /// not have to cross to `surface.rs` to see it hold.
    #[test]
    fn canonical_is_still_bit_identical_to_none_after_adding_hills() {
        let with_none = Detail::new(20260905, EARTH_RADIUS_M, None);
        let with_canonical = Detail::new(20260905, EARTH_RADIUS_M, Some(ReliefParams::canonical()));
        let p = SpherePoint::from_latlon(12.0, 34.0);
        for resolution in [None, Some(5000.0)] {
            let a = with_none.offset_m(&p, 100.0, resolution);
            let b = with_canonical.offset_m(&p, 100.0, resolution);
            assert_eq!(
                a.to_bits(),
                b.to_bits(),
                "None and Some(canonical()) must stay bit-identical at resolution {resolution:?}"
            );
        }
    }

    /// The mechanism `hills()` is FOR, isolated from noise sampling: at a site with large
    /// deliberate tectonic offset (what Task 2 called the "peak population"),
    /// `amplitude_m` must come out materially larger under `hills()` than under
    /// `canonical()`. `tectonic_m = 1200.0` fully saturates the quieting term's own scale
    /// (`quieting_scale_m`) on both sides, so this isolates the sign flip and the
    /// `mountain_m` multiplier from any partial-saturation effect.
    ///
    /// **Verified by mutation, not just written and trusted**: reverting `hills()` to
    /// `canonical()`'s `quieting_strength` (0.7) while leaving `mountain_m` at x4 still
    /// grows amplitude (mountain_m alone does that), so a weak version of this assertion
    /// could pass under a mutation that silently drops the sign flip. Run directly against
    /// that mutation with `hills_only_moves_the_three_named_fields` disabled (so it could
    /// not shadow this one by failing first): `mountain_m` alone gives an EXACT ratio of
    /// `4.0 * (1.0 - 0.7) / (1.0 - 0.7) = 4.0` at this tectonic magnitude -- not the
    /// "~2x" a rougher estimate might guess, measured directly instead. The ratio bound
    /// below (>= 10.0) is calibrated to that measurement: a mutation dropping the sign
    /// flip but keeping the x4 multiplier gives exactly 4.0, comfortably under 10.0, while
    /// `hills()` unmutated gives 22.67, comfortably over it. This is the sibling this
    /// test's "materially" claim could otherwise be shadowed by, and the exact-ratio
    /// assertion just above it would also have caught this same mutation on its own.
    #[test]
    fn hills_gives_materially_more_amplitude_than_canonical_on_tectonic_ground() {
        let peak_point = SpherePoint::from_latlon(0.0, 0.0);
        let peak_elevation_m = 2000.0; // saturates `high` in amplitude_m, as in a_mountain_gives_exactly_mountain_m
        let saturating_tectonic_m = 1200.0; // == canonical()'s quieting_scale_m: smooth(1.0) = 1.0 on both sides

        let canonical = Detail::new(20260905, EARTH_RADIUS_M, Some(ReliefParams::canonical()));
        let hills = Detail::new(20260905, EARTH_RADIUS_M, Some(ReliefParams::hills()));

        let canonical_amplitude =
            canonical.amplitude_m(&peak_point, peak_elevation_m, 0.0, saturating_tectonic_m);
        let hills_amplitude = hills.amplitude_m(&peak_point, peak_elevation_m, 0.0, saturating_tectonic_m);

        // Canonical: MOUNTAIN_M * (1.0 - 0.7) = MOUNTAIN_M * 0.3, fully quieted.
        // hills(): (MOUNTAIN_M * 4.0) * (1.0 - (-0.7)) = MOUNTAIN_M * 4.0 * 1.7, fully
        // amplified instead of quieted. The ratio is exactly (4.0 * 1.7) / 0.3 =
        // 22.666..., a large, exactly-derivable multiple -- not a vague "bigger".
        let want_ratio = (4.0 * 1.7) / 0.3;
        let got_ratio = hills_amplitude / canonical_amplitude;
        assert!(
            (got_ratio - want_ratio).abs() < 1e-9,
            "expected hills()/canonical() amplitude ratio {want_ratio}, got {got_ratio} \
             ({hills_amplitude} / {canonical_amplitude})"
        );
        assert!(
            got_ratio >= 10.0,
            "hills() must give materially (>= 10x) more amplitude than canonical() on \
             saturated tectonic ground, got {got_ratio}x -- a mutation dropping the sign \
             flip but keeping the x4 mountain_m multiplier alone gives exactly 4.0x, which \
             this bound must reject"
        );
    }

    /// The same claim, on the actual peak population Task 2 measured against, over the
    /// full `Surface` pipeline rather than `amplitude_m` in isolation -- noise sampling,
    /// octave planning and all. A modest 19x36 grid (10 deg spacing, 684 candidates,
    /// poles excluded) stands in for Task 2's own denser peak search: cheap enough for a
    /// unit test, ranked by `structural_m` against a canonical reference so the site
    /// chosen does not depend on `ReliefParams` at all, the same guarantee Task 2's own
    /// survey relied on.
    #[test]
    fn hills_gives_materially_more_relief_than_canonical_on_a_real_peak() {
        use crate::continentality::LAND_FRACTION;
        use crate::generation::DEFAULT_PLATE_COUNT;
        use crate::surface::Surface;
        use crate::tangent::TangentFrame;

        const SEED: i64 = 20_260_904; // task-2-report.md's own seed
        const LAT_STEPS: i32 = 17; // -80..=80 at 10 deg, poles excluded (TangentFrame degenerates there)
        const LON_STEPS: i32 = 36; // -180..170 at 10 deg

        let reference = Surface::new(SEED, EARTH_RADIUS_M, DEFAULT_PLATE_COUNT, LAND_FRACTION, None, None, None);

        let mut best: Option<(f64, f64, f64)> = None; // (structural_m, lat, lon)
        for i in 0..LAT_STEPS {
            let lat = -80.0 + (i as f64) * 10.0; // cast-ok: grid index widened to a coordinate
            for j in 0..LON_STEPS {
                let lon = -180.0 + (j as f64) * 10.0; // cast-ok: grid index widened to a coordinate
                let point = SpherePoint::from_latlon(lat, lon);
                let structural_m = reference.structural_m(&point);
                let replace = match best {
                    None => true,
                    Some((best_structural_m, _, _)) => structural_m > best_structural_m,
                };
                if replace {
                    best = Some((structural_m, lat, lon));
                }
            }
        }
        let (peak_structural_m, peak_lat, peak_lon) = best.expect("684-candidate grid is non-empty");
        // Sanity: this must actually be a peak, or the test proves nothing about the
        // population it claims to stand in for.
        assert!(
            peak_structural_m > 300.0,
            "the highest site on this grid ({peak_structural_m} m) is not high ground -- \
             the grid found no real peak to test against"
        );

        let world_canonical = Surface::new(
            SEED,
            EARTH_RADIUS_M,
            DEFAULT_PLATE_COUNT,
            LAND_FRACTION,
            None,
            Some(ReliefParams::canonical()),
            None,
        );
        let world_hills = Surface::new(
            SEED,
            EARTH_RADIUS_M,
            DEFAULT_PLATE_COUNT,
            LAND_FRACTION,
            None,
            Some(ReliefParams::hills()),
            None,
        );

        let relief_of = |surface: &Surface| -> f64 {
            let frame = TangentFrame::at_latlon(peak_lat, peak_lon, EARTH_RADIUS_M);
            let mut min_elevation_m = f64::INFINITY;
            let mut max_elevation_m = f64::NEG_INFINITY;
            let mut x = -1000.0_f64;
            while x <= 1000.0 {
                let p = frame.local_to_sphere(x, 0.0);
                let elevation_m = surface.elevation_m(&p, None);
                if elevation_m < min_elevation_m {
                    min_elevation_m = elevation_m;
                }
                if elevation_m > max_elevation_m {
                    max_elevation_m = elevation_m;
                }
                x += 50.0;
            }
            max_elevation_m - min_elevation_m
        };

        let canonical_relief_m = relief_of(&world_canonical);
        let hills_relief_m = relief_of(&world_hills);

        assert!(
            hills_relief_m > canonical_relief_m * 2.0,
            "hills() must give materially (> 2x) more 2 km transect relief than \
             canonical() at a real peak (lat {peak_lat}, lon {peak_lon}, structural \
             {peak_structural_m} m): canonical {canonical_relief_m} m, hills \
             {hills_relief_m} m"
        );
    }
}
