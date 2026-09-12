//! Ruling R-6's meander: the lateral shape a flat, wide, fall-free segment is given after it has
//! been traced. It moves the line, never the bed.

use super::{Chord, Ground, Segment};
use crate::detmath as m;
use crate::hydrology::{HydroParams, ReachPoint};
use crate::noise::Noise;

/// Salt for the meander's phase, so it is independent of every other noise field on the world.
const MEANDER_SALT: u64 = 0x4d45_414e_4445_5253;
/// Scales a unit vector into the noise lattice, so neighbouring segments get unrelated phases.
const MEANDER_FREQUENCY: f64 = 64.0;

/// Ruling R-6: a meander only where it can be drawn at this step (a wavelength of at least
/// four steps), where the river is flat (bed slope under `meander_max_slope`), and where no
/// fall was found. It is tapered to zero at both coarse points and kept inside the corridor.
/// It moves the line, never the bed.
///
/// A segment trimmed at the shore is left alone: `trace` returns at its mouth, and a mouth is a
/// position on the water's edge, not a line to be decorated.
pub(super) fn meander(segment: &mut Segment, ground: &Ground, params: &HydroParams, a: &ReachPoint, b: &ReachPoint) {
    if segment.mouth.is_some() || !segment.falls.is_empty() || segment.interior.is_empty() {
        return;
    }
    let chord = Chord::new(ground, a, b);
    let len = chord.len_m;
    let floor_m = if b.bed_m < a.bed_m { b.bed_m } else { a.bed_m };
    let wavelength = params.meander_wavelength_widths * a.width_m;
    let slope = (a.bed_m - floor_m) / len;
    let amplitude = params.meander_amplitude_widths * a.width_m;
    if !(slope < params.meander_max_slope && wavelength >= 4.0 * params.refine_step_m && amplitude > 0.0) {
        return;
    }
    let v = chord.start.vector;
    let n = Noise::new(ground.seed, MEANDER_SALT)
        .at(v.x * MEANDER_FREQUENCY, v.y * MEANDER_FREQUENCY, v.z * MEANDER_FREQUENCY);
    if !n.is_finite() {
        return;
    }
    let pi = std::f64::consts::PI;
    let phase = pi * (1.0 + n);
    for fine in segment.interior.iter_mut() {
        let envelope = m::sin(pi * fine.along_m / len);
        let mut shift = amplitude * envelope * m::sin(2.0 * pi * fine.along_m / wavelength + phase);
        let room_left = ground.corridor_m - fine.lateral_m;
        let room_right = ground.corridor_m + fine.lateral_m;
        if shift > room_left {
            shift = room_left;
        }
        if shift < -room_right {
            shift = -room_right;
        }
        fine.lateral_m += shift;
        fine.point = chord.at(fine.along_m, fine.lateral_m);
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::super::trace_segment;
    use crate::sphere::SpherePoint;

    /// Nearly flat ground falling east: 0.05% slope.
    fn flat(p: &SpherePoint) -> f64 { let (_, e) = north_east(p); 50.0 - 0.0005 * e }

    #[test]
    fn a_flat_wide_river_meanders_inside_its_corridor() {
        let a = wide(0.0, 0.0, 45.0, 1_000.0);
        let b = wide(0.0, 30_000.0 / M_PER_DEG, 30.0, 1_000.0);
        let g = ground(&flat);
        let seg = trace_segment(&g, &params(), &a, &b, None);
        let mut straight = params();
        straight.meander_amplitude_widths = 0.0;
        let plain = trace_segment(&g, &straight, &a, &b, None);
        assert_eq!(seg.interior.len(), plain.interior.len());
        let mut moved = 0usize;
        for (f, q) in seg.interior.iter().zip(&plain.interior) {
            let shift = f.lateral_m - q.lateral_m;
            assert!(shift <= 1_500.0 + 1e-9 && shift >= -1_500.0 - 1e-9, "shift {shift} exceeds 1.5 widths");
            assert!(f.lateral_m <= g.corridor_m && f.lateral_m >= -g.corridor_m);
            assert_eq!(f.bed_m, q.bed_m, "a meander moves the line, not the bed");
            if shift > 1.0 || shift < -1.0 { moved += 1; }
        }
        assert!(moved > seg.interior.len() / 2, "most stations moved: {moved}");
    }

    /// Step 3 mutation guard's permanent variant: the same flat river, on a corridor tight
    /// enough (500 m, half the meander's own 1.5-width amplitude) that clamping actually binds.
    #[test]
    fn a_flat_wide_river_meander_stays_inside_a_tight_corridor() {
        let a = wide(0.0, 0.0, 45.0, 1_000.0);
        let b = wide(0.0, 30_000.0 / M_PER_DEG, 30.0, 1_000.0);
        let mut g = ground(&flat);
        g.corridor_m = 500.0;
        let seg = trace_segment(&g, &params(), &a, &b, None);
        for f in &seg.interior {
            assert!(f.lateral_m <= 500.0 + 1e-9 && f.lateral_m >= -500.0 - 1e-9,
                    "station at {} m strayed to {} m outside the 500 m corridor", f.along_m, f.lateral_m);
        }
    }

    #[test]
    fn a_narrow_river_does_not_meander_at_this_resolution() {
        let a = wide(0.0, 0.0, 45.0, 100.0);
        let b = wide(0.0, 30_000.0 / M_PER_DEG, 30.0, 100.0);
        let g = ground(&flat);
        let mut straight = params();
        straight.meander_amplitude_widths = 0.0;
        assert_eq!(trace_segment(&g, &params(), &a, &b, None), trace_segment(&g, &straight, &a, &b, None));
    }

    #[test]
    fn a_steep_river_does_not_meander() {
        let steep = |p: &SpherePoint| { let (_, e) = north_east(p); 500.0 - 0.01 * e };
        let a = wide(0.0, 0.0, 495.0, 1_000.0);
        let b = wide(0.0, 30_000.0 / M_PER_DEG, 195.0, 1_000.0);
        let g = ground(&steep);
        let mut straight = params();
        straight.meander_amplitude_widths = 0.0;
        assert_eq!(trace_segment(&g, &params(), &a, &b, None), trace_segment(&g, &straight, &a, &b, None));
    }
}
