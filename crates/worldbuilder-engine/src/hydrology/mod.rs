//! Automatic water: the bake that finds where water collects and where it runs.
//!
//! See `docs/superpowers/specs/2026-09-10-automatic-water-design.md`. This module reads the
//! landform (`Surface::structural_m`), never the detail noise, and it changes nothing in the
//! default elevation path: a bake is requested, not implied.

pub mod heap;
pub mod buckets;
pub mod landgraph;
pub mod flood;
pub mod hollows;
pub mod routing;
pub mod flow;
pub mod reaches;
pub mod record;
pub mod bake;
pub mod refine;
pub mod ponds;
#[cfg(test)]
mod bake_tests;

use crate::sphere::SpherePoint;
use crate::surface::Surface;

// Ruling A: `ReachLine` and the bake tests need to name these without a second import path.
pub use reaches::{Downstream, ReachClass};
pub use bake::{bake_stages, record_of, BakeStages};

/// Everything a bake is told. Recorded in the world beside the record, so a re-bake with the
/// same params and the same land is the same water.
#[derive(Debug, Clone, PartialEq)]
pub struct HydroParams {
    pub total_nodes: u32,
    pub wetness_nodes: u32,
    pub keep_depth_m: f64,
    pub keep_area_m2: f64,
    pub pond_max_area_m2: f64,
    pub stream_flow_m2: f64,
    pub river_flow_m2: f64,
    pub great_flow_m2: f64,
    pub notch_fall_m: f64,
    pub evaporation_factor: f64,
    pub salt_flat_share: f64,
    pub forced_outlets: Vec<SpherePoint>,
    /// Ruling 12b-1: the floor for the stream threshold, in graph nodes rather than m^2 -- the
    /// effective stream threshold is `max(stream_flow_m2, min_stream_nodes * median land-node
    /// area)`, so a coarse graph's threshold rises to what it can actually resolve. Not a wasm
    /// param in 1a: `hydro_params_from` never sets this field, so a wasm bake always takes
    /// `earth_like`'s value.
    pub min_stream_nodes: f64,
    /// Ruling 12b-5: an open hollow (neither enclosed nor forced) larger than this is notched
    /// however deep it is -- a broad landform basin filled to its rim is a drained lowland at
    /// graph scale, not an inland sea several Caspians wide. Not a wasm param in 1a, for the
    /// same reason as `min_stream_nodes`.
    pub keep_max_area_m2: f64,
    /// Spec §6.6: the fine tracer's station spacing along a coarse segment.
    pub refine_step_m: f64,
    /// Ruling R-7: Douglas–Peucker horizontal tolerance for refined reaches. 500 m in
    /// `earth_like`: planned at 250 m, and raised by plan 1b-2 Task 8's size gate when the
    /// owner's world baked an 8,659,856-byte record at 250 m (the target is 8 MB).
    pub refine_simplify_m: f64,
    /// Ruling R-7: the vertical tolerance, on the bed.
    pub refine_vertical_m: f64,
    /// Spec §6.7: a fall drops at least this much ...
    pub fall_min_drop_m: f64,
    /// ... over at most this much of its length.
    pub fall_max_run_m: f64,
    /// Ruling R-6: meander wavelength, in channel widths.
    pub meander_wavelength_widths: f64,
    /// Ruling R-6: meander amplitude, in channel widths.
    pub meander_amplitude_widths: f64,
    /// Ruling R-6: a segment meanders only if its bed falls less steeply than this.
    pub meander_max_slope: f64,
    /// Spec §6.6: the fine search's cell.
    pub pond_cell_m: f64,
    /// How far either side of a refined reach the fine search looks.
    pub pond_search_radius_m: f64,
    /// The pond keep rule's depth.
    pub pond_keep_depth_m: f64,
    /// The pond keep rule's area.
    pub pond_keep_area_m2: f64,
    /// Ruling S-6: a candidate's terrain must be wetter than this share of the graph's land
    /// nodes, measured at the nearest node.
    pub pond_wetness_share: f64,
    /// Ruling S-6: and flatter than this, over one pond cell.
    pub pond_max_slope: f64,
    /// Ruling S-8: at most one kept body per this much searched area.
    pub pond_density_area_m2: f64,
}

