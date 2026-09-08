"""What a world is allowed to contain, and the words that betray a world it is not.

**One area mentioned engine sounds.** That is the whole reason this exists. A generator
writing prose will reach for whatever its training makes fluent, and fluent modern English
is full of machinery: engines idle, pipes hiss, lamps are switched on. Every one of those
is invisible in a sentence and fatal in a high fantasy world, and none of them is caught by
a word-count band or a graph measurement.

**This is a denylist and it is deliberately not an allowlist.** The same argument the
creature renamer settled: English is too large to enumerate, and an allowlist refused three
hundred and sixty-three perfectly ordinary words before it caught a single wrong one. What
CAN be enumerated is the small set of things that do not exist here - and a world with
gnomes experimenting with steam needs a list that admits steam and refuses petrol.

**A period is a property of the world, not of the generator.** Worldbuilder has no genre;
`FORBIDDEN` below is one world's answer, chosen by whoever owns that world. A steampunk
world would delete half of it and a science fiction world would keep none.
"""

import re

#: Words and phrases no high fantasy world may contain, by what they give away.
#:
#: Grouped so a refusal can say WHICH anachronism it found rather than only that it found
#: one - "gunpowder" and "diesel" are both wrong and they are wrong in different centuries.
FORBIDDEN = {
    "internal combustion": (
        "engine", "engines", "motor", "motors", "piston", "pistons", "carburettor",
        "carburetor", "exhaust", "petrol", "gasoline", "diesel", "fuel tank", "spark plug",
        "throttle", "ignition", "horsepower", "revving", "idling engine",
    ),
    "electricity": (
        "electric", "electrical", "electricity", "battery", "batteries", "wire", "wires",
        "wiring", "circuit", "circuits", "switch on", "switched on", "light bulb",
        "bulb", "generator", "voltage", "power line", "socket", "plug socket",
    ),
    "firearms": (
        "gun", "guns", "gunpowder", "rifle", "rifles", "pistol", "pistols", "musket",
        "muskets", "cannon", "cannons", "bullet", "bullets", "cartridge", "shotgun",
        "revolver", "artillery",
    ),
    "modern industry": (
        "factory", "factories", "conveyor", "assembly line", "plastic", "concrete",
        "asphalt", "tarmac", "rubber", "aluminium", "aluminum", "stainless steel",
        "machine shop", "industrial",
    ),
    "modern transport": (
        "car", "cars", "truck", "trucks", "lorry", "bus", "buses", "train", "trains",
        "railway", "railroad", "tractor", "bicycle", "motorcycle", "aeroplane",
        "airplane", "aircraft", "helicopter",
    ),
    "modern life": (
        "telephone", "phone", "radio", "television", "camera", "computer", "clock tower",
        "wristwatch", "newspaper", "photograph", "elevator", "escalator", "plumbing",
        "sewer pipe", "refrigerator", "thermostat",
    ),
    "wrong register": (
        "okay", "ok", "guys", "cool", "awesome", "teenager", "weekend", "schedule",
        "manager", "customer", "employee", "parking", "office",
    ),
}

#: What this world DOES have, despite looking like something on the list above.
#:
#: **Gnomes may experiment with steam.** The owner said so, and a lint that refuses the one
#: piece of technology the world is supposed to be discovering would be worse than no lint.
#: A boiler and a bellows are period; a carburettor is not, and "steam engine" is the phrase
#: where the two meet - so it is named here rather than left to the reader.
PERMITTED = (
    "steam", "boiler", "bellows", "forge", "furnace", "kiln", "cog", "cogs", "gear",
    "gears", "clockwork", "spring", "lever", "pulley", "winch", "crank", "waterwheel",
    "windmill", "mill", "lantern", "lamp", "oil lamp", "candle", "torch", "brazier",
    "alembic", "crucible", "loom", "spindle", "anvil", "cannonade",
)

#: Phrases that are permitted even though a forbidden word sits inside them.
#:
#: Checked before the denylist, because "engine" is refused and "siege engine" is a trebuchet.
PHRASE_EXCEPTIONS = (
    "siege engine", "siege engines", "engine of war", "wire-drawn", "gold wire",
    "silver wire", "wire brush", "spun wire", "bulb of garlic", "flower bulb",
    "bulbs of garlic", "switch of birch", "cannon bone",
)


def _normalise(text):
    return re.sub(r"\s+", " ", (text or "").lower())


def offences(text, forbidden=None, exceptions=PHRASE_EXCEPTIONS):
    """
    Every anachronism in one piece of text.

    Args:
        text (str): Prose, a room name, an item name - anything a player will read.
        forbidden (dict, optional): Category to words. Defaults to `FORBIDDEN`.
        exceptions (tuple, optional): Phrases that are allowed regardless.

    Returns:
        found (list): `(category, word)` pairs, in the order the categories are declared.

    Notes:
        **Whole words, not substrings.** "Wire" must not fire on "wiry", "car" must not fire
        on "cart" or "carve", and "gun" must not fire on "begun" - and a substring test does
        all three. That kind of false positive is what teaches somebody to switch a lint off.
    """
    haystack = _normalise(text)
    for phrase in exceptions:
        haystack = haystack.replace(phrase, " ")
    found = []
    for category, words in (forbidden or FORBIDDEN).items():
        for word in words:
            pattern = r"(?<!\w)%s(?!\w)" % re.escape(word)
            if re.search(pattern, haystack):
                found.append((category, word))
    return found


def clean(text, **kwargs):
    """Whether a piece of text is free of anachronism."""
    return not offences(text, **kwargs)


def check_area(area, **kwargs):
    """
    Every anachronism in a finished area, named by where it is.

    Args:
        area (dict): With `name`, `display_name` and `rooms` carrying `key` and `desc`.

    Returns:
        problems (list): Readable lines, one per offence.
    """
    problems = []
    for field in ("name", "display_name", "blurb"):
        for category, word in offences(area.get(field), **kwargs):
            problems.append("%s: %s (%s)" % (field, word, category))
    for room in area.get("rooms") or ():
        for field in ("key", "desc"):
            for category, word in offences(room.get(field), **kwargs):
                problems.append("room %s %s: %s (%s)"
                                % (room.get("id", "?"), field, word, category))
    return problems
