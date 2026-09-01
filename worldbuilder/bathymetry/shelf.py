"""
The continental shelf: the water a ship actually sails in.

For a maritime simulation this is the most important terrain on the planet. Mountains six
hundred kilometres inland are scenery; the first hundred metres below sea level is where
anchoring, sounding, grounding and pilotage all happen, and generic noise there produces
gorgeous continents with unusable coasts.

Three rules, and two of them are scars.

**The shelf sets a target depth, and the ground is blended towards it.** Not an offset
added on. A shelf describes what the coastal profile should *tend to*, and blending leaves
control over what it may override - so a trench crossing a continental margin is not
quietly flattened by something announcing that the water here is about a hundred metres.

**Every gate sits outside the support of what it gates.** The general form of M1.4's worst
bug, where a trench profile was still a thousand metres tall at the range limit and the
optimisation that skipped it became a cliff. Each gate below is either placed where the
function is already zero, or paired with a weight that has faded to nothing before it.

**Nothing is classified.** Not "is this a continent", not "is this an island", not "is
this near a coast". M1.4 produced four separate cliffs from four hard decisions taken on
continuous quantities, and every equivalent temptation here is answered with a weight.
"""

from dataclasses import dataclass

from ..geometry.sphere import EARTH_RADIUS_M

#: How far from the shore, in field units, a point may be and still be considered.
#: Measured against a gradient of about two parts in ten million per metre, this is
#: roughly a quarter of a thousand kilometres - comfortably wider than any shelf.
#:
#: The cheap first gate: a value, no gradient, no probes. Deep interiors and deep basins
#: fail it having done one field evaluation, which is the whole performance strategy,
#: because a gradient costs six times what a value does.
COASTAL_WINDOW = 0.055

#: Below this, the field is too flat to say where the coast is - the estimate `c / |grad|`
#: divides by nearly nothing and returns a distance to a shore on the far side of the
#: world. The weight has already faded out by here; this only stops the arithmetic.
MIN_GRADIENT = 1.0e-8

#: A gradient typical of a continental margin, measured: the median near real coastlines
#: on this generator is about 1.9 parts in ten million per metre. Steeper means a smaller
#: landmass, which gets a correspondingly narrower platform - an island does not deserve a
#: hundred-kilometre apron merely for being above water.
REFERENCE_GRADIENT = 2.0e-7

#: How far offshore the shelf break lies on a broad margin, and how deep the shelf is at
#: its outer edge. Beyond the break the weight fades and the macro depth takes over, which
#: is what draws the continental slope without anybody having to model one.
SHELF_BREAK_M = 80_000.0
SHELF_EDGE_M = -150.0
SLOPE_WIDTH_M = 70_000.0

#: How far inland the shelf's influence reaches. Small: it shapes the approach, not the
#: country behind it.
INLAND_REACH_M = 12_000.0

#: How much tectonic relief it takes to hold the shelf off. A trench or an uplift belt is
#: deliberate structure and outranks a general statement about coastal depth.
#:
#: Measured down from seven hundred. At that value a coast sitting on three hundred metres
#: of tectonic uplift still gave the shelf a weight of 0.59, which dragged the mountain
#: down to a hundred and twenty-five metres - the shelf quietly demolishing the range it
#: was supposed to defer to. Two hundred and fifty leaves it at 0.14 there, which shapes
#: the water without levelling the land.
TECTONIC_AUTHORITY_M = 250.0


def _smooth(fraction):
    """Smoothstep, clamped. Flat at both ends, so nothing it gates leaves a crease."""
    clamped = max(0.0, min(1.0, fraction))
    return clamped * clamped * (3.0 - 2.0 * clamped)


@dataclass(frozen=True)
class Reading:
    """
    The ground at a point, with the expensive intermediates that produced it.

    Attributes:
        elevation_m (float): Relative to datum, structural only.
        weight (float): How much say the shelf had, nothing to one.
        tectonic_m (float): What the plates contributed.

    """

    elevation_m: float
    weight: float
    tectonic_m: float


@dataclass(frozen=True)
class Coastal:
    """
    Where a point stands relative to the nearest shore, as far as can be told locally.

    Attributes:
        distance_m (float): Estimated metres to the shoreline. Positive inland, negative
            at sea.
        breadth (float): How broad the landmass is, from nothing to one. A proxy, from
            how gently continentality changes here.

    """

    distance_m: float
    breadth: float


