"""Things to look at: laws F1 and F2.

**F1: every town room carries one to three things to look at**, fitted to the room and to
the people who built it - a dwarf's hall has a pick rack and tally marks, an inn its tap board
and hearth. **F2: a road carries a landmark every eight to twelve rooms and something to look at
in one room in three.** Both are in the area-building laws (Gary's decision, 2026-09-10): a
SHOULD there, so hand-built zones written before them are warned; a MUST here, because a
generator that can furnish every room has no excuse not to.

**A room with nothing in it but its description is read once.** A room with a mirror in it is
looked at, and a mirror that shows a player their own face is the thing they tell somebody
about. That is the whole case for the law, and why the mirror is not just a word: it and the
clock are built as small typeclasses the export ships with the world, so the mirror reflects
whoever looks into it and the clock reads the game's own time.

**Keys are bare nouns** - "horse trough", not "a horse trough". Evennia puts the article on
itself when it lists a room's contents, and would make "the mirror" into "a the mirror".

Every description here is permanently true, the same rule a room's is held to: no weather,
no hour, nobody coming or going.
"""

#: Things in the street, by the voice of the people who built it. Each culture's list begins
#: with the street furniture its own vocabulary already mentions, so a description that says
#: "a low stone bench stands against the wall" and the bench a player can look at agree.
STREET = {
    "human": [
        ("horse trough", "A long trough of dressed stone, its rim worn smooth where generations "
                         "of bridles have rested. The water in it is clear and cold."),
        ("signpost", "A post leaning a little out of true, with arms pointing three ways. The "
                     "lettering has been recut more than once."),
        ("public well", "A well with a stone curb and a windlass of blackened oak. A chained "
                        "cup hangs from the frame."),
        ("notice board", "A board so thick with nails that there is little wood left to see. "
                         "Scraps of old notices cling under the heads."),
        ("painted sign", "A sign on an iron bracket, painted with a sheaf of wheat that someone "
                         "repaints before it can fade."),
        ("mounting block", "Three stone steps up to nothing, for getting onto a tall horse. "
                           "The top step is dished in the middle."),
    ],
    "dwarf": [
        ("iron rail", "A handrail of black iron set into the rock with lead, polished bright "
                      "along its length by hands."),
        ("lamp niche", "A niche cut square into the wall, its shutter of pierced iron. The stone "
                       "above it is soot-dark in a perfect fan."),
        ("boot-scraper", "An iron blade set in stone at the threshold, worn to a crescent by "
                         "centuries of boots."),
        ("tally marks", "Rows of chisel strokes cut into the wall at shoulder height, counted in "
                        "fives. Some rows are very old."),
        ("pick rack", "A rack of oak pegs holding picks by their heads, each handle burned with "
                      "its owner's mark."),
        ("carved lintel", "A lintel carved with a line of hammers, each a little different, as if "
                          "every mason who worked here added one."),
    ],
    "elf": [
        ("carved post", "A post of silver-grey wood carved with climbing leaves so fine the "
                        "chisel marks are hard to find."),
        ("rainwater bowl", "A shallow stone bowl set on a plinth, full of rainwater that holds "
                           "the sky in it."),
        ("bough arch", "A living bough trained over the way into an arch, grafted where it "
                       "meets its neighbour."),
        ("lantern", "A lantern of pale horn hung from a branch on a silk cord, its frame "
                    "carved with moths."),
        ("song stone", "A standing stone carved with a few lines of verse in a flowing hand; "
                       "the last line has been left unfinished."),
    ],
    "gnome": [
        ("cog rack", "A rack of unfinished cogs sorted by size, their teeth not yet cut, each "
                     "tagged with a scrap of card."),
        ("quenching barrel", "A barrel of dark water with a film of oil on it and a pair of "
                             "tongs hooked over the rim."),
        ("hand winch", "A hand-cranked winch bolted to a post, its rope run up to a pulley and "
                       "back. Someone has oiled it recently."),
        ("wall clock", "A clock of brass and blackened wood, its face crowded with more hands "
                       "than a clock has any need for."),
        ("speaking tube", "A brass tube coming out of the wall and ending in a flared mouth, "
                          "stoppered with a cork on a chain."),
    ],
    "saurathi": [
        ("mooring post", "A post driven deep into the mud, green with weed to the height of the "
                         "highest water, and scored by ropes above it."),
        ("reed rack", "A rack of split reed drying in bundles, bound with twisted grass."),
        ("sunning stone", "A flat stone, warm to the touch, worn into a shallow hollow by bodies "
                          "lying on it."),
        ("fish trap", "A trap of woven withies with a funnel mouth, propped against the wall to "
                      "dry."),
        ("clutch stone", "A ring of pale stones set in the bank where, the saurathi say, the "
                         "first eggs of this place were laid."),
    ],
    "halfling": [
        ("garden gate", "A gate hung on one good hinge and one bit of twine, painted a cheerful "
                        "blue a long time ago."),
        ("stile", "A stone stile worn smooth in a saddle where every foot in the village has "
                  "crossed it."),
        ("beehive", "A domed straw skep on a stand, with a hood of old sacking over it. It hums."),
        ("apple tree", "An old apple tree, heavy with fruit, propped on one side with a forked "
                       "branch."),
        ("water butt", "A barrel under the end of a downpipe, with a lid and a tin cup hung on a "
                       "nail beside it."),
        ("porch bench", "A bench under a porch, its seat polished by sitting, with a pipe-rack "
                        "carved into one arm."),
    ],
    "volgrin": [
        ("standing stone", "A standing stone worn smooth, with a hollow in one side where "
                           "shoulders have leaned against it."),
        ("picket line", "A rope strung between two posts for tethering horses, the ground under "
                        "it trodden hard."),
        ("hide windbreak", "A windbreak of stitched hides on poles, weighted at the foot with "
                           "stones."),
        ("grey cairn", "A cairn of grey stones as high as a rider's stirrup. Every stone was "
                       "carried here by somebody."),
        ("horse skull", "A horse's skull set on a pole, painted with red ochre around the eyes."),
    ],
    "felari": [
        ("drying net", "A net spread on poles to dry, mended in so many places that the mends "
                       "outnumber the knots."),
        ("basking rock", "A flat rock worn smooth by lying on, the warmest place on the shore."),
        ("fish rack", "A rack of split fish drying in rows, their silver gone to gold."),
        ("coracle", "An upturned coracle of hide over withies, patched with tar, with a paddle "
                    "tucked under it."),
        ("shell cairn", "A cairn of white shells, each one set there by somebody back safe from "
                        "the sea."),
    ],
    "lunari": [
        ("scratching post", "A post scored so deep by claws that it is more groove than wood."),
        ("ash ring", "A ring of cold ashes inside a circle of blackened stones, raked level."),
        ("hide rack", "A frame of lashed poles hung with hides scraped and stretched to dry."),
        ("lookout stump", "A tall stump with footholds cut into it, worn smooth, for seeing over "
                          "the grass."),
        ("moon stone", "A pale flat stone set upright and scratched with the phases of the moon, "
                       "the full one rubbed bright."),
    ],
    "aethari": [
        ("reflecting basin", "A shallow basin of pale stone, the water in it so still it could "
                             "be glass."),
        ("empty plinth", "A plinth with nothing on it, its top smooth and pale where something "
                         "once stood."),
        ("bronze dial", "A sun dial of bronze green with age, its gnomon cast as a heron's "
                        "neck."),
        ("colonnade relief", "A frieze carved along the colonnade: a procession of figures "
                             "carrying jars, their faces worn soft."),
        ("mosaic", "A mosaic set into the floor in blue and white tesserae, a fish chasing its "
                   "own tail."),
    ],
    "valran": [
        ("drystone fold", "A fold of drystone walling, not a stone of it mortared, and not a "
                          "stone of it loose."),
        ("peat stack", "A stack of cut peat under old sacking, built with the cut faces down "
                        "so the wet runs off it."),
        ("byre door", "A byre door tied shut with rope and a knot nobody but its owner could "
                      "undo."),
        ("waymark cairn", "A cairn at the turning, topped with a flat stone pointing the way "
                          "down."),
        ("shepherd's crook", "A crook of hazel hung on two nails by the door, its hook "
                             "polished by use."),
    ],
    "goblin": [
        ("midden", "A midden nobody tends, of bones, shells and broken pots, with a path worn "
                   "through it to somewhere."),
        ("barricade", "A barricade of stolen timber lashed with rope, still with a painted "
                      "number on one plank."),
        ("warning stake", "A stake with something on it that was probably once a warning."),
        ("bone fire pit", "A fire pit full of bones gnawed and cracked for the marrow."),
        ("trophy rack", "A rack of mismatched helmets, none of them goblin-sized, hung from pegs "
                        "like cooking pots."),
    ],
}

