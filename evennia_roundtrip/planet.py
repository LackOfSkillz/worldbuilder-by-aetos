"""A worldfile's planet block, as the three blocks the engine wants.

A saved world records twenty-four numbers with the studio's names for them - `mtnCount`,
`mtnWanderWave`, `quieting` - and the engine takes `continental_blend`,
`margin_warp_wavelength_m` and `quieting_strength`. Nothing translated between them, so
Python could build any world at all *except* one of the worlds actually saved. This is that
translation, and it is a port of `relief-params.js`, `tectonic-params.js` and
`coast-params.js` rather than an invention.

**Every block is a delta over the engine's own canonical preset, and that is the load-
bearing idea.** The studio holds no default literals: `reliefFromParams` starts from the
canonical record the engine hands it and overrides only fields the query string names. Its
own comment says why - the panel once kept a copy of the elevation ramp, drifted from the
engine's, and silently reverted a setting when opened, and the fix was to delete the copy
rather than correct it. Python is the third consumer of those numbers and follows the same
rule: canonical comes from `worldbuilder_engine`, never from a table here.

**A block nobody moved is `None`, not a copy of canonical.** `None` is the engine's own
canonical path, byte for byte. Passing a reconstructed canonical block instead would be
arithmetically identical today and would quietly become a different world the first time a
preset was retuned - the copy-drift failure again, one level up.

**A field the worldfile does not mention keeps canonical's value.** That is not leniency: a
worldfile records what a builder set, and the studio's own round trip drops any field still
at canonical so a shared link carries only what moved. Requiring every field here would
refuse every world the studio has ever saved.
"""

import worldbuilder_engine as engine

#: Studio name -> engine field, for each channel. Ported from the three `*_PARAM_NAMES`
#: tables in `viewer/public/app/`, which are the only place these pairings are stated.
RELIEF_NAMES = {
    "mountainM": "mountain_m",
    "quieting": "quieting_strength",
    "persistence": "octave_persistence",
}

TECTONIC_NAMES = {
    "mtnHeight": "continent_collision_m",
    "mtnWidth": "continent_collision_width_m",
    "mtnCount": "continental_blend",
    "mtnAsym": "collision_asymmetry",
    "mtnBelts": "suture_count",
    "mtnBeltSpacing": "suture_spread_m",
    "mtnStructure": "structure_depth",
    "mtnStructureWave": "structure_wavelength_m",
    "mtnWander": "margin_warp_m",
    "mtnWanderWave": "margin_warp_wavelength_m",
}

COAST_NAMES = {
    "coast": "amplitude",
    "coastBand": "window_spreads",
    "coastFreq": "frequency",
    "coastOctaves": "octaves",
    "coastGain": "gain",
    "coastLacunarity": "lacunarity",
}

#: The four scalars that are not a block: they are arguments in their own right.
SCALARS = {"seed": "world_seed", "radius": "radius_m", "plates": "plate_count",
           "land": "land_fraction"}

#: Studio keys that reach a system this translation does not cover, so that an unknown key
#: can be an error without every saved world tripping it.
#:
#: `size` and `maxLevel` are the tile pyramid, `clouds` and `lakeNodes` and `harbour` and
#: `featureCeiling` belong to layers above the surface. They are listed rather than ignored
#: silently, because the difference between "this belongs elsewhere" and "nobody has wired
#: this up yet" is exactly what a builder needs told.
ELSEWHERE = ("size", "maxLevel", "clouds", "lakeNodes", "harbour", "featureCeiling",
             "gully", "gullyDepth", "gullyWidth")


def _number(value):
    """The studio writes every planet value as a string; the engine wants floats."""
    return float(value)


