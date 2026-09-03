//! What the bottom is made of.
//!
//! Ported from `worldbuilder/bathymetry/substrate.py`. Maritime asks two things of a
//! world: how deep the water is, and what is under it. This is the second. An anchor
//! bites in mud and drags on rock; a hull that touches sand is aground and one that
//! touches rock is holed; a dredger can move one and not the other.
//!
//! **A category is the wrong shape for the answer, and the right shape for the
//! question.** The field is a *composition* - three fractions summing to one, each
//! varying smoothly - and the single-word answer is whichever is largest. Nothing
//! continuous is ever computed from the word.
//!
//! This module carries the module constants, `smooth` (reused, not duplicated - see
//! below), the `Composition` type with its normalising constructor, `blended_towards`,
//! `dominant` and `holding`, the `PURE` table, `natural`, `slope_at`, and now `at` and
//! `dominant_at` with the host plumbing they need. Bindings are a later task.
//!
//! **There is no `Substrate` type and no host trait, and that is deliberate.** Python
//! builds `Surface`'s five layers and hands `self` to `Substrate` on the last line of
//! `__init__`, so a Rust `Substrate<'a>` borrowing its host could not be a field of that
//! host - and the Python's own docstring says the thing *holds nothing*. So the four
//! host members Task 1's runtime census measured (`radius_m`, `structural_m(point)`,
//! `tectonics.offset_m(point)` and `features.placed`) are passed in: two callbacks, a
//! radius, and `&Features` concretely, since `Placed` and its load-bearing reach gate
//! are already ported and nothing should stand between this module and them.
//!
//! **Everything here is strict but `slope_at`.** `natural`, `Composition::new`,
//! `blended_towards`, `dominant`, `holding` and `smooth` reach zero transcendentals, so
//! their tests assert raw bits with no tolerance at all. `slope_at` reaches `math.hypot`
//! five times - once itself and once inside each of four `local_to_sphere` calls - and
//! carries the module's one bound, `SLOPE_DRIFT_REL`, measured rather than borrowed.

use crate::detmath as m;
use crate::features::Features;
use crate::sphere::SpherePoint;
use crate::tangent::TangentFrame;

/// The three, and there are only three on purpose.
pub const SAND: &str = "sand";
pub const MUD: &str = "mud";
pub const ROCK: &str = "rock";

/// How steep a bottom has to be before the fines are gone from it. Four per cent is a
/// steep seabed - four metres in a hundred - and a slope twice that is bare.
pub const ROCK_SLOPE: f64 = 0.04;

/// How much tectonic contribution makes ground rock regardless of how flat it is.
pub const ROCK_TECTONIC_M: f64 = 1200.0;

/// Wave base, and how far below it the fines have finished settling. Above the first
/// figure the sea keeps the bottom swept and sandy; below the second it is mud.
pub const SWEPT_M: f64 = -40.0;
pub const SETTLED_M: f64 = -120.0;

/// How far apart the two probes are that measure the slope.
pub const SLOPE_BASELINE_M: f64 = 60.0;

/// `max(0.0, min(1.0, fraction))` then the smoothstep `x * x * (3.0 - 2.0 * x)`.
///
/// `substrate.py`'s `_smooth` is character-for-character `detail.py`'s `_smooth` -
/// `max(0.0, min(1.0, fraction))` then the same smoothstep, in the same operand order -
/// so it is reused from `detail` here rather than adding a fourth copy of an identical
/// function. `shelf.rs` and `features.rs` already reuse it the same way, for the same
/// reason: the two formulas are bit-identical, not merely similar, and a fourth
/// transcription would just be a fourth place for the two to quietly drift apart.
pub use crate::detail::smooth;

/// What a piece of bottom is made of, as fractions that sum to one.
///
/// **Does not encode "the fractions sum to exactly one."** They sum to *very close to*
/// one - an exhaustive sweep of `natural`'s argument domain puts the pre-normalisation
/// total as low as `0.9999999999999998`, two ULP below 1.0 - so the normalising division
/// in `new` is never skipped and never assumed to be a no-op.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Composition {
    pub sand: f64,
    pub mud: f64,
    pub rock: f64,
}

impl Composition {
    /// Normalises the three fractions so they sum to one.
    ///
    /// `total <= 0.0` is a real branch, not a defensive one that never fires: a caller
    /// can construct a `Composition` directly (`PURE`'s own entries do), and
    /// `Composition(0.0, 0.0, 0.0)` must not divide by zero. **This does not converge as
    /// the inputs approach the boundary** - one ULP above zero the result points
    /// whichever way the triple points, and at zero it snaps to pure rock. That
    /// discontinuity is transcribed exactly, not smoothed over: `sand, mud, rock, total =
    /// 0.0, 0.0, 1.0, 1.0` in the Python's own order, so a degenerate triple always comes
    /// out rock, never sand or mud.
    pub fn new(sand: f64, mud: f64, rock: f64) -> Self {
        let total = sand + mud + rock;
        let (sand, mud, rock, total) = if total <= 0.0 {
            (0.0, 0.0, 1.0, 1.0)
        } else {
            (sand, mud, rock, total)
        };
        Composition {
            sand: sand / total,
            mud: mud / total,
            rock: rock / total,
        }
    }

    /// The one-word answer, for callers that want one.
    ///
    /// Tie precedence is ROCK > SAND > MUD, each an independent comparison in the
    /// Python's exact directions: rock wins outright when it is at least sand *and* at
    /// least mud (`>=` both ways, so a three-way tie is rock); otherwise sand wins when
    /// it is at least mud (`>=` again, so a sand/mud tie is sand), and mud is what is
    /// left. This is a genuine cliff — the smallest measured tie margin between two
    /// words is `2.109424e-15` — and no tolerance in a comparison could ever absorb a
    /// flip across it, since the output is a word rather than a number.
    pub fn dominant(&self) -> &'static str {
        if self.rock >= self.sand && self.rock >= self.mud {
            ROCK
        } else if self.sand >= self.mud {
            SAND
        } else {
            MUD
        }
    }

    /// How well an anchor holds here, nothing to one.
    pub fn holding(&self) -> f64 {
        self.mud * 1.0 + self.sand * 0.6
    }

    /// This composition moved some of the way towards another one.
    pub fn blended_towards(&self, other: &Composition, weight: f64) -> Composition {
        let keep = 1.0 - weight;
        Composition::new(
            self.sand * keep + other.sand * weight,
            self.mud * keep + other.mud * weight,
            self.rock * keep + other.rock * weight,
        )
    }
}

/// One pure fraction of each of the three, keyed by name - what a placed feature blends
/// the ground towards when it declares a substrate.
///
/// Looking up a name that is not one of `SAND`, `MUD` or `ROCK` (including the empty
/// string, which the Python's dict lookup treats differently from `None`) is deliberately
/// left undecided here - `Substrate::at`, which is the only consumer, is a later task and
/// is where that decision belongs.
pub fn pure(kind: &str) -> Option<Composition> {
    match kind {
        SAND => Some(Composition::new(1.0, 0.0, 0.0)),
        MUD => Some(Composition::new(0.0, 1.0, 0.0)),
        ROCK => Some(Composition::new(0.0, 0.0, 1.0)),
        _ => None,
    }
}

/// What ordinary ground here would be made of, before anything placed says otherwise.
///
/// Args:
/// elevation_m: The ground.
/// slope: From `slope_at`.
/// tectonic_m: The tectonic contribution.
///
/// Returns:
/// composition: Fractions summing to *very nearly* one - see `Composition`.
///
/// Notes:
/// Rock is claimed first, because steepness and uplift both overrule deposition -
/// fines cannot stay on a slope whatever the water is doing above it. What is left
/// divides between sand and mud on depth alone.
///
/// **Strict: zero transcendentals.** Everything here is `+ - * /` and `smooth`, which
/// is itself two comparisons and a polynomial, so every test below asserts raw bits
/// against the live Python with no tolerance at all.
///
/// **`max` is CPython's two-argument `max`, not `f64::max`.** It returns its FIRST
/// argument unless the second is strictly greater, which is the house form in
/// `plates.rs::margin_at` and `features.rs`. The order is observable rather than
/// cosmetic: `smooth(NaN)` is `1.0` (Python's `min(1.0, nan)` keeps `1.0`), so a NaN
/// argument produces a number rather than a NaN and which branch produced it depends
/// on the operand order.
pub fn natural(elevation_m: f64, slope: f64, tectonic_m: f64) -> Composition {
    let by_slope = smooth(slope / ROCK_SLOPE);
    let by_tectonics = smooth(tectonic_m.abs() / ROCK_TECTONIC_M);
    // Python: `max(_smooth(slope / ROCK_SLOPE), _smooth(abs(tectonic_m) / ROCK_TECTONIC_M))`.
    let rock = if by_tectonics > by_slope { by_tectonics } else { by_slope };
    let swept = smooth((elevation_m - SETTLED_M) / (SWEPT_M - SETTLED_M));
    let loose = 1.0 - rock;
    Composition::new(loose * swept, loose * (1.0 - swept), rock)
}

/// How much this module's ONE bounded answer may differ between the languages, as a
/// fraction of the answer.
///
/// **One ULP, and measured rather than borrowed.** `slope_at` is the only function in
/// `substrate.py` that reaches a transcendental at all, and the only one it reaches is
/// `math.hypot` - once directly, and once inside each of four `local_to_sphere` calls.
/// `hypot` is where CPython and Rust genuinely differ in algorithm: since 3.8 CPython
/// computes its own Neumaier-summed norm instead of calling libm, while `detmath::hypot`
/// is `libm::hypot`.
///
/// Measured over 7,363 points on the demo world, every one of them with the same
/// `structural_m` on both sides so that nothing but `slope_at` itself could differ:
///
/// | population | n | resolution | worst |
/// |---|---|---|---|
/// | pinnacle 2-D grid, +-140 m | 3,721 | 4.667 m/step | **1 ULP, rel 2.212201e-16** |
/// | drying-rock 2-D grid, +-90 m | 1,681 | 4.500 m/step | 1 ULP, rel 2.209807e-16 |
/// | open-water 2-D grid, 20-200 km offshore | 961 | 6,000 x 2,000 m/step | 1 ULP, rel 2.136838e-16 |
/// | area-uniform planetary scatter | 1,000 | - | 1 ULP, rel 2.168714e-16 |
///
/// **The case that produced the bound**: the pinnacle grid point with unit vector
/// `(0xbfe9b4f83b5543f9, 0x3fddf499e3c0d73f, 0xbfd7908693fbdfe0)`, where CPython
/// returns `0.2648427577219641` and `libm` returns `0.26484275772196414` - adjacent
/// doubles, 5.551115e-17 apart. Every one of the 7,363 disagreements is exactly one
/// ULP of the final `hypot`; **the four `local_to_sphere` calls agreed bit-for-bit at
/// every single point**, so none of the drift comes from the probe positions.
///
/// **That last fact is why this bound is fragile, and the fragility is the point.** A
/// single-ULP difference in one probe *coordinate* - which is one `cos`, `sin` or
/// `sqrt` away on a host whose libm differs from CPython's - moves the answer by up to
/// **5.147521e-12 absolute / 3.167834e-09 relative** on that same pinnacle grid, seven
/// orders of magnitude above this bound. So this figure covers `slope_at` **given the
/// same `structural_m` field and the same probe arithmetic**, and it must be
/// re-measured, not assumed, on any host where `local_to_sphere` stops agreeing.
///
/// **Not a bound on the whole stack.** Driving `slope_at` with the *Rust port's* own
/// `structural_m` (shelf + features, on a pinnacle-only demo world) instead of the
/// Python's moves the answer by up to `7.968304e-11` relative, because the port's
/// elevation itself differs by up to 3.07e-12 m. That drift belongs to `shelf.rs` and
/// `features.rs`, not here, and this constant must never be quoted for it.
pub const SLOPE_DRIFT_REL: f64 = 2.3e-16;

