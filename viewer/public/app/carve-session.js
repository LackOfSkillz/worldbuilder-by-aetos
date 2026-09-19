//! The carve, run: bake the bare world FOR CARVING, hold the bake, and build carved worlds from
//! it -- and when the engine refuses, say which refusal it was. **No DOM and no Cesium**: it takes
//! an `Engine`, a `build` callback and a `wait` hook, so `water-params.test.mjs` drives this exact
//! code against the shipped `.wasm` in node, and `main.js` drives it in the studio. The panel only
//! prints what this returns.
//
// # Why this is its own module
//
// The islands panel's review found its refusal note in a code path a failed `boot()` swallowed,
// so the owner saw "engine unavailable" for a block the engine had named. The fix there was
// `peakBootPlan`, a pure function main.js called before the build. The carve has FOUR refusals,
// three of which cannot be known before a build (a record belongs to a world, or was baked for
// carving, only as far as the door can judge with both in hand), so the equivalent here is not a
// pre-check but an outcome: `carve()` never throws for an engine refusal. It returns
// `{ carved: false, refusal }` with the refusal named, and the caller draws the bare world. A test
// can then hold "each refusal reaches the owner by name" against the same function the studio
// calls, rather than against a copy of it.
//
// # What is slow, and what is kept off the slider path
//
// - **The bake is slow**: over a minute on the owner's world, on the main thread, because the
//   carved world must live in the same engine instance as the held bake (an id means nothing in
//   another instance, and no export loads a record into one). `wait(message)` is called before
//   every blocking bake so the studio can paint the wait first.
// - **A bank-width drag never bakes.** The held bake is reused for every block; only a new carved
//   world is built from it, which costs a `Surface` and a pointer (the record and index are one
//   shared `Arc`, Task 5). A new bake happens only when there is none, or when the engine says the
//   held one is from another world (`WB_ERR_WRONG_WORLD`, i.e. the ground changed).
// - **`wb_water_check` is never called on a drag.** It is called after a refusal, to name it, and
//   once before a bake that a malformed block would otherwise waste (a minute, for a typo) -- where
//   a malformed block costs nothing and an admissible one one bare `Surface` with no bake and no
//   index.

import { decodeHydro } from "./water-preview.js";
import { carveRefusal, pondChange, pondChangeText } from "./water-params.js";

const WB_ERR_BUFFER = 2;
const WB_ERR_PARAM = 5;
const WB_ERR_WRONG_WORLD = 8;

export class CarveSession {
  /// `engine`: the `Engine` the carved worlds live in. `params`: the ordinary hydro-bake params
  /// (the studio passes the water preview's, so the carve is the record the preview draws);
  /// `forCarving` is added here and nowhere else. `ordinaryBake(params)`: an async function that
  /// bakes the SAME ground without the flag and resolves to its words -- the studio hands it to a
  /// pool worker so it runs beside the main-thread bake; `null` bakes it here, after the carve's
  /// own. `wait(message)`: awaited before every blocking bake.
  constructor(engine, { params, ordinaryBake = null, wait = async () => {} }) {
    this.engine = engine;
    this.params = params;
    this.ordinaryBake = ordinaryBake;
    this.wait = wait;
    /// The held bake for carving, `{ id, handle, words }`, or `null`.
    this.held = null;
    /// What the held bake did to the ponds, `pondChange`'s shape, or `null`.
    this.ponds = null;
    /// Counters a test (and the status line) read: how many bakes, how many checker calls.
    this.bakes = 0;
    this.checks = 0;
    /// The last outcome, for the panel.
    this.last = null;
  }

