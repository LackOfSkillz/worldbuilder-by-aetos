# Water 2b: water in the ground — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rivers and notches cut the terrain they flow through, and a baked record can no longer be read against the wrong world.

**Architecture:** Two pieces, in this order because the first is small and closes a correctness hole while the second is large and consumes the thing the first protects.

1. **A ground fingerprint on the wire.** The record gains a digest of the ground it was baked from, and `decode` refuses a record whose digest does not match the world it is being read against.
2. **The water layer**, a new stage in `Surface` after features and before detail (spec §8.1): a trapezoid cut to `bed_m` along each refined reach, `width_m` wide at the bank with banks blended over one width either side; notches cut the same way; lake beds not cut. Its authority damps detail by `1 − authority` so texture cannot dam a river.

**The layer is an opt-in parameter block**, in the idiom of `relief`, `tectonics`, `coast`, `gully` and `peaks`. Absent, every generated value is bit-identical to today.

**Spec:** `docs/superpowers/specs/2026-09-10-automatic-water-design.md` — §7 (the record), §8.1 (the layer), §8.2 (the index and its performance target).

**Carry-forward this plan discharges:** `docs/superpowers/reports/2026-09-10-water-1a-carry-forward.md`, "Routed to plan 2b" items 1, 2, 3 and 4, plus §8.2's performance target which becomes live the moment the carve lands.

## Global Constraints

- **An absent block must produce bit-identical worlds.** The parity corpus reports 156,011 compared / 0 divergent and every control unmoved: seed 147,387, erosion-k 216, water-pond 60, tectonic-warp 22,995. If that cannot pass, the design is wrong, not the test.
- **`GENERATOR_VERSION` must NOT be bumped.** Its bump test (`lib.rs:65-71`) is "the same seed *and the same parameters* through the new code would produce a different world." With the block absent it would not. **This is the whole reason the layer is opt-in** — see the ruling in Task 7.
- Determinism is the product. No `std` float maths outside `detmath.rs`; no `ceil` (write `-m::floor(-x)`); no `f64::min`/`max`/`clamp` (NaN-asymmetric, and the static guard does not catch them); `total_cmp` for float sorts; never iterate a `HashMap`/`HashSet` into output.
- **No `.abs()` in `src/` or `examples/`.** `tests/no_std_math.rs` holds a per-file ledger and counts may only fall. Use the ledger test to check, never a grep — a naive grep over `tectonics.rs` reports ten where the ledger says nine, because one hit is prose inside a comment.
- `// cast-ok: <reason>` on every `as u32/u64/i32/i64` and float-to-`usize` cast, and the reason must say what makes it safe.
- No panic reachable from an `extern "C"` boundary — wasm exports return status codes.
- Four places encode the record's layout and must agree word for word: `src/hydrology/record.rs`, `viewer/public/app/engine.js::hydroSummary`, `viewer/public/app/water-preview.js::decodeHydro`, `crates/worldbuilder-engine/tests/wasm_exports.rs`.
- Every figure in a report names its population, its method with parameters, and its host, and is obtained by running rather than transcribing.
- **Pins live in two places, and a task that moves a count updates both.** A verification report is the record; `.github/workflows/gates.yml` is the gate, and it asserts exact counts — the engine's run count per configuration (`expect:` at the five matrix rows, currently 865/865/867/987/989 with 11 ignored) and the Python suite's `--expect-total` (currently 576, conformance 167). The islands slice re-derived every pin into its report and never touched this file, and both of its PRs went red on a green branch. Re-derive with `cargo test -p worldbuilder-engine <cfg> -- --list` (and `--ignored`), summing each binary's trailer, and `pytest tests/ -q --collect-only`; never by adding to the old number.
- Commit subjects carry no franchise or third-party proper nouns; the commit feed is published publicly via a webhook.

---

### Task 1: The ground fingerprint

**Files:**
- Modify: `crates/worldbuilder-engine/src/hydrology/record.rs`
- Test: in-file `#[cfg(test)]`

