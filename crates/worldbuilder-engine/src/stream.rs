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
//!   file. §8.4 measures the discrepancy at 0.735x to 1.285x the ideal cell at the
//!   recommended jitter, so the constant is not even approximately right.
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

use crate::sphere::SpherePoint;

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

fn position_checksum(positions: &[SpherePoint]) -> u64 {
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
            if !(area_m2[i] > 0.0) {
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
    /// peeled completely — 20,000,000 of 20,000,000 at the largest size tried (§8.3) — and
    /// this turns that measurement into something the code cannot quietly lose.
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
        if self.height_m.len() != other.height_m.len() {
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

    /// A linear scan, deliberately: roots grow sub-linearly in node count — 300 at 10 k
    /// nodes and 5,647 at 20 M, a 19x rise for a 2,000x rise in nodes (§8.3) — so the lake
    /// table is small in absolute terms and a spatial index would be a structure to keep
    /// consistent for no measured gain.
    pub fn lake_at(&self, node: u32) -> Option<&Lake> {
        self.lakes.iter().find(|lake| lake.root_node == node)
    }

    pub fn reaches(&self) -> &[Reach] {
        &self.reaches
    }
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
}
