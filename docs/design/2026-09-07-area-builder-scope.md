# Scope addition: a visual area builder

The owner, adding to what "done" looks like:

> "it needs to allow the user to grab their areas from the evennia db, and let them place the areas on the
> map. and this will be an addition to the scope. I want to create a visual area builder, bake it in so if
> the user is brand new to evennia and doesnt have any areas, they can create evennia style areas visually."

**This is an addition and is recorded as one.** It roughly doubles the studio's surface, and pretending
otherwise would make the estimate dishonest.

## Two entry paths, not one

Until now the round trip assumed the author already has a world:

```
    existing Evennia db  ->  read areas  ->  place on globe  ->  export  ->  back to Evennia
```

There is now a second entry, and it is the one a **new** user takes:

```
    no areas at all  ->  BUILD areas visually  ->  place on globe  ->  export  ->  into Evennia
```

**Both converge at placement**, which is the same anchor-and-bearing per area either way. So the addition is
a new *front end*, not a new spine -- the export, the port mapping and the maritime adapter are unchanged.

## THE PART WORTH BORROWING IS NOT THE UI

The reference implementation is unpolished and unfinished by the owner's own account. **What is finished,
and is the valuable half, is the definition of a correct area.**

`docs/design/area-building-laws.md` in that project is a set of **numbered laws, each MUST / SHOULD / MAY**,
described as *"geometry and topology, specified tightly enough that a program can follow it without
judgement, and tightly enough that following it cannot produce a bad map."*

And critically, its own header:

> "Where a law carries a number, the number was **measured against 224 real areas of the measured corpus**
> -- 18,668 rooms from a published map export, 14,046 of them carrying their true position on the official
> map sheets -- not chosen because it sounded right."

**That is the same discipline this repository runs on**, arrived at independently: every figure names its
population and its method. It is enforced by `tools/area_lint.py`, which **certifies** an area rather than
merely passing it -- writing out every measurement, every finding by law, and accounting for every room
including the ones with nothing against them.

**A visual builder that enforces measured laws is a different product from a freeform room editor.** The
first cannot produce a bad map; the second mostly does. **Borrow the laws and the certifier first; the
editor is the easy half and should be built to serve them.**

## The architectural consequence, and it is the expensive one

**Today the worldfile answers "where are the areas".** If Worldbuilder can *create* areas, it must also
answer **"what are the areas"** -- rooms, exits, their topology, their descriptions.

That is a substantially larger schema, and it lands on the decision already recorded in
`2026-09-06-contrib-shape.md`: **the worldfile becomes a public interface the moment a contrib reads it.**
A schema that carries an entire authored world is a bigger promise to strangers than one carrying a list of
anchors.

**Two ways to keep that bounded, and one of them should be chosen deliberately:**

1. **The builder writes Evennia's own shapes**, and the worldfile still carries only placement. Worldbuilder
   creates rooms and exits *in the game's* format and never invents a parallel one.
2. **The worldfile carries areas fully**, and becomes the authoring format of record.

**Option 1 keeps the promise small and is the default unless somebody argues otherwise.** It also matches
the rule already chosen: *Evennia receives concrete values and never needs the engine at runtime.*

## Where this sits

The owner has already chosen the Evennia round trip as the next slice after climate. **This changes that
slice's shape rather than its position**: the reader and the exporter are still first, because **the builder
and the importer both feed the same placement step**, and placement is what proves the spine works.

**Suggested order, and it front-loads the cheapest proof:**

1. **Read** areas from a real database. The fixture exists; neither adapter path has ever been run.
2. **Place** them -- hand-authored anchors first, since that is untested and a studio is not required for it.
3. **Export**, with the port mapping from `2026-09-07-port-access.md`.
4. **Prove the round trip**: a ship sails between two areas' waters with consistent global positions.
5. **Then the visual builder**, borrowing the laws and the certifier, for the author with no areas at all.

**Step 4 is the demonstration the whole project is for.** Building the editor before it would mean authoring
worlds nobody has proven can round-trip.

## The honest size

A room-and-exit graph editor that enforces a validated law set is **not a small piece of work**, and the
project has just come out of a session where the owner asked, reasonably, whether it was getting too heavy
for its purpose. **The mitigation is that it is the LAST step of five, not the first** -- and by then the
spine will either have proven itself or not.
