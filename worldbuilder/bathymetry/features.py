"""
Specific things that must exist, put where somebody wants them.

Everything before this decides what ordinary ground looks like. This is the other channel:
a short list of named features, stamped at chosen places, because a test region needs a
channel *here* and a bar across *that* harbour mouth, and no amount of noise will oblige.

The division only works because M1.6 proved its half of it. Detail makes texture and
demonstrably not landforms - land fraction moves by under two per cent when it is applied
- so a bank on the chart is a bank somebody placed, and finding one means something.

**Composition is explicit.** A feature says what the ground should be and how it should
argue with what is already there. A bank raises the seabed and must not deepen it; a
channel carves and must not fill; a reef stands proud of whatever it sits on. Adding
offsets would have let a bank inside a channel cancel out into ordinary seabed.
"""

import math
from dataclasses import dataclass

from ..geometry.sphere import EARTH_RADIUS_M
from ..geometry.tangent import TangentFrame

#: How a feature argues with the ground it is placed on.
RAISE = "raise"    # only ever shallower - banks, reefs, bars, headlands
CARVE = "carve"    # only ever deeper - channels, dredged approaches, basins
SHAPE = "shape"    # whatever it says, either way

#: How much relief a feature must be asserting before it takes the ground's texture away
#: from it. A bar standing four metres proud of a flat seabed is a stated shape, and
#: thirty-five metres of coastal roughness would erase it; a feature nudging the bed by a
#: few centimetres has no business silencing anything.
#:
#: Three metres, because that is below the relief of the shallowest thing worth placing.
#: At six a drying rock stated at half a metre above datum came out half a metre below
#: it, which is the difference between a mark and a wreck.
SETTLE_M = 3.0


def _smooth(fraction):
    clamped = max(0.0, min(1.0, fraction))
    return clamped * clamped * (3.0 - 2.0 * clamped)


def _bump(distance_m, half_m):
    """One at the middle, nothing at the edge, flat at both ends."""
    if half_m <= 0.0:
        return 0.0
    return _smooth(1.0 - min(1.0, abs(distance_m) / half_m))


@dataclass(frozen=True)
class Feature:
    """
    One placed thing.

    Attributes:
        kind (str): What it is called, for diagnostics and for chart symbols.
        at (SpherePoint): Its middle.
        target_m (float): The elevation it wants the ground to be at its middle.
        length_m (float): How far it reaches along its bearing. Equal to the width for
            something round.
        width_m (float): How far it reaches either side of its bearing.
        bearing_deg (float): Which way it runs, degrees true.
        compose (str): `RAISE`, `CARVE` or `SHAPE`.
        marked (bool): Whether a chart should carry a symbol for it regardless of what
            the soundings say. True exactly when a chart sampling the terrain would
            print more water over the feature than there is - which is the whole
            reason the marks layer exists, and is measured rather than judged.
        substrate (str, optional): What it is made of, if it overrules the ordinary
            bottom. None leaves the bottom to be derived from the shape of the ground,
            which is right for a bank but wrong for a rock.

    """

    kind: str
    at: object
    target_m: float
    length_m: float
    width_m: float
    bearing_deg: float = 0.0
    compose: str = RAISE
    marked: bool = False
    substrate: str = None

    def reach_m(self):
        """Beyond this the bump is exactly nothing, so nothing need be evaluated."""
        return math.hypot(self.length_m, self.width_m)


