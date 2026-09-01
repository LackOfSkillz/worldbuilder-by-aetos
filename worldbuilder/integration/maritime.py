"""
A generated planet, presented as a map provider the maritime contrib can sail on.

**The dependency runs one way and this is the only file that knows both.** Maritime never
imports worldbuilder and never will; a contrib that needed a planet generator to be
installed would be a contrib nobody could adopt. So the adapter lives here, on the
generator's side, and it fits itself to maritime rather than the other way round.

Nothing in maritime changes to accept it. The interface it implements - `terrain_z_at`,
`bottom_type_at`, `hazards_touching` - is the interface the base provider already declares,
and maritime tests none of its providers with `isinstance`, so a duck answers. That is
checked rather than assumed: `maritime_provider` builds a real subclass when maritime is
importable, and the mixin below works standalone so the generator keeps no dependency at
all and can be tested without an Evennia install.

**A maritime region is a tangent plane, which is what a chart is.** Maritime works in flat
metres east and north within a named region; the planet works in unit vectors. One
`TangentFrame` per region converts between them, and the two-hundred-kilometre cap M1.1
measured is a cap on how large a region may be, not a limitation to work around - beyond
it, two charted points disagree by more than a ship's length.

**Depth is truth here, error is the chart's job.** `terrain_z_at` answers canonically,
because maritime already models a chart's ignorance separately: `charted_terrain_z_at` adds
a deterministic sounding error on top of whatever the world says. A provider that answered
approximately would be adding a second, unmodelled error underneath the first.
"""

import math

from ..bathymetry.substrate import MUD, ROCK, SAND
from ..geometry.tangent import TangentFrame

#: What worldbuilder calls a bottom, in maritime's vocabulary. The two happen to agree on
#: all three, which is not an accident - both took the names from a chart legend - but the
#: mapping is written out anyway, because the day one of them adds gravel is the day an
#: implicit identity becomes a silent bug.
BOTTOM_NAMES = {SAND: "sand", MUD: "mud", ROCK: "rock"}

#: How far apart the circles are that stand in for a long thin hazard, as a fraction of its
#: width. Two thirds, so consecutive circles overlap and a hull cannot pass between them.
MOLE_STEP = 0.66

#: The most circles one feature may become. A breakwater is a dozen; anything wanting
#: hundreds has been authored at the wrong scale and would make every step of every voyage
#: pay for it.
MOST_CIRCLES = 24

#: How far apart a chart's soundings are, and how much shoaler than one a piece of ground
#: has to be before it is a hazard rather than a bottom.
#:
#: The same rule the marks layer is defined by, applied one level down. A feature is marked
#: because a chart lies about it *somewhere*; a circle is a hazard where a chart lies about
#: it *there*. Without this, the outer circles of a breakwater - which taper into ordinary
#: seabed seventeen metres down - were reported as dangers, and a harbour approach was foul
#: for two kilometres either side of a structure nobody could hit.
CHART_SPACING_M = 400.0
LIES_BY_M = 2.0


def _distance_to_track(x, y, from_x, from_y, to_x, to_y):
    """
    How close a point comes to a line segment.

    Notes:
        Written out rather than imported from maritime, which has its own. Borrowing it
        would make the generator depend on the thing it is adapting to, for eight lines of
        arithmetic.

    """
    run_x, run_y = to_x - from_x, to_y - from_y
    length = run_x * run_x + run_y * run_y
    if length <= 0.0:
        return math.hypot(x - from_x, y - from_y)
    along = ((x - from_x) * run_x + (y - from_y) * run_y) / length
    along = max(0.0, min(1.0, along))
    return math.hypot(x - (from_x + along * run_x), y - (from_y + along * run_y))


class Danger:
    """
    One circular hazard, in a region's flat metres.

    Notes:
        Shaped like maritime's `Hazard` on purpose - same field names, same meaning of
        `top_z` as an elevation against datum rather than a depth. `maritime_provider`
        converts these into the real thing; standalone, they are what the tests see.

    """

    __slots__ = ("key", "x", "y", "radius", "top_z", "bottom", "region")

    def __init__(self, key, x, y, radius, top_z, bottom, region):
        self.key = key
        self.x, self.y = x, y
        self.radius = radius
        self.top_z = top_z
        self.bottom = bottom
        self.region = region

    def __repr__(self):
        return (f"Danger({self.key!r}, x={self.x:.0f}, y={self.y:.0f}, "
                f"radius={self.radius:.0f}, top_z={self.top_z:.1f})")


