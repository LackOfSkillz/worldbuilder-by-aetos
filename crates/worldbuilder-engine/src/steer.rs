//! The steering gradient: a world-anchored lattice of `grad(structural_m)`.
//!
//! **What this exists for.** The gully kernel in `detail.rs` needs to know which way the
//! ground falls. `.superpowers/sdd/notes/gradient-probe.md` measured, on this generator,
//! that the signal it must steer on is `grad(structural_m)` and not `grad(elevation_m)`:
//!
//! - On `ReliefParams::hills()` -- a preset this crate already ships -- `grad(elevation)` is
//!   **13.7x** the structural term and points a median **83.66 degrees** away from it, which
//!   is uncorrelated. A kernel steered on the full gradient works on the default world and
//!   degenerates the moment a preset is selected.
//! - `grad(structural_m)`'s *direction* is invariant to the differencing step to a p95 of
//!   **0.02 degrees** across a 26x range (76.35 m to 2,000 m). The full gradient's is not:
//!   p95 **71 degrees** across the same range, so there is no principled step for it.
//! - A term inside `elevation_m` steering on `grad(elevation_m)` steers on itself.
//!   `structural_m` is defined before detail is added, so there is no loop.
//!
//! **Why a lattice rather than four extra calls per texel.** The same probe measured the
//! naive per-texel form -- a `TangentFrame` plus four `structural_m` calls at every texel --
//! at **2.0x to 4.6x the entire elevation fill of the same tile**, across 18 measurements.
//! Only a step-invariant field can be cached on a coarse lattice, and §2.4 measured that it
//! is. So the gradient is evaluated on a fixed lattice and interpolated.
//!
//! **World-anchored, not tile-anchored, and that is the one property an implementation
//! could silently give away.** The lattice is a function of position on the planet and of
//! nothing else -- not of the caller's grid, not of a tile origin, not of a resolution. A
//! gradient taken from the query grid would make the gully displacement a function of *how
//! the caller sampled*, so a terrain mesh and a relief raster would disagree about where a
//! gully is, and a resolution-dependent term would enter a height the Python conformance
//! oracle has to reproduce.
//!
//! # The lattice geometry, and why it is a cubic lattice rather than lat/lon
//!
//! `gradient-probe.md` §1.3 measured a **tangent-plane** lattice inside one tile and said in
//! as many words that the spherical case was not measured. It named two candidates and
//! chose neither: `stream::spiral_point`'s analytic inverse, and a lat/lon lattice with a
//! pole rule.
//!
//! This is neither, and the reason is a cost cliff rather than taste. A lat/lon lattice at a
//! constant longitude step has a node spacing of `R cos(lat) dlon`, so a tile of fixed size
//! in metres crosses **more** cells the nearer it is to a pole -- at 89.9 degrees, 570 times
//! as many. A lattice whose cost depends on where you are looking is a lattice that is cheap
//! everywhere the probe measured and ruinous where it did not.
//!
//! A cubic lattice in unit-sphere space has no poles, no seam, and no such term: the eight
//! corners of the containing cell are integer arithmetic, the interpolation is trilinear,
//! and the node count per tile is the same at the equator and at the pole. `noise.rs`
//! already samples the sphere this way, and for the same reason -- "a two-dimensional field
//! cannot be wrapped onto a sphere without a seam down one meridian and a pinch at each
//! pole".
//!
//! Its error against the exact per-texel gradient is **measured, not assumed** -- see
//! `.superpowers/sdd/notes/gully-kernel.md`, and the assertion
//! `the_lattice_reproduces_the_exact_structural_gradient` in this module.
//!
//! # What is cached, and why a cache does not make this impure
//!
//! A node's gradient is a pure function of the node's integer coordinates, the planet, and
//! the differencing step. Caching it changes no answer; it only avoids recomputing one. The
//! cache is direct-mapped and fixed-size, so a long pan over a planet cannot grow it -- an
//! unbounded map would be 1.27e8 nodes at 2 km spacing on Earth.
//!
//! It is behind a `Mutex` rather than a `RefCell` because `bindings.rs` hands out
//! `&'static Surface` from a `Mutex<HashMap<..>>`, which requires `Surface: Sync`. The lock
//! is taken **once per query**, not once per node, so a texel pays one uncontended
//! acquisition and not eight.

use std::sync::Mutex;

use crate::detmath as m;
use crate::sphere::SpherePoint;
use crate::tangent::TangentFrame;
use crate::vectors::Vec3;