impl HydroParams {
    /// The spec's Earth-like starting values (section 6), tuned against the owner's world by
    /// Task 12b of plan 1a (see `.superpowers/sdd/2026-09-10-water-1a-coarse-bake/
    /// task-12b-report.md` for the measurements behind `min_stream_nodes` and
    /// `keep_max_area_m2`).
    pub fn earth_like(total_nodes: u32) -> Self {
        Self {
            total_nodes,
            wetness_nodes: 20_000,
            keep_depth_m: 8.0,
            keep_area_m2: 1.0e6,
            pond_max_area_m2: 1.0e6,
            stream_flow_m2: 2.5e8,
            river_flow_m2: 2.5e9,
            great_flow_m2: 1.0e11,
            notch_fall_m: 1.0,
            evaporation_factor: 1.0,
            salt_flat_share: 0.1,
            forced_outlets: Vec::new(),
            min_stream_nodes: 10.0,
            keep_max_area_m2: 4.0e11,
            refine_step_m: 1_500.0,
            refine_simplify_m: 500.0,
            refine_vertical_m: 1.0,
            fall_min_drop_m: 10.0,
            fall_max_run_m: 150.0,
            meander_wavelength_widths: 11.0,
            meander_amplitude_widths: 1.5,
            meander_max_slope: 0.002,
            pond_cell_m: 250.0,
            pond_search_radius_m: 3_000.0,
            pond_keep_depth_m: 2.0,
            pond_keep_area_m2: 50_000.0,
            pond_wetness_share: 0.6,
            pond_max_slope: 0.03,
            pond_density_area_m2: 5.0e8,
        }
    }
}

/// Ruling 12b-3: the node budget for a coarse bake, measured against the owner's world (studio
/// heap 372 MB at this count, under the 512 MB ceiling; an 80 s wasm bake). Bifurcation ratios
/// are reported by `BakeStats`, not forced toward the Earth-like 3-5 target -- the gap between
/// what this budget's graph resolves and that target is documented, not closed here.
pub const DEFAULT_TOTAL_NODES: u32 = 1_000_000;

/// What kind of standing water a body is. Salt vs fresh is `Body::fresh`; this is the shape
/// and the surface, not the chemistry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    Lake,
    Pond,
    SaltLake,
    SaltFlat,
}

/// One kept hollow: a lake, pond, salt lake or salt flat. `id` is the hollow's position among
/// kept hollows, in hollow order -- not the hollow's own index, which also counts notched ones.
#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    pub id: u32,
    pub kind: BodyKind,
    /// "Not closed": the lake has an outlet. Its water may still end in a closed lake downstream
    /// rather than the sea -- not the same meaning as `ReachLine::fresh`.
    pub fresh: bool,
    pub enclosed: bool,
    pub forced: bool,
    pub level_m: f64,
    pub area_m2: f64,
    pub depth_m: f64,
    pub outlet_reach: Option<u32>,
    pub anchor: (f64, f64),
    pub outline: Vec<(f64, f64)>,
    /// Where this body's water goes next: the first reach it feeds, the next body it drains
    /// straight into (no reach between them), the ocean, or nowhere (a closed lake). On the wire
    /// as of SCHEMA 3.
    pub downstream: Downstream,
}

/// One node along a reach's course.
#[derive(Debug, Clone, PartialEq)]
pub struct ReachPoint {
    pub lat_deg: f64,
    pub lon_deg: f64,
    /// The channel bed: the water surface here minus `depth_m` (at a mouth, the level of the
    /// water it runs into). Not the cut surface a notch point's third word carries.
    pub bed_m: f64,
    pub width_m: f64,
    pub depth_m: f64,
    pub flow_m2: f64,
}

/// A stream, river or great river, as a polyline of `ReachPoint`s.
#[derive(Debug, Clone, PartialEq)]
pub struct ReachLine {
    pub id: u32,
    pub class: ReachClass,
    pub order: u32,
    pub downstream: Downstream,
    /// SCHEMA 3: "its chain reaches the ocean" -- `true` if this reach's downstream chain
    /// (through reaches, then bodies via `Body::downstream`) ends at `Ocean`, `false` if it ends
    /// at a closed lake's sink (or the walk's bound runs out, which "everything drains" rules out
    /// on a bake that passed `drainage_check`). Not the same meaning as `Body::fresh`.
    pub fresh: bool,
    pub points: Vec<ReachPoint>,
}

