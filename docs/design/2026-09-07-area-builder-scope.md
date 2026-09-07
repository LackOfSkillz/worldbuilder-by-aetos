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

### DECIDED 2026-09-07: OPTION 2. The worldfile becomes the authoring format of record.

**The owner chose the larger schema over my recommendation**, and the reasoning is sound: a worldfile that
carries rooms, exits, topology and descriptions is **one portable artefact an author can version, diff and
share independently of any game**. Option 1 would have left the authored world trapped inside whichever
Evennia instance produced it.

**What this costs, stated plainly rather than discovered later:**

- **A much larger public interface.** Every field an area needs is now a documented promise to strangers,
  with a version and a compatibility policy. `2026-09-06-contrib-shape.md` called that the whole cost of the
  contrib decision; this multiplies it.
- **The format must survive Evennia changing.** A placement-only file is nearly immune to that; one carrying
  rooms and exits is not.
- **Two writers, one truth.** An author can now edit the worldfile *or* the game. **What happens when both
  moved is a real question and it should be answered before the schema is written**, not after somebody
  loses work. The version discipline already chosen -- fail closed, no silent substitution -- is the right
  starting posture.

**What it buys, and it is the reason to accept those costs:** an author's world stops being hostage to a
running server. It can be checked into their own repository, reviewed, rolled back, and handed to somebody
else. **That is a materially better product than a list of anchors.**

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

---

## Two further decisions, 2026-09-07

### The generator: a generate-and-reject loop

**Chosen over waiting for the law study and over hand-authored templates.**

The attraction is that it **works whether or not the laws turn out to be constructive.** A validator answers
"is this map bad?"; a generator needs "what must I emit?" -- and a rule set that can only reject is useless
to a builder that must produce. **A generate-and-reject loop sidesteps that entirely**: emit a candidate,
let the certifier score it, iterate until it passes. It reuses the linter as-is rather than needing the laws
re-expressed as constructions.

**The risk is convergence, and it must be measured rather than hoped.** Tight constraints can make a random
proposer effectively never succeed. **Before this ships, measure the acceptance rate and the iteration
count for a stated request** -- "80 rooms, shops and guild rooms" is the owner's own example and is the
right first case. If it does not converge, the proposer needs to be law-aware rather than random, and the
study that is running will say which laws could guide it.

**This project has a habit worth applying here: measure the thing before assuming it works.** A generator
that succeeds on a 20-room hamlet and never converges on an 80-room village would look fine in a demo.

### The model may write names, descriptions AND ambient text

**Chosen over names-only and over deferring the model entirely.**

**This is consistent with a rule this project already holds** -- *room descriptions are eternal; only
permanently-true things belong in a desc, and weather, light and season belong in ambient or stateful
descs.* Writing ambient text is therefore not scope creep; **it is where the variable prose is supposed to
live.** A model allowed only permanent descs would be tempted to smuggle weather into them, which is the
defect the rule exists to prevent.

**Three things this obliges:**

1. **The seam must work offline.** A user with no API key gets a complete, certified area with empty or
   placeholder prose. **The model is an enhancement, never a dependency** -- and that must be provable, not
   assumed.
2. **The model needs enough context to be consistent**: the room's type, its neighbours, and the area's
   theme. A model writing each room blind produces eighty unrelated paragraphs.
3. **Generated prose can contradict the world.** A description mentioning a river in an arid band, or a sea
   view from an inland room, is wrong in a way no linter currently catches. **The engine knows the answer**
   -- biome, elevation, distance to water -- so the context passed to the model should carry it, and a check
   that generated text does not contradict it is worth more than better prompting.