class WorldbuilderTerrain:
    """
    The generated world, answering maritime's questions in one region.

    Notes:
        Everything is computed on demand except the hazards, which are worked out once at
        construction. A region has a handful of marks and their positions never change, so
        recomputing them on every step of every voyage would be paying for nothing.

    """

    def __init__(self, surface, region, anchor=None, region_name="default",
                 features=None):
        """
        Args:
            surface (Surface): The generated planet.
            region (Region, optional): A region from `worldbuilder.regions`, which supplies
                the anchor, the features and the name.
            anchor (SpherePoint, optional): Where the region's origin sits, if there is no
                `Region` to take it from.
            region_name (str, optional): What maritime calls this coordinate space.
            features (Features, optional): Overrides the region's own.

        """
        self.surface = surface
        if region is not None:
            anchor = anchor or region.origin
            features = features if features is not None else region.features
        if anchor is None:
            raise ValueError("a maritime region needs an anchor to sit on")
        self.region = region
        self.region_name = region_name
        self.frame = TangentFrame.at(anchor, surface.radius_m)
        self.features = features if features is not None else surface.features
        self.dangers = self._survey()

    def point_at(self, position):
        """
        Where on the planet a maritime position is.

        Args:
            position: Anything with `x` and `y` in metres.

        Returns:
            point (SpherePoint): The same place, on the sphere.

        """
        return self.frame.local_to_sphere(position.x, position.y)

    def terrain_z_at(self, position):
        """
        Ground elevation at a point, ignoring any water above it.

        Args:
            position (WorldPosition): Where. Only x, y and region are used.

        Returns:
            terrain_z (float): Metres against datum. Negative is seabed.

        Notes:
            Canonical, always. Maritime models a chart's ignorance separately and adds its
            own deterministic sounding error on top of this; answering approximately here
            would put a second, unmodelled error underneath the first, and a ship taking a
            fix would be wrong in a way nothing had accounted for.

        """
        return self.surface.elevation_m(self.point_at(position))

    def bottom_type_at(self, position):
        """
        What the seabed is made of.

        Args:
            position (WorldPosition): Where.

        Returns:
            bottom (str): One of maritime's bottom types.

        """
        return BOTTOM_NAMES[self.surface.bottom_at(self.point_at(position)).dominant]

    def hazards_touching(self, before, after, width=0.0):
        """
        Every mark a hull would sweep through on this track.

        Args:
            before (WorldPosition): Where the step started.
            after (WorldPosition): Where it would end.
            width (float, optional): Her beam, in metres.

        Returns:
            hazards (tuple): What she would touch, shallowest first.

        Notes:
            **This is the other end of the marks layer, and the reason it exists.** A
            pinnacle a hundred and forty metres across is missed by sixty-three chart grids
            in sixty-four, and would be missed by a hull sampled at seven points on her
            outline for exactly the same reason. Measured as a circle against the whole
            corridor she swept, it cannot be missed at all.

            Shallowest first, so a caller taking the first entry gets the worst news.

        """
        reach = width * 0.5
        touched = [
            danger for danger in self.dangers
            if _distance_to_track(
                danger.x, danger.y, before.x, before.y, after.x, after.y
            ) <= danger.radius + reach
        ]
        touched.sort(key=lambda danger: -danger.top_z)
        return tuple(touched)

    def charted_dangers(self, position, reach):
        """
        Every mark inside the square a sheet covers.

        Args:
            position (WorldPosition): Where the sheet is centred.
            reach (float): How far it extends from there, in metres.

        Returns:
            dangers (tuple): What a survey would have recorded, shallowest first.

        Notes:
            The same circles `hazards_touching` measures a hull against, asked about a
            box instead of a track. That they are the same list is the point: a chart
            that showed one set of rocks while the physics used another would be a chart
            that lies in a new and more interesting way.

            A square, because that is the shape of the paper and a rock just off the
            corner of it is still on it.

        """
        reach = abs(reach)
        near = [
            danger for danger in self.dangers
            if abs(danger.x - position.x) <= reach and abs(danger.y - position.y) <= reach
        ]
        near.sort(key=lambda danger: -danger.top_z)
        return tuple(near)

    def _survey(self):
        """
        Work out every hazard in the region, once.

        Returns:
            dangers (tuple): Circles, in the region's flat metres.

        Notes:
            **A circle is a bad fit for a breakwater, so a breakwater becomes several.** A
            mole two kilometres long and three hundred and forty metres wide is a hazard,
            and a single circle round it would either miss most of it or declare two square
            kilometres of harbour approach foul. Overlapping circles of its own width,
            laid along its length, describe it closely enough that nothing passes through.

            Each circle is given the elevation of the ground at its own centre rather than
            the feature's stated target, because the ends of a mole taper and quoting the
            crest for them would put a wall where there is a ramp.

            **And a circle is only kept where a chart would lie about it**, which is the
            same rule that decided the feature was marked in the first place. Kept
            unconditionally, the outer circles of a breakwater - which taper into ordinary
            seabed seventeen metres down - came back as dangers, and two kilometres of
            harbour approach either side of the structure were foul ground for a hull that
            could not have touched anything.

        """
        dangers = []
        for placed in self.features.placed:
            feature = placed.feature
            if not feature.marked:
                continue
            bottom = BOTTOM_NAMES.get(feature.substrate or SAND, "sand")
            radius = min(feature.length_m, feature.width_m)
            kept = 0
            for centre in self._circles(placed):
                top_z = self.surface.elevation_m(centre)
                if top_z - self._charted(centre) <= LIES_BY_M:
                    continue
                kept += 1
                east, north = self.frame.sphere_to_local(centre)
                key = feature.kind if kept == 1 else f"{feature.kind} ({kept})"
                dangers.append(Danger(
                    key=key, x=east, y=north, radius=radius, top_z=top_z,
                    bottom=bottom, region=self.region_name,
                ))
        return tuple(dangers)

    def _charted(self, centre, spacing_m=CHART_SPACING_M):
        """
        The shoalest sounding a chart would print near a point.

        Notes:
            Sampled on a lattice anchored to the region, because a chart does not get to
            choose where its grid falls. Worked out once per candidate circle at
            construction, never on a voyage.

        """
        east, north = self.frame.sphere_to_local(centre)
        base_col, base_row = round(east / spacing_m), round(north / spacing_m)
        shoalest = -9e9
        for row in (base_row - 1, base_row, base_row + 1):
            for col in (base_col - 1, base_col, base_col + 1):
                sounding = self.surface.elevation_m(
                    self.frame.local_to_sphere(col * spacing_m, row * spacing_m),
                    spacing_m,
                )
                shoalest = max(shoalest, sounding)
        return shoalest

    def _circles(self, placed):
        """
        Where to put the circles that stand in for one feature.

        Args:
            placed (Placed): The feature and its frame.

        Returns:
            centres (list): Points on the sphere, along the feature.

        """
        feature = placed.feature
        narrow = min(feature.length_m, feature.width_m)
        long = max(feature.length_m, feature.width_m)
        if narrow <= 0.0 or long <= narrow * 1.2:
            return [feature.at]

        step = max(narrow * MOLE_STEP, 1.0)
        count = min(MOST_CIRCLES, int(2.0 * long / step) + 1)
        along_length = feature.length_m >= feature.width_m
        centres = []
        for index in range(count):
            offset = (2.0 * index / max(count - 1, 1) - 1.0) * long
            east, north = (offset, 0.0) if along_length else (0.0, offset)
            radians = math.radians(feature.bearing_deg)
            centres.append(placed.frame.local_to_sphere(
                east * math.sin(radians) + north * math.cos(radians),
                east * math.cos(radians) - north * math.sin(radians),
            ))
        return centres