/// A cut channel through a notched hollow's rim, as the falling course `routing::cut_route` or
/// `cut_path` left behind. SCHEMA 3 adds each point's width, so a notch can be drawn to scale
/// like a reach rather than as a bare line.
///
/// Each point is `(lat, lon, surface_m, width_m)`. The third is the cut surface -- the lowered
/// ground, which is the water surface through the cut -- not a bed below it the way a
/// `ReachPoint::bed_m` is (Ruling F-2).
#[derive(Debug, Clone, PartialEq)]
pub struct NotchLine {
    pub points: Vec<(f64, f64, f64, f64)>,
}

/// A drop along a reach. Empty in 1a: no waterfall geometry is derived yet.
#[derive(Debug, Clone, PartialEq)]
pub struct Fall {
    pub reach: u32,
    pub at: (f64, f64),
    pub height_m: f64,
}

/// Counts and summary numbers from one bake, for a survey or a log line -- never round-tripped
/// through anything but the record itself.
#[derive(Debug, Clone, PartialEq)]
pub struct BakeStats {
    pub nodes: u32,
    pub land_nodes: u32,
    pub hollows: u32,
    pub kept: u32,
    pub notched: u32,
    pub closed: u32,
    pub streams: u32,
    pub rivers: u32,
    pub great: u32,
    pub max_order: u32,
    pub bifurcation_min: f64,
    pub bifurcation_max: f64,
    /// Ruling 12b-1: the effective thresholds this bake actually used, after the
    /// resolution-aware floor -- so a record says what it used, not just what `HydroParams`
    /// asked for.
    pub stream_flow_m2: f64,
    pub river_flow_m2: f64,
    pub great_flow_m2: f64,
    /// SCHEMA 3's params echo: the request this bake actually ran with, alongside the effective
    /// thresholds above -- so a record (or a studio reading one) can show what was asked for, not
    /// only what a coarse graph raised it to. Mirrors `HydroParams` field for field, except the
    /// three flow thresholds (already covered above) and `forced_outlets` itself, which the two
    /// counts below stand in for.
    pub total_nodes: u32,
    pub wetness_nodes: u32,
    pub keep_depth_m: f64,
    pub keep_area_m2: f64,
    pub pond_max_area_m2: f64,
    pub keep_max_area_m2: f64,
    pub min_stream_nodes: f64,
    pub notch_fall_m: f64,
    pub evaporation_factor: f64,
    pub salt_flat_share: f64,
    /// How many forced-outlet points `params` asked for.
    pub forced_requested: u32,
    /// Of those, how many landed on a submerged member of a kept lake -- the same nearest-node
    /// mapping `hollows::forced_nodes` uses, checked against `Routing::lake_of` after routing.
    pub forced_matched: u32,
    /// SCHEMA 4, carry-forward I3: hollows with `capped == true` -- the record says how many
    /// there were, so the owner-world bake can show whether capped basins keep their inner
    /// lakes at 1M nodes.
    pub capped_basins: u32,
    /// Hollows with `inner_of_capped`: every inner hollow a capped basin's sub-flood revealed.
    pub capped_inner: u32,
    /// Of those, how many were kept (`fate == Fate::Keep`) rather than notched for sitting on
    /// their basin's way out.
    pub capped_inner_kept: u32,
    /// SCHEMA 4's refinement params echo (Ruling R-8: not wasm params, always `earth_like`'s
    /// values on a wasm bake). Mirrors `HydroParams` field for field.
    pub refine_step_m: f64,
    pub refine_simplify_m: f64,
    pub refine_vertical_m: f64,
    pub fall_min_drop_m: f64,
    pub fall_max_run_m: f64,
    pub meander_wavelength_widths: f64,
    pub meander_amplitude_widths: f64,
    pub meander_max_slope: f64,
    /// SCHEMA 5, Rulings S-2 and S-3: how many crossings the *coarse* record already had, before
    /// refinement traced anything. These are graph artifacts the crossing pass does not try to
    /// fix (Ruling S-2), so they are the number the refined count is judged against.
    pub crossings_coarse: u32,
    /// SCHEMA 5, Ruling S-4: how many crossings are left in the record as it ships, after the
    /// crossing pass, the meander and simplification. Recorded rather than asserted to be zero,
    /// because a coarse crossing cannot be straightened away.
    pub crossings_left: u32,
    /// SCHEMA 5, spec §6.6: every hollow the fine search found that passed the pond keep rule,
    /// before Ruling S-10's side clip, Ruling S-6's wetness and slope gates, Ruling S-7's drops,
    /// the cross-strip dedup and Ruling S-8's density cap.
    pub ponds_found: u32,
    /// Of those, how many reached the record as bodies. The gap between the two is what the
    /// gates and the cap removed, and it is a large gap by design: about half of all candidates
    /// are side-clipped alone.
    pub ponds_kept: u32,
    /// SCHEMA 5: the fine search's seven params, echoed the way the refinement params are (and
    /// not wasm params either -- a wasm bake always uses `earth_like`'s values).
    pub pond_cell_m: f64,
    pub pond_search_radius_m: f64,
    pub pond_keep_depth_m: f64,
    pub pond_keep_area_m2: f64,
    pub pond_wetness_share: f64,
    pub pond_max_slope: f64,
    pub pond_density_area_m2: f64,
}

