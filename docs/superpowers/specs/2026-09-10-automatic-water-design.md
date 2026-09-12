# Automatic water: rivers, lakes and ponds as part of world generation

Status: design, approved in conversation on 2026-09-10, awaiting the owner's review of this
document. Implementation follows by plan, one stage at a time.

The second project, better mountain building, gets its own design and spec after this one.
Its relationship to this work is set down in section 11.

---

## 1. Goal

A world gets its fresh water from the generator, not from a hand-run tool or a paintbrush.
One pass in the engine finds where water collects and where it runs, and it writes down
rivers, lakes, ponds and waterfalls as a small saved record. The terrain, the studio and the
Python area generator all read that one record. The paint tools become small adjustments on
top of it.

## 2. Owner decisions

These were taken in conversation and bind everything below.

| Decision | Choice |
|---|---|
| Where the pass lives | In the Rust engine, so the browser and native builds compute the same water. |
| How much water | **Earth-like**: a few great rivers per continent, lakes in real basins and uplands, ponds scattered rather than everywhere, most towns within a day's walk of fresh water. |
| Time budget | **Minutes are acceptable.** The result is saved with the world, so reopening costs nothing. Slider changes do not re-solve water; the water is marked out of date until the owner asks. |
| The world to keep | `worlds/world-1788998299904.json`: its planet, its inland sea and its island. Its areas are thrown away and regenerated. |
| The inland sea | Becomes a **freshwater great lake**. Its shoreline stays exactly where it is, and a carved outlet river drains it to the ocean. |
| Mountains | A separate project, built after water. The owner's world then gets the new mountains, with the coastline locked (section 11). |
| Paint tools | For minor tweaks only, not for designing the world. |

## 3. Why: what is wrong today, measured

- **Lakes are noise.** `wb_water_run` builds a graph of jittered Fibonacci points and samples the
  **full-detail surface** at each one (`wasm.rs:3853`, `surface.elevation_m(point, None)`). On
  the owner's world at the saved `lakeNodes=86000` (about 63 km spacing on a 9,309 km radius),
  it resolves **1,361 lake bodies, 638 of them a single node**. They are pits in a coarse
  sample of texture noise.
- **Lakes ignore rivers.** With the Ranger Camp world's 1,061 carve features installed, the count
  is still **exactly 1,361**. A carve a few hundred metres wide falls between points 63 km apart.
- **Lakes are drawn by guesswork.** The manifest carries only a level and a bounding box of node
  centres (`water.rs::water_manifest_from_graph`). The studio floods every texel below the level
  inside a box dilated by one node cell (`water.js::lakeLevelAt`, `dilateBodyExtents`). The
  result is speckled water with straight and round edges, and isolated square lakes.
- **Rivers are not generated.** `evennia_roundtrip/hydrology.py` traces rivers when somebody runs
  it, but nothing in the repository calls it. Its 950 rivers exist only in
  `worlds/aetosia-ranger-camp.json`, not in `worlds/aetosia.json` (which populate builds on) and
  not in the owner's world. The manifest's `rivers` field has been reserved and empty since slice
  5b.
- **The generator's fresh water is a proxy.** `generate.py::river_points` treats any carve
  feature as fresh water, painted harbours included. `lake_points` reads a `water` block that
  nothing writes. The oracle it relies on (`planet.elevation_from_worldfile` → `surface_open`)
  calls a binding that is missing from this checkout's `bindings.rs`.
- **Features have no spatial index.** `Features::apply` scans every feature for every sample.
  About 1,000 features cost roughly 5 ms per sample, against 43 µs for 97 (`hydrology.py`
  measurements). Automatic rivers would make that far worse.
- **The inland sea is an enclosed basin.** On the owner's world it is a separate water component
  of about 42 million km², 1,277 m deep at its centre. The lowest ridge between it and the
  ocean is **39.4 m, at 26.00°S 30.25°W** (measured at ¼°; 35.3 m at 1°). It reads as "sea"
  today only because everything below the datum is treated as ocean.

## 4. Scope

**In scope:**
- An engine bake that produces rivers, lakes, ponds and waterfalls.
- An engine water layer that cuts them into the ground, with a spatial index that also serves
  painted features.
