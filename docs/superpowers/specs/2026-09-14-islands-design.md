# Islands — design

**Status:** design, awaiting owner review.
**Author:** this session, from measurements taken 2026-09-13/14.
**Spec it extends:** the generator's surface stack (`continentality.rs`, `tectonics.rs`,
`shelf.rs`, `detail.rs`).

---

## 1. The problem, measured

This generator makes no islands. Not few — none.

- Over 20,000 area-uniform points on the owner's world, **no point of open ocean rises above
  the datum**. The 65 points the finer layers lift out of water are all shoreline wobble, the
  deepest by −634 m.
- The maximum sub-datum elevation offshore is −0.0 m at every plate count tested (3, 8, 22,
  48) — that value is the shoreline itself, not a shoal.
- The island-arc code exists and cannot work. `ISLAND_ARC_M` is **700 m**
  (`tectonics.rs:101`) against `ABYSS_M` of **−4,600 m** (`continentality.rs:25`). It is
  short by a factor of about six.

So every archipelago in every world so far has been authored by hand, which is why the demo
coast places its six islands individually.

**The consequence beyond scenery:** a maritime bundle baked from this generator ships
soundings and an empty danger list, because there is nothing offshore for a hull to strike.
An accurate chart of nothing.

## 2. What we are building

Three mechanisms, because they produce three different things a world wants, and they are
independent enough to land separately:

| | Produces | Lives in | Analogue |
|---|---|---|---|
| **Peaks** | Sparse tall cones rising straight from the abyss, singly and in small groups | Above continentality, post-calibration | Hawaii, the Azores |
| **Arc crests** | Chains of islands strung along oceanic-oceanic plate margins | `tectonics.rs`, on the existing arc | Japan, the Aleutians |
| **Fragments** | Larger islands with their own continental shelves | *Inside* continentality, pre-datum | Britain, Madagascar |

### 2.1 Why fragments go in a different place from the other two

This is the load-bearing design decision.

`Shelf` derives a continental shelf from **continentality's gradient** — it reads where the
land/sea field falls away and builds the break, the slope and the inland reach from it. So a
landmass expressed *in* the continentality field gets a shelf for free, with the correct
bathymetry around it, and `populate` will measure real harbour approaches on it.

A landmass expressed *above* continentality gets no shelf: it rises straight out of deep
water. Which is exactly right for a volcanic peak and exactly wrong for Madagascar.

Therefore:

- **Fragments** are a term inside `Continentality`, added to the field before the datum
  comparison. They inherit shelves, gradients and everything downstream of them.
- **Peaks** and **arc crests** are added after, to elevation. They are steep-to, unshelved,
  and a hull can be in 2,000 m of water a kilometre off the beach — which is what such an
  island is actually like, and is navigationally interesting in a way a shelf is not.

### 2.2 Where peaks actually go — corrected after reading the code

The first draft of this spec said peaks should copy the coast-roughening term's slot in
`Continentality::above_shore`. **That is impossible, and the reason is worth recording.**

- `Continentality::at` does **not** return metres. It returns the raw normalised fBm value,
  roughly `[-1, 1]` (`continentality.rs:362-365`). Metres come out of `elevation_from_above`
  (`:482-501`).
- That function **caps its input at 1.0**, so the most any perturbation of `above_shore` can
  produce is `CONTINENT_M * 1.0^0.75` = **700 m**. A 4,700 m cone is arithmetically
  unreachable through that slot — the same ceiling that defeats the island arc, reached by a
  different road.
- Worse, raising `above_shore` at a mid-ocean point pushes `|value|` outside
  `COASTAL_WINDOW = 0.055` (`shelf.rs:140`), so `Shelf::coastal` returns `None` and the
  shelf machinery switches *off*. Which is right for a peak and fatal for a fragment.

