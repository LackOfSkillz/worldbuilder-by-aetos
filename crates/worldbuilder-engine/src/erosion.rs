//! Uplift and erodibility as explicit, recorded inputs to the erosion bake.
//!
//! This module implements no solver yet -- that is Tasks 2 through 4 of the same slice.
//! What it settles is the *interface* every one of those tasks consumes: how `u` (uplift)
//! and `k` (erodibility) enter the equation
//!
//! ```text
//! dh/dt = u - k * A^m * s^n        with n = 1, m = 0.5
//! ```
//!
//! (docs/design/2026-09-02-mark-2-world-studio.md §14.1, Cordonnier et al. 2016), and how
//! that entry is recorded so a baked graph can say which inputs produced it.
//!
//! # A run is reproducible from its recorded parameters and nothing else
//!
//! [`ErosionParams`] exists because two bakes that differ in uplift or erodibility are not
//! comparable, and a stored graph has to say which it was baked with. Every value the
//! solver's answer depends on belongs in this type -- a constant that instead lives as a
//! literal inside the solver is a hidden input: two runs that differ only in it look
//! identical in the record. `StreamGraph::build`'s `BuildParams` sets the house rule for
//! this already: `pond_max_drainage_area_m2` is a required field with no default, "so that
//! nobody inherits a number nobody chose" (`stream.rs`). `ErosionParams` follows it --
//! `#[derive(Default)]` is deliberately absent, and every field is populated by the caller.
//!
//! [`STREAM_POWER_AREA_EXPONENT`] and [`STREAM_POWER_SLOPE_EXPONENT`] are the opposite
//! case: the paper fixes `m = 0.5` and `n = 1`, so those are spec constants with their
//! source cited here, not fields on `ErosionParams` and not bare literals scattered through
//! the solver. A caller cannot tune them because the equation being solved is only the
//! stream power equation when they hold; a different exponent is a different equation, not
//! a calibration of this one.
//!
//! # Uniform for now, and what that forecloses
//!
//! §14.4 lists both `u` and `k` as **recomputable** from parameters rather than fields that
//! must be stored per node, so storing them on every node (and growing every serialised
//! graph by two `f64`s per node for values a formula can regenerate) is not required. This
//! task takes the smallest legitimate first step: **uniform values, identical at every
//! node.** [`ErosionParams::uplift_at`] and [`ErosionParams::erodibility_at`] already take a
//! node index, so a caller (and every later task) already calls them per node -- but today
//! both ignore the index and return the same scalar for the whole planet.
//!
//! **This is a real limitation, not a placeholder that is already finished.** A uniform
//! uplift field has no tectonic signal in it: a planet with an active collision margin on
//! one side and a stable craton on the other erodes as if both were rising at the same
//! rate, because nothing here reads `plates.rs`, `tectonics.rs` or the node's position to
//! find out otherwise. The two methods keep the per-node call shape so that a later task can
//! make `u` and `k` vary with a plate margin, a rock type from `substrate.rs`, or a stored
//! per-node field, without changing the signature every earlier caller already uses. Making
//! that variation real is not this task's job and is not done by writing the shape down.
//!
//! # The traversal direction the solver needs is the opposite of what `peel()` returns
//!
//! `StreamGraph::peel()` documents its own order as "a topological order from leaves to
//! roots" -- exactly what drainage-area accumulation needs, since a node's contribution to
//! its receiver can only be added once every one of *its own* contributors has been folded
//! in.
//!
//! §14.3 says the implicit solver instead walks "the stream trees from root to leaves",
//! because each node's height update reads its receiver's *already-updated* height -- a
//! receiver has to be resolved before the node that drains into it, which is the reverse
//! dependency of the accumulation pass. **The solver therefore consumes `peel().order`
//! reversed, not a new traversal.** `peel()` is deterministic by construction (its ready
//! queue is seeded in ascending index order and consumed FIFO), and that determinism is
//! exactly what makes reversing it -- rather than writing a second walk -- safe to rely on
//! for a reproducible bake. Getting this backwards produces a graph that still looks like
//! terrain, because every node still gets *some* update; it is simply the wrong one, and
//! nothing about the output announces that.
//!
//! # Bit-exact, but not because the inner loop avoids transcendentals
//!
//! **An earlier draft of this doc claimed this module could hold the strict bit-for-bit
//! contract because its exponents avoid transcendentals. Wrong on both halves -- see the
//! plan's `cea453f` correction, which this section now matches.**
//!
//! With `n = 1`, `s^1` is `s` -- no call at all. With `m = 0.5`, `A^0.5` is `sqrt`, which
//! IEEE-754 requires to be *correctly rounded*. That is real and worth keeping: it means
//! `detmath::powf` never enters the inner loop of a 100-300 iteration bake. **But slope
//! needs the distance from a node to its receiver**, and
//! [`crate::sphere::SpherePoint::distance_to`] is `angle_to * radius_m` with
//! `angle_to = detmath::atan2(across, along)` -- so the step this module implements calls
//! `atan2` after all, once per node per step. That is expected and correct here, not a
//! violation to route around.
//!
//! The strict-versus-bounded split this project otherwise cares about governs
//! *Python-against-Rust* conformance, and it does not apply to this module at all:
//! `worldbuilder/` has no stream power and this plan forbids writing one, so there is no
//! Python side to diverge from. The only equality claims a test here can make are **native
//! against WASM** and **run against run** -- and both hold bit-for-bit regardless of
//! `atan2`, because native and WASM share the same pure-Rust `libm` crate rather than each
//! dispatching to its own platform's. A test in this module therefore still compares **bit
//! patterns, never a tolerance** -- not because the loop is transcendental-free, but because
//! both sides of every comparison this module can make run the identical `libm`.
//!
//! Distances are computed **once per step, before the walk**, via [`receiver_distances_m`],
//! rather than once per node per iteration inside a caller's loop: Task 3's iteration calls
//! it once and reuses the result across all 100-300 iterations, instead of recomputing
//! `atan2` for the same node/receiver pair on every one of them. Positions themselves are
//! not stored on the graph (`GraphHeader` keeps only a `position_checksum`); a caller
//! regenerates them once with `stream::node_positions` and passes them in.
//!
//! If a later task finds itself reaching for `powf` in the inner loop, the formulation has
//! drifted away from `n = 1, m = 0.5` -- that is the moment to come back and change this
//! doc, not to add a tolerance and move on. `atan2` in the distance computation is not that
//! signal; it was always expected here.
//!
//! # A root has no receiver, so it is held fixed rather than uplifted
//!
//! `StreamGraph::downhill_of` returns `None` exactly at a root, and `stream.rs`'s own
//! invariant (`RootIsNeitherMouthNorLake` / `RootIsBothMouthAndLake`) says every root is
//! either a mouth (the sea) or a pond/lake outlet -- in either case, the local **base
//! level** its whole basin erodes toward, not a slope with a downhill neighbour to measure.
//! Uplifting a root anyway would raise that base level independently of anything eroding
//! into it, which is not what "base level" means: the land upstream would spend the whole
//! bake chasing a floor that never stopped rising under it. This module instead holds a
//! root's height fixed for the step ([`erode_step`] copies it through unchanged). Applying
//! `u * dt` at roots instead is the other defensible choice -- it is a different equation,
//! not a bug in this one, and a later task that wants a rising sea floor or a filling lake
//! should change this decision explicitly rather than inherit it by accident.

use std::sync::OnceLock;

use crate::detmath;
use crate::sphere::SpherePoint;
use crate::stream::StreamGraph;

/// The area exponent `m` in `dh/dt = u - k * A^m * s^n`.
///
/// Fixed by Cordonnier et al. (2016) and by
/// `docs/design/2026-09-02-mark-2-world-studio.md` §14.1 ("n = 1, m = 0.5"). A spec
/// constant, not a tunable: solving the equation with a different exponent is solving a
/// different equation, so this is named and cited rather than left as a literal `0.5`
/// wherever the solver needs it.
///
/// **This constant documents the spec's value; it is not a switch.** `implicit_receiver_update`
/// hardcodes `detmath::sqrt(area_m2)` for `A^0.5` rather than reading this constant, because
/// `sqrt` is a distinct, cheaper, correctly-rounded operation and not `powf` called with an
/// argument -- there is no generic `A^m` in the inner loop to parameterise. Editing this
/// constant's value therefore changes nothing except `the_stream_power_exponents_match_the_spec`,
/// which exists to fail loudly the moment that becomes true, rather than let a wrong sense of
/// "increased granularity" imply that increasing this constant changes the solver's behaviour.
/// If `A^m` is ever generalised past `m = 0.5`, that is the moment this constant becomes a real
/// input to `powf` and this note should be deleted, not the moment to add a tolerance.
pub const STREAM_POWER_AREA_EXPONENT: f64 = 0.5;

/// The slope exponent `n` in `dh/dt = u - k * A^m * s^n`.
///
/// Fixed at `1` by the same source as [`STREAM_POWER_AREA_EXPONENT`]. At this value `s^n`
/// is `s` itself -- multiplication, not a call. **This buys a cost win, not a conformance
/// upgrade**: it means `detmath::powf` never enters the inner loop of a 100-300 iteration
/// bake. It does not make this module's inner loop transcendental-free -- `erode_step`
/// still calls `detmath::atan2` by way of `SpherePoint::distance_to` (see the module doc's
/// "Bit-exact, but not because the inner loop avoids transcendentals" section) -- and the
/// strict/bounded contract split this project otherwise cares about governs
/// Python-against-Rust conformance, which does not apply here at all since erosion has no
/// Python counterpart. An earlier version of this doc claimed the opposite ("the strict
/// bit-for-bit contract... not the bounded one") and was corrected in `cea453f`; this
/// paragraph must keep agreeing with the module doc above, not repeat the retracted claim.
///
/// **Like [`STREAM_POWER_AREA_EXPONENT`], this constant documents the spec's value; it is
/// not a switch.** `implicit_receiver_update` computes slope as a bare `(h_i - h_r) / d`,
/// which *is* `s^1` -- there is no `powf(s, n)` call anywhere for this to parameterise, so
/// editing this constant changes nothing except the pin test that checks it still equals
/// `1.0`. Wiring it through a real `powf(s, STREAM_POWER_SLOPE_EXPONENT)` would put a
/// transcendental in the inner loop for a case that is always `n = 1` today -- a real cost
/// paid for a cosmetic generality -- so this stays a documented, pinned value rather than a
/// parameter threaded through the solver.
pub const STREAM_POWER_SLOPE_EXPONENT: f64 = 1.0;

/// Everything the stream power equation needs beyond the graph itself, and everything a
/// bake must record for its answer to be reproducible.
///
/// `Copy`, `Debug`, `PartialEq` to match `stream.rs::BuildParams`, so a test (or a caller
/// deciding whether two bakes are comparable) can compare two parameter sets directly.
/// No `Default`: every field is something a caller chose, not something inherited silently.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ErosionParams {
    /// `u`, the tectonic uplift rate, in metres per year, applied uniformly across the
    /// whole graph (see the module doc's "Uniform for now" section). Positive raises the
    /// land; the equation subtracts erosion from it, so a node with no drainage above it
    /// still rises at this rate.
    pub uplift_m_per_yr: f64,

    /// `k`, erodibility, in inverse years. Dimensional check: `A^0.5` (`A` in m^2)
    /// contributes a length in metres, `s` is dimensionless (rise over run), so for
    /// `k * A^0.5 * s` to be an erosion rate in metres per year, `k` itself carries units of
    /// `1/yr`. Uniform across the graph, for the same reason and with the same limitation as
    /// `uplift_m_per_yr`.
    pub erodibility_per_yr: f64,

    /// `dt`, the timestep of one implicit step, in years. Task 1's review flagged the
    /// timestep as a value the result unambiguously depends on: two bakes that differ only
    /// in `dt` erode by different amounts and are not comparable runs, so `dt` belongs on
    /// this recorded, `PartialEq`-comparable type rather than living as a solver constant or
    /// a bare argument that a stored graph's header says nothing about. Named with its unit
    /// like its siblings, for the same reason they are.
    pub timestep_yr: f64,

    /// The convergence criterion for [`erode_to_convergence`]'s loop, in metres: the
    /// **maximum absolute per-node height change produced by one `erode_step` call**, over
    /// every node in the graph. A run stops the moment this falls to or below the
    /// threshold. Named for what it measures rather than "tolerance" -- a run that stops at
    /// a different threshold ends at different heights, so this is exactly the kind of
    /// value Task 1's `dt` review already flagged: a hidden input if it lived as a module
    /// constant instead of a recorded field.
    ///
    /// A loose threshold makes "converges quickly" true by construction (see
    /// `erode_to_convergence`'s doc and this module's convergence-sweep binary for the
    /// measured consequence of tightening it), so choose it deliberately rather than by
    /// habit.
    ///
    /// **This is a bound on the rate of change, not on the distance from steady state.**
    /// With `c` the dimensionless number `erode_step`'s doc names, this threshold and the
    /// actual remaining residual differ by roughly a factor of `1/c` -- see
    /// `erode_to_convergence`'s "`threshold` is a per-step bound, not a distance from the
    /// fixed point" section for the derivation and a worked example at this crate's own
    /// test constants.
    pub max_height_change_per_step_m: f64,

    /// The iteration cap for [`erode_to_convergence`]. A run that reaches this many steps
    /// without satisfying `max_height_change_per_step_m` has **not converged** --
    /// [`ErosionRun::NotConverged`] says so explicitly rather than returning a plain
    /// `Vec<f64>` indistinguishable from a converged answer, which is this project's
    /// signature defect (see the module doc). Two runs that differ only in this cap can
    /// stop at different heights, so it is recorded here rather than passed as a loose loop
    /// bound.
    pub max_iterations: u32,
}

