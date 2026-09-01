# Changelog

All notable changes to Worldbuilder by Aetos.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). There is no
released version yet; everything below is Mark 1, the prototype whose purpose is to give
the maritime contrib a planet to demonstrate on and to find out what a real generator will
need to get right.

Entries are phase by phase, because each phase was scoped, built, measured and reviewed
before the next began. Where a number appears it was measured rather than chosen, and
where a bug is described it is because the shape of it is worth keeping.

## [Unreleased]

### Added — M1.8, what the bottom is made of

- `worldbuilder/bathymetry/substrate.py` — sand, mud and rock, as a **composition** rather
  than a category. Three fractions summing to one, each varying smoothly; the one-word
  answer is whichever is largest, and nothing continuous is ever computed from the word.
  Three names would otherwise be about as hard a decision as this engine contains.
- Derived from slope, depth and tectonic contribution, and overridden smoothly by anything
  placed. A pinnacle is rock because somebody said so; a dredged basin is mud because that
  is what settles in still water behind a mole.
- `Composition.holding` — how well an anchor bites, expressed from the fractions rather
  than the word, because a bottom that is half rock is genuinely half as good.
- `Feature.substrate`, `Surface.bottom_at`, and a substrate on every placed feature.
- `worldbuilder/debug/bottom.py` — the composition drawn as a mixture, not three flat
  colours, so a boundary that should be a gradient cannot hide as one that is an edge.

### Fixed — M1.8

- **The slope probe could not see what it was measuring.** A finite difference is blind to
  anything narrower than its baseline, and at six hundred metres it straddled the pinnacle:
  the bottom a hundred and thirty metres from a rock standing twenty metres proud read
  perfectly flat, while the bottom three hundred metres away read steep because one probe
  landed on it. Rock came out in *rings*. The baseline is sixty metres now — smaller than
  the narrowest thing placed — which costs nothing, because measured across the planet the
  structural slope distribution is identical at 300 m, 600 m and 2 km. Structure is smooth
  at every scale; only features and detail are not.
- **A gentle regional swell was being called rock.** At a 400 m threshold on tectonic
  contribution, the whole demonstration coast came out a third rock, because a passive
  margin carries about 150 m of broad tectonic rise. Twelve hundred metres is the scale of
  real tectonic structure — a trench wall, a ridge crest — and the slope term is already
  there to catch steepness.

### Measured — M1.8

A bottom costs 661 µs against 150 µs for a sounding — 4.4×, which is four probes and a
frame. Affordable because a ship sounds continuously and anchors once. The steepest
*structural* ground anywhere on this planet is 1.4 % at any baseline, so on Mark 1 worlds
the slope term fires almost entirely on placed features, where slopes reach 27 %.

### Added — M1.7, placed features and the marks layer

- `worldbuilder/bathymetry/features.py` — a small explicit list of things somebody put
  somewhere, stamped onto generated ground. `RAISE`, `CARVE` and `SHAPE` say how each
  argues with what is already there, so a bank inside a channel cannot cancel out into
  ordinary seabed the way added offsets would have let it.
- `worldbuilder/regions/demo.py` — the demonstration coast: harbour basin, entrance, two
  moles, bar, approach channel, two flanking banks, a drying rock, an isolated pinnacle, a
  headland and steep-to water off it. Constructed rather than found, because searching the
  globe for a good natural harbour needs the global enumeration pass Mark 1 exists to
  avoid.
- `Features.marks_near` — the second channel. A hundred-and-forty-metre pinnacle cannot
  survive a chart sampled every four hundred metres, so charts carry isolated dangers as
  symbols while the terrain carries the rock at full height for anything that can ground
  on it.
- **What gets marked is derived, not judged.** A feature is marked exactly when a chart
  would print more water over it than there is. The first rule was a size heuristic and it
  was wrong: it left the moles, two kilometres long and three hundred and forty metres
  wide, over which a four-hundred-metre grid prints six metres of water where a four-metre
  breakwater stands, and the harbour bar, over whose three-metre crest it prints seven.