#: Things inside a shop, by its trade marker - the same markers `stock` and `people` key on.
INTERIOR = {
    "inn": [
        ("tap board", "A board of chalked tallies behind the counter, rubbed out and rewritten "
                      "so often the slate has gone grey."),
        ("hearth", "A wide hearth with a spit and a blackened kettle hook, the stones around it "
                   "worn by boots warming themselves."),
        ("painting", "A painting of a ship in full sail, darkened by years of smoke so that "
                     "only the white of the sails still shows."),
        ("antlers", "A pair of antlers above the door, hung with a forgotten hat."),
    ],
    "weaponsmith": [
        ("anvil", "An anvil on an oak stump, its face polished and its horn scarred."),
        ("quench trough", "A trough of dark water with a sheen of scale on it and a pair of "
                          "tongs across it."),
        ("blade rack", "A rack of blades waiting for hilts, each tagged with a scrap of leather."),
    ],
    "armourer": [
        ("armour stand", "A wooden stand wearing a mail shirt, a gorget and nothing else, like "
                         "a knight who left in a hurry."),
        ("riveting block", "A block of iron dished with a hundred shallow dents, a rivet set "
                           "still lying in one."),
        ("shield wall", "A wall hung with shield blanks, some painted, most bare."),
    ],
    "general store": [
        ("shelves", "Shelves floor to ceiling, stacked with a little of everything and labelled "
                    "in a careful hand."),
        ("scales", "A pair of brass scales on the counter with a row of weights beside them, "
                   "the smallest no bigger than a fingernail."),
        ("barrel of nails", "A barrel of nails sorted by length into hessian sacks."),
    ],
    "alchemist": [
        ("still", "A copper still with a coiled worm, polished bright where it is touched and "
                  "green where it is not."),
        ("jar shelf", "A shelf of stoppered jars, each labelled in a spidery hand, a few "
                      "labelled only with a warning."),
        ("mortar", "A stone mortar and pestle, stained a dozen colours at once."),
    ],
    "bank": [
        ("strongbox", "A strongbox bound in iron with three locks, each a different make."),
        ("ledger desk", "A tall desk with a ledger chained to it, open at a page of neat "
                        "columns."),
    ],
    "healer": [
        ("herb bundles", "Bundles of herbs hung from the beams to dry, each tied with a "
                         "different coloured thread."),
        ("cot", "A narrow cot with a clean blanket folded at its foot."),
    ],
    "stable": [
        ("tack wall", "A wall of pegs hung with bridles, halters and a saddle waiting for "
                      "stitching."),
        ("feed bin", "A lidded bin of oats with a battered scoop in it."),
        ("stall", "A stall with a name chalked on the board over it, the name rubbed out and "
                  "written again more than once."),
    ],
    "temple": [
        ("altar", "An altar of a single dressed stone, bare but for a folded cloth."),
        ("offering bowl", "A shallow bowl on a stand, half full of small offerings: a coin, a "
                          "button, a child's carved horse."),
        ("candle rack", "A rack of iron cups for candles, crusted with years of old wax."),
    ],
    "market": [
        ("awning", "A striped awning on poles, its colours faded unevenly where the sun has "
                   "reached it."),
        ("trestle", "A trestle table with a brass measuring rule nailed along its edge."),
        ("price slate", "A slate on a nail with prices chalked on it and crossed out and "
                        "chalked again."),
    ],
    "camp": [
        ("fire ring", "A ring of blackened stones with a tripod over it for a pot."),
        ("bedroll", "A bedroll tied with a strap and propped under a lean-to of branches."),
    ],
    "cache": [
        ("marked stone", "A stone with a notch cut in it, turned so the notch points at "
                         "something."),
        ("oilcloth bundle", "A bundle wrapped in oilcloth and tied, tucked well back out of the "
                            "wet."),
    ],
    "shrine": [
        ("niche", "A niche in the stone holding a small figure worn faceless by touching."),
        ("prayer strips", "Strips of cloth tied to a branch above the shrine, a few new, most "
                          "faded to nothing."),
    ],
}