impl ErosionParams {
    /// `u` at a node. Uniform today -- every node receives `uplift_m_per_yr` regardless of
    /// `node` -- but the per-node signature is the seam a later task uses to source uplift
    /// from something spatial (a plate margin, a stored field) without moving every caller.
    /// See the module doc for what uniform uplift forecloses.
    pub fn uplift_at(&self, _node: u32) -> f64 {
        self.uplift_m_per_yr
    }

    /// `k` at a node. Uniform today, for the same reason and with the same limitation as
    /// [`ErosionParams::uplift_at`].
    pub fn erodibility_at(&self, _node: u32) -> f64 {
        self.erodibility_per_yr
    }
}

/// `peel().order` reversed: root-to-leaves, the direction the implicit solver walks so that
/// a node's receiver is already updated when the node itself is processed (see the module
/// doc's traversal-direction section). A thin, deliberately trivial wrapper -- **not a
/// second traversal** -- so every later task reaches for this name instead of writing
/// `graph.peel().order.iter().rev()` at each call site and one of them getting it backwards.
pub fn root_to_leaves(graph: &StreamGraph) -> Vec<u32> {
    let mut order = graph.peel().order;
    order.reverse();
    order
}

/// The great-circle distance from every node to its downhill receiver, in metres --
/// `distances[node]` is `positions[node].distance_to(&positions[receiver], radius_m)`.
///
/// Computed as a single pass over the whole graph, **once**, so [`erode_step`] (and Task
/// 3's loop over it) never recomputes `atan2` for a node/receiver pair that has not moved:
/// positions are fixed for the whole bake, only heights change per step. A root has no
/// receiver, so its entry is `0.0` -- a placeholder that [`erode_step`]'s `None` branch
/// structurally never indexes into, since a root's update does not consult distance at
/// all (see the module doc). `0.0` was kept rather than `f64::NAN` -- which would fail
/// louder if some future caller ever did read it, at the cost of turning today's `is_finite`
/// check into something that would need to special-case roots -- because the read really is
/// unreachable today: making it loud is a Task 3 concern, for whichever task first starts
/// holding this vector across an iteration loop and so first has an opportunity to misuse it.
///
/// `positions` must be indexed exactly like the graph, i.e. `stream::node_positions` called
/// with the same seed and count the graph itself was built from; this function trusts that
/// rather than re-deriving it, the same way `StreamGraph::build` trusts the positions it is
/// handed.
pub fn receiver_distances_m(graph: &StreamGraph, positions: &[SpherePoint]) -> Vec<f64> {
    debug_assert_eq!(
        positions.len(),
        graph.node_count() as usize, // cast-ok: node_count is bounded (stream.rs::MAX_NODES)
        "positions must be exactly as long as the graph has nodes"
    );
    let radius_m = graph.header().radius_m;
    (0..graph.node_count())
        .map(|node| match graph.downhill_of(node) {
            Some(receiver) => {
                let from = &positions[node as usize]; // cast-ok: a node index into usize
                let to = &positions[receiver as usize]; // cast-ok: a node index into usize
                from.distance_to(to, radius_m)
            }
            None => 0.0, // a root: never read by erode_step, see the module doc
        })
        .collect()
}

/// One implicit step of `dh/dt = u - k * A^0.5 * s` over the whole graph, walking
/// [`root_to_leaves`] so every node's receiver is already at its new height when the node
/// itself is updated.
///
/// # Derivation
///
/// Slope at node `i` with receiver `r` and receiver-distance `d` is `s = (h_i - h_r) / d`.
/// Discretising implicitly -- the receiver height on the right-hand side is the *new* one,
/// not the old -- gives
///
/// ```text
/// (h_i' - h_i) / dt = u - k * sqrt(A_i) * (h_i' - h_r') / d
/// ```
///
/// Multiply through by `dt`, expand, and collect every `h_i'` term on the left. With
/// `c = k * dt * sqrt(A_i) / d`:
///
/// ```text
/// h_i' - h_i = u*dt - c*h_i' + c*h_r'
/// h_i' * (1 + c) = h_i + u*dt + c*h_r'
/// h_i' = (h_i + u*dt + c*h_r') / (1 + c)
/// ```
///
/// `h_r'` is the receiver's already-updated height, which only exists at this point in the
/// walk because the walk is root-to-leaves.
///
/// # `c` is the dimensionless number this whole method actually responds to
///
/// `c = k * dt * sqrt(A_i) / d` above is not an intermediate convenience -- it is the single
/// dimensionless group that governs everything about how one step (and therefore a whole
/// [`erode_to_convergence`] run) behaves. The closed form says a node moves a fraction
/// `c / (1 + c)` of the way from its old height toward `(h_i + u*dt + c*h_r') / c` each step
/// -- so `c` alone sets the per-step relaxation rate, and the number of steps needed to reach
/// a given per-step-change threshold scales as `ln(1/threshold) / c` (see
/// [`erode_to_convergence`]'s doc). `u`, `k`, `d`, and `A` each varying independently would
/// suggest four separate knobs; naming `c` says there is really only one that the *count*
/// depends on, and the other three enter only through this ratio. **A comparison against a
/// different `k`/`dt`, a different mesh spacing, or a different published result is not
/// like-for-like unless `c` is comparable too** -- see `erode_to_convergence`'s doc and
/// `src/bin/erosion_convergence_sweep.rs`'s corrected verdict for what happens when that
/// comparison is made without checking `c` first.
///
/// A root has no `r`, `s`, or `d` at all -- see the module doc for why this function holds
/// a root's height fixed rather than applying `u * dt` to it.
///
/// `heights[node]` is `h` before the step; the returned vector is `h'` after it, same
/// length and same indexing. `distances_m` is [`receiver_distances_m`]'s output for this
/// graph; passing it in rather than recomputing it here is what keeps `atan2` out of a
/// caller's per-iteration cost (see the module doc).
///
/// `d` can only be zero if two nodes coincide, which `StreamGraph::build` already rejects
/// (`GraphError::CoincidentNodes`) before a graph exists to call this on -- so this function
/// adds no division guard. A guard that silently substituted a value for `d = 0` would hide
/// exactly the defect the builder already refuses to let through; the well-formed
/// precondition is enforced upstream, once, rather than defended against redundantly here.
pub fn erode_step(graph: &StreamGraph, heights: &[f64], distances_m: &[f64], params: &ErosionParams) -> Vec<f64> {
    let count = graph.node_count() as usize; // cast-ok: node_count is bounded (stream.rs::MAX_NODES)
    // `assert_eq!`, not `debug_assert_eq!`: the bake runs in release, which is the one build
    // a debug assertion does not reach. The two failure modes are not symmetric. A slice that
    // is too SHORT panics anyway on the first out-of-range index, loudly and safely. A slice
    // that is too LONG never trips a bounds check at all -- `next` is cloned from `heights`,
    // so the extra entries are carried through untouched and returned, and the caller gets an
    // array longer than the graph with a silent tail nobody wrote. That is a quiet
    // wrong-length result, which is the failure this project keeps finding.
    //
    // These are two comparisons per STEP, not per node, so the release-time cost is nil
    // against a 100-300 iteration bake over millions of nodes.
    assert_eq!(heights.len(), count, "heights must be exactly as long as the graph has nodes");
    assert_eq!(
        distances_m.len(),
        count,
        "distances_m must be exactly as long as the graph has nodes"
    );
    let mut next = heights.to_vec();
    let dt = params.timestep_yr;

    for node in root_to_leaves(graph) {
        let idx = node as usize; // cast-ok: a node index into usize
        debug_assert!(idx < count, "root_to_leaves must only yield indices within the graph");
        let uplift = params.uplift_at(node);

        match graph.downhill_of(node) {
            None => {
                // A root: held fixed for the step, not uplifted. See the module doc's
                // "A root has no receiver" section for why.
                next[idx] = heights[idx];
            }
            Some(receiver) => {
                let receiver_idx = receiver as usize; // cast-ok: a node index into usize
                // `next[receiver_idx]` is that node's NEW height, not the old one -- sound
                // only because root_to_leaves visits every receiver before the node that
                // drains into it, so by the time this line runs the receiver has already
                // been overwritten in this same pass.
                let receiver_h_new = next[receiver_idx];
                next[idx] = implicit_receiver_update(
                    heights[idx],
                    receiver_h_new,
                    graph.drainage_area_m2(node),
                    distances_m[idx],
                    uplift,
                    params.erodibility_at(node),
                    dt,
                );
            }
        }
    }

    next
}

/// The thermal-erosion slope cap: `tan(30 degrees)`, computed once and cached, cited to
/// §14.1 ("A thermal-erosion correction caps slopes at 30 degrees, to stop the equation
/// growing spikes where drainage area is small.").
///
/// **The comparison this is for is on slope (`rise/run`, dimensionless -- the same `s` in
/// `erode_step`'s `s = (h_i - h_r) / d`), not on an angle.** Comparing a per-edge angle
/// against 30 degrees would need `detmath::atan` (or `atan2`) once per edge per iteration,
/// on top of the `atan2` [`receiver_distances_m`] already pays once per node per bake --
/// for a bake that already runs 100-300 iterations over millions of nodes, that is a real
/// cost paid for nothing: converting `s` to an angle and back buys no additional
/// information a direct comparison against `tan(30 degrees)` doesn't already have.
///
/// **`OnceLock`, not a `const fn`.** `detmath::tan` is not `const` -- it is not even
/// `f64::tan`, since this crate's whole `detmath` module exists so that every
/// transcendental call is routed through `libm` instead of the platform's own math library
/// (see that module's doc: native and WASM disagree on `f64::sin` in 2,441 of 100,000
/// samples at a single bit each) -- so this value cannot be a compile-time constant the way
/// [`STREAM_POWER_AREA_EXPONENT`] is. `bindings.rs` already has the precedent for "compute a
/// derived value once, lazily, and reuse it" (`OnceLock<ContinentalityCache>`,
/// `OnceLock<SurfaceCache>`), which is what "one constant, computed once" (this task's
/// brief) means in a language where `tan` is not `const`. This module's own
/// `slope_cap_tan_is_computed_once_and_matches_tan_30_degrees` test pins the numeric value
/// this evaluates to, so a future change to `detmath::tan` (or to the 30-degree figure)
/// fails loudly here rather than silently drifting.
pub fn slope_cap_tan() -> f64 {
    static SLOPE_CAP_TAN: OnceLock<f64> = OnceLock::new();
    *SLOPE_CAP_TAN.get_or_init(|| detmath::tan(detmath::to_radians(30.0)))
}

