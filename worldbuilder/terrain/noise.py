"""
Deterministic noise, sampled in three dimensions on the sphere.

**Three dimensions, not two.** A two-dimensional field cannot be wrapped onto a sphere
without a seam down one meridian and a pinch at each pole - the two places a globe always
eventually fails. Sampling a volume at the point's own position on the unit sphere has
neither, because there is no wrapping happening at all: the sphere simply passes through
the noise.

**Hashed, never sequenced.** Every lattice value comes from an integer hash of its own
coordinates and the seed, so the value at a lattice point depends on nothing but that
point. There is no generator whose position could matter and no order that could change
an answer.
"""

MASK = (1 << 64) - 1
SCALE = float(1 << 64)


def _lattice(ix, iy, iz, seed):
    """
    Args:
        ix (int): Lattice coordinate.
        iy (int): Lattice coordinate.
        iz (int): Lattice coordinate.
        seed (int): The world's seed.

    Returns:
        value (float): In [0, 1), and the same forever for the same arguments.

    Notes:
        An integer avalanche rather than a cryptographic digest. A hash from `hashlib`
        would be just as deterministic and about thirty times slower, and this is called
        eight times per octave per sample.

    """
    h = (ix * 0x9E3779B97F4A7C15) ^ (iy * 0xC2B2AE3D27D4EB4F) ^ (iz * 0x165667B19E3779F9)
    h = (h ^ (seed * 0x27D4EB2F165667C5)) & MASK
    h ^= h >> 33
    h = (h * 0xFF51AFD7ED558CCD) & MASK
    h ^= h >> 33
    h = (h * 0xC4CEB9FE1A85EC53) & MASK
    h ^= h >> 33
    return h / SCALE


class Noise:
    """
    A field of value noise, with the lattice memoised.

    Notes:
        **This cache is not the kind of caching the design forbids.** What is banned is
        quantising a *position* to a grid and reusing a neighbour's answer, because that
        moves things - a reef that shifts two hundred metres when somebody zooms. This
        memoises a pure function of three integers and a seed, returning exactly the value
        it would otherwise recompute. Nothing is approximated and no answer changes.

        It earns its place because of the scales involved. Continental noise has
        wavelengths of thousands of kilometres, so an entire chart - two hundred
        kilometres across - falls inside one or two lattice cells. Without the cache every
        sample rehashes the same eight corners.

    """

    def __init__(self, seed, salt=0):
        #: Salted so that two fields on the same world - continentality here, roughness
        #: later - are independent rather than the same shape at different amplitudes.
        self.seed = (seed * 0x100000001B3) ^ (salt * 0x9E3779B97F4A7C15)
        self._corners = {}

    def _fill(self, key):
        """
        The eight lattice values around one cell, worked out once and kept.

        Notes:
            **Cached by cell rather than by corner: the same values in one lookup instead
            of eight.** The first version kept a dictionary of corners and called a method
            per corner. Profiling a chart redraw found 2.9 million of those calls, and the
            *call overhead* was twice the cost of the dictionary lookup inside it.

            Neighbouring cells store their shared corners twice over, which is eight times
            the memory for the same coverage and worth it - a corner is a float, and what
            it buys is seven fewer Python-level lookups on the hottest path in the engine.

        """
        ix, iy, iz = key
        seed = self.seed
        jx, jy, jz = ix + 1, iy + 1, iz + 1
        corners = (
            _lattice(ix, iy, iz, seed), _lattice(jx, iy, iz, seed),
            _lattice(ix, jy, iz, seed), _lattice(jx, jy, iz, seed),
            _lattice(ix, iy, jz, seed), _lattice(jx, iy, jz, seed),
            _lattice(ix, jy, jz, seed), _lattice(jx, jy, jz, seed),
        )
        self._corners[key] = corners
        return corners

    def at(self, x, y, z):
        """
        Args:
            x (float): Position in noise space.
            y (float): Position in noise space.
            z (float): Position in noise space.

        Returns:
            value (float): In [0, 1), smooth and continuous everywhere.

        Notes:
            Trilinear between the eight surrounding lattice values, with each fraction
            put through a smoothstep first. Straight linear interpolation would leave
            visible creases along every lattice plane - and on terrain a crease is a cliff
            somebody sails into.

            Written flat rather than tidily. This is called about forty times per terrain
            sample and several million times per chart, so the arithmetic is inline, the
            lattice lookup is one dictionary hit, and nothing here builds an object.

        """
        ix, iy, iz = int(x // 1), int(y // 1), int(z // 1)
        fx, fy, fz = x - ix, y - iy, z - iz
        ux = fx * fx * (3.0 - 2.0 * fx)
        uy = fy * fy * (3.0 - 2.0 * fy)
        uz = fz * fz * (3.0 - 2.0 * fz)

        key = (ix, iy, iz)
        corners = self._corners.get(key)
        if corners is None:
            corners = self._fill(key)
        c000, c100, c010, c110, c001, c101, c011, c111 = corners

        x00 = c000 + (c100 - c000) * ux
        x10 = c010 + (c110 - c010) * ux
        x01 = c001 + (c101 - c001) * ux
        x11 = c011 + (c111 - c011) * ux
        y0 = x00 + (x10 - x00) * uy
        y1 = x01 + (x11 - x01) * uy
        return y0 + (y1 - y0) * uz

    def fbm(self, point, frequency, octaves, gain=0.5, lacunarity=2.0):
        """
        Several octaves of noise, summed.

        Args:
            point (SpherePoint): Where on the planet.
            frequency (float): Cycles per radian of arc, roughly, for the first octave.
            octaves (int): How many to sum. Each is half the amplitude and twice the
                frequency of the one before.
            gain (float, optional): How much quieter each octave is.
            lacunarity (float, optional): How much finer each octave is.

        Returns:
            value (float): Centred on zero, roughly in [-1, 1].

        Notes:
            The octave count is a parameter rather than a constant because a chart drawn
            at twenty-two miles has samples four hundred metres apart, and octaves finer
            than that are invisible - they cost time to produce detail below the
            resolution being drawn, and they alias while doing it. Dropping them there is
            both faster and more correct, which is why the caller decides.

        """
        vector = point.vector
        total = 0.0
        amplitude = 1.0
        loudest = 0.0
        for _ in range(octaves):
            total += (
                self.at(vector.x * frequency, vector.y * frequency, vector.z * frequency)
                - 0.5
            ) * amplitude
            loudest += amplitude
            amplitude *= gain
            frequency *= lacunarity
        return 2.0 * total / loudest if loudest else 0.0
