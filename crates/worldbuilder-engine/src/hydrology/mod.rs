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

use crate::sphere::SpherePoint;

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
}

impl HydroParams {
    /// The spec's Earth-like starting values (section 6). Task 12 of plan 1a tunes them against
    /// the owner's world and records the result here.
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
        }
    }
}