/// Everything a bake produces: the standing water, the channels, the notches that drain the
/// hollows the keep rule declined, the falls, and the stats. `record::encode`/`decode` give
/// this a flat `f64` wire form.
#[derive(Debug, Clone, PartialEq)]
pub struct HydroRecord {
    pub bodies: Vec<Body>,
    pub reaches: Vec<ReachLine>,
    pub notches: Vec<NotchLine>,
    pub falls: Vec<Fall>,
    pub stats: BakeStats,
}

/// Why a bake could not be produced.
#[derive(Debug, Clone, PartialEq)]
pub enum HydroError {
    Params(&'static str),
    Sampling,
    /// Ruling C1-c: the routing broke "everything drains" -- the node is the lowest-index one
    /// whose receiver chain cycles or stops on dry land (see `flow::drainage_check`). The bake
    /// refuses rather than silently losing that node's water.
    Drainage(u32),
}

/// The bake, end to end: `bake_stages`, then `record_of`, then `refine::refine`, then
/// `ponds::search` (spec §6.6).
///
/// The fine pond search runs **last**, and it must: it walks strips along the *refined* lines, so
/// it cannot run before they exist, and by Ruling S-5 what it adds changes no routing, no reach
/// and no notch. The two grounds it is handed are deliberately different (Ruling S-9) -- the
/// landform for the geometry, the landform with its detail field for the cells it searches.
pub fn bake(surface: &Surface, params: &HydroParams) -> Result<HydroRecord, HydroError> {
    let stages = bake_stages(surface, params)?;
    let mut record = record_of(&stages, params);
    let height = |p: &SpherePoint| surface.structural_m(p);
    let ground = refine::Ground::for_surface(surface, &height, params);
    refine::refine(&mut record, &ground, params);
    let detail = ponds::pond_ground(surface, params);
    ponds::search(&mut record, &stages.graph, &stages.routing.lake_of, &ground, &detail, params);
    Ok(record)
}

/// Walks downstream from every reach and fails on a revisit. `mod.rs` owns it (rather than
/// `reaches.rs`) because the survey (a later task) reuses it against `ReachLine`, the public
/// record type, not `reaches::Reach`.
pub fn reaches_are_acyclic(reaches: &[ReachLine]) -> bool {
    // 0 unvisited, 1 on the current walk, 2 known to end without a cycle.
    let mut state = vec![0u8; reaches.len()];
    let mut walk: Vec<usize> = Vec::new();
    for start in 0..reaches.len() {
        let mut here = start;
        loop {
            if state[here] == 2 {
                break;
            }
            if state[here] == 1 {
                return false;
            }
            state[here] = 1;
            walk.push(here);
            match reaches[here].downstream {
                Downstream::Reach(next) => {
                    let next = next as usize;
                    if next >= reaches.len() {
                        return false;
                    }
                    here = next;
                }
                _ => break,
            }
        }
        for &id in &walk {
            state[id] = 2;
        }
        walk.clear();
    }
    true
}