- A point query, "what water is here", in wasm and in Python.
- The worldfile record and its fingerprint.
- The studio's bake, drawing, water panel and adjustment tools.
- Water use in the Python generator.
- The owner's world: the great lake, and the trial and final water passes.

**Not in scope:** deltas, braided channels, seasonal flooding, tides on lakes, groundwater,
erosion of the land (that is the mountains project), and changing the coastline, ever.

## 5. Architecture

```
planet params + painted features
        |
        v
  [landform surface]  structural_m + features, without detail or gully noise
        |
        v
  BAKE (engine, deterministic, once per land fingerprint)
    1. land graph        land-only nodes, neighbours, per-node area and wetness
    2. priority flood    from the ocean inward: routes, hollows, depth and area
    3. judge hollows     keep as lake or pond / notch the rim / close the basin
    4. accumulate flow   wetness-weighted, downhill, into every node
    5. extract reaches   stream / river / great river, by named thresholds
    6. refine            trace reaches at 1-2 km, outline lakes, find small lakes and ponds
    7. derive falls      steep drops along a reach
        |
        v
  HYDRO RECORD  (versioned, fingerprinted, stored in the worldfile)
        |
        +--> WATER LAYER in Surface: cuts channels and notches, damps detail, via a spatial index
        +--> water_at(point): ocean | lake | pond | river | none, level, depth, fresh, body id
                 |-- studio: per-texel drawing, water panel
                 |-- Python generator: fresh water, docks, ferries, dry-ground checks
                 +-- maritime: the named-waters manifest (Mark 2 section 13.2)
```

### 5.1 What the bake reads

It reads `Surface::structural_m` with painted features applied (the landform), **not**
`elevation_m`. Detail and gully noise are texture, and texture must not decide where water
goes. That single change removes the aliasing that makes the pit lakes.

### 5.2 What counts as the ocean

**Ruling W1.** The ocean is below-datum water connected to the world ocean: the largest
below-datum component, together with anything joined to it. Every other enclosed below-datum
basin is a **lake**. This replaces slice 5b's "below the datum is always sea".

- **Connectivity is decided on the landform, before any channel or notch is cut.** An outlet
  channel whose bed runs below the datum from a lake to the sea does not join the two. The lake
  stays a lake, and its outlet stays a river. Recorded body ids are what `water_at` answers from,
  not a re-run of connectivity.
- **An enclosed below-datum basin is never filled to its rim.** Its level is the datum, so its
  shoreline does not move (the coastline lock, section 11). Section 6.3's "fill to the spill
  level" applies only to hollows above the datum.
- **Its outlet** is a notch from its datum shoreline through its lowest rim to the ocean, cut if
  the section 6.3 water balance says it overflows (fresh). Otherwise it is closed (salt).

The owner's inland sea gets a per-body override recorded in the world: `outlet: forced`. That
makes it fresh whatever the balance says. By default its outlet is the lowest ridge found by the
bake, expected at the measured 26.00°S 30.25°W.

## 6. The bake

### 6.1 Land graph

- **Nodes:** land-only points, from the same jittered Fibonacci construction as `stream.rs`,
  drawn at a higher total count with the sea points discarded before any neighbour work.
- **Target:** about 15 km spacing on the owner's world (roughly 2 million land nodes). Measured
  in stage 1 against wasm memory. The hard ceiling is 512 MB of bake memory in a browser worker,
  and the node count is lowered, not the ceiling raised, if that is exceeded.
- **Per node:** position, landform height, area (as `stream.rs` estimates it), and **wetness**
  from the climate module (the moisture field the relief palette already calibrates from).

### 6.2 Priority flood

A single sweep inward from every ocean-adjacent node (Barnes et al. 2014, "Priority-flood"),
always expanding the lowest node on the frontier:
- **Ties** at one spill level break by the node's own ground height, then by node index, so the
  flood reaches the lowest ground first and the order is the same on every build. The height key
  is load-bearing: with ties broken by index alone, parent chains stop following valley floors
  and the drainage check fails on some hand-built graphs (see `flood.rs`).