/// How steep the ground is, as a rise over a run.
///
/// Args:
/// radius_m: The planet's radius, from the host surface.
/// point: Where.
/// baseline_m: How far apart the probes are. `SLOPE_BASELINE_M` is the default the
/// Python names; Rust has no default arguments, so callers pass it.
/// structural_m: The host's structural ground, as a callback. `Substrate` holds no
/// state and the Python reaches exactly four members of its surface (Task 1's runtime
/// census); this is the only one `slope_at` needs, so it is passed rather than a trait
/// object standing in for a whole `Surface`.
///
/// Returns:
/// slope: Dimensionless. Nothing on a flat bottom.
///
/// Notes:
/// Four probes and a frame, which is the expensive part of this module by a wide
/// margin. It is affordable only because bottom type is asked far less often than
/// depth - a ship anchors once and sounds continuously.
///
/// **The module's only bounded function** - see `SLOPE_DRIFT_REL`. Everything else in
/// `substrate.rs` is strict.
pub fn slope_at(
    radius_m: f64,
    point: &SpherePoint,
    baseline_m: f64,
    structural_m: &dyn Fn(&SpherePoint) -> f64,
) -> f64 {
    let frame = TangentFrame::at(point, radius_m);
    let half = baseline_m * 0.5;
    // Probe order is transcribed, not chosen: east's two probes, then north's. A host
    // `structural_m` that memoises, counts or logs can tell the difference.
    let east = (structural_m(&frame.local_to_sphere(half, 0.0))
        - structural_m(&frame.local_to_sphere(-half, 0.0)))
        / baseline_m;
    let north = (structural_m(&frame.local_to_sphere(0.0, half))
        - structural_m(&frame.local_to_sphere(0.0, -half)))
        / baseline_m;
    m::hypot(east, north)
}


/// What a placed feature declared, that `PURE` has no entry for.
///
/// **The `Option` `pure` returns is a trap, and this is the exit from it.** Python's
/// `PURE[declared]` is a `dict` lookup that raises `KeyError` on any word that is not
/// `SAND`, `MUD` or `ROCK` - and an empty string is such a word. `tests/test_conformance.py`
/// already pins that `substrate=""` survives the FFI crossing *distinct from `None`*
/// (`test_features_substrate_none_survives_the_crossing_as_none_not_as_empty`), so a
/// value this port guarantees can reach `at` is a value that makes the Python raise.
/// Continuing past it - `if let Some(c) = pure(declared)` and on to the next feature -
/// would answer where the Python refuses to, and disagreeing about whether an answer
/// *exists* is the worst divergence this module could carry.
///
/// **Not the same thing as `declared is None`.** That branch is a genuine skip: `None`
/// means "derive the bottom from the shape of the ground", the feature has nothing to
/// say about substrate, and the Python `continue`s before it even asks for a weight.
/// This error is the other case entirely - a feature that *did* declare something, and
/// declared something no composition exists for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownSubstrate {
    /// The word the feature declared, verbatim - including the empty string.
    pub declared: String,
}

impl std::fmt::Display for UnknownSubstrate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "no pure composition for the declared substrate {:?}; Python raises KeyError({:?})",
            self.declared, self.declared,
        )
    }
}

impl std::error::Error for UnknownSubstrate {}

/// What the bottom is made of here.
///
/// Args:
/// radius_m: The planet's radius, from the host surface.
/// point: Where.
/// elevation_m: The structural ground, if already to hand.
/// tectonic_m: The tectonic contribution, if already to hand.
/// slope: The slope, if already to hand.
/// structural_m: The host's structural ground, as a callback.
/// tectonic_offset_m: The host's `tectonics.offset_m`, as a callback.
/// features: The host's placed features, concretely - `Placed` and its reach gate are
/// already ported, so nothing stands between this and `features.rs`.
///
/// Returns:
/// composition: Fractions summing to *very nearly* one - see `Composition`. `Err` where
/// the Python raises `KeyError` - see `UnknownSubstrate`.
///
/// Notes:
/// **The three optionals are `is None` SENTINELS, and a supplied `0.0` is a value.**
/// This codebase holds three non-interchangeable optional idioms - the sentinel here,
/// plain default substitution (`radius_m=EARTH_RADIUS_M`) and `detail.py`'s falsy check
/// where `0.0` counts as absent. Matched with `Option`/`match`, never with a
/// zero-or-falsy test: a caller handing in an elevation of exactly `0.0` (datum) has
/// supplied a value, and re-deriving it would call the host and get a different number.
///
/// **The resolution order is observable and is transcribed, not chosen**: elevation,
/// then tectonic, then slope. Each one left `None` triggers a *different* host call -
/// `structural_m(point)`, `tectonics.offset_m(point)`, and then `slope_at`'s own four
/// `structural_m` probes - so a host that memoises, counts or logs can tell one order
/// from another. Measured against the live Python with a recording proxy: all three
/// `None` gives `[structural, tectonic, structural x4]`, and every partial combination
/// drops exactly its own call from that sequence.
///
/// **`slope` is resolved LAST but declared THIRD**, which is Python's own arrangement
/// (`at(point, elevation_m=None, slope=None, tectonic_m=None)` resolves elevation,
/// tectonic, slope). Task 1 ruled that the Rust parameters run in *resolution* order so
/// the signature and the behaviour cannot drift apart; the keyword names are what a
/// binding must map, not the positions.
///
/// **Iteration over `placed` preserves order**, for the same reason `features.rs` does:
/// blending is not commutative. Two features declaring different substrates over the
/// same water give different answers reversed - measured, e.g. pure ROCK one way and
/// `(0.0, 0.344, 0.656)` the other at the same point.
#[allow(clippy::too_many_arguments)]
pub fn at(
    radius_m: f64,
    point: &SpherePoint,
    elevation_m: Option<f64>,
    tectonic_m: Option<f64>,
    slope: Option<f64>,
    structural_m: &dyn Fn(&SpherePoint) -> f64,
    tectonic_offset_m: &dyn Fn(&SpherePoint) -> f64,
    features: &Features,
) -> Result<Composition, UnknownSubstrate> {
    let elevation_m = match elevation_m {
        Some(value) => value,
        None => structural_m(point),
    };
    let tectonic_m = match tectonic_m {
        Some(value) => value,
        None => tectonic_offset_m(point),
    };
    let slope = match slope {
        Some(value) => value,
        None => slope_at(radius_m, point, SLOPE_BASELINE_M, structural_m),
    };

    let mut composition = natural(elevation_m, slope, tectonic_m);
    for placed in &features.placed {
        let declared = match placed.feature.substrate.as_deref() {
            None => continue,
            Some(declared) => declared,
        };
        let weight = placed.weight_at(point);
        // Python: `if weight > 0.0:`. Transcribed, not simplified away - blending at a
        // weight of exactly zero is NOT the identity, because `blended_towards` re-enters
        // `Composition::new` and the renormalising division there moves fractions that
        // do not sum to exactly one. See the test for the measured populations.
        if weight > 0.0 {
            // Python: `PURE[declared]`, INSIDE the guard, and it raises. The lookup only
            // happens where the feature actually applies, so an unreachable feature
            // declaring nonsense is not an error - and where it does apply, an unknown
            // word must refuse to answer rather than quietly skip the blend.
            let towards = match pure(declared) {
                Some(towards) => towards,
                None => return Err(UnknownSubstrate { declared: declared.to_string() }),
            };
            composition = composition.blended_towards(&towards, weight);
        }
    }
    Ok(composition)
}

