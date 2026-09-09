# The first generated world, judged by the game's own laws

**Run:** `20260908T232418Z-populate-130` — 130 areas, 127 roads, 9,160 rooms, 19,554 exits.
**Judged by:** `area_lint.lint`, `prose_lint.measure` and `draw_lint` from the maritime
contrib's `area-design/`, against `area-building-laws.md`. Nothing was re-implemented: these
are the same functions the hand-authored world is gated on, fed from the worldfile instead
of from a module.

**Headline: 0 of 257 places pass. 3,614 errors and 4,988 warnings.**

That number needs its context. The laws were written for areas a person authored one room
at a time, and no generator has ever been measured against them before. This is a baseline,
not a verdict — but the ranking inside it is what the next run should be aimed at.

## What is actually wrong, in order of what it costs

| Law | Count | What it means |
|---|---:|---|
| G1 (MUST) | 1,761 | Rooms sharing a street name are not one connected run — "Peel Board Walk covers 8 rooms in 8 separate pieces" |
| R8 (MUST) | 1,664 | A straight run of 3+ linked rooms that share no street name |
| S10 | 1,699 | A dead end with nothing in it — a player walks there and is told they wasted the trip |
| L7 (MUST) | 1,152 | Rooms in neighbouring cells that are not joined |
| G6 | 509 | A description promises a way out the room does not have |
| W6 | 334 | Time of day written into permanent text |
| P4 | 256 | An area with no point of interest at all — nothing entered by a door, gate or stair |
| S8 | 248 | Over a third of rooms are chokepoints |
| L6 (MUST) | 188 | Two lines meet where there is no room |
| S1/S2/S4/S7 | ~500 | Corridor tubes, too few junctions, no loops, runs of 27 corridor rooms |

## The single root cause

**The lattice is correct where it exists and full of holes.**

- Every compass exit agrees with the cells it joins: **10,542 of 10,546 (100%)**. The
  generator's geometry is not confused.
- But only **5,267 of 8,384 (63%)** pairs of rooms in neighbouring cells are actually
  joined by an exit.

A street is a row of that lattice. A row with a hole in it is two pieces wearing one name,
which is G1. A straight run nobody named is R8. A gap where two ways cross is L6. A room
with one way in and out is S8 and S10. **Nine of the ten findings above are the same defect
seen from different angles.**

## What the fixes actually buy — measured, not guessed

Four naming schemes and a densified lattice, each run through the real linter:

| Scenario | G1 | R8 | L7 | S10 | duplicate labels | total errors |
|---|---:|---:|---:|---:|---:|---:|
| as built | 928 | 1,225 | 1,152 | 1,445 | 1,956 | 2,341 |
| row only | 949 | 1,225 | 1,152 | 1,445 | 3,393 | 2,362 |
| ends + crossings *(committed)* | 1,465 | 1,140 | 1,152 | 1,445 | **490** | 2,793 |
| every room a crossing | 1,693 | 1,024 | 1,152 | 1,445 | 490 | 2,905 |
| named by straight run | **614** | 1,009 | 1,152 | 1,445 | 689 | **1,811** |
| joined + as built | 730 | 1,677 | **3** | **411** | 1,956 | 2,632 |
| joined + named by run | 997 | 1,501 | 3 | 411 | 523 | 2,723 |

*(settled areas only; the whole-world table above includes roads)*

Three things fall out of this, and two of them are uncomfortable.

**1. The street fix I committed (`ce64a2b`) is a partial regression.** It does what it was
asked to — duplicate map labels fall from 1,956 to 490, which was the complaint — but G1
rises by 537. Naming a section for the way it crosses makes the room claim the crossing
street too, and the columns of a holed lattice have holes, so every column name arrives
already broken. It should not ship as it stands.

**2. Naming from the exit graph's straight runs beats every lattice-row scheme.** G1 614,
total errors 1,811 against 2,341 as built — the best result of anything tried, and it needs
no geometry change. A run is contiguous by definition, so a name taken from one cannot span
pieces. This is what R8 and G1 are both *about*, and the current code names from rows
instead, which is a different thing that usually coincides.

**3. Density and naming pull against each other.** Joining every neighbouring pair nearly
eliminates the shape findings — L7 1,152 → 3, dead ends 1,445 → 411, S8 121 → 13 — and
makes naming *worse*, because a dense lattice has many more straight runs to name and many
more crossings to disambiguate. There is no order in which these two can be fixed
separately. **The lattice and the street naming are one design problem.**

## Prose

The prose is inside the bands and dull.

```
words         min 34, median 43, max 54     (canon 34-79, median 53)
too thin      none
too fat       none
sentences     2 rooms outside 2-4
second person 7 rooms say "you"             (canon 5.4%)
openings      Rutted x1094, Packed x958, Worn x880, Grass x852, Dry x614
```

- **Nothing is illegal and nothing is good.** Every description clears the floor and none
  reaches the middle of the canon band. Median 43 against canon's 53 means every room is
  three-quarters of a room.
- **Five words open 4,398 of 9,160 rooms — 48%.** The band checks catch a room that is too
  short; nothing catches ten thousand rooms that start the same way, because no hand-author
  has ever done that. This is the most visible generated-ness in the world and no law fires
  on it.
- **G6, 509 rooms, is a room lying to a player** — the description names a way out that
  isn't there. `retell_exits` was added for exactly this and does not cover it.
