# Inland areas and their nearest port

The owner's requirement:

> "we will need a way to let maritime know about an area thats not directly on the water. if its inland
> maritime will need to know the closest area with a port so it can get them to an area via the closest
> water accessible port"

This is the first **concrete field** anybody has needed in the worldfile, and it arrives while the schema is
still unsettled -- which is the right order.

## Where this belongs, and it is not at runtime

**Computed once at export, written into the worldfile as a value.** Roadmap section 3.1's rule holds:
*"Evennia receives concrete tags and never needs the engine at runtime."* A game asking "which port serves
this area" must get an answer from a file, not from a terrain generator.

That also makes it **inspectable and diffable** -- a builder can look at the mapping, disagree with it, and
override it, which they will.

## What a port actually is, and the trap in the obvious definition

**The obvious definition is wrong: "near water" is not "reachable by ship."**

- A lake-locked settlement is near water and no ship arrives.
- The engine already distinguishes these: slice 5b resolved lake **bodies** with spill levels, and made the
  deliberate decision that **the sea is the mapping's fallback rather than a body in it** -- the datum is
  carried once in `sea_level_m` and ocean bodies are not enumerated. So "water that is not an enumerated
  body" is exactly "the sea", which is what a ship needs.
- **A port is therefore an area whose nearby navigable water is the sea**, not merely water.

**Two further honesties:**

1. **Depth matters.** `bottom_at` and the shelf model already answer it, and `maritime.py::_survey` already
   builds `Danger` records from them. **A cove too shallow for a hull is not a port**, and maritime already
   models a chart's ignorance separately (`charted_terrain_z_at` adds a sounding error), so the worldfile
   should carry the truth and let maritime add its own error.
2. **The reach is a parameter, not a constant.** How far inland an area can be and still "have a port"
   depends on the game. It belongs in the export's inputs, stated, not hidden in a threshold.

## What "nearest" means, and where the honest limit is

**Great-circle distance between anchors is the cheap answer and it ignores mountains.** A port 40 km away
across a 4,000 m range is not nearer than one 90 km along a valley.

**Recommendation: ship the cheap answer first, and say in the field's own documentation that it is
straight-line.** Then a builder who knows better can override it, and nobody is misled about what the number
means. This project has repeatedly been bitten by figures that did not say what they measured.

**The better answer needs a cost surface** -- slope-weighted travel over the terrain -- which is a real piece
of work and, notably, **the same shape as the drainage problem that has already defeated two approaches**:
accumulated cost over an unbounded domain is not point-evaluable. **But this one is legitimately allowed to
be a bake**, because it runs at export over a *finite* set of areas, not per query. That is a genuine
difference and worth stating so nobody rules it out by analogy.

## The fields this adds

Per area, all computed at export:

- **`has_port`** -- whether sea-navigable water of sufficient depth lies within the stated reach.
- **`port_area`** -- the name of the nearest area for which `has_port` is true. Itself, if it is a port.
- **`port_distance_m`** -- and it must say by what metric, because straight-line and overland differ.
- **`port_bearing_deg`** -- optional, cheap, and what a game needs to say "the road runs south-east".

**And the schema needs a way for a builder to override any of them**, because the generator will be wrong
about somewhere that matters and the game must win that argument.

## What already exists

- **`maritime.py` converts local to global**: `WorldbuilderTerrain(surface, region, anchor)` builds a
  `TangentFrame`, and `point_at` turns maritime x/y into a `SpherePoint`. **Global position from local
  position is done.**
- **`Region`** already carries a name, an anchor and orientation, a reach and its features -- so an area's
  placement is already the shape this needs.
- **The water manifest** already resolves bodies and levels, and the sea is already the fallback.
- **`bottom_at` and the shelf model** already answer depth.

**What does not exist is the reader, the placement, the export, and this mapping** -- which is the same
missing middle the contrib note describes, now with one more field in it.

## The order this argues for

1. **Read areas from a real game database.** The fixture is there; both adapter paths are designed and
   neither has been run.
2. **Place them** -- one anchor and bearing per area, hand-authored first. **A studio is for choosing those
   visually, which matters at fifty areas and not at five.**
3. **Compute this mapping at export** and write it with everything else.
4. **Prove it end to end**: a ship sails from one area's waters to another's, and an inland area reports a
   port a traveller can actually reach.

**Step 4 is the demonstration that the whole project is for**, and it is reachable without the studio.
