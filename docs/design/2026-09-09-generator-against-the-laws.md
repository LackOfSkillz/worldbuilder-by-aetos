# The generator against the area-building laws

**What was audited:** all 71 laws in the maritime contrib's `area-building-laws.md`, the 36
codes its `area_lint` and `prose_lint` actually check, and the generator's own rules. Every
number below is measured on twelve-area runs from the same seed and region, linted by the
game's own checkers.

**Headline: warnings 1,020 → 441. Errors 338 → 332.** Every prose law now passes. Every
geometry law that was failing still is, and they are one problem.

---

## 1. The laws are enforced less than half

71 laws are stated. **36 have a check behind them; 35 do not.** The unchecked list includes
T1, T1a, T2, T3, T4, P5, G4, B1, B2, E4 — which is precisely where the generator's worst
violations were, and why nothing caught them for a hundred and thirty areas.

`check_discipline.py` in that repo ends with *"When you add a rule to this document, add its
check."* The area laws are a different document and did not get that clause.

---

## 2. Violations found and fixed

| Law | | What was wrong | Now |
|---|---|---|---|
| **P5** | MUST | A shop was the street room. The corpus behind this law is 7,737 rooms and it is the most consistent thing in it. | Interior, entered from the street |
| **T1** | MUST | Shops were entered by compass — `north` into the weaponsmith. | `go smithy` |
| **T1a** | MUST | No door to come back through. | Same noun out, `out` aliased |
| **T2** | MUST | Shops stood on the street lattice and constrained its geometry. | Off-lattice, keyed to a parent room |
| **T3** | SHOULD | — | Exactly one street door each |
| **T5** | SHOULD | 146 rooms had a noun exit their description never mentioned. | 0 |
| **G4** | MUST | Nothing on the street said what a shop traded in. | The doorway sentence names the trade |
| **W2** | SHOULD | 341 rooms ran to five or six sentences against a band of 2–4. **Mine**, from rotating the openings without counting them. | 0 |
| **W6** | context | 79 rooms put an hour of the day in permanent text. | 0 |
| **W4** | SHOULD | Two vocabulary lines addressed the reader as "you". | 0 |
| **W5** | SHOULD | Openings were 48% five words; canon opens 44% "The", 18% "A". | The/A/An now 19%/24%, 18 distinct three-word openings |
| **W1** | SHOULD | Median 43 words against canon's 53. | Median 48 |
| **G1/R8** | MUST | Street names collided across a town (birthday problem on 16 heads × 6 kinds). | Drawn without replacement |

Two faults were introduced by earlier fixes in this same session and are counted above as
found-and-fixed: **W2**, and `STREET_HEAD`/`ENDS` being deleted with `retell_exits` — which
nothing caught, because no test called `street_plan` afterwards. Every settled area would
have raised `NameError` on the next run.

---

## 3. A law that contradicted its own checker

**G5** says in as many words: *"Interiors are exempt — being off-lattice is what T2 asks of
them."* Its check exempted only rooms whose exits are literally named `out`. **T1a** requires
leaving by the noun, and the law's own figure is that canon uses `out` for **4%** of
door-like links. So every interior built exactly to the law was warned about by the law's
checker: **134 in one twelve-area run.**

Fixed in `area_lint.lint`, which now takes a declared `interiors` set the way it already
takes `hidden` and `one_way`, read from the same `interior` tag the client's land map uses.
G5: 134 → 0.

**Note:** `area-design/` is gitignored in the contrib — local world-shaping tooling, not
shipped — so this fix is not in a commit there. It is the one change in this audit with no
home.

---

## 4. Outstanding, in the order they cost

All four are one defect seen from different angles: **the lattice is correct where it exists
and full of holes.** 100% of compass exits agree with the cells they join; only 63% of rooms
in neighbouring cells are joined by an exit.

| Law | | Count | What it is |
|---|---|---:|---|
| **S10** | SHOULD | 269 | A dead end with nothing in it |
| **G1** | MUST | 217 | Rooms sharing a street name are not one connected run |
| **L7** | MUST | 108 | Rooms in neighbouring cells not joined |
| **R8** | MUST | 95 | A straight run of 3+ rooms sharing no street name |
| **L6** | MUST | 20 | Two lines cross where there is no room |
| **P7** | — | 19 | A walk longer than canon's 90th percentile |
| **S8** | SHOULD | 19 | Half the rooms are chokepoints (limit 35%) |
| **S9/S1/S2/S7/E5** | SHOULD | ~21 | Stringy roads: corridor tubes, too few junctions, no loops |
| **R5** | SHOULD | 5 | A single room called a street |

**The measured plan**, from the earlier report: naming streets from the exit graph's own
straight runs beats every lattice-row scheme (G1 614 against 928 as built) and needs no
geometry change; joining neighbouring rooms nearly eliminates the shape findings (L7
1,152 → 3, dead ends 1,445 → 411) and makes naming worse. **They have to be done together.**

## 5. Not applicable, and worth saying so

**V1–V3** (vertical), **H1–H5** (secrets), **B1–B4** (area boundaries as declared rooms):
the generator makes no z-levels, no secrets, and joins areas by roads rather than declared
boundary rooms. B1 and B2 are MUSTs that a road network arguably violates in spirit — a
player crossing from one area to the next is not told they are leaving. That is a design
question, not a bug, and it is the one item in this audit I have not resolved either way.