/// Direct-mapped cache slots. Sized against the working set rather than against the
/// planet: a 258 x 258 post tile at the viewer's finest spacing spans about 20 km, which at
/// 2 km lattice spacing touches on the order of 250 distinct nodes. 4,096 slots is more
/// than an order of magnitude of headroom, costs 224 KiB, and -- unlike a `HashMap` -- has
/// a ceiling. A miss is never wrong, only slower, so the collision rate is a performance
/// property and not a correctness one.
const CACHE_SLOTS: usize = 4_096;

#[derive(Clone, Copy)]
struct Slot {
    /// The node this slot holds, or `None` for an empty slot. The full triple is stored and
    /// compared rather than a packed key, so there is no coordinate range this cache can
    /// silently alias -- a packed key would have a spacing below which two nodes collide
    /// and one of them answers for the other.
    key: Option<(i64, i64, i64)>,
    /// `grad(structural_m)` at the node, lifted into world space as
    /// `east * d/dx + north * d/dy`. Stored as a world vector rather than as a pair in the
    /// node's own frame so that interpolating between nodes never has to reconcile two
    /// tangent frames -- which is the term that would misbehave at a pole.
    value: Vec3,
}

/// A fixed lattice of `grad(structural_m)` over the whole planet.
pub struct SteerLattice {
    radius_m: f64,
    /// The lattice spacing **and** the central-differencing step, deliberately the same
    /// number. `gradient-probe.md` §2.4: there is no accuracy argument for a step finer
    /// than the lattice, because the direction does not move with the step.
    spacing_m: f64,
    /// `spacing_m / radius_m` -- the lattice pitch in unit-sphere units.
    cell: f64,
    cache: Mutex<Vec<Slot>>,
}

impl SteerLattice {
    pub fn new(radius_m: f64, spacing_m: f64) -> Self {
        Self {
            radius_m,
            spacing_m,
            cell: spacing_m / radius_m,
            cache: Mutex::new(vec![Slot { key: None, value: Vec3::new(0.0, 0.0, 0.0) }; CACHE_SLOTS]),
        }
    }

    pub fn spacing_m(&self) -> f64 {
        self.spacing_m
    }

    /// Where in the cache a node lives. An integer avalanche of the three coordinates, the
    /// same shape as `Noise::lattice`'s, truncated to the slot count. Nothing about the
    /// world depends on this function; a bad hash costs collisions and no correctness.
    fn slot_of(key: (i64, i64, i64)) -> usize {
        let (ix, iy, iz) = key;
        let mut h = (ix as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) // cast-ok: signed lattice coordinate reinterpreted for hashing, no float anywhere near it
            ^ (iy as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F) // cast-ok: as above
            ^ (iz as u64).wrapping_mul(0x1656_67B1_9E37_79F9); // cast-ok: as above
        h ^= h >> 33;
        h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
        h ^= h >> 33;
        (h as usize) % CACHE_SLOTS // cast-ok: a hash to an index, immediately reduced modulo the slot count
    }

    /// The gradient at one lattice node, in world space.
    ///
    /// The node's integer coordinates name a point in the cubic lattice; it is normalised
    /// onto the sphere before anything is evaluated there, because `structural_m` is a
    /// function of a point on the surface and the raw lattice point is not on it. The
    /// eight corners of a cell containing a unit vector all have a magnitude within
    /// `cell * sqrt(3)` of 1, so the normalisation is never near zero -- but it is guarded
    /// anyway and answers a flat gradient rather than a NaN if it ever were.
    fn node_gradient<F: Fn(&SpherePoint) -> f64>(&self, key: (i64, i64, i64), structural: &F) -> Vec3 {
        let (ix, iy, iz) = key;
        // cast-ok: a lattice coordinate to a float for the node's position; |i| <= radius/spacing, far below 2^53
        let raw = Vec3::new(ix as f64 * self.cell, iy as f64 * self.cell, iz as f64 * self.cell);
        let node = match SpherePoint::from_vector(&raw) {
            Some(node) => node,
            None => return Vec3::new(0.0, 0.0, 0.0),
        };
        let frame = TangentFrame::at(&node, self.radius_m);
        let h = self.spacing_m;
        let gx = (structural(&frame.local_to_sphere(h, 0.0)) - structural(&frame.local_to_sphere(-h, 0.0)))
            / (2.0 * h);
        let gy = (structural(&frame.local_to_sphere(0.0, h)) - structural(&frame.local_to_sphere(0.0, -h)))
            / (2.0 * h);
        frame.east.scaled(gx).add(&frame.north.scaled(gy))
    }