- Detail now defers to placed features, in proportion to how much relief each is actually
  asserting. Coastal roughness runs to thirty-five metres and would have erased a bar
  standing three metres proud of the bottom.
- `worldbuilder/debug/harbour.py` — the region drawn twice, as truth and as a chart would
  sample it, in depth bands rather than a smooth ramp.
- `setup.cfg` — flake8 configuration, which the repository had never had.

### Fixed — M1.7

- Three placement bugs, all found by measuring rather than by looking at the render. The
  approach channel reached over the harbour bar and dredged it away. The flanking banks
  were given an alongshore bearing, which made them thirteen kilometres long *parallel to
  the beach* and put both of them on top of the channel they were meant to flank. The
  channel was then stated at fifteen metres on a shelf already twenty-five metres down,
  where a one-way carve correctly did nothing at all.
- The demonstration anchor, twice. The land gradient had been measured at a sampled
  candidate twenty kilometres inland of the actual shore, giving a "coast" whose land rose
  at four tenths of a metre a kilometre — the harbour cut into it was a slightly deeper
  patch of submerged plain. The seaward bearing had then been taken from the steepest
  descent of *continentality* rather than of the finished field, which put the alongshore
  axis at an angle to the beach: a line meant to run parallel to the shore went from
  fourteen metres of land at one end to fourteen of water at the other. It now holds
  between 6.3 and 7.5 metres over sixteen kilometres.
- Unused imports in `shelf`, `kinematics`, `plate_map`, `projection_error`, `model` and
  two test modules; over-long lines throughout. All pre-existing, all invisible until
  there was a lint configuration to find them.

### Known — M1.7

Placed features hold their stated shape exactly; the ordinary shelf around them carries
twelve to fifteen metres of detail displacement in four to ten kilometres of water. That is
inside everything M1.6 measured and asserted, so it is not a regression, but it is more
roughness than a demonstration channel wants. Retuning it belongs to a phase that can
re-measure M1.6's coastline-shift table afterwards. Recorded rather than silently adjusted.

### Measured — M1.7

Sixty-four chart grids swept across an isolated pinnacle, at three resolutions. The rock
stands twenty-four metres proud of a twenty-eight-metre bottom and is a hundred and forty
metres across:

| chart spacing | grids that found it | shoalest sounding printed |
|---|---|---|
| 400 m | 1 in 64 | −21.6 m to −3.5 m |
| 200 m | 5 in 64 | −21.1 m to −3.5 m |
| 100 m | 21 in 64 | −20.9 m to −3.5 m |

Sampling finer buys hit rate, not certainty, and the spread is the real problem: whether
the danger appears depends on where the grid happens to fall, so it would blink in and out
as a ship moved. That is the argument for the marks layer, and it is a measurement.

## M1.6 — detail that knows how closely it is being looked at

### Added

- `worldbuilder/terrain/detail.py` — roughness in two bands, twenty kilometres down to two
  hundred and fifty metres. The first layer in the engine that answers differently
  depending on how finely it is being sampled.
- `worldbuilder/terrain/surface.py` — the assembled world.
- `worldbuilder/debug/lod_shift.py` — coastline displacement by sampling resolution.

### Decided

- **Detail is texture, never structure.** Noise can make coves, shoals and little islands,
  and if it did, every hazard on a chart would be an accident of a noise spectrum rather
  than a thing somebody put there. Amplitude stays well under the relief it decorates.
- **Octaves fade rather than switch off**, between twice and four times the sample
  spacing. Dropping one the instant it stops being representable would be a cliff in
  *resolution* — the same bug M1.4 produced four times, moved into a different axis.
- **Canonical is a defined thing**: every configured octave down to two hundred and fifty
  metres, not infinite detail. Without that written down, a fifty-metre octave added later
  would silently change every coastline in every world.

### Fixed

- The assembled pipeline cost a hundred and thirty microseconds a sample against the
  thirty-five the shelf measured alone, because the top layer asked the shelf for its
  weight and the tectonics for their offset separately — recomputing the gradient twice
  and the plate work three times, behind a comment claiming otherwise. One pass now.

### Measured

