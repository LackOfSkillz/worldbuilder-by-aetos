# Post-MVP — move area generation into the studio

Recorded 2026-09-08 from a question asked while watching a populate run. **Not a spec
amendment and not approved for implementation.** It records where the work happens today,
why, and what would have to change to move it.

---

## The question

> "Are you rebuilding these areas ahead of time and then Worldbuilder just adds them to the
> map? It's OK if you do, but somewhere down the road I imagine Worldbuilder would actually
> do the job of actually building the areas."

## Where the work happens today

Nothing is pre-built and nothing is cached between runs. Pressing **populate world** spawns
`evennia_roundtrip.generate` and every area is computed from scratch in that process: the
sites are surveyed against the live oracle, the lattices are grown, the prose is written,
the gates are run, the roads are laid. A hundred and twenty-five areas takes about seven
seconds.

What *is* deliberately delayed is the **reveal**. The generator writes one line per area as
it lands and the viewer paces those onto the globe over about a minute, because seven
seconds is faster than anybody can watch. So the pins do lag the computation — by up to a
minute — but they are drawing work that was done in the same run, moments earlier, not work
baked in advance.

The split is:

| what | where | why |
|---|---|---|
| terrain, water, climate | Rust engine, in the browser via WASM | point-evaluable, needs to answer a tile in microseconds |
| area generation | Python, on the server | reads the same engine through pyo3; the building laws and linters live here |
| drawing, painting, watching | the viewer | it is the only part a person touches |

## Why it is split that way now

The building laws, the prose bands, the culture table and the linters are Python and
already existed. Reimplementing them in the viewer to generate in the browser would mean two
implementations of the same rules, which is the failure this project keeps naming: two
answers to one question.

## What moving it would take

1. **The laws would have to move or be shared.** `area_lint`, `prose_lint` and the culture
   table are the specification, not decoration. Either they move to the engine and Python
   calls them, or they stay and the studio calls out to them — but there must remain exactly
   one implementation.
2. **The oracle is already in both places.** The engine answers elevation in the browser and
   through pyo3 in Python, so terrain questions are not the obstacle.
3. **Water is not.** `wb_water_run` is exported to WebAssembly and has no pyo3 binding at
   all, which is why lakes reach the generator through a `water` block in the worldfile
   rather than by asking. That gap points the same way: one engine, two surfaces, and the
   surfaces are not level.
4. **The run record would need a home.** `runs/` gives every generation an immutable
   directory with its manifest, its feed and its worldfile, and rollback moves a pointer.
   Whatever generates, that guarantee has to survive.

## The honest summary

Today: the studio commissions the work and shows it; Python does it.
Later: the studio could do it, once the laws have one home and the engine's two surfaces
carry the same functions.

Neither is required for the MVP. The current split produces a world that passes every gate
and a run that can be rolled back, which is what the MVP is for.
