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

/// The bake, end to end: `bake_stages` then `record_of`.
pub fn bake(surface: &Surface, params: &HydroParams) -> Result<HydroRecord, HydroError> {
    let stages = bake_stages(surface, params)?;
    Ok(record_of(&stages, params))
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