def _block(planet, names, canonical):
    """One channel: canonical, overlaid with what the worldfile actually names.

    Returns `None` when nothing was moved - see the module note on why that is not the same
    as returning a copy of canonical.
    """
    block = dict(canonical)
    touched = False
    for studio_name, field in names.items():
        if studio_name not in planet:
            continue
        value = _number(planet[studio_name])
        # The studio drops a field still at canonical when it writes a link, so a value
        # equal to canonical means "explicitly unmoved" and must not mark the block dirty.
        # Without this, a world that set nothing but wrote every field out would take the
        # non-canonical path and be a different world for no stated reason.
        if value == canonical[field]:
            continue
        block[field] = value
        touched = True
    return block if touched else None


def scalars(planet):
    """
    The four arguments every surface call takes.

    Returns:
        four (dict): `world_seed`, `radius_m`, `plate_count`, `land_fraction`.

    Raises:
        KeyError: If the worldfile names none of them. A planet with no seed is not a
            planet this can reproduce, and guessing one would produce a different world in
            silence.
    """
    missing = [key for key in SCALARS if key not in planet]
    if missing:
        raise KeyError("planet block is missing %s" % ", ".join(sorted(missing)))
    return {
        "world_seed": int(_number(planet["seed"])),
        "radius_m": _number(planet["radius"]),
        "plate_count": int(_number(planet["plates"])),
        "land_fraction": _number(planet["land"]),
    }


def blocks(planet):
    """
    The three opt-in blocks a worldfile's planet asks for.

    Args:
        planet (dict): A worldfile's `planet` block, values as strings or numbers.

    Returns:
        three (dict): `relief`, `tectonics` and `coast`, each a dict or None.
    """
    return {
        "relief": _block(planet, RELIEF_NAMES, engine.relief_canonical()),
        "tectonics": _block(planet, TECTONIC_NAMES, engine.tectonics_canonical()),
        "coast": _block(planet, COAST_NAMES, engine.coast_canonical()),
    }


def unmapped(planet):
    """
    Studio keys this translation does not carry to the engine.

    Returns:
        found (dict): `elsewhere` for keys that belong to another system, and `unknown` for
        keys nothing here recognises at all.

    Notes:
        **The second list is the one that matters.** A key nobody recognises is either a new
        engine parameter that has not been wired up or a typo in a saved file, and both look
        exactly like a world that generates fine and is not the world that was saved. The
        `gully=65"` incident was one stray quote doing precisely that.
    """
    known = set(RELIEF_NAMES) | set(TECTONIC_NAMES) | set(COAST_NAMES) | set(SCALARS)
    return {
        "elsewhere": sorted(k for k in planet if k in ELSEWHERE),
        "unknown": sorted(k for k in planet if k not in known and k not in ELSEWHERE),
    }


def elevation_at(planet, resolution_m=None, features=None, features_radius_m=None):
    """
    A `(latitude_deg, longitude_deg) -> metres` for this world.

    Args:
        planet (dict): The worldfile's planet block.
        resolution_m (float, optional): Detail cutoff, passed straight through.
        features (list, optional): Authored features, as the binding's tuples.
        features_radius_m (float, optional): The radius pre-built features were placed at.

    Returns:
        sample (callable): The oracle every placement tool has been missing.

    Notes:
        This is the whole point of the exercise. `place`, `docks` and the site scorer all
        want to ask the ground a question, and until now the only answer Python could give
        came from four of this world's twenty-four parameters - which put a boat ramp four
        hundred metres under water and reported it as an ordinary depth.
    """
    import math

    four = scalars(planet)
    three = blocks(planet)

    def sample(latitude_deg, longitude_deg):
        lat, lon = math.radians(latitude_deg), math.radians(longitude_deg)
        cos_lat = math.cos(lat)
        return engine.surface_elevation_m(
            four["world_seed"], four["radius_m"], four["plate_count"],
            four["land_fraction"],
            cos_lat * math.cos(lon), cos_lat * math.sin(lon), math.sin(lat),
            resolution_m=resolution_m,
            features=features, features_radius_m=features_radius_m,
            relief=three["relief"], tectonics=three["tectonics"], coast=three["coast"],
        )

    return sample
