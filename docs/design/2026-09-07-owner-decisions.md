# Owner decisions, 2026-09-07

Three questions put to the owner directly, with the state of play behind each.

## 1. After climate: THE EVENNIA ROUND TRIP

**Chosen over finishing climate, the studio, and hardening.**

This is the project's actual purpose and **none of it exists**. What does exist is both ends:

- **The engine** answers `f(SpherePoint)` for elevation, water, temperature and moisture.
- **`worldbuilder/integration/maritime.py` already turns a local position into a global one.**
  `WorldbuilderTerrain(surface, region, anchor)` builds a `TangentFrame`, and `point_at` converts maritime
  x/y to a `SpherePoint`. Maritime imports nothing from Worldbuilder; the adapter fits itself to the
  interface maritime already declares.
- **`Region`** already carries a name, an anchor and orientation, a reach and its features -- **so an area's
  placement is already the shape this needs.**

**The missing middle is the reader, the placement, the export, and the port mapping.** No code reads the
dragonsire fixture; both adapter paths (`explicit_adapter` against zone attributes, `graph_inference` against
an exit graph) are designed and neither has ever been run. No worldfile writer exists -- the word appears in
this codebase only in the stream-graph format, which is unrelated.

**The minimum that works is not the studio.** Read area names -> assign each an anchor and bearing by hand ->
write the mapping -> maritime builds a frame per region. **A studio is for choosing anchors visually, which
matters at fifty areas and not at five, and the hand-authored version has never been tried.**

**Blocking prerequisite, stated in `2026-09-06-contrib-shape.md` and unchanged:** the worldfile becomes a
**public interface** the moment a contrib reads it -- versioned schema, documented semantics per field, and a
fail-closed policy for meeting a newer generator. **That is the whole cost of the contrib decision and it is
cheap now and expensive later.** `2026-09-07-port-access.md` adds the first four concrete fields to it.

## 2. Drainage: FIX THE RESOLUTION FADE ONLY

**Chosen over stopping, changing the metric, and dropping it.**

Three approaches have been tried. **Erosion is dead** -- the stream graph is **661x too coarse** (50,500 m
node spacing against a 76 m post) and the valleys wanted are a 0.5-2 km feature. **The comb had the wrong
topology** -- 0 merges and 0 splits in 319 contour-row pairs. **The merging harmonic works**: counts fall
34% and 31% downhill while widths rise 90% and 74%, and the previously-dead site now branches.

**But the wide view got worse each round.** At 6 km it is the best the kernel has looked; at 25 km it is a
finer stipple than the comb's corduroy.

**The fade is the right lever because it is the only view that regressed.** Make the term suppress harder at
coarse resolution so 25 km never sees it. **One clear success test, no new shaping.**

**The deeper observation stands and is recorded rather than acted on:** the confluence metric rewards finer
grain, and grain is what the eye reads. **Three rounds each improved the numbers and worsened the wide
view.** If the fade does not settle it, that divergence -- not the shaping -- is what needs attention.

## 3. Merging: ALL AS ONE

**Chosen over landing bottom-up and over waiting.** This supersedes roadmap section 8.2, which recorded a
one-PR-at-a-time sequence.

The branch is **300+ commits ahead of `master`** and nothing has merged. **The trade is explicit: this
abandons the per-slice review record in exchange for landing the work.** Recorded here so the choice is
legible later rather than looking like drift.

**Sequencing constraint:** switching branches changes the working tree, so **the merge cannot happen while an
agent is working in it.** It waits for the running task, and the tree must be clean apart from the three
known paths -- `worldbuilder/integration/maritime.py` (pre-existing, unstaged), untracked `.claude/`, and
untracked `crates/worldbuilder-engine/parity/native.txt`.
