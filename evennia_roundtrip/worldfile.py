"""The worldfile: a saved world, and the interface a contrib reads.

**This is the format of record**, which was the owner's decision over the smaller
alternative of carrying placement alone. It costs a larger public promise and buys an
author a world that is not hostage to a running server: one file to version, diff, review
and hand to somebody else.

**Two halves that save very differently.** The planet is entirely described by its seed and
its parameters - determinism is the save format, and the header is all it needs. The
authored half is somebody's work: anchors, bearings, spacings, and every override. Losing
it is losing the only copy. So the header costs bytes and the areas cost whatever they
cost.

**Version discipline: fail closed.** A worldfile records the generator that produced it. A
newer generator reading an older file must refuse rather than silently render something
else - a planet that quietly changes after an engine change is worse than one that will
not open. `check_version` is where that refusal lives.
"""

import json
from dataclasses import asdict

#: Bumped whenever a field changes meaning. Adding an optional field does not bump it.
#:
#: `features` arrived without a bump under exactly that rule: a reader that ignores it gets
#: the planet it always got. **But a reader that ignores it now gets the WRONG GROUND**, so
#: the field is not optional in practice, and a consumer that means to draw this world has
#: to apply them. That is a documentation problem rather than a schema one, and it is
#: written here rather than discovered by somebody whose harbour is a hillside.
WORLDFILE_VERSION = 1

#: What produced the coordinates in a file. A file made by a different generator version
#: is refused rather than reinterpreted.
GENERATOR = "worldbuilder"


class VersionRefused(Exception):
    """Raised rather than opening a worldfile this build cannot reproduce."""


def build(planet, source, placements, layouts, ports, generator_version,
          areas_by_name=None, features=()):
    """
    Assemble a worldfile from everything the pipeline produced.

    Args:
        planet (dict): `seed`, `radius_m`, `plate_count`, `land_fraction`.
        source (dict): Where the areas were read from.
        placements (dict): Area name to (Anchor, list of PlacedRoom).
        layouts (dict): Area name to Layout.
        ports (dict): From `place.port_mapping`.
        generator_version (str): The engine build these coordinates came from.

    Returns:
        document (dict): Ready for `write`.

    """
    areas = []
    for name in sorted(placements):
        anchor, rooms = placements[name]
        layout = layouts[name]
        areas.append(
            {
                "name": name,
                "anchor": {
                    "latitude_deg": anchor.latitude_deg,
                    "longitude_deg": anchor.longitude_deg,
                    "bearing_deg": anchor.bearing_deg,
                    "room_spacing_m": anchor.room_spacing_m,
                },
                # Not decoration. These say how much the coordinates below can be trusted,
                # and a builder reading a surprising map needs them next to the map.
                "layout_quality": {
                    "root_room": layout.root,
                    "components": layout.components,
                    "inferred_rooms": len(layout.inferred),
                    "conflicts": len(layout.conflicts),
                    "agreement": round(layout.agreement, 4),
                },
                "port": ports.get(name, {}),
                "rooms": [asdict(room) for room in rooms],
                # **The exits travel with the rooms.** A worldfile carrying positions and no
                # connections describes where a game's rooms are and not what its map is, and
                # the studio cannot draw a path between two docks from a scatter of points.
                "exits": [
                    {"name": e.name, "source": e.source, "destination": e.destination}
                    for e in ((areas_by_name or {}).get(name).exits
                              if (areas_by_name or {}).get(name) else [])
                ],
            }
        )
    return {
        "worldfile_version": WORLDFILE_VERSION,
        "generator": {"name": GENERATOR, "version": generator_version},
        "planet": planet,
        # **The changes somebody made to the planet, in order.** A world is a planet, the
        # authored changes, and the rooms placed on the result - and a file carrying the
        # first and the last describes a river town with no river.
        #
        # ORDER IS MEANING and the list is not a set: a bar listed after the channel it
        # crosses sits on the carved bottom; listed before, the channel cuts through it.
        "features": list(features),
        "source": source,
        "areas": areas,
    }


def write(document, path):
    """Save a worldfile. Sorted keys and a trailing newline, so it diffs."""
    with open(path, "w", encoding="utf-8") as handle:
        json.dump(document, handle, indent=2, sort_keys=True)
        handle.write("\n")
    return path


def read(path, generator_version=None):
    """
    Open a worldfile, refusing one this build cannot reproduce.

    Args:
        path (str): The file.
        generator_version (str, optional): This build's version. When given, a file from
            any other version is refused.

    Returns:
        document (dict): The worldfile.

    Raises:
        VersionRefused: The file's schema or generator does not match.

    """
    with open(path, encoding="utf-8") as handle:
        document = json.load(handle)
    check_version(document, generator_version)
    return document


def check_version(document, generator_version=None):
    """Refuse a worldfile rather than reinterpret it. No silent substitution."""
    found = document.get("worldfile_version")
    if found != WORLDFILE_VERSION:
        raise VersionRefused(
            "worldfile schema %r, this build reads %r" % (found, WORLDFILE_VERSION)
        )
    if generator_version is None:
        return document
    stamped = (document.get("generator") or {}).get("version")
    if stamped != generator_version:
        raise VersionRefused(
            "worldfile was generated by %r, this build is %r; the same seed would not "
            "reproduce the same planet, so it will not be opened" % (stamped, generator_version)
        )
    return document
