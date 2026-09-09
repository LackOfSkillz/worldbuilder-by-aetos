"""Who is actually standing in the rooms.

**The world reported a population it never made.** Every area carried an `npcs` figure and
the tally beside the globe added them up, but the number was `rooms x density` and nothing
in the worldfile answered to it - so a run that announced three thousand people exported
none, and a player walked a hundred rooms of a market town without meeting anybody. The
count is now the length of a list, which is the only kind of count that cannot drift.

**Named by trade and station, not by proper name.** "A stallholder" is a person a player can
talk to, rob or follow; "Eldrin Ashvale" is an invented proper noun in a world whose names
are supposed to come from its cultures, and inventing three thousand of them is how a
generator starts writing lore nobody asked it for. Roles also survive translation into any
setting, which proper names do not.

**A shop gets its keeper before anywhere else gets anybody.** A shop with goods on the
counter and nobody behind it is the thing a player notices first, and the room's own name
already says which trade is being kept.
"""

import random

#: Who keeps each trade, keyed by the marker in the room's name. The keys are the same ones
#: `stock.WARES` uses, so the room that was stocked as a weaponsmith is kept by a smith and
#: neither table has to know about the other.
KEEPERS = {
    "inn": "an innkeeper",
    "weaponsmith": "a smith",
    "armourer": "an armourer",
    "general store": "a storekeeper",
    "alchemist": "an apothecary",
    "counting house": "a clerk",
    "healer": "a healer",
    "stables": "an ostler",
    "shrine": "a shrine-keeper",
    "market stalls": "a stallholder",
    "camp": "a camp cook",
}

#: Everybody else, by the voice of the place. Ordinary people doing ordinary things, because
#: a street where every passer-by is remarkable is a street nobody believes.
FOLK = {
    "human": ("a carter", "a water-carrier", "a porter", "an old woman on a stool",
              "a boy running an errand", "a sweeper"),
    "dwarf": ("a hauler", "a stone-cutter", "a lamp-trimmer", "a tally-keeper",
              "an off-shift miner", "a girl carrying tools"),
    "elf": ("a bough-walker", "a keeper of the young trees", "a quiet archer",
            "a gatherer with a basket", "a rope-mender"),
    "gnome": ("a bench-hand", "an apprentice covered in oil", "a belt-minder",
              "a note-taker", "a fetcher of small parts"),
    "saurathi": ("a reed-cutter", "a punt-poler", "a fish-smoker", "a net-mender",
                 "a watcher on the causeway"),
    "halfling": ("a hedger", "a churn-carrier", "a boy with a dog", "an orchard-picker",
                 "a woman with a basket of eggs"),
    "volgrin": ("a drover", "a horse-breaker", "a picket-watcher", "a saddler",
                "a child among the ponies"),
    "felari": ("a quay-hand", "a rope-coiler", "a fish-seller", "a boat-minder",
               "a diver drying off"),
    "lunari": ("a watch-keeper", "a herd-follower", "a ridge-walker", "a horn-carrier",
               "a wrapped figure resting"),
    "aethari": ("a terrace-sweeper", "a water-steward", "a reader of the steps",
                "a bearer of jars", "a keeper of the colonnade"),
    "valran": ("a shepherd", "a fold-mender", "a peat-cutter", "a woman spinning",
               "a boy counting sheep"),
    "goblin": ("a scavenger", "a sharp-eyed lookout", "a keeper of the midden",
               "a squabbling pair", "a small one hiding"),
    "road": ("a carter resting his team", "a pedlar with a pack", "a pilgrim",
             "a drover following his beasts", "a mounted messenger"),
}

#: What is alive in the wild, by the voice of the ground. These are the "NPCs" of a hunting
#: area, and they are the reason the ground is worth walking into.
QUARRY = {
    "game_wood": ("a red deer", "a roe doe", "a woodcock", "a fox at the edge of cover",
                  "a boar rooting"),
    "game_marsh": ("a mallard", "a heron", "a snipe", "a marsh hare", "a flight of duck"),
    "game_moor": ("a moor hare", "a red grouse", "a ram on the skyline", "a curlew",
                  "a hill fox"),
    "game": ("a deer", "a hare", "a covey of quail", "a pheasant", "a wild goat"),
    "wild_marsh": ("a marsh adder", "a wading bird", "something moving in the reeds"),
    "wild_upland": ("a mountain hare", "an eagle very high up", "a wild goat"),
    "wild_shore": ("a gull", "a seal on the rocks", "a crab among the stones"),
    "wild": ("a wary animal", "something watching from cover"),
}

#: How many of the ordinary sort stand in a settlement, per room. A city is busier than a
#: hamlet by more than its size, which is most of what makes it feel like a city.
DENSITY = {"city": 0.9, "seat": 0.8, "town": 0.6, "village": 0.45, "hamlet": 0.3,
           "camp": 0.25, "road": 0.12}


def keeper_for(room_key, keepers=None):
    """
    Who keeps this room, or None if it is not a shop.

    Args:
        room_key (str): The room's name, which says what trade it is.
        keepers (dict, optional): The table to read, for a test that wants its own.

    Returns:
        role (str or None): What to call the person behind the counter.
    """
    lowered = (room_key or "").lower()
    for trade in keepers or KEEPERS:
        if trade in lowered:
            return (keepers or KEEPERS)[trade]
    return None


def populate(area, voice, rng, density=None):
    """
    Put people in an area's rooms, in place.

    Args:
        area (dict): One area, with `rooms` and a `size`.
        voice (str): Which vocabulary the place speaks, as `naming` uses the word.
        rng (random.Random): The world's own generator, so a seed reproduces a world.
        density (dict, optional): Rooms-to-people by settlement size.

    Returns:
        people (int): How many were placed, which is what the tally should report.

    Notes:
        **Every shop first, then the rest scattered.** Filling rooms in order would put
        the whole population in the north-west corner of the lattice, which is what
        happened to the first draft of the trade rooms and reads as a queue rather than a
        town.

        A hunting ground is populated from `QUARRY` instead: the things worth walking in
        for are animals, and a deer is as much an NPC as a stallholder is.
    """
    rooms = area.get("rooms") or []
    if not rooms:
        return 0

    wild = QUARRY.get(voice)
    pool = list(wild or FOLK.get(voice) or FOLK["human"])
    placed = 0

    for room in rooms:
        room["people"] = []

    if not wild:
        for room in rooms:
            role = keeper_for(room.get("key"))
            if role:
                room["people"].append({"name": role, "role": "keeper"})
                placed += 1

    rate = (density or DENSITY).get(area.get("size"), 0.3)
    if wild:
        # The wild is thinner than a street and does not keep shop.
        rate = 0.35
    wanted = int(round(len(rooms) * rate))
    open_rooms = [room for room in rooms if not room["people"]]
    rng.shuffle(open_rooms)
    for room in open_rooms[:wanted]:
        room["people"].append({"name": rng.choice(pool),
                               "role": "quarry" if wild else "folk"})
        placed += 1
    return placed


def count(area):
    """
    Args:
        area (dict): One area.

    Returns:
        people (int): How many people its rooms actually hold.
    """
    return sum(len(room.get("people") or ()) for room in (area.get("rooms") or ()))