| sampling | mean coastline shift | worst | shorelines lost |
|---|---|---|---|
| canonical | 0.00 km | 0.00 km | 0 |
| 500 m | 0.10 km | 0.50 km | 0 |
| 2 km | 0.40 km | 0.50 km | 0 |
| 10 km | 4.80 km | 7.00 km | 0 |

A 96×96 chart: canonical 929 ms, 500 m 456 ms, 10 km 321 ms. Band-limiting does real work.

## M1.5 — the shelf, and the water a ship actually sails in

### Added

- `worldbuilder/bathymetry/shelf.py` — coastal bathymetry blended over the macro terrain.
  A target depth blended towards, never an offset added on, so a trench crossing a
  continental margin survives instead of being filled in.
- `Continentality.above_shore`, because sea level is a calibrated quantile of the field
  and nowhere near its zero.

### Decided

- **A performance gate must sit outside the support of what it gates, or fade to nothing
  before it.** The general form of M1.4's worst bug. Four weights replace four decisions
  that could each have been a hard test.

### Fixed

- The seaward exponent from M1.3 was 0.5, which put the seabed a thousand metres down
  eighty kilometres offshore and left the shelf inventing nine hundred metres of ground
  rather than shaping a margin. Linear puts it at about two hundred and forty.
- Tectonic authority came down from seven hundred metres to two hundred and fifty; at
  seven hundred the shelf dragged a coastal mountain range down to a hundred and
  twenty-five metres.
- The distance estimate divided the raw field by its gradient and reported coastlines
  fifteen hundred kilometres away.

### Measured

Full terrain 34.79 µs a sample; 12.4 % of samples ever pay for a gradient.

## M1.4 — tectonic terrain

### Added

- `worldbuilder/terrain/tectonics.py` — uplift belts, island arcs, trenches and ridges, as
  a contribution added to the continental base rather than an elevation of its own.

### Fixed

Four discontinuities, all one species: **a hard decision taken on a continuous quantity.**

| what | how | how big |
|---|---|---|
| branch on crust type | hard test on continentality | 552 m |
| branch on `motion.kind` | hard test on a threshold | 1,172 m |
| scaling the axis by lean | not a threshold — worse | 926 m |
| the phantom-bisector filter | hard test on cell membership | 143 m |

Picking the nearest margin could never have been continuous: the distance is smooth but
the neighbour's *identity* jumps, and the relative motion, the normal and the ground on
either side jump with it. All margins in range are summed now.

## M1.3 — continents that owe nothing to the plates

### Added

- `worldbuilder/terrain/continentality.py` — an independent field, not derived from plate
  geometry. Plates carry motion; crust is its own thing.
- Sea level by quantile calibration against a target land fraction.

### Fixed

- Land fraction read 24.6 / 46.2 / 33.9 % across three seeds after calibration, which was
  the diagnostic being wrong rather than the generator: equirectangular pixels are not
  equal-area. Weighted by cos(latitude) it reads 28.9 / 29.1 / 28.8 %.

## M1.2 — plates that move

### Added

- `worldbuilder/plates/` — plates, Euler poles, angular velocity, nearest-two lookup,
  margin distance, and boundary classification derived rather than stored.

### Fixed

- A 531 km discontinuity in margin distance, from measuring only the second-nearest
  bisector: |A−B| and |A−C| differ, so the argmin changing makes the distance jump. The
  minimum is taken over all bisectors now.

## M1.1 — the projection, and the region cap it earns

### Added

- `worldbuilder/geometry/` — `Vec3`, `SpherePoint`, `TangentFrame`. Canonical position is
  a unit vector, so there is no seam and no pole singularity.
- `worldbuilder/debug/projection_error.py`.

### Decided

- **Azimuthal equidistant**, and a **200 km region cap** measured rather than asserted: at
  200 km the worst error between two charted points is 5.68 m, well under a ship length
  and far under the four hundred metres between printed soundings. At 500 km it is 89 m.
- The error does not depend on latitude — identical at 0°, 45° and 80° to six decimal
  places, which is a real consequence of working in unit vectors.
