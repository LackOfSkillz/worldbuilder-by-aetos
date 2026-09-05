//! Feature-aware availability: refine further where a feature is, and nowhere else.
//
// Task 4 capped refinement at level 12 and justified it in metres: the generated field has
// a measured resolution floor of 78.125 m, level 12 puts posts 76.35 m apart, and level 13
// quadruples the tile count to redraw the same tilted plane. That reasoning is sound **and
// it is only about generated ground**.
//
// Authored features are a different channel. `Features::apply` is analytic and sits
// entirely outside the octave schedule, so a feature is *resolution-independent
// point-wise*: the extraction's harbour reads exactly +4.00 m at its centre at
// `resolution_m` of -1, 76.35, 152.7 and 305.4 alike. But a heightmap is a **grid**, and
// the grid is what runs out. Task 4 measured the level-12 tile containing that harbour
// topping out at **-819.4 m against a +4 m target**, reaching +3.2 m only at level 16.
//
// So the cap is right for ground and four levels short for anything anybody placed. A
// viewer that caps below its own authored relief cannot show the one thing a field viewer
// is for. What follows refines past the ground cap **only inside a feature's footprint**.
//
// # The footprint is the engine's, not a guess
//
// `Placed::weight_at` opens with a gate: `if point.vector.dot(&self.feature.at.vector) <
// self.cos_reach { return 0.0 }`, where `cos_reach = cos(hypot(length_m, width_m) /
// radius_m)`. Beyond `reach_m` great-circle metres from its centre a feature contributes
// **exactly** nothing -- not "negligibly", exactly, by an early return the engine's own
// docs call load-bearing. That circle is the footprint used here.
//
// The lat/lon box below is a **conservative superset** of that spherical cap: the latitude
// pad is exact, and the longitude pad is taken at the extreme latitude of the band, which
// is the widest the cap ever gets. Over-including costs a few tiles; under-including would
// silently lose a feature, so the error is deliberately one-directional.
//
// # How deep is deep enough
//
// `bump(distance_m, half_m)` reaches zero at `half_m`, so a feature's full extent is
// `2 * length_m` along by `2 * width_m` across; the mole in this slice's harbour is
// 200 x 60 m in those terms and therefore 400 x 120 m on the ground. The rule here is
// **post spacing <= min(length_m, width_m) / 8** -- sixteen posts across the narrow
// dimension's full extent. That is not an aesthetic choice, it is fitted to Task 4's
// measured convergence at that harbour:
//
//     L12 (76.4 m) -> -819.4 m    L15 (9.5 m) -> -39.6 m
//     L13 (38.2 m) -> -457.7 m    L16 (4.8 m) -> +3.2 m
//     L14 (19.1 m) -> -40.8 m     L17 (2.4 m) -> +3.2 m
//
// `width_m / 4` would have chosen level 15, which reads -39.6 m against +4 m and is wrong.
// `width_m / 8` chooses 16, which is where it converges. `?featureCeiling=` and the
// `feature-resolves` check keep the rule falsifiable rather than asserted.

import { metresPerDegree, postSpacingM } from "./terrain.js";

/// The two ways to get this wrong, reachable by `?fault=`.
///
/// They are opposites on purpose. `feature-blind` is the Task 4 behaviour left in place --
/// the bug this whole task exists to remove, and the one a plausible-looking availability
/// hook wired to the wrong mechanism would silently leave behind. `feature-everywhere`
/// refines to the feature depth over the entire globe, which is the failure mode on the
/// other side: it renders, it looks *better*, and it is 256x the tiles for ground that has
/// nothing left to say below 78.125 m.
export const AVAILABILITY_FAULTS = {
  featureBlind: "feature-blind",
  featureEverywhere: "feature-everywhere",
};

/// A hard ceiling on feature refinement, independent of the rule above.
///
/// A one-metre feature would ask for level 18 and a ten-centimetre one for level 22, and
/// `getTileDataAvailable` answering yes forever is the same out-of-memory failure as
/// answering `undefined`. 18 is four levels past the deepest thing this slice's fixtures
/// need and puts posts 1.19 m apart.
export const FEATURE_CEILING = 18;

/// Posts across the narrow dimension's half-extent before a feature is called resolved.
export const POSTS_PER_HALF_EXTENT = 8;

/// The great-circle radius, in metres, beyond which a feature contributes exactly zero.
/// `hypot(length_m, width_m)`, the same quantity as `Feature::reach_m`.
export function reachM(feature) {
  return Math.hypot(feature.lengthM, feature.widthM);
}

/// The deepest level worth drawing for one feature.
export function featureLevel(feature, radiusM, size, ceiling = FEATURE_CEILING) {
  const narrow = Math.min(feature.lengthM, feature.widthM);
  if (!(narrow > 0)) return 0;
  const wanted = narrow / POSTS_PER_HALF_EXTENT;
  for (let level = 0; level <= ceiling; level += 1) {
    if (postSpacingM(level, size, radiusM) <= wanted) return level;
  }
  return ceiling;
}

/// The conservative lat/lon box of a feature's footprint, in degrees.
///
/// Returns `{ southDeg, northDeg, lonStartDeg, lonSpanDeg, level }`. Longitude is given as
/// a start and a span so a box that crosses the antimeridian needs no special case at the
/// call site -- the overlap test below is circular.
export function footprint(feature, radiusM, size, ceiling = FEATURE_CEILING) {
  const perDegree = metresPerDegree(radiusM);
  const padLat = reachM(feature) / perDegree;
  const southDeg = Math.max(-90, feature.latitudeDeg - padLat);
  const northDeg = Math.min(90, feature.latitudeDeg + padLat);
  // The widest the cap gets in longitude is at whichever bounding latitude is nearer a
  // pole, because a degree of longitude shrinks as cos(latitude).
  const extreme = Math.max(Math.abs(southDeg), Math.abs(northDeg));
  const cos = Math.cos((extreme * Math.PI) / 180);
  const padLon = cos <= 1e-9 ? 180 : Math.min(180, reachM(feature) / (perDegree * cos));
  return {
    southDeg,
    northDeg,
    lonStartDeg: feature.longitudeDeg - padLon,
    lonSpanDeg: 2 * padLon,
    level: featureLevel(feature, radiusM, size, ceiling),
    feature,
  };
}