class Placed:
    """
    A feature with its frame worked out, ready to be asked about.

    Notes:
        The tangent frame is built once at construction. Building one per sample would
        have made a handful of stamped features cost more than the entire tectonic system.

    """

    def __init__(self, feature, radius_m=EARTH_RADIUS_M):
        self.feature = feature
        self.frame = TangentFrame.at(feature.at, radius_m)
        radians = math.radians(feature.bearing_deg)
        self._along_e, self._along_n = math.sin(radians), math.cos(radians)
        self._across_e, self._across_n = math.cos(radians), -math.sin(radians)
        self._cos_reach = math.cos(min(math.pi, feature.reach_m() / radius_m))

    def weight_at(self, point):
        """
        How strongly this feature applies here.

        Args:
            point (SpherePoint): Where.

        Returns:
            weight (float): Nothing to one, smooth everywhere.

        Notes:
            The rejection is three multiplies against a cosine worked out once, and it
            sits strictly outside the support: past `reach_m` the bump is already exactly
            zero, so skipping the projection there cannot make a step. The rule the whole
            engine is built on, applied to the cheapest gate in it.

        """
        if point.vector.dot(self.feature.at.vector) < self._cos_reach:
            return 0.0
        east, north = self.frame.sphere_to_local(point)
        along = east * self._along_e + north * self._along_n
        across = east * self._across_e + north * self._across_n
        return _bump(along, self.feature.length_m) * _bump(across, self.feature.width_m)


class Features:
    """
    Every placed feature on a world.

    Notes:
        A list, iterated. There are a dozen of these, not a million: they are the things
        somebody deliberately put somewhere, and a world wanting thousands has wanted a
        generator rather than a stamp.

    """

    def __init__(self, features=(), radius_m=EARTH_RADIUS_M):
        self.placed = tuple(Placed(feature, radius_m) for feature in features)
        self.radius_m = radius_m

    def __len__(self):
        return len(self.placed)

    def __iter__(self):
        return iter([placed.feature for placed in self.placed])

    def apply(self, point, elevation_m):
        """
        The ground after everything placed here has had its say, and how much say it had.

        Args:
            point (SpherePoint): Where.
            elevation_m (float): The ground before features.

        Returns:
            shaped (tuple): `(metres, authority)`. Authority runs nothing to one and is
                what detail uses to get out of the way.

        Notes:
            **`RAISE` and `CARVE` are one-way, and that is not a hard decision in
            disguise.** A raise whose target is already below the ground contributes
            nothing, and at the moment the two are equal it contributes nothing either -
            so the switch happens exactly where the effect is zero and the ground stays
            continuous. The same argument every tectonic gate had to survive.

            Authority needs that argument made a second time and differently. It is not
            zero at the switch merely because the contribution is: it would jump from
            nothing to the full weight the instant a feature began to apply. So it ramps
            over `SETTLE_M` of relief - which is also the behaviour worth having, since a
            feature reshaping the bed by centimetres should not take its texture away.

            Order is meaning here. A bar listed after the channel it lies across sits on
            the carved bottom, which is the right story; listed before, the channel would
            cut straight through it.

        """
        result = elevation_m
        authority = 0.0
        for placed in self.placed:
            weight = placed.weight_at(point)
            if weight <= 0.0:
                continue
            lift = placed.feature.target_m - result
            if placed.feature.compose == RAISE and lift <= 0.0:
                continue
            if placed.feature.compose == CARVE and lift >= 0.0:
                continue
            result += weight * lift
            authority = max(authority, weight * _smooth(abs(lift) / SETTLE_M))
        return result, authority

    def marks_near(self, point, within_m):
        """
        Placed features close enough to belong on a chart as symbols.

        Args:
            point (SpherePoint): Where the chart is centred.
            within_m (float): How much sea it covers.

        Returns:
            marks (tuple): `(distance_m, feature)` pairs, nearest first.

        Notes:
            **The second channel, and the reason it has to exist.** A pinnacle a hundred
            metres across cannot survive a chart sampled every four hundred: it is not
            smoothed away, it is *missed* - and worse, whether it is missed depends on
            where the sample grid happens to fall, so it would blink in and out as a ship
            moved. Real charts answer this by giving isolated dangers a symbol instead of
            a contour, and so does this. The terrain still carries the rock at full height
            for anything that asks canonically, which is everything that can run aground.

        """
        found = []
        for placed in self.placed:
            if not placed.feature.marked:
                continue
            distance = point.distance_to(placed.feature.at, self.radius_m)
            if distance <= within_m:
                found.append((distance, placed.feature))
        found.sort(key=lambda pair: pair[0])
        return tuple(found)
