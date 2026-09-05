//! The stream graph: the second data class the core holds, beside the stateless field.
//!
//! `Surface` answers "what is the elevation at this point" from a seed alone, statelessly,
//! for ever. A drainage network cannot be answered that way: whether water leaving a place
//! reaches the sea depends on every other place, so the answer has to be *built* and then
//! *held*. That is the whole of CORE-001 — the core holds two representations from the
//! start, and nothing here may add a field to `Surface`
//! (`.superpowers/sdd/notes/2026-09-04-core-001-extraction.md` §4.1).
//!
//! # Struct-of-arrays, and why the field list is fixed now rather than later
//!
//! The arrays are parallel and indexed by node. A node is a `u32` index and nothing else:
//! 20 M nodes fit `u32` with 200x headroom (§8.1), and an array-of-structs would make
//! every future column a change to every existing record.
//!
//! Three fields look optional and are not (§3.2):
//!
//! - **`area_m2`**, per node, never a shared constant. If it were added later, every
//!   already-stored `drainage_area_m2` would silently change meaning from "cell count x a
//!   constant" to "sum of areas" — same bytes, different semantics, undetectable from the
//!   file. **This crate measures the discrepancy at 0.737x to 1.260x the ideal cell**
//!   at the recommended jitter — `measure_cell_area_spread`, this crate's own spiral
//!   sampler, n = 5,000 nodes at seed 20260904, jitter 0.15, against 4,000,000
//!   Monte-Carlo probes, CV 0.0754 — so the constant is not even approximately right.
//!   (The extraction's §8.4 quotes 0.735x to 1.285x. That is *its* probe field, a
//!   different population, and it is quoted separately rather than blended with this one.)
//! - **`flags`**, a bitset. Adding a boundary tag later as a *value* is fine; adding the
//!   *field* later means every existing graph has no boundary tag, so mouths cannot be
//!   identified, so a water manifest cannot be produced from an old graph at all.
//! - **`sea_level_m`**, in the header. Mouth-versus-lake is a function of it, so a graph
//!   built at one datum and read at another is wrong without being malformed.
//!
//! # `downhill`, not `receiver`
//!
//! The literature calls this edge the *receiver*. That word is already occupied in this
//! codebase in its object-oriented sense — `bindings.rs` and `tests/test_conformance.py`
//! both use "receiver" to mean the instance a method is called on — and a reader meeting
//! two senses of one word in one crate has to disambiguate every occurrence for ever. The
//! stored edge is therefore `downhill`, which names the thing rather than the role and
//! collides with nothing (§10.1, item 5).
//!
//! # What this module does not do
//!
//! No stream power equation, no implicit solver, no thermal correction, no lake overflow
//! algorithm, no mutation of `height_m` after construction, no exact spherical Voronoi.
//! All of that is slice 5's (§9). `outflow_lake` is *reserved at its sentinel* and
//! `reaches` is *reserved empty*: the records exist so that filling them later is not a
//! schema break.

use crate::detmath as m;
use crate::sphere::SpherePoint;
use crate::vectors::{Vec3, DEGENERATE};

/// A node's downhill target when it has none. **Named, and asserted never to be a valid
/// index**, because the two defensible conventions — this, and a self-edge — are not
/// interchangeable: a file written under one is silently wrong under the other, and a
/// peeling loop reads them differently (§3.1).
///
/// `u32::MAX` is chosen over the self-edge so that a self-edge stays *detectable as a bug*
/// rather than becoming the normal case. `validate` reports `SelfDownhill` for one.
pub const NO_DOWNHILL: u32 = u32::MAX;

/// The same sentinel in the lake table's `outflow_lake`, which slice 1p reserves and never
/// populates.
pub const NO_LAKE: u32 = u32::MAX;

/// The largest node count a graph may have, so that `NO_DOWNHILL` can never be reached by
/// a legal index. One below the sentinel, and that is the entire reason for the constant.
pub const MAX_NODES: u32 = u32::MAX - 1;

/// True when `sentinel` would address a real node in a graph of `node_count` nodes — the
/// defect this type exists to make impossible.
///
/// Factored out rather than inlined so that it can be exercised *with a sentinel that is
/// in range*. A guard only ever called with the good value proves nothing about itself.
pub fn sentinel_is_a_valid_index(node_count: u32, sentinel: u32) -> bool {
    sentinel < node_count
}

/// True when a node count can be addressed without colliding with the sentinel.
pub fn node_count_fits(count: usize) -> bool {
    count <= MAX_NODES as usize // cast-ok: widening a u32 bound to usize
}

/// The node flag bits. A bitset rather than four bools: additional bits are add-later-safe
/// (§3.2) and four of the eight are still spare.
pub mod flag {
    /// Strictly above the header's `sea_level_m`.
    pub const LAND: u8 = 1 << 0;
    /// At or below `sea_level_m`. The sea is the boundary of the land drainage domain, so
    /// exactly one of `LAND` and `BOUNDARY` is set on every node.
    pub const BOUNDARY: u8 = 1 << 1;
    /// A root that is a boundary node: flow that arrives here has left the land.
    pub const MOUTH: u8 = 1 << 2;
    /// A node belonging to a lake. Slice 1p sets it on the lake's root only; slice 5 fills
    /// in the rest of the members when it computes levels.
    pub const LAKE_MEMBER: u8 = 1 << 3;
    /// The bits this slice defines. Everything above is reserved and must read as zero.
    pub const DEFINED: u8 = LAND | BOUNDARY | MOUTH | LAKE_MEMBER;
}

/// How the node positions were produced. In the header because a graph must be refusable
/// when it does not match the field it is asked to sit beside, and because a second
/// resolution is a *new graph* rather than a wider node (§3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SamplingKind {
    /// Positions were handed to `build` by the caller rather than generated by a sampler
    /// this crate names. Tests and embedded fields use this.
    Supplied = 0,
    /// The spiral-plus-hashed-jitter sampler. Task 4 populates it; the discriminant is
    /// reserved here so the format does not renumber later.
    Spiral = 1,
}

/// §13.2 distinguishes a lake from a pond, and it is a size threshold, so it must not be a
/// bare bool: a bool records the *answer* and loses the *question*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LakeKind {
    Pond = 0,
    Lake = 1,
}

/// A lake is a non-boundary root (§14.2). That is its whole identity, so the table is
/// keyed on the root node.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lake {
    pub root_node: u32,
    /// The water surface. Slice 1p has no fill algorithm, so this is the root's own
    /// elevation — an empty basin. Slice 5 raises it.
    pub level_m: f64,
    pub kind: LakeKind,
    /// **Reserved.** The lake super-graph's overflow edge is slice 5's; this stays at
    /// `NO_LAKE` throughout slice 1p. The *field* is here because adding it later would be
    /// a schema break (§3.1).
    pub outflow_lake: u32,
}

/// A river reach, with its bed gradient. §13.2: "Rivers ship with reaches from the start."
/// The record exists; slice 1p populates none.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reach {
    pub from_node: u32,
    pub to_node: u32,
    pub gradient: f64,
}

/// Everything a reader needs in order to refuse a graph rather than guess about it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GraphHeader {
    /// `crate::GENERATOR_VERSION` at build time. Without it the artifact cannot fail
    /// closed (§5).
    pub generator_version: u32,
    pub world_seed: u64,
    pub radius_m: f64,
    pub node_count: u32,
    pub sampling_kind: SamplingKind,
    /// The datum the mouth/lake classification was made at.
    pub sea_level_m: f64,
    /// FNV-1a over the node positions' IEEE-754 bit patterns. Positions are *derived*, not
    /// stored (§3.3): a reader regenerates them and compares this, and a mismatch is a
    /// refusal rather than a guess. Eight bytes instead of 24 per node.
    pub position_checksum: u64,
}

/// What `build` is given.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BuildParams {
    pub world_seed: u64,
    pub radius_m: f64,
    pub sea_level_m: f64,
    pub sampling_kind: SamplingKind,
    /// A lake root whose drainage area is at or below this is a pond. **Not measured**:
    /// no probe computed lake levels (§10.2), so the threshold is the caller's to state
    /// and slice 5's to calibrate. It is a required parameter rather than a default so
    /// that nobody inherits a number nobody chose.
    pub pond_max_drainage_area_m2: f64,
}

/// Why `build` refused its input.
#[derive(Debug, Clone, PartialEq)]
pub enum GraphError {
    LengthMismatch { field: &'static str, len: usize, expected: usize },
    TooManyNodes { count: usize },
    NeighbourOutOfRange { node: u32, neighbour: u32 },
    SelfNeighbour { node: u32 },
    CoincidentNodes { node: u32, neighbour: u32 },
    NonFiniteHeight { node: u32 },
    NonPositiveArea { node: u32 },
    /// The graph was assembled and then failed its own invariants. A build that produced
    /// this is a bug in this module, not in its caller.
    Invalid(Vec<GraphDefect>),
}

/// An invariant the graph does not satisfy. `validate` returns every one it finds rather
/// than the first, because a partition failure and a cycle have different causes and a
/// reader wants both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphDefect {
    /// The sentinel addresses a real node. Property 4.
    SentinelIsAValidIndex { count: u32, sentinel: u32 },
    DownhillOutOfRange { node: u32, target: u32 },
    /// A self-edge. Under this type's convention that is a bug, not "root".
    SelfDownhill { node: u32 },
    NotDescending { node: u32, target: u32 },
    /// Nodes survived the peel, so some of them are on a cycle. Property 1.
    Cycle { unpeeled: u32 },
    /// Property 2, the "neither" arm.
    RootIsNeitherMouthNorLake { node: u32 },
    /// Property 2, the "both" arm.
    RootIsBothMouthAndLake { node: u32 },
    LakeAtNonRoot { node: u32 },
    MouthAtNonRoot { node: u32 },
    DuplicateLakeRoot { node: u32 },
    LakeFlagWithoutRecord { node: u32 },
    ReservedFlagBitSet { node: u32, bits: u8 },
}

/// The result of peeling the downhill relation leaves-first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peel {
    /// How many nodes came off. Equal to the node count exactly when the relation is a
    /// forest.
    pub peeled: u32,
    /// The peel order: a topological order from leaves to roots. Deterministic — the ready
    /// queue is seeded in ascending index order and consumed first-in-first-out — which is
    /// what makes the drainage accumulation reproducible.
    pub order: Vec<u32>,
}

/// The graph. Fields are private so that the only way to obtain one is through `build`,
/// which validates; the in-module tests reach past that on purpose to plant defects.
#[derive(Debug, Clone)]
pub struct StreamGraph {
    header: GraphHeader,
    height_m: Vec<f64>,
    area_m2: Vec<f64>,
    downhill: Vec<u32>,
    drainage_area_m2: Vec<f64>,
    flags: Vec<u8>,
    lakes: Vec<Lake>,
    reaches: Vec<Reach>,
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv1a_u64(state: u64, value: u64) -> u64 {
    let mut hash = state;
    let mut shift = 0;
    while shift < 64 {
        let byte = (value >> shift) & 0xff;
        hash = (hash ^ byte).wrapping_mul(FNV_PRIME);
        shift += 8;
    }
    hash
}

/// FNV-1a over the node positions' IEEE-754 bit patterns, in index order.
///
/// **Public, and that is the point.** `streamfmt` stores this in the header and stores no
/// positions; a reader that regenerates a node set has to be able to hash it with *this*
/// function. Task 5 dropped its `positions_match` rather than copy the hash, and was right
/// to: a second copy of a hash is a version bump waiting to happen -- the two would agree
/// until one was touched, and the disagreement would surface as every existing worldfile
/// failing its own checksum. One definition, exported.
///
/// The bit pattern, not the value: `-0.0` and `0.0` are different positions to a
/// bit-identical rebuild, and NaN is not equal to itself.
pub fn position_checksum(positions: &[SpherePoint]) -> u64 {
    let mut hash = FNV_OFFSET;
    for point in positions {
        hash = fnv1a_u64(hash, point.vector.x.to_bits());
        hash = fnv1a_u64(hash, point.vector.y.to_bits());
        hash = fnv1a_u64(hash, point.vector.z.to_bits());
    }
    hash
}

impl StreamGraph {
    /// Build a graph over a node set the caller supplies.
    ///
    /// Sampling is deliberately not here: Task 4 owns where the nodes are, and this type
    /// owns what the edges between them mean. The separation is what lets the sampler be
    /// swapped behind the same type at the price of a version bump (Ruling 1).
    ///
    /// The edge rule is **steepest descent under a strict `height[j] < height[i]` test**,
    /// gradient measured as drop over great-circle distance so that an uneven node spacing
    /// does not disguise itself as a slope. Ties go to the lower index. Strictness is what
    /// makes a cycle impossible (§8.3), and the peel asserts it rather than assuming it.
    pub fn build(
        params: &BuildParams,
        positions: &[SpherePoint],
        height_m: &[f64],
        area_m2: &[f64],
        neighbours: &[Vec<u32>],
    ) -> Result<Self, GraphError> {
        let count = positions.len();
        if !node_count_fits(count) {
            return Err(GraphError::TooManyNodes { count });
        }
        if height_m.len() != count {
            return Err(GraphError::LengthMismatch {
                field: "height_m",
                len: height_m.len(),
                expected: count,
            });
        }
        if area_m2.len() != count {
            return Err(GraphError::LengthMismatch {
                field: "area_m2",
                len: area_m2.len(),
                expected: count,
            });
        }
        if neighbours.len() != count {
            return Err(GraphError::LengthMismatch {
                field: "neighbours",
                len: neighbours.len(),
                expected: count,
            });
        }
        let node_count = count as u32; // cast-ok: bounded by node_count_fits above

        let mut flags = vec![0u8; count];
        for i in 0..count {
            let index = i as u32; // cast-ok: bounded by node_count_fits above
            if !height_m[i].is_finite() {
                return Err(GraphError::NonFiniteHeight { node: index });
            }
            // `> 0.0` alone admits `+inf`: whole-branch review of slice 5a found that a
            // world with `radius_m` in roughly `[3.8e153, ~5e307)` overflows
            // `node_areas_m2`'s `4*pi*r^2` to `+inf`, which this check let straight
            // through into the graph. `erosion.rs`'s implicit update then divides by
            // `sqrt(inf)`, produces NaN, and trips its own release-time assertion --
            // an abort across the `extern "C"` boundary `wb_erosion_run` exposes, and one
            // this check should have refused before a graph existed for that solver to
            // run on. `NonPositiveArea` is refused here on the SAME invariant a caller
            // actually needs -- a finite, positive area -- not on a second, new variant;
            // `+inf` is not positive-and-finite any more than `0.0` or a NaN reading of
            // "not positive" is.
            if !(area_m2[i] > 0.0) || !area_m2[i].is_finite() {
                return Err(GraphError::NonPositiveArea { node: index });
            }
            flags[i] = if height_m[i] > params.sea_level_m { flag::LAND } else { flag::BOUNDARY };
        }

        let mut downhill = vec![NO_DOWNHILL; count];
        for i in 0..count {
            let index = i as u32; // cast-ok: bounded by node_count_fits above
            let mut best_target = NO_DOWNHILL;
            let mut best_gradient = 0.0f64;
            for &raw in &neighbours[i] {
                if raw >= node_count {
                    return Err(GraphError::NeighbourOutOfRange { node: index, neighbour: raw });
                }
                if raw == index {
                    return Err(GraphError::SelfNeighbour { node: index });
                }
                let j = raw as usize; // cast-ok: checked against node_count above
                let drop_m = height_m[i] - height_m[j];
                if !(drop_m > 0.0) {
                    continue;
                }
                let angle = positions[i].angle_to(&positions[j]);
                if !(angle > 0.0) {
                    return Err(GraphError::CoincidentNodes { node: index, neighbour: raw });
                }
                let gradient = drop_m / (params.radius_m * angle);
                // House form: an explicit comparison, never f64::max. Strictly greater, so
                // a tie keeps the neighbour already found, which is the lower index because
                // the neighbour list is walked in order.
                if gradient > best_gradient {
                    best_gradient = gradient;
                    best_target = raw;
                }
            }
            downhill[i] = best_target;
        }

        let mut graph = StreamGraph {
            header: GraphHeader {
                generator_version: crate::GENERATOR_VERSION,
                world_seed: params.world_seed,
                radius_m: params.radius_m,
                node_count,
                sampling_kind: params.sampling_kind,
                sea_level_m: params.sea_level_m,
                position_checksum: position_checksum(positions),
            },
            height_m: height_m.to_vec(),
            area_m2: area_m2.to_vec(),
            downhill,
            drainage_area_m2: area_m2.to_vec(),
            flags,
            lakes: Vec::new(),
            reaches: Vec::new(),
        };

        // Drainage accumulates in peel order, which is leaves-first, so every contribution
        // has arrived before a node is spent. Summation order is therefore fixed by index,
        // and that is what makes the f64 result bit-identical between rebuilds.
        let peel = graph.peel();
        for &i in &peel.order {
            let target = graph.downhill[i as usize]; // cast-ok: a node index into usize
            if target == NO_DOWNHILL {
                continue;
            }
            let carried = graph.drainage_area_m2[i as usize]; // cast-ok: a node index
            graph.drainage_area_m2[target as usize] += carried; // cast-ok: a node index
        }

        // §14.2: a root that is not on the boundary *is* a lake. The classification is
        // therefore total by construction, and `validate` then checks that it stayed so.
        for i in 0..count {
            if graph.downhill[i] != NO_DOWNHILL {
                continue;
            }
            let index = i as u32; // cast-ok: bounded by node_count_fits above
            if graph.flags[i] & flag::BOUNDARY != 0 {
                graph.flags[i] |= flag::MOUTH;
                continue;
            }
            graph.flags[i] |= flag::LAKE_MEMBER;
            let kind = if graph.drainage_area_m2[i] <= params.pond_max_drainage_area_m2 {
                LakeKind::Pond
            } else {
                LakeKind::Lake
            };
            graph.lakes.push(Lake {
                root_node: index,
                level_m: graph.height_m[i],
                kind,
                outflow_lake: NO_LAKE,
            });
        }

        match graph.validate() {
            Ok(()) => Ok(graph),
            Err(defects) => Err(GraphError::Invalid(defects)),
        }
    }