**Interfaces:**
- Produces: `pub fn ground_fingerprint(surface: &Surface) -> [u8; 16]`, and `SCHEMA` 6 → **7** with the digest in the header.

**Why this exists, and it is not cosmetic.** The carry-forward calls it the highest-value item on plan 2b's list. Today a bake queried against a genuinely different world of the same radius "answers that world's ground against this record's levels — a wrong answer, not an error — and nothing can check it, because nothing on the wire ties a record to a world." Both wasm exports and `engine.js` say so in as many words, and Ruling Q-20 is waiting on it.

**Sign the ground, not the parameters.** A world has a seed, but a caller may hold a `Surface` built from parameters the record never recorded, and the engine has already been bitten by a declared-but-unchecked field: the record carries `OFF_RADIUS_M` in its header and `decode` compares only the format and generator versions, so a record baked at 9,309 km loads silently against a 6,371 km world. Declaring is what we already do and it does not work. So sample the ground.

**Sample `structural_m`, not `elevation_m`, and this is load-bearing.** The water layer in Task 3 *changes* `elevation_m`. A digest over `elevation_m` would therefore depend on whether the layer is active, which makes the record's own fingerprint depend on the record — circular. `structural_m` is defined before detail and before the layer, so it is the ground the bake was actually computed from.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn two_worlds_that_differ_anywhere_get_different_fingerprints() {
    // The property the whole guard rests on. A seed change, a plate change, a land
    // change and a radius change must each move the digest.
    let base = fingerprint_of(SEED, RADIUS_M, PLATES, LAND);
    assert_ne!(base, fingerprint_of(SEED + 1, RADIUS_M, PLATES, LAND), "seed");
    assert_ne!(base, fingerprint_of(SEED, 6_371_000.0, PLATES, LAND), "radius");
    assert_ne!(base, fingerprint_of(SEED, RADIUS_M, PLATES + 1, LAND), "plates");
    assert_ne!(base, fingerprint_of(SEED, RADIUS_M, PLATES, LAND + 0.05), "land");
}

#[test]
fn the_same_world_fingerprints_the_same_every_time() {
    let once = fingerprint_of(SEED, RADIUS_M, PLATES, LAND);
    for _ in 0..8 {
        assert_eq!(once, fingerprint_of(SEED, RADIUS_M, PLATES, LAND));
    }
}

#[test]
fn every_probe_moves_when_the_radius_does_so_the_margin_is_not_one_lucky_point() {
    // Measured on the maritime side of this project at a 9,309 km -> 6,371 km change:
    // all 64 probes moved past a millimetre rounding, minimum 0.44 m. A digest that
    // differs because ONE probe moved is a digest one interpolation change from
    // colliding, so assert the margin rather than the inequality.
    let wide = surface_at(RADIUS_M);
    let narrow = surface_at(6_371_000.0);
    let mut moved = 0usize;
    let mut smallest = f64::INFINITY;
    for index in 0..PROBE_COUNT {
        let point = probe_point(index);
        let a = wide.structural_m(&point);
        let b = narrow.structural_m(&point);
        let gap = if a > b { a - b } else { b - a };
        if gap >= 0.001 { moved += 1; }
        if gap < smallest { smallest = gap; }
    }
    assert_eq!(moved, PROBE_COUNT, "only {moved} of {PROBE_COUNT} probes moved");
    assert!(smallest > 0.001, "smallest probe movement {smallest} m is at the rounding floor");
}

#[test]
fn a_probe_point_is_the_same_on_every_machine() {
    // The scatter must come from integer arithmetic, not a float sequence, or two
    // hosts disagree and every record looks foreign. Pin the first three exactly.
    let first = probe_point(0);
    assert_eq!(first.vector.x.to_bits(), /* implementer pins from a run */ 0);
}
```

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo test --release -p worldbuilder-engine --lib hydrology::record -- fingerprint`
Expected: compile failure — none of these exist.

- [ ] **Step 3: Write the fingerprint**