#: The two things that do something, and which shops keep them. Built as their own
#: typeclasses by the export: the mirror shows whoever looks into it, the clock reads the
#: game's time.
SPECIAL = {
    "mirror": ("mirror", "A tall mirror in a frame of carved dark wood, the silvering spotted "
                         "at the corners."),
    "clock": ("clock", "A clock in a tall case of polished wood, its pendulum swinging behind a "
                       "little glass window."),
}
SPECIAL_IN = {"inn": ("mirror", "clock"), "healer": ("mirror",), "general store": ("clock",),
              "bank": ("clock",), "armourer": ("mirror",)}

#: Things beside a road.
ROAD = [
    ("milestone", "A milestone with the distances cut into two faces, the numbers filled with "
                  "lichen."),
    ("wayside trough", "A stone trough fed by a pipe from the bank, overflowing into the "
                       "ditch."),
    ("bench", "A bench cut from a single fallen trunk, the seat worn smooth."),
    ("boundary post", "A post with a different mark cut into each side - two parishes agreeing "
                      "where one ends."),
    ("gibbet post", "An old gibbet post with nothing hanging from it any more but a length of "
                    "rusted chain."),
    ("mile cross", "A small stone cross at the verge, its arms worn stubby."),
    ("fallen oak", "An oak that came down across the verge long ago and was cut back just far "
                   "enough to pass."),
    ("drover's stone", "A flat stone at the verge where drovers rest their packs, scratched "
                       "with initials."),
]