    /// Peel the relation leaves-first: repeatedly remove nodes nothing points at.
    ///
    /// **This is the forest test.** If any node remains unpeeled it is on a cycle, and a
    /// cycle would make slice 5's root-to-leaf walk non-terminating. Every probe run
    /// peeled completely -- 20,000,000 of 20,000,000 at the largest size tried (§8.3) --
    /// and Task 6 reproduced that inside this crate, over a real `Surface` field rather
    /// than a probe one, and then went past it: **50,000,000 of 50,000,000**, with complete
    /// peels at 10,000, 100,000, 200,000, 1,000,000, 5,000,000 and 20,000,000 as well. This
    /// turns those measurements into something the code cannot quietly lose.
    ///
    /// Safe on a corrupted graph: an out-of-range target is ignored rather than indexed.
    pub fn peel(&self) -> Peel {
        let count = self.height_m.len();
        let mut indegree = vec![0u32; count];
        for &target in &self.downhill {
            if target == NO_DOWNHILL {
                continue;
            }
            let t = target as usize; // cast-ok: a node index into usize
            if t < count {
                indegree[t] += 1;
            }
        }
        let mut order: Vec<u32> = Vec::with_capacity(count);
        for i in 0..count {
            if indegree[i] == 0 {
                order.push(i as u32); // cast-ok: bounded by the graph's own node count
            }
        }
        let mut head = 0usize;
        while head < order.len() {
            let node = order[head] as usize; // cast-ok: a node index into usize
            head += 1;
            let target = self.downhill[node];
            if target == NO_DOWNHILL {
                continue;
            }
            let t = target as usize; // cast-ok: a node index into usize
            if t >= count {
                continue;
            }
            indegree[t] -= 1;
            if indegree[t] == 0 {
                order.push(target);
            }
        }
        Peel { peeled: order.len() as u32, order } // cast-ok: bounded by the node count
    }

    /// Every invariant this type claims, checked. `build` calls it and refuses rather than
    /// returning a graph that fails it.
    pub fn validate(&self) -> Result<(), Vec<GraphDefect>> {
        let mut defects = Vec::new();
        let count = self.height_m.len();
        let node_count = self.header.node_count;

        // Property 4. Checked first because everything below indexes with it.
        if sentinel_is_a_valid_index(node_count, NO_DOWNHILL) {
            defects.push(GraphDefect::SentinelIsAValidIndex {
                count: node_count,
                sentinel: NO_DOWNHILL,
            });
        }

        for i in 0..count {
            let index = i as u32; // cast-ok: a node index, bounded by construction
            let bits = self.flags[i] & !flag::DEFINED;
            if bits != 0 {
                defects.push(GraphDefect::ReservedFlagBitSet { node: index, bits });
            }
            let target = self.downhill[i];
            if target == NO_DOWNHILL {
                continue;
            }
            if target == index {
                defects.push(GraphDefect::SelfDownhill { node: index });
                continue;
            }
            if target >= node_count {
                defects.push(GraphDefect::DownhillOutOfRange { node: index, target });
                continue;
            }
            if !(self.height_m[target as usize] < self.height_m[i]) { // cast-ok: node index
                defects.push(GraphDefect::NotDescending { node: index, target });
            }
        }

        // Property 1.
        let peel = self.peel();
        if peel.peeled != node_count {
            defects.push(GraphDefect::Cycle { unpeeled: node_count - peel.peeled });
        }

        // Property 2. The lake table is keyed on its root, so a duplicate key is a defect
        // in its own right rather than a silent last-one-wins.
        let mut lake_at = vec![false; count];
        for lake in &self.lakes {
            let root = lake.root_node as usize; // cast-ok: a node index into usize
            if root >= count || self.downhill[root] != NO_DOWNHILL {
                defects.push(GraphDefect::LakeAtNonRoot { node: lake.root_node });
                continue;
            }
            if lake_at[root] {
                defects.push(GraphDefect::DuplicateLakeRoot { node: lake.root_node });
                continue;
            }
            lake_at[root] = true;
        }
        for i in 0..count {
            let index = i as u32; // cast-ok: a node index, bounded by construction
            let is_root = self.downhill[i] == NO_DOWNHILL;
            let is_mouth = self.flags[i] & flag::MOUTH != 0;
            let flagged_lake = self.flags[i] & flag::LAKE_MEMBER != 0;
            if !is_root {
                if is_mouth {
                    defects.push(GraphDefect::MouthAtNonRoot { node: index });
                }
                continue;
            }
            if is_mouth && lake_at[i] {
                defects.push(GraphDefect::RootIsBothMouthAndLake { node: index });
            } else if !is_mouth && !lake_at[i] {
                defects.push(GraphDefect::RootIsNeitherMouthNorLake { node: index });
            }
            if flagged_lake != lake_at[i] {
                defects.push(GraphDefect::LakeFlagWithoutRecord { node: index });
            }
        }

        if defects.is_empty() {
            Ok(())
        } else {
            Err(defects)
        }
    }

    /// Property 3's comparison. Bits, not values: two graphs whose drainage areas differ in
    /// the last bit are different graphs, and `==` on `f64` would call them equal for NaN
    /// never and for `-0.0`/`0.0` wrongly.
    pub fn bit_identical_to(&self, other: &StreamGraph) -> bool {
        if self.header.generator_version != other.header.generator_version
            || self.header.world_seed != other.header.world_seed
            || self.header.node_count != other.header.node_count
            || self.header.sampling_kind != other.header.sampling_kind
            || self.header.position_checksum != other.header.position_checksum
            || self.header.radius_m.to_bits() != other.header.radius_m.to_bits()
            || self.header.sea_level_m.to_bits() != other.header.sea_level_m.to_bits()
        {
            return false;
        }
        if self.downhill != other.downhill || self.flags != other.flags {
            return false;
        }
        // All three columns, not just `height_m`: a legal graph has them all at
        // `node_count`, but the loop below indexes all three off `height_m`'s length, so
        // checking one and indexing three is a panic waiting for the first caller that
        // builds a `StreamGraph` some other way.
        if self.height_m.len() != other.height_m.len()
            || self.area_m2.len() != other.area_m2.len()
            || self.drainage_area_m2.len() != other.drainage_area_m2.len()
            || self.area_m2.len() != self.height_m.len()
            || other.area_m2.len() != other.height_m.len()
            || self.drainage_area_m2.len() != self.height_m.len()
            || other.drainage_area_m2.len() != other.height_m.len()
        {
            return false;
        }
        for i in 0..self.height_m.len() {
            if self.height_m[i].to_bits() != other.height_m[i].to_bits()
                || self.area_m2[i].to_bits() != other.area_m2[i].to_bits()
                || self.drainage_area_m2[i].to_bits() != other.drainage_area_m2[i].to_bits()
            {
                return false;
            }
        }
        if self.lakes.len() != other.lakes.len() || self.reaches.len() != other.reaches.len() {
            return false;
        }
        for i in 0..self.lakes.len() {
            let (a, b) = (&self.lakes[i], &other.lakes[i]);
            if a.root_node != b.root_node
                || a.kind != b.kind
                || a.outflow_lake != b.outflow_lake
                || a.level_m.to_bits() != b.level_m.to_bits()
            {
                return false;
            }
        }
        for i in 0..self.reaches.len() {
            let (a, b) = (&self.reaches[i], &other.reaches[i]);
            if a.from_node != b.from_node
                || a.to_node != b.to_node
                || a.gradient.to_bits() != b.gradient.to_bits()
            {
                return false;
            }
        }
        true
    }

    pub fn header(&self) -> &GraphHeader {
        &self.header
    }

    pub fn node_count(&self) -> u32 {
        self.header.node_count
    }

    pub fn height_m(&self, node: u32) -> f64 {
        self.height_m[node as usize] // cast-ok: a node index into usize
    }

    pub fn area_m2(&self, node: u32) -> f64 {
        self.area_m2[node as usize] // cast-ok: a node index into usize
    }

    pub fn drainage_area_m2(&self, node: u32) -> f64 {
        self.drainage_area_m2[node as usize] // cast-ok: a node index into usize
    }

    pub fn flags_of(&self, node: u32) -> u8 {
        self.flags[node as usize] // cast-ok: a node index into usize
    }

    pub fn has_flag(&self, node: u32, bit: u8) -> bool {
        self.flags[node as usize] & bit != 0 // cast-ok: a node index into usize
    }

    /// The stored value, sentinel included. Prefer `downhill_of`; this exists so a test can
    /// assert what is actually in the array rather than what an accessor decided.
    pub fn downhill_raw(&self, node: u32) -> u32 {
        self.downhill[node as usize] // cast-ok: a node index into usize
    }

    /// `None` at a root. The sentinel never escapes as a number a caller might index with.
    pub fn downhill_of(&self, node: u32) -> Option<u32> {
        let target = self.downhill_raw(node);
        if target == NO_DOWNHILL {
            None
        } else {
            Some(target)
        }
    }

    pub fn has_downhill(&self, node: u32) -> bool {
        self.downhill_raw(node) != NO_DOWNHILL
    }

    /// Ascending index order, so the answer does not depend on how it was collected.
    pub fn roots(&self) -> Vec<u32> {
        let mut out = Vec::new();
        for i in 0..self.downhill.len() {
            if self.downhill[i] == NO_DOWNHILL {
                out.push(i as u32); // cast-ok: a node index, bounded by construction
            }
        }
        out
    }

    pub fn mouth_count(&self) -> usize {
        self.flags.iter().filter(|f| *f & flag::MOUTH != 0).count()
    }

    pub fn lakes(&self) -> &[Lake] {
        &self.lakes
    }

    /// A linear scan, and **Task 6 measured that this will not survive slice 5**.
    ///
    /// It was chosen on the extraction's §8.3 figures -- 300 roots at 10,000 nodes and
    /// 5,647 at 20,000,000, "a 19x rise for a 2,000x rise in nodes" -- which make the lake
    /// table small in absolute terms and a spatial index a structure to keep consistent for
    /// no gain. **Those figures are a property of the extraction's probe field, not of this
    /// code, and neither the counts nor the sub-linearity survive contact with a real
    /// elevation field.** Over this crate's own `Surface`, Task 6's `streambench` measures
    /// 489 roots at 10,000 nodes (4.89%) and 597,687 at 20,000,000 (2.99%), of which
    /// **225,821 are lakes**: a **1,222x** rise for that same 2,000x rise in nodes, which
    /// is very nearly linear. The root *fraction* does still fall as spacing tightens,
    /// exactly as §8.3 describes and for the reason it gives; what does not hold is that
    /// the absolute count stays small, because a real field has relief at every scale the
    /// spacing can resolve and the probe field did not.
    ///
    /// So a scan of this table is 225,821 comparisons at planet scale, not 5,647. Slice 5
    /// calls it per lake while filling basins and will have to index it; the signature does
    /// not change when it does, which is why this is a note and not a fix.
    pub fn lake_at(&self, node: u32) -> Option<&Lake> {
        self.lakes.iter().find(|lake| lake.root_node == node)
    }