/// The one-word answer, which is what the maritime interface asks for.
///
/// Python's `dominant_at(point, **known)` forwards its keywords to `at`; Rust has no
/// `**kwargs`, so the three optionals appear explicitly, in `at`'s own order.
#[allow(clippy::too_many_arguments)]
pub fn dominant_at(
    radius_m: f64,
    point: &SpherePoint,
    elevation_m: Option<f64>,
    tectonic_m: Option<f64>,
    slope: Option<f64>,
    structural_m: &dyn Fn(&SpherePoint) -> f64,
    tectonic_offset_m: &dyn Fn(&SpherePoint) -> f64,
    features: &Features,
) -> Result<&'static str, UnknownSubstrate> {
    at(
        radius_m,
        point,
        elevation_m,
        tectonic_m,
        slope,
        structural_m,
        tectonic_offset_m,
        features,
    )
    .map(|composition| composition.dominant())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::sphere::EARTH_RADIUS_M;

    // -- natural ------------------------------------------------------------------
    //
    // STRICT. Every expected value below is `.to_bits()` measured from the live Python
    // (`.venv/Scripts/python.exe`, CPython 3.11.0) on the demo world
    // `Surface(WORLD_SEED, features=demo_region().features)`. No tolerance appears
    // anywhere in this section, and if one were ever needed that would be a finding.

    /// A row of `(elevation_m, slope, tectonic_m)` with the bits Python's `natural`
    /// returns for it.
    fn check(elevation_m: f64, slope: f64, tectonic_m: f64, sand: u64, mud: u64, rock: u64) {
        let c = natural(elevation_m, slope, tectonic_m);
        assert_eq!(c.sand.to_bits(), sand, "sand at ({elevation_m}, {slope}, {tectonic_m})");
        assert_eq!(c.mud.to_bits(), mud, "mud at ({elevation_m}, {slope}, {tectonic_m})");
        assert_eq!(c.rock.to_bits(), rock, "rock at ({elevation_m}, {slope}, {tectonic_m})");
    }

    // THE SLOPE CLAMP IS NOT DEAD CODE, and the corpus that shows it is a 2-D grid
    // through a small steep feature.
    //
    // Population: the demo world's 140 m pinnacle - `coast.at(8_000, 6_500)`, a 70x70 m
    // RAISE to -3.5 m standing in about 25 m of water. Scan: a 301 x 301 grid over
    // +-140 m, so 90,601 points at 0.9333 m/step, each one a full four-probe
    // `slope_at` at the 60 m baseline.
    //
    //   max slope 0.32564163814958486 = 8.1410 x ROCK_SLOPE
    //
    // For contrast, and this is the trap the slice was warned about:
    //
    //   3,000-point area-uniform planetary scatter   never reaches the clamp - reads DEAD
    //   401-point LINE through the same pinnacle,
    //       0.7000 m/step                            7.6275 x
    //   1,601-point LINE, 0.1750 m/step              7.6275 x - UNCHANGED at 4x density
    //   1,681-point GRID, 7.0000 m/step              8.0602 x
    //   14,641-point GRID, 2.3333 m/step             8.1336 x
    //   90,601-point GRID, 0.9333 m/step             8.1410 x
    //
    // The steepest ground is off-axis, because a feature's weight is a product of two
    // `bump` factors, so a line finds a lower maximum and does not improve with
    // resolution. Resolution does not rescue a line; a second dimension does.
    #[test]
    fn the_slope_clamp_saturates_eight_times_over_inside_the_pinnacle() {
        let steepest = 0.32564163814958486;
        assert!(steepest / ROCK_SLOPE > 8.0, "{}", steepest / ROCK_SLOPE);
        // python: natural(-13.555741119553378, 0.32564163814958486, 141.62312444146505)
        check(
            -13.555741119553378,
            0.32564163814958486,
            141.62312444146505,
            0x0000000000000000,
            0x0000000000000000,
            0x3ff0000000000000,
        );
        assert_eq!(natural(-13.555741119553378, steepest, 141.62312444146505).dominant(), ROCK);
    }

    // The steepest point on the drying rock's own grid: 241 x 241 over +-90 m, 58,081
    // points at 0.7500 m/step, max slope 0.2238756855508825 = 5.5969 x ROCK_SLOPE.
    // (A line through that feature reaches only 4.6182 x.)
    #[test]
    fn the_drying_rock_saturates_the_clamp_five_and_a_half_times_over() {
        let steepest = 0.2238756855508825;
        assert!(steepest / ROCK_SLOPE > 5.5, "{}", steepest / ROCK_SLOPE);
        // python: natural(-8.984513693095938, 0.2238756855508825, 169.1946481820712)
        check(
            -8.984513693095938,
            0.2238756855508825,
            169.1946481820712,
            0x0000000000000000,
            0x0000000000000000,
            0x3ff0000000000000,
        );
    }

    // The three points on the same 90,601-point pinnacle grid that land on the
    // interesting parts of the smoothstep rather than past its top: a hair below the
    // clamp edge, halfway up, and the gentlest ground the grid contains. The first of
    // these carries a sand fraction of 3.4e-8 - a value no tolerance-based test would
    // ever notice, and exactly the kind this module's strictness exists to pin.
    #[test]
    fn natural_matches_the_python_across_the_smoothstep_on_the_pinnacle_grid() {
        // python: slope 0.03999238125468961 = 0.99981 x ROCK_SLOPE
        check(
            -23.452850847355254,
            0.03999238125468961,
            141.69479791264007,
            0x3e7d3620b1800000,
            0x0000000000000000,
            0x3fefffffc593be9d,
        );
        // python: slope 0.020002473162274437 = 0.50006 x ROCK_SLOPE
        check(
            -23.71588971416213,
            0.020002473162274437,
            141.71845343968133,
            0x3fdffe7b015585c6,
            0x0000000000000000,
            0x3fe000c27f553d1d,
        );
        // python: slope 0.00032088213944900457 = 0.00802 x ROCK_SLOPE, dominant sand
        check(
            -23.178383458133315,
            0.00032088213944900457,
            141.64680663229154,
            0x3feec4860de096cd,
            0x0000000000000000,
            0x3fa3b79f21f69332,
        );
        assert_eq!(
            natural(-23.178383458133315, 0.00032088213944900457, 141.64680663229154).dominant(),
            SAND,
        );
    }

    // Depth alone divides what the rock term leaves, and the two wave-base constants are
    // where it divides. Above SWEPT_M it is all sand, below SETTLED_M all mud, and the
    // midpoint is an even split.
    #[test]
    fn depth_divides_the_loose_fraction_between_sand_and_mud() {
        check(-400.0, 0.0, 0.0, 0x0, 0x3ff0000000000000, 0x0); // pure mud
        check(-5.0, 0.0, 0.0, 0x3ff0000000000000, 0x0, 0x0); // pure sand
        check(30.0, 0.0, 0.0, 0x3ff0000000000000, 0x0, 0x0); // above datum, still sand
        check(-80.0, 0.0, 0.0, 0x3fe0000000000000, 0x3fe0000000000000, 0x0); // an even split
        assert_eq!(natural(-400.0, 0.0, 0.0).dominant(), MUD);
        assert_eq!(natural(-5.0, 0.0, 0.0).dominant(), SAND);
        // A sand/mud tie at exactly the midpoint resolves to SAND, per `dominant`.
        assert_eq!(natural(-80.0, 0.0, 0.0).dominant(), SAND);
    }

    // Either term can win the `max`, and the tectonic one is taken through `abs`, so an
    // equal amount of downward tectonic contribution makes the same rock. All three of
    // these produce the identical composition, which is the point: the two paths into
    // `rock` are interchangeable once they agree on a number.
    #[test]
    fn either_term_can_claim_the_rock_and_the_tectonic_one_is_absolute() {
        // 900 / 1200 = 0.75 -> smooth -> 0.84375, against 0.01 / 0.04 = 0.25 -> 0.15625
        check(-80.0, 0.01, 900.0, 0x3fb4000000000000, 0x3fb4000000000000, 0x3feb000000000000);
        // 0.03 / 0.04 = 0.75 -> 0.84375, against 300 / 1200 = 0.25 -> 0.15625
        check(-80.0, 0.03, 300.0, 0x3fb4000000000000, 0x3fb4000000000000, 0x3feb000000000000);
        // the same, downwards
        check(-80.0, 0.0, -900.0, 0x3fb4000000000000, 0x3fb4000000000000, 0x3feb000000000000);
        // both saturated
        check(-80.0, 1.0, 5000.0, 0x0, 0x0, 0x3ff0000000000000);
    }

    // `smooth` clamps unconditionally, so `natural` is total over the whole real line
    // and over the non-finite values too. NaN is the interesting one: Python's
    // `min(1.0, nan)` returns 1.0 (the comparison `nan < 1.0` is false), so
    // `_smooth(nan)` is 1.0 and a NaN slope makes pure ROCK rather than a NaN
    // composition. Bit-for-bit, not "approximately rock".
    #[test]
    fn a_nan_or_infinite_slope_saturates_rather_than_poisoning_the_answer() {
        check(-80.0, f64::NAN, 0.0, 0x0, 0x0, 0x3ff0000000000000);
        check(-80.0, f64::INFINITY, 0.0, 0x0, 0x0, 0x3ff0000000000000);
        check(-80.0, -1.0, 0.0, 0x3fe0000000000000, 0x3fe0000000000000, 0x0);
    }

    // THE COMPOSITION DOES NOT SUM TO ONE, and here is an argument pair that shows it
    // from inside `natural` itself rather than from the abstract (rock, swept) domain.
    //
    // Population: `natural`'s own arguments, swept over elevation in [SETTLED_M,
    // SWEPT_M] and slope in [0, ROCK_SLOPE], 1,201 x 1,201 = 1,442,401 pairs at
    // 0.0667 m and 3.33e-5 per step. Minimum pre-normalisation total
    // 0.9999999999999998 - two ULP below one - at elevation -119.8 m, slope
    // 0.0025666666666666667. A 1,001 x 1,001 grid sample of the (rock, swept) domain
    // reaches the same minimum - it is a sample, not the domain, but the bound it
    // illustrates is algebraic and unconditional: every term of
    // `loose*swept + loose*(1-swept) + rock` is non-negative and
    // `swept + (1-swept) >= 1` in reals, so the pre-normalisation total is bounded below
    // by ~0.5 for any `(rock, swept)` in `[0,1]^2`, independent of any grid's resolution.
    //
    // The consequence is observable: all three fractions differ from the undivided
    // triple, so the normalising division in `Composition::new` is NOT a no-op and must
    // never be skipped, simplified away, or asserted around.
    #[test]
    fn natural_does_not_produce_a_composition_that_sums_to_one() {
        let elevation_m = -119.8;
        let slope = 0.0025666666666666667;
        // python: natural(-119.8, 0.0025666666666666667, 0.0)
        check(
            elevation_m,
            slope,
            0.0,
            0x3ef3655d63b63ec9,
            0x3fef9efd22c1f7db,
            0x3f883704a0d02e62,
        );
        // The same three numbers BEFORE the division, measured from the Python's own
        // intermediates: loose * swept, loose * (1 - swept) and rock.
        let c = natural(elevation_m, slope, 0.0);
        assert_eq!(c.sand.to_bits() - 0x3ef3655d63b63ec8, 1);
        assert_eq!(c.mud.to_bits() - 0x3fef9efd22c1f7d9, 2);
        assert_eq!(c.rock.to_bits() - 0x3f883704a0d02e60, 2);
        // And after normalising, whether the three sum to one depends on the ORDER they
        // are added in - `sand + mud + rock` gives exactly 1.0 here and
        // `rock + mud + sand` gives 1.0000000000000002. Both figures measured from the
        // same Python call. An invariant that is true in one summation order and false
        // in another is not an invariant, which is the whole reason this module states
        // none.
        assert_eq!((c.sand + c.mud + c.rock).to_bits(), 0x3ff0000000000000);
        assert_eq!((c.rock + c.mud + c.sand).to_bits(), 0x3ff0000000000001);
    }

    // -- slope_at -----------------------------------------------------------------

    /// An analytic structural field: a gentle regional tilt with one small steep feature
    /// on it, 140 m across and 20 m proud.
    ///
    /// **Pure arithmetic, so it is bit-identical in both languages by construction** -
    /// no transcendental, no lattice, no noise. That is the whole point: with the same
    /// field on both sides, every difference between Python's `slope_at` and this one is
    /// `slope_at`'s own, which is what `SLOPE_DRIFT_REL` had to be measured over. The
    /// Python twin of this function is in the Task 3 report.
    fn analytic_field(point: &SpherePoint) -> f64 {
        const CX: u64 = 0xbfe9b4f1ee09b585;
        const CY: u64 = 0x3fddf4b589429ce6;
        const CZ: u64 = 0xbfd7907eeec7ac24;
        const HALF_M: f64 = 140.0;
        const RISE_M: f64 = 20.0;
        let v = point.vector;
        let plane = 1000.0 * v.z - 400.0 * v.x;
        let dx = v.x - f64::from_bits(CX);
        let dy = v.y - f64::from_bits(CY);
        let dz = v.z - f64::from_bits(CZ);
        let d2 = (dx * dx + dy * dy + dz * dz) * (EARTH_RADIUS_M * EARTH_RADIUS_M);
        let t = d2 / (HALF_M * HALF_M);
        if t >= 1.0 {
            return plane;
        }
        plane + RISE_M * (1.0 - t) * (1.0 - t)
    }

    fn at_bits(x: u64, y: u64, z: u64) -> SpherePoint {
        SpherePoint {
            vector: crate::vectors::Vec3::new(
                f64::from_bits(x),
                f64::from_bits(y),
                f64::from_bits(z),
            ),
        }
    }

    /// Assert against a Python-measured answer at the measured bound, and report the
    /// margin in units of that bound when it fails.
    fn within_bound(measured: f64, python_bits: u64, label: &str) {
        let expected = f64::from_bits(python_bits);
        let drift = (measured - expected).abs();
        let allowed = SLOPE_DRIFT_REL * expected.abs();
        assert!(
            drift <= allowed,
            "{label}: rust {measured:?} vs python {expected:?}, drift {drift:e} is {:.3}x the measured bound",
            drift / allowed,
        );
    }

    // slope_at against the live Python over the analytic field above, at nine named
    // places: the steepest and gentlest points of a 61 x 61 grid over the feature
    // (+-140 m, 3,721 points at 4.6667 m/step, max slope 0.20512779389794383 = 5.1282 x
    // ROCK_SLOPE), the middle, two half-radius points on each probe axis, open ground
    // well clear of it, both a pole and the equator (where the frame's basis is chosen
    // rather than derived), and one call at a different baseline.
    #[test]
    fn slope_at_matches_the_python_over_an_analytic_field() {
        let r = EARTH_RADIUS_M;
        let f: &dyn Fn(&SpherePoint) -> f64 = &analytic_field;
        for (label, x, y, z, baseline, expected) in [
            ("steepest on the grid", 0xbfe9b4e99f32ff81u64, 0x3fddf4abdae05d49u64,
             0xbfd790af7df72e35u64, 60.0f64, 0x3fca41a0a72569feu64),
            ("gentlest on the grid", 0xbfe9b50e8298177b, 0x3fddf496d6747331,
             0xbfd790293d35b251, 60.0, 0x3f00ca72cc26eb5d),
            ("the middle of the feature", 0xbfe9b4f1ee09b585, 0x3fddf4b589429ce6,
             0xbfd7907eeec7ac24, 60.0, 0x3f2623162c1703fd),
            ("halfway out along east", 0xbfe9b4fd879af116, 0x3fddf48db7bcfc1d,
             0xbfd7907eeec1903f, 60.0, 0x3fc9bec7aa0c81d9),
            ("halfway out along north", 0xbfe9b4f9429ad332, 0x3fddf4be140134be,
             0xbfd7905416093110, 60.0, 0x3fc9ba60834f2ea1),
            ("off the feature entirely", 0xbfe9b64e1ad903ce, 0x3fddea638265acb9,
             0xbfd797aabcfd57a0, 60.0, 0x3f2623004a0adf02),
            ("the north pole", 0x0, 0x0, 0x3ff0000000000000, 60.0, 0x3f1075655d000000),
            ("the equator at longitude zero", 0x3ff0000000000000, 0x0, 0x0, 60.0,
             0x3f2492beb4411111),
            ("a 600 m baseline at the middle", 0xbfe9b4f1ee09b585, 0x3fddf4b589429ce6,
             0xbfd7907eeec7ac24, 600.0, 0x3f2623162cec094a),
        ] {
            let p = at_bits(x, y, z);
            within_bound(slope_at(r, &p, baseline, f), expected, label);
        }
    }

    // ALL NINE OF THOSE AGREE BIT-FOR-BIT, so the test above does not by itself
    // demonstrate that `SLOPE_DRIFT_REL` is needed at all - it only shows the bound is
    // not being leaned on. This one does demonstrate it, from the case the bound was
    // sized to.
    //
    // The operands are the east and north differences `slope_at` formed at the pinnacle
    // grid point `(0xbfe9b4f83b5543f9, 0x3fddf499e3c0d73f, 0xbfd7908693fbdfe0)` - the
    // worst of 7,363 measured points. Both languages were handed the identical pair, and
    // the last call of `slope_at` returns different doubles: CPython's Neumaier-summed
    // `math.hypot` gives `0.2648427577219641`, `libm::hypot` the adjacent double
    // `0.26484275772196414`. One ULP, 5.551115e-17 apart, 2.212201e-16 relative -
    // which is what `SLOPE_DRIFT_REL` covers, with about four per cent to spare.
    #[test]
    fn the_final_hypot_is_where_the_two_languages_actually_disagree() {
        let east = f64::from_bits(0xbfd0b724e4171245);
        let north = f64::from_bits(0x3fa67aabd9f84722);
        let python = f64::from_bits(0x3fd0f32f09bfe3f0);
        let rust = m::hypot(east, north);
        assert_eq!(rust.to_bits(), 0x3fd0f32f09bfe3f1);
        assert_ne!(rust.to_bits(), python.to_bits());
        assert_eq!(rust.to_bits() - python.to_bits(), 1, "exactly one ULP, not more");
        let drift = (rust - python).abs();
        assert!(drift <= SLOPE_DRIFT_REL * python, "{drift:e}");
        // ... and it is not comfortably inside the bound by an order of magnitude, which
        // is what "sized to the measurement" means. If this ever passes with room to
        // spare the bound has been widened by somebody.
        assert!(drift > 0.5 * SLOPE_DRIFT_REL * python, "{drift:e}");
    }

    // The steepest of those nine is 5.13 x ROCK_SLOPE, so the analytic corpus is steep
    // ground and not only gentle ground - the same requirement the pinnacle scans meet
    // for `natural`.
    #[test]
    fn the_analytic_corpus_contains_genuinely_steep_ground() {
        let p = at_bits(0xbfe9b4e99f32ff81, 0x3fddf4abdae05d49, 0xbfd790af7df72e35);
        let slope = slope_at(EARTH_RADIUS_M, &p, 60.0, &analytic_field);
        assert!(slope / ROCK_SLOPE > 5.0, "{}", slope / ROCK_SLOPE);
    }

    // A level bottom has no slope anywhere, at any baseline, including at a pole where
    // the frame's east is chosen rather than derived.
    #[test]
    fn a_level_bottom_has_no_slope() {
        let flat: &dyn Fn(&SpherePoint) -> f64 = &|_: &SpherePoint| -37.5;
        for p in [
            at_bits(0x3ff0000000000000, 0x0, 0x0),
            at_bits(0x0, 0x0, 0x3ff0000000000000),
            at_bits(0xbfe9b4f1ee09b585, 0x3fddf4b589429ce6, 0xbfd7907eeec7ac24),
        ] {
            for baseline in [SLOPE_BASELINE_M, 600.0, 2000.0] {
                assert_eq!(slope_at(EARTH_RADIUS_M, &p, baseline, flat).to_bits(), 0u64);
            }
        }
    }

    // The probes are taken in the Python's order - east positive, east negative, north
    // positive, north negative - and a host that can tell the difference is entitled to.
    #[test]
    fn the_four_probes_are_taken_in_the_pythons_order() {
        use std::cell::RefCell;
        let seen = RefCell::new(Vec::new());
        let recording: &dyn Fn(&SpherePoint) -> f64 = &|p: &SpherePoint| {
            seen.borrow_mut().push(p.vector);
            0.0
        };
        let origin = at_bits(0x3ff0000000000000, 0x0, 0x0);
        slope_at(EARTH_RADIUS_M, &origin, SLOPE_BASELINE_M, recording);
        let probes = seen.borrow();
        assert_eq!(probes.len(), 4);
        let frame = TangentFrame::at(&origin, EARTH_RADIUS_M);
        let half = SLOPE_BASELINE_M * 0.5;
        for (got, want) in probes.iter().zip([
            frame.local_to_sphere(half, 0.0),
            frame.local_to_sphere(-half, 0.0),
            frame.local_to_sphere(0.0, half),
            frame.local_to_sphere(0.0, -half),
        ]) {
            assert_eq!(got.x.to_bits(), want.vector.x.to_bits());
            assert_eq!(got.y.to_bits(), want.vector.y.to_bits());
            assert_eq!(got.z.to_bits(), want.vector.z.to_bits());
        }
    }

    // -- SLOPE_BASELINE_M's two docstring claims, re-derived ------------------------
    //
    // Both are re-derived here rather than copied: the numbers below come out of this
    // crate's own arithmetic, and the Python figures are quoted alongside only so the
    // two can be seen to agree.

    /// The demo world carrying nothing but its pinnacle.
    ///
    /// Seed 20260831, 22 plates, Earth radius, the default land fraction - the same
    /// fixture `shelf.rs` and `tests/test_conformance.py` use - and the one feature:
    /// a 70 x 70 m RAISE to -3.5 m at the demo coast's `at(8_000, 6_500)`, which stands
    /// in about 25 m of water, so the thing is 140 m across and about 20 m proud.
    /// Measured on the Python side: the pinnacle stands clear of the other 24 demo
    /// features, so a world holding only it gives `slope_at` the identical answers the
    /// full demo world does, to the last bit at all three offsets below.
    fn pinnacle_world() -> (crate::shelf::Shelf, crate::features::Features, SpherePoint) {
        let land = crate::continentality::Continentality::new(
            20260831,
            EARTH_RADIUS_M,
            crate::continentality::LAND_FRACTION,
        );
        let plates = crate::generation::plates_for(20260831, 22);
        let tectonics = crate::tectonics::Tectonics::new(plates, land, EARTH_RADIUS_M);
        let shelf = crate::shelf::Shelf::new(tectonics, land, EARTH_RADIUS_M);
        let centre = at_bits(0xbfe9b4f1ee09b585, 0x3fddf4b589429ce6, 0xbfd7907eeec7ac24);
        let pinnacle = crate::features::Feature {
            kind: "pinnacle".to_string(),
            at: centre,
            target_m: -3.5,
            length_m: 70.0,
            width_m: 70.0,
            bearing_deg: 0.0,
            compose: crate::features::RAISE.to_string(),
            marked: true,
            substrate: Some(ROCK.to_string()),
        };
        let features = crate::features::Features::new([pinnacle], EARTH_RADIUS_M);
        (shelf, features, centre)
    }

    // CLAIM ONE: a 600 m baseline aliases a 140 m pinnacle - flat at 130 m, steep at
    // 300 m.
    //
    // The offsets run along the frame's OWN east axis, which is the axis two of the
    // probes are taken on; displacing along another bearing does not reproduce the claim
    // at all, and that is a property of the claim rather than a flaw in it. A 600 m
    // baseline puts its probes at +-300 m, so at 130 m out both probes miss the 70 m
    // half-extent and at 300 m out one probe lands on the summit.
    //
    // Python, on the same world:  130 m -> 0.0030642168819708746 at 600 m
    //                             300 m -> 0.035846816649796255  at 600 m
    #[test]
    fn a_six_hundred_metre_baseline_aliases_the_pinnacle() {
        let (shelf, features, centre) = pinnacle_world();
        let structural: &dyn Fn(&SpherePoint) -> f64 =
            &|p: &SpherePoint| features.apply(p, shelf.elevation_m(p)).0;
        let frame = TangentFrame::at(&centre, EARTH_RADIUS_M);
        let r = EARTH_RADIUS_M;

        // The ordinary seabed hereabouts, measured 5 km clear of the rock.
        let background = slope_at(r, &frame.local_to_sphere(-5_000.0, 0.0), 600.0, structural);

        let near = frame.local_to_sphere(-130.0, 0.0);
        let near_600 = slope_at(r, &near, 600.0, structural);
        let near_60 = slope_at(r, &near, SLOPE_BASELINE_M, structural);
        // Both probes miss: the 600 m answer is the background to within a per cent, and
        // agrees with the short baseline's answer at the same place to four decimal
        // places (measured 4.0e-6 relative here, and 4.0e-6 in the Python).
        assert!((near_600 / background - 1.0).abs() < 1e-2, "{near_600} vs {background}");
        assert!((near_600 / near_60 - 1.0).abs() < 1e-4, "{near_600} vs {near_60}");
        assert!(near_600 / ROCK_SLOPE < 0.1, "{}", near_600 / ROCK_SLOPE);

        let far = frame.local_to_sphere(-300.0, 0.0);
        let far_600 = slope_at(r, &far, 600.0, structural);
        let far_60 = slope_at(r, &far, SLOPE_BASELINE_M, structural);
        // One probe lands on the rock: the 600 m answer is an order of magnitude over
        // the true local slope, and nearly at ROCK_SLOPE - an aliased rock field over
        // ground that is in fact as flat as the ground 170 m closer in.
        assert!(far_600 / far_60 > 11.0, "{far_600} vs {far_60}");
        assert!(far_600 / ROCK_SLOPE > 0.85, "{}", far_600 / ROCK_SLOPE);
        // ... and the SHORT baseline is not fooled at either offset.
        assert!((far_60 / near_60 - 1.0).abs() < 1e-3, "{far_60} vs {near_60}");
    }

    // CLAIM TWO: `structural_m` has no detail on it, so its slope distribution is the
    // same at 300, 600 and 2,000 m and a short baseline costs nothing.
    //
    // Population: the bare world (no features at all - the claim is about structure),
    // 600 area-uniform points by the golden-angle spiral, each probed at all three
    // baselines, 7,200 probes in all. Python, at 1,200 points: p50 9.181e-04 for all
    // three baselines and p90 4.2159e-03 / 4.2158e-03 / 4.2153e-03 for 300 / 600 /
    // 2,000 m, worst relative quantile difference over p5..p99 of 2.88e-04 (300 vs 600)
    // and 1.64e-03 (2,000 vs 600).
    #[test]
    fn the_structural_slope_distribution_does_not_care_about_the_baseline() {
        let land = crate::continentality::Continentality::new(
            20260831,
            EARTH_RADIUS_M,
            crate::continentality::LAND_FRACTION,
        );
        let plates = crate::generation::plates_for(20260831, 22);
        let tectonics = crate::tectonics::Tectonics::new(plates, land, EARTH_RADIUS_M);
        let shelf = crate::shelf::Shelf::new(tectonics, land, EARTH_RADIUS_M);
        let bare = crate::features::Features::new([], EARTH_RADIUS_M);
        let structural: &dyn Fn(&SpherePoint) -> f64 =
            &|p: &SpherePoint| bare.apply(p, shelf.elevation_m(p)).0;

        const COUNT: usize = 600;
        let golden = std::f64::consts::PI * (3.0 - m::sqrt(5.0));
        let points: Vec<SpherePoint> = (0..COUNT)
            .map(|index| {
                let i = index as f64; // cast-ok: a loop counter under 600, exact in f64
                let n = COUNT as f64; // cast-ok: a fixed literal count, exact in f64
                let z = 1.0 - 2.0 * (i + 0.5) / n;
                let lat = m::to_degrees(m::asin(z));
                let lon = (m::to_degrees(golden * i) + 180.0) % 360.0 - 180.0;
                SpherePoint::from_latlon(lat, lon)
            })
            .collect();

        let sorted = |baseline: f64| {
            let mut v: Vec<f64> = points
                .iter()
                .map(|p| slope_at(EARTH_RADIUS_M, p, baseline, structural))
                .collect();
            v.sort_by(|a, b| a.partial_cmp(b).expect("no NaN slopes on a bare world"));
            v
        };
        let short = sorted(300.0);
        let middle = sorted(600.0);
        let long = sorted(2000.0);

        // Quantile against quantile from p5 to p99, skipping the exactly-zero low tail
        // (about a tenth of the planet reads exactly flat at every baseline, which is
        // itself the claim in its strongest form).
        let mut worst_short: f64 = 0.0;
        let mut worst_long: f64 = 0.0;
        for step in 5..100usize {
            let k = step * (COUNT - 1) / 100;
            let reference = middle[k];
            if reference == 0.0 {
                continue;
            }
            let a = (short[k] - reference).abs() / reference;
            let b = (long[k] - reference).abs() / reference;
            worst_short = if a > worst_short { a } else { worst_short };
            worst_long = if b > worst_long { b } else { worst_long };
        }
        assert!(worst_short < 1e-3, "300 m vs 600 m: {worst_short:e}");
        assert!(worst_long < 5e-3, "2000 m vs 600 m: {worst_long:e}");
        // The claim has teeth only if the distribution is not degenerate.
        assert!(middle[COUNT - 1] > 1e-3, "max slope {}", middle[COUNT - 1]);
    }

    #[test]
    fn constants_match_the_python_verbatim() {
        // Raw bits, per this module's own claim to assert that way with no tolerance -
        // see F4 in the task-1-2 review. `SAND`/`MUD`/`ROCK` have no bit representation
        // to compare, so those three stay plain `&str` equality.
        assert_eq!(ROCK_SLOPE.to_bits(), 0x3fa47ae147ae147b);
        assert_eq!(ROCK_TECTONIC_M.to_bits(), 0x4092c00000000000);
        assert_eq!(SWEPT_M.to_bits(), 0xc044000000000000);
        assert_eq!(SETTLED_M.to_bits(), 0xc05e000000000000);
        assert_eq!(SLOPE_BASELINE_M.to_bits(), 0x404e000000000000);
        assert_eq!(SAND, "sand");
        assert_eq!(MUD, "mud");
        assert_eq!(ROCK, "rock");
    }

    #[test]
    fn smooth_saturates_at_both_ends() {
        // Verified against detail::smooth's own suite; repeated here so this module's
        // test file stands alone as evidence that the reused function behaves as this
        // module needs, not only as detail.rs needs. Raw bits per F4 above.
        assert_eq!(smooth(-10.0).to_bits(), 0x0);
        assert_eq!(smooth(10.0).to_bits(), 0x3ff0000000000000);
        assert_eq!(smooth(0.5).to_bits(), 0x3fe0000000000000);
        // The three probes above agree with EITHER `c*c*(3.0-2.0*c)` (this module's
        // form) or the re-associated `3.0*c*c - 2.0*c*c*c` - they only disagree on the
        // 59.7% of `c` in [0,1) where rounding lands differently, and 0.5 is not one of
        // them. `c = 0.3` is: python `0.3*0.3*(3.0-2.0*0.3)` bit-exact
        // `0x3fcba5e353f7ced9`; the re-associated form gives `...ced8`, one ULP low.
        // This one value is what tells the two formulas apart; the saturation probes
        // above do not.
        assert_eq!(smooth(0.3).to_bits(), 0x3fcba5e353f7ced9);
    }

    // The domain-sweep argument above (and `natural`'s own doc comment) rests on a
    // premise this module never pinned: that `smooth`, reused from `detail`, clamps
    // UNCONDITIONALLY, so its output is in `[0,1]` for every `f64` - not just for the
    // in-range fractions `natural` happens to pass it. Population: three domain
    // extremes chosen to trip each clamp on its own - `NaN` and `+inf` bypass the low
    // clamp (`fraction < 1.0` is false for both, so `upper` is forced to `1.0` before
    // the high clamp ever runs) while `-1e308` bypasses the high clamp (`fraction < 1.0`
    // is true, so `upper` carries the huge negative value into the low clamp). Values
    // measured directly from `detail::smooth`'s own arithmetic, not asserted around.
    #[test]
    fn smooth_clamps_unconditionally_even_at_the_extremes_the_domain_sweep_never_visits() {
        assert_eq!(smooth(f64::NAN).to_bits(), 0x3ff0000000000000);
        assert_eq!(smooth(f64::INFINITY).to_bits(), 0x3ff0000000000000);
        assert_eq!(smooth(-1e308).to_bits(), 0x0);
    }

    // python: Composition(0.0, 0.0, 0.0) -> dominant "rock",
    // sand=0x0 mud=0x0 rock=0x3ff0000000000000
    #[test]
    fn a_degenerate_composition_does_not_divide_by_nothing() {
        let c = Composition::new(0.0, 0.0, 0.0);
        assert_eq!(c.sand.to_bits(), 0x0);
        assert_eq!(c.mud.to_bits(), 0x0);
        assert_eq!(c.rock.to_bits(), 0x3ff0000000000000);
        assert_eq!(c.dominant(), ROCK);
    }

    // python: Composition(-1.0, 0.5, 0.4) -> dominant "rock" (total = -0.1 <= 0.0)
    #[test]
    fn a_negative_total_also_snaps_to_pure_rock() {
        let c = Composition::new(-1.0, 0.5, 0.4);
        assert_eq!(c.sand.to_bits(), 0x0);
        assert_eq!(c.mud.to_bits(), 0x0);
        assert_eq!(c.rock.to_bits(), 0x3ff0000000000000);
        assert_eq!(c.dominant(), ROCK);
    }

    // python: Composition(1e-300, -1e-300, 0.0) -> dominant "rock" (total cancels to
    // exact 0.0, which is <= 0.0)
    #[test]
    fn a_total_that_cancels_to_exact_zero_takes_the_guard() {
        let c = Composition::new(1e-300, -1e-300, 0.0);
        assert_eq!(c.sand.to_bits(), 0x0);
        assert_eq!(c.mud.to_bits(), 0x0);
        assert_eq!(c.rock.to_bits(), 0x3ff0000000000000);
        assert_eq!(c.dominant(), ROCK);
    }

    // python: Composition(5e-324, 0.0, 0.0) -> dominant "sand". One ULP above zero the
    // guard does not fire, and the fallback direction (pure rock) is not what the
    // triple points to. That is the "does not converge" cliff the brief calls out.
    #[test]
    fn one_ulp_above_zero_the_guard_does_not_fire() {
        let c = Composition::new(5e-324, 0.0, 0.0);
        assert_eq!(c.sand.to_bits(), 0x3ff0000000000000);
        assert_eq!(c.mud.to_bits(), 0x0);
        assert_eq!(c.rock.to_bits(), 0x0);
        assert_eq!(c.dominant(), SAND);
    }

    // python: Composition(1.0, 1.0, 1.0) -> dominant "rock", all three fractions equal
    // (a three-way tie resolves to rock, the first comparison's `>=` on both sides).
    #[test]
    fn a_three_way_tie_resolves_to_rock() {
        let c = Composition::new(1.0, 1.0, 1.0);
        assert_eq!(c.sand.to_bits(), 0x3fd5555555555555);
        assert_eq!(c.mud.to_bits(), 0x3fd5555555555555);
        assert_eq!(c.rock.to_bits(), 0x3fd5555555555555);
        assert_eq!(c.dominant(), ROCK);
    }

    // python: Composition(0.5, 0.5, 0.0) -> dominant "sand" (sand/mud tie, no rock;
    // sand wins the second comparison's `>=`).
    #[test]
    fn a_sand_mud_tie_resolves_to_sand() {
        let c = Composition::new(0.5, 0.5, 0.0);
        assert_eq!(c.sand.to_bits(), 0x3fe0000000000000);
        assert_eq!(c.mud.to_bits(), 0x3fe0000000000000);
        assert_eq!(c.rock.to_bits(), 0x0);
        assert_eq!(c.dominant(), SAND);
    }

    // python: Composition(0.0, 0.5, 0.5) -> dominant "rock" (rock/mud tie; rock wins).
    #[test]
    fn a_rock_mud_tie_resolves_to_rock() {
        let c = Composition::new(0.0, 0.5, 0.5);
        assert_eq!(c.dominant(), ROCK);
    }

    // python: Composition(0.5, 0.0, 0.5) -> dominant "rock" (rock/sand tie; rock wins).
    #[test]
    fn a_rock_sand_tie_resolves_to_rock() {
        let c = Composition::new(0.5, 0.0, 0.5);
        assert_eq!(c.dominant(), ROCK);
    }

    // python: Composition(0.0, 1.0, 0.0) -> dominant "mud".
    #[test]
    fn pure_mud_is_mud() {
        let c = Composition::new(0.0, 1.0, 0.0);
        assert_eq!(c.dominant(), MUD);
    }

    // The smallest measured tie margin (2.109424e-15) proves a genuine word-flip is
    // reachable from a one-ULP-scale nudge; these two pin the direction each way.
    // python: Composition(0.5 + 1e-15, 0.5 - 1e-15, 0.0) -> dominant "sand",
    //   sand=0x3fe0000000000009 mud=0x3fdfffffffffffee
    #[test]
    fn a_hairline_sand_lead_over_mud_still_flips_the_word() {
        let c = Composition::new(0.5 + 1e-15, 0.5 - 1e-15, 0.0);
        assert_eq!(c.sand.to_bits(), 0x3fe0000000000009);
        assert_eq!(c.mud.to_bits(), 0x3fdfffffffffffee);
        assert_eq!(c.dominant(), SAND);
    }

    // python: Composition(0.5 - 1e-15, 0.5 + 1e-15, 0.0) -> dominant "mud",
    //   sand=0x3fdfffffffffffee mud=0x3fe0000000000009
    #[test]
    fn a_hairline_mud_lead_over_sand_still_flips_the_word() {
        let c = Composition::new(0.5 - 1e-15, 0.5 + 1e-15, 0.0);
        assert_eq!(c.sand.to_bits(), 0x3fdfffffffffffee);
        assert_eq!(c.mud.to_bits(), 0x3fe0000000000009);
        assert_eq!(c.dominant(), MUD);
    }

    // python: mud=Composition(0,1,0), sand=Composition(1,0,0), rock=Composition(0,0,1)
    //   mud.holding() = 0x3ff0000000000000 (1.0)
    //   sand.holding() = 0x3fe3333333333333 (0.6)
    //   rock.holding() = 0x0 (0.0)
    #[test]
    fn holding_ranks_the_three_the_way_ground_tackle_does() {
        let mud = Composition::new(0.0, 1.0, 0.0);
        let sand = Composition::new(1.0, 0.0, 0.0);
        let rock = Composition::new(0.0, 0.0, 1.0);
        assert_eq!(mud.holding().to_bits(), 0x3ff0000000000000);
        assert_eq!(sand.holding().to_bits(), 0x3fe3333333333333);
        assert_eq!(rock.holding().to_bits(), 0x0);
    }

    // python: PURE[SAND] = Composition(1,0,0), PURE[MUD] = Composition(0,1,0),
    //   PURE[ROCK] = Composition(0,0,1) - each already normalised, so lookup must not
    //   perturb the bits.
    #[test]
    fn pure_gives_back_exactly_normalised_unit_compositions() {
        let sand = pure(SAND).unwrap();
        assert_eq!(sand.sand.to_bits(), 0x3ff0000000000000);
        assert_eq!(sand.mud.to_bits(), 0x0);
        assert_eq!(sand.rock.to_bits(), 0x0);

        let mud = pure(MUD).unwrap();
        assert_eq!(mud.sand.to_bits(), 0x0);
        assert_eq!(mud.mud.to_bits(), 0x3ff0000000000000);
        assert_eq!(mud.rock.to_bits(), 0x0);

        let rock = pure(ROCK).unwrap();
        assert_eq!(rock.sand.to_bits(), 0x0);
        assert_eq!(rock.mud.to_bits(), 0x0);
        assert_eq!(rock.rock.to_bits(), 0x3ff0000000000000);
    }

    #[test]
    fn pure_returns_none_for_an_unrecognised_name() {
        assert_eq!(pure(""), None);
        assert_eq!(pure("kelp"), None);
    }

    // python:
    //   a = Composition(0.2, 0.3, 0.5); b = Composition(0.7, 0.1, 0.2)
    //   a.blended_towards(b, 0.37) ->
    //     sand=0x3fd8a3d70a3d70a4 mud=0x3fcced916872b021 rock=0x3fd8e5604189374c,
    //     dominant "rock"
    #[test]
    fn blended_towards_matches_the_python_bit_for_bit() {
        let a = Composition::new(0.2, 0.3, 0.5);
        let b = Composition::new(0.7, 0.1, 0.2);
        let blended = a.blended_towards(&b, 0.37);
        assert_eq!(blended.sand.to_bits(), 0x3fd8a3d70a3d70a4);
        assert_eq!(blended.mud.to_bits(), 0x3fcced916872b021);
        assert_eq!(blended.rock.to_bits(), 0x3fd8e5604189374c);
        assert_eq!(blended.dominant(), ROCK);
    }

    // python: a.blended_towards(b, 0.0) ->
    //   sand=0x3fc999999999999a mud=0x3fd3333333333333 rock=0x3fe0000000000000
    // (identical to `a` itself, bit for bit - weight 0.0 keeps everything and blends in
    // nothing, but still runs back through Composition's normalising constructor).
    #[test]
    fn blending_with_zero_weight_reproduces_the_original_bit_for_bit() {
        let a = Composition::new(0.2, 0.3, 0.5);
        let b = Composition::new(0.7, 0.1, 0.2);
        let blended = a.blended_towards(&b, 0.0);
        assert_eq!(blended.sand.to_bits(), a.sand.to_bits());
        assert_eq!(blended.mud.to_bits(), a.mud.to_bits());
        assert_eq!(blended.rock.to_bits(), a.rock.to_bits());
        assert_eq!(blended.sand.to_bits(), 0x3fc999999999999a);
        assert_eq!(blended.mud.to_bits(), 0x3fd3333333333333);
        assert_eq!(blended.rock.to_bits(), 0x3fe0000000000000);
    }

    // python: a.blended_towards(b, 1.0) ->
    //   sand=0x3fe6666666666666 mud=0x3fb999999999999a rock=0x3fc999999999999a
    // (identical to `b` itself, bit for bit).
    #[test]
    fn blending_with_full_weight_reproduces_the_other_bit_for_bit() {
        let a = Composition::new(0.2, 0.3, 0.5);
        let b = Composition::new(0.7, 0.1, 0.2);
        let blended = a.blended_towards(&b, 1.0);
        assert_eq!(blended.sand.to_bits(), b.sand.to_bits());
        assert_eq!(blended.mud.to_bits(), b.mud.to_bits());
        assert_eq!(blended.rock.to_bits(), b.rock.to_bits());
        assert_eq!(blended.sand.to_bits(), 0x3fe6666666666666);
        assert_eq!(blended.mud.to_bits(), 0x3fb999999999999a);
        assert_eq!(blended.rock.to_bits(), 0x3fc999999999999a);
    }

    // `blended_towards` calls `Composition::new`, not a raw struct literal, and this is
    // the test that notices if that stops being true. The three tests above cannot: `a`
    // and `b` are each built from operands that happen to sum to exactly 1.0, `keep` is
    // exactly `1.0 - weight` for every weight tried, and `keep + weight` rounds back to
    // exactly 1.0 for all of them - so the pre-normalisation total is exactly 1.0 and
    // `Composition::new`'s division is a no-op that a raw struct literal would not be
    // missed by.
    //
    // Population: one concrete pair, not a scan - `a = natural(-119.8,
    // 0.0025666666666666667, 0.0)` (the same argument pair the sum-to-one test above
    // measures a 2-ULP-low pre-normalisation total for) blended towards
    // `b = Composition::new(0.7, 0.1, 0.2)` at `weight = 0.1`. At this weight the raw,
    // un-normalised total is `1.0000000000000002` - one ULP high - so the normalising
    // division moves every one of the three fields by exactly one bit. Measured
    // directly from this arithmetic (Python, IEEE-754 double, bit-identical to Rust's
    // `+`/`*`/`/` here): dropping the division would give
    // `sand=0x3fb1ec9c6bf920f8 mud=0x3fecc768f133b0fe rock=0x3f9f607029a55c8e` instead of
    // the normalised values asserted below.
    #[test]
    fn blended_towards_cannot_skip_compositions_normalising_division() {
        let a = natural(-119.8, 0.0025666666666666667, 0.0);
        let b = Composition::new(0.7, 0.1, 0.2);
        let blended = a.blended_towards(&b, 0.1);
        assert_eq!(blended.sand.to_bits(), 0x3fb1ec9c6bf920f7);
        assert_eq!(blended.mud.to_bits(), 0x3fecc768f133b0fc);
        assert_eq!(blended.rock.to_bits(), 0x3f9f607029a55c8c);
    }

    // -- at -----------------------------------------------------------------------
    //
    // The host is a duck-typed stand-in on both sides: Task 1's runtime census found
    // `Substrate` reaches exactly four members of its surface, so the Python twin of
    // every test below is a plain object carrying `radius_m`, `structural_m`,
    // `tectonics.offset_m` and `features.placed` - no `Surface` anywhere. That is what
    // makes these cases STRICT rather than bounded: the structural field is
    // `analytic_field` less 500 m (pure arithmetic, bit-identical by construction, see
    // `slope_at`'s corpus above) and the tectonic field is a plane through the unit
    // vector, so nothing but `at` itself can differ.

    /// The analytic field, dropped 500 m so the bottom is genuinely below wave base.
    ///
    /// Deliberately deep: at the shallow field's own elevations `swept` saturates at
    /// 1.0, so supplying `elevation_m=0.0` and letting the host be asked produce the
    /// *same* composition and the sentinel test proves nothing. 500 m down, a supplied
    /// `0.0` is all sand and a derived `-526.86` is all mud.
    fn deep_field(point: &SpherePoint) -> f64 {
        analytic_field(point) - 500.0
    }

    /// A tectonic contribution that is a plane through the unit vector - arithmetic
    /// only, so it too is bit-identical in both languages.
    fn analytic_tectonic(point: &SpherePoint) -> f64 {
        900.0 * point.vector.y - 300.0 * point.vector.x
    }

    fn declared_feature(
        kind: &str,
        centre: SpherePoint,
        length_m: f64,
        width_m: f64,
        bearing_deg: f64,
        substrate: Option<&str>,
    ) -> crate::features::Feature {
        crate::features::Feature {
            kind: kind.to_string(),
            at: centre,
            target_m: -3.5,
            length_m,
            width_m,
            bearing_deg,
            compose: crate::features::RAISE.to_string(),
            marked: true,
            substrate: substrate.map(|word| word.to_string()),
        }
    }

    /// The three features the corpus is built on, at the centres the Python placed them:
    /// a 70x70 ROCK on the analytic feature's own middle, a 300x120 MUD 60 m east of it
    /// on a 30-degree bearing, and a 200x200 with `substrate=None` 40 m north.
    fn rock_feature() -> crate::features::Feature {
        declared_feature(
            "rock",
            at_bits(0xbfe9b4f1ee09b585, 0x3fddf4b589429ce6, 0xbfd7907eeec7ac24),
            70.0,
            70.0,
            0.0,
            Some(ROCK),
        )
    }

    fn mud_feature() -> crate::features::Feature {
        declared_feature(
            "mud",
            at_bits(0xbfe9b4fbdf627056, 0x3fddf49367f598cc, 0xbfd7907eeec32f28),
            300.0,
            120.0,
            30.0,
            Some(MUD),
        )
    }

    fn plain_feature() -> crate::features::Feature {
        declared_feature(
            "plain",
            at_bits(0xbfe9b4f61e5e43b9, 0x3fddf4ba6ad5ff1d, 0xbfd7906672ee9ba8),
            200.0,
            200.0,
            0.0,
            None,
        )
    }

    fn corpus_features() -> crate::features::Features {
        crate::features::Features::new(
            [rock_feature(), mud_feature(), plain_feature()],
            EARTH_RADIUS_M,
        )
    }

    /// `at` with the corpus host, and the three optionals as given.
    fn corpus_at(
        point: &SpherePoint,
        elevation_m: Option<f64>,
        tectonic_m: Option<f64>,
        slope: Option<f64>,
        features: &crate::features::Features,
    ) -> Result<Composition, UnknownSubstrate> {
        at(
            EARTH_RADIUS_M,
            point,
            elevation_m,
            tectonic_m,
            slope,
            &deep_field,
            &analytic_tectonic,
            features,
        )
    }

    /// The three values every STRICT case below supplies, so that no host call and no
    /// transcendental stands between the two languages: the elevation and slope that
    /// `natural`'s own does-not-sum-to-one case uses, and a flat tectonic field.
    const STRICT_ELEVATION_M: f64 = -119.8;
    const STRICT_SLOPE: f64 = 0.0025666666666666667;
    const STRICT_TECTONIC_M: f64 = 0.0;

    fn strict_at(
        point: &SpherePoint,
        features: &crate::features::Features,
    ) -> Result<Composition, UnknownSubstrate> {
        corpus_at(
            point,
            Some(STRICT_ELEVATION_M),
            Some(STRICT_TECTONIC_M),
            Some(STRICT_SLOPE),
            features,
        )
    }

    fn assert_bits(c: &Composition, sand: u64, mud: u64, rock: u64, label: &str) {
        assert_eq!(c.sand.to_bits(), sand, "sand at {label}");
        assert_eq!(c.mud.to_bits(), mud, "mud at {label}");
        assert_eq!(c.rock.to_bits(), rock, "rock at {label}");
    }

    // THE THREE OPTIONALS ARE `is None` SENTINELS, AND THE RESOLUTION ORDER IS
    // OBSERVABLE.
    //
    // Both halves measured against the live Python with a recording proxy standing in
    // for the host. All three `None` gives
    //
    //     ['structural', 'tectonic', 'structural', 'structural', 'structural', 'structural']
    //
    // - one structural probe for the elevation, one tectonic, then `slope_at`'s four -
    // and each optional that IS supplied removes exactly its own call and nothing else.
    // A supplied `0.0` removes the call too, which is the whole point: `0.0` is the
    // datum, a perfectly ordinary elevation, and a falsy check would re-derive it.
    #[test]
    fn the_three_optionals_are_none_sentinels_resolved_in_the_pythons_order() {
        use std::cell::RefCell;
        let empty = crate::features::Features::new([], EARTH_RADIUS_M);
        let centre = at_bits(0xbfe9b4f1ee09b585, 0x3fddf4b589429ce6, 0xbfd7907eeec7ac24);

        for (elevation_m, tectonic_m, slope, expected) in [
            (
                None,
                None,
                None,
                &["structural", "tectonic", "structural", "structural", "structural", "structural"]
                    [..],
            ),
            (
                Some(0.0),
                None,
                None,
                &["tectonic", "structural", "structural", "structural", "structural"][..],
            ),
            (
                None,
                Some(0.0),
                None,
                &["structural", "structural", "structural", "structural", "structural"][..],
            ),
            (None, None, Some(0.0), &["structural", "tectonic"][..]),
            (Some(0.0), Some(0.0), Some(0.0), &[][..]),
        ] {
            let seen = RefCell::new(Vec::new());
            let structural: &dyn Fn(&SpherePoint) -> f64 = &|p: &SpherePoint| {
                seen.borrow_mut().push("structural");
                deep_field(p)
            };
            let tectonic: &dyn Fn(&SpherePoint) -> f64 = &|p: &SpherePoint| {
                seen.borrow_mut().push("tectonic");
                analytic_tectonic(p)
            };
            at(
                EARTH_RADIUS_M,
                &centre,
                elevation_m,
                tectonic_m,
                slope,
                structural,
                tectonic,
                &empty,
            )
            .expect("no feature declares a substrate here");
            assert_eq!(
                seen.borrow().as_slice(),
                expected,
                "host calls for ({elevation_m:?}, {tectonic_m:?}, {slope:?})",
            );
        }
    }

    // ... and `0.0` is a VALUE, not an absence, in the answer as well as in the call
    // count. The deep field reads -526.86 m at the middle, well below SETTLED_M, so a
    // derived elevation is all mud; a supplied 0.0 is above SWEPT_M and all sand. A
    // falsy check would silently return the first where the caller asked for the second.
    #[test]
    fn a_supplied_elevation_of_exactly_zero_is_a_value_and_not_an_absence() {
        let empty = crate::features::Features::new([], EARTH_RADIUS_M);
        let centre = at_bits(0xbfe9b4f1ee09b585, 0x3fddf4b589429ce6, 0xbfd7907eeec7ac24);
        let derived = corpus_at(&centre, None, Some(0.0), Some(0.0), &empty).unwrap();
        let supplied = corpus_at(&centre, Some(0.0), Some(0.0), Some(0.0), &empty).unwrap();
        // python: Substrate(FakeSurface(deep_field, ...)).at(centre, slope=0.0, tectonic_m=0.0)
        assert_bits(&derived, 0x0, 0x3ff0000000000000, 0x0, "derived elevation");
        assert_bits(&supplied, 0x3ff0000000000000, 0x0, 0x0, "supplied elevation 0.0");
        assert_eq!(derived.dominant(), MUD);
        assert_eq!(supplied.dominant(), SAND);
    }

    // AN UNKNOWN SUBSTRATE IS AN ERROR, AND THE EMPTY STRING IS THE ONE THAT CAN ARRIVE.
    //
    // `PURE[declared]` is a dict lookup in the Python and it raises. Measured on the
    // live Python against this same host: `KeyError('')`, `KeyError('gravel')` and
    // `KeyError('Rock')` - the last because the table is case-sensitive and nothing
    // normalises the word on the way in.
    //
    // `tests/test_conformance.py` already pins that `substrate=""` crosses the FFI
    // boundary as `""` and not as `None`, so this is not a hypothetical input: it is a
    // value the port guarantees can reach here.
    #[test]
    fn an_unknown_substrate_is_an_error_rather_than_a_quietly_skipped_feature() {
        let centre = at_bits(0xbfe9b4f1ee09b585, 0x3fddf4b589429ce6, 0xbfd7907eeec7ac24);
        for word in ["", "gravel", "Rock", "SAND", " sand"] {
            let features = crate::features::Features::new(
                [declared_feature("odd", centre, 70.0, 70.0, 0.0, Some(word))],
                EARTH_RADIUS_M,
            );
            assert_eq!(
                strict_at(&centre, &features),
                Err(UnknownSubstrate { declared: word.to_string() }),
                "substrate {word:?} should have refused to answer",
            );
        }
        // The three real words do not.
        for word in [SAND, MUD, ROCK] {
            let features = crate::features::Features::new(
                [declared_feature("known", centre, 70.0, 70.0, 0.0, Some(word))],
                EARTH_RADIUS_M,
            );
            assert!(strict_at(&centre, &features).is_ok(), "substrate {word:?} should answer");
        }
    }

    // THE LOOKUP IS INSIDE THE GUARD, so a feature declaring nonsense somewhere else on
    // the planet does not poison this point: Python evaluates `PURE[declared]` only when
    // `weight > 0.0`, and at 3 km out the weight is exactly `0.0`. Measured: the Python
    // returns the ordinary composition there and raises at the middle, with the same
    // world both times.
    #[test]
    fn an_unknown_substrate_out_of_reach_does_not_raise() {
        let centre = at_bits(0xbfe9b4f1ee09b585, 0x3fddf4b589429ce6, 0xbfd7907eeec7ac24);
        let far = at_bits(0xbfe9b6e2e2812110, 0x3fddee0ad1a148a9, 0xbfd7907ec2f31ed6);
        let features = crate::features::Features::new(
            [declared_feature("empty", centre, 70.0, 70.0, 0.0, Some(""))],
            EARTH_RADIUS_M,
        );
        assert!(strict_at(&centre, &features).is_err());
        let out_of_reach = strict_at(&far, &features).expect("out of reach, so never looked up");
        // python: the same three bits `natural(-119.8, 0.0025666666666666667, 0.0)` gives
        assert_bits(
            &out_of_reach,
            0x3ef3655d63b63ec9,
            0x3fef9efd22c1f7db,
            0x3f883704a0d02e62,
            "far",
        );
    }

    // A DECLARED `None` IS A GENUINE SKIP, and not the same thing as the error above.
    //
    // In the Python it skips *before* the weight is asked for - measured with a counting
    // proxy round `Placed`, `weight_at` calls came back `[0, 1]` for a `None` feature and
    // a ROCK one. Rust cannot observe that call from outside `Placed`, so what is
    // asserted here is the consequence: the `plain` feature covers the middle at weight
    // 0.896 and changes nothing at all, bit for bit.
    #[test]
    fn a_none_substrate_is_skipped_and_changes_nothing() {
        let centre = at_bits(0xbfe9b4f1ee09b585, 0x3fddf4b589429ce6, 0xbfd7907eeec7ac24);
        let with_plain =
            crate::features::Features::new([rock_feature(), plain_feature()], EARTH_RADIUS_M);
        let without = crate::features::Features::new([rock_feature()], EARTH_RADIUS_M);
        let covered = strict_at(&centre, &with_plain).unwrap();
        let bare = strict_at(&centre, &without).unwrap();
        assert_bits(
            &covered,
            bare.sand.to_bits(),
            bare.mud.to_bits(),
            bare.rock.to_bits(),
            "a None-substrate feature",
        );
        // and it is genuinely overhead - the feature is over the point at weight 0.896.
        let placed = crate::features::Placed::new(plain_feature(), EARTH_RADIUS_M);
        assert!(placed.weight_at(&centre) > 0.89, "{}", placed.weight_at(&centre));
    }

    // THE `weight > 0.0` GUARD IS BIT-OBSERVABLE, and this is one of the points that
    // shows it with no transcendental anywhere in the comparison.
    //
    // 69 m west and 25 m north of the middle: the ROCK feature is still on the point at
    // weight 4.296169e-4, and the MUD feature's weight is exactly `0.0`. Guarded, MUD is
    // skipped and the answer is the ROCK blend; ungated, `blended_towards(PURE[MUD],
    // 0.0)` re-enters `Composition::new` with `keep = 1.0`, and the renormalising
    // division there is NOT a no-op - the three fractions do not sum to exactly one, so
    // every one of them moves by an ULP.
    //
    // POPULATION MATTERS AND IS QUOTED WITH EVERY RATE. On the real `at()` path over the
    // demo coast - a 61 x 61 grid at 1,500 m per step, 3,721 points, the full 25-feature
    // demo world - guarded and ungated differ at **67 points, 1.80%**, worst absolute
    // shift 2.220446e-16, worst relative 1.249555e-15, worst 11 ULP, and zero `dominant`
    // flips. The same grid at 50 m per step differs at 167 points (4.49%); over
    // uniform-random `Composition` triples the rate is 19.24%; and it is the population,
    // not the guard, that moves those figures.
    #[test]
    fn the_weight_guard_is_bit_observable_and_is_not_an_optimisation() {
        let guard_point = at_bits(0xbfe9b4e91d0cebc8, 0x3fddf4dfd6169558, 0xbfd7906fa15a89ff);
        let features = corpus_features();
        let got = strict_at(&guard_point, &features).unwrap();
        // python: the guarded answer, which is the one the port must produce.
        assert_bits(
            &got,
            0x3ef3633b4a02648a,
            0x3fef9b82d5522e28,
            0x3f8915990dcf748b,
            "the guarded answer",
        );

        // The ungated answer, computed here from this crate's own arithmetic, so the
        // difference is demonstrated rather than asserted from a comment.
        let mut ungated = natural(STRICT_ELEVATION_M, STRICT_SLOPE, STRICT_TECTONIC_M);
        for placed in &features.placed {
            let declared = match placed.feature.substrate.as_deref() {
                None => continue,
                Some(declared) => declared,
            };
            let towards = pure(declared).unwrap();
            ungated = ungated.blended_towards(&towards, placed.weight_at(&guard_point));
        }
        // python: the ungated sand, one ULP above the guarded one.
        assert_eq!(ungated.sand.to_bits(), 0x3ef3633b4a02648b);
        assert_ne!(ungated.sand.to_bits(), got.sand.to_bits());
        assert_ne!(ungated.mud.to_bits(), got.mud.to_bits());
        assert_ne!(ungated.rock.to_bits(), got.rock.to_bits());
        // and the MUD feature really is at exactly zero weight here, which is the
        // condition the guard is reading.
        let mud = crate::features::Placed::new(mud_feature(), EARTH_RADIUS_M);
        assert_eq!(mud.weight_at(&guard_point).to_bits(), 0u64);
        let rock = crate::features::Placed::new(rock_feature(), EARTH_RADIUS_M);
        assert!(rock.weight_at(&guard_point) > 0.0);
    }

    // ITERATION OVER `placed` PRESERVES ORDER, because blending is not commutative and
    // the last feature to speak has the loudest voice. Both orders measured on the live
    // Python over the same three points.
    #[test]
    fn the_order_of_placed_is_preserved_because_blending_is_not_commutative() {
        let forward = corpus_features();
        let reversed = crate::features::Features::new(
            [mud_feature(), rock_feature(), plain_feature()],
            EARTH_RADIUS_M,
        );
        for (label, x, y, z, forwards, backwards) in [
            (
                "30 m east",
                0xbfe9b4f6e6b74c4au64,
                0x3fddf4a4789d8801u64,
                0xbfd7907eeec68ce5u64,
                [0x3eaef3cde062f068u64, 0x3fed85b9b15cacf8u64, 0x3fb3d222fb33a810u64],
                [0x3eaef3cde062f068u64, 0x3fd926d65e2af2fbu64, 0x3fe36c92e1ada87cu64],
            ),
            (
                "60 m east, 20 m north",
                0xbfe9b4fdf78d42b7,
                0x3fddf495d8bfec34,
                0xbfd79072b0d72695,
                [0x3ea142b59491f32b, 0x3feff2b6d71cd17a, 0x3f5a90296faa7983],
                [0x3ea142b59491f32d, 0x3fee917b206cf983, 0x3fa6e83cb67ad349],
            ),
            (
                "the middle, where reversing changes the WORD",
                0xbfe9b4f1ee09b585,
                0x3fddf4b589429ce6,
                0xbfd7907eeec7ac24,
                [0x0, 0x3fe2a8a0501428b5, 0x3fdaaebf5fd7ae96],
                [0x0, 0x0, 0x3ff0000000000000],
            ),
        ] {
            let p = at_bits(x, y, z);
            let one_way = strict_at(&p, &forward).unwrap();
            let other_way = strict_at(&p, &reversed).unwrap();
            assert_bits(&one_way, forwards[0], forwards[1], forwards[2], label);
            assert_bits(&other_way, backwards[0], backwards[1], backwards[2], label);
            assert_ne!(
                one_way, other_way,
                "{label}: reversing the order changed nothing, so order is not preserved",
            );
        }
        // At the middle the ROCK feature has weight exactly 1.0, so with ROCK last the
        // answer is pure rock and with MUD last it is not - a different word, not a
        // different ULP.
        let middle = at_bits(0xbfe9b4f1ee09b585, 0x3fddf4b589429ce6, 0xbfd7907eeec7ac24);
        assert_eq!(strict_at(&middle, &forward).unwrap().dominant(), MUD);
        assert_eq!(strict_at(&middle, &reversed).unwrap().dominant(), ROCK);
    }

    // `at` against the live Python at six points of the corpus, every optional supplied,
    // so this is STRICT - raw bits, no tolerance.
    #[test]
    fn at_matches_the_python_bit_for_bit_with_the_optionals_supplied() {
        let features = corpus_features();
        for (label, x, y, z, sand, mud, rock) in [
            (
                "the middle",
                0xbfe9b4f1ee09b585u64,
                0x3fddf4b589429ce6u64,
                0xbfd7907eeec7ac24u64,
                0x0u64,
                0x3fe2a8a0501428b5u64,
                0x3fdaaebf5fd7ae96u64,
            ),
            (
                "30 m east",
                0xbfe9b4f6e6b74c4a,
                0x3fddf4a4789d8801,
                0xbfd7907eeec68ce5,
                0x3eaef3cde062f068,
                0x3fed85b9b15cacf8,
                0x3fb3d222fb33a810,
            ),
            (
                "60 m east, 20 m north",
                0xbfe9b4fdf78d42b7,
                0x3fddf495d8bfec34,
                0xbfd79072b0d72695,
                0x3ea142b59491f32b,
                0x3feff2b6d71cd17a,
                0x3f5a90296faa7983,
            ),
            (
                "40 m north",
                0xbfe9b4f61e5e43b9,
                0x3fddf4ba6ad5ff1d,
                0xbfd7906672ee9ba8,
                0x3ede7a244cc955ab,
                0x3fe7b140f953c285,
                0x3fd09d5f93342e2c,
            ),
            (
                "3 km out, clear of everything",
                0xbfe9b6e2e2812110,
                0x3fddee0ad1a148a9,
                0xbfd7907ec2f31ed6,
                0x3ef3655d63b63ec9,
                0x3fef9efd22c1f7db,
                0x3f883704a0d02e62,
            ),
            (
                "the guard point",
                0xbfe9b4e91d0cebc8,
                0x3fddf4dfd6169558,
                0xbfd7906fa15a89ff,
                0x3ef3633b4a02648a,
                0x3fef9b82d5522e28,
                0x3f8915990dcf748b,
            ),
        ] {
            let p = at_bits(x, y, z);
            assert_bits(&strict_at(&p, &features).unwrap(), sand, mud, rock, label);
        }
    }

    // The fully-resolved path - every optional `None`, so the host is asked five times
    // and `slope_at` runs - agrees bit-for-bit with handing the same three numbers in.
    // Cross-language this path carries `slope_at`'s bound and nothing else new, so it is
    // pinned here against this crate's own arithmetic rather than against a second
    // measured constant; the Python's own words at these points are quoted alongside.
    #[test]
    fn resolving_the_optionals_gives_the_same_answer_as_supplying_them() {
        let features = corpus_features();
        for (label, x, y, z) in [
            ("the middle", 0xbfe9b4f1ee09b585u64, 0x3fddf4b589429ce6u64, 0xbfd7907eeec7ac24u64),
            ("30 m east", 0xbfe9b4f6e6b74c4a, 0x3fddf4a4789d8801, 0xbfd7907eeec68ce5),
            ("40 m north", 0xbfe9b4f61e5e43b9, 0x3fddf4ba6ad5ff1d, 0xbfd7906672ee9ba8),
        ] {
            let p = at_bits(x, y, z);
            let resolved = corpus_at(&p, None, None, None, &features).unwrap();
            let supplied = corpus_at(
                &p,
                Some(deep_field(&p)),
                Some(analytic_tectonic(&p)),
                Some(slope_at(EARTH_RADIUS_M, &p, SLOPE_BASELINE_M, &deep_field)),
                &features,
            )
            .unwrap();
            assert_eq!(resolved, supplied, "{label}");
        }
        // python, resolving everything: 'mud' at the middle, 'rock' 40 m north.
        let middle = at_bits(0xbfe9b4f1ee09b585, 0x3fddf4b589429ce6, 0xbfd7907eeec7ac24);
        let north = at_bits(0xbfe9b4f61e5e43b9, 0x3fddf4ba6ad5ff1d, 0xbfd7906672ee9ba8);
        assert_eq!(corpus_at(&middle, None, None, None, &features).unwrap().dominant(), MUD);
        assert_eq!(corpus_at(&north, None, None, None, &features).unwrap().dominant(), ROCK);
    }

    // `dominant_at` is `at().dominant`, error and all - it must not answer a word where
    // `at` refuses to answer at all.
    #[test]
    fn dominant_at_is_at_dominant_and_carries_the_same_refusal() {
        let features = corpus_features();
        let middle = at_bits(0xbfe9b4f1ee09b585, 0x3fddf4b589429ce6, 0xbfd7907eeec7ac24);
        let word = dominant_at(
            EARTH_RADIUS_M,
            &middle,
            None,
            None,
            None,
            &deep_field,
            &analytic_tectonic,
            &features,
        );
        assert_eq!(word, Ok(MUD));

        let odd = crate::features::Features::new(
            [declared_feature("odd", middle, 70.0, 70.0, 0.0, Some(""))],
            EARTH_RADIUS_M,
        );
        assert_eq!(
            dominant_at(
                EARTH_RADIUS_M,
                &middle,
                Some(STRICT_ELEVATION_M),
                Some(STRICT_TECTONIC_M),
                Some(STRICT_SLOPE),
                &deep_field,
                &analytic_tectonic,
                &odd,
            ),
            Err(UnknownSubstrate { declared: String::new() }),
        );
    }
}