- **W6, 334 rooms, puts the time of day in permanent text** ("before evening"). A
  description is eternal; weather, light and time belong in ambient text.

## What the next run should change

In the order the measurements support:

1. **Name streets from straight runs, not lattice rows.** Best single change: −530 errors,
   no geometry work. Revisit `ce64a2b` as part of this, not before it.
2. **Join neighbouring rooms, or stop placing them adjacent.** 37% of touching pairs are
   unjoined. This is the root of L7, S8, S10, L6 and most of S1/S2/S4/S7.
3. **Design 1 and 2 together.** The table above shows each one making the other worse when
   done alone.
4. **Give every dead end something**, or stop generating dead ends: 1,699 findings.
5. **Fix G6 properly** — 509 rooms promise exits they lack, after a fix that was supposed
   to close it.
6. **Move time-of-day language out of permanent descriptions** (334 rooms).
7. **Break the opening-word monoculture.** Not a law today; arguably should be one.

## What this says about the laws themselves

Two gaps worth considering, since the laws are ours to change:

- **There is no law against sameness.** 48% of rooms opening with one of five words passes
  every check. A hand-author cannot produce that failure, so no rule was ever needed; a
  generator produces it by default.
- **G1 and R8 are two statements about runs, enforced against names.** Making the naming
  read the run graph directly — as tested above — closes most of both. It may be worth
  saying so in Part 13 rather than leaving each generator to rediscover it.

---

## What walking the world added

The lint above reads the worldfile. These came from importing it into a real game and
walking it, which found things no linter was asked about.

### 8. The population did not exist

`npcs` was `rooms x density` — a number, computed and reported, with nothing behind it. No
worldfile carried a single person. Every population figure this generator has printed, in
every run, described people who were never made; the tally panel added them up and showed a
total. The docstring on the function that produced it says *"Counted, because a predicted
number is a different claim"*, which is exactly the mistake it was making.

**Fixed.** `people.py` places a keeper in every shop and scatters folk through the rest at a
density set by settlement size; hunting grounds get quarry instead — a deer is as much an
NPC as a stallholder. Named by trade and station (`a stallholder`, `an ostler`, `a red
deer`) rather than by invented proper nouns, which keeps three thousand pieces of accidental
lore out of the world. `npcs` is now `len(list)`. The existing 130-area run re-peopled to
**4,478 people across 257 places**.

### 9. Every description listed the exits, and so did the game

    Ways lead east, south and north.
    Exits: east, south, and north

Two lines, three facts, twice, in every room of the world. Law G6 forbids a description that
*promises* a way the room has not got; it never asked for the promise. **Fixed** — the
sentence is gone, along with `retell_exits` and `ways_sentence`, which existed only to keep
it honest as roads were added.

### 10. A description does not know what its room is

`the shrine` was described with a hand-cranked winch, chalk sums on a board and a kettle
being silenced; `the alchemist` smelled of solder and steam. The prose is voiced by the
**area's** culture and never consults the **room's** purpose, so every room in a gnome town
reads as a workshop whatever its sign says. A shrine that reads as a workshop is the same
defect as a shop with no shopkeeper: the name says one thing and the text says another.

**Not fixed.** It needs a per-purpose vocabulary layered over the cultural one — the shrine's
own furniture, the healer's own smells — and that is a body of writing, not a bug.

### 11. The openings were a signature

Five words opened 48% of 9,160 rooms. **Partly fixed**: the description now rotates which of
four sentences leads, including one that opens on sound and smell rather than on ground.
Measured over 3,000 human-voiced rooms, the top five openings fell from 48% to **32%**, with
18 distinct three-word openings per culture. Still not what a person writing one room at a
time produces.

### 12. The rooms are legal and thin

Median 43 words against canon's 53, inside a band of 34-79. Removing the ways sentence and
spending it on a second cultural detail brings the median to **45** — better, still short.
This is the ceiling of the template approach: more length from the same vocabulary means
more repetition, not more room.

**Proposed, not built: AI-assisted description as a user-chosen option.** Two generators
behind one switch — the rules-based one, which is free, instant and deterministic, and a
model-written one for comparison. The rules to hold to if it is built:

- **The seed must still reproduce the world.** Model-written prose is generated once, into
  the run's own files, and never at play time. A run either has AI descriptions baked in or
  it does not.
- **The choice is the user's, per run**, beside the world and area count — not a setting
  buried in a config.
- **The same laws gate both.** Word band, no weather, no time of day, no second person, no
  promised exits. A model that writes beautifully and breaks G6 is a worse generator, and
  `prose_lint` already measures the difference.
- **Cost is stated before the run**, because 9,160 rooms is a real bill and a real wait.

The interesting result is the comparison itself: the same world, described twice, measured
by the same linter.

### 13. A shop had goods and nobody to sell them

Wares were an attribute on the room — invisible to a player and unreachable by any command.
**Fixed**: the keeper carries the stock and their description lists it, so clicking a
shopkeeper shows what they sell. Buying is not possible and is not the generator's to fix:
the host contrib deliberately ships no economy.

### 14. The generated world does not join the world that was already there

An import adds a world beside the existing one with **zero exits between them**. The exporter
wires only what is in its own file and has no way to know where a game's edge is. Not a bug —
a decision nobody has made yet: either the import owns the whole world, or the generator is
given a room in an existing game and grows toward it.