    pub fn reaches(&self) -> &[Reach] {
        &self.reaches
    }
}

// ---- node sampling ------------------------------------------------------------------
//
// §14.1 asks for "Poisson-sampled points". **This is not a Poisson-disc sampler, and the
// substitution is deliberate.** Three reasons, each measured or quoted rather than argued:
//
// 1. Bridson's algorithm is *definitionally sequential* — sample n depends on samples
//    1..n-1 — which is exactly the shape `generation.rs`'s module doc forbids in terms
//    ("Every value here is hashed, never drawn from a sequence"). A sequential sampler in
//    CORE-001 would be self-defeating: the slice whose purpose is that adding a field later
//    costs nothing would introduce the one sampler where adding a draw later moves every
//    node.
// 2. The Fibonacci spiral is *measurably the more regular point set*. Un-jittered, its
//    minimum neighbour separation is 0.872 x nominal spacing (§8.2, and re-measured here at
//    every size from 3,200 to 20,000,000). Bridson guarantees only its own radius r, and r
//    for a given point count sits well below nominal spacing because disc packing is loose.
// 3. §14.1's *stated* reason for the representation is that points and areas "transfer to a
//    sphere directly — no projection, no grid seam, no pole singularity". Those are
//    properties of the spiral, not of Poisson-disc sampling specifically. The property is
//    what matters and the spiral has it.
//
// Nothing in Bridson needs a banned API. The blocker is architecture, not the constraint
// list, and that is why it is recorded here rather than left to be rediscovered.

/// The nominal spacing of `count` nodes on a unit sphere, in radians: the side of the
/// equal-area square patch each node owns, `sqrt(4*pi/count)`.
///
/// Every jitter and separation figure in this module is a multiple of this, never an
/// absolute angle. That is the whole lesson of `generation::JITTER_RAD`, which is an
/// absolute 0.18 rad sized for 22 plates: at 20,000,000 nodes it is about 227x the spacing,
/// so every node would collide with several others and `build` would refuse the lot.
pub fn nominal_spacing_rad(count: u32) -> f64 {
    let n = f64::from(count);
    m::sqrt(4.0 * std::f64::consts::PI / n)
}

/// The same spacing on a planet of `radius_m`. 22,584.6 m at 1,000,000 nodes on Earth.
pub fn nominal_spacing_m(count: u32, radius_m: f64) -> f64 {
    nominal_spacing_rad(count) * radius_m
}

/// How far a node may be nudged off the even spiral, **as a fraction of nominal spacing**.
///
/// # Chosen by measurement, and the bound is provable rather than hopeful
///
/// The jitter adds `a*east + b*north` with `a` and `b` each drawn uniformly from
/// `[-J, +J]` where `J = NODE_JITTER_FRACTION * nominal_spacing_rad(count)`. The largest
/// tangent displacement is therefore `J*sqrt(2)` at the corners of that square, and the
/// arc displacement is `atan(J*sqrt(2))`, which is smaller. Two nodes can approach each
/// other by at most twice that, so
///
/// ```text
/// min_separation >= (0.872 - 2*sqrt(2)*NODE_JITTER_FRACTION) x nominal
/// ```
///
/// where 0.872 is the un-jittered spiral's own measured minimum — measured here at
/// **0.8723 at every one of 3,200 / 20,000 / 200,000 / 1,000,000 / 20,000,000 nodes**, to
/// four figures, which is why it can be used as a constant in a bound.
///
/// **Measured minimum separation, as a multiple of nominal spacing.** Seed 20260904, every
/// node in the set, candidate pairs from `neighbour_offsets` (whose sufficiency is checked
/// against brute force in the tests):
///
/// ```text
///  jitter |    3,200 |   20,000 |  200,000 | 1,000,000 | 20,000,000 | proved floor
///    0.00 |   0.8723 |   0.8723 |   0.8723 |    0.8723 |     0.8723 |  0.872
///    0.10 |   0.7215 |   0.7053 |   0.6907 |    0.6694 |          - |  0.589
///    0.15 |   0.5988 |   0.5783 |   0.5618 |    0.5312 |     0.5322 |  0.448
///    0.20 |   0.4768 |   0.4517 |   0.4329 |    0.3930 |          - |  0.307
///    0.30 |   0.2394 |   0.2013 |   0.1754 |    0.1165 |          - |  0.024
///    0.45 |   0.0805 |   0.0332 |   0.0054 |    0.0026 |          - | -0.401
/// ```
///
/// **0.15 is the choice, and 0.20 is not.** At 0.20 the measured ratio is already 0.393 at
/// a million nodes — *below* the 0.40 the type asserts — and its proved floor of 0.307 is
/// below it too. 0.15 clears the assertion by a third at every size measured, and clears it
/// by proof rather than by sample. At 0.45 two nodes land **59.6 m apart on a 22,584.6 m
/// lattice**; `StreamGraph::build` returns `CoincidentNodes` rather than resolving such a
/// pair, so this constant is the difference between a graph and a refusal.
///
/// **The failure does not announce itself at small sizes.** At 3,200 nodes even 0.45 keeps
/// 0.08 x nominal and a graph builds; the ratio falls by a further factor of thirty by a
/// million. A jitter validated at a few thousand nodes and shipped for twenty million is
/// exactly the shape of this bug.
///
/// Larger jitter also widens the cell-area spread — 0.737x to 1.260x the ideal cell at 0.15
/// against 0.524x to 1.546x at 0.30 (measured below) — and stream power goes as area^0.5,
/// so that spread is an erosion-rate error at exactly the headwater nodes that are hardest
/// to stabilise.
///
/// Smaller jitter is not free either: at 0.0 the point set is a visible spiral, its cells
/// are near-identical (CV 0.005), and the drainage network inherits its arms.
pub const NODE_JITTER_FRACTION: f64 = 0.15;

/// The un-jittered Fibonacci spiral's minimum neighbour separation, as a fraction of
/// nominal spacing. A property of the spiral itself, independent of seed and (measured)
/// stable in node count; the separation floor above is derived from it.
pub const SPIRAL_MIN_SEPARATION_FRACTION: f64 = 0.872;

/// The separation a sampled node set must keep, as a fraction of nominal spacing. Asserted
/// rather than assumed: this is the number a future tweak to `NODE_JITTER_FRACTION` would
/// silently cross, and crossing it produces sliver cells and slopes that divide by almost
/// nothing long before it produces an outright `CoincidentNodes`.
pub const MIN_SEPARATION_FRACTION: f64 = 0.40;

/// How many neighbours each node keeps. Eight is what every probe in the extraction used,
/// and it is comfortably above the six a hexagonal packing needs, so a node whose cell has
/// been stretched by jitter still has its real neighbours in the list.
pub const NEIGHBOUR_COUNT: usize = 8;

/// How many of those neighbours the area estimate uses. **Five, and the number was swept
/// rather than assumed**: against Monte-Carlo cell areas the RMS relative error runs
/// 7.79% at k = 2, 2.83% at k = 4, **2.46% at k = 5**, 3.37% at k = 6 and 4.90% at k = 8
/// (n = 20,000, and the same ordering at n = 5,000). The far neighbours of a stretched
/// cell sit *across* it rather than around it, so including them pulls every estimate
/// back towards the mean and throws away the variation the field exists to record.
pub const AREA_NEIGHBOUR_COUNT: usize = 5;

/// Distinct salts so the two tangent draws are independent. Named constants rather than 0
/// and 1 because a future third draw taking the "next" number is precisely how a hashed
/// sampler acquires a sequence by accident.
const JITTER_EAST_SALT: u64 = 0x6E6F_6465_2D65_6173; // "node-eas"
const JITTER_NORTH_SALT: u64 = 0x6E6F_6465_2D6E_6F72; // "node-nor"

/// An integer avalanche, not a BLAKE2b digest.
///
/// `generation::fraction` formats a `String` and takes an 8-byte digest of it, measured at
/// 332.1 ns per call (§8.6). Two draws per node at 20,000,000 nodes is 40,000,000 digests
/// and roughly 13 seconds of a 19-second sampling stage. The avalanche is the same one
/// `noise.rs` already uses, is equally index-addressed and equally free of any sequence,
/// and costs about a nanosecond.
///
/// **It is a different hash, so this sampler is not interchangeable with `spread`.** Nodes
/// and plates jitter by different numbers on the same seed, on purpose: they are different
/// point sets and nothing should be able to confuse them.
fn node_hash(world_seed: u64, index: u32, salt: u64) -> u64 {
    let mut h = world_seed.wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ u64::from(index).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ salt.wrapping_mul(0x1656_67B1_9E37_79F9);
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    h = h.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    h ^= h >> 33;
    h
}

const HASH_SCALE: f64 = 18_446_744_073_709_551_616.0; // 2^64, exactly representable

/// A number in `[0, 1)` from the seed, the node index and a salt, and from nothing else.
fn node_fraction(world_seed: u64, index: u32, salt: u64) -> f64 {
    // cast-ok: a u64 hash to f64 over an exact power of two, as `generation::fraction` does
    (node_hash(world_seed, index, salt) as f64) / HASH_SCALE
}

/// The un-jittered spiral point. Public so a test can compare a sampled node against the
/// lattice it was nudged off, rather than re-deriving the lattice by hand and proving only
/// that the test author can copy an expression.
pub fn spiral_point(index: u32, count: u32) -> SpherePoint {
    // `golden = pi * (3 - sqrt(5))` computed rather than written as a decimal literal:
    // sqrt(5) is correctly rounded, `3 - sqrt(5)` is exact, one multiplication follows.
    let golden = std::f64::consts::PI * (3.0 - m::sqrt(5.0));
    let i = f64::from(index);
    let n = f64::from(count);
    let z = 1.0 - 2.0 * (i + 0.5) / n;
    let inner = 1.0 - z * z;
    // House form, `plates.rs::margin_at`: an explicit if/else, never `f64::max`, and in the
    // operand order the Python's `max(0.0, ...)` uses so a NaN floors instead of spreading.
    let ring = m::sqrt(if inner > 0.0 { inner } else { 0.0 });
    let angle = golden * i;
    SpherePoint { vector: Vec3::new(m::cos(angle) * ring, m::sin(angle) * ring, z) }
}

/// Where node `index` of `count` sits, for `world_seed`.
///
/// Index-addressed: depends on those three arguments and nothing else. No sequence, no
/// accumulator, no dependence on how many nodes were sampled before it. Split out from
/// `node_positions` so a caller — or a test — can ask for one node without materialising
/// twenty million.
///
/// `jitter_scale` is deliberately not a parameter: a caller who could pass 0 could pass
/// 0.45, and the un-jittered spiral is reachable for tests through `spiral_point` instead.
pub fn node_position(world_seed: u64, index: u32, count: u32) -> SpherePoint {
    node_position_at_jitter(world_seed, index, count, NODE_JITTER_FRACTION)
}

/// The sampler with its jitter fraction exposed. **Private on purpose.** It exists so the
/// measurement tests can sweep the fraction and so a test can plant a value that produces
/// coincident nodes and watch `build` refuse it; no caller outside this module may choose
/// a jitter, because a chosen jitter is a different planet.
fn node_position_at_jitter(
    world_seed: u64,
    index: u32,
    count: u32,
    jitter_fraction: f64,
) -> SpherePoint {
    let point = spiral_point(index, count);
    let jitter = jitter_fraction * nominal_spacing_rad(count);
    let nudge_east = (2.0 * node_fraction(world_seed, index, JITTER_EAST_SALT) - 1.0) * jitter;
    let nudge_north = (2.0 * node_fraction(world_seed, index, JITTER_NORTH_SALT) - 1.0) * jitter;

    let mut sideways = Vec3::new(0.0, 0.0, 1.0).cross(&point.vector);
    // `(0,0,1) x point` is `(-y, x, 0)`, whose length is exactly the spiral's ring radius,
    // smallest at index 0 where it is about `sqrt(2/count)`. Reaching DEGENERATE (1e-9)
    // would need count above 2e18, which `MAX_NODES` forbids four hundred million times
    // over. Carried anyway, in the same form `generation::spread_impl` carries it, so that
    // the two samplers do not differ in a way a reader has to explain.
    if sideways.length() < DEGENERATE {
        sideways = Vec3::new(1.0, 0.0, 0.0).cross(&point.vector);
    }
    let east = sideways
        .normalised()
        .expect("point is a unit vector, so sideways cannot be the zero vector here");
    let north = point.vector.cross(&east);
    let nudged = point.vector.add(&east.scaled(nudge_east)).add(&north.scaled(nudge_north));
    SpherePoint::from_vector(&nudged).expect("a unit vector plus a bounded nudge is non-zero")
}

/// Every node position, in index order.
pub fn node_positions(world_seed: u64, count: u32) -> Vec<SpherePoint> {
    node_positions_at_jitter(world_seed, count, NODE_JITTER_FRACTION)
}

fn node_positions_at_jitter(world_seed: u64, count: u32, jitter_fraction: f64) -> Vec<SpherePoint> {
    let mut out = Vec::with_capacity(count as usize); // cast-ok: a node count into usize
    for index in 0..count {
        out.push(node_position_at_jitter(world_seed, index, count, jitter_fraction));
    }
    out
}

/// The index offsets at which a node's near neighbours are found on a Fibonacci spiral.
///
/// Node `i` and node `i + d` are close only when `d * golden mod 2*pi` is close to zero,
/// and the offsets with that property are the **Fibonacci numbers** — they are the
/// convergents of the golden ratio's continued fraction, which is what makes the spiral a
/// spiral. Small multiples and small offsets are included because jitter moves points and
/// because the first few rings are not asymptotic.
///
/// This is a *candidate* set, not an answer: `nearest_neighbours` measures every candidate.
/// The set's sufficiency is checked against brute force in the tests, at sizes where brute
/// force is affordable, and that check is the reason to trust it at sizes where it is not.
pub fn neighbour_offsets(count: u32) -> Vec<u32> {
    let mut offsets: Vec<u32> = Vec::new();
    let half = if count / 2 > 1 { count / 2 } else { 1 };
    for d in 1..=32u32 {
        if d <= half {
            offsets.push(d);
        }
    }
    // Consecutive Fibonacci pairs are the lattice basis at each latitude, so a near
    // neighbour is `a*F_k + b*F_(k+1)` for small integer `a` and `b`. Taking |combination|
    // rather than a signed offset is right because `nearest_neighbours` walks both
    // directions from each node anyway.
    let (mut a, mut b) = (1i64, 2i64);
    while a <= i64::from(half) {
        for p in -3i64..=3 {
            for q in -3i64..=3 {
                let combo = (p * a + q * b).abs();
                if combo >= 1 && combo <= i64::from(half) {
                    offsets.push(combo as u32); // cast-ok: bounded by half, itself a u32
                }
            }
        }
        let next = a + b;
        a = b;
        b = next;
    }
    offsets.sort_unstable();
    offsets.dedup();
    offsets
}

/// The `k` nearest nodes to `index`, nearest first.
///
/// Ordered by `(angle.to_bits(), node_index)` — **integers, never a float comparison**.
/// A non-negative `f64`'s bit pattern orders the same way the number does, so this is the
/// same order without a comparison that could be decided differently by a different
/// instruction schedule; the index breaks every remaining tie one fixed way. `plates.rs`,
/// `generation.rs` and `sphere.rs` each carry a comment about this class of bug.
pub fn nearest_neighbours(
    positions: &[SpherePoint],
    offsets: &[u32],
    index: u32,
    k: usize,
) -> Vec<u32> {
    let count = positions.len();
    let i = index as usize; // cast-ok: a node index into usize
    let mut candidates: Vec<(u64, u32)> = Vec::with_capacity(offsets.len() * 2);
    for &offset in offsets {
        let offset_usize = offset as usize; // cast-ok: an index offset into usize
        if offset_usize >= count {
            continue;
        }
        // Both directions, and no wrap. **The reason is cost, not correctness** — a
        // mutation that wrapped the offsets was run, and it changes no neighbour list,
        // because index 0 and index count-1 are the spiral's two poles and a candidate
        // three radians away never survives a selection made on distance. Wrapping would
        // simply measure candidates that cannot win, twice per offset per node.
        if i >= offset_usize {
            let j = i - offset_usize;
            let node = j as u32; // cast-ok: a node index, bounded by the slice length
            candidates.push((positions[i].angle_to(&positions[j]).to_bits(), node));
        }
        if i + offset_usize < count {
            let j = i + offset_usize;
            let node = j as u32; // cast-ok: a node index, bounded by the slice length
            candidates.push((positions[i].angle_to(&positions[j]).to_bits(), node));
        }
    }
    candidates.sort_unstable();
    candidates.dedup();
    candidates.truncate(k);
    candidates.into_iter().map(|(_, node)| node).collect()
}

/// The neighbour lists for a whole sampling, in the shape `StreamGraph::build` wants.
pub fn node_neighbours(positions: &[SpherePoint], k: usize) -> Vec<Vec<u32>> {
    let count = positions.len();
    let offsets = neighbour_offsets(count as u32); // cast-ok: a node count, bounded by build
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        out.push(nearest_neighbours(positions, &offsets, i as u32, k)); // cast-ok: node index
    }
    out
}

/// Each node's share of the planet's surface, in square metres.
///
/// # The method, and why it is not a Voronoi diagram
///
/// **No spherical Voronoi.** An exact spherical Voronoi diagram is a degeneracy-prone
/// exact-predicate problem — cocircular sites, collinear sites, sites at antipodes — and
/// under DETERMINISM-001 a predicate that is *nearly* right is a different planet, not a
/// slightly wrong one. It deserves its own slice if it is ever wanted, and slice 5 can add
/// it without changing a single type, because the field is already here.
///
/// What is here instead is a **normalised local-density estimate**: a node's cell area goes
/// as the square of its own mean neighbour distance, and the whole set is then scaled so it
/// sums to the sphere. In symbols, with `d_i` the mean great-circle angle from node `i` to
/// its neighbours,
///
/// ```text
/// area_i = 4*pi*R^2 * d_i^2 / sum_j d_j^2
/// ```
///
/// Two properties earn it:
///
/// - **The total is exact by construction.** `sum(area_i)` is the sphere's area to within
///   accumulated rounding, so a drainage area accumulated over a whole continent cannot
///   drift away from the geography. A per-cell estimate that was locally better but summed
///   to 1.03 spheres would be worse where it matters most.
/// - **It varies where the geometry varies**, which is the entire reason §3.2 refuses to
///   let this be a shared constant: the true cell area ranges **0.737x to 1.260x** the
///   ideal at this jitter — measured on this crate's own sampler by
///   `measure_cell_area_spread` (n = 5,000 nodes at seed 20260904, jitter 0.15, 4,000,000
///   Monte-Carlo probes, CV 0.0754), *not* the extraction's §8.4 figure of 0.735x to
///   1.285x, which is its probe field — and stream power goes as area^0.5, so a constant
///   is a systematic erosion error at headwaters.
///
/// # Its error, stated
///
/// Against Monte-Carlo nearest-node areas — n = 20,000 nodes at seed 20260904, jitter 0.15,
/// against 8,000,000 probe points drawn from a much denser spiral at seed 777000001 and
/// jitter 0.30 so the two sets are not aligned — this estimator has an **RMS relative error
/// of 2.46%, a worst single-node error of 13.7%, and a correlation of 0.9479** with the
/// true cell area. It recovers a spread of 0.756x to 1.270x against a true 0.737x to
/// 1.260x, **both at n = 5,000 nodes** — so it tracks the variation rather than flattening
/// it. The CVs of 0.0731 and 0.0753 sometimes quoted beside those ranges belong to the
/// estimator-error measurement at **n = 20,000**; they are the same quantity at a different
/// population and must not be read as this range's own.
///
/// The number to compare that against is **the constant it replaces**, whose RMS relative
/// error is by definition the true spread's own CV of 7.5% and whose correlation with the
/// truth is **undefined** -- a constant has zero standard deviation, so Pearson's r divides
/// by zero. The intuition "it carries no information about the cell" is right; the number is
/// not zero, it does not exist. So this is roughly a threefold reduction in area error, for
/// one extra
/// pass over a neighbour list that had to be built anyway.
///
/// It is an approximation and is documented as one; what it must not be is an *unstated*
/// approximation, because `drainage_area_m2` is a sum of these and a reader of the file has
/// no way to tell which method produced it.
pub fn node_areas_m2(
    positions: &[SpherePoint],
    neighbours: &[Vec<u32>],
    radius_m: f64,
) -> Vec<f64> {
    let count = positions.len();
    let mut weight = Vec::with_capacity(count);
    let mut total = 0.0f64;
    for i in 0..count {
        let mut sum = 0.0f64;
        let mut used = 0.0f64;
        for &j in neighbours[i].iter().take(AREA_NEIGHBOUR_COUNT) {
            sum += positions[i].angle_to(&positions[j as usize]); // cast-ok: a node index
            used += 1.0;
        }
        // A node with no neighbours cannot happen for count >= 2. Written as a branch
        // rather than a division that would produce NaN, because `build` refuses a
        // non-positive area and a NaN one would be refused for the wrong reason.
        let mean = if used > 0.0 { sum / used } else { 1.0 };
        let w = mean * mean;
        weight.push(w);
        total += w;
    }
    let sphere_m2 = 4.0 * std::f64::consts::PI * radius_m * radius_m;
    let mut out = Vec::with_capacity(count);
    for w in weight {
        out.push(sphere_m2 * w / total);
    }
    out
}

