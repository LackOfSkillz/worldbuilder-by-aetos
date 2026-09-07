"""The round trip, end to end, in one command.

    read an Evennia database  ->  lay each area out  ->  place it on a generated planet
      ->  write a worldfile  ->  apply it back to Evennia  ->  read it back and check

**It never writes to the database it read.** The source is opened read-only and copied
first; the copy is what gets written. A demonstration does not get to modify somebody's
game, and neither should the first version of the real tool.

Run it:

    python -m evennia_roundtrip.demo --database path/to/game.db3 --out world.json
"""

import argparse
import json
import os
import sys

from worldbuilder.terrain.surface import Surface

from . import apply as apply_module
from . import evdb, layout, place, worldfile

#: The planet these coordinates are on. The same four numbers reproduce it exactly, which
#: is why the worldfile carries them rather than a copy of the terrain.
DEMO_PLANET = {
    "seed": 20260904,
    "radius_m": 6371000.0,
    "plate_count": 12,
    "land_fraction": 0.29,
}

#: This build. A worldfile stamped with anything else is refused rather than reopened.
GENERATOR_VERSION = "0.1.0-roundtrip"


def choose(areas, count, minimum_rooms):
    """Pick areas worth demonstrating: connected, laid out by direction, big enough.

    Sorted by how well the walk agreed with itself, because an area whose exits contradict
    each other makes a bad first map and the point here is to show the pipeline working,
    not to hide that some areas are hard.
    """
    scored = []
    for area in areas.values():
        if len(area.rooms) < minimum_rooms:
            continue
        built = layout.spread(layout.build(area))
        scored.append((built.components, -built.agreement, -len(area.rooms), area.name, built))
    scored.sort()
    return [(name, built, areas[name]) for _, _, _, name, built in scored[:count]]


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--database", required=True, help="Evennia .db3 to read")
    parser.add_argument("--out", default="world.json", help="worldfile to write")
    parser.add_argument("--applied", default=None,
                        help="where to put the written-to copy (default: alongside --out)")
    parser.add_argument("--areas", type=int, default=2, help="how many areas to place")
    parser.add_argument("--min-rooms", type=int, default=20)
    parser.add_argument("--spacing", type=float, default=place.DEFAULT_ROOM_SPACING_M)
    parser.add_argument("--separation", type=float, default=80000.0,
                        help="metres between anchors; small values cluster the areas into "
                             "one region, which is what a single game's world looks like")
    parser.add_argument("--region-radius", type=float, default=400000.0,
                        help="how far from the first harbour the other areas may sit")
    parser.add_argument("--world", default=None,
                        help="a worldfile whose planet these areas are placed on. Without it "
                             "the demo planet is used, and coordinates from one planet are "
                             "meaningless on another.")
    parser.add_argument("--fit", action="store_true",
                        help="shrink each area's room spacing until it sits on land, and "
                             "say so. Without it an area half in the sea exports clean.")
    parser.add_argument("--anchor-room", action="append", default=None,
                        metavar="AREA:ROOM=LAT,LON[,BEARING][,SPACING]",
                        help="put one NAMED ROOM at a chosen point. A builder points at a "
                             "dock, not at whichever room the graph walk happened to start "
                             "from. ROOM matches a room key, case-insensitively.")
    parser.add_argument("--anchor", action="append", default=None, metavar="NAME=LAT,LON[,BEARING]",
                        help="place a named area at a chosen point instead of a found coast. "
                             "A builder's decision beats a search, and this is where that "
                             "decision lives until the studio exists.")
    arguments = parser.parse_args(argv)

    print("reading %s" % arguments.database)
    areas = evdb.read(arguments.database)
    print(evdb.census(areas, minimum=arguments.min_rooms))
    print()

    chosen = choose(areas, arguments.areas, arguments.min_rooms)
    if not chosen:
        print("no area had %d rooms" % arguments.min_rooms)
        return 1

    planet = dict(DEMO_PLANET)
    if arguments.world:
        with open(arguments.world, encoding="utf-8") as handle:
            saved = json.load(handle)["planet"]
        planet = {
            "seed": int(saved["seed"]),
            "radius_m": float(saved.get("radius", DEMO_PLANET["radius_m"])),
            "plate_count": int(saved.get("plates", DEMO_PLANET["plate_count"])),
            "land_fraction": float(saved.get("land", DEMO_PLANET["land_fraction"])),
        }
        # **Say what is being ignored rather than ignoring it.** The Python oracle takes four
        # parameters; a worldfile from the studio carries more, and the mountain and coast
        # sliders are among them. Elevations here come from the four-parameter planet, so a
        # world that uses the others gets ground from a DIFFERENT planet unless somebody is
        # told. This is the same failure that put 226 rooms four kilometres under water.
        ignored = sorted(k for k in saved
                         if k not in {"seed", "radius", "plates", "land"})
        if ignored:
            print("WARNING: this build reads seed, radius, plates and land only.")
            print("         %d parameters in the worldfile are NOT applied: %s"
                  % (len(ignored), ", ".join(ignored)))
            print("         Elevations below are from the four-parameter planet.")
    print("planet: seed %s, radius %.0f m, %d plates, land %.2f"
          % (planet["seed"], planet["radius_m"], planet["plate_count"],
             planet["land_fraction"]))

    surface = Surface(
        planet["seed"],
        radius_m=planet["radius_m"],
        plate_count=planet["plate_count"],
        land_fraction=planet["land_fraction"],
    )

    # Anchors are FOUND, not assumed. The first version of this demo reused a latitude and
    # longitude measured on a different planet, and placed every room in four kilometres of
    # water - correctly, and uselessly. Choosing anchors visually is the studio's job; until
    # the studio exists, the planet is asked where its coasts are.
    # Ask for at least one anchor on a genuine harbour and let the rest fall where the
    # coast allows, so the port mapping has both cases to answer rather than neither.
    # Hand-authored anchors first. Anything named here is placed where the builder said and
    # never searched for, because the search answers "where COULD this go" and a builder
    # answers "where does this go".
    fixed = {}
    for entry in arguments.anchor or []:
        name, _, rest = entry.partition("=")
        parts = [float(p) for p in rest.split(",")]
        fixed[name] = place.Anchor(parts[0], parts[1],
                                   parts[2] if len(parts) > 2 else 0.0, arguments.spacing)
    # Resolved after the layouts exist, because placing a named room needs its cell.
    room_anchors = {}
    for entry in arguments.anchor_room or []:
        target, _, rest = entry.partition("=")
        area_name, _, room_key = target.partition(":")
        parts = [float(p) for p in rest.split(",")]
        room_anchors[area_name] = (room_key, parts)

    if fixed or room_anchors:
        print("anchors given by the builder: %s"
              % ", ".join(sorted(set(fixed) | set(room_anchors))))

    # **Naming an anchor is naming an area.** Without this the selector kept its own
    # ranking, placed two areas the builder had not asked for, ignored the two he had,
    # and then failed on an empty list - a confusing way to say "you asked for these and
    # I chose others".
    wanted = set(fixed) | set(room_anchors)
    if wanted:
        named = [entry for entry in chosen if entry[0] in wanted]
        for name in sorted(wanted - {entry[0] for entry in chosen}):
            if name in areas:
                named.append((name, layout.spread(layout.build(areas[name])), areas[name]))
            else:
                print("no area called %r in this database" % name)
        chosen = named
        if not chosen:
            print("none of the anchored areas exist in this database")
            return 1

    points = []
    if all(name in wanted for name, _built, _area in chosen):
        # Every area is placed by hand, so the coast search has nothing to decide. Running
        # it anyway is minutes of work whose answer is thrown away.
        print("every area is hand-anchored; no coast search needed")
    else:
        print("searching for anchors: one harbour, then its neighbourhood...")
        points = list(place.coastal_anchors(surface, 1, require_port=True))
        if points:
            # The rest are found NEAR the harbour, because a game's areas are one world and
            # not pins scattered over a globe. `separation_m` cannot do this - it is a
            # MINIMUM distance, so lowering it from 80 km to 12 km left the anchors 4,500 km
            # apart, unchanged. A radius around a chosen centre is what actually clusters.
            for point in place.coastal_anchors(
                surface, len(chosen) * 3, separation_m=arguments.separation,
                near=points[0], within_m=arguments.region_radius,
            ):
                if len(points) >= len(chosen):
                    break
                if all(point.distance_to(other, surface.radius_m) > 1.0 for other in points):
                    points.append(point)
        unanchored = [name for name, _b, _a in chosen if name not in wanted]
        if len(points) < len(unanchored):
            print("found %d coastal anchors for %d areas that need one"
                  % (len(points), len(unanchored)))
            keep = set(wanted) | set(unanchored[: len(points)])
            chosen = [entry for entry in chosen if entry[0] in keep]

    bearings = (0.0, 30.0, 300.0, 120.0, 210.0)
    found_anchors = [
        place.Anchor(lat, lon, bearings[index % len(bearings)], arguments.spacing)
        for index, (lat, lon) in enumerate(point.to_latlon() for point in points)
    ]

    placements, layouts, area_by_name = {}, {}, {}
    next_found = 0
    for name, built, area in chosen:
        if name in room_anchors:
            room_key, parts = room_anchors[name]
            match = [rid for rid, room in area.rooms.items()
                     if room.key.lower().replace(" ", "_") == room_key.lower()
                     or room.key.lower() == room_key.lower()]
            if not match:
                print("no room like %r in %s; skipping" % (room_key, name))
                continue
            anchor = place.anchor_for_room(
                built, match[0], parts[0], parts[1],
                parts[2] if len(parts) > 2 else 0.0,
                parts[3] if len(parts) > 3 else arguments.spacing,
                radius_m=planet["radius_m"],
            )
            print("  %s anchored on %r at %.6f, %.6f"
                  % (name, area.rooms[match[0]].key, parts[0], parts[1]))
        elif name in fixed:
            anchor = fixed[name]
        else:
            anchor = found_anchors[next_found]
            next_found += 1
        area_by_name[name] = area
        if arguments.fit:
            spacing, _ = place.fit_spacing(
                built, area, anchor.latitude_deg, anchor.longitude_deg,
                anchor.bearing_deg, surface,
            )
            if spacing and spacing != anchor.room_spacing_m:
                print("  %s: %.0f m spacing puts rooms in the water; %.0f m fits"
                      % (name, anchor.room_spacing_m, spacing))
                anchor = place.Anchor(anchor.latitude_deg, anchor.longitude_deg,
                                      anchor.bearing_deg, spacing)
            elif spacing is None:
                print("  %s: no spacing tried keeps this area on land" % name)
        rooms = place.place(built, area, anchor, surface)
        fit = place.land_fit(rooms)
        if not fit["fits"]:
            print("  LAND CHECK FAILED for %s: %d rooms on water that should not be: %s"
                  % (name, fit["wet_unexpected"],
                     ", ".join("%s (%.1f m)" % o for o in fit["offenders"][:6])))
        else:
            print("  land check: %d dry, %d wet by design (%s)"
                  % (fit["dry"], fit["wet_expected"],
                     "ramps, slips and docks" if fit["wet_expected"] else "none"))
        placements[name] = (anchor, rooms)
        layouts[name] = built
        wet = sum(1 for room in rooms if room.submerged)
        heights = [room.elevation_m for room in rooms]
        print(
            "placed %-26s %3d rooms at %.4f,%.4f bearing %.0f  "
            "ground %.0f..%.0f m, %d submerged, agreement %.0f%%"
            % (name, len(rooms), anchor.latitude_deg, anchor.longitude_deg,
               anchor.bearing_deg, min(heights), max(heights), wet,
               built.agreement * 100)
        )

    ports = place.port_mapping(placements, surface)
    for name, entry in sorted(ports.items()):
        if entry["has_port"]:
            print("  %-26s has its own port" % name)
        elif entry["port_area"]:
            print("  %-26s inland; nearest port %s, %.1f km (%s)"
                  % (name, entry["port_area"], (entry["port_distance_m"] or 0) / 1000.0,
                     entry["port_metric"]))
        else:
            print("  %-26s inland, and no placed area has a port" % name)

    document = worldfile.build(
        planet=planet,
        source={
            "database": os.path.basename(arguments.database),
            "area_key_priority": list(evdb.AREA_KEY_PRIORITY),
        },
        placements=placements,
        layouts=layouts,
        ports=ports,
        generator_version=GENERATOR_VERSION,
        areas_by_name=area_by_name,
    )
    worldfile.write(document, arguments.out)
    print("\nwrote %s (%d bytes)" % (arguments.out, os.path.getsize(arguments.out)))

    reopened = worldfile.read(arguments.out, generator_version=GENERATOR_VERSION)
    print("reopened it; version check passed")

    applied = arguments.applied or os.path.join(
        os.path.dirname(os.path.abspath(arguments.out)), "applied.db3"
    )
    apply_module.copy_database(arguments.database, applied)
    report = apply_module.apply(reopened, applied)
    print("applied to %s: %d rooms across %d areas, %d missing"
          % (applied, report["rooms"], report["areas"], len(report["missing"])))

    # The proof. Not "the write did not raise" - the values are read back out of the
    # database through a second connection and compared against the file.
    sample = [room["id"] for room in reopened["areas"][0]["rooms"][:5]]
    back = apply_module.verify(applied, sample)
    expected = {room["id"]: room for room in reopened["areas"][0]["rooms"]}
    bad = 0
    for room_id, values in back.items():
        want = expected[room_id]
        if (abs(values.get(apply_module.WB_LATITUDE, 1e9) - want["latitude_deg"]) > 1e-12
                or abs(values.get(apply_module.WB_LONGITUDE, 1e9) - want["longitude_deg"]) > 1e-12):
            bad += 1
    print("\nread back %d rooms from the game database; %d disagreed with the worldfile"
          % (len(back), bad))
    for room_id in sample[:3]:
        values = back[room_id]
        print("  #%-6d %-34s %9.5f, %9.5f  %7.1f m  port=%s"
              % (room_id, expected[room_id]["key"][:34],
                 values[apply_module.WB_LATITUDE], values[apply_module.WB_LONGITUDE],
                 values[apply_module.WB_ELEVATION], values[apply_module.WB_PORT_AREA]))
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
