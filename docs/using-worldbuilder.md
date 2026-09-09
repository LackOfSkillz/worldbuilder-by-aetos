# Using Worldbuilder

Everything that exists today, what it does, and how to drive it. If you only read one
section, read **Quickstart**; if something surprises you, the section on that feature says
why it behaves that way.

---

## Quickstart

Three commands and a browser.

```bash
cd viewer && npm run serve          # the studio, on http://localhost:8137
```

Open it, pick a world in the **WORLD** panel, open **POPULATE**, choose a count, press
**generate**. Watch the dial. When it reads COMPLETE:

```bash
python -m evennia_roundtrip.export --worldfile runs/<run-id>/worldfile.json \
       --into /path/to/your-game/world --name aetosia
```

Then, in your game:

```
@py from world.build_aetosia import build; build(self)
```

That is the whole loop: **paint a planet → populate it with areas → export → build into
Evennia**. Everything below is detail.

---

## The studio

`viewer/` is a browser front end over the generator. It draws the planet on a globe with
CesiumJS, and every panel writes into the same worldfile the Python side reads.

### Painting a world

The **PAINT** panel holds five brushes: mountains, islands, rivers, lakes and coast. Strokes
are *ghosted* as you draw and only committed when you press apply, so a rebuild happens once
rather than on every mouse move.

Shapes are organic by construction. A painted mountain is a ridge of overlapping lobes with
spurs, not a smooth cone; an island has inlets; a river meanders and its channel is cut so a
hull can actually follow it. The composition rule is that a raise never lowers and a carve
never shoals - so two strokes that overlap cannot undo each other.

### Saving and reopening a world

**save world** writes to `worlds/<name>.json` on the server - not to your downloads folder.
The file carries the planet's parameters, every painted feature, and, after a run, every
generated area and road. **open worldfile** reads one back.

A saved world can be grown: the generator appends to the areas a world file already carries,
so opening a world and running populate again adds to it rather than starting over.

### Watching a run

The dial reports three things because they fail differently:

- the **needle** says how far through the run is;
- the **words** say which stage it is in - choosing sites, founding cities, settling the
  starting ground, building areas, laying roads, laying the ferry lines, sounding the
  ground, checking every place can be reached, writing the world, complete;
- the **clock** says that anything is happening at all, and is the only one that never
  stops. A run is silent for the better part of a minute before its first pin while the
  ground is scored, and silence is indistinguishable from a hang.

**run summary** reopens the tally after you close it, rebuilding it from the newest complete
run on disk - so a run survives the browser being closed.

### Reading the map

- Hover an area for its name, people, purpose, faction, rooms, shops, wares, inhabitants,
  docks and level band.
- Click an area to fly down to it and draw its rooms.
- Rooms are drawn as dots, not labels - twenty-two names over a village is a wall of text
  laid across the thing it labels. Hover a room for its name; click it for a card with its
  description, what is through its door and what that shop sells. The card closes on its ×.
- A room with a keeper or goods behind its door is drawn larger and warmer.

---

## The generator

`evennia_roundtrip/` turns a painted planet into places.

```bash
python -m evennia_roundtrip.generate --world worlds/aetosia.json --count 400 \
       --label populate --region=-26.5,26.5,-36.5,36.0
```

`--region` is `lat_low,lat_high,lon_low,lon_high`; omit it for the whole planet. Each run
writes to `runs/<timestamp>-<label>/`: the worldfile, the progress feed, the roads, the
crossings, the refusals, the soundings, and a manifest with the summary.

### How places are chosen

Sites are grown outward from seeds on shores and rivers, scored against what each culture
needs, and offered to cultures in rotation by how far behind quota each one is - one pass
down the whole list before anybody gets a second helping. Without the rotation, whichever
culture sits highest in the table eats the good ground.

Eight candidate sites are grown per area asked for. That margin is wide because the
particular peoples need particular ground: a marsh town wants half a metre to twenty-five
above the sea, nearly flat, with a landing; a fishing village wants a shore. At three
candidates per area every such culture came out with one or two areas whatever its quota
said - not refused, just never offered anything that fitted.

### Cities

One major city per fifty areas, founded **first**, so the rest of the world is sited around
them. They are placed on a grid rather than by score, because every site score rewards water
and unconstrained they string along the coast and leave the interior to hamlets. The grid
decides that a region *has* a city; the ground inside the cell decides where it stands.

A city is 160 street rooms - above canon's ceiling on purpose, and recorded as a deliberate
departure. A cell that already holds one of your own places is skipped: that place is its
region's city, which is also what stops a generated capital being dropped beside a
hand-built one.