Design, to be followed rather than reinvented:

- **A fixed, irregular scatter from integer arithmetic.** A regular grid can agree between two worlds that happen to share a shelf; a float sequence can differ between hosts. Derive each probe from a cheap integer hash of its index, exactly as maritime's `bake.fingerprint` does, then map to a sphere point. `PROBE_COUNT = 64` is the measured-sufficient figure.
- **Hash with `blake2`**, which this crate already vendors — see the `blake2_bytes` test suite. Not a hand-rolled mix.
- **Round each sample before hashing** (millimetres), so a bit-level difference below any physical meaning does not make a record foreign.
- **Sample `structural_m`.** See the reasoning above; a comment must state it, because sampling `elevation_m` looks more natural and is wrong.

- [ ] **Step 4: Put it in the header and bump the schema**

`SCHEMA` 6 → 7, digest in the header. Update all four twins listed in the Global Constraints — `record.rs`, `engine.js::hydroSummary`, `water-preview.js::decodeHydro`, `tests/wasm_exports.rs` — and say in the report which word offsets moved.

- [ ] **Step 5: Run the suites and commit**

Run `cargo test --release -p worldbuilder-engine --features wasm` and the viewer suite.

```bash
git add crates/worldbuilder-engine/src/hydrology/record.rs viewer/public/app
git commit -m "Sign a record with the ground it was baked from"
```

---

### Task 2: Refuse a record from another world

**Files:**
- Modify: `crates/worldbuilder-engine/src/hydrology/record.rs` (`decode`), `src/wasm.rs`, `src/bindings.rs`, `viewer/public/app/engine.js`

**Interfaces:**
- Consumes: Task 1's `ground_fingerprint`.
- Produces: a decode refusal, a wasm status code for it, and the same refusal through the Python door.

- [ ] **Step 1: Write the failing tests**

The test that matters: bake a record on world A, then read it against world B of **the same radius**, and require a refusal rather than an answer. That exact case is the hole this task closes, and a radius change is *not* a sufficient test of it — the header already carries the radius, and nothing compares it.

Also pin: a record read against its own world is accepted; the refusal is a status code and never a panic across the boundary; and the three doors (native, wasm, Python) agree about the same record.

- [ ] **Step 2: Run, watch fail, implement, run again**

Expected first: the mismatched record is accepted and answers, which is the bug.

- [ ] **Step 3: Commit**

```bash
git commit -m "A record from another world is refused, not answered"
```

---

### Task 3: The water layer

**Files:**
- Create: `crates/worldbuilder-engine/src/water/layer.rs`
- Modify: `crates/worldbuilder-engine/src/surface.rs`, `src/water/mod.rs`

**Interfaces:**
- Consumes: `WaterIndex::candidates` from plan 2a — **this is what makes the carve affordable**, and it is why 2a built an index before anything needed one.
- Produces: `WaterLayer::cut_m(point, ground_m) -> (f64, f64)` returning the cut ground and the layer's authority, and `Option<WaterParams>` threaded through a new `Surface::with_water` constructor.

**The layer is a second phase, never the bake's own input — added at pre-flight, and it is the load-bearing architecture of this plan.** The bake *reads* `elevation_m`: the pond search samples it (`hydrology/ponds.rs`, Ruling Q-16 — ponds are found in the detail field). The layer *writes* `elevation_m`. A bake run on a surface whose layer is active would therefore find ponds in terrain shaped by its own output, which is circular and has no fixed point to converge to. So:

1. **Bake on a surface without the layer.** Always. `Surface::with_water` must never be the surface a bake is computed from, and a test should make that impossible rather than merely documented.
2. **Carve by joining a record to a world.** A carved world is built from the same parameters as the bare one, plus the water block, plus the record. Because the layer changes `elevation_m` and never `structural_m`, the carved world and the bare world it was baked from **fingerprint identically** — which is precisely why Task 1 samples `structural_m`.
3. **The join is where Task 2's refusal lives.** Joining a record to a world whose fingerprint differs is refused there, once, rather than checked per sample.

