// Turn a drawn route into a navigable channel the engine actually carves.
//
// **This is the authored-world-change path, and it is not an exception to point-evaluability.**
// A feature is a shape stated at a place; asking whether a point lies inside it is a pure
// function of that point. Flood-filling a channel across a heightfield would have needed a
// raster and a traversal order, and would have cost the property the whole engine is built on.
// A chain of `carve` segments costs nothing of the kind.
//
// **Measured before it was written**, on the owner's own world with the Python oracle:
//
//     one segment, ground 33.70 m, target -8 m, 300 x 90 m   ->  -8.00 m, weight 1.000
//     a straight chain, sampled on the centreline               ->  -8.00 m at every overlap
//
//     features    0      5     22     60    150
//     us/sample 184.7  199.1  172.1  175.5  169.0
//
// No trend in the cost. The feature scan does not rise above the terrain evaluation at a
// hundred and fifty segments, so a river with a segment every hundred metres can run twenty
// kilometres before anybody needs to think about it again.
//
// **Overlap is not optional on a bend, and that was learned the hard way.** A first test cut
// its segments to 62% of their span along a curving path and reported the channel 29 m
// shallower than asked. Nothing was wrong with the engine: the sample sat off every
// segment's centreline, in the gap between them. `OVERLAP` below is why that cannot happen
// here, and it is a number rather than a comment for the same reason.

/// How far each segment reaches along its own bearing, as a multiple of half the leg it
/// covers.
///
/// **4.5, and it was measured rather than reasoned.** A feature's weight is one at its own
/// middle and falls to nothing at its stated reach, so a chain of segments carves DEEPEST at
/// the midpoints and SHALLOWEST at the nodes between them. A river built at 1.35 looked
/// carved - it had banks and a bed and a proper cross-section - and was not navigable
/// anywhere, because the check walked it and found the bottom rising into daylight at every
/// node.
///
/// Sounded along a 3.51 km channel of 25 segments, draught 2.5 m, 90 soundings:
///
///     overlap   min depth   shoaling soundings
///       1.35     -21.30 m        65
///       2.0       -9.34 m        19
///       3.0       -0.53 m         1
///       4.5       +5.99 m         0   <- navigable
///       6.0       +8.26 m         0
///
/// Below 4.5 the channel has gaps a hull would find. Above it, nothing improves that a
/// deeper target would not do better, and every extra metre of reach is a wider disturbance
/// either side of the river.
const OVERLAP = 4.5;

/// A channel that shoals toward its head, because a river does.
///
/// Both ends are stated rather than one: a game saying "ships sail up to this town" is
/// asserting a depth at the town, and asserting it at the river mouth instead would be a
/// different and easier promise.
export const DEFAULT_RIVER = {
  mouthDepthM: -9.0,
  headDepthM: -3.0,
  mouthWidthM: 110.0,
  headWidthM: 45.0,
};

function bearingBetween(a, b) {
  const toRad = (d) => (d * Math.PI) / 180;
  const [la1, lo1, la2, lo2] = [a[0], a[1], b[0], b[1]].map(toRad);
  const y = Math.sin(lo2 - lo1) * Math.cos(la2);
  const x = Math.cos(la1) * Math.sin(la2)
    - Math.sin(la1) * Math.cos(la2) * Math.cos(lo2 - lo1);
  return ((Math.atan2(y, x) * 180) / Math.PI + 360) % 360;
}

function metresBetween(a, b, radiusM) {
  const toRad = (d) => (d * Math.PI) / 180;
  const dLat = toRad(b[0] - a[0]);
  const dLon = toRad(b[1] - a[1]);
  const h = Math.sin(dLat / 2) ** 2
    + Math.cos(toRad(a[0])) * Math.cos(toRad(b[0])) * Math.sin(dLon / 2) ** 2;
  return 2 * Math.asin(Math.sqrt(h)) * radiusM;
}

