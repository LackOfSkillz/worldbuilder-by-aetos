//! Brushes that paint landforms, by writing the same feature records the engine already
//! composites.
//
// **A stroke is not pixels, it is a landform.** Every brush here emits exactly the record
// type the generator has always used - `RAISE` for ground that must stand proud, `CARVE`
// for ground that must be cut - so a painted mountain is a real rise the site scorer can
// measure prominence on, and a painted channel is water a ferry can actually sail. Nothing
// is baked: the seed still makes the base, and strokes composite over it in order, which is
// why the layers panel can reorder them and mean it.
//
// **Five brushes, deliberately.** The value is a small set of tools over a real physical
// model, not a large set over a bitmap. Each one is a couple of numbers over the same
// record, and the differences between them are honest differences in what the ground does.

/// The brushes. `compose` and the sign of `target` are the whole difference between them.
export const BRUSHES = {
  mountain: {
    label: "Mountain", compose: "raise", target: 1800, size: 40000, shape: "ridge",
    hint: "a range, not a wall: peaks, saddles and spurs off a drawn crest line.",
  },
  island: {
    label: "Island", compose: "raise", target: 60, size: 6000, shape: "lobes",
    hint: "raises seabed above datum. Headlands and coves, not a disc; drag for a coast.",
  },
  hill: {
    label: "Hills", compose: "raise", target: 260, size: 18000, shape: "ridge",
    hint: "gentler broken ground. The band the site scorer likes for settlements.",
  },
  lake: {
    label: "Lake", compose: "carve", target: -12, size: 4000, shape: "lobes",
    hint: "cuts a basin with a ragged shore. Below the ground it sits in, so an upland lake is not at sea level.",
  },
  channel: {
    label: "Channel", compose: "carve", target: -14, size: 2000, shape: "meander",
    hint: "cuts navigable water that wanders, pools and throws off backwaters. Stays as deep as asked.",
  },
};

/// How much a chained stroke's segments overlap so the chain does not shoal at its nodes.
///
/// **Measured, not chosen.** The first river carved deepest at the midpoints and shallowest
/// at the nodes, shoaling at 34 of 90 soundings, because a chain of bumps meets at its
/// edges. 4.5 is the factor that makes the joins as deep as the middles, and it is the same
/// constant `river.js` uses - imported in spirit rather than restated, and if it ever moves
/// this must move with it.
export const OVERLAP = 4.5;

/// How each brush breaks a drawn gesture into ground.
///
/// **Nothing this file emits is a circle or a ruled line, because nothing in a landscape
/// is.** A single feature per gesture is the cheapest thing to write and the one thing a
/// reader always spots: a perfectly round island, a river that runs like a pen stroke, a
/// ridge with a flat top. So every brush declares a shape, and every shape is built from
/// the same three moves - break the gesture into pieces, give each piece its own size and
/// its own step off the line, and hang smaller things off it.
///
/// **What differs between them is what the ground is allowed to do, not how rough it is.**
/// Ground may be as broken as it likes. Navigable water may not: see `MEANDER`.
export const SHAPES = {
  /// A crest with peaks, saddles and spurs. Raise only.
  ridge: {
    pieces: 3,
    height: [0.52, 1.0],
    width: [0.62, 1.18],
    wander: 0.42,
    spurChance: 0.34,
    spurHeight: [0.40, 0.72],
    spurLength: [0.45, 0.85],
  },
  /// A coastline made of overlapping lobes rather than one disc.
  ///
  /// **A union of circles is not a circle.** Where two lobes overlap the outline runs
  /// straight out to a headland; where three meet at a gap it closes into a cove. So an
  /// irregular shore comes out of stacking round features off-centre, without needing a
  /// single non-round primitive - and because `RAISE` only ever lifts and `CARVE` only ever
  /// cuts, a lobe can never undo its neighbour. Bays are the ground the lobes did not
  /// reach.
  lobes: {
    //: Lobes around the main one, and how far out and how big they sit.
    count: [3, 6],
    reach: [0.30, 0.78],
    size: [0.34, 0.82],
    height: [0.45, 1.0],
    //: Outlying rocks and sandbars, well off the main body.
    skerries: [0, 2],
    skerryReach: [0.95, 1.65],
    skerrySize: [0.14, 0.30],
    skerryHeight: [0.14, 0.46],
    //: How far along a dragged path a new cluster is dropped, as a fraction of the size.
    step: 0.62,
  },
  /// A watercourse that wanders, pools, narrows and throws off backwaters.
  ///
  /// **The one rule that is not aesthetic.** A drawn channel must stay navigable end to
  /// end, and the whole reason `OVERLAP` exists is that a chain of carves shoals where its
  /// links meet. So the fairway is never allowed to become shallower than the depth that
  /// was asked for: `pool` only ever cuts DEEPER, and `width` has a floor. Backwaters may
  /// be as shallow as they like, and cannot shoal anything, because `Features::apply` skips
  /// a carve whose target is above the ground already there - a four-metre creek crossing a
  /// fourteen-metre fairway contributes exactly nothing to the fairway.
  meander: {
    //: Levels of midpoint displacement, and how far a midpoint may step off the chord.
    levels: 2,
    swing: 0.32,
    //: Depth as a multiple of the asked-for depth. Never below 1: never shallower.
    pool: [1.0, 1.34],
    //: Width as a fraction of the asked-for width, floored so the fairway cannot pinch.
    width: [0.78, 1.30],
    //: Backwaters: short, shallow, narrow, thrown off at roughly a right angle.
    creekChance: 0.30,
    creekDepth: [0.32, 0.68],
    creekWidth: [0.30, 0.60],
    creekLength: [0.55, 1.20],
  },
};

