# What ships as an Evennia contrib: the glue, not the engine

The owner's proposal, and it is the right shape:

> "your evennia contrib would consist of like the glue needed to integrate your rust project into an
> evennia game"

**This is already the recorded decision, arrived at from a different direction.** Roadmap section 3.1 says,
of climate tags: *"Evennia receives concrete tags and never needs the engine at runtime."* The proposal
generalises that from tags to the whole integration, and generalising it is correct.

---

## Why a contrib cannot be the engine

Evennia contribs are **pure Python, in Evennia's own repository, under its licence, reviewed by its
maintainers**. A contrib that vendored a Rust crate, a wasm artifact, or a `maturin`-built wheel would ask
Evennia's maintainers to take on:

- a **build toolchain** they do not have (a Rust compiler, `wasm-pack`, a native wheel per platform),
- a **binary artifact** in a source tree,
- and **a dependency that can fail to install** on any platform they support.

That is a hard sell on its merits and a reasonable one to refuse. **The engine is not what an Evennia game
needs at runtime anyway.**

## What an Evennia game actually needs

**A file, and code that applies it.** The generation is expensive, deterministic and out-of-band; the game
is none of those things. So:

```
    Worldbuilder (Rust engine + studio)        <- heavy, external, versioned
                    |
                    v
             a worldfile               <- plain data, checked in, diffable
                    |
                    v
    the contrib: read it, apply it to Evennia  <- pure Python, no engine
```

**The contrib is the bottom box, and only the bottom box.**

## What we already have that fits this shape

- **`worldbuilder/integration/`** exists in this repo, with `maritime.py` in it. The seam is already cut.
- **Two adapter paths are already designed** and named in the Mark 2 spec: `explicit_adapter` against zone
  attributes, and `graph_inference` against an exit graph. **Both are exercisable for the first time** now
  that a real game database is available as a fixture -- the testbed had no zone attribute of any spelling
  and could only ever have tested one of them.
- **Slice 2a** is exactly this work, and it is parked rather than unplanned.
- **The engine already refuses to be needed at runtime**: worldfiles carry concrete values, and the version
  discipline (VERSION-001, fail closed, no silent substitution) exists so a stale worldfile is an error
  rather than a quiet wrong answer.

## What this decision changes

**It makes the worldfile format a public interface**, which it currently is not. Today it is whatever the
studio writes and the applier reads. As a contrib boundary it needs:

- a **stated schema with a version**, and the fail-closed behaviour already chosen for tags,
- **documented semantics for every field**, because a third party will read it without this repository,
- and **a compatibility policy** -- what a contrib does when it meets a worldfile from a newer generator.

**It also sharpens what the engine may assume.** Anything the contrib needs must be *in the file*. That is a
useful constraint: it forces the generator to commit to values rather than leaving the game to re-derive
them, which is the same discipline that made the water manifest carry resolved lake levels instead of a
recipe for computing them.

## What it does not change

- **The engine stays here**, versioned independently, free to be a Rust crate with a wasm build and a Python
  wheel for testing. None of that is Evennia's problem.
- **The studio stays here.** A contrib does not need a viewer.
- **The conformance oracle stays here.** 157 tests comparing Rust against Python are a development tool, not
  a shipping artifact.

## Open questions, worth answering before writing a line of contrib

1. **Does the contrib create rooms, or annotate existing ones?** The dragonsire fixture has **1,616 `Room`
   and 2,619 `Exit` objects** with a `region` attribute on **1,686 objects in production use**, alongside
   `zone`, `zone_id`, `area`, `area_id`, `canonical_area` and `canonical_area_name`. **A real game already
   has a world**, and the messy prior state is what section 2 promises to accommodate rather than require
   the absence of.
2. **What is the smallest useful contrib?** "Apply a worldfile to a fresh game" is a demo; "annotate an
   existing 1,600-room game with terrain, climate and water without breaking it" is the real request and a
   much harder one. **Shipping the first while claiming the second would be the wrong trade.**
3. **Which Evennia version, and what is the support window?**
4. **Does it need the dragonsire fixture to be tested honestly, and can a contrib's test suite carry
   anything like it?** That fixture is a real game's data and **cannot ship** -- so the contrib needs a
   synthetic equivalent that exercises both adapter paths, which the old testbed provably could not.

## The honest risk

**A contrib is a promise to strangers.** Everything above is cheap while this repository is the only
consumer, and expensive the moment somebody else's game depends on the worldfile format. **The schema
question above is therefore not paperwork -- it is the whole cost of this decision**, and it should be
settled before slice 2a resumes rather than discovered during review.