    /// The steering gradient at `point`, in `frame`'s basis, as `(d/dx, d/dy)` in m/m.
    ///
    /// Trilinear between the eight nodes of the containing cubic cell, then projected onto
    /// the query point's own tangent plane. The interpolation is continuous everywhere, so
    /// the height field this steers is continuous everywhere; its *derivative* is not
    /// continuous across a cell face, which is a property of trilinear interpolation and is
    /// invisible in a height.
    ///
    /// `frame` is the caller's frame at `point` rather than one built here, because
    /// `elevation_m` already has one and building a second would be the same six
    /// transcendentals twice.
    pub fn at<F: Fn(&SpherePoint) -> f64>(
        &self,
        point: &SpherePoint,
        frame: &TangentFrame,
        structural: &F,
    ) -> (f64, f64) {
        let v = point.vector;
        let (fx, fy, fz) = (v.x / self.cell, v.y / self.cell, v.z / self.cell);
        let (bx, by, bz) = (m::floor(fx), m::floor(fy), m::floor(fz));
        if !(bx.abs() < LATTICE_LIMIT && by.abs() < LATTICE_LIMIT && bz.abs() < LATTICE_LIMIT) {
            // A spacing so fine, or a radius so large, that the lattice index is not
            // nameable. `Noise::at` answers the same question with a NaN; here a flat
            // gradient is the honest answer, because a steer of zero switches the gully
            // term off rather than propagating a NaN into a height. Unreachable on any
            // record the C ABI admits -- the bound there is a spacing of at least a metre
            // on a planet of at most 1e9 m, which is 1e9 indices against this 9e18.
            return (0.0, 0.0);
        }
        let (ix, iy, iz) = (bx as i64, by as i64, bz as i64); // cast-ok: guarded above against the saturation `Noise::at` documents; each is a finite value below 9e18
        let (tx, ty, tz) = (fx - bx, fy - by, fz - bz);

        // One lock for the whole query, not one per node.
        let mut cache = match self.cache.lock() {
            Ok(cache) => cache,
            // A poisoned lock means another thread panicked while holding it. The cached
            // values are still values of a pure function, so the data is not suspect; take
            // it and carry on rather than propagate a panic across what may be a nounwind
            // boundary.
            Err(poisoned) => poisoned.into_inner(),
        };

        let mut total = Vec3::new(0.0, 0.0, 0.0);
        for corner in 0..8u32 {
            let (dx, dy, dz) = (corner & 1, (corner >> 1) & 1, (corner >> 2) & 1);
            let key = (ix + dx as i64, iy + dy as i64, iz + dz as i64); // cast-ok: a 0-or-1 corner selector widened for lattice arithmetic, no float anywhere near it
            let wx = if dx == 1 { tx } else { 1.0 - tx };
            let wy = if dy == 1 { ty } else { 1.0 - ty };
            let wz = if dz == 1 { tz } else { 1.0 - tz };
            let weight = wx * wy * wz;
            let slot = Self::slot_of(key);
            let value = if cache[slot].key == Some(key) {
                cache[slot].value
            } else {
                let value = self.node_gradient(key, structural);
                cache[slot] = Slot { key: Some(key), value };
                value
            };
            total = total.add(&value.scaled(weight));
        }
        (total.dot(&frame.east), total.dot(&frame.north))
    }
}