/// A small deterministic generator, seeded from a place on the globe.
///
/// **Seeded from the coordinates, not from `Math.random`.** The same stroke drawn on the
/// same spot must produce the same mountain: a world is a seed plus a list of features, and
/// a range that reshuffled itself on every reload would break that promise at the first
/// save-and-open.
export function noiseAt(latDeg, lonDeg, salt) {
  let h = 2166136261 ^ salt;
  for (const v of [latDeg * 1e4, lonDeg * 1e4]) {
    h = Math.imul(h ^ (v | 0), 16777619);
  }
  return () => {
    h += 0x6d2b79f5;
    let t = h;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const between = (rand, range) => range[0] + rand() * (range[1] - range[0]);

/// Walk `distanceM` from a point along a bearing, on a sphere of `radiusM`.
function along(latDeg, lonDeg, bearingDeg, distanceM, radiusM) {
  const rad = Math.PI / 180;
  const lat = latDeg * rad, lon = lonDeg * rad, brg = bearingDeg * rad;
  const d = distanceM / radiusM;
  const lat2 = Math.asin(Math.sin(lat) * Math.cos(d)
    + Math.cos(lat) * Math.sin(d) * Math.cos(brg));
  const lon2 = lon + Math.atan2(Math.sin(brg) * Math.sin(d) * Math.cos(lat),
                                Math.cos(d) - Math.sin(lat) * Math.sin(lat2));
  return [lat2 / rad, ((lon2 / rad + 540) % 360) - 180];
}

/// Break one straight leg into the pieces of a range.
///
/// Args:
///   a, c: the leg's ends, `[lat, lon]`.
///   leg: its length in metres.
///   brg: its bearing.
///   base: `{ target, width, kind, compose }` from the brush and its sliders.
///   radiusM: the planet's radius.
///
/// Returns the feature records for that leg.
function ridgePieces(a, c, leg, brg, base, radiusM) {
  const s = SHAPES.ridge;
  const rand = noiseAt(a[0], a[1], Math.round(brg));
  const out = [];
  const n = s.pieces;
  const step = leg / n;
  for (let i = 0; i < n; i += 1) {
    // The piece's own centre along the leg, then stepped off it. Wandering is what stops
    // the crest reading as a ruled line without moving it far enough to leave the stroke.
    const width = base.width * between(rand, s.width);
    const alongM = step * (i + 0.5);
    const centre = along(a[0], a[1], brg, alongM, radiusM);
    const off = (rand() - 0.5) * 2 * s.wander * width;
    const at = along(centre[0], centre[1], brg + 90, off, radiusM);
    out.push({
      ...base.record,
      latitude_deg: Number(at[0].toFixed(6)),
      longitude_deg: Number(at[1].toFixed(6)),
      target_m: Math.round(base.target * between(rand, s.height)),
      length_m: (step / 2) * OVERLAP,
      width_m: Math.round(width),
      bearing_deg: Number((brg + (rand() - 0.5) * 14).toFixed(3)),
    });
    // A spur: shorter, lower, thrown off at roughly a right angle. Spurs are most of what
    // makes a range look like erosion rather than extrusion, and they are what a valley
    // between two of them is made of.
    if (rand() < s.spurChance) {
      const side = rand() < 0.5 ? 90 : -90;
      const spurLength = step * between(rand, s.spurLength);
      const foot = along(at[0], at[1], brg + side, spurLength / 2, radiusM);
      out.push({
        ...base.record,
        latitude_deg: Number(foot[0].toFixed(6)),
        longitude_deg: Number(foot[1].toFixed(6)),
        target_m: Math.round(base.target * between(rand, s.spurHeight)),
        length_m: (spurLength / 2) * OVERLAP,
        width_m: Math.round(width * 0.7),
        bearing_deg: Number(((brg + side + 360) % 360).toFixed(3)),
      });
    }
  }
  return out;
}

function bearing(a, b) {
  const y = Math.sin((b[1] - a[1]) * Math.PI / 180) * Math.cos(b[0] * Math.PI / 180);
  const x = Math.cos(a[0] * Math.PI / 180) * Math.sin(b[0] * Math.PI / 180)
          - Math.sin(a[0] * Math.PI / 180) * Math.cos(b[0] * Math.PI / 180)
            * Math.cos((b[1] - a[1]) * Math.PI / 180);
  return (Math.atan2(y, x) * 180 / Math.PI + 360) % 360;
}

/// A cluster of lobes standing for one round thing: an island, a lake, a lone hill.
///
/// The main lobe carries the asked-for size and target so the thing is the size it was
/// drawn; everything else is smaller, lower and off-centre, which is what turns a disc into
/// a coastline. See `SHAPES.lobes`.
function lobesAt(latDeg, lonDeg, base, radiusM, rand) {
  const s = SHAPES.lobes;
  const out = [{
    ...base.record,
    latitude_deg: Number(latDeg.toFixed(6)),
    longitude_deg: Number(lonDeg.toFixed(6)),
    target_m: Math.round(base.target),
    length_m: Math.round(base.width),
    width_m: Math.round(base.width),
    bearing_deg: 0,
  }];
  const lobes = Math.round(between(rand, s.count));
  for (let i = 0; i < lobes; i += 1) {
    // Bearings are spread round the compass with a jitter rather than drawn uniformly:
    // uniform bearings clump, and a clump of lobes is a bulge on one side rather than an
    // outline.
    const brg = (360 * i) / lobes + (rand() - 0.5) * (360 / lobes);
    const reach = base.width * between(rand, s.reach);
    const at = along(latDeg, lonDeg, brg, reach, radiusM);
    const size = Math.round(base.width * between(rand, s.size));
    out.push({
      ...base.record,
      latitude_deg: Number(at[0].toFixed(6)),
      longitude_deg: Number(at[1].toFixed(6)),
      target_m: Math.round(base.target * between(rand, s.height)),
      length_m: size,
      width_m: size,
      bearing_deg: 0,
    });
  }
  const skerries = Math.round(between(rand, s.skerries));
  for (let i = 0; i < skerries; i += 1) {
    const brg = rand() * 360;
    const at = along(latDeg, lonDeg, brg, base.width * between(rand, s.skerryReach), radiusM);
    const size = Math.round(base.width * between(rand, s.skerrySize));
    out.push({
      ...base.record,
      latitude_deg: Number(at[0].toFixed(6)),
      longitude_deg: Number(at[1].toFixed(6)),
      target_m: Math.round(base.target * between(rand, s.skerryHeight)),
      length_m: size,
      width_m: size,
      bearing_deg: 0,
    });
  }
  return out;
}

/// Bend a drawn line into one that wanders.
///
/// **Midpoint displacement, twice.** A hand draws a river as three or four long strokes; a
/// river does not run in long strokes. Each segment's midpoint is stepped off the chord by
/// a fraction of the segment's own length, and then the shorter segments that result are
/// stepped again by proportionately less. What comes out has bends at every scale, which is
/// the property a hand-drawn line lacks and the reason a drawn river reads as drawn.
///
/// The ends are never moved: the line still starts and finishes where it was put.
export function meanderPath(points, radiusM, rand, swing, levels) {
  let path = points.slice();
  for (let level = 0; level < levels; level += 1) {
    const next = [path[0]];
    for (let i = 0; i < path.length - 1; i += 1) {
      const a = path[i], c = path[i + 1];
      const leg = metres(a, c, radiusM);
      if (leg > 0) {
        const brg = bearing(a, c);
        const mid = along(a[0], a[1], brg, leg / 2, radiusM);
        // Each level swings less than the last, in proportion to the shorter legs it is
        // working on, so the bends nest instead of fighting.
        const off = (rand() - 0.5) * 2 * swing * leg;
        const stepped = along(mid[0], mid[1], brg + 90, off, radiusM);
        next.push(stepped);
      }
      next.push(c);
    }
    path = next;
    swing *= 0.55;
  }
  return path;
}

/// Cut a whole watercourse: a piece per leg, a piece over every bend, and backwaters.
///
/// **The bends are where a chain of carves shoals, and a keel piece is what closes them.**
/// `length_m` is a HALF-length to the engine, so a leg's piece reaches `2.25` legs either
/// way and the join between two of them sits at `0.222` of that - a weight of `0.874` each.
/// Two of those leave `(1 - 0.874)^2`, about 1.6%, of the distance between the ground and
/// the depth asked for. That is nothing when a bayou is being cut out of ground already
/// near sea level, and it is fourteen metres when the same brush is dragged across
/// nine-hundred-metre upland - which is exactly where the measured groundings were.
///
/// So every interior bend gets its own piece centred ON the node, where its weight is one
/// and the cut is therefore exact. It costs one feature per bend and it is the difference
/// between a river that holds its depth over any ground and one that holds it over gentle
/// ground only.
function channelPath(line, base, radiusM, rand) {
  const out = [];
  const legs = [];
  for (let i = 0; i < line.length - 1; i += 1) {
    const a = line[i], c = line[i + 1];
    const leg = metres(a, c, radiusM);
    if (leg <= 0) continue;
    const brg = bearing(a, c);
    legs.push({ a, c, leg, brg });
    out.push(...channelPieces(a, c, leg, brg, base, radiusM, rand));
  }
  // **The two ends shoal for the same reason a bend does, and nothing is beyond them to
  // help.** A line's last point sits at `0.222` of its piece's half-length with only ONE
  // piece reaching it, so 12.6% of the cut is left undone there - thirty metres of it when
  // the brush is crossing upland, which is a bar across the mouth of every river drawn.
  // A piece centred on each end closes them the same way the bends are closed.
  for (const end of legs.length ? [
    { at: legs[0].a, brg: legs[0].brg, span: legs[0].leg },
    { at: legs[legs.length - 1].c, brg: legs[legs.length - 1].brg,
      span: legs[legs.length - 1].leg },
  ] : []) {
    out.push({
      ...base.record,
      latitude_deg: Number(end.at[0].toFixed(6)),
      longitude_deg: Number(end.at[1].toFixed(6)),
      target_m: Math.round(base.target),
      length_m: (end.span / 2) * OVERLAP,
      width_m: Math.round(base.width),
      bearing_deg: Number(end.brg.toFixed(3)),
    });
  }
  for (let i = 0; i < legs.length - 1; i += 1) {
    const before = legs[i], after = legs[i + 1];
    const span = Math.min(before.leg, after.leg);
    // **Two pieces at the bend, one per leg, and not one on the average bearing.** The
    // averaged version was measured and it does not work: a piece bisecting a bend is
    // off-axis from both legs, so seven kilometres back along the approach the line has
    // already left its half-width and the piece contributes nothing there. That is where
    // every remaining shoal was - at 0.95 of one leg and 0.04 of the next, on either side
    // of a node that was supposed to be covered. A piece aligned with each leg sits ON the
    // line for its whole approach, which is the thing that had to be true.
    for (const brg of [before.brg, after.brg]) {
      out.push({
        ...base.record,
        latitude_deg: Number(before.c[0].toFixed(6)),
        longitude_deg: Number(before.c[1].toFixed(6)),
        target_m: Math.round(base.target),
        length_m: (span / 2) * OVERLAP,
        width_m: Math.round(base.width),
        bearing_deg: Number(brg.toFixed(3)),
      });
    }
  }
  return out;
}

/// One leg of a watercourse: the fairway piece, and sometimes a backwater off it.
function channelPieces(a, c, leg, brg, base, radiusM, rand) {
  const s = SHAPES.meander;
  const width = Math.round(base.width * between(rand, s.width));
  const mid = along(a[0], a[1], brg, leg / 2, radiusM);
  const out = [{
    ...base.record,
    latitude_deg: Number(mid[0].toFixed(6)),
    longitude_deg: Number(mid[1].toFixed(6)),
    // Deeper only. A pool is a hole in the bed, and a bed that got shallower here would be
    // the shoal `OVERLAP` exists to prevent.
    target_m: Math.round(base.target * between(rand, s.pool)),
    length_m: (leg / 2) * OVERLAP,
    width_m: width,
    bearing_deg: Number(brg.toFixed(3)),
  }];
  if (rand() < s.creekChance) {
    const side = rand() < 0.5 ? 90 : -90;
    const creek = leg * between(rand, s.creekLength);
    const foot = along(mid[0], mid[1], brg + side, creek / 2, radiusM);
    out.push({
      ...base.record,
      latitude_deg: Number(foot[0].toFixed(6)),
      longitude_deg: Number(foot[1].toFixed(6)),
      // Shallower, and provably harmless: a carve whose target sits above the ground
      // already there is skipped outright, so a creek crossing the fairway does nothing
      // to it.
      target_m: Math.round(base.target * between(rand, s.creekDepth)),
      length_m: (creek / 2) * OVERLAP,
      width_m: Math.round(width * between(rand, s.creekWidth)),
      bearing_deg: Number(((brg + side + 360) % 360).toFixed(3)),
    });
  }
  return out;
}

//: The most lobe clusters one drawn line may spend.
//:
//: **A brush is small and a drag at planet zoom is not.** Stepping a six-kilometre island
//: brush along a four-hundred-kilometre drag at the spacing that makes a continuous coast
//: asks for a hundred clusters - seven hundred features for one gesture, which is slow to
//: composite and was never what the hand meant. Past this count the step widens instead, so
//: a short drag draws one coastline and a long one draws a chain of islands along the same
//: line. Both are honest readings of the gesture; neither costs more than this.
const MAX_CLUSTERS = 32;

/// Drop lobe clusters along a whole drawn line, so a dragged island brush makes a coast at
/// close range and an archipelago at long range.
function lobesAlongPath(line, base, radiusM, rand) {
  let total = 0;
  for (let i = 0; i < line.length - 1; i += 1) {
    total += metres(line[i], line[i + 1], radiusM);
  }
  const wanted = Math.max(base.width * SHAPES.lobes.step, 1);
  const step = Math.max(wanted, total / MAX_CLUSTERS);
  const out = [];
  let walked = 0;
  let next = step / 2;
  for (let i = 0; i < line.length - 1; i += 1) {
    const a = line[i], c = line[i + 1];
    const leg = metres(a, c, radiusM);
    if (leg <= 0) continue;
    const brg = bearing(a, c);
    while (next <= walked + leg) {
      const at = along(a[0], a[1], brg, next - walked, radiusM);
      out.push(...lobesAt(at[0], at[1], base, radiusM, rand));
      next += step;
    }
    walked += leg;
  }
  if (!out.length) out.push(...lobesAt(line[0][0], line[0][1], base, radiusM, rand));
  return out;
}

function metres(a, b, radiusM) {
  const p = Math.PI / 180;
  const h = Math.sin((b[0] - a[0]) * p / 2) ** 2
    + Math.cos(a[0] * p) * Math.cos(b[0] * p) * Math.sin((b[1] - a[1]) * p / 2) ** 2;
  return 2 * Math.asin(Math.sqrt(h)) * radiusM;
}

/// One dab: the records for a single thing placed at a point.
///
/// Returns an ARRAY, because nothing a brush places is one feature any more. A lone hill is
/// a cluster of lobes for the same reason an island is: a disc reads as a drawing.
export function dab(brush, latitudeDeg, longitudeDeg, { size, target, layer } = {}) {
  const b = BRUSHES[brush];
  const width = size || b.size;
  const base = {
    target: target === undefined ? b.target : target,
    width,
    record: {
      kind: layer || `painted ${brush}`,
      compose: b.compose,
      substrate: "derive",
      marked: false,
    },
  };
  const rand = noiseAt(latitudeDeg, longitudeDeg, 17);
  return lobesAt(latitudeDeg, longitudeDeg, base, EARTHISH_M, rand);
}

//: The radius a lone dab lays its lobes out on when no world has said otherwise.
//:
//: A dab's offsets are metres along the ground and the sphere they are walked on barely
//: changes them at these distances - a lobe half a brush-width out lands within a metre of
//: the same place on any planet a person would build. The strokes get the real radius,
//: which is where it does matter.
const EARTHISH_M = 6371000;

/// A stroke along a path, broken up the way its brush's shape says.
///
/// Every brush routes through here, and the shape decides what a segment becomes: a piece
/// of crest with spurs, a run of coastline, or a length of watercourse with its pools and
/// backwaters. The one thing common to all of them is `OVERLAP` - the joins between pieces
/// must reach past each other, or the chain shoals where its links meet.
export function stroke(brush, points, radiusM, { size, target, layer } = {}) {
  const b = BRUSHES[brush];
  const width = size || b.size;
  const peak = target === undefined ? b.target : target;
  const record = {
    kind: layer || `painted ${brush}`,
    compose: b.compose,
    substrate: "derive",
    marked: false,
  };
  const base = { target: peak, width, record };
  // Seeded from where the stroke starts, so redrawing the same line gives the same river.
  const rand = noiseAt(points[0][0], points[0][1], points.length);
  // A watercourse is bent BEFORE it is cut into pieces: the wander belongs to the line, and
  // bending each piece separately would give a row of kinks rather than a meander.
  const line = b.shape === "meander"
    ? meanderPath(points, radiusM, rand, SHAPES.meander.swing, SHAPES.meander.levels)
    : points;

  // Lobes are laid out against the whole line rather than leg by leg, because their
  // spacing has a budget and a budget cannot be spent one leg at a time.
  if (b.shape === "lobes") return lobesAlongPath(line, base, radiusM, rand);
  // A watercourse is cut against the whole line as well: its bends are joins between legs,
  // and a join cannot be closed from inside one of the legs that makes it.
  if (b.shape === "meander") return channelPath(line, base, radiusM, rand);

  const out = [];
  for (let i = 0; i < line.length - 1; i += 1) {
    const a = line[i], c = line[i + 1];
    const leg = metres(a, c, radiusM);
    if (leg <= 0) continue;
    const brg = bearing(a, c);
    if (b.shape === "ridge") {
      out.push(...ridgePieces(a, c, leg, brg, base, radiusM));
    } else {
      out.push({
        ...record,
        latitude_deg: Number(((a[0] + c[0]) / 2).toFixed(6)),
        longitude_deg: Number(((a[1] + c[1]) / 2).toFixed(6)),
        target_m: peak,
        length_m: (leg / 2) * OVERLAP,
        width_m: width,
        bearing_deg: Number(brg.toFixed(3)),
      });
    }
  }
  return out;
}

//: What a held stroke looks like before it is real ground.
//:
//: **A ghost is not a preview of the terrain, it is a preview of the intent.** Rebuilding
//: the globe to show one dab costs seconds, so the honest cheap thing to draw is the
//: feature's own footprint - where it is, how big it is, which way it lies and whether it
//: raises or carves. That is exactly the record the engine will composite, drawn flat.
const GHOST = {
  raise: { fill: "rgba(214,166,96,0.34)", edge: "rgba(214,166,96,0.85)" },
  carve: { fill: "rgba(86,150,214,0.34)", edge: "rgba(86,150,214,0.85)" },
};

/// Draw held features as translucent footprints, and take them away again.
///
/// Returns `{ show, clear, count }`. Entities are clamped to the ground so a ghost sits on
/// the terrain that is there now, which is the terrain the stroke is about to change.
///
/// **A footprint alone is invisible at the zoom people paint from.** A forty-kilometre
/// brush is under a pixel wide from twenty thousand kilometres up, so the first ghosts
/// were drawn correctly and could not be seen - which is indistinguishable from a brush
/// that does nothing. So every feature also gets a screen-space mark: a line along its own
/// axis if it is a stroke, a dot if it is a dab. Those keep their width in pixels, so a
/// ghost is legible from orbit and the true footprint appears underneath it on approach.
function ghostLayer(viewer, Cesium) {
  const entities = [];
  let live = null;
  const radiusM = viewer.scene.globe.ellipsoid.maximumRadius;
  return {
    /// Ghost a finished stroke.
    ///
    /// Args:
    ///   features: the records that were held.
    ///   path: the points the hand actually drew, when there were any.
    ///
    /// **The line drawn is the line the hand drew.** The first version derived it from each
    /// feature's `length_m`, which is the half-leg times `OVERLAP` - two and a quarter times
    /// the segment it stands for. Every ghost segment overshot both its ends, so a smooth
    /// drag came back as a pile of crossing diagonals and looked like a bug in the brush
    /// rather than a bug in the drawing of it. The compositing length is for the engine; the
    /// path is for the eye.
    show(features, path) {
      if (path && path.length > 1) {
        const paint = GHOST[(features[0] || {}).compose] || GHOST.raise;
        entities.push(viewer.entities.add({
          polyline: {
            positions: Cesium.Cartesian3.fromDegreesArray(
              path.flatMap((p) => [p[1], p[0]])),
            width: 3,
            material: Cesium.Color.fromCssColorString(paint.edge),
            clampToGround: true,
          },
        }));
      }
      for (const f of features) {
        const paint = GHOST[f.compose] || GHOST.raise;
        const edge = Cesium.Color.fromCssColorString(paint.edge);
        if (!path || path.length < 2) {
          entities.push(viewer.entities.add({
            position: Cesium.Cartesian3.fromDegrees(f.longitude_deg, f.latitude_deg),
            point: {
              pixelSize: 8,
              color: Cesium.Color.fromCssColorString(paint.fill),
              outlineColor: edge,
              outlineWidth: 1.5,
              heightReference: Cesium.HeightReference.CLAMP_TO_GROUND,
            },
          }));
        }
        entities.push(viewer.entities.add({
          position: Cesium.Cartesian3.fromDegrees(f.longitude_deg, f.latitude_deg),
          ellipse: {
            // The engine reads `length_m` and `width_m` as full extents; Cesium wants
            // semi-axes, so both are halved here rather than in the record.
            semiMajorAxis: Math.max(f.length_m, f.width_m) / 2,
            semiMinorAxis: Math.min(f.length_m, f.width_m) / 2,
            rotation: Cesium.Math.toRadians(90 - (f.bearing_deg || 0)),
            material: Cesium.Color.fromCssColorString(paint.fill),
            // **Flat at datum, not clamped.** Cesium refuses an outline on ground-clamped
            // geometry and ignores a `heightReference` with no height, so asking for both
            // bought two warnings and neither effect. A footprint is a plan view of where
            // the feature will be; sea level is the honest place to draw it, and the
            // screen-space mark above carries legibility at any zoom.
            height: 0,
            outline: true,
            outlineColor: edge,
          },
        }));
      }
    },
    /// A line that follows the cursor while the hand is still moving.
    ///
    /// **Without this a long drag showed nothing until it was released**, which reads as a
    /// brush that is not working - so the hand lets go and tries again, and a range that
    /// should have been one stroke arrives as ten short ones. That is exactly what happened.
    /// The trail is a single entity reading the live path, so it costs one polyline no
    /// matter how many nodes the drag lays down.
    trail(points) {
      if (live) viewer.entities.remove(live);
      live = viewer.entities.add({
        polyline: {
          positions: new Cesium.CallbackProperty(() => (points.length > 1
            ? Cesium.Cartesian3.fromDegreesArray(points.flatMap((p) => [p[1], p[0]]))
            : []), false),
          width: 2,
          material: new Cesium.PolylineDashMaterialProperty({
            color: Cesium.Color.WHITE.withAlpha(0.75),
          }),
          clampToGround: true,
        },
      });
    },
    endTrail() {
      if (live) viewer.entities.remove(live);
      live = null;
    },
    clear() {
      for (const entity of entities) viewer.entities.remove(entity);
      entities.length = 0;
      if (live) viewer.entities.remove(live);
      live = null;
    },
    count: () => entities.length,
  };
}

function el(tag, cls, text) {
  const n = document.createElement(tag);
  if (cls) n.className = cls;
  if (text !== undefined) n.textContent = text;
  return n;
}

/// Build the paint tools into a parent element.
///
/// Args:
///   parent: where to attach.
///   viewer, Cesium: for picking a point on the globe.
///   onPaint: `(features, brushName) => void` when a stroke is laid down. The stroke is
///     HELD, not applied - it is ghosted on the globe and nothing rebuilds.
///   hooks: `{ onApply, onDiscard }`. `onApply` is what actually makes the held strokes
///     ground, and is the only expensive call in this file.
export function buildTools(parent, viewer, Cesium, onPaint, hooks = {}) {
  const wrap = el("div", "wb-section");
  wrap.append(el("div", "wb-section-title", "paint"));

  const grid = el("div", "wb-brushes");
  const sizeRow = el("div", "wb-row");
  const sizeLabel = el("label", "wb-brush-field");
  const size = document.createElement("input");
  size.type = "range"; size.min = "500"; size.max = "120000"; size.step = "500";
  const heightRow = el("div", "wb-row");
  const heightLabel = el("label", "wb-brush-field");
  const height = document.createElement("input");
  height.type = "range"; height.min = "-400"; height.max = "4000"; height.step = "10";
  const hint = el("div", "wb-note-line");
  const arm = el("button", "wb-mini", "click the globe: off");
  arm.type = "button";
  const chain = el("button", "wb-mini", "chain: off");
  chain.type = "button";
  const commit = el("button", "wb-mini", "finish stroke");
  commit.type = "button";
  commit.disabled = true;
  const apply = el("button", "wb-mini wb-mini-go", "apply 0 strokes");
  apply.type = "button";
  apply.disabled = true;
  const discard = el("button", "wb-mini", "discard");
  discard.type = "button";
  discard.disabled = true;
  const ghosts = ghostLayer(viewer, Cesium);
  let heldCount = 0;

  let current = "mountain";
  let armed = false;
  let chaining = false;
  let path = [];

  const paintUI = () => {
    const b = BRUSHES[current];
    for (const node of grid.children) {
      node.classList.toggle("wb-brush-on", node.dataset.brush === current);
    }
    sizeLabel.textContent = `size ${(Number(size.value) / 1000).toFixed(1)} km`;
    heightLabel.textContent = (b.compose === "carve" ? "depth " : "height ")
      + `${Number(height.value)} m`;
    hint.textContent = b.hint;
    arm.textContent = armed ? "click the globe: ON" : "click the globe: off";
    arm.classList.toggle("wb-brush-on", armed);
    chain.textContent = chaining ? `chain: ${path.length} node(s)` : "chain: off";
    chain.classList.toggle("wb-brush-on", chaining);
    commit.disabled = !(chaining && path.length > 1);
    apply.textContent = `apply ${heldCount} stroke${heldCount === 1 ? "" : "s"}`;
    apply.disabled = heldCount === 0;
    discard.disabled = heldCount === 0;
  };

  for (const [name, b] of Object.entries(BRUSHES)) {
    const node = el("button", "wb-brush", b.label);
    node.type = "button";
    node.dataset.brush = name;
    node.addEventListener("click", () => {
      current = name;
      size.value = String(b.size);
      height.value = String(b.target);
      paintUI();
    });
    grid.append(node);
  }
  size.addEventListener("input", paintUI);
  height.addEventListener("input", paintUI);

  arm.addEventListener("click", () => {
    armed = !armed;
    takeTheDrag(armed);
    if (!armed) { dragging = false; path = []; ghosts.endTrail(); }
    paintUI();
  });
  chain.addEventListener("click", () => {
    // Kept as an explicit multi-click mode for placing a long line node by node, which a
    // drag cannot do across a camera move. A drag is the ordinary way; this is the careful
    // one.
    chaining = !chaining;
    path = [];
    ghosts.endTrail();
    paintUI();
  });


  // **One road for every finished stroke.** Chain-commit, drag-release and single dab all
  // end here, so there is exactly one place that decides a stroke is ghosted and held
  // rather than applied - and no way for a gesture to quietly take the expensive path.
  const lay = (features, drawn) => {
    ghosts.endTrail();
    if (!features.length) return;
    ghosts.show(features, drawn);
    heldCount += features.length;
    paintUI();
    if (onPaint) onPaint(features, current);
  };

  apply.addEventListener("click", async () => {
    apply.disabled = true;
    apply.textContent = "applying...";
    try {
      if (hooks.onApply) await hooks.onApply();
      ghosts.clear();
      heldCount = 0;
    } finally {
      paintUI();
    }
  });
  discard.addEventListener("click", () => {
    ghosts.clear();
    heldCount = 0;
    if (hooks.onDiscard) hooks.onDiscard();
    paintUI();
  });

  const radiusM = () => {
    const spec = (window.__wb && window.__wb.spec) || {};
    return Number(spec.radius) || 6371000;
  };

  commit.addEventListener("click", () => {
    // A chain that never got two nodes still has a trail on the globe; ending it here is
    // what stops an abandoned chain leaving a dashed line behind with nothing held.
    if (path.length < 2) { ghosts.endTrail(); return; }
    const features = stroke(current, path, radiusM(),
                            { size: Number(size.value), target: Number(height.value) });
    const drawn = path;
    path = [];
    lay(features, drawn);
  });

  // **A brush must take the drag away from the camera.** Cesium owns click-and-drag for
  // rotating the globe, so an armed brush that only listened for clicks did nothing while
  // the world spun under the cursor - which is exactly what a paint tool must not do. So
  // arming the brush disables camera rotation and disarming gives it back, and the drag
  // becomes a stroke.
  //
  // Down, move, up: press to begin, drag to lay a line, release to commit. A press and
  // release without moving is a single dab, which is the same gesture an image editor
  // gives you and needs no separate mode.
  const controller = viewer.scene.screenSpaceCameraController;
  const cameraDefaults = {
    rotate: controller.enableRotate,
    translate: controller.enableTranslate,
    tilt: controller.enableTilt,
    look: controller.enableLook,
  };
  const takeTheDrag = (mine) => {
    controller.enableRotate = mine ? false : cameraDefaults.rotate;
    controller.enableTranslate = mine ? false : cameraDefaults.translate;
    controller.enableTilt = mine ? false : cameraDefaults.tilt;
    controller.enableLook = mine ? false : cameraDefaults.look;
    viewer.scene.canvas.style.cursor = mine ? "crosshair" : "";
  };

  const groundAt = (windowPosition) => {
    const ray = viewer.camera.getPickRay(windowPosition);
    const hit = ray && viewer.scene.globe.pick(ray, viewer.scene);
    if (!hit) return null;
    const c = Cesium.Cartographic.fromCartesian(hit);
    return [Cesium.Math.toDegrees(c.latitude), Cesium.Math.toDegrees(c.longitude)];
  };

  //: How far the cursor must travel before a drag lays another node, in screen pixels.
  //: Small enough to follow a curve, large enough that a stroke is not a thousand
  //: features - the same decimation the rivers needed, applied at the input end.
  const NODE_EVERY_PX = 26;

  let dragging = false;
  let lastPixel = null;
  let moved = false;

  const handler = new Cesium.ScreenSpaceEventHandler(viewer.scene.canvas);

  handler.setInputAction((event) => {
    if (!armed) return;
    dragging = true;
    moved = false;
    lastPixel = event.position;
    const point = groundAt(event.position);
    if (point) path.push(point);
    ghosts.trail(path);
    paintUI();
  }, Cesium.ScreenSpaceEventType.LEFT_DOWN);

  handler.setInputAction((event) => {
    if (!armed || !dragging) return;
    const px = event.endPosition;
    if (lastPixel && Math.hypot(px.x - lastPixel.x, px.y - lastPixel.y) < NODE_EVERY_PX) {
      return;
    }
    lastPixel = px;
    moved = true;
    const point = groundAt(px);
    if (point) path.push(point);
    paintUI();
  }, Cesium.ScreenSpaceEventType.MOUSE_MOVE);

  handler.setInputAction(() => {
    if (!armed || !dragging) return;
    dragging = false;
    // **Chain mode collects; it does not lay.** A click is a press and a release, so
    // releasing used to finish a stroke of one node - every click became its own dab and
    // the node list never grew past one. Which is exactly what "I could not draw it all as
    // one line" looks like from the other side of the screen. In chain mode the release
    // keeps the node and waits for `finish stroke`; a drag inside chain mode still adds its
    // nodes, so the two ways of laying a long line are the same list.
    if (chaining) {
      paintUI();
      return;
    }
    const features = (moved && path.length > 1)
      ? stroke(current, path, radiusM(),
               { size: Number(size.value), target: Number(height.value) })
      : (path.length
         ? dab(current, path[0][0], path[0][1],
               { size: Number(size.value), target: Number(height.value) })
         : []);
    const drawn = path;
    path = [];
    lay(features, drawn);
  }, Cesium.ScreenSpaceEventType.LEFT_UP);

  sizeRow.append(sizeLabel);
  heightRow.append(heightLabel);
  wrap.append(grid, sizeRow, size, heightRow, height, hint,
              el("div", "wb-row").appendChild(arm).parentNode);
  const row2 = el("div", "wb-row");
  row2.append(chain, commit);
  const row3 = el("div", "wb-row");
  row3.append(apply, discard);
  wrap.append(row2, row3);
  parent.append(wrap);

  size.value = String(BRUSHES[current].size);
  height.value = String(BRUSHES[current].target);
  paintUI();
  return {
    stop: () => { takeTheDrag(false); handler.destroy(); ghosts.clear(); },
    held: () => heldCount,
    brush: () => current,
  };
}