/// A complete node set: where the nodes are, who their neighbours are, and how much of the
/// planet each one owns. Exactly the three arrays `StreamGraph::build` asks for, minus the
/// heights, which come from `Surface` and are not this module's business.
#[derive(Debug, Clone)]
pub struct NodeSampling {
    pub positions: Vec<SpherePoint>,
    pub neighbours: Vec<Vec<u32>>,
    pub area_m2: Vec<f64>,
}

/// Sample `count` nodes for `world_seed` on a planet of `radius_m`.
///
/// `None` when the count cannot be addressed alongside `NO_DOWNHILL`, or is below two: a
/// one-node planet has no neighbour relation and therefore no drainage, and returning a
/// degenerate graph would be worse than declining.
pub fn sample_nodes(world_seed: u64, count: u32, radius_m: f64) -> Option<NodeSampling> {
    let fits = node_count_fits(count as usize); // cast-ok: a node count widened to usize
    if count < 2 || !fits {
        return None;
    }
    let positions = node_positions(world_seed, count);
    let neighbours = node_neighbours(&positions, NEIGHBOUR_COUNT);
    let area_m2 = node_areas_m2(&positions, &neighbours, radius_m);
    Some(NodeSampling { positions, neighbours, area_m2 })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- the deterministic stand-in field -------------------------------------------

    const ROWS: usize = 40;
    const COLS: usize = 80;

    fn splitmix64(x: u64) -> u64 {
        let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A latitude/longitude lattice, not a sphere sampler: Task 4 owns sampling, and this
    /// fixture deliberately looks nothing like it. Longitude wraps, latitude does not, so
    /// the adjacency has genuine edges and genuine interior.
    struct Field {
        positions: Vec<crate::sphere::SpherePoint>,
        height_m: Vec<f64>,
        area_m2: Vec<f64>,
        neighbours: Vec<Vec<u32>>,
    }

    fn lattice_field(seed: u64) -> Field {
        let count = ROWS * COLS;
        let mut positions = Vec::with_capacity(count);
        let mut height_m = Vec::with_capacity(count);
        let mut area_m2 = Vec::with_capacity(count);
        let mut neighbours = Vec::with_capacity(count);
        let rows_f = (ROWS - 1) as f64; // cast-ok: a lattice extent, already an integer
        let cols_f = COLS as f64; // cast-ok: a lattice extent, already an integer
        let sphere_m2 = 4.0 * std::f64::consts::PI * 6_371_000.0 * 6_371_000.0;
        let cell = sphere_m2 / (count as f64); // cast-ok: a lattice extent, already integer
        for r in 0..ROWS {
            let r_f = r as f64; // cast-ok: a lattice row index, already an integer
            let latitude_deg = -80.0 + 160.0 * r_f / rows_f;
            let shrink = crate::detmath::cos(crate::detmath::to_radians(latitude_deg));
            for c in 0..COLS {
                let c_f = c as f64; // cast-ok: a lattice column index, already an integer
                let longitude_deg = -180.0 + 360.0 * c_f / cols_f;
                positions.push(crate::sphere::SpherePoint::from_latlon(latitude_deg, longitude_deg));
                let index = r * COLS + c;
                let bits = splitmix64(seed ^ splitmix64(index as u64)); // cast-ok: a lattice index
                let unit = ((bits >> 11) as f64) / 9_007_199_254_740_992.0; // cast-ok: 53 bits into f64
                height_m.push(-1500.0 + 4500.0 * unit);
                area_m2.push(cell * (0.5 + shrink));
                let mut near = Vec::with_capacity(4);
                let west = (c + COLS - 1) % COLS;
                let east = (c + 1) % COLS;
                near.push((r * COLS + west) as u32); // cast-ok: a lattice index into u32
                near.push((r * COLS + east) as u32); // cast-ok: a lattice index into u32
                if r > 0 {
                    near.push(((r - 1) * COLS + c) as u32); // cast-ok: a lattice index into u32
                }
                if r + 1 < ROWS {
                    near.push(((r + 1) * COLS + c) as u32); // cast-ok: a lattice index into u32
                }
                neighbours.push(near);
            }
        }
        Field { positions, height_m, area_m2, neighbours }
    }

    fn params(seed: u64, sea_level_m: f64) -> BuildParams {
        BuildParams {
            world_seed: seed,
            radius_m: crate::sphere::EARTH_RADIUS_M,
            sea_level_m,
            sampling_kind: SamplingKind::Supplied,
            pond_max_drainage_area_m2: 1.0e10,
        }
    }

    /// `reaches` is reserved empty in slice 1p (`reaches_are_reserved_and_empty_in_this_
    /// slice`), so a graph off `built` can never witness the reach comparisons. This plants
    /// one record by hand -- the comparison has to be pinned now, or slice 5 inherits a
    /// column that `bit_identical_to` names and no test exercises.
    fn built_with_a_planted_reach(seed: u64, sea_level_m: f64) -> StreamGraph {
        let mut graph = built(seed, sea_level_m);
        assert!(graph.reaches.is_empty(), "slice 1p ships no reaches; this plants the first");
        graph.reaches.push(Reach { from_node: 5, to_node: 6, gradient: 0.25 });
        graph
    }

    /// One ULP up, by bits, so a perturbation is the smallest change that still is one.
    fn flip_last_bit(x: f64) -> f64 {
        f64::from_bits(x.to_bits() ^ 1)
    }

    fn built(seed: u64, sea_level_m: f64) -> StreamGraph {
        let field = lattice_field(seed);
        StreamGraph::build(
            &params(seed, sea_level_m),
            &field.positions,
            &field.height_m,
            &field.area_m2,
            &field.neighbours,
        )
        .expect("the stand-in field builds a valid graph")
    }

    // ---- property 4: the sentinel behaves as a root, not as an index ------------------

    #[test]
    fn the_sentinel_is_named_and_is_not_a_reachable_node_index() {
        assert_eq!(NO_DOWNHILL, u32::MAX);
        assert_eq!(NO_LAKE, u32::MAX);
        // The whole point: no legal graph can address the sentinel.
        assert_eq!(MAX_NODES, u32::MAX - 1);
        assert!(!sentinel_is_a_valid_index(MAX_NODES, NO_DOWNHILL));
        assert!(node_count_fits(MAX_NODES as usize)); // cast-ok: widening a u32 to usize
        assert!(!node_count_fits(NO_DOWNHILL as usize)); // cast-ok: widening a u32 to usize
    }

    #[test]
    fn the_sentinel_check_catches_a_sentinel_that_is_a_valid_index() {
        // The planted defect: a sentinel inside the node range. If this ever returns
        // false the guard above is vacuous.
        assert!(sentinel_is_a_valid_index(5, 3));
        assert!(sentinel_is_a_valid_index(5, 4));
        assert!(!sentinel_is_a_valid_index(5, 5));
    }

    #[test]
    fn a_root_reports_no_downhill_rather_than_an_index() {
        let graph = built(20_260_904, 0.0);
        let roots = graph.roots();
        assert!(!roots.is_empty(), "the stand-in field must produce roots");
        for &root in &roots {
            assert!(!graph.has_downhill(root));
            assert_eq!(graph.downhill_of(root), None);
            assert_eq!(graph.downhill_raw(root), NO_DOWNHILL);
        }
        for i in 0..graph.node_count() {
            if graph.has_downhill(i) {
                assert!(graph.downhill_of(i).is_some());
                assert_ne!(graph.downhill_raw(i), NO_DOWNHILL);
            }
        }
    }

    #[test]
    fn a_self_downhill_is_a_defect_not_the_root_convention() {
        let mut graph = built(20_260_904, 0.0);
        let victim = *graph.roots().first().expect("a root");
        graph.downhill[victim as usize] = victim; // cast-ok: a node index into usize
        let defects = graph.validate().expect_err("a self-loop must not validate");
        assert!(defects.contains(&GraphDefect::SelfDownhill { node: victim }));
    }

    // ---- property 1: the downhill relation is a strict forest -------------------------

    #[test]
    fn the_downhill_relation_is_a_strict_forest() {
        let graph = built(20_260_904, 0.0);
        let peel = graph.peel();
        assert_eq!(
            peel.peeled,
            graph.node_count(),
            "every node must peel; unpeeled nodes are a cycle"
        );
        assert_eq!(peel.order.len(), graph.node_count() as usize); // cast-ok: a node count
        assert!(graph.validate().is_ok());
    }

    #[test]
    fn peeling_catches_a_planted_two_cycle() {
        let mut graph = built(20_260_904, 0.0);
        // Two nodes that point at each other: nothing outside the pair can ever bring
        // either to in-degree zero.
        let a = 0u32;
        let b = graph.downhill_of(a).unwrap_or(1);
        graph.downhill[a as usize] = b; // cast-ok: a node index into usize
        graph.downhill[b as usize] = a; // cast-ok: a node index into usize
        let peel = graph.peel();
        assert!(peel.peeled < graph.node_count(), "the peel must strand the cycle");
        let defects = graph.validate().expect_err("a cycle must not validate");
        assert!(defects.iter().any(|d| matches!(d, GraphDefect::Cycle { .. })));
    }

    #[test]
    fn peeling_catches_a_planted_self_loop() {
        let mut graph = built(20_260_904, 0.0);
        graph.downhill[7] = 7;
        assert!(graph.peel().peeled < graph.node_count());
    }

    #[test]
    fn downhill_is_strictly_descending_and_the_check_catches_an_ascent() {
        let graph = built(20_260_904, 0.0);
        for i in 0..graph.node_count() {
            if let Some(target) = graph.downhill_of(i) {
                assert!(graph.height_m(target) < graph.height_m(i), "node {i} does not descend");
            }
        }
        // The mutation: point a node at a strictly higher neighbour.
        let mut broken = built(20_260_904, 0.0);
        let mut planted = None;
        for i in 0..broken.node_count() {
            if let Some(target) = broken.downhill_of(i) {
                broken.downhill[target as usize] = i; // cast-ok: a node index into usize
                planted = Some(target);
                break;
            }
        }
        let planted = planted.expect("the field must have at least one descending edge");
        let defects = broken.validate().expect_err("an ascent must not validate");
        assert!(defects
            .iter()
            .any(|d| matches!(d, GraphDefect::NotDescending { node, .. } if *node == planted)));
    }

    // ---- property 2: roots partition into mouths and lakes ----------------------------

    #[test]
    fn every_root_is_exactly_one_of_a_mouth_and_a_lake() {
        let graph = built(20_260_904, 0.0);
        let roots = graph.roots();
        let mut mouths = 0usize;
        let mut lakes = 0usize;
        for &root in &roots {
            let is_mouth = graph.has_flag(root, flag::MOUTH);
            let is_lake = graph.lake_at(root).is_some();
            assert!(is_mouth != is_lake, "root {root} is both or neither");
            if is_mouth {
                mouths += 1;
            } else {
                lakes += 1;
            }
        }
        assert_eq!(mouths + lakes, roots.len());
        assert!(mouths > 0 && lakes > 0, "the fixture must exercise both arms");
        assert_eq!(graph.lakes().len(), lakes);
        assert!(graph.validate().is_ok());
    }

    #[test]
    fn the_partition_catches_a_root_that_is_neither() {
        let mut graph = built(20_260_904, 0.0);
        let victim = *graph
            .roots()
            .iter()
            .find(|&&r| graph.has_flag(r, flag::MOUTH))
            .expect("a mouth root");
        graph.flags[victim as usize] &= !flag::MOUTH; // cast-ok: a node index into usize
        let defects = graph.validate().expect_err("a root with no class must not validate");
        assert!(defects.contains(&GraphDefect::RootIsNeitherMouthNorLake { node: victim }));
    }

    #[test]
    fn the_partition_catches_a_root_that_is_both() {
        let mut graph = built(20_260_904, 0.0);
        let victim = *graph
            .roots()
            .iter()
            .find(|&&r| graph.lake_at(r).is_some())
            .expect("a lake root");
        graph.flags[victim as usize] |= flag::MOUTH; // cast-ok: a node index into usize
        let defects = graph.validate().expect_err("a double-classed root must not validate");
        assert!(defects.contains(&GraphDefect::RootIsBothMouthAndLake { node: victim }));
    }

    #[test]
    fn the_partition_catches_a_class_hung_on_a_non_root() {
        let mut graph = built(20_260_904, 0.0);
        let inner = (0..graph.node_count())
            .find(|&i| graph.has_downhill(i))
            .expect("an interior node");
        graph.flags[inner as usize] |= flag::MOUTH; // cast-ok: a node index into usize
        let defects = graph.validate().expect_err("a mouth mid-stream must not validate");
        assert!(defects.contains(&GraphDefect::MouthAtNonRoot { node: inner }));

        let mut other = built(20_260_904, 0.0);
        other.lakes.push(Lake {
            root_node: inner,
            level_m: 0.0,
            kind: LakeKind::Pond,
            outflow_lake: NO_LAKE,
        });
        let defects = other.validate().expect_err("a lake mid-stream must not validate");
        assert!(defects.contains(&GraphDefect::LakeAtNonRoot { node: inner }));
    }

    #[test]
    fn two_lake_records_on_one_root_are_a_defect() {
        let mut graph = built(20_260_904, 0.0);
        let duplicate = *graph.lakes().first().expect("a lake");
        graph.lakes.push(duplicate);
        let defects = graph.validate().expect_err("a duplicated lake must not validate");
        assert!(defects.contains(&GraphDefect::DuplicateLakeRoot { node: duplicate.root_node }));
    }

    #[test]
    fn sea_level_is_load_bearing_for_the_classification() {
        // The same field at two datums must classify differently, or `sea_level_m` in the
        // header would be decoration and a graph read at the wrong datum would look fine.
        let low = built(20_260_904, -1400.0);
        let high = built(20_260_904, 2900.0);
        assert_ne!(low.mouth_count(), high.mouth_count());
        assert_ne!(low.lakes().len(), high.lakes().len());
        assert!(low.lakes().len() > high.lakes().len());
    }

    /// **The root count is invariant across SEA LEVELS at a fixed node count. It is not
    /// invariant across node counts, and quoting it without its population is how a wrong
    /// number travels.**
    ///
    /// The reason is one line of `build`: a root is a node with no strictly-lower
    /// neighbour, and the datum appears nowhere in that test. The datum only decides how
    /// the roots are *labelled*, so it moves the mouth/lake split and never the total.
    ///
    /// **The population is this module's 40x80 lattice fixture, 3,200 nodes**, where 633
    /// roots is about 19.8% of the nodes. Roots are a **fraction of the node set, and the
    /// fraction falls as spacing tightens**: at coarse spacing nearly every node is a local
    /// extremum, and as nodes come closer together flow organises into chains. Every figure
    /// in circulation is a different population and none of them contradicts another --
    /// 19.8% here at 3,200 lattice nodes; the extraction's §8.3 3% at 10,000 and 0.03% at
    /// 20,000,000 over its probe field; and 4.89% at 10,000 falling to 2.99% at 20,000,000
    /// over this crate's own `Surface` (Task 6's `streambench`). A root count means nothing
    /// without the node count and the field it was taken over.
    #[test]
    fn the_root_count_is_invariant_across_datums_at_a_fixed_node_count() {
        let counts: Vec<(f64, usize, usize, usize)> = [-1400.0f64, 0.0, 2900.0]
            .iter()
            .map(|&datum| {
                let graph = built(20_260_904, datum);
                (datum, graph.roots().len(), graph.mouth_count(), graph.lakes().len())
            })
            .collect();
        for &(datum, roots, mouths, lakes) in &counts {
            assert_eq!(roots, 633, "the 3,200-node lattice has 633 roots at datum {datum}");
            assert_eq!(mouths + lakes, roots, "every root is exactly one of the two");
        }
        // And the split does move, which is what makes the invariance a statement rather
        // than a tautology about a classification nothing depends on.
        assert_eq!(counts[0].2, 64);
        assert_eq!(counts[1].2, 554);
        assert_eq!(counts[2].2, 633);
        // 3,200 nodes, so 633 roots is 19.78% -- an order of magnitude above the fraction
        // the same code produces at a million nodes. The percentage is the thing that
        // moves with node count, so the population is quoted with the number, always.
        assert_eq!(ROWS * COLS, 3_200);
    }

    // ---- property 3: rebuilds are bit-identical --------------------------------------

    #[test]
    fn rebuilds_are_bit_identical_for_the_same_seed_and_parameters() {
        let a = built(20_260_904, 0.0);
        let b = built(20_260_904, 0.0);
        assert!(a.bit_identical_to(&b), "same seed and parameters must rebuild exactly");
    }

    #[test]
    fn a_different_seed_gives_a_different_graph() {
        // The negative control. Without it the equality above passes for a graph that
        // ignores its inputs entirely.
        let a = built(20_260_904, 0.0);
        let b = built(20_260_905, 0.0);
        assert!(!a.bit_identical_to(&b));
    }

    #[test]
    fn bit_identity_notices_a_single_last_bit() {
        let a = built(20_260_904, 0.0);
        let mut b = built(20_260_904, 0.0);
        let bits = b.drainage_area_m2[3].to_bits() ^ 1;
        b.drainage_area_m2[3] = f64::from_bits(bits);
        assert!(!a.bit_identical_to(&b), "bit identity must compare bits, not values");
    }

    /// One flipped bit in `drainage_area_m2` is not a negative control for eleven
    /// comparisons. The review found that deleting the `area_m2`, `height_m`, `flags` or
    /// `header.world_seed` compare from `bit_identical_to` left all tests green -- only
    /// `drainage_area_m2` was actually pinned, and `area_m2` is the field §3.2 argues
    /// hardest to carry from slice 1. This walks **every column the comparison names**, so
    /// deleting any one of them turns this test red.
    #[test]
    fn bit_identity_notices_a_change_in_every_column() {
        let a = built_with_a_planted_reach(20_260_904, 0.0);
        assert!(!a.lakes.is_empty(), "the fixture must have a lake for the lake columns");

        let perturbations: [(&str, fn(&mut StreamGraph)); 22] = [
            ("header.generator_version", |g| g.header.generator_version ^= 1),
            ("header.world_seed", |g| g.header.world_seed ^= 1),
            ("header.node_count", |g| g.header.node_count -= 1),
            ("header.sampling_kind", |g| g.header.sampling_kind = SamplingKind::Spiral),
            ("header.position_checksum", |g| g.header.position_checksum ^= 1),
            ("header.radius_m", |g| g.header.radius_m = flip_last_bit(g.header.radius_m)),
            ("header.sea_level_m", |g| g.header.sea_level_m = flip_last_bit(g.header.sea_level_m)),
            ("downhill", |g| g.downhill[7] ^= 1),
            ("flags", |g| g.flags[7] ^= flag::MOUTH),
            ("height_m", |g| g.height_m[3] = flip_last_bit(g.height_m[3])),
            ("area_m2", |g| g.area_m2[3] = flip_last_bit(g.area_m2[3])),
            ("drainage_area_m2", |g| {
                g.drainage_area_m2[3] = flip_last_bit(g.drainage_area_m2[3]);
            }),
            ("height_m.len", |g| {
                g.height_m.pop();
            }),
            ("area_m2.len", |g| {
                g.area_m2.pop();
            }),
            ("drainage_area_m2.len", |g| {
                g.drainage_area_m2.pop();
            }),
            ("lakes.len", |g| {
                g.lakes.pop();
            }),
            ("lakes[0].root_node", |g| g.lakes[0].root_node ^= 1),
            ("lakes[0].kind", |g| {
                g.lakes[0].kind =
                    if g.lakes[0].kind == LakeKind::Pond { LakeKind::Lake } else { LakeKind::Pond };
            }),
            ("lakes[0].outflow_lake", |g| g.lakes[0].outflow_lake ^= 1),
            ("lakes[0].level_m", |g| g.lakes[0].level_m = flip_last_bit(g.lakes[0].level_m)),
            ("reaches.len", |g| {
                g.reaches.pop();
            }),
            ("reaches[0].gradient", |g| {
                g.reaches[0].gradient = flip_last_bit(g.reaches[0].gradient);
            }),
        ];

        for (column, perturb) in perturbations {
            let mut b = built_with_a_planted_reach(20_260_904, 0.0);
            perturb(&mut b);
            assert!(
                !a.bit_identical_to(&b),
                "a change to {column} must not be reported bit-identical"
            );
            assert!(!b.bit_identical_to(&a), "and the comparison must be symmetric in {column}");
        }
    }

    /// The reach endpoints, split out because they take the planted-reach fixture and
    /// because `reaches.len()` is witnessed separately (a graph with no reach against one
    /// with a reach).
    #[test]
    fn bit_identity_notices_a_changed_reach_endpoint() {
        let bare = built(20_260_904, 0.0);
        let planted = built_with_a_planted_reach(20_260_904, 0.0);
        assert!(!bare.bit_identical_to(&planted), "a graph that grew a reach is a different graph");

        for (column, perturb) in [
            (
                "from_node",
                (|g: &mut StreamGraph| g.reaches[0].from_node ^= 1) as fn(&mut StreamGraph),
            ),
            ("to_node", |g: &mut StreamGraph| g.reaches[0].to_node ^= 1),
        ] {
            let mut b = built_with_a_planted_reach(20_260_904, 0.0);
            perturb(&mut b);
            assert!(
                !planted.bit_identical_to(&b),
                "a change to reaches[0].{column} must be noticed"
            );
        }
    }

    /// The doc comment on `bit_identical_to` claims bits and never `==`. These are the two
    /// perturbations that tell the two predicates apart, applied to **every float column**:
    ///
    /// - `0.0 == -0.0` is true and the bit patterns differ, so a `to_bits()` comparison must
    ///   call the pair *different* and a `==` comparison would call it the same;
    /// - `NaN == NaN` is false and two identical NaNs have identical bits, so `to_bits()`
    ///   must call the pair *identical* and `==` would call it different.
    ///
    /// Downgrading any float comparison in `bit_identical_to` from `to_bits()` to `==` turns
    /// one half or the other red. Before this test, `height_m` and `drainage_area_m2` could
    /// both be downgraded with the whole suite still green.
    #[test]
    fn bit_identity_compares_bits_and_not_values_in_every_float_column() {
        let columns: [(&str, fn(&mut StreamGraph, f64)); 7] = [
            ("header.radius_m", |g, v| g.header.radius_m = v),
            ("header.sea_level_m", |g, v| g.header.sea_level_m = v),
            ("height_m[3]", |g, v| g.height_m[3] = v),
            ("area_m2[3]", |g, v| g.area_m2[3] = v),
            ("drainage_area_m2[3]", |g, v| g.drainage_area_m2[3] = v),
            ("lakes[0].level_m", |g, v| g.lakes[0].level_m = v),
            ("reaches[0].gradient", |g, v| g.reaches[0].gradient = v),
        ];
        for (column, set) in columns {
            let mut a = built_with_a_planted_reach(20_260_904, 0.0);
            let mut b = built_with_a_planted_reach(20_260_904, 0.0);
            set(&mut a, 0.0);
            set(&mut b, -0.0);
            assert!(
                !a.bit_identical_to(&b),
                "{column}: +0.0 and -0.0 are `==` and are not the same bits"
            );

            let mut a = built_with_a_planted_reach(20_260_904, 0.0);
            let mut b = built_with_a_planted_reach(20_260_904, 0.0);
            set(&mut a, f64::NAN);
            set(&mut b, f64::NAN);
            assert!(
                a.bit_identical_to(&b),
                "{column}: two NaNs with the same bits are the same graph; `==` would disagree"
            );
        }
    }

    // ---- the fields that look optional and are not -----------------------------------

    #[test]
    fn area_is_per_node_and_varies() {
        let graph = built(20_260_904, 0.0);
        let first = graph.area_m2(0);
        assert!((0..graph.node_count()).any(|i| graph.area_m2(i) != first));
        for i in 0..graph.node_count() {
            assert!(graph.area_m2(i) > 0.0);
            assert!(graph.drainage_area_m2(i) >= graph.area_m2(i), "a node drains at least itself");
        }
    }

    #[test]
    fn drainage_over_the_roots_accounts_for_every_cell_exactly_once() {
        let graph = built(20_260_904, 0.0);
        let mut total = 0.0f64;
        for i in 0..graph.node_count() {
            total += graph.area_m2(i);
        }
        let mut at_roots = 0.0f64;
        for &root in &graph.roots() {
            at_roots += graph.drainage_area_m2(root);
        }
        let error = (at_roots - total).abs() / total;
        assert!(error < 1.0e-9, "drainage lost or double-counted: relative {error}");
    }

    #[test]
    fn the_flags_bitset_has_room_left() {
        let graph = built(20_260_904, 0.0);
        let used = flag::LAND | flag::BOUNDARY | flag::MOUTH | flag::LAKE_MEMBER;
        assert_eq!(used, 0b0000_1111, "four bits used, four spare");
        for i in 0..graph.node_count() {
            assert_eq!(graph.flags_of(i) & !used, 0, "a spare bit is set");
            // land and boundary are the two halves of the datum test, never both.
            assert!(graph.has_flag(i, flag::LAND) != graph.has_flag(i, flag::BOUNDARY));
        }
    }

    #[test]
    fn the_header_carries_what_a_reader_needs_to_refuse() {
        let graph = built(20_260_904, 0.0);
        let header = graph.header();
        assert_eq!(header.generator_version, crate::GENERATOR_VERSION);
        assert_eq!(header.world_seed, 20_260_904);
        assert_eq!(header.node_count, (ROWS * COLS) as u32); // cast-ok: a lattice extent
        assert_eq!(header.radius_m, crate::sphere::EARTH_RADIUS_M);
        assert_eq!(header.sea_level_m, 0.0);
        assert_eq!(header.sampling_kind, SamplingKind::Supplied);
    }

    #[test]
    fn the_position_checksum_moves_when_a_position_moves() {
        // §3.3: positions are derived, not stored, so the checksum is the only thing that
        // can refuse a graph rebuilt under a changed sampler.
        let field = lattice_field(20_260_904);
        let a = StreamGraph::build(
            &params(20_260_904, 0.0),
            &field.positions,
            &field.height_m,
            &field.area_m2,
            &field.neighbours,
        )
        .expect("builds");
        let mut moved = field.positions.clone();
        let last = moved[9].vector;
        moved[9] = crate::sphere::SpherePoint::from_vector(&crate::vectors::Vec3::new(
            f64::from_bits(last.x.to_bits() ^ 1),
            last.y,
            last.z,
        ))
        .expect("still a direction");
        let b = StreamGraph::build(
            &params(20_260_904, 0.0),
            &moved,
            &field.height_m,
            &field.area_m2,
            &field.neighbours,
        )
        .expect("builds");
        assert_ne!(a.header().position_checksum, b.header().position_checksum);
    }

    // ---- reserved, not populated -----------------------------------------------------

    #[test]
    fn reaches_are_reserved_and_empty_in_this_slice() {
        let graph = built(20_260_904, 0.0);
        assert!(graph.reaches().is_empty(), "slice 5 populates reaches, not slice 1p");
    }

    #[test]
    fn lake_outflow_is_reserved_at_its_sentinel() {
        let graph = built(20_260_904, 0.0);
        assert!(!graph.lakes().is_empty());
        for lake in graph.lakes() {
            assert_eq!(lake.outflow_lake, NO_LAKE, "the lake super-graph is slice 5's");
            assert!(lake.level_m.is_finite());
            assert!(graph.has_flag(lake.root_node, flag::LAKE_MEMBER));
        }
    }

    // ---- construction refuses malformed input -----------------------------------------

    #[test]
    fn build_refuses_mismatched_array_lengths() {
        let field = lattice_field(20_260_904);
        let err = StreamGraph::build(
            &params(20_260_904, 0.0),
            &field.positions,
            &field.height_m[..10],
            &field.area_m2,
            &field.neighbours,
        )
        .expect_err("a short height array must be refused");
        assert!(matches!(err, GraphError::LengthMismatch { .. }));
    }

    #[test]
    fn build_refuses_a_neighbour_out_of_range_or_a_self_neighbour() {
        let field = lattice_field(20_260_904);
        let mut bad = field.neighbours.clone();
        bad[0][0] = (ROWS * COLS) as u32; // cast-ok: a lattice extent
        let err = StreamGraph::build(
            &params(20_260_904, 0.0),
            &field.positions,
            &field.height_m,
            &field.area_m2,
            &bad,
        )
        .expect_err("an out-of-range neighbour must be refused");
        assert!(matches!(err, GraphError::NeighbourOutOfRange { .. }));

        let mut selfy = field.neighbours.clone();
        selfy[4][0] = 4;
        let err = StreamGraph::build(
            &params(20_260_904, 0.0),
            &field.positions,
            &field.height_m,
            &field.area_m2,
            &selfy,
        )
        .expect_err("a self-neighbour must be refused");
        assert!(matches!(err, GraphError::SelfNeighbour { node: 4 }));
    }

    #[test]
    fn build_refuses_a_non_finite_height_or_a_non_positive_area() {
        let field = lattice_field(20_260_904);
        let mut heights = field.height_m.clone();
        heights[2] = f64::NAN;
        let err = StreamGraph::build(
            &params(20_260_904, 0.0),
            &field.positions,
            &heights,
            &field.area_m2,
            &field.neighbours,
        )
        .expect_err("NaN height must be refused");
        assert!(matches!(err, GraphError::NonFiniteHeight { node: 2 }));

        let mut areas = field.area_m2.clone();
        areas[5] = 0.0;
        let err = StreamGraph::build(
            &params(20_260_904, 0.0),
            &field.positions,
            &field.height_m,
            &areas,
            &field.neighbours,
        )
        .expect_err("a zero area must be refused");
        assert!(matches!(err, GraphError::NonPositiveArea { node: 5 }));
    }

    /// The abort the whole-branch review found: `> 0.0` alone admits `+inf`, and
    /// `4*pi*radius_m^2` overflows to `+inf` for `radius_m` above roughly `3.78e153` --
    /// exactly what a caller reaches through `wb_erosion_run` on a world built with an
    /// enormous `radius_m` and otherwise-ordinary erosion parameters. This test does not
    /// go through a `Surface` or a real radius at all -- it plants `f64::INFINITY`
    /// directly in the area column, which is the one property that matters here (`build`
    /// cannot distinguish "an enormous sphere overflowed" from "a caller passed infinity
    /// directly"; both must be refused the same way). `f64::NAN` is checked alongside it
    /// because `area_m2[i] > 0.0` is already `false` for a NaN reading -- this test pins
    /// that the OR'd `.is_finite()` clause does not accidentally invert that for infinity
    /// while leaving NaN's existing refusal alone.
    #[test]
    fn build_refuses_an_infinite_or_nan_area() {
        let field = lattice_field(20_260_904);

        let mut areas = field.area_m2.clone();
        areas[7] = f64::INFINITY;
        let err = StreamGraph::build(
            &params(20_260_904, 0.0),
            &field.positions,
            &field.height_m,
            &areas,
            &field.neighbours,
        )
        .expect_err("an infinite area must be refused, not treated as > 0.0");
        assert!(matches!(err, GraphError::NonPositiveArea { node: 7 }));

        let mut areas = field.area_m2.clone();
        areas[9] = f64::NEG_INFINITY;
        let err = StreamGraph::build(
            &params(20_260_904, 0.0),
            &field.positions,
            &field.height_m,
            &areas,
            &field.neighbours,
        )
        .expect_err("a negative-infinite area must be refused");
        assert!(matches!(err, GraphError::NonPositiveArea { node: 9 }));

        let mut areas = field.area_m2.clone();
        areas[3] = f64::NAN;
        let err = StreamGraph::build(
            &params(20_260_904, 0.0),
            &field.positions,
            &field.height_m,
            &areas,
            &field.neighbours,
        )
        .expect_err("a NaN area must still be refused after this check gained an OR clause");
        assert!(matches!(err, GraphError::NonPositiveArea { node: 3 }));
    }

    // ---- the `drop_m > 0.0` filter, on its own ---------------------------------------

    /// The edge rule's strictness is enforced **twice** — the `drop_m > 0.0` filter, and
    /// `gradient > best_gradient` starting from `0.0` — and Task 3 recorded that relaxing
    /// the first to `>= 0.0` fails no test, because a zero drop yields a zero gradient
    /// which the second guard rejects anyway. Defence in depth, not a hole; but a guard
    /// with no test of its own is a guard nobody can remove *deliberately*.
    ///
    /// This is the one behaviour the filter has that the second guard cannot supply:
    /// **the filter runs before the coincidence check**, so a neighbour that is not
    /// strictly below is never measured, and therefore never reported as coincident. Two
    /// nodes at the same place and the same height are skipped by each other and the
    /// graph builds; the same pair at *different* heights is refused
    /// (`a_sampler_that_returns_a_duplicate_position_is_refused_by_build` covers that
    /// arm). Relaxing this filter to `>= 0.0` turns the build below into a
    /// `CoincidentNodes` refusal, which is what gives the line its own coverage.
    ///
    /// The asymmetry is a real property of the type and is recorded rather than fixed:
    /// coincident nodes are a defect the *sampler* is asserted never to produce
    /// (`sampled_nodes_keep_their_minimum_separation`), and `build` refuses them wherever
    /// they could affect an edge.
    #[test]
    fn the_drop_filter_runs_before_the_coincidence_check() {
        let positions = vec![
            crate::sphere::SpherePoint::from_latlon(10.0, 20.0),
            // Exactly node 0's position: `angle_to` is 0.0, not merely small.
            crate::sphere::SpherePoint::from_latlon(10.0, 20.0),
            crate::sphere::SpherePoint::from_latlon(-40.0, 100.0),
        ];
        assert_eq!(positions[0].angle_to(&positions[1]), 0.0);
        // Nodes 0 and 1 are coincident AND at equal height, so neither is strictly below
        // the other and the filter skips the pair before the angle is ever taken.
        let heights = [500.0f64, 500.0, -10.0];
        let areas = [1_000.0f64, 1_000.0, 1_000.0];
        let neighbours = vec![vec![1u32, 2], vec![0u32, 2], vec![0u32, 1]];
        let graph = StreamGraph::build(
            &params(20_260_904, 0.0),
            &positions,
            &heights,
            &areas,
            &neighbours,
        )
        .expect("a coincident pair at equal height is skipped, not refused");
        // Both still drain to node 2, which they are strictly above and not coincident
        // with, so the skip is of that one pair and not of the whole neighbour list.
        assert_eq!(graph.downhill_of(0), Some(2));
        assert_eq!(graph.downhill_of(1), Some(2));
        assert_eq!(graph.downhill_of(2), None);

        // The other arm: the same coincident pair at *different* heights is a refusal,
        // because now the filter passes and the angle is measured.
        let heights = [500.0f64, 400.0, -10.0];
        let err = StreamGraph::build(
            &params(20_260_904, 0.0),
            &positions,
            &heights,
            &areas,
            &neighbours,
        )
        .expect_err("a coincident pair with a real drop between them must be refused");
        assert!(matches!(err, GraphError::CoincidentNodes { node: 0, neighbour: 1 }));
    }

    /// The second half of the same strictness, kept separate so a mutation to either
    /// guard has a test that names it: a neighbour at *equal* height is never chosen, and
    /// a neighbour strictly *above* is never chosen, when the two are not coincident.
    #[test]
    fn an_equal_or_higher_neighbour_is_never_a_downhill_target() {
        let positions = vec![
            crate::sphere::SpherePoint::from_latlon(10.0, 20.0),
            crate::sphere::SpherePoint::from_latlon(10.5, 20.0),
            crate::sphere::SpherePoint::from_latlon(11.0, 20.0),
        ];
        // Node 0's only neighbours are one at exactly its height and one above it.
        let heights = [500.0f64, 500.0, 900.0];
        let areas = [1_000.0f64, 1_000.0, 1_000.0];
        let neighbours = vec![vec![1u32, 2], vec![0u32], vec![0u32]];
        let graph = StreamGraph::build(
            &params(20_260_904, 1_000.0),
            &positions,
            &heights,
            &areas,
            &neighbours,
        )
        .expect("a flat-and-uphill node set builds");
        assert_eq!(graph.downhill_of(0), None, "equal height is not descent");
        assert_eq!(graph.downhill_of(1), None, "equal height is not descent");
        assert_eq!(graph.downhill_of(2), Some(0));
    }
}
/// The measurements the sampler's constants were chosen from.
///
/// `#[ignore]`d, because they take minutes and allocate half a gigabyte — but they are
/// *tests*, not a throwaway script, so they compile on every run and cannot rot silently
/// while the constants they justify stay in the source. Run them with
///
/// ```text
/// cargo test --release --lib --no-default-features -- --ignored --nocapture
/// ```
///
/// Every figure they print names its population, its method and its parameters. The host
/// is in the report; nothing here can know it.
#[cfg(test)]
mod measurements {
    use super::*;

    const SEED: u64 = 20260904;
    const RADIUS_M: f64 = 6_371_000.0;

    /// Smallest great-circle separation over the whole node set, as a multiple of nominal
    /// spacing, plus the mean nearest-neighbour distance in metres.
    ///
    /// **Method.** Candidate pairs are `(i, i + d)` for every `d` in `neighbour_offsets`,
    /// which is the Fibonacci-convergent set the sampler itself uses; `neighbours_match_
    /// brute_force` below is what licenses that restriction. The winner is selected on the
    /// largest dot product — exact, monotone in the angle, and free of `atan2` in the inner
    /// loop — and only the winning pair's angle is then measured with `angle_to`.
    fn separation_stats(count: u32, jitter: f64) -> (f64, f64, f64) {
        let positions = node_positions_at_jitter(SEED, count, jitter);
        let offsets = neighbour_offsets(count);
        let n = positions.len();
        let mut best_dot = -2.0f64;
        let mut best_pair = (0usize, 0usize);
        let mut nearest_dot = vec![-2.0f64; n];
        for &offset in &offsets {
            let d = offset as usize; // cast-ok: an index offset into usize
            if d >= n {
                continue;
            }
            for i in 0..(n - d) {
                let j = i + d;
                let dot = positions[i].vector.dot(&positions[j].vector);
                if dot > best_dot {
                    best_dot = dot;
                    best_pair = (i, j);
                }
                if dot > nearest_dot[i] {
                    nearest_dot[i] = dot;
                }
                if dot > nearest_dot[j] {
                    nearest_dot[j] = dot;
                }
            }
        }
        let min_rad = positions[best_pair.0].angle_to(&positions[best_pair.1]);
        let spacing_rad = nominal_spacing_rad(count);
        let mut mean_nearest = 0.0f64;
        for i in 0..n {
            let clamped = if nearest_dot[i] < 1.0 { nearest_dot[i] } else { 1.0 };
            let angle = m::atan2(m::sqrt(if 1.0 - clamped * clamped > 0.0 {
                1.0 - clamped * clamped
            } else {
                0.0
            }), clamped);
            mean_nearest += angle * RADIUS_M;
        }
        mean_nearest /= n as f64; // cast-ok: a node count to f64 for a mean
        (min_rad * RADIUS_M, min_rad / spacing_rad, mean_nearest)
    }

    #[test]
    #[ignore = "minutes, and half a gigabyte at the largest size"]
    fn measure_minimum_separation() {
        println!();
        println!("min separation. seed {SEED}, radius {RADIUS_M} m, all nodes, candidate");
        println!("offsets = neighbour_offsets(count).");
        println!("{:>10} {:>7} {:>12} {:>12} {:>10} {:>12}", "n", "jitter", "spacing m",
                 "min sep m", "x nominal", "mean nn m");
        for &count in &[3_200u32, 20_000, 200_000, 1_000_000] {
            for &jitter in &[0.0f64, 0.10, 0.15, 0.20, 0.30, 0.45] {
                let (min_m, ratio, mean_nn) = separation_stats(count, jitter);
                println!(
                    "{:>10} {:>7.2} {:>12.1} {:>12.1} {:>10.4} {:>12.1}",
                    count, jitter, nominal_spacing_m(count, RADIUS_M), min_m, ratio, mean_nn
                );
            }
        }
    }

    #[test]
    #[ignore = "twenty million nodes: about half a gigabyte and a few minutes"]
    fn measure_minimum_separation_at_twenty_million() {
        println!();
        for &jitter in &[0.0f64, NODE_JITTER_FRACTION] {
            let (min_m, ratio, mean_nn) = separation_stats(20_000_000, jitter);
            println!(
                "n = 20,000,000  jitter {:.2}  spacing {:.1} m  min sep {:.1} m  \
                 {:.4} x nominal  mean nn {:.1} m",
                jitter, nominal_spacing_m(20_000_000, RADIUS_M), min_m, ratio, mean_nn
            );
        }
    }

    /// Monte-Carlo cell areas: every probe point is assigned to the node it is nearest to,
    /// and a node's area is its probe share of the sphere.
    ///
    /// **Method.** Probes are a much denser spiral at a *different* seed and a *different*
    /// jitter, so the probe set is not aligned with the node set. The node search is
    /// restricted to an index window, which is exact rather than approximate because the
    /// spiral's `z` is strictly decreasing in index: a node more than `half_window`
    /// indices away is more than the search radius away in `z` alone. The window is proved
    /// sufficient by `worst`, printed with every run — a value approaching the search
    /// radius would mean the window was too small.
    fn monte_carlo_areas(count: u32, jitter: f64, probes: u32) -> Vec<f64> {
        let positions = node_positions_at_jitter(SEED, count, jitter);
        let n = positions.len();
        let spacing = nominal_spacing_rad(count);
        // 3.5 spacings of search radius, expressed as an index window: dz = 2/count per
        // index, so a z-difference of `r` is `count * r / 2` indices.
        let radius = 3.5 * spacing;
        let window = (radius * f64::from(count) / 2.0) as usize + 2; // cast-ok: a window size
        let mut counts = vec![0u64; n];
        let mut worst = 0.0f64;
        for p in 0..probes {
            let probe = node_position_at_jitter(777_000_001, p, probes, 0.30);
            let z = probe.vector.z;
            // The inverse of `z = 1 - 2(i + 0.5)/count`, floored into an index.
            let centre_f = m::floor((1.0 - z) * f64::from(count) / 2.0);
            let centre = centre_f as i64; // cast-ok: already floored, mirrors noise.rs
            let window_i = window as i64; // cast-ok: a window size, not a float
            let lo = if centre - window_i > 0 { centre - window_i } else { 0 };
            let hi_raw = centre + (window as i64); // cast-ok: a window size
            let hi = if hi_raw < (n as i64) { hi_raw } else { n as i64 }; // cast-ok: a node count
            let mut best_dot = -2.0f64;
            let mut best = 0usize;
            let mut i = lo;
            while i < hi {
                let idx = i as usize; // cast-ok: bounded by lo >= 0 and hi <= n
                let dot = positions[idx].vector.dot(&probe.vector);
                if dot > best_dot {
                    best_dot = dot;
                    best = idx;
                }
                i += 1;
            }
            counts[best] += 1;
            let angle = positions[best].angle_to(&probe);
            if angle > worst {
                worst = angle;
            }
        }
        println!(
            "  probes {probes}, window +/-{window} indices, search radius {:.4} rad, \
             worst assigned distance {:.4} rad",
            radius, worst
        );
        assert!(worst < radius, "the index window was too small: {worst} >= {radius}");
        let sphere = 4.0 * std::f64::consts::PI * RADIUS_M * RADIUS_M;
        // cast-ok: a probe count to f64, and a per-node tally to f64
        counts.iter().map(|&c| sphere * (c as f64) / f64::from(probes)).collect()
    }

    fn spread(values: &[f64]) -> (f64, f64, f64, f64) {
        let n = values.len();
        let ideal = 4.0 * std::f64::consts::PI * RADIUS_M * RADIUS_M / (n as f64); // cast-ok
        let mut ratios: Vec<f64> = values.iter().map(|v| v / ideal).collect();
        ratios.sort_by(|a, b| a.partial_cmp(b).expect("no NaN areas"));
        let mut mean = 0.0f64;
        for r in &ratios {
            mean += r;
        }
        mean /= n as f64; // cast-ok: a node count to f64 for a mean
        let mut var = 0.0f64;
        for r in &ratios {
            var += (r - mean) * (r - mean);
        }
        var /= n as f64; // cast-ok: a node count to f64 for a variance
        (m::sqrt(var) / mean, ratios[0], ratios[n / 2], ratios[n - 1])
    }

    #[test]
    #[ignore = "millions of nearest-node queries"]
    fn measure_cell_area_spread() {
        println!();
        let count = 5_000u32;
        let probes = 4_000_000u32;
        println!(
            "cell area spread. nodes n = {count} at seed {SEED}; probes = a spiral of \
             {probes} points at seed 777000001, jitter 0.30, so the two sets are not \
             aligned. Ratios are to the ideal cell 4*pi*R^2/n."
        );
        // **The probe set is quasi-uniform, not random, so the binomial noise floor does
        // not apply and must not be subtracted.** A binomial argument would put the floor
        // at sqrt(count/probes) = 3.5%; the jitter-0.00 row below measures the whole
        // apparatus at CV 0.5%, an order of magnitude under that, which is evidence that
        // the low-discrepancy probe set beats the binomial estimate rather than evidence
        // that the measurement is biased. The 0.00 row is therefore the measurement's own
        // noise reference, and every other row is read against it.
        for &jitter in &[0.0f64, 0.15, 0.30, 0.45] {
            let areas = monte_carlo_areas(count, jitter, probes);
            let (cv, p00, p50, p100) = spread(&areas);
            println!(
                "  jitter {jitter:.2}: CV {cv:.4} min {p00:.3} median {p50:.3} max {p100:.3}"
            );
        }
    }

    #[test]
    #[ignore = "millions of nearest-node queries"]
    fn measure_area_estimator_error() {
        println!();
        let count = 20_000u32;
        let probes = 8_000_000u32;
        let jitter = NODE_JITTER_FRACTION;
        println!(
            "area_m2 estimator vs Monte-Carlo. n = {count}, seed {SEED}, jitter {jitter:.2}, \
             {probes} probes (seed 777000001, jitter 0.30)."
        );
        println!(
            "  the shipped estimator uses the nearest {AREA_NEIGHBOUR_COUNT} of the",
        );
        println!(
            "  {NEIGHBOUR_COUNT} stored neighbours; the variant rows below chose that k."
        );
        let truth = monte_carlo_areas(count, jitter, probes);
        let positions = node_positions_at_jitter(SEED, count, jitter);
        let neighbours = node_neighbours(&positions, NEIGHBOUR_COUNT);
        variant_sweep(&positions, &neighbours, &truth);
        let estimate = node_areas_m2(&positions, &neighbours, RADIUS_M);
        let n = count as usize; // cast-ok: a node count into usize
        let mut rms = 0.0f64;
        let mut worst = 0.0f64;
        let (mut mt, mut me) = (0.0f64, 0.0f64);
        for i in 0..n {
            let rel = (estimate[i] - truth[i]) / truth[i];
            rms += rel * rel;
            if rel.abs() > worst {
                worst = rel.abs();
            }
            mt += truth[i];
            me += estimate[i];
        }
        rms = m::sqrt(rms / (n as f64)); // cast-ok: a node count to f64
        mt /= n as f64; // cast-ok: a node count to f64
        me /= n as f64; // cast-ok: a node count to f64
        let (mut cov, mut vt, mut ve) = (0.0f64, 0.0f64, 0.0f64);
        for i in 0..n {
            cov += (truth[i] - mt) * (estimate[i] - me);
            vt += (truth[i] - mt) * (truth[i] - mt);
            ve += (estimate[i] - me) * (estimate[i] - me);
        }
        let r = cov / m::sqrt(vt * ve);
        let (cv_t, _, _, _) = spread(&truth);
        let (cv_e, p00, p50, p100) = spread(&estimate);
        println!("  Monte-Carlo CV {cv_t:.4}; estimator CV {cv_e:.4}");
        println!("  estimator ratios: min {p00:.3} median {p50:.3} max {p100:.3}");
        println!("  RMS relative error {rms:.4}, worst {worst:.4}, correlation r = {r:.4}");
    }


    /// A scratch comparison of candidate `area_m2` estimators against the same Monte-Carlo
    /// truth, so the one in `node_areas_m2` is a choice rather than the first thing tried.
    fn variant_sweep(positions: &[SpherePoint], neighbours: &[Vec<u32>], truth: &[f64]) {
        let n = positions.len();
        let sphere = 4.0 * std::f64::consts::PI * RADIUS_M * RADIUS_M;
        let labels = ["k=2", "k=3", "k=4", "k=5", "k=6", "k=7", "k=8"];
        for (v, label) in labels.iter().enumerate() {
            let mut w = Vec::with_capacity(n);
            let mut total = 0.0f64;
            for i in 0..n {
                let take = v + 2;
                let mut sum = 0.0f64;
                let mut used = 0.0f64;
                for &j in neighbours[i].iter().take(take) {
                    let a = positions[i].angle_to(&positions[j as usize]); // cast-ok: index
                    sum += a;
                    used += 1.0;
                }
                let mean = sum / used;
                let x = mean * mean;
                w.push(x);
                total += x;
            }
            let mut rms = 0.0f64;
            let (mut me, mut mt) = (0.0f64, 0.0f64);
            let est: Vec<f64> = w.iter().map(|x| sphere * x / total).collect();
            for i in 0..n {
                let rel = (est[i] - truth[i]) / truth[i];
                rms += rel * rel;
                me += est[i];
                mt += truth[i];
            }
            rms = m::sqrt(rms / (n as f64)); // cast-ok: a node count to f64
            me /= n as f64; // cast-ok: a node count to f64
            mt /= n as f64; // cast-ok: a node count to f64
            let (mut cov, mut vt, mut ve) = (0.0f64, 0.0f64, 0.0f64);
            for i in 0..n {
                cov += (truth[i] - mt) * (est[i] - me);
                vt += (truth[i] - mt) * (truth[i] - mt);
                ve += (est[i] - me) * (est[i] - me);
            }
            println!("  variant {}: RMS {:.4} r {:.4}", label, rms, cov / m::sqrt(vt * ve));
        }
    }

    /// Every node's k nearest, by the offset candidate set, against every node's k nearest
    /// by comparing it to all the others. This is what makes the offset set an argument
    /// rather than a hope.
    #[test]
    #[ignore = "O(n^2) at 20,000 nodes"]
    fn neighbours_match_brute_force() {
        println!();
        for &count in &[512u32, 3_200, 20_000] {
            for &jitter in &[0.0f64, NODE_JITTER_FRACTION, 0.30] {
                let positions = node_positions_at_jitter(SEED, count, jitter);
                let offsets = neighbour_offsets(count);
                let n = positions.len();
                let mut mismatched = 0usize;
                let mut mismatched_six = 0usize;
                for i in 0..n {
                    let node = i as u32; // cast-ok: a test node index
                    let fast =
                        nearest_neighbours(&positions, &offsets, node, NEIGHBOUR_COUNT);
                    let mut all: Vec<(u64, u32)> = Vec::with_capacity(n - 1);
                    for j in 0..n {
                        if j == i {
                            continue;
                        }
                        let n = j as u32; // cast-ok: a node index within the slice
                        all.push((positions[i].angle_to(&positions[j]).to_bits(), n));
                    }
                    all.sort_unstable();
                    let slow: Vec<u32> =
                        all.into_iter().take(NEIGHBOUR_COUNT).map(|(_, j)| j).collect();
                    if fast != slow {
                        mismatched += 1;
                    }
                    if fast[..6] != slow[..6] {
                        mismatched_six += 1;
                    }
                }
                println!(
                    "  n = {}, jitter {:.2}: {} of {} differ over k = {}, {} over 6",
                    count, jitter, mismatched, n, NEIGHBOUR_COUNT, mismatched_six
                );
                if jitter == NODE_JITTER_FRACTION {
                    assert_eq!(mismatched, 0, "the candidate set missed a true neighbour");
                }
            }
        }
    }
}
/// The sampler's properties, at sizes a default `cargo test` can afford.
///
/// **There is no Python oracle for any of this** — the node sampler is new in this slice
/// and has nothing to be conformant with. So every test here asserts a *property*, and
/// each one was confirmed to fail against a deliberately wrong sampler before it was
/// believed. The wrong versions are not deleted: the ones that can be expressed as data
/// rather than as an edit are kept below, under `deliberately_wrong`.
#[cfg(test)]
mod sampling_tests {
    use super::*;

    const SEED: u64 = 20260904;
    const RADIUS_M: f64 = 6_371_000.0;

    /// The smallest separation in a node set, as a multiple of nominal spacing.
    ///
    /// Shared by the guard tests and by `deliberately_wrong`, so that the thing asserting
    /// the property and the thing proving the assertion can fail are the same code.
    fn min_separation_ratio(positions: &[SpherePoint]) -> f64 {
        let count = positions.len();
        let offsets = neighbour_offsets(count as u32); // cast-ok: a test's node count
        let mut best_dot = -2.0f64;
        let mut best = (0usize, 0usize);
        for &offset in &offsets {
            let d = offset as usize; // cast-ok: an index offset into usize
            if d >= count {
                continue;
            }
            for i in 0..(count - d) {
                let dot = positions[i].vector.dot(&positions[i + d].vector);
                if dot > best_dot {
                    best_dot = dot;
                    best = (i, i + d);
                }
            }
        }
        let angle = positions[best.0].angle_to(&positions[best.1]);
        angle / nominal_spacing_rad(count as u32) // cast-ok: a test's node count
    }

    // ---- the spiral itself ------------------------------------------------------------

    #[test]
    fn nominal_spacing_is_the_side_of_an_equal_area_patch() {
        for &count in &[2u32, 100, 3_200, 1_000_000, 20_000_000] {
            let s = nominal_spacing_rad(count);
            let covered = s * s * f64::from(count);
            let sphere = 4.0 * std::f64::consts::PI;
            assert!(
                (covered - sphere).abs() < 1e-9 * sphere,
                "count {count}: {covered} patches do not cover {sphere}"
            );
        }
    }

    #[test]
    fn the_spiral_has_no_pole_singularity_and_no_seam() {
        // §14.1's stated reason for this representation. Every point is a unit vector, no
        // point is NaN, and the two extreme indices are the two poles rather than a pinch.
        for &count in &[3u32, 101, 5_000] {
            for index in 0..count {
                let v = spiral_point(index, count).vector;
                let length = v.length();
                assert!((length - 1.0).abs() < 1e-12, "count {count} index {index}: {length}");
            }
            let first = spiral_point(0, count).vector.z;
            let last = spiral_point(count - 1, count).vector.z;
            assert!(first > 0.0 && last < 0.0, "the ends are not the two hemispheres");
        }
    }

    #[test]
    fn the_spiral_z_is_strictly_decreasing_in_index() {
        // Not decoration: the Monte-Carlo measurement inverts it to build an index window,
        // and `neighbour_offsets` is only meaningful because index is a latitude order.
        let count = 4_000u32;
        for index in 1..count {
            let above = spiral_point(index - 1, count).vector.z;
            let here = spiral_point(index, count).vector.z;
            assert!(here < above, "index {index}: {here} is not below {above}");
        }
    }

    // ---- the jitter -------------------------------------------------------------------

    #[test]
    fn the_jitter_is_actually_wired_to_something() {
        // A jitter multiplied by a constant that happens to be zero, or hashed on a key
        // that ignores the index, would leave a perfect spiral and every separation test
        // below would still pass. This is the test that would not.
        let count = 1_000u32;
        let mut moved = 0;
        for index in 0..count {
            let plain = spiral_point(index, count);
            let jittered = node_position(SEED, index, count);
            if plain.angle_to(&jittered) > 0.0 {
                moved += 1;
            }
        }
        assert_eq!(moved, count, "some nodes were not nudged at all");
    }

    #[test]
    fn the_jitter_never_exceeds_the_bound_the_separation_floor_is_derived_from() {
        // The floor `0.872 - 2*sqrt(2)*J` is only a floor if no node moves further than
        // `atan(J*sqrt(2))`. Asserting the premise, not just the conclusion.
        let count = 5_000u32;
        let jitter = NODE_JITTER_FRACTION * nominal_spacing_rad(count);
        let bound = m::atan2(jitter * m::sqrt(2.0), 1.0);
        let mut worst = 0.0f64;
        for index in 0..count {
            let moved = spiral_point(index, count).angle_to(&node_position(SEED, index, count));
            if moved > worst {
                worst = moved;
            }
        }
        assert!(worst <= bound, "a node moved {worst}, past the bound {bound}");
        // And the bound is not vacuous: something got most of the way to it.
        assert!(worst > 0.5 * bound, "nothing approached the bound; is the jitter alive?");
    }

    #[test]
    fn the_two_tangent_draws_are_independent() {
        // Same salt for both would nudge every node along one diagonal, which no
        // separation or area test would notice.
        assert_ne!(JITTER_EAST_SALT, JITTER_NORTH_SALT);
        let count = 2_000u32;
        let mut same = 0;
        for index in 0..count {
            let a = node_fraction(SEED, index, JITTER_EAST_SALT);
            let b = node_fraction(SEED, index, JITTER_NORTH_SALT);
            if (a - b).abs() < 1e-12 {
                same += 1;
            }
        }
        assert_eq!(same, 0, "{same} nodes drew the same number twice");
    }

    // ---- determinism ------------------------------------------------------------------

    #[test]
    fn a_node_does_not_depend_on_how_many_were_sampled_before_it() {
        // The property `generation.rs`'s module doc is written to protect, restated for
        // nodes: index-addressed, never sequential.
        let count = 777u32;
        let all = node_positions(SEED, count);
        for &index in &[0u32, 1, 5, 100, 500, 776] {
            let alone = node_position(SEED, index, count);
            let i = index as usize; // cast-ok: a test's node index
            assert_eq!(alone.vector.x.to_bits(), all[i].vector.x.to_bits());
            assert_eq!(alone.vector.y.to_bits(), all[i].vector.y.to_bits());
            assert_eq!(alone.vector.z.to_bits(), all[i].vector.z.to_bits());
        }
    }

    #[test]
    fn resampling_is_bit_identical_and_reseeding_is_not() {
        let count = 500u32;
        let a = node_positions(SEED, count);
        let b = node_positions(SEED, count);
        let c = node_positions(SEED + 1, count);
        let mut differed = 0;
        for i in 0..a.len() {
            assert_eq!(a[i].vector.z.to_bits(), b[i].vector.z.to_bits(), "node {i} moved");
            if a[i].angle_to(&c[i]) > 0.0 {
                differed += 1;
            }
        }
        assert_eq!(differed, a.len(), "a different seed left some node exactly where it was");
    }

    #[test]
    fn the_node_sampler_is_not_the_plate_sampler() {
        // Different hash, deliberately (`node_hash` is an avalanche; `generation::fraction`
        // is a BLAKE2b digest of a formatted string). If someone "tidied" one into the
        // other, nodes and plates would share a jitter pattern and this would catch it.
        let count = 22u32;
        let mut same = 0;
        for index in 0..count {
            let node = node_position(SEED, index, count);
            let seed = SEED as i64; // cast-ok: this test's seed into the plate sampler's type
            let plate = crate::generation::spread(seed, index as usize, count as usize);
            if node.angle_to(&plate) < 1e-12 {
                same += 1;
            }
        }
        assert_eq!(same, 0, "the node sampler reproduced the plate sampler");
    }

    // ---- separation -------------------------------------------------------------------

    #[test]
    fn the_separation_floor_clears_the_asserted_minimum_by_arithmetic() {
        // Not a sample: the worst case the jitter box permits, evaluated.
        let floor = SPIRAL_MIN_SEPARATION_FRACTION - 2.0 * m::sqrt(2.0) * NODE_JITTER_FRACTION;
        assert!(
            floor > MIN_SEPARATION_FRACTION,
            "the proved floor {floor} is not above the asserted {MIN_SEPARATION_FRACTION}"
        );
    }

    #[test]
    fn sampled_nodes_keep_their_minimum_separation() {
        for &count in &[512u32, 3_200] {
            let ratio = min_separation_ratio(&node_positions(SEED, count));
            assert!(
                ratio >= MIN_SEPARATION_FRACTION,
                "count {count}: min separation {ratio} x nominal, below {MIN_SEPARATION_FRACTION}"
            );
        }
    }

    #[test]
    fn the_unjittered_spiral_matches_the_constant_the_floor_is_built_on() {
        // `SPIRAL_MIN_SEPARATION_FRACTION` is a measurement pinned as a constant, and a
        // constant nobody re-checks is a constant that drifts away from its measurement.
        for &count in &[512u32, 3_200, 20_000] {
            let positions: Vec<SpherePoint> =
                (0..count).map(|i| spiral_point(i, count)).collect();
            let ratio = min_separation_ratio(&positions);
            assert!(
                (ratio - SPIRAL_MIN_SEPARATION_FRACTION).abs() < 0.001,
                "count {count}: the un-jittered spiral measured {ratio}"
            );
        }
    }

    #[test]
    fn the_offset_candidate_set_agrees_with_brute_force_on_a_sample() {
        // `neighbour_offsets` is an argument about the golden ratio's convergents, and an
        // argument is not a measurement. The full sweep — every node, three jitters, up to
        // 20,000 nodes — is `measurements::neighbours_match_brute_force`; this is the
        // affordable spot check that runs on every `cargo test`, so a shrunken offset set
        // cannot reach a commit.
        let count = 4_000u32;
        let positions = node_positions(SEED, count);
        let offsets = neighbour_offsets(count);
        let mut checked = 0;
        let mut index = 0u32;
        while index < count {
            let fast = nearest_neighbours(&positions, &offsets, index, NEIGHBOUR_COUNT);
            let i = index as usize; // cast-ok: a test's node index
            let mut all: Vec<(u64, u32)> = Vec::with_capacity(positions.len());
            for j in 0..positions.len() {
                if j != i {
                    let n = j as u32; // cast-ok: a node index within the slice
                    all.push((positions[i].angle_to(&positions[j]).to_bits(), n));
                }
            }
            all.sort_unstable();
            let slow: Vec<u32> =
                all.into_iter().take(NEIGHBOUR_COUNT).map(|(_, j)| j).collect();
            assert_eq!(fast, slow, "node {index}'s neighbours are not its nearest");
            checked += 1;
            index += 37;
        }
        assert!(checked > 100, "only {checked} nodes were checked");
    }

    #[test]
    fn an_exact_tie_between_two_neighbours_goes_to_the_lower_index() {
        // The `(angle_bits, index)` ordering's second element does nothing on a sampled
        // planet — two distinct nodes are never at bit-identical angles, and a mutation
        // that dropped the index from the sort key changed no neighbour list anywhere.
        // It is insurance, and insurance nobody has ever claimed on is indistinguishable
        // from decoration. This constructs the tie the sampler will not.
        let centre = SpherePoint::from_latlon(0.0, 0.0);
        let east = SpherePoint::from_latlon(0.0, 1.0);
        let west = SpherePoint::from_latlon(0.0, -1.0);
        assert_eq!(
            centre.angle_to(&east).to_bits(),
            centre.angle_to(&west).to_bits(),
            "the fixture does not actually tie"
        );
        let positions = vec![centre, east, west];
        // Offsets in descending order, so the tying candidates are *pushed* highest-index
        // first. A sort keyed on the angle alone would then leave node 2 in front, which is
        // exactly the mutation this test exists to catch.
        let nearest = nearest_neighbours(&positions, &[2, 1], 0, 1);
        assert_eq!(nearest, vec![1], "the tie did not go to the lower index");
    }

    // ---- areas ------------------------------------------------------------------------

    #[test]
    fn the_areas_sum_to_the_whole_planet() {
        let count = 2_000u32;
        let sampling = sample_nodes(SEED, count, RADIUS_M).expect("a sampleable count");
        let mut total = 0.0f64;
        for a in &sampling.area_m2 {
            total += a;
        }
        let sphere = 4.0 * std::f64::consts::PI * RADIUS_M * RADIUS_M;
        assert!(
            (total - sphere).abs() < 1e-9 * sphere,
            "the cells sum to {total}, not {sphere}"
        );
    }

    #[test]
    fn every_area_is_positive_because_build_refuses_anything_else() {
        let sampling = sample_nodes(SEED, 1_000, RADIUS_M).expect("a sampleable count");
        for (i, a) in sampling.area_m2.iter().enumerate() {
            assert!(a.is_finite() && *a > 0.0, "node {i} has area {a}");
        }
    }

    #[test]
    fn the_areas_vary_which_is_the_entire_reason_the_field_exists() {
        // §3.2: if these were all equal, a shared constant would have been the right type
        // and every stored drainage area would mean something different.
        let count = 2_000u32;
        let sampling = sample_nodes(SEED, count, RADIUS_M).expect("a sampleable count");
        let ideal = 4.0 * std::f64::consts::PI * RADIUS_M * RADIUS_M / f64::from(count);
        let (mut lo, mut hi) = (f64::INFINITY, 0.0f64);
        for a in &sampling.area_m2 {
            if *a < lo {
                lo = *a;
            }
            if *a > hi {
                hi = *a;
            }
        }
        assert!(lo / ideal < 0.95, "the smallest cell is {} x ideal", lo / ideal);
        assert!(hi / ideal > 1.05, "the largest cell is {} x ideal", hi / ideal);
    }

    #[test]
    fn the_area_estimate_reproduces_the_spread_it_was_calibrated_against() {
        // The Monte-Carlo truth at jitter 0.15 has CV 0.0753 (n = 20,000 nodes,
        // 8,000,000 probes). This estimator's own CV is a function of
        // `AREA_NEIGHBOUR_COUNT` and nothing else: 0.0844 at k = 4, **0.0718 at k = 5**,
        // 0.0635 at k = 6, 0.0506 at k = 8, at n = 2,000. Only k = 5 lands in the band,
        // so this test pins the swept choice without needing four million probe points.
        let count = 2_000u32;
        let sampling = sample_nodes(SEED, count, RADIUS_M).expect("a sampleable count");
        let n = f64::from(count);
        let mut mean = 0.0f64;
        for a in &sampling.area_m2 {
            mean += a;
        }
        mean /= n;
        let mut var = 0.0f64;
        for a in &sampling.area_m2 {
            var += (a - mean) * (a - mean);
        }
        let cv = m::sqrt(var / n) / mean;
        assert!(
            cv > 0.066 && cv < 0.080,
            "area CV {cv} at k = {AREA_NEIGHBOUR_COUNT}; the true spread is 0.0753"
        );
    }

    // ---- the whole thing, into `build` -------------------------------------------------

    /// A stand-in height field. Not `Surface`: this module must not depend on it, and a
    /// sampler test that needed a planet's worth of tectonics to run would not be run.
    fn stand_in_height_m(point: &SpherePoint) -> f64 {
        let v = &point.vector;
        3_000.0 * (v.x * 3.0 + v.y * 5.0 + v.z * 7.0)
            + 900.0 * (v.x * v.y * 11.0 - v.z * v.z * 13.0)
    }

    fn build_from_sampling(count: u32) -> Result<StreamGraph, GraphError> {
        let sampling = sample_nodes(SEED, count, RADIUS_M).expect("a sampleable count");
        let heights: Vec<f64> = sampling.positions.iter().map(stand_in_height_m).collect();
        let params = BuildParams {
            world_seed: SEED,
            radius_m: RADIUS_M,
            sea_level_m: 0.0,
            sampling_kind: SamplingKind::Spiral,
            // Stated by this test, not defaulted by the type. See
            // `pond_max_drainage_area_m2` on `BuildParams`.
            pond_max_drainage_area_m2: 4.0e9,
        };
        StreamGraph::build(
            &params,
            &sampling.positions,
            &heights,
            &sampling.area_m2,
            &sampling.neighbours,
        )
    }

    #[test]
    fn a_sampled_node_set_builds_a_graph_that_validates() {
        let graph = build_from_sampling(4_000).expect("the sampler feeds build");
        assert_eq!(graph.node_count(), 4_000);
        assert_eq!(graph.header().sampling_kind, SamplingKind::Spiral);
        graph.validate().expect("a built graph satisfies its own invariants");
        let peel = graph.peel();
        assert_eq!(peel.peeled, 4_000, "the downhill relation is not a forest");
        assert!(!graph.roots().is_empty(), "a planet with no roots is not a planet");
    }

    #[test]
    fn the_graphs_total_drainage_is_the_planet() {
        // Every node's own area enters exactly one root's total, so the roots' drainage
        // must sum to the sphere. This is the end-to-end check that the areas, the
        // accumulation and the forest all agree.
        let graph = build_from_sampling(3_000).expect("the sampler feeds build");
        let mut total = 0.0f64;
        for root in graph.roots() {
            total += graph.drainage_area_m2(root);
        }
        let sphere = 4.0 * std::f64::consts::PI * RADIUS_M * RADIUS_M;
        assert!(
            (total - sphere).abs() < 1e-6 * sphere,
            "the roots drain {total}, the planet is {sphere}"
        );
    }

    #[test]
    fn two_builds_of_one_seed_are_bit_identical() {
        let a = build_from_sampling(1_500).expect("built");
        let b = build_from_sampling(1_500).expect("built");
        assert!(a.bit_identical_to(&b), "the same seed produced two different graphs");
    }

    #[test]
    fn a_one_node_planet_is_declined_rather_than_degenerated() {
        assert!(sample_nodes(SEED, 0, RADIUS_M).is_none());
        assert!(sample_nodes(SEED, 1, RADIUS_M).is_none());
        assert!(sample_nodes(SEED, 2, RADIUS_M).is_some());
    }

    // ---- the threshold nobody has measured ---------------------------------------------

    /// `pond_max_drainage_area_m2` is required-with-no-default, and **this slice did not
    /// measure it either**.
    ///
    /// It cannot be measured here, and the reason is structural rather than a shortage of
    /// time. §13.2's distinction is between a pond and a lake — a *water body* — and the
    /// size of a water body is the area its surface covers at the level it fills to. Slice
    /// 1p has no fill algorithm: `Lake::level_m` is set to the root node's own elevation,
    /// an empty basin, and `LAKE_MEMBER` is set on the root alone. So the only quantity
    /// this slice can offer is the *drainage area arriving at a root*, which is the
    /// catchment feeding the basin and not the basin. A small mountain tarn at the head of
    /// a large steep catchment and a broad shallow lake at the head of a small one are on
    /// opposite sides of §13.2 and can have the same root drainage area.
    ///
    /// A number derived from the stand-in noise field would be worse than no number: it
    /// would be a measurement of the stand-in, it would look measured, and per the note on
    /// `BuildParams` it is exactly how an unmeasured number becomes a permanent one. So it
    /// stays a required parameter, and slice 5 — which owns the fill — calibrates it.
    ///
    /// What *can* be pinned now is that it has no default, and this is that pin: `Default`
    /// must not be implemented for `BuildParams`, or a caller acquires the number by
    /// omission.
    #[test]
    fn build_params_has_no_default_so_the_pond_threshold_must_be_stated() {
        use std::marker::PhantomData;

        // The inherent method wins over the blanket trait method when, and only when, the
        // `Default` bound is satisfied — so this reports whether the impl exists.
        struct Probe<T>(PhantomData<T>);
        trait NoDefault {
            fn has_default() -> bool {
                false
            }
        }
        impl<T> NoDefault for Probe<T> {}
        impl<T: Default> Probe<T> {
            fn has_default() -> bool {
                true
            }
        }

        assert!(Probe::<f64>::has_default(), "the probe cannot see a Default that is there");
        assert!(
            !Probe::<BuildParams>::has_default(),
            "BuildParams gained a Default, and with it a pond threshold nobody chose"
        );
    }

    // ---- the wrong implementations ----------------------------------------------------

    /// Each of these is a sampler that is wrong in a way somebody would plausibly write,
    /// paired with the check above that catches it. A property test nobody has watched
    /// fail is a property test that might be asserting nothing.
    mod deliberately_wrong {
        use super::*;

        #[test]
        fn inheriting_generations_absolute_jitter_collapses_the_node_set() {
            // The exact mistake the brief names: `JITTER_RAD` is 0.18 **radians**, sized
            // for 22 plates. Expressed as a fraction of node spacing it is enormous, and
            // the separation guard must not survive it.
            let count = 3_200u32;
            let as_fraction = crate::generation::JITTER_RAD / nominal_spacing_rad(count);
            assert!(as_fraction > 2.0, "the inherited constant is {as_fraction} x spacing");
            let positions = node_positions_at_jitter(SEED, count, as_fraction);
            let ratio = min_separation_ratio(&positions);
            assert!(
                ratio < MIN_SEPARATION_FRACTION,
                "the guard did not notice a {as_fraction} x spacing jitter: ratio {ratio}"
            );
        }

        #[test]
        fn the_jitter_the_extraction_warned_about_breaks_the_separation_claim() {
            // 0.45 x spacing: the setting that put two nodes 47.8 m apart on a 22.6 km
            // lattice. At 3,200 nodes it still *looks* survivable, which is the point —
            // the guard is a ratio, so it catches it at a size a test can afford.
            let positions = node_positions_at_jitter(SEED, 3_200, 0.45);
            let ratio = min_separation_ratio(&positions);
            assert!(ratio < MIN_SEPARATION_FRACTION, "0.45 x spacing measured {ratio}");
        }

        #[test]
        fn a_sampler_that_forgot_to_jitter_fails_the_liveness_check() {
            // The mirror image: a jitter wired to zero passes every separation and area
            // test and is still wrong. `the_jitter_is_actually_wired_to_something` is the
            // one that fails, and here it is failing.
            //
            // The comparison is a threshold rather than an equality, and the reason is a
            // real property of the code: `node_position` ends in `SpherePoint::from_vector`,
            // which **normalises**, while `spiral_point` uses the non-normalising
            // constructor. Dividing an already-unit vector by its own length of 1 +/- one
            // ulp changes bits, so 137 of these 1,000 nodes move by about 1e-16 rad at zero
            // jitter. `generation::pole` carries a comment about exactly this. A live
            // jitter moves nodes by 1e-3 rad — thirteen orders of magnitude more — so the
            // two cases are in no danger of being confused.
            let count = 1_000u32;
            let positions = node_positions_at_jitter(SEED, count, 0.0);
            let mut worst = 0.0f64;
            for index in 0..count {
                let i = index as usize; // cast-ok: a test's node index
                let moved = spiral_point(index, count).angle_to(&positions[i]);
                if moved > worst {
                    worst = moved;
                }
            }
            assert!(worst < 1e-12, "a zero jitter moved a node by {worst} rad");
            let live = node_position(SEED, 500, count);
            let alive = spiral_point(500, count).angle_to(&live);
            assert!(alive > 1e-6, "the live sampler only moved node 500 by {alive} rad");
        }

        #[test]
        fn a_sampler_that_returns_a_duplicate_position_is_refused_by_build() {
            // `StreamGraph::build` refuses coincident nodes rather than resolving them, so
            // the sampler's whole separation story is what stands between a world and a
            // refusal. This proves the refusal is real and reachable from sampled data.
            let count = 200u32;
            let sampling = sample_nodes(SEED, count, RADIUS_M).expect("a sampleable count");
            let mut positions = sampling.positions.clone();
            let victim = sampling.neighbours[0][0] as usize; // cast-ok: a node index
            positions[victim] = positions[0];
            let mut heights: Vec<f64> =
                positions.iter().map(super::stand_in_height_m).collect();
            // The coincidence check only runs on a pair with a genuine drop between them.
            heights[0] = 9_000.0;
            heights[victim] = 0.0;
            let params = BuildParams {
                world_seed: SEED,
                radius_m: RADIUS_M,
                sea_level_m: -20_000.0,
                sampling_kind: SamplingKind::Spiral,
                pond_max_drainage_area_m2: 4.0e9,
            };
            let result = StreamGraph::build(
                &params,
                &positions,
                &heights,
                &sampling.area_m2,
                &sampling.neighbours,
            );
            let victim32 = victim as u32; // cast-ok: a node index into the error
            let expected = GraphError::CoincidentNodes { node: 0, neighbour: victim32 };
            match result {
                Err(actual) => assert_eq!(actual, expected),
                Ok(_) => panic!("build accepted two coincident nodes"),
            }
        }

        #[test]
        fn a_constant_area_is_measurably_wrong_and_the_variation_test_says_so() {
            // If `area_m2` were the shared constant §3.2 refuses, the smallest and largest
            // cells would both be exactly the ideal. Here is that sampler, failing.
            let count = 2_000u32;
            let ideal = 4.0 * std::f64::consts::PI * RADIUS_M * RADIUS_M / f64::from(count);
            let constant = vec![ideal; count as usize]; // cast-ok: a test's node count
            let (mut lo, mut hi) = (f64::INFINITY, 0.0f64);
            for a in &constant {
                if *a < lo {
                    lo = *a;
                }
                if *a > hi {
                    hi = *a;
                }
            }
            assert!(lo / ideal >= 0.95 && hi / ideal <= 1.05, "the stand-in was not constant");
            // ... whereas the real one varies by more than the test's threshold.
            let sampling = sample_nodes(SEED, count, RADIUS_M).expect("a sampleable count");
            let (mut rlo, mut rhi) = (f64::INFINITY, 0.0f64);
            for a in &sampling.area_m2 {
                if *a < rlo {
                    rlo = *a;
                }
                if *a > rhi {
                    rhi = *a;
                }
            }
            assert!(rlo / ideal < 0.95 && rhi / ideal > 1.05);
        }

        #[test]
        fn an_unnormalised_area_estimate_does_not_sum_to_the_planet() {
            // The raw `d^2` weights are the natural thing to write and are wrong by a
            // factor that depends on the node count, which is exactly the sort of error
            // that looks like a plausible planet.
            let count = 1_000u32;
            let positions = node_positions(SEED, count);
            let neighbours = node_neighbours(&positions, NEIGHBOUR_COUNT);
            let mut raw_total = 0.0f64;
            for i in 0..positions.len() {
                let mut sum = 0.0f64;
                for &j in neighbours[i].iter().take(AREA_NEIGHBOUR_COUNT) {
                    sum += positions[i].angle_to(&positions[j as usize]); // cast-ok: index
                }
                let mean = sum / (AREA_NEIGHBOUR_COUNT as f64); // cast-ok: a fixed k to f64
                raw_total += mean * mean * RADIUS_M * RADIUS_M;
            }
            let sphere = 4.0 * std::f64::consts::PI * RADIUS_M * RADIUS_M;
            assert!(
                (raw_total - sphere).abs() > 0.05 * sphere,
                "the unnormalised weights happened to sum to the sphere: {raw_total}"
            );
            // And the shipped one does sum to it.
            let areas = node_areas_m2(&positions, &neighbours, RADIUS_M);
            let mut total = 0.0f64;
            for a in &areas {
                total += a;
            }
            assert!((total - sphere).abs() < 1e-9 * sphere);
        }

        #[test]
        fn the_two_ends_of_the_spiral_are_not_each_other_s_neighbours() {
            // **A correction to what this test first claimed.** It was written to assert
            // that a *wrapping* offset search (`(i + d) % count`) would make index 0 and
            // index count-1 adjacent. Mutating `nearest_neighbours` to wrap and re-running
            // showed it does not: those two nodes are the spiral's poles, nearly antipodal,
            // and selection is by measured distance, so a wrapping search would merely add
            // candidates that never win. Not wrapping is therefore a **cost** decision, not
            // a correctness one, and the comment in `nearest_neighbours` says so.
            //
            // What is left is the property that is actually true and actually worth
            // holding: whatever the candidate set, no node's neighbour list may contain a
            // node on the far side of the planet.
            let count = 1_000u32;
            let positions = node_positions(SEED, count);
            let last = (count - 1) as usize; // cast-ok: a test's node index
            let across = positions[0].angle_to(&positions[last]);
            assert!(across > 3.0, "the ends of the spiral are only {across} rad apart");
            let offsets = neighbour_offsets(count);
            let reach = 3.0 * nominal_spacing_rad(count);
            let mut index = 0u32;
            while index < count {
                let i = index as usize; // cast-ok: a test's node index
                for &j in &nearest_neighbours(&positions, &offsets, index, NEIGHBOUR_COUNT) {
                    // cast-ok: a node index bounded by the slice length
                    let d = positions[i].angle_to(&positions[j as usize]);
                    assert!(d < reach, "node {index} calls node {j} a neighbour at {d} rad");
                }
                index += 7;
            }
        }

        #[test]
        fn a_spiral_without_the_half_offset_puts_a_node_exactly_on_the_pole() {
            // `z = 1 - 2*(i + 0.5)/n`, not `1 - 2*i/n`. Dropping the half puts index 0 at
            // z = 1 exactly, where the east vector is degenerate and the jitter has no
            // defined direction — the pole singularity §14.1 says this representation does
            // not have.
            let count = 1_000u32;
            let wrong_z = 1.0 - 2.0 * 0.0 / f64::from(count);
            assert_eq!(wrong_z.to_bits(), 1.0f64.to_bits());
            let right_z = spiral_point(0, count).vector.z;
            assert!(right_z < 1.0, "index 0 sits on the pole at z = {right_z}");
            let ring = m::sqrt(1.0 - right_z * right_z);
            assert!(ring > DEGENERATE, "the east direction is degenerate: ring {ring}");
        }

        #[test]
        fn a_neighbour_list_of_consecutive_indices_is_not_a_neighbour_list() {
            // The obvious wrong answer: "the nodes near i are i-4..i+4". On a Fibonacci
            // spiral those are strung along the spiral arm, not around the node, and the
            // area estimate built on them would be a measurement of the arm.
            let count = 4_000u32;
            let positions = node_positions(SEED, count);
            let offsets = neighbour_offsets(count);
            let mid = count / 2;
            let real = nearest_neighbours(&positions, &offsets, mid, NEIGHBOUR_COUNT);
            let naive: Vec<u32> = (1..=4).flat_map(|d| [mid - d, mid + d]).collect();
            let mut shared = 0;
            for n in &naive {
                if real.contains(n) {
                    shared += 1;
                }
            }
            assert!(shared < naive.len(), "the naive list was the real one after all");
            let i = mid as usize; // cast-ok: a test's node index
            let far = naive
                .iter()
                // cast-ok: a test's node index
                .map(|&j| positions[i].angle_to(&positions[j as usize]))
                .fold(0.0f64, |acc, a| if a > acc { a } else { acc });
            let near = real
                .iter()
                // cast-ok: a test's node index
                .map(|&j| positions[i].angle_to(&positions[j as usize]))
                .fold(0.0f64, |acc, a| if a > acc { a } else { acc });
            assert!(far > near, "the consecutive indices were not further away");
        }
    }
}