The layer therefore carries the record, not a handful of scalars — unlike every earlier block. Say in the report how the record reaches `Surface` (owned, shared, or referenced by a held bake's id) and what that costs in memory, because a 7 MB record per world handle is a real number on the owner's world.

**Spec §8.1, quoted so nobody has to go and look:**
- River channels: a trapezoid cut to `bed_m` along each refined reach, `width_m` wide at the bank, banks blended over one width either side.
- Notches: cut the same way.
- **Lake beds: not cut.** A kept lake is an existing hollow; its outline and level decide the water surface.

**Three things to get right, each of which has already bitten this project:**

1. **Carry the layer's authority out, do not apply damping here.** `Surface::elevation_m` owns the composition, and `Features::apply` already returns `(shaped, authority)` for exactly this reason. Task 4 consumes the authority.
2. **A cut may only ever lower ground.** Expect a mouth's bed to be able to *rise* at the last step — that is Ruling R-4's I4 side effect and it is known. Clamp so the layer never raises, and pin it, because a raising "cut" is a dam.
3. **The absent path must be an early return, not a zero cut.** `-0.0 + 0.0` is `+0.0` and flips a sign bit; the islands slice hit exactly this and the gate is what saved it.

- [ ] **Step 1: Write the failing tests**

- a point in mid-channel sits at `bed_m`, exactly;
- a point one width beyond the bank is untouched, bit-for-bit;
- the blend between them is monotone and has no step exceeding the analytic bound — **derive that bound from `bed_m`, `width_m` and the step, in the test, rather than hard-coding it**; a hard-coded continuity bound went stale twice in the islands slice when a constant moved;
- a lake's interior is **not** cut, which is the spec's explicit exception and the easiest thing to get wrong by symmetry with reaches;
- the layer never raises ground anywhere, over an area-uniform sweep;
- an absent block leaves `structural_m` and `elevation_m` bit-identical.

- [ ] **Step 2: Run, watch fail, implement, run again**

- [ ] **Step 3: Commit**

```bash
git commit -m "A river cuts the ground it runs through"
```

---

### Task 4: The detail damping

**Files:**
- Modify: `crates/worldbuilder-engine/src/surface.rs`

**It ships with Task 3 or not at all.** The carry-forward is explicit: "a carved channel with undamped detail is the failure the damping exists to prevent." Do not merge Task 3 without this.

- [ ] **Step 1: Write the failing test**

Detail amplitude inside a channel must be multiplied by `1 − authority`, exactly as `Features::apply`'s result is at `surface.rs:402`. The test that matters is behavioural, not structural: **over a channel's length, no detail sample may rise above the water level** — that is what "texture cannot dam a river" means, and it is the property, where "amplitude was multiplied" is only the mechanism.

- [ ] **Step 2: Run, watch fail, implement, run again**

- [ ] **Step 3: Commit**

```bash
git commit -m "Texture defers to a channel instead of damming it"
```

---

### Task 4b: A pond a channel drains is not a pond

*Added mid-plan, from what Task 3's lake-bed fix exposed. Rulings C-16 to C-19 in the ledger.*

**Files:**
- Modify: `crates/worldbuilder-engine/src/hydrology/ponds.rs` (the keep rule), and the tests beside it

**What happened.** Task 3's review found the carve cutting lake beds; the fix stopped the layer cutting any point a body claims. That turned one case from a pit into a **dam**. At 30k nodes a fine-found pond sits at 342.8 m while a reach through its outline has its bed at 135 m. That reach is a **notch** — a cut through a ridge that keeps the drainage continuous — and the pond sits on the ridge top. Cut through, the pond had a 190 m pit in its floor; left uncut, the pond's footprint now blocks the notch and the river runs into a 190 m wall across its own channel.

**The cause is the record, not the carve.** The bake finds that pond in bare terrain, where it is a genuine hollow. But the record describes the **carved** world: a notch is a cut the record says will be made, which is why even plan 2a's bare-world query already answers River at points whose ground stands 200 m above the water. In the carved world the notch drains the pond, so the pond does not exist.

**The fix: a fine-found body whose outline is crossed by a reach or notch whose water level lies below the body's own level is not kept.** It is not a closed hollow once the cut it sits on is made.

- Decide "crossed" with the same geometry the query and the carve use — `claim_bodies` and `along_leg` — never a third copy (Ruling C-12).
- State the tolerance the level comparison uses, and why: a river flowing *through* a pond at the pond's own level is a pond on a river, not a drained pond, and must survive.
- **Coarse lakes are out of scope for the drop.** Ruling C-17: measure, on the owner's world and both parity worlds, how far off each coarse lake's level its inflowing reaches arrive, separated from the pond case above, and report it. Rule afterwards with the numbers.
- **Ruling C-19:** add the fixture Task 4 lacked — a feature over a channel — so the multiplicative damping `(1 − a_f)(1 − a_w)` is pinned against the additive form, which currently passes every test.

**Tests:** the 30k and 60k bakes, where reaches do cross bodies: after the fix, **no sample along any channel stands above the water the query reports there**, which is the dam test, and it must be shown failing against the commit before this task. Also: a pond a river flows through at its own level survives.

**This moves hydro parity values legitimately** — fewer bodies. Divergent must stay 0; report which groups' compared values moved and why, and do not describe it as a regression. The `water-pond` control may move too: say by how much and confirm it moves only through the dropped bodies.

```bash
git commit -m "A pond a channel drains is not a pond"
```

---

### Task 5: The wasm door, and its pins

*Added at pre-flight. The draft plan had no ABI task, yet Task 7's parity group needs a wasm door to bit-compare the carve across the boundary, and Task 8 asks whether the carve is visible in the studio.*

**Files:**
- Modify: `crates/worldbuilder-engine/src/wasm.rs`, `crates/worldbuilder-engine/tests/wasm_exports.rs`

**The peaks block is the template** — islands Tasks 4 and 5, merged in #34: a stride, preset selectors, per-field domains, `water_is_admissible` built on `within()`, `decode_water`/`encode_water` in one place, an arg enum, a reader, and three exports `wb_world_new_water` / `wb_water_preset` / `wb_water_check`, entered in `WB_EXPORTS` (whose count self-validates in `wasm_exports.rs`, so there is no number to bump there).

**Two things peaks did not have to face:**

1. **The door takes a record, not only scalars.** Per Task 3's architecture, `wb_world_new_water` builds a world from the usual parameters plus the water block **plus a held bake** — reference it by the id `hydroHold` already issues (plan 2a) rather than copying 7 MB across the boundary. A stale or unknown id is a status code, not a panic.
2. **The fingerprint refusal is a status code at this door.** A bake whose fingerprint differs from the world being built is refused with its own named status, distinct from `WB_ERR_PARAM`, so a host can tell "your block is malformed" from "that record belongs to another world". Pin that the checker and the constructor agree on it.

Pin, in `wasm_exports.rs`, everything peaks pinned — and the two things this task adds: **a record from another world of the same radius is refused at the door**, and **a canonical water block with no reaches produces a world bit-identical to `wb_world_new`**. Verify a field swap fails a test by swapping two slots in `decode_water`, watching it fail, and reverting — two of peaks' fields shared a domain and made a swap invisible in two of three mirrors, which is the failure this is guarding.

```bash
git commit -m "A door that joins a record to its own world, and refuses any other"
```

---

### Task 6: The studio

*Added at pre-flight, for the same reason as Task 5.*

**Files:**
- Create: `viewer/public/app/water-params.js`, `viewer/test/water-params.test.mjs`
- Modify: `viewer/public/app/engine.js`, `viewer/public/app/controls.js`, `viewer/public/app/main.js`

**`peak-params.js` is the template**, including the two things its review had to fix: pin `WATER_FIELDS` order against the Rust struct's own declared order rather than against itself, and make any refusal the engine can issue *reachable and correctly named* — `peakBootPlan()` checks a record before the world build so a refusal is surfaced rather than swallowed as "engine unavailable". Here there are two refusals to surface: a malformed block, and a record from another world.

**An untouched panel writes no water parameter at all**, so a reload takes the absent path and a saved world is bit-identical. **Preset values come from `wb_water_preset`, never transcribed into JS** — and no preset value is written into a comment either, since that is how the islands slice's stale `11%` and `"0.36"` happened.

```bash
git commit -m "Let the studio carve a river, and say why when it will not"
```

---

### Task 7: The version decision, and the parity gate

**Files:**
- Modify: `crates/worldbuilder-engine/src/lib.rs` (only if the decision is to bump — it is not), `crates/worldbuilder-engine/parity/parity_dump.rs`

**The decision, taken here and recorded:** **`GENERATOR_VERSION` is not bumped.** The carry-forward left it "untaken" on the grounds that §8.1's carve changes `elevation_m`, which is what the version exists to describe. That reasoning holds only if the carve is unconditional. It is not: the layer is an opt-in block, so a world built from the same seed and the same parameters through the new code is bit-identical, and `lib.rs:65-71`'s bump test is not met. A bump becomes correct the day the block's default changes, and that is a separate, deliberate act which invalidates every saved world — the same shape as the islands slice, where the owner chose opt-in for exactly this reason.

- [ ] **Step 1: Prove the corpus did not move**

```bash
cargo run --release -p worldbuilder-engine --example parity_dump --features wasm > /tmp/native.txt
cd crates/worldbuilder-engine/parity && node parity.mjs /tmp/native.txt
```
Expected: `156011 values compared, 0 divergent`, and the four controls at their existing figures.

**If anything moved, stop.** It means the layer reaches the canonical path, which is a defect and not a new pin.

- [ ] **Step 2: Add a parity group that exercises the layer**

A group with the block *present*, so the carve itself is bit-compared native against wasm. Mirror `water_at/plain` from plan 2a.

- [ ] **Step 3: Commit**

```bash
git commit -m "Pin the carve across the boundary, and leave the version alone"
```

---

### Task 8: The performance target, the survey, and the report

**Files:**
- Create: `docs/superpowers/reports/2026-09-14-water-2b-verification.md`

**§8.2's performance target becomes live with this plan and this plan owns measuring it:** `elevation_m` no more than 20% slower. Plan 2a could not test it because it added no stage to `Surface`; Task 3 adds one.

- [ ] **Step 1: Measure it**

`elevation_m` with the block absent against with it present, on the owner's world, enough samples that the figure is not noise. State the population, the method with parameters, and the host. If the target is missed, say by how much rather than reporting a pass — and note that plan 2a ledgered two known costs that land here: the index is 98% empty `Vec` headers (about 20 MB holding 439 KB of entries) and `inside_ring` allocates a `Vec` and walks the ring three times per sample (Ruling Q-23). Either may be what stands between the layer and the target, and both were deferred to this plan.

- [ ] **Step 2: Bake the owner's world and look at it**

`worlds/world-1788998299904.json`, 1M nodes, its own 86,000 wetness nodes, forced outlet at 0°N 0°E. Report the great lake, the reach that drains it, and whether the carve is visible in the studio. Pay particular attention to the inland sea, which the owner has said must survive.

- [ ] **Step 3: Write the report and re-derive every pin by running it**

Engine, Python, conformance, viewer, parity and all four controls, `check:wasm`, and the EOL guard. Never transcribe a figure. **Then move `.github/workflows/gates.yml` to match** — the five engine `expect:` rows and the Python `--expect-total` — re-derived per the Global Constraints, with a dated comment saying what moved and why. Every earlier task that adds tests moves these counts; this is the task that makes the gate agree with the record before the branch is pushed.

- [ ] **Step 4: Commit**

```bash
git commit -m "Water 2b: the carve measured, and the owner's world sounded"
```
