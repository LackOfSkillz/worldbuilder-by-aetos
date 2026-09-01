"""
Three-dimensional vectors, which is all the algebra a sphere needs.

Deliberately small and deliberately dependency-free. Everything above this file works in
unit vectors from the planet's centre, so what is wanted here is exactly the operations
that serve that: dot and cross products, normalisation, and enough arithmetic to build a
basis. Nothing else earns its place.

Frozen, because a position is a reading. Code that could edit one in place would change it
for everything else holding the same object, and the whole design rests on the same point
answering the same way every time it is asked.

**Slotted, because this is the hottest type in the engine.** A chart redraw builds and
reads hundreds of thousands of these, and a frozen dataclass without slots keeps a
per-instance dictionary and goes through `object.__setattr__` to fill it. Slots make
construction and attribute access materially cheaper and change no value whatsoever -
which was checked by hashing every answer the world gives before and after.
"""

import math
from dataclasses import dataclass


@dataclass(frozen=True, slots=True)
class Vec3:
    """
    Attributes:
        x (float): Towards longitude zero on the equator.
        y (float): Towards ninety degrees east on the equator.
        z (float): Towards the north pole.

    """

    x: float
    y: float
    z: float

    def __add__(self, other):
        return Vec3(self.x + other.x, self.y + other.y, self.z + other.z)

    def __sub__(self, other):
        return Vec3(self.x - other.x, self.y - other.y, self.z - other.z)

    def scaled(self, factor):
        """
        Args:
            factor (float): What to multiply each component by.

        Returns:
            scaled (Vec3): The same direction, a different length.

        """
        return Vec3(self.x * factor, self.y * factor, self.z * factor)

    def dot(self, other):
        """
        Returns:
            product (float): The scalar product.

        Notes:
            For two unit vectors this is the cosine of the angle between them, which is
            what makes it the whole of "which of these is nearer" without a single
            trigonometric call.

        """
        return self.x * other.x + self.y * other.y + self.z * other.z

    def cross(self, other):
        """
        Returns:
            product (Vec3): A vector at right angles to both.

        """
        return Vec3(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )

    def length(self):
        """
        Returns:
            length (float): How long the vector is.

        """
        return math.sqrt(self.dot(self))

    def normalised(self):
        """
        Returns:
            unit (Vec3): The same direction, length one.

        Raises:
            ValueError: If the vector has no length, and so no direction to preserve.

        """
        magnitude = self.length()
        if magnitude == 0.0:
            raise ValueError("A zero vector has no direction to normalise.")
        return self.scaled(1.0 / magnitude)


#: The axis the planet turns about, and so the direction of the north pole. Used to build
#: local frames: east is the direction at right angles to both this and straight up, which
#: is exactly what "east" means anywhere it means anything at all.
NORTH_AXIS = Vec3(0.0, 0.0, 1.0)

#: What to build a frame from when the north axis is no use, which happens precisely at
#: the poles - where "east" has no meaning and any answer is as good as any other. The
#: direction chosen does not matter. That the same direction is chosen every time does.
POLAR_FALLBACK = Vec3(1.0, 0.0, 0.0)

#: How nearly parallel two vectors may be before their cross product stops being a
#: trustworthy direction. At the poles the true value is zero; this is the band around it
#: where the arithmetic has lost its nerve before the maths has.
DEGENERATE = 1e-9