#: Places worth stopping at on a road: its points of interest (F2, at G3's cadence).
LANDMARKS = [
    ("spring", "A spring welling up into a stone basin somebody built around it long ago, "
               "with a cup on a chain."),
    ("ruined watchtower", "The stump of a watchtower, its stair climbing to nothing, the view "
                          "from the top still worth the climb."),
    ("boundary stone", "A great boundary stone as tall as a man, carved on its two faces with "
                       "the marks of two lordships long gone."),
    ("old bridge", "A packhorse bridge of three low arches, too narrow for a cart, kept for "
                   "walkers."),
    ("lone oak", "A single oak far older than anything else in sight, its lower branches "
                 "propped on posts."),
    ("waystone", "A tall stone with a hollow worn in its top where travellers leave a pebble "
                 "for luck."),
    ("roadside shrine", "A shrine in a little stone house of its own, with a bench outside for "
                        "those who stop."),
    ("barrow", "A long green barrow beside the road, grassed over and never ploughed."),
]

#: What a boat ramp has on it. A ramp is added to its town after the town is furnished.
RAMP = ("mooring bollard", "A squat bollard of weathered timber, its waist worn into a deep "
                           "groove by ropes.")

#: How many things a town room gets (F1).
LEAST, MOST = 1, 3

#: How often a road room gets something to look at, and how often a landmark (F2).
ROAD_EVERY = 3
LANDMARK_EVERY = 10


def _thing(key, desc, kind="fixture"):
    return {"key": key, "desc": desc, "kind": kind}