/// Corrects every edge whose slope exceeds [`slope_cap_tan`] by lowering the **upstream**
/// (higher) node to `receiver_height + cap * distance` -- never by raising the downstream
/// (lower) one. See "Which node moves" below for why that direction, not the other.
///
/// # Traversal direction: the same reason `erode_step` walks root-to-leaves
///
/// This walks [`root_to_leaves`], exactly like [`erode_step`]: a node's correction reads its
/// receiver's height as `next[receiver_idx]`, which is the receiver's **already-corrected**
/// value only because root-to-leaves visits every receiver before the node draining into it
/// (see the module doc's "traversal direction" section, which `erode_step` already relies on
/// for the same reason). This matters here specifically because lowering a node changes what
/// "the receiver's height" means for every node still upstream of it in the same tree --
/// walking `peel().order` (leaves-to-roots) instead would let an upstream node compare
/// itself against a receiver height that has not been corrected yet, silently
/// under-correcting depending on visit order rather than depending on the actual slope.
///
/// # Which node moves: the lower one is never raised
///
/// A slope violation between an upstream node and its receiver can be fixed two ways --
/// lower the upstream node to `receiver + cap * d`, or raise the receiver to
/// `upstream - cap * d`. This function does only the first. Raising the receiver instead
/// would be a different physical claim: per the module doc's "held fixed" section, a root is
/// the local base level its whole basin erodes toward, and raising it to satisfy an upstream
/// slope constraint would mean this correction -- not the uplift/erosion balance
/// `erode_to_convergence` actually models -- is doing the work of raising land. Lowering the
/// upstream node instead matches what "thermal erosion" describes physically: material moves
/// downslope away from a point that got too steep; the valley floor does not rise to meet
/// it. `a_capped_field_never_raises_any_node` (below) asserts this directly, comparing every
/// node's post-cap height against its pre-cap height.
///
/// # Never `f64::min`
///
/// The corrected height is computed as `if slope > cap { receiver_h + cap * d } else { raw_h }`
/// -- an explicit branch, the same house form `plates.rs::margin_at` and this module's own
/// `max_abs_height_change` use, never `raw_h.min(receiver_h + cap * d)`. `f64::min` is
/// NaN-asymmetric (`x.min(NaN)` and `NaN.min(x)` disagree), which matters concretely here: a
/// NaN `raw_h` run through `.min()` could silently resolve to either the poisoned value or
/// the finite cap depending on argument order. The explicit branch instead lets a NaN
/// `raw_h` fail the `>` comparison (NaN compares `false` against everything) and fall
/// through to `else raw_h` unchanged -- so a NaN height stays exactly NaN rather than being
/// silently replaced by a finite, cap-derived number. That is the property
/// `erode_to_convergence`'s NaN assertion (see [`max_abs_height_change`]'s doc) depends on
/// this function not routing around: `the_cap_does_not_launder_a_nan_height` (below) checks
/// it directly.
///
/// A root has no receiver, no slope, and nothing to check -- `next[idx]` already carries
/// `heights[idx]` through unchanged from the initial `to_vec()`, exactly like `erode_step`'s
/// own root branch.
pub fn cap_slopes(graph: &StreamGraph, heights: &[f64], distances_m: &[f64]) -> Vec<f64> {
    let count = graph.node_count() as usize; // cast-ok: node_count is bounded (stream.rs::MAX_NODES)
    assert_eq!(heights.len(), count, "heights must be exactly as long as the graph has nodes");
    assert_eq!(
        distances_m.len(),
        count,
        "distances_m must be exactly as long as the graph has nodes"
    );
    let cap = slope_cap_tan();
    let mut next = heights.to_vec();

    for node in root_to_leaves(graph) {
        let idx = node as usize; // cast-ok: a node index into usize
        if let Some(receiver) = graph.downhill_of(node) {
            let receiver_idx = receiver as usize; // cast-ok: a node index into usize
            // Already corrected by this same pass -- sound only because root_to_leaves
            // visits every receiver before the node that drains into it, exactly the
            // precondition erode_step's own receiver read relies on.
            let receiver_h_new = next[receiver_idx];
            let raw_h = heights[idx];
            let d = distances_m[idx];
            // Compared as a RISE (`raw_h - receiver_h_new` against `cap * d`), not as a
            // slope (dividing by `d` first): the two are mathematically the same
            // inequality for `d > 0` (guaranteed for every non-root node -- see
            // `erode_step`'s own "d can only be zero if two nodes coincide" note), but
            // comparing rises means the branch below reads `receiver_h_new + cap * d`
            // straight off the same `cap * d` product used in the check, with no division
            // anywhere in this loop at all -- cheaper than divide-then-reconstruct, and it
            // sidesteps the ULP-level rounding a caller would hit by inverting a division
            // to re-derive a slope afterward (this module's own tests measure that
            // separately; see `slope_cap_violations`'s doc).
            let cap_rise = cap * d;
            next[idx] = if (raw_h - receiver_h_new) > cap_rise { receiver_h_new + cap_rise } else { raw_h };
        }
        // else: a root, held through unchanged -- see this function's doc.
    }

    next
}

/// How many nodes differ, by exact bit pattern, between `before` and `after` --
/// specifically meant for a `before`/`after` pair straddling one [`cap_slopes`] call, where
/// it counts exactly the edges that call corrected.
///
/// **Bit comparison, not `!=`, and not "moved by more than epsilon":** a node `cap_slopes`
/// left alone carries its input value through the `else` branch with no arithmetic
/// performed on it at all (see that function's doc), so an uncorrected node is bit-identical
/// to its input, never merely close. Comparing bits therefore draws an exact line between
/// "this node was touched" and "this node was not," with no threshold to tune and no risk of
/// a real, tiny correction being missed by too loose a numeric tolerance. It also handles a
/// NaN input correctly where `!=` would not: `NaN != NaN` is `true` in IEEE-754, so a NaN
/// carried through unchanged would be miscounted as "clamped" by a naive `!=` scan; comparing
/// `to_bits()` instead sees the identical bit pattern and correctly reports no change.
///
/// This exists for measurement, not correctness -- see [`erode_to_convergence_with_clamp_counts`]
/// and `src/bin/erosion_convergence_sweep.rs`, which uses it to report how many edges the
/// cap actually corrected on a given run rather than leaving that to be inferred from an
/// iteration count that could move for either "the cap never fired" or "the cap fired and it
/// didn't matter" -- two very different findings that look identical from the outside if
/// nobody counts.
pub fn slope_cap_clamped_count(before: &[f64], after: &[f64]) -> usize {
    debug_assert_eq!(before.len(), after.len(), "clamp count must compare same-length height arrays");
    before.iter().zip(after.iter()).filter(|(b, a)| b.to_bits() != a.to_bits()).count()
}

/// The outcome of [`erode_to_convergence`]. Deliberately **not** a bare `Vec<f64>`: this
/// project has already shipped checks that counted zero work as success and fingerprints
/// that could not notice a change (see the module doc and `erode_to_convergence`'s own
/// doc), and a run that hit [`ErosionParams::max_iterations`] without converging returning
/// something that *looks* like a converged answer would be exactly that defect again. A
/// caller must match on this enum to get at the heights at all, so "did this converge" is
/// not something a caller can forget to check.
///
/// Both variants carry the same two things -- `heights` (the state after the last step run,
/// converged or not) and `iterations` (how many `erode_step` calls it took to get there) --
/// because Task 6 records a measured iteration count, and if that count only existed for
/// the success path a caller building that record for a non-converged run would have to
/// re-derive it by some other route that could disagree with this one.
#[derive(Debug, Clone, PartialEq)]
pub enum ErosionRun {
    /// The maximum per-node height change fell to or below
    /// [`ErosionParams::max_height_change_per_step_m`] after this many `erode_step` calls.
    Converged { heights: Vec<f64>, iterations: u32 },
    /// [`ErosionParams::max_iterations`] steps ran and the change was still above
    /// threshold on the last one. `heights` is the state after that last step -- **not** a
    /// converged answer, and a caller that unwraps this variant's heights and uses them as
    /// though it were is exactly the mistake this enum exists to make impossible to make
    /// silently.
    NotConverged { heights: Vec<f64>, iterations: u32 },
}

/// How much work [`cap_slopes`] actually did over a whole [`erode_to_convergence_with_clamp_counts`]
/// run -- summed across every iteration, not just the last one (by the time a run has
/// converged or hit its cap, the *last* iteration's own field is by definition already
/// compliant, so counting only that iteration would report zero regardless of how much
/// correction happened earlier in the run).
///
/// **Why this exists at all:** wiring the cap into the loop (this task's chosen ordering,
/// see `erode_to_convergence`'s doc) does not by itself prove the cap ever does anything at
/// any given `(u, k, dt)` -- a run whose slopes never approach 30 degrees converges to
/// bit-identical heights whether or not `cap_slopes` is even called. A count of zero and a
/// count that was never taken look the same from the outside; this type exists so the
/// distinction is a number in the record rather than an inference from an unchanged
/// iteration count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClampStats {
    /// The number of (node, iteration) pairs `cap_slopes` corrected, summed over every
    /// iteration this run executed. Not "distinct nodes corrected at least once" -- a node
    /// pinned at the cap for 50 consecutive iterations because its uncorrected slope keeps
    /// re-exceeding it counts 50 times here, which is the true amount of correction work
    /// done, not the size of some final set.
    pub total_edges_clamped: u64,
    /// How many of this run's iterations corrected at least one edge. Together with
    /// `total_edges_clamped` this distinguishes "the cap fired occasionally, briefly" from
    /// "the cap fired on every single iteration" at the same total count.
    pub iterations_with_a_clamp: u32,
}

/// The maximum absolute per-node height change between two same-length height arrays --
/// the exact quantity [`ErosionParams::max_height_change_per_step_m`] is a threshold on.
/// Never `f64::max`/`.max()`: both are NaN-asymmetric (`plates.rs::margin_at` is the house
/// precedent for avoiding them), so this folds with an explicit `if b > a` the way
/// `plates.rs`'s own largest-single-step-change check does. `.abs()` is exempt from the
/// no-std-math ban -- it is exact, not a transcendental.
///
/// **NaN-poisoning, deliberately in the OPPOSITE direction from `plates.rs`'s own fold.**
/// `plates.rs::margin_at`'s max exists to find the largest of values that are never expected
/// to be NaN, so `NaN > largest` silently losing to a finite `largest` is harmless there. Here
/// a NaN height change is not noise to discard -- one NaN node poisoning the whole max down to
/// `0.0` is exactly how a broken run would get reported as [`ErosionRun::Converged`], which is
/// this project's signature defect arriving through this fold instead of through the cap. So
/// this fold treats `change.is_nan()` as an explicit win over `largest`: one NaN anywhere in
/// the comparison makes the whole result `NaN`, which [`erode_to_convergence`] then asserts
/// against rather than silently comparing `NaN <= threshold` (always `false`, which would have
/// looked like ordinary non-convergence -- itself still not the loud failure a poisoned run
/// deserves).
fn max_abs_height_change(before: &[f64], after: &[f64]) -> f64 {
    debug_assert_eq!(before.len(), after.len(), "convergence check must compare same-length height arrays");
    before
        .iter()
        .zip(after.iter())
        .map(|(a, b)| (b - a).abs())
        .fold(0.0f64, |largest, change| if change > largest || change.is_nan() { change } else { largest })
}