- **Output per node:** its receiver (the downstream node) and its **spill level**, which is its own
  height or the fill level of the hollow it sits in.
- A hollow is a connected set of nodes whose spill level exceeds their height. Its depth is the
  maximum difference, its area the sum of node areas, and its outlet the rim node the sweep left
  through.

### 6.3 Judging each hollow

Three outcomes:

| Outcome | Rule (initial values, calibrated in stage 1) | Result |
|---|---|---|
| **Keep** | depth ≥ 8 m and area ≥ 1 km², or `outlet: forced` | Lake or pond, filled to its spill level, draining through its outlet. |
| **Notch** | anything that fails "keep" | The rim along the outlet route is cut down to the hollow's floor level, plus a one-metre fall. It drains, and the cut is recorded as a channel. |
| **Close** | kept, and inflow < evaporation | A lake with no outlet: salt, or a dry salt flat if inflow is below 10% of evaporation. |

- **Inflow** is the accumulated wetness-weighted flow of Section 6.4 arriving at the lake.
- **Evaporation** is lake area × an aridity factor from the climate module.
- **Pond or lake:** a surface under 1 km² is a pond; 1 km² and over is a lake.

**Ruling W2.** Mark 2 section 14.2 says the fill-or-breach question dissolves under
Cordonnier erosion. This pass runs **without** erosion, so it answers the question explicitly.
When the mountains project adds erosion, most hollows will erode away before this pass runs.
The same rules then judge whatever remains, so the design does not change.

### 6.4 Flow

Each node contributes `area × wetness`. Contributions are summed downstream along receivers
from the leaves to the mouths. A kept lake passes its inflow to its outlet, unless it is closed.

### 6.5 Reaches

These thresholds are recorded in the world and are initial values to be calibrated:

| Class | Flow threshold (as km² of fully wet catchment) |
|---|---|
| stream | ≥ 250 |
| river | ≥ 2,500 |
| great river | ≥ 100,000 |

- **Calibration target:** Strahler ordering of the extracted network must show a bifurcation
  ratio between 3 and 5 at every order pair with at least 10 streams. That is the measured
  signature of real river networks.
- **Width** = `a × Q^0.5` and **depth** = `c × Q^0.4` (Leopold–Maddock hydraulic geometry).
  `a` and `c` are chosen so a stream at its threshold is 3 m wide by 0.5 m deep and a great
  river at its threshold is 1,000 m wide.

### 6.5a Calibration, 1a

Task 12b (plan 1a) measured a 500k/1M-node coarse bake against the owner's real world and ruled
on five points the sections above left open. Full measurements and reasoning are in
`.superpowers/sdd/2026-09-10-water-1a-coarse-bake/task-12b-brief.md` and
`task-12b-report.md`; the table states the outcome.