def maritime_provider(surface, region, region_name="default", tide_provider=None):
    """
    A map provider maritime can be handed directly.

    Args:
        surface (Surface): The generated planet.
        region (Region): Which piece of it this coordinate space covers.
        region_name (str, optional): What maritime calls the space.
        tide_provider (optional): Maritime's tide provider, if not the flat default.

    Returns:
        provider: A `MaritimeMapProvider` subclass backed by the generated world.

    Raises:
        ImportError: If maritime is not installed, with a message saying so rather than a
            traceback through somebody else's package.

    Notes:
        The one function here that needs maritime present. `WorldbuilderTerrain` above
        carries all of the behaviour and none of the dependency, which is what lets the
        generator test this without an Evennia install - and what would let a different
        game with a different interface reuse it.

        Subclassing rather than duck-typing, when maritime *is* there, because the base
        provider derives water depth, submersion and the surface and seabed positions from
        `terrain_z_at` and the tides. Those are real behaviour and reimplementing them here
        would be two sources of truth for the same water.

    """
    try:
        from evennia.contrib.full_systems.maritime.bathymetry import MaritimeMapProvider
    except ImportError as exc:  # pragma: no cover - depends on what is installed
        raise ImportError(
            "maritime_provider needs the maritime contrib installed. Use "
            "WorldbuilderTerrain directly for the terrain without it."
        ) from exc

    class _WorldbuilderProvider(WorldbuilderTerrain, MaritimeMapProvider):
        def __init__(self):
            MaritimeMapProvider.__init__(self, tide_provider)
            WorldbuilderTerrain.__init__(
                self, surface, region, region_name=region_name
            )

    return _WorldbuilderProvider()
