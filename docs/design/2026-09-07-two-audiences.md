# Two audiences, one engine: the dev panel is not the product

The owner, after a session of building intricate controls:

> "for what this really is, we may be exposing too much intricate planet building sliders to the players.
> what they really want is to make a world, maps of their world, and be able to let their world be on a
> sphere. So maritime can circumnavigate the globe and travel overseas to different areas."

**This is a product decision and it is correct.** It also costs almost nothing, because it is mostly a
question of what to *show*, not what to build.

## REFINED BY THE OWNER: easy and advanced, both user-facing

The first version of this document put the intricate controls behind a **developer** toggle -- ours, not
theirs. **The owner corrected it:**

> "I think we should have a easy mode and advanced. that way a user that wants all the levers can experiment
> with them, but a user that wants to just create a world fast and use mostly defaults with the basic
> sliders can"

**This is better, and the difference is not cosmetic.**

- **Nobody is locked out.** A curious author gets the same instrument we have, rather than hitting a wall
  where the tool stops explaining itself.
- **Advanced becomes a documented feature rather than a debug backdoor**, which changes what it owes:
  labels an outsider can read, grouping that means something, and travel on every slider that a stranger can
  aim. **A dev panel may be cryptic; a shipped advanced mode may not.**
- **Easy mode is the default**, so the fast path is the one somebody falls into.

**The cost is real and worth stating**: a *hidden* dev panel needs no polish, and a *shipped* advanced panel
does. Every control in it becomes a promise. That is a larger job than hiding one -- but it is the right
one, because the alternative is a tool that quietly implies its users cannot be trusted with it.

**What does not change:** the engine keeps every parameter either way, and the measured defaults matter
more, not less. **An author in easy mode is trusting the hundred sliders they chose not to open.**

## The two audiences

**The developer** -- us, now -- needs every knob, because finding a good baseline is exactly what this
session has been doing. `continental_blend`, `harmonic_band_m`, `slope_reference`, `octave_persistence`:
these exist so a defensible default can be *measured* rather than guessed. **They have earned their keep and
none of them should be deleted.**

**The author** -- somebody with an Evennia game and a map they want on a globe -- needs almost none of it.
In the owner's words, they want to **pull their database, place each area on the globe, print some maps, and
feed the planetary positions back**. Everything else is noise that makes the tool look like a physics
simulator they did not ask for.

**A toggle, not a deletion.** The dev panel stays behind a switch. Nothing is lost and nothing has to be
re-derived later.

## What the author panel actually needs, and how much of it exists

The owner named the vocabulary themselves, and it is intent-level rather than mechanism-level:

| Author control | What it drives today | State |
|---|---|---|
| more land / more ocean | `land_fraction` | **exists**, a slider already |
| more islands | coastline raggedness (`CoastParams`) | **exists**, the `fractal` preset and a slider |
| taller mountains / no mountains | `continent_collision_m` | **exists**, the `ranges` preset and a slider |
| more / less desert | moisture bands, the upwind march | **climate, in flight** |
| more / less forest | the same two axes | **climate, in flight** |
| more / less tropical | temperature's equator/pole pair | **exists** (`ClimateParams`) |

**Almost every intent-level control already maps onto something built and measured.** The author panel is
mostly a *presentation* layer over parameters that exist -- naming them in the author's language, choosing
sane travel, and hiding the rest.

**The honest exception is that intent-level controls are not one-to-one.** "More islands" is coastline
amplitude *and* land fraction interacting; "no mountains" is collision amplitude *and* the structure field.
**A mapping that pretends one slider is one parameter will mislead**, and this project has already shipped a
control whose label promised something its parameter could not deliver -- `mountain_m` in the relief panel
was a roughness budget wearing the word "mountain".

## What this reframes

**The studio stops being a nice-to-have and becomes the product.** It has been described as "place areas
visually, which matters at fifty areas and not at five". That was true for *us*. For an author with an
existing game it is **the entire interaction** -- pull the database, see the areas, put them somewhere,
export.

**Cartography stops being a later slice and becomes a deliverable.** "Print out some cool maps" is a stated
want, not a flourish.

**And the round trip is confirmed as the spine**, which the owner already chose: read the database, place,
export, and let maritime answer where anything is on the globe. `maritime.py` already converts a local
position to a global one; `Region` already carries the anchor and bearing placement needs.

## What this does NOT change

- **The engine keeps every parameter.** Author-facing simplicity is a panel, not an amputation.
- **The measured defaults matter more, not less.** An author who sees six sliders is trusting the hundred
  they cannot see. **Every default they inherit is one somebody had to justify** -- which is what all the
  sweeps and presets in this repo are for.
- **The dev panel stays reachable**, because the next baseline still has to be found by somebody.

## The risk worth naming

**An author panel invites a promise the generator cannot keep.** "More desert" implies control the moisture
model may not have -- and the climate slice has already measured that **79-90% of the arid band on
Earth-sized worlds is dried by a truncated march** rather than by measured dryness. A slider labelled
"desert" sitting on top of that is a label writing a cheque the engine does not cash.

**The fix is not to hide the limitation but to bound the promise**: fewer controls, each of which does
what its name says, with the rest of the physics left to defaults that were measured rather than chosen.