/// Do two arcs of longitude overlap, on the circle?
///
/// Both are `[start, start + span]` in degrees, any start, span in `[0, 360]`. Inclusive at
/// the ends: a tile that merely touches a footprint is refined, because the alternative is
/// to argue about a boundary post.
export function arcsOverlap(startA, spanA, startB, spanB) {
  if (spanA >= 360 || spanB >= 360) return true;
  const relative = (((startB - startA) % 360) + 360) % 360;
  return relative <= spanA || 360 - relative <= spanB;
}

/// Build the `getTileDataAvailable` function.
///
/// **It never returns `undefined`.** The prototype's `undefined` falls through to
/// `HeightmapTerrainData.isChildAvailable`, which is always true for the default child
/// mask, and refinement is then bounded only by screen-space error: Task 4 measured
/// `maxDepthVisited` going 13 -> 16 -> 18 -> 22 -> 25 with the heap still climbing.
///
/// And the gate that makes a `false` bite is `QuadtreePrimitive.visitTile`'s
/// `allAreUpsampled`, not `GlobeSurfaceTileProvider.canRefine` -- `canRefine` returns
/// `childAvailable !== undefined`, which is **true when the answer is `false`**. So a
/// `false` here works by making the child fail, be upsampled from its parent, and stop the
/// parent refining once all four of its children are upsampled. One available child among
/// the four is enough to keep a branch alive, which is exactly the behaviour wanted: the
/// quadtree walks down to a feature and nowhere else.
export function createAvailability({
  radiusM,
  size,
  groundMaxLevel,
  features = [],
  ceiling = FEATURE_CEILING,
  tilingScheme,
  fault = null,
}) {
  const footprints = fault === AVAILABILITY_FAULTS.featureBlind
    ? []
    : features.map((f) => footprint(f, radiusM, size, ceiling));
  const featureMaxLevel = footprints.reduce((a, f) => Math.max(a, f.level), groundMaxLevel);

  /// Which footprints, if any, want a tile drawn at this level.
  function wanting(x, y, level) {
    if (footprints.length === 0) return [];
    const r = tilingScheme.tileXYToRectangle(x, y, level);
    const southDeg = (r.south * 180) / Math.PI;
    const northDeg = (r.north * 180) / Math.PI;
    const westDeg = (r.west * 180) / Math.PI;
    const spanDeg = ((r.east - r.west) * 180) / Math.PI;
    return footprints.filter((f) => (
      f.level >= level
      && f.northDeg >= southDeg && f.southDeg <= northDeg
      && arcsOverlap(westDeg, spanDeg, f.lonStartDeg, f.lonSpanDeg)
    ));
  }

  const available = (x, y, level) => {
    if (level <= groundMaxLevel) return true;
    if (level > featureMaxLevel) return false;
    // The other way to be wrong: yes everywhere down to the feature depth. Never
    // `undefined`, bounded, renders beautifully, and 256x the tiles.
    if (fault === AVAILABILITY_FAULTS.featureEverywhere) return true;
    return wanting(x, y, level).length > 0;
  };

  available.footprints = footprints;
  available.featureMaxLevel = featureMaxLevel;
  available.groundMaxLevel = groundMaxLevel;
  available.wanting = wanting;
  return available;
}

/// Exactly which tiles feature-aware availability adds, level by level.
///
/// **This is the cost, and it is enumerable rather than estimated.** The extra tiles are
/// reachable only by descending from an available parent, so the set is built the same way
/// the quadtree walks it: start with the ground-cap tiles that intersect a footprint, then
/// keep the children the availability function says yes to. A whole-globe scan at level 16
/// would be 8.6 billion tiles; this is a few dozen.
export function extraTiles(availability, tilingScheme, groundMaxLevel) {
  const footprints = availability.footprints;
  if (footprints.length === 0) return [];
  let frontier = new Map();
  for (const f of footprints) {
    const corners = [
      [f.southDeg, f.lonStartDeg], [f.southDeg, f.lonStartDeg + f.lonSpanDeg],
      [f.northDeg, f.lonStartDeg], [f.northDeg, f.lonStartDeg + f.lonSpanDeg],
    ];
    for (const [latitudeDeg, longitudeDeg] of corners) {
      const carto = Cesium.Cartographic.fromDegrees(
        ((((longitudeDeg + 180) % 360) + 360) % 360) - 180, latitudeDeg,
      );
      const { x, y } = tilingScheme.positionToTileXY(carto, groundMaxLevel);
      frontier.set(`${x}/${y}`, { x, y });
    }
  }
  const rows = [];
  for (let level = groundMaxLevel + 1; level <= availability.featureMaxLevel; level += 1) {
    const next = new Map();
    for (const parent of frontier.values()) {
      for (const dx of [0, 1]) {
        for (const dy of [0, 1]) {
          const x = parent.x * 2 + dx;
          const y = parent.y * 2 + dy;
          if (availability(x, y, level)) next.set(`${x}/${y}`, { x, y });
        }
      }
    }
    rows.push({ level, tiles: next.size, postSpacingM: null });
    frontier = next;
    if (next.size === 0) break;
  }
  return rows;
}