/// Iterate [`erode_step`] until the maximum per-node height change in one step falls to or
/// below [`ErosionParams::max_height_change_per_step_m`], or until
/// [`ErosionParams::max_iterations`] steps have run -- whichever comes first.
///
/// # What "converged" means, precisely
///
/// **What is measured:** the maximum absolute height change across every node in the graph
/// produced by a single `erode_step` call, via [`max_abs_height_change`] -- not a mean, not
/// a per-node threshold, not a relative change. One node still moving by more than the
/// threshold means the run has not converged, even if every other node has stopped moving
/// entirely.
///
/// **What happens at the cap:** [`ErosionRun::NotConverged`] is returned, carrying the
/// state after the `max_iterations`th step and that same count -- not
/// [`ErosionRun::Converged`], and not a bare `Vec<f64>` a caller could mistake for one. See
/// the enum's own doc for why that distinction is a type, not a comment.
///
/// **What happens on a NaN.** A NaN per-node change is asserted against, loudly, rather than
/// silently reported as either variant -- see [`max_abs_height_change`]'s doc for why a plain
/// `NaN <= threshold` (always `false`) would still have been too quiet. Not reachable today
/// (`erode_step`'s own inputs are checked upstream), but a later arithmetic path into this
/// loop (a thermal correction, say) could reach it, and this loop should not have to be
/// re-audited to notice when one does.
///
/// **Whether the count is part of the output:** yes, on both variants, as `iterations` --
/// see [`ErosionRun`]'s doc for why both need it rather than only the success path.
///
/// # `threshold` is a per-step bound, not a distance from the fixed point
///
/// The map this loop iterates is a contraction with ratio `1 / (1 + c)` (`c` per
/// [`erode_step`]'s "dimensionless number this whole method actually responds to" section),
/// so a per-step change of `eps` corresponds to a REMAINING distance from the true fixed
/// point of roughly `eps / c` -- not `eps`. At this crate's own test constants (`c` on the
/// order of `1.0e-3`), a `max_height_change_per_step_m` of `1.0e-4` still leaves the field
/// roughly `0.1` m from steady state, and a threshold of `1.0` m leaves it roughly a
/// kilometre away, moving slowly rather than sitting still. **"Converged" here means
/// "moving slower than `threshold` per step", not "within `threshold` of steady state"** --
/// a caller that wants the latter must either tighten `threshold` by roughly `1/c`, or accept
/// that this criterion is a rate bound. `a_converged_run_is_a_fixed_point` below tests the
/// weaker, still-real claim (one more step does not exceed the threshold either) -- with `c`
/// this small that claim is close to automatic, since the map having already taken one step
/// under the threshold barely changes on the next; it is a real regression guard against the
/// loop comparing the wrong two arrays, not evidence of proximity to the true fixed point.
///
/// # This calls `erode_step`; it does not re-implement its update
///
/// Every step in this loop is exactly one `erode_step` call over `distances_m` (computed
/// once by the caller via [`receiver_distances_m`] and reused across every iteration, per
/// the module doc's "Distances are computed once per step" section) -- there is no second
/// copy of the implicit update here for the two logic paths to drift apart on. It is
/// followed by exactly one [`cap_slopes`] call over the same step's output -- see the next
/// section for why that call sits *inside* this loop rather than after it.
///
/// # The cap runs per-iteration, not once at the end
///
/// This was the one real design fork in Task 4 (see that task's brief): [`cap_slopes`] could
/// have run once, after this loop returns, on whichever variant's `heights` it produced.
/// This function instead calls it every iteration, immediately after `erode_step` and before
/// the convergence check reads `next`. Both are legitimate solvers; they are not the same
/// solver, and the choice made here changes what "converged" means:
///
/// - **Per-iteration (chosen here).** §14.1's own phrasing is "to stop the equation growing
///   spikes" -- present continuous, describing something happening *while* the equation
///   runs, not a spike that is allowed to fully form and then trimmed off afterward. A spike
///   at a low-`A` node does not stay local: `implicit_receiver_update` reads a node's
///   *receiver's* new height, so an uncorrected spike one iteration becomes an input to
///   every node upstream of it on the next -- exactly the compounding this loop's own
///   root-to-leaves order exists to avoid in the other direction (stale receivers).
///   Capping every iteration means the height fed into the *next* `erode_step` call is
///   always a physically bounded one, so an already-steep node cannot use its own
///   unphysical steepness as the seed for a worse one on the following step. This also
///   means the criterion `max_abs_height_change` measures is the change in a
///   **cap-corrected** field, so "converged" means "the corrected field stopped moving" --
///   consistent with `a_converged_run_is_a_fixed_point`, which is still true of the
///   returned heights because the loop's own fixed point now *is* the capped one.
/// - **Once at the end (the alternative, not chosen).** Cheaper: one `cap_slopes` call
///   total instead of one per iteration -- for a 100-300 iteration bake that is a real
///   saving, since `cap_slopes` is a second full `O(nodes)` pass over the graph on top of
///   `erode_step`'s own. But the field this loop iterates would then be the *uncapped*
///   one throughout every step, and `erode_step`'s implicit update reads a node's
///   receiver's new height -- so the uncapped solver could spend its whole iteration
///   budget converging a low-`A` node toward an uncapped, unphysically steep steady state
///   (§14.1's whole reason for wanting a cap in the first place), and only *afterward*
///   silently overwrite that steady state with something the solver never actually
///   converged to. `a_converged_run_is_a_fixed_point`'s claim -- one more `erode_step` from
///   the returned heights does not move any node past the threshold -- would then no
///   longer hold for the *returned* (capped) heights, only for the discarded uncapped
///   ones: the brief calls this out directly (the returned field stops being "a fixed
///   point of the thing that was iterated"). That silent loss of a tested property, not
///   the extra `O(nodes)` pass, is why this function does not take the cheaper path.
///
/// **Consequence for Task 3's convergence figures:** capping every iteration means Task 3's
/// `erosion_convergence_sweep` numbers are measured against a solver this task modified, not
/// the one Task 3 measured. That sweep has been re-run against this version -- see
/// `src/bin/erosion_convergence_sweep.rs`'s own doc and this crate's `task-4-report.md` for
/// the before/after counts and whether they moved.
///
/// # Iterations-to-convergence is measured, not assumed -- and it is a function of `c`, not of "this implementation"
///
/// §14.3 of the design doc claims 100-300 iterations on a 50 x 50 km planar domain with the
/// paper's own uplift and erodibility, and separately claims that count does not depend on
/// resolution -- the second half is why a planetary bake is thought feasible at all. This
/// function makes that count observable (`iterations` on either variant of the result) so it
/// can be measured here rather than the paper's own numbers being reported as though they
/// held unmeasured. `src/bin/erosion_convergence_sweep.rs` is where that measurement actually
/// happens, since a multi-resolution sweep is too slow to run on every push.
///
/// **The count this loop takes is governed by `c` (see [`erode_step`]'s doc), not by
/// resolution and not by "this implementation" as a monolith.** `N ≈ ln(1/threshold) / c`
/// (up to the constant residual/root terms the sweep's own doc discusses), so two runs with
/// the same `c` converge in comparable counts regardless of node count, and two runs at
/// different `c` are not a like-for-like test of anything resolution-related even if
/// everything else about them matches. At this crate's default test constants
/// (`k = 1.0e-6 /yr`, `dt = 1000 yr`), `c` on a sphere sampled at planetary densities lands
/// around `1.0e-3`; the sweep binary's doc records what happens to the count when `c` is
/// instead pushed toward the paper's implied regime.
pub fn erode_to_convergence(
    graph: &StreamGraph,
    heights: &[f64],
    distances_m: &[f64],
    params: &ErosionParams,
) -> ErosionRun {
    erode_to_convergence_with_clamp_counts(graph, heights, distances_m, params).0
}

/// Identical algorithm to [`erode_to_convergence`] -- this IS its implementation, not a
/// second copy of it; `erode_to_convergence` is a thin wrapper that discards the second
/// return value -- but also reports [`ClampStats`] for the run: how much work
/// [`cap_slopes`] actually did, summed over every iteration.
///
/// See [`ClampStats`]'s own doc for why this is worth a whole return value rather than
/// something a caller could infer from the iteration count: wiring the cap into this loop
/// does not by itself guarantee it ever corrects anything at a given `(u, k, dt)`, and a
/// run whose slopes never approach the cap converges to bit-identical heights with or
/// without `cap_slopes` in the loop at all. `src/bin/erosion_convergence_sweep.rs` calls
/// this (not the plain wrapper) specifically so its own report can say how many edges were
/// clamped in each row, rather than leaving "did the cap do anything here" to be guessed
/// from whether the iteration count moved.
pub fn erode_to_convergence_with_clamp_counts(
    graph: &StreamGraph,
    heights: &[f64],
    distances_m: &[f64],
    params: &ErosionParams,
) -> (ErosionRun, ClampStats) {
    let mut current = heights.to_vec();
    let mut total_edges_clamped: u64 = 0;
    let mut iterations_with_a_clamp: u32 = 0;
    for iteration in 1..=params.max_iterations {
        let stepped = erode_step(graph, &current, distances_m, params);
        // The thermal cap runs INSIDE this loop, on every iteration's output, before the
        // convergence check -- see this function's "The cap runs per-iteration, not once at
        // the end" section below for why, and for what the alternative would have cost.
        let next = cap_slopes(graph, &stepped, distances_m);
        let clamped_this_iteration = slope_cap_clamped_count(&stepped, &next);
        if clamped_this_iteration > 0 {
            total_edges_clamped += clamped_this_iteration as u64; // cast-ok: a node count widened, never negative
            iterations_with_a_clamp += 1;
        }
        let change = max_abs_height_change(&current, &next);
        // `assert!`, not `debug_assert!`: this loop runs in release, same reasoning as
        // `erode_step`'s own slice-length checks. `change.is_nan()` here means SOME node's
        // height went non-finite -- `max_abs_height_change`'s fold is deliberately
        // NaN-poisoning so this can catch it -- and a plain `NaN <= threshold` comparison
        // is always `false`, which would have silently fallen through to "keep iterating"
        // and eventually reported ErosionRun::NotConverged for a run that was not merely
        // slow, it was broken. Not reachable today (erode_step's own inputs are checked
        // upstream of this loop), but a later arithmetic path into this loop should not have
        // to be re-audited for this failure mode to be caught; the loop itself catches it.
        assert!(
            !change.is_nan(),
            "erode_to_convergence: a NaN height change appeared at iteration {iteration} \
             (graph has {} nodes) -- some node's height went non-finite. This is a defect in \
             whatever produced this step's heights, not ordinary non-convergence, and \
             continuing would silently poison every later step and could report \
             ErosionRun::NotConverged as though this were merely slow.",
            graph.node_count(),
        );
        current = next;
        if change <= params.max_height_change_per_step_m {
            return (
                ErosionRun::Converged { heights: current, iterations: iteration },
                ClampStats { total_edges_clamped, iterations_with_a_clamp },
            );
        }
    }
    // `max_iterations == 0` falls straight through to here without the loop body ever
    // running (`1..=0` is an empty range) -- matching `erode_to_convergence`'s
    // pre-instrumentation behaviour of returning `NotConverged` with `iterations: 0` and
    // the input heights untouched (see this crate's Task 3 report, "concerns"). The
    // `ClampStats` for that case is honestly `{0, 0}`, not a placeholder: zero iterations
    // ran, so the cap had no opportunity to do anything.
    (
        ErosionRun::NotConverged { heights: current, iterations: params.max_iterations },
        ClampStats { total_edges_clamped, iterations_with_a_clamp },
    )
}