**Peaks go into `Tectonics::offset_m` instead** (`tectonics.rs:843`), and this turns out to be
the better site on the merits rather than a fallback. `Shelf::evaluate` reads that offset as
`tectonic` and puts it in `macro_elevation` *and* in `Reading.tectonic_m`, and three
downstream consumers key off `tectonic_m` in exactly the way a volcanic island wants:

| consumer | effect of a 4,700 m tectonic offset | why that is right |
|---|---|---|
| `Shelf::weight` authority (`shelf.rs:223`) | `1 − smooth(4700/250)` = **0** | the shelf is held off structurally, so the peak is steep-to by construction rather than by accident |
| `Detail::amplitude_m` quieting (`detail.rs:589`) | saturates | surface texture defers to the cone instead of covering it |
| `substrate::natural` (`substrate.rs:200`) | `smooth(4700/1200)` saturates → **100% rock** | a volcanic island is rock, and this comes free |

Putting peaks in `Shelf::evaluate`'s `macro_elevation` instead would get none of those: a
mid-ocean cone would receive *undamped mountain roughness* and read as sand where it wasn't
steep. Volcanism is a tectonic phenomenon, so the tectonic offset is where it belongs both
physically and mechanically.

Fragments still cannot use the coast slot either — they need the gradient to move so `Shelf`
draws a real break, and only `Continentality::at` does that. That is the most invasive site in
the file (it feeds calibration and the tectonic margin probes), which is the real reason
fragments land last.

## 3. Hard requirement — an absent block changes nothing

Islands are an **optional parameter block**, in the idiom this crate already uses for
`relief`, `tectonics`, `coast` and `gully`: a struct with a `canonical()` preset, an
`Option<IslandParams>` on `Surface`, and a `wb_world_new_island` constructor beside the
others.

**When the block is absent, every generated value must be bit-identical to today.** Not
close — identical.

This is a requirement, not an aspiration, and it buys three things:

1. The owner's saved world reloads exactly as it is. Its 400 areas, 394 roads, 12 ferry lines
   and its inland sea are untouched until islands are switched on deliberately.
2. **No parity pin moves, and no conformance pin moves.** The corpus is unaffected.
3. **`GENERATOR_VERSION` does not need a bump.** The bump test in `lib.rs:65-71` is "the same
   seed *and the same parameters*, run through the new code, would produce a different world."
   With the block absent, it would not.

The test for this is a parity run with no island block, asserting zero divergent values
against the existing corpus. If that test cannot be made to pass, the design is wrong and not
the test.

## 4. The three terms

### 4.1 Peaks

A separate noise field, windowed to deep water, thresholded so that only rare maxima break
the surface.

- **Sparsity from a lattice, not a threshold.** A thresholded fBm gives no direct control over
  *how many* islands a world gets. Instead, use `Noise::lattice_at` — one candidate peak per
  lattice node, its existence decided by a hashed value against a density knob, its position
  jittered within the cell, its height drawn from a third salt. This is the pattern the gully
  kernel already uses for its pivots (`detail.rs:511-512`), and `lattice_at` is `pub(crate)`
  and documented for exactly this: "its pivots ARE lattice nodes and what it needs from each
  is a fixed per-node jitter rather than a field."
  **Density and height then become independent knobs**, which they are not if sparsity comes
  out of an amplitude.
- **Window.** Zero unless the surrounding seabed is deep, so a peak cannot erupt on a
  continental shelf. Read the window off `Continentality::base_elevation` (metres, so the
  threshold is stateable as a depth) rather than off `above_shore`.
- **Amplitude.** Must clear `|ABYSS_M|` — of order 4,700 m — or nothing surfaces. This is the
  number that defeats the existing arc term and must not be repeated.
- **Footprint.** A cone 10–40 km across. Detail's canonical wavelength is 250 m, so the shape
  stays well above the roughness floor.
- **Beware the gully lattice.** `GullyParams::canonical().steer_lattice_m` is 2,000 m, so a
  cone narrower than about 2 km will alias in the steering lattice when a gully block is
  active. Keep the footprint well above that.

### 4.2 Arc crests