| Ruling | What it changes | Why |
|---|---|---|
| 12b-1: resolution-aware thresholds | Section 6.5's thresholds become floors, not fixed values: `stream = max(stream_flow_m2, min_stream_nodes × median land-node area)`, `river = max(river_flow_m2, 10 × stream)`, `great = max(great_flow_m2, 10 × river)`. `HydroParams.min_stream_nodes = 10.0` in `earth_like`. `BakeStats` records the three effective values used. | At 500k nodes a land node stands for ~2,200 km² of catchment, above the 250 km² stream threshold -- every land node would be a channel. The plan's "stream ×2, up to 4 steps" lever (Ruling 12b-4: superseded) cannot land: ×16 would put stream above river. |
| 12b-2: record only the notches that matter | A notch route is recorded only if a node of it is a channel node of a recorded reach, or it is an outlet cut from a fresh enclosed pocket. Routing still cuts and keeps every notch; only the record filters. | Notches, not reaches, set record size at coarse resolution (41,202 of them at 500k, all sub-resolution dips). Stage 2's fine tracing is what should carve a drained dip a recorded river never crosses. |
| 12b-3: node budget | `pub const DEFAULT_TOTAL_NODES: u32 = 1_000_000;` | Studio heap 372 MB at 1M nodes, under the 512 MB ceiling from 6.1; an 80 s wasm bake on the owner's world. Bifurcation ratios are reported (Section 6.5's 3-5 target), not forced to it -- the gap is a known property of this budget, not closed here. |
| 12b-4: plan deviation | The "stream ×2, up to 4 steps" calibration lever (Section 6.5) is superseded by 12b-1. | The lever is structurally unable to land at this resolution (see 12b-1's reasoning). |
| 12b-5: no lake larger than the Caspian | `HydroParams.keep_max_area_m2 = 4.0e11` in `earth_like`. In Section 6.3's judging, an open hollow (neither enclosed nor forced) with `area_m2 > keep_max_area_m2` is notched however deep it is. Enclosed basins are exempt (the coastline lock already keeps them). | The owner's world kept open basins of 0.6-2.0M km² at 290-629 m fill levels -- broad landform basins filled to their rims, not real lakes (Earth's largest, the Caspian, is 0.371M km²). Real basins this large are breached by a great river; stage 2's mountains erosion will reshape them anyway. |

Neither `min_stream_nodes` nor `keep_max_area_m2` is a wasm parameter in 1a -- a wasm bake
always takes `earth_like`'s value for both.

### 6.6 Refinement

- **Tracing.** Each reach is re-traced on the landform at 1.5 km steps. At each station the tracer
  takes the lowest allowed ground, and the bed never rises: where the ground rises the bed holds,
  which is a cut. The path stays within a corridor of one graph spacing around the coarse route, so
  it cannot cross into another basin. On slopes under 0.2%, a fixed meander (amplitude and wavelength
  scaled to width, driven by the engine's deterministic noise) is applied to the carved line,
  never outside the corridor. Tributaries join at a shared vertex. Rivers end at the coast or at a
  lake shore.
  - The coarse reach points are kept exactly, and tracing runs between each consecutive pair.
    That is what makes the shared junction vertex free. The coarse beds already fall, because a
    later notch cut that runs into an earlier one standing above it re-lowers it (Rulings R-1 and
    R-9).
  - The bed follows the ground down, less the channel's depth, and never falls below the coarse
    segment's lower end. Where the ground rises, the bed holds, which is a cut. A fine dip met on
    the way is not judged as a new lake: the bed stays level across it. A new body mid-reach
    would change routing after the drainage check (Ruling R-2).
  - A segment that is not the reach's last never steps onto ground at or below the datum, so a
    lowest-ground search along a coast cannot wander into the sea. The last segment of a reach
    into the sea or a lake ends at the first station whose ground is at or below that water:
    the shore trim (Ruling R-3).
  - Where every candidate at a station is at or below the datum, the tracer steps back toward its
    chord in half-spacing increments and takes the first lateral whose ground is above the datum,
    trying the chord point itself last. It does not hold the line where it is, which used to leave
    a station on sea ground tens of kilometres sideways on a coast. If even the chord point is at
    or below the datum -- a coarse chord across a bay -- the chord point is kept. That is the one
    exception to the rule above (Ruling R-3a).
  - A mouth's bed is the lower of the bed that reaches it and the water level, so the bed never
    rises, mouths included (Ruling R-4).
  - The corridor keeps a refined line inside its own coarse route's basin, but it does not stop
    two reaches from crossing one another inside their corridors, and neither did the coarse
    graph: at 1M nodes the plain world has 61 coarse crossings and 1,957 refined, and the seed 1
    `ranges` world 95 and 3,922 (final review, Ruling FF-3). Plan 1b-3 fixes it, before stage 2
    carves crossing channels into each other.
  - A meander is drawn only where a wavelength (11 widths) of at least four steps (6 km) can
    represent it, the segment's bed slope is under 0.2%, and the segment carries no fall. Its
    amplitude is 1.5 widths, tapered to zero at both coarse points, and clamped inside the
    corridor. It moves the line, never the bed (Ruling R-6).

**Scope of plan 1b-2.** Lake outlines and small lakes and ponds, below, are plan 1b-3 (shores),
which starts with a spike. A 250 m fill of the owner's 41.33M km² great lake would be about
6.6×10⁸ cells, and its 250 m outline alone would break the 8 MB record target. So the outline
method must be decided on measurements first. Plan 1b-2 does the channels, which stage 2's
carving needs.

- **Lake outlines (Ruling S-1, as replaced by plan 1b-3's Task 1).** A lake's extent is **not**
  traced. It is recorded as an unordered set of **shore points** — the lake's **shore members**
  (the graph nodes it floods that have a neighbour it does not) followed by its **collar** (the
  distinct non-member nodes adjacent to a member) — plus, per body, how many of those points are
  members and the greatest distance from a shore member to a collar neighbour of it. Section 8.3's
  query decides what is inside from those points and the lake's level; nothing about the extent is
  ordered, so nothing needs a ring, a walk or the triangulation the k-nearest graph cannot give.
  Interior members are not recorded: they cannot move the boundary.
  - **Why not a ring, and why not a 250 m trace.** Both were measured at 1,000,000 nodes on four
    worlds (`docs/superpowers/reports/2026-09-12-water-1b3-outlines-design.md`). A collar **ring**
    cannot be built: a minimum-turn edge walk closes on a 3-cycle covering 0.3–4.1% of the collar
    on 11 of 12 bodies, and the one substantial ring it produced self-crossed 6 times; an angular
    sort about the centroid closes but leaves up to 24.5% of a body's own members outside its
    ring. A **250 m contour** costs 2.4–8.3 MB a world, and the owner world's great lake alone
    costs 1.5–3.3 MB against a 1 MB budget. The shore-point set costs 0.073 MB on the owner world.
  - **Ponds keep the 250 m trace.** A pond's surface is under 1 km² by definition, so its outline
    is a few dozen points. A pond is filled at 250 m resolution from its lowest point up to its
    level, within its coarse basin, and the outline is traced and simplified to 250 m tolerance.
    **`kind` says which of the two geometries a body carries** (§7): a lake's outline is a
    shore-point set, a pond's is a traced curve.
- **Small lakes and ponds.** A fine hollow search (250 m cells) runs only within 3 km of refined
  river lines, and in terrain with wetness above the 60th percentile and slopes under 3%. It has
  **its own keep rule: depth ≥ 2 m and area ≥ 0.05 km²**. Section 6.3's rule (area ≥ 1 km²)
  could never keep a pond, whose surface is under 1 km² by definition. Found hollows that fail
  are simply not recorded; at this scale they are texture and need no notch. At most one small
  lake or pond per 500 km² of searched area is kept, the deepest first.
  - **A find at or above `pond_max_area_m2` is not recorded** (Ruling T1-2, plan 1b-3's Task 1).
    This search finds *small* lakes and ponds. A find that large has no coarse basin and therefore
    no shore points, so it could only be recorded as a traced curve — which would break `kind` as
    the discriminator between the two geometries. It is dropped and counted in the record's stats,
    rather than recorded as a `lake` whose outline is a curve. A body that large which the coarse
    graph *did* resolve is a coarse body already and is unaffected.

### 6.7 Waterfalls

Wherever a refined reach falls at least 10 m over at most 150 m of its length, a waterfall is
recorded with its height and its two ends (Mark 2 section 13.3). Maritime reads it as the
upstream limit of navigation.

- A fall is recorded as its upper end (`falls[].at`) and height. Both ends are inserted as reach
  points, protected from simplification, so stage 2 later carves a real step there instead of
  smoothing it into a ramp. The lower end is the next point on the reach (Ruling R-5).

## 7. The hydro record

Stored in the worldfile as a `hydrology` block:

```
hydrology: {
  version: <hydro schema version>,
  generator_version: <GENERATOR_VERSION the bake ran under>,
  land_fingerprint: <hash of planet params + features + engine version>,
  params: { thresholds, keep rule, pond limit, meander, search limits },
  adjustments: [ ...owner tweaks, section 9.2... ],
  bodies: [ { id, kind: lake|salt_lake|salt_flat|pond, fresh, level_m,
              outline: [[lat, lon], ...], shore_member_count, shore_reach_m,
              outlet: reach id | null, override: null|"forced"|"closed" } ],
  reaches: [ { id, class, fresh, downstream: reach id | body id | "ocean",
               points: [[lat, lon, bed_m, width_m, depth_m, flow], ...] } ],
  notches: [ { points: [[lat, lon, surface_m, width_m], ...] } ],
  falls:   [ { reach, at: [lat, lon], height_m } ]
}
```

- **`kind` says what `outline` is** (Ruling S-1 as replaced, and Ruling T1-2, both plan 1b-3's
  Task 1). The field carries one of two geometries and **`kind` is the discriminator** — never
  `shore_member_count`, and never any other sentinel value. `kind` is one of the four
  `BodyKind` variants (`hydrology/mod.rs`), and **three of them take the shore-point branch and
  one takes the traced curve**:
  - **`lake`, `salt_lake`, `salt_flat`: `outline` is a set, not a curve.** Its first
    `shore_member_count` points are the body's shore members and the rest are its collar (§6.6).
    Within each half the points are in ascending graph-node order; that order is fixed only so the
    record is deterministic and carries no geometric meaning. **No consumer may join consecutive
    points into an edge.** `shore_reach_m` is the greatest length of a member-to-collar step whose
    collar end stands above the body's level, and it is what bounds the extent in §8.3's test.
  - **`pond`: `outline` is a traced 250 m curve** (§6.6), and **its points are joined in order.**
    A pond writes `shore_member_count = 0` and `shore_reach_m = 0.0`. **Both are unused on this
    branch** and are written only so the encoder and its wire twins have a definite value; a reader
    that consults them instead of `kind` is reading the record wrong.
- **A fall's `at` is its upper end**; the lower end is the next point on that reach, and the bed
  drops by `height_m` between them (Ruling R-5, §6.7).
- **A reach point's third value is the bed**: the water surface there minus the channel's depth.
  **A notch point's third value is the cut surface**: the lowered ground, which is the water
  surface through the cut. Where a notch point and a reach point sit on the same place, the notch
  value minus the reach's depth is the reach's bed, and the two widths are equal.
- **Body `fresh` means "not closed"**: the lake has an outlet, though its water may still end in a
  closed lake downstream. **Reach `fresh` means "its chain reaches the ocean"**.

- **Size target:** under 8 MB of JSON for the owner's world, measured in stage 1. Coordinates are
  rounded to 1e-5 degrees and levels to centimetres.
- **The fingerprint decides freshness.** On opening a world, a matching fingerprint loads the
  record as-is. A mismatch keeps the old record drawn, marks it **out of date**, and offers the
  bake.
- **Versioning:** the bake and the water layer change what a seed produces, so
  `GENERATOR_VERSION` is bumped per VERSION-001 (`lib.rs:44–90`). A worldfile from before the
  bump opens with no `hydrology` block. It is treated as out of date, and nothing is inferred.

## 8. Water in the ground

### 8.1 The water layer

This is a new stage in `Surface`, after features and before detail:
- **River channels:** a trapezoid cut to `bed_m` along each refined reach. It is `width_m` wide
  at the bank, with banks blended over one width either side.
- **Notches:** cut the same way.
- **Lake beds:** not cut. A kept lake is an existing hollow; the lake's outline and level decide
  the water surface.
- **Detail damping:** the layer's authority (1 inside a channel, falling to 0 at the blended bank)
  multiplies detail amplitude by `1 − authority`, exactly as `Features::apply` does. So texture
  cannot dam a river or raise an island in mid-channel.

### 8.2 Spatial index

- **Structure:** a fixed cube-sphere cell grid of about 50 km cells, built once per record. Each
  cell lists the reach segments, notch segments, lake outlines and **painted features** whose
  influence reaches it.
- **Use:** a sample looks up its cell and tests only what the cell lists.
- **Performance target:** on the owner's world with its full record, the median cost of
  `elevation_m` rises by no more than 20% over the same world with no water. With 1,000 painted
  features, it is at least 10× faster than today's linear scan.

### 8.3 The query

`water_at(point) -> { kind, level_m, depth_m, fresh, body_id }`

| kind | when |
|---|---|
| `ocean` | below the datum and connected to the ocean (Ruling W1) |
| `lake` / `salt_lake` / `salt_flat` | inside a body of that kind's extent — the **shore-point** test — and at or below its level (Ruling S-1, as replaced) |
| `pond` | inside a pond's **traced 250 m curve** and at or below its level |
| `river` | within half a reach's width of its centre line; the level is the bed plus depth |
| `none` | anything else |

- **Inside a body's extent** (Ruling S-1, as replaced by plan 1b-3's Task 1) is decided from the
  body's recorded shore points (§7), in one pass over them, for each body §8.2's cell lists as
  reaching the point. **Which branch is taken is decided by `kind`** (Ruling T1-2), not by any
  sentinel in the data:
  - **`lake`, `salt_lake`, `salt_flat`.** Let `dm` be the great-circle distance to the nearest
    **member** point of that body and `dc` the distance to its nearest **collar** point (ties to
    the lower outline index; no collar point means `dc` is infinite). The point is inside if
    `dm <= dc` **or** `dm <= shore_reach_m`. The first clause holds the body's interior, the second
    the shore band the level contour crosses — and because the contour crosses each member-to-collar
    step whose collar end is above the level somewhere along that step, and `shore_reach_m` is the
    longest such step, every point of the contour is inside.
  - **`pond`.** Its outline is a traced curve and its points are joined in order; the point is
    inside if it is inside that curve. `shore_member_count` and `shore_reach_m` are not consulted.
  - **Where more than one body claims the point, the smaller `dm` wins; ties go to the lower body
    id** (Ruling T1-3). `shore_reach_m` is a per-body maximum, so a ridge narrower than it really
    can put a point inside two extents at once, and without this rule the answer would depend on
    the order §8.2's cell happens to list the bodies in. A pond's curve test yields to a lake only
    through this same rule, taking the pond's distance to its own nearest outline point as its `dm`.
  - `ocean` is still decided first.
- **Exposed as:** a wasm export (and a per-tile batch form for the relief workers) and a PyO3
  binding. The PyO3 work adds the missing `surface_open` family the Python oracle already calls.
- **Maritime:** the manifest of Mark 2 section 13.2 is produced from the record. Bodies become
  named waters and reaches become ordered reaches with bed gradient. A lake's level ignores tide,
  so **the great lake stops having tides**; that is intended.

## 9. The studio

### 9.1 Bake and drawing

- **"Work out the water"** runs the bake in a pool worker. A progress line names the step:
  flooding, judging hollows, tracing rivers, outlining lakes, finding ponds. Terrain edits queue
  behind it. Slider changes mark the water out of date instead of re-running it.
- **Drawing:** relief tiles colour water texels from the batch query. Lakes and ponds use
  `LAKE_STOPS` at their true depth, rivers are tinted by class and drawn from their lines, and
  salt water is tinted distinctly from fresh. `dilateBodyExtents`, the box-and-level rule and the
  square point bodies are removed.
- **A water panel:** counts by class, lakes, ponds, salt bodies, waterfalls, the bifurcation
  ratios, the record size, and the bake time.

### 9.2 Adjustments, replacing painting as world design

The adjustment kinds are:
- nudge a reach (move one refined vertex within its corridor)
- set a lake's level (within its basin)
- force or close an outlet
- add a spring (a new reach source)
- add a pond
- add a peak (a raise feature, kept for the mountains project)

**Rules:**
- Adjustments are stored in `hydrology.adjustments` and re-applied after every bake.
- An adjustment that no longer applies after a re-bake (for example, its reach has gone) is
  listed as **stale** in the water panel and never dropped silently.
- The existing paint tools stay for raise and carve features. They are documented as tweaks, and
  they go through the new index.

## 10. The Python generator

- **`water_at` via the binding replaces:**
  - `river_points` and `lake_points` (fresh water becomes proximity to fresh bodies and reaches,
    measured by the query)
  - the `at < 0` water tests in docks, ramps, `rooms_under_water`, `dry_enough` and ferries
- **Ferries** treat the great lake as navigable fresh water. Its outlet river is a real route to
  the ocean.
- **Halflings' `needs: fresh`** is then met by real rivers. The "settled for less" pass stays as
  the fallback it was meant to be.

## 11. The mountains project, and the coastline lock

- **Order.** This water work comes first, and the mountains project second. Mountains get a
  separate spec covering ranges with spines, spurs, summits and passes, foothills and plateaus,
  and valleys carved by drainage using Cordonnier stream-power erosion (Mark 2 section 14),
  driven by this pass's graph.
- **Re-baking.** After a mountain change the water is simply baked again (Ruling W2).
- **The coastline lock** is a property test shared by both projects. At a fixed scatter of one
  million points, land-or-water classification must be identical before and after any change that
  claims to keep the coast. Any difference fails the change.

## 12. Fixing the owner's world

1. Copy `worlds/world-1788998299904.json` to `worlds/world-1788998299904.before-water.json`,
   and never overwrite it.
2. **Trial (end of stage 4):** open the current planet, bake, and check the great lake, its
   outlet, the rivers and the counts. The result is saved as a separate trial world, not over the
   original.
3. **Final (after the mountains project):**
   - apply the new mountains and pass the coastline lock
   - bake the water
   - regenerate the 400 areas, and curate if wanted
   - save under a new name the owner chooses, keeping the backup

## 13. Stages

Each stage has its own implementation plan, its own tests and its own review. It lands on its
own branch or PR.

| Stage | Delivers | Done when |
|---|---|---|
| 1. The bake (two plans: **1a** coarse, **1b** refinement) | 1a: land graph, priority flood, hollow judgement, flow, reaches, the record at graph resolution, the wasm export, parity, calibration. 1b: refinement (tracing, outlines, small lakes and ponds), falls. A native survey binary and a wasm export. | The record is bit-identical native vs wasm (parity harness), the section 14 properties hold, and calibration is reported on the owner's world. |
| 2. Water in the ground | Water layer, spatial index (features too), `water_at` in wasm and PyO3, the `hydrology` block and fingerprint, the version bump. | The index meets the section 8.2 targets, conformance and parity are green with the counts updated, and `water_at` agrees with the record at sampled points. |
| 3. The studio | Bake worker and progress, per-texel drawing, water panel, adjustments. The old box drawing is removed. | Screenshots of the owner's world show true shorelines and rivers, adjustments survive a re-bake, and stale adjustments are shown. |
| 4. Generator and trial | `water_at` in the generator, the trial on the owner's world. | Fresh water is found from real water, a 400-area trial run places halflings on real rivers, and the trial world is saved beside the original. |

## 14. Properties the tests hold

These are properties, not a Python twin. Water has no Python oracle, by the slice 5b ruling.

1. **Deterministic:** the same inputs give a byte-identical record, native and wasm.
2. **Everything drains:** every land node reaches the ocean, a lake with an outlet, or a closed
   lake. Nothing is left in an unjudged hollow.
3. **No pit lakes:** no kept body lies below the keep rule unless it has `forced`.
4. **Connected rivers:** every reach's `downstream` exists, the network is acyclic, and every
   tributary shares its junction vertex with its receiver.
5. **Downhill:** along a reach the bed never rises except at a recorded notch, where it holds.
6. **Lakes drain:** every non-closed lake has exactly one outlet reach, starting on its outline.
7. **Earth-like:** the bifurcation ratio is between 3 and 5 on the owner's world at the
   calibrated thresholds, reported alongside the counts.
8. **Coastline lock:** baking never changes land-or-water classification at the million-point
   scatter, except inside recorded river channels and notches.
9. **The query agrees with the record** at every sampled point of every body outline and reach.
10. **Mutation guards:** each property test is shown to fail against a deliberately broken input
    (a cyclic reach, an undrained hollow, a moved coast). A property that cannot fail proves
    nothing.

## 15. Risks and measurements that decide details

- **Bake memory in the browser** decides the land-node count (section 6.1). Measure first, and
  lower the count before raising any ceiling.
- **Bake time** on the owner's world is reported per step. Minutes are acceptable; tens of
  minutes are not, and would mean running the refinement natively in the server as a documented
  fallback.
- **The inland sea's outlet** depends on the landform, not the full-detail surface. If the lowest
  landform ridge differs from the measured 26.00°S 30.25°W, the bake's answer stands and is
  reported.
- **Maritime's tide model** loses tides on the inland sea when it becomes a lake. The contrib
  keeps its own repository and its own rules; that consequence is reported to it, not patched
  from here.