/// The closed form itself, isolated from graph traversal so it can be tested against exact
/// arithmetic identities directly (a real `StreamGraph` node can never have `area_m2 == 0`
/// -- `drainage_area_m2` includes the node's own area, and `build` rejects non-positive
/// area -- so the zero-area case is only reachable by calling this function directly).
///
/// See [`erode_step`]'s doc for the derivation this implements, and its "`c` is the
/// dimensionless number this whole method actually responds to" section for what the local
/// `c` computed below actually governs -- it is not a scratch intermediate, it is the
/// method's one real parameter.
fn implicit_receiver_update(
    h_i: f64,
    receiver_h_new: f64,
    area_m2: f64,
    distance_m: f64,
    uplift: f64,
    erodibility: f64,
    dt: f64,
) -> f64 {
    // `c`, per erode_step's doc: the dimensionless relaxation number `k * dt * sqrt(A) / d`.
    // Every iteration's step size, and therefore erode_to_convergence's whole iteration
    // count, is a function of this number and nothing else that isn't folded into it.
    let c = erodibility * dt * detmath::sqrt(area_m2) / distance_m;
    (h_i + uplift * dt + c * receiver_h_new) / (1.0 + c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sphere::EARTH_RADIUS_M;
    use crate::stream::{sample_nodes, BuildParams, SamplingKind};

    // ---- a small, real graph, built the same way lib.rs's World tests build one ---------

    const SEED: u64 = 20_260_904;
    const NODES: u32 = 300;

    fn splitmix64(x: u64) -> u64 {
        let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A deterministic, non-flat height field -- this module owns no field of its own, and
    /// borrowing `Surface` here would give `erosion.rs`'s tests a dependency this module
    /// does not otherwise have. The only property the fixture needs is a real graph whose
    /// `peel()` actually forms a forest, which any finite, non-constant height field over a
    /// connected node set gives it.
    fn heights_for(seed: u64, count: u32) -> Vec<f64> {
        (0..count)
            .map(|i| {
                let bits = splitmix64(seed ^ splitmix64(i as u64)); // cast-ok: a node index, already an integer
                let unit = ((bits >> 11) as f64) / 9_007_199_254_740_992.0; // cast-ok: 53 bits into f64
                -500.0 + 4000.0 * unit
            })
            .collect()
    }

    /// A graph and the positions it was built from, together -- `StreamGraph` does not
    /// store positions (only their checksum), so any test that needs a distance has to
    /// keep the positions alongside the graph itself, exactly as a real caller would.
    fn small_graph_and_positions() -> (StreamGraph, Vec<SpherePoint>, Vec<f64>) {
        let sampling = sample_nodes(SEED, NODES, EARTH_RADIUS_M).expect("a node set");
        let heights = heights_for(SEED, NODES);
        let graph = StreamGraph::build(
            &BuildParams {
                world_seed: SEED,
                radius_m: EARTH_RADIUS_M,
                sea_level_m: -600.0,
                sampling_kind: SamplingKind::Spiral,
                pond_max_drainage_area_m2: 1.0e10,
            },
            &sampling.positions,
            &heights,
            &sampling.area_m2,
            &sampling.neighbours,
        )
        .expect("a graph over a real field builds");
        (graph, sampling.positions, heights)
    }

    fn small_graph() -> StreamGraph {
        small_graph_and_positions().0
    }

    fn default_test_params() -> ErosionParams {
        ErosionParams { uplift_m_per_yr: 1.0e-3, erodibility_per_yr: 1.0e-6, timestep_yr: 1000.0, max_height_change_per_step_m: 0.05, max_iterations: 1000 }
    }

    // ---- the spec constants are pinned, not silently driftable ---------------------------

    #[test]
    fn the_stream_power_exponents_match_the_spec() {
        assert_eq!(STREAM_POWER_AREA_EXPONENT, 0.5);
        assert_eq!(STREAM_POWER_SLOPE_EXPONENT, 1.0);
    }

    // ---- ErosionParams is the recorded, comparable interface ------------------------------

    #[test]
    fn erosion_params_is_copy_and_compares_by_value() {
        let a = ErosionParams { uplift_m_per_yr: 1.0e-4, erodibility_per_yr: 2.0e-6, timestep_yr: 1000.0, max_height_change_per_step_m: 0.05, max_iterations: 1000 };
        let b = a; // Copy, not a move -- `a` must still be usable below.
        assert_eq!(a, b);

        let different =
            ErosionParams { uplift_m_per_yr: 1.0e-4, erodibility_per_yr: 3.0e-6, timestep_yr: 1000.0, max_height_change_per_step_m: 0.05, max_iterations: 1000 };
        assert_ne!(a, different, "two runs differing only in erodibility must not compare equal");

        let different_dt =
            ErosionParams { uplift_m_per_yr: 1.0e-4, erodibility_per_yr: 2.0e-6, timestep_yr: 2000.0, max_height_change_per_step_m: 0.05, max_iterations: 1000 };
        assert_ne!(a, different_dt, "two runs differing only in timestep must not compare equal");

        let different_threshold =
            ErosionParams { uplift_m_per_yr: 1.0e-4, erodibility_per_yr: 2.0e-6, timestep_yr: 1000.0, max_height_change_per_step_m: 0.5, max_iterations: 1000 };
        assert_ne!(
            a, different_threshold,
            "two runs differing only in the convergence threshold must not compare equal"
        );

        let different_cap =
            ErosionParams { uplift_m_per_yr: 1.0e-4, erodibility_per_yr: 2.0e-6, timestep_yr: 1000.0, max_height_change_per_step_m: 0.05, max_iterations: 2000 };
        assert_ne!(a, different_cap, "two runs differing only in the iteration cap must not compare equal");
    }

    #[test]
    fn debug_output_shows_all_fields() {
        let params = ErosionParams { uplift_m_per_yr: 1.0e-4, erodibility_per_yr: 2.0e-6, timestep_yr: 1000.0, max_height_change_per_step_m: 0.05, max_iterations: 1000 };
        let text = format!("{params:?}");
        assert!(text.contains("uplift_m_per_yr"));
        assert!(text.contains("erodibility_per_yr"));
        assert!(text.contains("timestep_yr"));
        assert!(text.contains("max_height_change_per_step_m"));
        assert!(text.contains("max_iterations"));
    }

    // ---- uniform now, and honestly so ------------------------------------------------------

    #[test]
    fn uplift_and_erodibility_are_uniform_across_every_node() {
        let params =
            ErosionParams { uplift_m_per_yr: 5.0e-4, erodibility_per_yr: 7.0e-7, timestep_yr: 1000.0, max_height_change_per_step_m: 0.05, max_iterations: 1000 };
        for node in [0u32, 1, 42, NODES - 1] {
            assert_eq!(params.uplift_at(node), params.uplift_m_per_yr);
            assert_eq!(params.erodibility_at(node), params.erodibility_per_yr);
        }
    }

    // ---- the traversal direction: reversed peel order starts at the roots ----------------

    #[test]
    fn root_to_leaves_visits_every_receiver_before_the_node_that_drains_into_it() {
        // The property the solver actually depends on, and the one an off-by-a-direction
        // bug breaks: for every edge `node -> target` in the downhill relation, `target`
        // must be processed BEFORE `node`, because the implicit step reads the receiver's
        // already-updated height. This holds edge-by-edge regardless of how many separate
        // trees the forest has, unlike "all roots come before all leaves" -- which is false
        // whenever one tree is shallower than another (a small tree's root can finish
        // peeling before a deep tree's leaves do).
        let graph = small_graph();
        let peel = graph.peel();
        assert_eq!(peel.peeled, graph.node_count(), "the height field must not have plateaued into a cycle");

        let reversed = root_to_leaves(&graph);
        assert_eq!(reversed.len(), peel.order.len());

        let mut position_of = vec![0usize; graph.node_count() as usize]; // cast-ok: node_count is bounded, mirroring stream.rs's own casts
        for (position, &node) in reversed.iter().enumerate() {
            position_of[node as usize] = position; // cast-ok: a valid node index into usize
        }

        let mut edges_checked = 0;
        for node in 0..graph.node_count() {
            if let Some(target) = graph.downhill_of(node) {
                assert!(
                    position_of[target as usize] < position_of[node as usize], // cast-ok: a valid node index into usize
                    "receiver {target} of node {node} must come before it in root-to-leaves order"
                );
                edges_checked += 1;
            }
        }
        assert!(edges_checked > 0, "a graph with no downhill edges at all is not a meaningful fixture");

        // The reverse claim, stated directly rather than only implied by the edge check:
        // `peel.order` UNREVERSED puts every receiver AFTER the node that drains into it
        // (that is what "leaves to roots" means), which is the wrong order for the solver.
        // Using it directly instead of `root_to_leaves` would violate the property above on
        // every single edge, which is the shape of bug this test exists to catch.
        let mut unreversed_position_of = vec![0usize; graph.node_count() as usize]; // cast-ok: node_count is bounded
        for (position, &node) in peel.order.iter().enumerate() {
            unreversed_position_of[node as usize] = position; // cast-ok: a valid node index into usize
        }
        for node in 0..graph.node_count() {
            if let Some(target) = graph.downhill_of(node) {
                assert!(
                    unreversed_position_of[target as usize] > unreversed_position_of[node as usize], // cast-ok: a valid node index into usize
                    "peel().order unreversed is expected to put every receiver after its own node"
                );
            }
        }
    }

    #[test]
    fn root_to_leaves_reorders_rather_than_reinvents() {
        // Same multiset of nodes as `peel().order`, exactly reversed -- proving this
        // function is `peel()`'s own order read backwards, not an independent walk that
        // could disagree with it.
        let graph = small_graph();
        let peel = graph.peel();
        let mut expected = peel.order.clone();
        expected.reverse();
        assert_eq!(root_to_leaves(&graph), expected);
    }

    // ---- receiver_distances_m -------------------------------------------------------------

    #[test]
    fn receiver_distances_m_matches_sphere_point_distance_to_directly() {
        let (graph, positions, _heights) = small_graph_and_positions();
        let radius_m = graph.header().radius_m;
        let distances = receiver_distances_m(&graph, &positions);
        assert_eq!(distances.len(), graph.node_count() as usize); // cast-ok: node_count is bounded

        let mut receiver_edges_checked = 0;
        for node in 0..graph.node_count() {
            match graph.downhill_of(node) {
                Some(receiver) => {
                    let expected = positions[node as usize] // cast-ok: a node index into usize
                        .distance_to(&positions[receiver as usize], radius_m); // cast-ok: a node index into usize
                    assert_eq!(
                        distances[node as usize].to_bits(), // cast-ok: a node index into usize
                        expected.to_bits(),
                        "node {node}'s receiver distance must be exactly SpherePoint::distance_to's own answer"
                    );
                    receiver_edges_checked += 1;
                }
                None => assert_eq!(distances[node as usize], 0.0, "a root's distance entry is an unread placeholder"), // cast-ok: a node index into usize
            }
        }
        assert!(receiver_edges_checked > 0, "a graph with no downhill edges at all is not a meaningful fixture");
    }

    // ---- erode_step: determinism -----------------------------------------------------------

    #[test]
    fn erode_step_is_bit_identical_across_two_runs_of_the_same_inputs() {
        // Population: every one of NODES = 300 nodes, built TWICE from independent calls to
        // `small_graph_and_positions()` -- two distinct `StreamGraph`s, `Vec<SpherePoint>`s
        // and `Vec<f64>`s at different addresses, rather than one set of inputs reused --
        // so this cannot pass by accident of aliasing or allocator reuse. Method:
        // `receiver_distances_m` then `erode_step`, run once per independent build,
        // compared by `f64::to_bits()` element-by-element (never `==`, which would let two
        // differently-signed zeros or a native/WASM tolerance pass unnoticed). Host: this
        // crate's own test process (native), run-against-run rather than
        // native-against-WASM.
        //
        // What this test cannot claim: it says nothing about the module doc's
        // native-against-WASM equality (that would need a WASM harness this crate's unit
        // tests do not run -- see `viewer/`'s `build:wasm:self-test`), and both builds still
        // execute the same code path in the same process, so it cannot catch a
        // hypothetical dependency on iteration order over an actual `HashMap` (this module
        // has none; `StreamGraph::peel()`'s queue is a `Vec`, not a hash-ordered
        // structure). It does rule out the weaker failure modes above it, which is what a
        // "same inputs, same process" version of this test could not.
        let (graph_a, positions_a, heights_a) = small_graph_and_positions();
        let (graph_b, positions_b, heights_b) = small_graph_and_positions();
        let distances_a = receiver_distances_m(&graph_a, &positions_a);
        let distances_b = receiver_distances_m(&graph_b, &positions_b);
        let params = default_test_params();

        let first = erode_step(&graph_a, &heights_a, &distances_a, &params);
        let second = erode_step(&graph_b, &heights_b, &distances_b, &params);

        assert_eq!(first.len(), second.len());
        for i in 0..first.len() {
            assert_eq!(first[i].to_bits(), second[i].to_bits(), "node {i} disagreed bit-for-bit between two independent builds");
        }
    }

    // ---- erode_step: the traversal direction is load-bearing -------------------------------

    #[test]
    #[should_panic(expected = "heights must be exactly as long as the graph has nodes")]
    fn erode_step_rejects_a_heights_slice_longer_than_the_graph() {
        // The asymmetric case, and the reason these checks are `assert_eq!` rather than
        // `debug_assert_eq!`. A slice that is too SHORT would panic anyway on the first
        // out-of-range index. A slice that is too LONG trips no bounds check at all: `next`
        // is cloned from `heights`, so the surplus entries ride through untouched and get
        // returned, handing the caller an array longer than the graph with a tail nobody
        // wrote. Without a release-time assertion that is silent, and the bake runs in
        // release -- the one build a debug assertion does not reach.
        let (graph, positions, heights) = small_graph_and_positions();
        let distances = receiver_distances_m(&graph, &positions);
        let mut too_long = heights.clone();
        too_long.push(0.0);
        let _ = erode_step(&graph, &too_long, &distances, &default_test_params());
    }

    #[test]
    #[should_panic(expected = "distances_m must be exactly as long as the graph has nodes")]
    fn erode_step_rejects_a_distances_slice_that_does_not_match_the_graph() {
        // The same check on the other slice, named separately so a failure says WHICH
        // argument was wrong. Two arguments validated by one message is a message that
        // sends the reader to the wrong place half the time.
        let (graph, positions, heights) = small_graph_and_positions();
        let mut distances = receiver_distances_m(&graph, &positions);
        distances.pop();
        let _ = erode_step(&graph, &heights, &distances, &default_test_params());
    }

    #[test]
    fn erode_step_would_differ_if_walked_leaves_to_roots_instead_of_root_to_leaves() {
        // The property an off-by-a-direction bug breaks, made concrete rather than only
        // asserted by inspection: re-run the exact same per-node closed form but drive it
        // with `peel().order` (leaves-to-roots, `erosion.rs`'s own module doc names this as
        // the WRONG direction for this solver) instead of `root_to_leaves`. The only
        // difference between this block and `erode_step` is which order `next[]` gets
        // written in, so any disagreement below comes from the direction, not the formula.
        let (graph, positions, heights) = small_graph_and_positions();
        let distances = receiver_distances_m(&graph, &positions);
        let params = default_test_params();

        let correct = erode_step(&graph, &heights, &distances, &params);

        let mut wrong_direction = heights.clone();
        for node in graph.peel().order {
            // NOTE: `peel().order`, NOT `root_to_leaves(&graph)` -- the bug under test.
            let idx = node as usize; // cast-ok: a node index into usize
            match graph.downhill_of(node) {
                None => wrong_direction[idx] = heights[idx],
                Some(receiver) => {
                    let receiver_idx = receiver as usize; // cast-ok: a node index into usize
                    // In this (wrong) order the receiver has NOT been overwritten yet when
                    // a leaf-ward node reaches it, so this reads the OLD receiver height --
                    // exactly the defect root-to-leaves exists to avoid.
                    let receiver_h_stale = wrong_direction[receiver_idx];
                    wrong_direction[idx] = implicit_receiver_update(
                        heights[idx],
                        receiver_h_stale,
                        graph.drainage_area_m2(node),
                        distances[idx],
                        params.uplift_at(node),
                        params.erodibility_at(node),
                        params.timestep_yr,
                    );
                }
            }
        }

        assert_ne!(
            correct, wrong_direction,
            "walking leaves-to-roots must disagree with root-to-leaves somewhere in this fixture"
        );

        // Not vacuous: the fixture must actually contain a chain two hops deep, i.e. some
        // node whose receiver itself has its own receiver -- otherwise every h_r' is a
        // root's untouched height under both orders and the two walks would coincide by
        // accident rather than because the direction matters.
        let has_two_hop_chain = (0..graph.node_count())
            .any(|node| graph.downhill_of(node).and_then(|r| graph.downhill_of(r)).is_some());
        assert!(has_two_hop_chain, "fixture must contain a downhill chain at least two hops deep");
    }

    // ---- erode_step: the root policy is held-fixed, not uplift-only -----------------------

    #[test]
    fn a_root_is_held_at_its_old_height_rather_than_uplifted() {
        // The module doc chose "held fixed" over "uplift-only" because a root is the local
        // base level of its basin (a mouth or a lake outlet -- stream.rs's own invariant
        // says every root is exactly one of those), and the equation erodes everything
        // else toward that level rather than raising the level itself. That choice was
        // documented but previously asserted nowhere: a review confirmed by mutation that
        // flipping the root branch to `heights[idx] + uplift * dt` left all other tests
        // green. This test exists so that mutation is instead caught here, exactly, by bit
        // pattern -- not "close to unchanged", since holding fixed means literally copying
        // the old value through with no arithmetic at all.
        let (graph, positions, heights) = small_graph_and_positions();
        let distances = receiver_distances_m(&graph, &positions);
        // Uplift is deliberately large and nonzero: if a future edit silently switched the
        // root branch to uplift-only, a zero uplift would let it pass unnoticed by
        // coincidence.
        let params = ErosionParams { uplift_m_per_yr: 5.0e-2, erodibility_per_yr: 1.0e-6, timestep_yr: 1000.0, max_height_change_per_step_m: 0.05, max_iterations: 1000 };

        let next = erode_step(&graph, &heights, &distances, &params);

        let roots = graph.roots();
        assert!(!roots.is_empty(), "fixture must contain at least one root to check this against");
        for root in roots {
            let idx = root as usize; // cast-ok: a node index into usize
            assert_eq!(
                next[idx].to_bits(),
                heights[idx].to_bits(),
                "root {root} must be held at its old height exactly, not uplifted by u*dt"
            );
        }
    }

    // ---- implicit_receiver_update: the exact arithmetic identities -------------------------

    #[test]
    fn zero_drainage_area_rises_by_exactly_uplift_times_dt() {
        // sqrt(0) = 0, so c = 0 and the closed form collapses to `(h_i + u*dt + 0) / 1`,
        // an exact identity rather than a tolerance -- dividing by 1.0 is a no-op, and the
        // numerator is a single addition chain with no term to round away.
        let h_i = 1234.5;
        let uplift = 3.0e-3;
        let dt = 500.0;
        let result = implicit_receiver_update(h_i, /* receiver_h_new */ -999.0, 0.0, 10.0, uplift, 1.0e-5, dt);
        assert_eq!(result.to_bits(), (h_i + uplift * dt).to_bits());
    }

    #[test]
    fn a_receiver_at_the_same_height_erodes_nothing() {
        // s = (h_i - h_r) / d = 0 when h_i == h_r, so with uplift also zero the update
        // is `(h + 0 + c*h) / (1 + c)` -- algebraically `h*(1+c)/(1+c)`, which cancels to
        // `h` exactly. Division by `1 + c` is not guaranteed bit-exact for an arbitrary
        // `c` the way dividing by the literal `1.0` is (the zero-drainage-area test
        // above), so this was originally written with a tolerance on the assumption that
        // rounding here was unavoidable. It was measured, not assumed: for these inputs
        // `c` evaluates to exactly `0.024`, and every intermediate -- `c*h`, `h + c*h`,
        // `1.0 + c`, and the final quotient -- lands on its exact value, so the result is
        // bit-identical to `h`. `f64::to_bits()` says so directly; there is nothing here
        // for a tolerance to forgive. (The general claim that an arbitrary `c` is not
        // guaranteed exact is still true -- it just doesn't apply to this `c`.)
        let h = 777.0;
        let result = implicit_receiver_update(h, h, 4.0e6, 250.0, 0.0, 3.0e-6, 1000.0);
        assert_eq!(result.to_bits(), h.to_bits(), "expected exactly {h}, got {result}");
    }

    // ---- erode_step: no NaN, no infinity, anywhere -----------------------------------------

    #[test]
    fn erode_step_output_is_finite_everywhere() {
        let (graph, positions, heights) = small_graph_and_positions();
        let distances = receiver_distances_m(&graph, &positions);
        let params = default_test_params();
        let next = erode_step(&graph, &heights, &distances, &params);

        let non_finite: Vec<usize> = next.iter().enumerate().filter(|(_, h)| !h.is_finite()).map(|(i, _)| i).collect();
        assert!(non_finite.is_empty(), "non-finite output at node indices {non_finite:?}");
    }

    // ---- erode_step: monotone in erodibility --------------------------------------------

    #[test]
    fn increasing_erodibility_never_raises_a_node_with_drainage_above_it() {
        // Every non-root node in this graph has `h_i > h_r` (the downhill relation is
        // strictly descending -- stream.rs's own invariant), and uplift is held positive
        // and fixed, so `h_r' < h_i + u*dt` holds at every node. Under that condition the
        // closed form's derivative with respect to `c` (and so with respect to k, since
        // `c` is monotone increasing in k) has a fixed sign -- see erode_step's derivation
        // -- so increasing k while everything else is held fixed must not raise any
        // non-root node's answer.
        let (graph, positions, heights) = small_graph_and_positions();
        let distances = receiver_distances_m(&graph, &positions);

        let low_k = ErosionParams { uplift_m_per_yr: 1.0e-3, erodibility_per_yr: 1.0e-8, timestep_yr: 1000.0, max_height_change_per_step_m: 0.05, max_iterations: 1000 };
        let high_k = ErosionParams { uplift_m_per_yr: 1.0e-3, erodibility_per_yr: 1.0e-4, timestep_yr: 1000.0, max_height_change_per_step_m: 0.05, max_iterations: 1000 };

        let with_low_k = erode_step(&graph, &heights, &distances, &low_k);
        let with_high_k = erode_step(&graph, &heights, &distances, &high_k);

        // Zero tolerance, not a rounding allowance: measured on this fixture (300 nodes,
        // seed 20_260_904, `SamplingKind::Spiral`, these two `k` values, `u = 1.0e-3`,
        // `dt = 1000.0`), every one of the 262 draining nodes falls by at least 3.46 m
        // between `low_k` and `high_k` -- nowhere near the boundary a rounding error could
        // cross. A tolerance here would be six-plus orders of magnitude looser than that
        // margin and would silently absorb a real formula regression instead of catching
        // it, which is exactly backwards for a module whose claim is bit-exact
        // determinism. If a future fixture or parameter choice ever needs slack, add it
        // deliberately and cite the measured margin that justifies it, the way this
        // comment does.
        let mut violations = Vec::new();
        let mut a_real_decrease_happened = false;
        for node in 0..graph.node_count() {
            let idx = node as usize; // cast-ok: a node index into usize
            if graph.downhill_of(node).is_none() {
                continue; // roots are held fixed regardless of k; nothing to check
            }
            if with_high_k[idx] > with_low_k[idx] {
                violations.push((node, with_low_k[idx], with_high_k[idx]));
            }
            if with_high_k[idx] < with_low_k[idx] {
                a_real_decrease_happened = true;
            }
        }
        assert!(violations.is_empty(), "higher erodibility raised a draining node: {violations:?}");
        assert!(a_real_decrease_happened, "fixture and k values must produce a measurable difference somewhere");
    }

    // ---- erode_to_convergence -------------------------------------------------------------
    //
    // Fixture: the same 300-node graph as the rest of this module (seed 20_260_904,
    // `SamplingKind::Spiral`), `u = 1.0e-3 m/yr`, `k = 1.0e-6 /yr`, `dt = 1000 yr` -- the
    // same as `default_test_params`, plus a convergence threshold/cap that these tests set
    // per-property rather than inheriting a shared default, since the whole point of this
    // section is what changing those two values does.

    fn convergence_test_params(max_height_change_per_step_m: f64, max_iterations: u32) -> ErosionParams {
        ErosionParams {
            uplift_m_per_yr: 1.0e-3,
            erodibility_per_yr: 1.0e-6,
            timestep_yr: 1000.0,
            max_height_change_per_step_m,
            max_iterations,
        }
    }

    #[test]
    fn erode_to_convergence_is_bit_identical_and_same_iteration_count_across_two_runs() {
        // Same property as erode_step's own determinism test, one level up: two
        // independent graph builds, run to convergence, compared by bit pattern AND by
        // iteration count -- a solver that converges deterministically but at a different
        // step count on a repeat run is exactly as unreproducible as one with different
        // final heights.
        let (graph_a, positions_a, heights_a) = small_graph_and_positions();
        let (graph_b, positions_b, heights_b) = small_graph_and_positions();
        let distances_a = receiver_distances_m(&graph_a, &positions_a);
        let distances_b = receiver_distances_m(&graph_b, &positions_b);
        let params = convergence_test_params(1.0e-3, 20_000);

        let first = erode_to_convergence(&graph_a, &heights_a, &distances_a, &params);
        let second = erode_to_convergence(&graph_b, &heights_b, &distances_b, &params);

        let (ErosionRun::Converged { heights: first_h, iterations: first_i }, ErosionRun::Converged { heights: second_h, iterations: second_i }) = (first, second) else {
            panic!("this fixture and cap are expected to converge; a NotConverged result here means the cap needs raising, not that this test should tolerate it");
        };
        assert_eq!(first_i, second_i, "two independent builds converged in different iteration counts");
        assert_eq!(first_h.len(), second_h.len());
        for i in 0..first_h.len() {
            assert_eq!(first_h[i].to_bits(), second_h[i].to_bits(), "node {i} disagreed bit-for-bit between two independent runs to convergence");
        }
    }

    #[test]
    fn convergence_is_monotone_in_the_threshold() {
        // A tighter threshold must never converge in FEWER iterations than a looser one,
        // on the same graph and the same everything else. Three thresholds spanning two
        // orders of magnitude, not two, so a flat result across all three would be visible
        // rather than hiding behind a single coincidental pair.
        let (graph, positions, heights) = small_graph_and_positions();
        let distances = receiver_distances_m(&graph, &positions);
        let cap = 20_000;

        let loose = convergence_test_params(1.0, cap);
        let medium = convergence_test_params(1.0e-2, cap);
        let tight = convergence_test_params(1.0e-4, cap);

        let run = |p: &ErosionParams| match erode_to_convergence(&graph, &heights, &distances, p) {
            ErosionRun::Converged { iterations, .. } => iterations,
            ErosionRun::NotConverged { .. } => {
                panic!("threshold {} did not converge within the {cap}-iteration cap this test relies on", p.max_height_change_per_step_m)
            }
        };

        let loose_iters = run(&loose);
        let medium_iters = run(&medium);
        let tight_iters = run(&tight);

        assert!(
            medium_iters >= loose_iters,
            "a tighter threshold ({}) took fewer iterations ({medium_iters}) than a looser one ({}, {loose_iters})",
            medium.max_height_change_per_step_m, loose.max_height_change_per_step_m
        );
        assert!(
            tight_iters >= medium_iters,
            "a tighter threshold ({}) took fewer iterations ({tight_iters}) than a looser one ({}, {medium_iters})",
            tight.max_height_change_per_step_m, medium.max_height_change_per_step_m
        );
        // Not vacuous: at least one of the two gaps above must be a strict increase, or a
        // threshold three orders of magnitude apart would have proven nothing about
        // monotonicity, only that iteration counts are non-negative.
        assert!(
            tight_iters > loose_iters,
            "loosest ({}) and tightest ({}) threshold converged in the same {loose_iters} iterations; \
             this fixture does not distinguish them",
            loose.max_height_change_per_step_m, tight.max_height_change_per_step_m
        );
    }

    #[test]
    fn a_converged_run_is_a_fixed_point() {
        // The property that makes "converged" mean something: one more erode_step from a
        // converged state must itself move no node by more than the threshold. This is
        // cheap (one extra step) and it is the test that would catch a convergence check
        // comparing the wrong two arrays (e.g. first-vs-last instead of consecutive steps).
        let (graph, positions, heights) = small_graph_and_positions();
        let distances = receiver_distances_m(&graph, &positions);
        let params = convergence_test_params(1.0e-3, 20_000);

        let ErosionRun::Converged { heights: converged, iterations } = erode_to_convergence(&graph, &heights, &distances, &params) else {
            panic!("this fixture and cap are expected to converge");
        };
        assert!(iterations > 1, "a fixed point one step away from the START is not a meaningful test of the LOOP");

        let one_more = erode_step(&graph, &converged, &distances, &params);
        let change = max_abs_height_change(&converged, &one_more);
        assert!(
            change <= params.max_height_change_per_step_m,
            "one more erode_step from a converged state moved a node by {change} m, above the {} m threshold",
            params.max_height_change_per_step_m
        );
    }

    #[test]
    fn a_small_cap_reports_non_convergence_rather_than_success() {
        // Construct a run that genuinely cannot converge within the cap: an impossibly
        // tight threshold (below the smallest positive f64 height change this fixture
        // could ever produce) paired with a cap of exactly 1 iteration. This is the
        // property the module doc calls out as the failure mode most likely here --
        // returning a bare `Vec<f64>` indistinguishable from a converged one -- asserted by
        // constructing a run that cannot converge, not merely by inspecting that the call
        // returned.
        let (graph, positions, heights) = small_graph_and_positions();
        let distances = receiver_distances_m(&graph, &positions);
        let params = convergence_test_params(0.0, 1);

        let result = erode_to_convergence(&graph, &heights, &distances, &params);
        match result {
            ErosionRun::NotConverged { heights: after, iterations } => {
                assert_eq!(iterations, 1, "the cap is 1 iteration; a NotConverged result must report exactly that many");
                assert_eq!(after.len(), heights.len());
            }
            ErosionRun::Converged { .. } => {
                panic!("a threshold of exactly 0.0 m converging is not credible for a real, non-flat height field")
            }
        }
    }

    #[test]
    fn erode_to_convergence_output_is_finite_everywhere() {
        let (graph, positions, heights) = small_graph_and_positions();
        let distances = receiver_distances_m(&graph, &positions);
        let params = convergence_test_params(1.0e-3, 20_000);

        let (ErosionRun::Converged { heights: result, .. } | ErosionRun::NotConverged { heights: result, .. }) =
            erode_to_convergence(&graph, &heights, &distances, &params);

        let non_finite: Vec<usize> = result.iter().enumerate().filter(|(_, h)| !h.is_finite()).map(|(i, _)| i).collect();
        assert!(non_finite.is_empty(), "non-finite output at node indices {non_finite:?}");
    }

    // ---- erode_to_convergence: a NaN height change is a loud panic, not a quiet result ----

    #[test]
    #[should_panic(expected = "a NaN height change appeared at iteration")]
    fn erode_to_convergence_panics_loudly_on_a_nan_poisoned_height_change() {
        // Review finding 4: `max_abs_height_change`'s old fold silently discarded a NaN
        // (`NaN > largest` is false), so a NaN-poisoned run reported `Converged` on its very
        // first step -- the exact "non-convergence mistaken for success" shape the brief
        // spent a whole section on, arriving through a different door. Not reachable via a
        // real StreamGraph today (`build` rejects non-positive area and coincident nodes,
        // which are the only routes to a zero denominator), so this test reaches it the
        // only way currently possible: a raw `heights` slice handed to this function
        // directly, exactly as a caller could construct even though `StreamGraph::build`
        // itself would refuse to.
        let (graph, positions, mut heights) = small_graph_and_positions();
        let distances = receiver_distances_m(&graph, &positions);
        // Poisoning a NON-root node: a root is held fixed (module doc's "held fixed" policy),
        // so `next[root] = heights[root]` would carry the same NaN through unchanged and
        // `(NaN - NaN).abs()` is still NaN -- either choice works, but a draining node
        // exercises the arithmetic path (implicit_receiver_update), not just a copy.
        let draining_node = (0..graph.node_count())
            .find(|&n| graph.downhill_of(n).is_some())
            .expect("fixture must contain at least one draining node");
        heights[draining_node as usize] = f64::NAN; // cast-ok: a node index into usize
        let params = convergence_test_params(1.0e-3, 20_000);

        let _ = erode_to_convergence(&graph, &heights, &distances, &params);
    }

    // ---- the slope cap: the constant --------------------------------------------------

    #[test]
    fn slope_cap_tan_is_computed_once_and_matches_tan_30_degrees() {
        // §14.1: "A thermal-erosion correction caps slopes at 30 degrees". Computed once
        // via `detmath::tan` (never `f64::tan` -- see that module's doc) and cached in a
        // `OnceLock` -- calling it twice must return the exact same bits, and that value
        // must equal an independently-computed `detmath::tan(detmath::to_radians(30.0))`
        // right here, not merely "close to" it.
        let cached = slope_cap_tan();
        let recomputed = crate::detmath::tan(crate::detmath::to_radians(30.0));
        assert_eq!(cached.to_bits(), recomputed.to_bits(), "the cached value must match a fresh detmath::tan call exactly");
        assert_eq!(slope_cap_tan().to_bits(), cached.to_bits(), "a second call must return the identical cached bits, not recompute");
        // The well-known value of tan(30 degrees) is 1/sqrt(3) ~= 0.5773502691896257.
        // Measured on this build via `detmath::tan(detmath::to_radians(30.0))`:
        // 0.57735026918962573 (bit pattern 4603375528459645724) -- pinned as a literal so a
        // future change to `detmath::tan`'s libm backend, or an accidental switch to a
        // different angle, fails loudly here rather than drifting silently.
        assert_eq!(cached.to_bits(), 4603375528459645724u64, "slope_cap_tan() drifted from its measured, pinned value");
        assert!((0.57..0.585).contains(&cached), "slope_cap_tan() = {cached} is not in the right ballpark for tan(30 degrees)");
    }

    // ---- fixture helper: a real node this fixture can build the §14.1 pathology on --------

    /// A node from `small_graph` that drains directly into a root, chosen with the smallest
    /// drainage area of any such node. Draining into a root matters because a root's height
    /// never moves (module doc's "held fixed" section) regardless of how many `erode_step`
    /// calls run -- so this node's receiver is a fixed base level for the whole test, and
    /// the smallest area gives it the smallest `sqrt(A)` in the fixture, hence (per
    /// `erode_step`'s `c = k * dt * sqrt(A) / d` doc) the smallest relaxation rate for any
    /// given `k` -- quantitatively what "barely erodes" (§14.1) means.
    fn lowest_area_root_adjacent_node(graph: &StreamGraph, distances_m: &[f64]) -> (u32, u32, f64, f64) {
        (0..graph.node_count())
            .filter_map(|node| {
                let receiver = graph.downhill_of(node)?;
                if graph.downhill_of(receiver).is_some() {
                    return None; // receiver is not itself a root
                }
                let area = graph.drainage_area_m2(node);
                let distance = distances_m[node as usize]; // cast-ok: a node index into usize
                Some((node, receiver, area, distance))
            })
            .min_by(|a, b| a.2.partial_cmp(&b.2).expect("area is never NaN on a built graph"))
            .expect("fixture must contain at least one node draining directly into a root")
    }

    /// `u` and `k` for the pathology tests below, derived analytically from the target
    /// node's own `area_m2` and `distance_m` rather than guessed. With a root-fixed
    /// receiver, `erode_step`'s closed form is a contraction toward a steady-state slope
    /// `s_eq = u / (k * sqrt(A))` (set `dh/dt = 0` in `dh/dt = u - k * sqrt(A) * s`),
    /// approached at rate `c = k * dt * sqrt(A) / d` per step (`erode_step`'s "dimensionless
    /// number" section: each step closes a fraction `c / (1 + c)` of the remaining gap).
    /// Solving for `u` and `k` so that `s_eq` is `SAFETY_FACTOR_OVER_CAP` times the cap, and
    /// `c` is `TARGET_RELAXATION_RATE`:
    ///
    /// ```text
    /// s_eq = u / (k * sqrt(A)) = SAFETY_FACTOR_OVER_CAP * cap
    /// c    = k * dt * sqrt(A) / d = TARGET_RELAXATION_RATE
    /// =>  u = TARGET_RELAXATION_RATE * SAFETY_FACTOR_OVER_CAP * cap * d / dt
    ///     k = TARGET_RELAXATION_RATE * d / (dt * sqrt(A))
    /// ```
    ///
    /// With `TARGET_RELAXATION_RATE = 0.05`, reaching even 1/3 of the way to `s_eq`
    /// (already past the cap, since `s_eq` is `SAFETY_FACTOR_OVER_CAP = 3` times it) needs
    /// only `ln(2/3) / ln(1 - 0.05) ~= 7.9` iterations -- comfortably inside the
    /// `PATHOLOGY_ITERATIONS = 60` these tests run, with margin if the fixture ever
    /// changes slightly.
    const SAFETY_FACTOR_OVER_CAP: f64 = 3.0;
    const TARGET_RELAXATION_RATE: f64 = 0.05;
    const PATHOLOGY_DT_YR: f64 = 1000.0;
    const PATHOLOGY_ITERATIONS: u32 = 60;

    fn pathology_params(area_m2: f64, distance_m: f64) -> ErosionParams {
        let cap = slope_cap_tan();
        let uplift_m_per_yr = TARGET_RELAXATION_RATE * SAFETY_FACTOR_OVER_CAP * cap * distance_m / PATHOLOGY_DT_YR;
        let erodibility_per_yr = TARGET_RELAXATION_RATE * distance_m / (PATHOLOGY_DT_YR * crate::detmath::sqrt(area_m2));
        ErosionParams {
            uplift_m_per_yr,
            erodibility_per_yr,
            timestep_yr: PATHOLOGY_DT_YR,
            // Not exercised by the tests that only drive `erode_step`/`cap_slopes`
            // directly, but `ErosionParams` has no `Default`, so something must be chosen.
            max_height_change_per_step_m: 1.0e-6,
            max_iterations: PATHOLOGY_ITERATIONS,
        }
    }

    /// Runs raw, UNCAPPED `erode_step` `PATHOLOGY_ITERATIONS` times. Deliberately not
    /// `erode_to_convergence`, whose loop (after this task) calls `cap_slopes` every
    /// iteration -- this is the "before" half of the before/after pair the brief asks for.
    fn run_uncapped(graph: &StreamGraph, heights: &[f64], distances_m: &[f64], params: &ErosionParams) -> Vec<f64> {
        let mut current = heights.to_vec();
        for _ in 0..PATHOLOGY_ITERATIONS {
            current = erode_step(graph, &current, distances_m, params);
        }
        current
    }

    // ---- demonstrate the pathology BEFORE demonstrating its removal -----------------------

    #[test]
    fn a_low_drainage_area_node_grows_past_the_cap_without_correction() {
        // The brief: "a test that merely shows 'no edge exceeds 30 degrees afterwards'
        // proves the cap ran, not that it was needed... Demonstrate the pathology first."
        // §14.1 says spikes appear "where drainage area is small", because the erosion term
        // carries sqrt(A) (STREAM_POWER_AREA_EXPONENT) -- a low-A node barely erodes while
        // its uplift keeps adding height every step. This picks the fixture's own
        // smallest-area, root-adjacent node, derives u and k analytically so its
        // steady-state slope is 3x the cap, and runs the UNCORRECTED solver -- no
        // `cap_slopes` anywhere in this test -- to show the slope it actually produces
        // exceeds tan(30 degrees).
        let (graph, positions, heights) = small_graph_and_positions();
        let distances = receiver_distances_m(&graph, &positions);
        let (target, receiver, area, distance) = lowest_area_root_adjacent_node(&graph, &distances);
        let params = pathology_params(area, distance);

        let spiked = run_uncapped(&graph, &heights, &distances, &params);

        let target_idx = target as usize; // cast-ok: a node index into usize
        let receiver_idx = receiver as usize; // cast-ok: a node index into usize
        assert_eq!(
            spiked[receiver_idx].to_bits(),
            heights[receiver_idx].to_bits(),
            "the receiver is a root and must be held fixed regardless of the spike upstream of it"
        );

        let cap = slope_cap_tan();
        let slope = (spiked[target_idx] - spiked[receiver_idx]) / distance;
        assert!(
            slope > cap,
            "node {target} (area {area:.3e} m^2, distance {distance:.1} m to root {receiver}) reached slope \
             {slope:.4} after {PATHOLOGY_ITERATIONS} uncapped steps -- expected it to exceed the cap {cap:.4} \
             (tan(30 degrees)), which is the pathology this task's correction exists to stop. If this fails, \
             the derivation in `pathology_params`'s doc no longer matches this fixture."
        );
    }

    // ---- the correction: cap_slopes fixes exactly that pathology --------------------------

    /// Every edge whose height difference exceeds `cap * distance` by more than a few ULPs
    /// of that product. Returns `(node, excess_slope)` for each violation.
    ///
    /// **Why this compares rises (`h_i - h_r` against `cap * d`) rather than re-deriving
    /// `(h_i - h_r) / d` and comparing that to `cap` with a bare `>`:** `cap_slopes` assigns
    /// a corrected height as the SUM `receiver_h + cap * d`. Recomputing a slope from the
    /// result by inverting that sum -- a separate subtraction, then a separate division --
    /// is NOT guaranteed bit-exact by IEEE-754 (`(a + b) - a == b` does not hold in
    /// general). Measured on this fixture: at the magnitudes involved (`cap * d` on the
    /// order of 1.0e5-1.0e6 m against receiver heights only in the thousands), that
    /// round-trip lands the recomputed slope 1-2 ULPs above `cap` even on a height
    /// `cap_slopes` assigned EXACTLY as `receiver_h + cap * d` -- an absolute height error
    /// around 1.0e-10 m, meaningless against a planetary bake and nothing like the RAW,
    /// uncorrected pathology this task demonstrates (there the slope exceeds the cap by a
    /// factor of `SAFETY_FACTOR_OVER_CAP = 3`, i.e. by whole units of slope, not a fraction
    /// of an Angstrom). This function instead checks the same SUM `cap_slopes` itself
    /// computed, with a 4-ULP allowance on that comparison -- measured, not assumed: the
    /// worst violation actually observed by the naive re-derivation was 2 ULPs, so 4 leaves
    /// a real margin without being loose enough to hide anything the pathology test above
    /// would recognise as a genuine spike.
    fn slope_cap_violations(graph: &StreamGraph, heights: &[f64], distances_m: &[f64], cap: f64) -> Vec<(u32, f64)> {
        let mut violations = Vec::new();
        for node in 0..graph.node_count() {
            if let Some(receiver) = graph.downhill_of(node) {
                let idx = node as usize; // cast-ok: a node index into usize
                let receiver_idx = receiver as usize; // cast-ok: a node index into usize
                let d = distances_m[idx];
                let cap_rise = cap * d;
                let actual_rise = heights[idx] - heights[receiver_idx];
                let tolerance = 4.0 * f64::EPSILON * cap_rise.abs().max(1.0);
                if actual_rise > cap_rise + tolerance {
                    violations.push((node, (actual_rise - cap_rise) / d));
                }
            }
        }
        violations
    }

    #[test]
    fn cap_slopes_binds_every_edge_to_the_cap() {
        // The "after" half: apply the correction to the SAME pathological field the test
        // above produced, and check EVERY edge, not a sample.
        let (graph, positions, heights) = small_graph_and_positions();
        let distances = receiver_distances_m(&graph, &positions);
        let (_target, _receiver, area, distance) = lowest_area_root_adjacent_node(&graph, &distances);
        let params = pathology_params(area, distance);
        let spiked = run_uncapped(&graph, &heights, &distances, &params);

        let corrected = cap_slopes(&graph, &spiked, &distances);
        let cap = slope_cap_tan();

        let edges_checked = (0..graph.node_count()).filter(|&n| graph.downhill_of(n).is_some()).count();
        assert!(edges_checked > 0, "a graph with no downhill edges is not a meaningful fixture");

        let violations = slope_cap_violations(&graph, &corrected, &distances, cap);
        assert!(violations.is_empty(), "edges still exceeding the cap after correction: {violations:?}");
    }

    #[test]
    fn cap_slopes_is_idempotent() {
        let (graph, positions, heights) = small_graph_and_positions();
        let distances = receiver_distances_m(&graph, &positions);
        let (_target, _receiver, area, distance) = lowest_area_root_adjacent_node(&graph, &distances);
        let params = pathology_params(area, distance);
        let spiked = run_uncapped(&graph, &heights, &distances, &params);

        let once = cap_slopes(&graph, &spiked, &distances);
        let twice = cap_slopes(&graph, &once, &distances);

        assert_eq!(once.len(), twice.len());
        for i in 0..once.len() {
            assert_eq!(
                once[i].to_bits(),
                twice[i].to_bits(),
                "node {i} changed on a second cap_slopes pass over an already-corrected field"
            );
        }
    }

    #[test]
    fn cap_slopes_never_raises_a_node() {
        // Brief item 6: whichever node this correction moves, it must never RAISE one --
        // lowering the upstream node is a different physical claim than raising the
        // receiver, and cap_slopes's own doc says it always does the former. Checked
        // against the pathological field, where a real correction actually happens --
        // against an already-compliant field "never raises" would be trivially true
        // because nothing moves at all.
        let (graph, positions, heights) = small_graph_and_positions();
        let distances = receiver_distances_m(&graph, &positions);
        let (_target, _receiver, area, distance) = lowest_area_root_adjacent_node(&graph, &distances);
        let params = pathology_params(area, distance);
        let spiked = run_uncapped(&graph, &heights, &distances, &params);

        let corrected = cap_slopes(&graph, &spiked, &distances);

        let mut any_lowered = false;
        for i in 0..spiked.len() {
            assert!(
                corrected[i] <= spiked[i],
                "node {i} was RAISED by cap_slopes ({} -> {}), which is a different correction than the one this function documents",
                spiked[i],
                corrected[i]
            );
            if corrected[i] < spiked[i] {
                any_lowered = true;
            }
        }
        assert!(any_lowered, "the pathological fixture must produce at least one real correction, or this test proves nothing");
    }

    #[test]
    fn cap_slopes_is_bit_identical_across_two_independent_runs() {
        let (graph_a, positions_a, heights_a) = small_graph_and_positions();
        let (graph_b, positions_b, heights_b) = small_graph_and_positions();
        let distances_a = receiver_distances_m(&graph_a, &positions_a);
        let distances_b = receiver_distances_m(&graph_b, &positions_b);
        let (_t, _r, area, distance) = lowest_area_root_adjacent_node(&graph_a, &distances_a);
        let params = pathology_params(area, distance);

        let spiked_a = run_uncapped(&graph_a, &heights_a, &distances_a, &params);
        let spiked_b = run_uncapped(&graph_b, &heights_b, &distances_b, &params);

        let corrected_a = cap_slopes(&graph_a, &spiked_a, &distances_a);
        let corrected_b = cap_slopes(&graph_b, &spiked_b, &distances_b);

        assert_eq!(corrected_a.len(), corrected_b.len());
        for i in 0..corrected_a.len() {
            assert_eq!(
                corrected_a[i].to_bits(),
                corrected_b[i].to_bits(),
                "node {i} disagreed bit-for-bit between two independent builds"
            );
        }
    }

    #[test]
    fn cap_slopes_output_is_finite_everywhere() {
        let (graph, positions, heights) = small_graph_and_positions();
        let distances = receiver_distances_m(&graph, &positions);
        let (_t, _r, area, distance) = lowest_area_root_adjacent_node(&graph, &distances);
        let params = pathology_params(area, distance);
        let spiked = run_uncapped(&graph, &heights, &distances, &params);

        let corrected = cap_slopes(&graph, &spiked, &distances);
        let non_finite: Vec<usize> = corrected.iter().enumerate().filter(|(_, h)| !h.is_finite()).map(|(i, _)| i).collect();
        assert!(non_finite.is_empty(), "non-finite output at node indices {non_finite:?}");
    }

    #[test]
    fn cap_slopes_does_not_launder_a_nan_height() {
        // `erode_to_convergence`'s NaN assertion depends on a NaN height change surviving
        // through to `max_abs_height_change`, not being silently replaced by a finite,
        // cap-corrected value along the way. Checked on `cap_slopes` in isolation: a NaN
        // fed in at a draining node must come out NaN, not some finite `receiver + cap * d`.
        let (graph, positions, heights) = small_graph_and_positions();
        let distances = receiver_distances_m(&graph, &positions);
        let draining_node = (0..graph.node_count())
            .find(|&n| graph.downhill_of(n).is_some())
            .expect("fixture must contain at least one draining node");
        let mut poisoned = heights.clone();
        poisoned[draining_node as usize] = f64::NAN; // cast-ok: a node index into usize

        let corrected = cap_slopes(&graph, &poisoned, &distances);
        let idx = draining_node as usize; // cast-ok: a node index into usize
        assert!(
            corrected[idx].is_nan(),
            "cap_slopes replaced a NaN height with a finite value ({}), which would let a poisoned run \
             report success instead of the loud panic erode_to_convergence's NaN check expects",
            corrected[idx]
        );
    }

    // ---- the loop: erode_to_convergence never returns a slope above the cap ---------------

    #[test]
    fn erode_to_convergence_output_never_exceeds_the_slope_cap() {
        // The end-to-end claim: running the SAME pathological configuration through the
        // real convergence loop (which now caps every iteration, not just once at the end)
        // must not return a field with any edge above the cap, converged or not.
        let (graph, positions, heights) = small_graph_and_positions();
        let distances = receiver_distances_m(&graph, &positions);
        let (_t, _r, area, distance) = lowest_area_root_adjacent_node(&graph, &distances);
        let mut params = pathology_params(area, distance);
        // Unlike the raw pathology test above, this drives the real convergence loop, so
        // it needs a real threshold and an iteration cap generous enough to either converge
        // or report NotConverged honestly -- either is an acceptable result for this test,
        // which checks the CAP invariant, not convergence itself.
        params.max_height_change_per_step_m = 1.0e-3;
        params.max_iterations = 20_000;

        let (ErosionRun::Converged { heights: result, .. } | ErosionRun::NotConverged { heights: result, .. }) =
            erode_to_convergence(&graph, &heights, &distances, &params);

        let cap = slope_cap_tan();
        // See `slope_cap_violations`'s doc for why this compares rises rather than
        // re-deriving `(h_i - h_r) / d` with a bare `>` against `cap`.
        let violations = slope_cap_violations(&graph, &result, &distances, cap);
        assert!(violations.is_empty(), "erode_to_convergence returned edges above the cap: {violations:?}");

        let non_finite: Vec<usize> = result.iter().enumerate().filter(|(_, h)| !h.is_finite()).map(|(i, _)| i).collect();
        assert!(non_finite.is_empty(), "non-finite output at node indices {non_finite:?}");
    }

    // ---- ClampStats: the cap's own activity is a reported number, not an inference ---------

    #[test]
    fn slope_cap_clamped_count_counts_exactly_the_changed_bit_patterns() {
        let before = vec![1.0, 2.0, 3.0, f64::NAN];
        let mut after = before.clone();
        after[1] = 2.5; // a real change
                        // after[3] stays the exact same NaN bit pattern as before[3] -- must NOT be
                        // counted, since `!=` would wrongly count it (`NaN != NaN` is `true`).
        assert_eq!(slope_cap_clamped_count(&before, &after), 1);

        let unchanged = before.clone();
        assert_eq!(
            slope_cap_clamped_count(&before, &unchanged),
            0,
            "a NaN carried through unchanged must not be miscounted as clamped"
        );
    }

    #[test]
    fn erode_to_convergence_with_clamp_counts_agrees_with_the_plain_wrapper() {
        // `erode_to_convergence` must be exactly `erode_to_convergence_with_clamp_counts`
        // with the second element dropped -- not a second implementation that could drift
        // from it. Checked by running both on the same inputs and comparing the `ErosionRun`
        // half bit-for-bit.
        let (graph, positions, heights) = small_graph_and_positions();
        let distances = receiver_distances_m(&graph, &positions);
        let params = convergence_test_params(1.0e-3, 20_000);

        let plain = erode_to_convergence(&graph, &heights, &distances, &params);
        let (instrumented, _stats) = erode_to_convergence_with_clamp_counts(&graph, &heights, &distances, &params);
        assert_eq!(plain, instrumented);
    }

    #[test]
    fn erode_to_convergence_clamps_nothing_at_this_crates_default_test_constants() {
        // This is the answer to a question the sweep binary's own numbers raise but cannot
        // settle on their own: at `u = 1.0e-3 m/yr`, `k = 1.0e-6 /yr`, `dt = 1000 yr` (this
        // crate's own `default_test_params`/`convergence_test_params`), is Task 3's
        // convergence count UNCHANGED by this task because the cap runs and never binds, or
        // because it is not actually being exercised? Answered directly here: zero edges
        // clamped, over the entire run, on the same 300-node fixture the rest of this module
        // tests against. A steady-state slope of `u / (k * sqrt(A))` at these constants is
        // far below `tan(30 degrees)` for every node in this graph (see
        // `lowest_area_root_adjacent_node`'s doc and the pathology test above, which needs a
        // deliberately exaggerated `k` to ever reach the cap) -- this is the quantitative
        // reason, not a coincidence of this one run.
        let (graph, positions, heights) = small_graph_and_positions();
        let distances = receiver_distances_m(&graph, &positions);
        let params = convergence_test_params(1.0e-3, 20_000);

        let (result, stats) = erode_to_convergence_with_clamp_counts(&graph, &heights, &distances, &params);
        assert!(matches!(result, ErosionRun::Converged { .. }), "this fixture and cap are expected to converge");
        assert_eq!(
            stats,
            ClampStats { total_edges_clamped: 0, iterations_with_a_clamp: 0 },
            "the cap fired at this crate's own default test constants -- if this is now failing, the constants moved \
             into a regime where the cap actually matters, which is worth its own report, not a quiet test update"
        );
    }

    #[test]
    fn erode_to_convergence_with_clamp_counts_is_nonzero_under_the_pathology() {
        // The complementary case: under the SAME exaggerated (u, k) this module's pathology
        // test uses -- where the cap demonstrably needs to do something -- `ClampStats` must
        // say so with a nonzero count, not just report a plausible-looking `Converged`.
        let (graph, positions, heights) = small_graph_and_positions();
        let distances = receiver_distances_m(&graph, &positions);
        let (_t, _r, area, distance) = lowest_area_root_adjacent_node(&graph, &distances);
        let mut params = pathology_params(area, distance);
        params.max_height_change_per_step_m = 1.0e-3;
        params.max_iterations = 20_000;

        let (_result, stats) = erode_to_convergence_with_clamp_counts(&graph, &heights, &distances, &params);
        assert!(
            stats.total_edges_clamped > 0,
            "expected the cap to have corrected at least one edge under the pathological configuration, got {stats:?}"
        );
        assert!(stats.iterations_with_a_clamp > 0);
    }
}