The existing arc is a broad 110 km rise. A real island arc is a **narrow crest on a broad
rise**, and only the crest surfaces — which is why arcs are chains rather than walls.

So: keep `ISLAND_ARC_M` and `ISLAND_ARC_WIDTH_M` as they are, and add a crest term along the
arc axis — narrow, tall, and modulated along its length so it surfaces intermittently.

Raising `ISLAND_ARC_M` to 4,700 instead would build a 110 km wide, 4.7 km tall ridge: a
continent shaped like a bow, not an archipelago. **Explicitly rejected.**

This is the one mechanism that makes plate count matter for geography. Continentality cannot
see plates — land fraction is bit-identical at 3, 8, 22 and 48 plates — and the only thing
plate count changes today is how much of the planet is an active margin (2.6% at three
plates, 26.7% at forty-eight). Arc crests turn that into visible islands, so a high-plate
world finally looks different rather than merely being different.

### 4.3 Fragments

A mid-frequency term added to the continentality field, windowed away from existing
continents so it calves separate masses rather than growing the ones already there.

- **In the field, pre-datum**, so `Shelf` builds a shelf around each fragment.
- **Windowed off the continents** by `above_shore`, on the far side of the same window peaks
  use: fragments want water that is deep, but not the deepest abyss, mirroring where real
  continental fragments sit.
- **Frequency between continentality's coarsest octave and the coast term's**, so a fragment
  is smaller than a continent and larger than a headland.

## 5. What this changes that callers must know

**`land_fraction` becomes the continental land fraction.** Peaks and arc crests add land
after calibration, so total land exceeds the requested figure. Fragments are inside
calibration and so are absorbed by it — the threshold moves to accommodate them, meaning
switching fragments on slightly *shrinks* the continents to keep the total at the requested
value.

That asymmetry is real and must be documented rather than smoothed over. `Surface` should be
able to report both numbers: requested fraction, and achieved fraction including islands.

**Hydrology.** Islands are land, so the hydrology bake will place nodes on them and may give
them drainage. At 1,000,000 nodes, spacing is about 33 km on the owner's world — so an island
20 km across gets zero or one node, and will have no rivers. This is acceptable and should be
stated: **islands smaller than the node spacing have no hydrology**, and that is a resolution
limit, not a bug. Fragments, being larger, will drain.

**Maritime.** This is what makes a bundle navigable. Once islands exist, the sounding-derived
shoal scan has something to find, and the derivation is provably complete relative to the
generator: nothing narrower than detail's 250 m canonical wavelength can exist, so fine
sheets at 125 m cells or below cannot miss a shoal. An "is this an island" predicate becomes
meaningful.

## 6. House rules that bind this work

- Determinism is the product. No `std` float maths outside `detmath.rs`; no `ceil` (write
  `-m::floor(-x)`); no `.abs()`; no `f64::min`/`max`/`clamp`; `total_cmp` for float sorts;
  never iterate a `HashMap`/`HashSet` into output.
- `// cast-ok:` justifying every integer and float-to-`usize` cast.
- No panic reachable from an `extern "C"` boundary — wasm exports return status codes.
- Noise through the existing `Noise`/`fbm` machinery, not a new generator.
- Every figure in a report names its population, its method with parameters, and its host.

## 7. Open questions for the owner

1. **How much island?** A target — "about 0.5% of the planet's surface as islands at the
   canonical preset" — would let the presets be calibrated rather than guessed. My
   recommendation: 0.3–0.8%, enough that a voyage meets islands and few enough that they stay
   remarkable.
2. **Do fragments belong in the first slice?** They are the invasive one: inside calibration,
   shrinking continents to pay for themselves. Peaks and arc crests are purely additive. A
   defensible order is peaks → arc crests → fragments, landing the safe two first.
3. **Should the canonical preset be non-zero?** §3 requires the *absent* block to change
   nothing. A separate question is whether `IslandParams::canonical()` — used when someone
   asks for islands without saying how much — should be a modest default or must be stated
   explicitly.