  /// Carve `spec`'s world with the block `water`, baking on the bare world `bare` if needed, and
  /// build it through `build(carvedSpec)` -- which must throw the engine's `Error` (with
  /// `.status`) on a refusal. `water === null` releases nothing and builds nothing: the caller
  /// draws the bare world.
  ///
  /// Returns `{ carved, refusal, notes, ponds, baked, bakeMs }`. `refusal` is the one that stopped
  /// the carve (`carveRefusal`'s `{ status, name, text }`), or `null`. `notes` are refusals that
  /// were met and dealt with -- a `WB_ERR_WRONG_WORLD` followed by a re-bake -- named, because a
  /// minute's wait nobody explains looks like a hang.
  async carve({ spec, water, bare, build }) {
    const outcome = { carved: false, refusal: null, notes: [], ponds: this.ponds, baked: false, bakeMs: 0 };
    this.last = outcome;
    if (water === null) return outcome;
    try {
      if (this.held === null) {
        // Before a bake a malformed block would waste: judged before the world is looked at. Only
        // the world's four scalars go with it -- the other blocks are read BEFORE the water block,
        // so leaving them out is what makes a `WB_ERR_PARAM` here the water block's and nobody
        // else's. (The scalars built the bare world already, so they are admissible.)
        const { seed, radiusM, plateCount, landFraction } = spec;
        const status = this.check({ seed, radiusM, plateCount, landFraction, water, bake: null });
        if (status === WB_ERR_PARAM || status === WB_ERR_BUFFER) {
          outcome.refusal = carveRefusal(status);
          return outcome;
        }
        await this.bake(spec, bare, outcome);
      }
      try {
        build({ ...spec, water, bake: this.held });
      } catch (error) {
        if (error.status !== WB_ERR_WRONG_WORLD || outcome.baked) throw error;
        // The held bake is from other ground: the world changed since it was baked. Named, then
        // re-baked -- the one refusal the studio answers itself, because its answer is certain.
        outcome.notes.push(carveRefusal(WB_ERR_WRONG_WORLD));
        this.release();
        await this.bake(spec, bare, outcome);
        build({ ...spec, water, bake: this.held });
      }
      outcome.carved = true;
      outcome.ponds = this.ponds;
      return outcome;
    } catch (error) {
      // An engine refusal carries its status: `newWorld` asked `wb_water_check` for it after the
      // handle of 0, and `hydroHold` read it off `wb_hydro_bake`. Anything without one is not a
      // refusal and is not this function's to explain.
      if (typeof error.status !== "number") throw error;
      outcome.refusal = carveRefusal(error.status);
      return outcome;
    }
  }

  /// `wb_water_check`, counted. A whole world build: see the module header for when it is called.
  check(spec) {
    this.checks += 1;
    return this.engine.checkWater(spec);
  }

  /// Bake `bare` for carving and hold it; count what it did to the ponds against an ordinary bake
  /// of the same ground and params. Throws the engine's `Error` (with `.status`) on a refusal --
  /// `WB_ERR_CARVED` if `bare` is in fact a carved world.
  async bake(spec, bare, outcome) {
    await this.wait("baking the rivers for carving - over a minute on a large world, and the page "
      + "will not respond until it is done");
    // Started FIRST, so a worker runs the ordinary bake beside the main thread's carve bake.
    const ordinaryJob = this.ordinaryBake ? this.ordinaryBake(this.params) : null;
    const started = performance.now();
    const bake = this.engine.hydroHold({ handle: bare, params: { ...this.params, forCarving: true } });
    this.bakes += 1;
    let ordinaryWords;
    try {
      ordinaryWords = ordinaryJob
        ? await ordinaryJob
        : this.engine.hydroBake({ handle: bare, params: this.params });
    } catch (error) {
      this.engine.hydroFree(bake);
      throw error;
    }
    this.held = bake;
    this.ponds = pondChange(decodeHydro(ordinaryWords), decodeHydro(bake.words));
    outcome.baked = true;
    outcome.bakeMs = performance.now() - started;
    outcome.ponds = this.ponds;
    return bake;
  }

  /// Free the held bake, if any. A carved world already built from it keeps its own share of the
  /// record (Task 5), so this never un-carves what is drawn.
  release() {
    if (this.held !== null) {
      this.engine.hydroFree(this.held);
      this.held = null;
      this.ponds = null;
    }
  }
}

/// The owner-facing text for an outcome: the refusal, the notes met on the way, and the pond
/// account -- **every part of it named**, so the panel prints this and adds nothing of its own.
export function carveOutcomeText(outcome) {
  if (outcome === null) return "";
  const parts = outcome.notes.map((note) => `${note.text} Re-baked for carving.`);
  if (outcome.refusal) {
    parts.push(`${outcome.refusal.text} The bare world is drawn.`);
  } else if (outcome.carved) {
    if (outcome.baked) parts.push(`baked for carving in ${(outcome.bakeMs / 1000).toFixed(1)} s.`);
    if (outcome.ponds) parts.push(pondChangeText(outcome.ponds));
  }
  return parts.join(" ");
}