### Levels

Bands radiate from the first city: 1-5, 6-10, 11-20, 21-40, 41-60, 61-100, each a fraction
of half the planet's circumference. Six areas are settled inside the innermost ring on
purpose - it is six hundred kilometres across while areas sit a hundred to two hundred miles
apart, so left to chance a four-hundred-area world put one area in it.

### Streets, shops and people

A settlement is a lattice of street rooms. A street keeps its name for its whole length and
its sections are named for the ways they cross - `Market Row, West End`, `Market Row at Kiln
Lane` - so no two rooms share a name and walking east holds the street.

**A shop is a room you walk into.** It is an interior keyed to the street room that opens
onto it, entered by its own noun and left by the same noun with `out` as an alias, off the
lattice so it can never bend a street, and named in the street's description so the noun you
type is the noun you read:

    Kings Row at Mill Yard
    ...A curtain hangs across the surgery door.
    go surgery  ->  the healer's rooms

Every trade has a door, including the open-air ones - `go stall`, `go camp`. A city cycles
the trade list rather than running out of it, and a second general store is named for the
street it stands on.

People stand in the rooms: a keeper in every shop, folk through the streets, quarry in the
hunting grounds. They are named by trade and station - `a stallholder`, `an ostler`, `a red
deer` - rather than by invented proper nouns.

### Goods

Each settlement works in one colour and one material, so its goods read as a set. Durable
goods carry the mark of the place that made them:

    a dust-red rawhide-hilted dagger in a plain sheath from Farsteading
    a silver antler-bound buckler from Elderbough

Food and drink do not - you carry a dagger home from the badlands, not the broth. The
material makes the part it could actually make - a hilt, a binding, a handle - so nothing is
a spider silk short sword. No two settlements sell the same keepsake; staples stay common.

### Roads and ferries

Every area is joined to the network by road. Roads pathfind around water and over passes,
carry a room every five miles, meet at crossroads, and hang side paths off themselves for
hunting grounds. A road that would cross water is refused and a boat is asked for instead -
checked at each room *and* along the span between rooms, because rooms are five miles apart
and a two-mile channel fits between two of them unnoticed.

The end rooms of a road name the places they join, so a player knows they are leaving one
area for another.

Ferries run as a service rather than as accidents: one terminal per shore, a hub per sea,
shore-to-shore hops where going round by the hub would be most of the journey again, and
crossing times spread across a thirty-to-fifty-minute band so they do not all take the same
thirty minutes. Two hulls per line in opposite phase, so a boat leaves both ends at once and
they pass in mid-water.

### What a run checks about itself

- **Reachability** - every place can be walked or sailed to; the count of stranded areas is
  in the summary.
- **Soundings** - no room stands below sea level and no span crosses open water.
  `water.json` names every place that fails, and the counts sit in the summary beside
  `stranded`.
- **Period** - every generated phrase goes through an anachronism lint.
- **Prose bands** - word count, sentence count, no weather, no time of day, no second
  person.

---

## Exporting into Evennia

```bash
python -m evennia_roundtrip.export --worldfile runs/<run>/worldfile.json \
       --into /path/to/game/world --name aetosia
```

Writes two files beside each other: `<name>_world.json` and `build_<name>.py`. The builder is
sixty lines that read the JSON; neither file imports anything from this project, so a game
that has those two files needs nothing else installed.

    @py from world.build_aetosia import build; build(self)

Running it twice does not double the world. Every room carries its worldfile id as `wb_id`
and is found by it, so a second run updates what is there and a later export refreshes
descriptions in place.

It builds rooms, exits (with `out` aliased on the way out of an interior), the people, and
the keepers with their stock. Room and exit typeclasses are the game's own
(`typeclasses.rooms.Room`, `typeclasses.exits.Exit`, `typeclasses.characters.Character`), so
it lands in a stock Evennia without changing anything.

**Two things it does not do.** It does not join the generated world to rooms you built by
hand - the exporter wires only what is in its own file, and has no way to know where your
world's edge is. And a bulk import is a long write: stop the server first, or the running
game will not see the new rooms until it reloads.

---

## Where things are

    worldbuilder/            the planet: geometry, plates, terrain, bathymetry
    evennia_roundtrip/       the area generator, and the Evennia exporter
    viewer/public/app/       the studio
    viewer/scripts/serve.mjs the dev server the studio talks to
    runs/                    one directory per run
    worlds/                  saved worlds
    docs/design/             why things are the way they are

## Tests

```bash
python -m pytest tests -q          # the planet and the generator
cd viewer && node --test test/*.test.mjs
```