/// Build the engine's feature records for one river.
///
/// Args:
///   points: `[[lat, lon], ...]` in order, MOUTH FIRST. Order decides which end is deep.
///   radiusM: the planet's radius.
///   shape: overrides for `DEFAULT_RIVER`.
///
/// Returns an array of `{ latitudeDeg, longitudeDeg, targetM, lengthM, widthM, bearingDeg,
/// compose, substrate }` - exactly what `Engine.newWorld` takes.
export function riverFeatures(points, radiusM, shape = {}) {
  const s = { ...DEFAULT_RIVER, ...shape };
  const out = [];
  for (let i = 0; i < points.length - 1; i += 1) {
    const a = points[i];
    const b = points[i + 1];
    const leg = metresBetween(a, b, radiusM);
    if (leg <= 0) continue;
    // How far up the river this segment sits, so depth and width can taper.
    const t = points.length > 2 ? i / (points.length - 2) : 0;
    out.push({
      latitudeDeg: (a[0] + b[0]) / 2,
      longitudeDeg: (a[1] + b[1]) / 2,
      targetM: s.mouthDepthM + (s.headDepthM - s.mouthDepthM) * t,
      // Half-length, times the overlap. Segments must reach past their own midpoints or a
      // bend leaves the channel with a gap in it.
      lengthM: (leg / 2) * OVERLAP,
      widthM: s.mouthWidthM + (s.headWidthM - s.mouthWidthM) * t,
      bearingDeg: bearingBetween(a, b),
      compose: "carve",
      substrate: "derive",
    });
  }
  return out;
}

/// Read a saved route and turn it into river features.
export async function riverFromRoute(name, radiusM, shape = {}) {
  const file = `${String(name).replace(/[^A-Za-z0-9_-]+/g, "-")}.json`;
  const response = await fetch(`/routes/${file}`);
  if (!response.ok) throw new Error(`no route called ${file}`);
  const document_ = await response.json();
  const points = (document_.nodes || [])
    .slice()
    .sort((p, q) => (p.order ?? 0) - (q.order ?? 0))
    .map((node) => [node.latitude_deg, node.longitude_deg]);
  if (points.length < 2) throw new Error("a river needs at least two nodes");
  return { features: riverFeatures(points, radiusM, shape), points, name: document_.name };
}

/// Walk a finished channel and confirm it holds its depth.
///
/// **An unchecked river is a river that is nine metres deep except in the one place a hull
/// would find.** A game asserting that ships reach a town is asserting a depth along a path,
/// and that assertion is testable exactly the way the port mapping tests a harbour.
///
/// `elevationAt(lat, lon)` comes from the built world, so this measures what was actually
/// carved rather than what was requested.
export function soundChannel(points, elevationAt, radiusM, draughtM = 2.5, stepM = 40) {
  const readings = [];
  for (let i = 0; i < points.length - 1; i += 1) {
    const a = points[i];
    const b = points[i + 1];
    const leg = metresBetween(a, b, radiusM);
    const steps = Math.max(1, Math.round(leg / stepM));
    for (let s = 0; s < steps; s += 1) {
      const f = s / steps;
      const lat = a[0] + (b[0] - a[0]) * f;
      const lon = a[1] + (b[1] - a[1]) * f;
      readings.push({ lat, lon, depthM: -elevationAt(lat, lon) });
    }
  }
  const shoal = readings.filter((r) => r.depthM < draughtM);
  const depths = readings.map((r) => r.depthM);
  return {
    soundings: readings.length,
    minDepthM: Math.min(...depths),
    maxDepthM: Math.max(...depths),
    draughtM,
    shoalings: shoal.length,
    navigable: shoal.length === 0,
    // Where it fails, not just that it does. A shoal at one bend is a fixable thing.
    worst: shoal.sort((p, q) => p.depthM - q.depthM).slice(0, 5)
      .map((r) => ({ lat: +r.lat.toFixed(5), lon: +r.lon.toFixed(5),
                     depthM: +r.depthM.toFixed(2) })),
  };
}