class Shelf:
    """
    Coastal bathymetry, laid over the macro terrain.

    Notes:
        Reads the tectonic layer rather than replacing it, and the composition is a blend
        rather than a sum - which is what lets a trench survive crossing a margin.

    """

    def __init__(self, tectonics, continentality, radius_m=EARTH_RADIUS_M):
        self.tectonics = tectonics
        self.land = continentality
        self.radius_m = radius_m

    def coastal(self, point):
        """
        How far the shore is, estimated from the field and its slope.

        Args:
            point (SpherePoint): Anywhere on the planet.

        Returns:
            coastal (Coastal or None): None where the question is not worth asking.

        Notes:
            **A local linear estimate, and named to admit it.** Continentality crosses
            zero at the shore, so dividing the value by the magnitude of its gradient
            gives the distance to that crossing if the field carried on at the same slope
            - which it does not, exactly. Over the tens of kilometres a shelf occupies,
            against a field whose features are thousands of kilometres wide, it is close
            enough to build on and nowhere near an exact geodesic distance to the final
            shoreline.

            The value is checked before the gradient is taken, and that ordering is the
            whole performance strategy of this file: the gradient costs six times what the
            value does, and most of a planet is deep interior or deep basin.

        """
        value = self.land.above_shore(point)
        if abs(value) > COASTAL_WINDOW:
            # Far from any shore. The weight below is already zero here, so this gate is
            # outside the support of what it gates rather than a cliff in it.
            return None

        gradient = self.land.gradient(point)
        slope = gradient.magnitude()
        if slope < MIN_GRADIENT:
            return None

        return Coastal(
            distance_m=value / slope,
            breadth=_smooth(REFERENCE_GRADIENT / slope),
        )

    def target_depth_m(self, coastal):
        """
        What the water ought to be doing at this distance from shore.

        Args:
            coastal (Coastal): From `coastal`.

        Returns:
            metres (float): The depth the shelf tends towards.

        Notes:
            Only the shelf itself is described. The continental slope is not modelled at
            all - beyond the break the blend weight fades and the macro ocean depth comes
            back, and the transition between them *is* the slope. One fewer profile to
            write and one fewer place for two descriptions of the same water to disagree.

        """
        offshore = -coastal.distance_m
        if offshore <= 0.0:
            return 0.0
        break_at = SHELF_BREAK_M * max(0.15, coastal.breadth)
        return SHELF_EDGE_M * _smooth(offshore / break_at)

    def weight(self, point, coastal, tectonic_m=None):
        """
        How much say the shelf has here.

        Args:
            point (SpherePoint): Where.
            coastal (Coastal): From `coastal`.
            tectonic_m (float, optional): The tectonic offset, if the caller already has
                it. Worked out again if not, which is what makes it worth passing.

        Returns:
            weight (float): Nothing to one.

        Notes:
            Four things fade it, and every one of them replaces a decision that could have
            been a hard test:

            *How far from the shore*, so the shelf reaches its own edge at nothing rather
            than being cut off at the window boundary.

            *How broad the landmass is*, so a continent gets a wide shelf and an isolated
            island a narrow platform with steep-to water - without either being classified
            as anything.

            *How far inland*, quickly, because a shelf shapes the approach and not the
            country behind it.

            *What the plates already said*. A trench is deliberate deep structure and
            outranks a general remark about coastal depth. Without this the shelf would
            cheerfully fill a subduction trench in with a hundred metres of water and the
            most dramatic thing on the chart would vanish.

        """
        offshore = -coastal.distance_m
        break_at = SHELF_BREAK_M * max(0.15, coastal.breadth)

        if offshore >= 0.0:
            seaward = 1.0 - _smooth((offshore - break_at) / SLOPE_WIDTH_M)
        else:
            seaward = 1.0 - _smooth(-offshore / INLAND_REACH_M)

        if tectonic_m is None:
            tectonic_m = self.tectonics.offset_m(point)
        authority = 1.0 - _smooth(abs(tectonic_m) / TECTONIC_AUTHORITY_M)

        return seaward * coastal.breadth * authority

    def evaluate(self, point):
        """
        The ground here, and the working that produced it.

        Args:
            point (SpherePoint): Anywhere on the planet.

        Returns:
            reading (Reading): Elevation, the shelf's weight, and the tectonic offset.

        Notes:
            The intermediates come back because the layer above wants them and they are
            expensive. Asking separately cost the gradient twice and the tectonics three
            times over, which took a whole-pipeline chart from three hundred milliseconds
            to twelve hundred - a comment claiming the values were "recovered rather than
            recomputed where it is free" while they were being recomputed.

        """
        tectonic = self.tectonics.offset_m(point)
        macro = self.land.base_elevation(point) + tectonic

        coastal = self.coastal(point)
        if coastal is None:
            return Reading(elevation_m=macro, weight=0.0, tectonic_m=tectonic)

        weight = self.weight(point, coastal, tectonic)
        if weight <= 0.0:
            return Reading(elevation_m=macro, weight=0.0, tectonic_m=tectonic)

        shaped = macro + weight * (self.target_depth_m(coastal) - macro)
        return Reading(elevation_m=shaped, weight=weight, tectonic_m=tectonic)

    def elevation_m(self, point):
        """
        The ground, with coastal shaping applied.

        Args:
            point (SpherePoint): Anywhere on the planet.

        Returns:
            metres (float): Relative to datum.

        """
        return self.evaluate(point).elevation_m