/// The largest lattice coordinate this module will name, mirroring `noise.rs`'s
/// `LATTICE_LIMIT` and drawn at the same round number for the same reason: `i64::MAX` is
/// about 9.223e18, and a saturating `as i64` followed by `+ 1` is the abort that constant
/// exists to refuse.
const LATTICE_LIMIT: f64 = 9.0e18;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sphere::EARTH_RADIUS_M;

    /// A field whose gradient is known in closed form: height rises with latitude at a
    /// fixed rate per metre of northing. Its gradient is `(0, slope)` in every tangent
    /// frame on the planet, so any error the lattice introduces is the whole of the
    /// difference.
    fn linear_north(slope: f64) -> impl Fn(&SpherePoint) -> f64 {
        move |p: &SpherePoint| {
            let (lat, _) = p.to_latlon();
            slope * m::to_radians(lat) * EARTH_RADIUS_M
        }
    }

    #[test]
    fn the_lattice_recovers_a_known_gradient_in_every_tangent_frame() {
        // A lattice of gradients is worth nothing if the projection back into the query
        // point's frame is wrong, and a field whose gradient is constant is the one case
        // where interpolation error cannot hide a projection error.
        let lattice = SteerLattice::new(EARTH_RADIUS_M, 2_000.0);
        let field = linear_north(0.005);
        let mut worst: f64 = 0.0;
        for lat_step in -8..=8 {
            for lon_step in -8..=8 {
                // cast-ok: a small loop counter to a float for a probe coordinate
                let lat = lat_step as f64 * 10.0;
                let lon = lon_step as f64 * 22.5; // cast-ok: as above
                let point = SpherePoint::from_latlon(lat, lon);
                let frame = TangentFrame::at(&point, EARTH_RADIUS_M);
                let (gx, gy) = lattice.at(&point, &frame, &field);
                let error = m::hypot(gx - 0.0, gy - 0.005);
                if error > worst {
                    worst = error;
                }
            }
        }
        // The field is linear in northing but the sphere is not flat, so the lattice's
        // trilinear blend of eight exact node gradients is not exactly the node gradient.
        // Measured worst case over these 289 sites: see the assertion's own margin.
        assert!(
            worst < 1.0e-6,
            "the lattice must recover a constant gradient to well inside the 0.005 m/m \
             slope reference; worst error {worst}"
        );
    }

    #[test]
    fn the_cache_answers_exactly_what_a_cold_lattice_answers() {
        // The cache is the only mutable state in the generator. If it ever answered
        // differently from a cold evaluation, every world would depend on what had been
        // drawn before it -- which is the failure a memo of a pure function cannot have and
        // must be shown not to have anyway.
        //
        // **Proved red by mutation**: replacing the key check in `at` with
        // `cache[slot].key.is_some()`, so a colliding slot answers for the node it is not.
        //
        // **And one mutation that stays GREEN, recorded rather than hidden**: dropping a
        // coordinate from `slot_of`'s hash changes no answer at all. That is a property of a
        // direct-mapped cache that VERIFIES its key rather than a hole in this test -- a bad
        // hash costs collisions and never correctness, which is exactly why `Slot` stores the
        // whole triple instead of a packed key. The load-bearing line is the comparison, not
        // the hash, and the mutation above is the one that finds it.
        let field = linear_north(0.003);
        let warm = SteerLattice::new(EARTH_RADIUS_M, 2_000.0);
        for lat_step in -4..=4 {
            for lon_step in -4..=4 {
                let lat = lat_step as f64 * 20.0; // cast-ok: a small loop counter to a float for a probe coordinate
                let lon = lon_step as f64 * 40.0; // cast-ok: as above
                let point = SpherePoint::from_latlon(lat, lon);
                let frame = TangentFrame::at(&point, EARTH_RADIUS_M);
                warm.at(&point, &frame, &field);
            }
        }
        for lat_step in -4..=4 {
            for lon_step in -4..=4 {
                let lat = lat_step as f64 * 20.0; // cast-ok: a small loop counter to a float for a probe coordinate
                let lon = lon_step as f64 * 40.0; // cast-ok: as above
                let point = SpherePoint::from_latlon(lat, lon);
                let frame = TangentFrame::at(&point, EARTH_RADIUS_M);
                let cold = SteerLattice::new(EARTH_RADIUS_M, 2_000.0);
                let (cx, cy) = cold.at(&point, &frame, &field);
                let (wx, wy) = warm.at(&point, &frame, &field);
                assert_eq!(cx.to_bits(), wx.to_bits(), "cached x at {lat},{lon}");
                assert_eq!(cy.to_bits(), wy.to_bits(), "cached y at {lat},{lon}");
            }
        }
    }

    #[test]
    fn the_steer_is_continuous_across_a_lattice_cell_boundary() {
        // Trilinear interpolation is continuous, and a discontinuity here would be a step
        // in the ground exactly along a lattice plane -- the crease `noise.rs` records
        // straight linear interpolation leaving, in a field the eye reads as a cliff.
        let lattice = SteerLattice::new(EARTH_RADIUS_M, 2_000.0);
        let field = linear_north(0.004);
        // Walk a line in metres far finer than the 2 km lattice, so several cell faces are
        // crossed, and assert no step larger than the smooth variation either side.
        let origin = SpherePoint::from_latlon(31.0, 17.0);
        let walk = TangentFrame::at(&origin, EARTH_RADIUS_M);
        let mut previous: Option<(f64, f64)> = None;
        let mut worst: f64 = 0.0;
        for step in 0..2_000 {
            let point = walk.local_to_sphere(step as f64 * 10.0, 0.0); // cast-ok: a loop counter to a float for a distance in metres
            let frame = TangentFrame::at(&point, EARTH_RADIUS_M);
            let here = lattice.at(&point, &frame, &field);
            if let Some((px, py)) = previous {
                let jump = m::hypot(here.0 - px, here.1 - py);
                if jump > worst {
                    worst = jump;
                }
            }
            previous = Some(here);
        }
        assert!(
            worst < 1.0e-7,
            "a 10 m step must never move the steer by anything approaching the slope \
             reference; worst jump {worst}"
        );
    }
}