def _street_pool(voice):
    return STREET.get(voice) or STREET["human"]


def furnish_town(area, voice, rng):
    """
    Put one to three things to look at in every room of a town (F1).

    Args:
        area (dict): A named area; interiors carry their `trade`.
        voice (str): The culture's voice, for the street things.
        rng (random.Random): The world's own generator.

    Returns:
        placed (int): How many things were put in.

    Notes:
        **Different things in neighbouring rooms where it can.** Drawn without replacement
        within a room, and the street pool is shuffled once per town and walked, so a street
        does not have a signpost in every room of it.
    """
    placed = 0
    street = list(_street_pool(voice))
    rng.shuffle(street)
    at = 0
    for room in area.get("rooms") or ():
        count = rng.randint(LEAST, MOST)
        if room.get("interior"):
            pool = [_thing(k, d) for k, d in INTERIOR.get(room.get("trade") or "", ())]
            for kind in SPECIAL_IN.get(room.get("trade") or "", ()):
                key, desc = SPECIAL[kind]
                pool.append(_thing(key, desc, kind))
            if not pool:
                pool = [_thing(k, d) for k, d in street]
            rng.shuffle(pool)
            chosen = pool[:count]
        else:
            chosen = []
            while len(chosen) < min(count, len(street)):
                key, desc = street[at % len(street)]
                at += 1
                if all(key != thing["key"] for thing in chosen):
                    chosen.append(_thing(key, desc))
        room["fixtures"] = chosen
        placed += len(chosen)
    return placed


def furnish_road(road, rng):
    """
    Put landmarks and things to look at along a road (F2).

    Returns:
        placed (dict): `things` and `landmarks` put in.

    Notes:
        **Landmarks spaced, not scattered.** One every `LANDMARK_EVERY` rooms and at least
        one, set at even intervals and never at the very ends - the ends are where a road
        says which places it joins, and a landmark there would be a landmark in a town.
    """
    ground = [room for room in road.get("rooms") or () if not room.get("interior")]
    things = list(ROAD)
    rng.shuffle(things)
    marks = list(LANDMARKS)
    rng.shuffle(marks)
    placed = {"things": 0, "landmarks": 0}
    # **Enough by either measure of the road's length.** Spaced along the rooms a player walks,
    # but the area linter counts a road by all its rooms, wayside shrines and camps included,
    # and asks for one per twelve of those. Measured on the first run: 39 rooms walked, 51
    # counted, 3 landmarks placed and 4 asked for.
    counted = len(road.get("rooms") or ())
    wanted = (max(1, -(-len(ground) // LANDMARK_EVERY), counted // 12)
              if len(ground) >= 3 else 0)
    spots = {int(round(len(ground) * (n + 1) / (wanted + 1))) for n in range(wanted)}
    spots = {min(max(1, s), len(ground) - 2) for s in spots} if len(ground) >= 3 else set()
    for index, room in enumerate(ground):
        chosen = []
        if index in spots:
            key, desc = marks[len(spots & set(range(index))) % len(marks)]
            chosen.append(_thing(key, desc, "landmark"))
            room["landmark"] = key
            placed["landmarks"] += 1
        if index % ROAD_EVERY == 0:
            key, desc = things[(index // ROAD_EVERY) % len(things)]
            chosen.append(_thing(key, desc))
        if chosen:
            room["fixtures"] = chosen
            placed["things"] += len(chosen)
    for room in road.get("rooms") or ():
        if room.get("interior"):
            pool = INTERIOR.get(room.get("trade") or "", ())
            if pool:
                key, desc = pool[rng.randrange(len(pool))]
                room["fixtures"] = [_thing(key, desc)]
                placed["things"] += 1
    return placed


def unfurnished(area):
    """Rooms of a town that break F1: ids with fewer than one or more than three things."""
    return [room["id"] for room in area.get("rooms") or ()
            if not LEAST <= len(room.get("fixtures") or ()) <= MOST]
