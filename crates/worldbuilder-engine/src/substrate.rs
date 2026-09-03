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
//! `dominant` and `holding`, and the `PURE` table. `Substrate`'s `natural`, `slope_at`
//! and `at` are a later task.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_match_the_python_verbatim() {
        assert_eq!(ROCK_SLOPE, 0.04);
        assert_eq!(ROCK_TECTONIC_M, 1200.0);
        assert_eq!(SWEPT_M, -40.0);
        assert_eq!(SETTLED_M, -120.0);
        assert_eq!(SLOPE_BASELINE_M, 60.0);
        assert_eq!(SAND, "sand");
        assert_eq!(MUD, "mud");
        assert_eq!(ROCK, "rock");
    }

    #[test]
    fn smooth_saturates_at_both_ends() {
        // Verified against detail::smooth's own suite; repeated here so this module's
        // test file stands alone as evidence that the reused function behaves as this
        // module needs, not only as detail.rs needs.
        assert_eq!(smooth(-10.0), 0.0);
        assert_eq!(smooth(10.0), 1.0);
        assert_eq!(smooth(0.5), 0.5);
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
}
