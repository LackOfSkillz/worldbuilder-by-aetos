"""What the rooms are called and what a player reads when they get there.

**The prose laws are satisfied by construction, not by hoping.** `prose_lint` wants 34 to
79 words in 2 to 4 sentences, and every exit's own noun named in the text so a player can
read a description and know where they can go. A model asked to hit all three freehand
misses often enough that the gate becomes the bottleneck; a template hits them every time
and the flavour comes from the slots. So the skeleton is here and the vocabulary is per
culture - which is also what makes a dwarf hold read differently from an elf village
rather than the same three sentences with the nouns swapped.

**Names are assembled, never invented.** A generator that coins words produces the kind of
apostrophe-strewn nonsense nobody wants, and every coined token is a name somebody has to
check against a trademark. Every part below is an ordinary English word or a plain compound
of two, which is how real places are named - Oxford, Blackpool, Sevenoaks - and which cannot
collide with anybody's invented canon.
"""

#: The compass words a description may use for its exits, and how they read in prose.
WAY = {
    "north": "north", "south": "south", "east": "east", "west": "west",
    "northeast": "north-east", "northwest": "north-west",
    "southeast": "south-east", "southwest": "south-west",
}

#: Per culture: what its places are made of, what stands in them, what a player hears.
#:
#: Keyed by the RACE rather than the culture name, so a dwarf hold and a dwarf mining
#: village share a vocabulary and differ in their size and their purpose - which is the
#: real relationship between them.
VOICE = {
    "dwarf": {
        "detail": (
                   "Chisel marks run in even courses where the rock was squared.",
                   "A gutter cut into the floor carries water away down the slope.",
                   "Names are cut into the stone at shoulder height, some very old.",
                   "The ceiling is low enough that a tall visitor learns to stoop.",
                   "Ore-dust has worked into every joint of the masonry.",
        ),
        "head": ("Delve", "Hold", "Deep", "Gate", "Stair", "Vault", "Forge", "Adit"),
        "tail": ("stone", "iron", "hammer", "anvil", "granite", "slate", "ember", "quarry"),
        "street": ("stair", "gallery", "undercut", "gate road", "cut", "landing"),
        "surface": ("dressed granite", "cut slate", "worn basalt", "packed grit"),
        "fixture": ("an iron rail", "a shuttered lamp niche", "a worn boot-scraper",
                    "a low stone bench", "a bracket of black iron"),
        "sound": ("a hammer rings somewhere below", "water drips in a cistern",
                  "the forge-draught sighs through the cut",
                  "picks knock in the dark beyond"),
        "smell": ("cold stone and coal smoke", "hot iron", "wet rock and lamp oil"),
    },
    "elf": {
        "detail": (
                   "Nothing here was built so much as persuaded into place.",
                   "The path has been walked so long it lies below the roots.",
                   "What was cut here was cut a long time ago and has healed.",
                   "Shade lies deep enough that the ground never fully dries.",
                   "A rope of woven grass marks a boundary somebody respects.",
        ),
        "head": ("Green", "Willow", "Fern", "Quiet", "Long", "Elder", "Silver", "Alder"),
        "tail": ("hollow", "reach", "glade", "water", "wood", "bough", "spring", "walk"),
        "street": ("walk", "green way", "path", "bough road", "leaf way", "run"),
        "surface": ("moss over flagstone", "swept earth", "pale sand", "root-laced ground"),
        "fixture": ("a carved post", "a stone bowl of rainwater", "a low woven fence",
                    "a bough trained into an arch", "a lantern hung from a branch"),
        "sound": ("leaves move overhead", "a thrush works the undergrowth",
                  "water runs somewhere out of sight", "the wood is very quiet"),
        "smell": ("leaf mould and cold water", "resin", "wet bark"),
    },
    "human": {
        "detail": (
                   "Somebody has patched the worst of the ruts with broken brick.",
                   "Shutters stand open above, and washing hangs across the gap.",
                   "The buildings lean together until their eaves nearly touch.",
                   "A gutter runs down the middle and is not much respected.",
                   "Trade has worn a shine into the doorsteps on both sides.",
        ),
        "head": ("Market", "Mill", "Bridge", "Kings", "Old", "Nether", "Upper", "Salt"),
        "tail": ("ford", "bridge", "market", "wharf", "row", "gate", "field", "cross"),
        "street": ("street", "row", "lane", "market", "wharf", "yard"),
        "surface": ("river cobble", "rutted mud", "board and gravel", "flagstone"),
        "fixture": ("a horse trough", "a leaning signpost", "a public well",
                    "a stack of empty crates", "a notice board thick with nails"),
        "sound": ("carts grind past", "a dog barks two doors down",
                  "somebody is arguing about a price", "gulls quarrel on the roofline"),
        "smell": ("woodsmoke and wet wool", "bread and horse", "river mud"),
    },
    "gnome": {
        "detail": (
                   "Half-finished work is left where it stands, and nobody moves it.",
                   "Chalk sums cover a board, argued over and part rubbed out.",
                   "Everything here is bolted down, and most of it is adjustable.",
                   "A drive-belt runs overhead from somewhere to somewhere else.",
                   "The whole place is arranged for somebody a good deal shorter.",
        ),
        "head": ("Cog", "Tinker", "Bellows", "Kettle", "Copper", "Spark", "Whistle"),
        "tail": ("works", "yard", "bench", "hill", "shop", "row", "steading"),
        "street": ("works yard", "bench row", "lane", "shop row", "cut"),
        "surface": ("swept brick", "iron plate over earth", "packed cinders"),
        "fixture": ("a rack of unfinished cogs", "a barrel of quenching water",
                    "a bench under an awning", "a hand-cranked winch"),
        "sound": ("a treadle clacks behind a shutter", "something hisses and is scolded",
                  "small hammers tap out of time", "a kettle shrieks and is silenced"),
        "smell": ("hot brass and lamp oil", "solder and steam", "sawdust"),
    },
    "saurathi": {
        "detail": (
                   "The boards give underfoot and settle again once you pass.",
                   "Green water shows between the planks, slow and untroubled.",
                   "Everything here is built to be rebuilt after the next flood.",
                   "The stand is raised on piles well above the usual water.",
                   "Reed grows to head height on both sides of the way.",
        ),
        "head": ("Reed", "Warm", "Mud", "Basking", "Still", "Green", "Long"),
        "tail": ("bank", "shallow", "mire", "trace", "shelf", "water", "stand"),
        "street": ("board walk", "trace", "reed way", "causeway", "landing"),
        "surface": ("split boards over water", "trodden reed", "warm dry silt"),
        "fixture": ("a mooring post green with weed", "a rack of drying reed",
                    "a flat sunning stone", "a fish trap propped to dry"),
        "sound": ("frogs start and stop", "water slaps the boards",
                  "something heavy slides into the water", "insects drone over the reeds"),
        "smell": ("warm mud and green water", "drying fish", "reed smoke"),
    },
    "halfling": {
        "detail": (
                   "Every hedge here is laid and kept, and none of it is straight.",
                   "The doors are round-topped and set low into the bank.",
                   "A well-tended garden runs right up to the edge of the path.",
                   "Ground that is not walked on is growing something edible.",
                   "Chimneys smoke from under the turf on both sides.",
        ),
        "head": ("Barley", "Apple", "Honey", "Low", "Meadow", "Brook", "Butter"),
        "tail": ("field", "hollow", "bottom", "acre", "lane", "orchard", "bank"),
        "street": ("lane", "field path", "orchard walk", "green", "cart track"),
        "surface": ("beaten earth", "grass worn to soil", "straw over mud"),
        "fixture": ("a gate hung on one hinge", "a stone stile", "a beehive on a stand",
                    "a laden apple tree", "a water butt under a downpipe"),
        "sound": ("bees work the hedge", "a cow complains in the next field",
                  "somebody is singing badly indoors"),
        "smell": ("cut hay", "baking and woodsmoke", "apples going over"),
    },
    "volgrin": {
        "detail": (
                   "The ground is open in every direction and hides nothing.",
                   "Wheel ruts run away straight until the distance takes them.",
                   "Everything here can be struck and moved before evening.",
                   "The horizon is a long way off and there is a lot of sky.",
                   "Old fire-scars mark where the camp stood in other years.",
        ),
        "head": ("Broad", "Wind", "Open", "Far", "Standing", "Grass"),
        "tail": ("steading", "run", "reach", "camp", "moot", "ground"),
        "street": ("run", "open way", "drove road", "moot ground"),
        "surface": ("cropped turf", "dry grass and dust", "hoof-cut earth"),
        "fixture": ("a standing stone worn smooth", "a picket line",
                    "a windbreak of hides on poles", "a cairn of grey stones"),
        "sound": ("wind moves the grass in long waves", "horses shift on the picket",
                  "a hawk calls somewhere very high"),
        "smell": ("dust and dry grass", "horse and leather", "rain that has not arrived"),
    },
    "felari": {
        "detail": (
                   "Every flat surface is being used to dry something.",
                   "The stone holds the heat of the day well into the evening.",
                   "Boats are drawn up above the tide line and turned over.",
                   "Steps have been cut where the rock was too steep to walk.",
                   "Nothing here is far from the sound of water.",
        ),
        "head": ("Net", "Tide", "Sun", "Warm", "Salt", "Low"),
        "tail": ("landing", "rock", "quay", "step", "cove", "walk"),
        "street": ("landing", "step", "quay", "walk"),
        "surface": ("sun-warmed stone", "sand over rock", "salted boards"),
        "fixture": ("a net spread to dry", "a flat rock worn smooth by lying on",
                    "a rack of split fish", "an upturned coracle"),
        "sound": ("water works under the stones", "a net is shaken out",
                  "somebody is asleep and snoring softly"),
        "smell": ("salt and drying fish", "warm stone", "tar"),
    },
    "lunari": {
        "detail": (
                   "Trails come in from three directions and leave by one.",
                   "Everything is placed so it can be seen from the high ground.",
                   "The undergrowth has been cut back to open the sight lines.",
                   "Sleeping places are dug in close together under the trees.",
                   "There is no clutter here and nothing left lying about.",
        ),
        "head": ("Moon", "Long", "Grey", "Night", "Pack", "Cold"),
        "tail": ("holt", "run", "ridge", "howl", "hollow", "watch"),
        "street": ("run", "ridge path", "hollow way", "watch line"),
        "surface": ("needle-strewn earth", "frost-cracked stone", "trodden pine litter"),
        "fixture": ("a scratching post scored deep", "a ring of cold ashes",
                    "a rack of drying hides", "a lookout stump"),
        "sound": ("the pines tick in the wind", "something answers a long way off",
                  "the camp is watchful and quiet"),
        "smell": ("pine resin and cold ash", "wet fur", "snow coming"),
    },
    "aethari": {
        "detail": (
                   "The stonework is joined so closely the seams are hard to find.",
                   "Everything is set square, and the proportion is deliberate.",
                   "Age has done nothing to this place except soften its edges.",
                   "There is more space here than the traffic requires.",
                   "What decoration there is has been carved rather than added.",
        ),
        "head": ("High", "Pale", "Quiet", "Grey", "Sea", "Far"),
        "tail": ("terrace", "quay", "hall", "prospect", "stair", "gate"),
        "street": ("terrace", "colonnade", "stair", "prospect", "quay"),
        "surface": ("pale dressed stone", "swept marble", "fitted flagstone"),
        "fixture": ("a shallow reflecting basin", "a plinth with nothing on it",
                    "a rail of pale stone", "a bronze dial green with age"),
        "sound": ("the sea is a long way below", "a bell is struck once",
                  "voices carry and are not raised"),
        "smell": ("salt and cold stone", "cut lavender", "clean air"),
    },
    "valran": {
        "detail": (
                   "Everything is built low and heavy against the weather.",
                   "The walls are drystone, and they have been rebuilt often.",
                   "Water finds its way through here whenever it rains.",
                   "Turf has been cut from the bank and stacked to dry.",
                   "The track is stone where it is not simply mud.",
        ),
        "head": ("High", "Stone", "Winter", "Cairn", "Rough", "Far"),
        "tail": ("steading", "fold", "shieling", "howe", "burn", "brae"),
        "street": ("track", "fold path", "burnside", "brae"),
        "surface": ("wet turf", "loose scree", "peat and stone"),
        "fixture": ("a drystone fold", "a peat stack under sacking",
                    "a byre with its door tied shut", "a cairn at the turning"),
        "sound": ("the burn is loud after rain", "sheep complain on the hill",
                  "wind worries at the thatch"),
        "smell": ("peat smoke", "wet wool", "cold rain"),
    },
    "goblin": {
        "detail": (
                   "Nothing here was made; all of it was taken and made to serve.",
                   "The ground has been fought over often enough to show it.",
                   "Shelters are propped against each other and none stand alone.",
                   "Everything of value is either buried or being sat on.",
                   "Somebody has scratched a tally into the rock and stopped.",
        ),
        "head": ("Scree", "Rot", "Crook", "Sharp", "Bone", "Low"),
        "tail": ("scratch", "midden", "warren", "hole", "camp", "run"),
        "street": ("run", "scratch", "crawl", "midden path"),
        "surface": ("churned filth", "loose scree", "bare trodden dirt"),
        "fixture": ("a midden nobody tends", "a barricade of stolen timber",
                    "a stake with something on it", "a fire pit full of bones"),
        "sound": ("something is being fought over", "a lookout jeers and is ignored",
                  "there is a great deal of shouting"),
        "smell": ("smoke and spoiled meat", "wet fur and worse", "old blood"),
    },
    "wild": {
        "detail": (
                   "Nothing here has been built, cut, or tended by anybody.",
                   "The trail was made by animals and is kept open by use.",
                   "Cover comes right to the edge of the path on both sides.",
                   "Whatever passes through here does so without hurrying.",
                   "There are tracks in the soft ground and they are not old.",
        ),
        "head": ("Bare", "Thorn", "Grey", "Deep", "Rough", "Cold", "Far"),
        "tail": ("hunt", "ground", "waste", "thicket", "hollow", "scrub", "reach"),
        "street": ("game trail", "deer path", "gully", "thicket way", "clearing"),
        "surface": ("trampled bracken", "loose leaf litter", "root and stone"),
        "fixture": ("a game trail worn deep", "bones picked clean",
                    "a thorn brake nothing has forced", "a wallow churned to mud"),
        "sound": ("nothing moves, and that is worse", "something large shifts its weight",
                  "birds went quiet a moment ago"),
        "smell": ("rot and wet leaf", "musk", "cold earth"),
    },
}

#: When a culture has no voice of its own, it borrows this one.
DEFAULT_VOICE = VOICE["human"]

#: The rooms every settlement has, in the order the laws expect to find them.
#:
#: **A shop is a room reached from the street through a door** - the building laws' Part 5,
#: a MUST. So these are room names, not props: the inn is somewhere a player stands.
TRADES = (
    ("inn", "the %s Inn"), ("weaponsmith", "the weaponsmith"),
    ("armourer", "the armourer"), ("general store", "the general store"),
    ("alchemist", "the alchemist"), ("bank", "the counting house"),
    ("healer", "the healer's rooms"), ("stable", "the stables"),
    ("temple", "the shrine"), ("market", "the market stalls"),
)

#: Which trades stand in a place with nobody settled in it.
#:
#: A hunting ground has no armourer. Giving one to every area was how "village" and
#: "wilderness" ended up the same place with different scenery.
WILD_TRADES = (
    ("camp", "a hunter's camp"), ("cache", "a cache under stones"),
    ("shrine", "a wayside shrine"),
)


def voice_for(race):
    """The vocabulary a culture speaks in."""
    return VOICE.get(race or "", DEFAULT_VOICE)


def place_name(race, rng):
    """A settlement name: two ordinary words, the way real places are named."""
    voice = voice_for(race)
    return "%s%s" % (rng.choice(voice["head"]), rng.choice(voice["tail"]))


def room_names(race, count, rng, settled=True):
    """
    Names for every room in an area: street rooms, with the trades dealt among them.

    Args:
        race (str): Whose place this is.
        count (int): How many rooms.
        rng (random.Random): The world's own generator.
        settled (bool): False for a hunting ground or a wild camp, which has no shops.

    Returns:
        names (list): One per room.

    Notes:
        The building laws want roughly one point of interest per three street rooms, so the
        trades are dealt out at that rate rather than clumped at the start - a settlement
        whose first ten rooms are all shops and whose last forty are all street reads as a
        list, because it is one.
    """
    voice = voice_for(race)
    streets = voice["street"]
    trades = list(TRADES if settled else WILD_TRADES)
    rng.shuffle(trades)
    names = []
    trade_at = 0
    for index in range(count):
        if index and index % 3 == 0 and trade_at < len(trades):
            _, template = trades[trade_at]
            trade_at += 1
            names.append(template % place_name(race, rng) if "%s" in template else template)
        else:
            names.append("%s %s" % (rng.choice(("a", "the")), rng.choice(streets)))
    return names


def _sentence_case(text):
    return text[0].upper() + text[1:] if text else text


def ways_sentence(exits):
    """The sentence that names every way out. See `describe`."""
    ways = [WAY.get(name, name) for name in exits]
    if not ways:
        return "There is no way on from here but the way you came."
    if len(ways) == 1:
        return "The only way on lies %s." % ways[0]
    return "Ways lead %s and %s." % (", ".join(ways[:-1]), ways[-1])


def describe(exits, race, rng, band=(34, 79)):
    """
    A room description that satisfies the prose laws by construction.

    Args:
        exits (list): The exit names leading out, e.g. `["north", "west"]`.
        race (str): Whose place this is, choosing the vocabulary.
        rng (random.Random): The world's own generator.
        band (tuple): The word count the laws allow.

    Returns:
        text (str): Three or four sentences, inside the word band, naming every way out.

    Notes:
        **Every exit is named because a player cannot see.** The laws require it and the
        reason is not tidiness: a description that mentions two of three ways out is a
        description that hides a door, and hidden doors are found by typing every compass
        point in every room, which is the least interesting thing a game can ask for.

        **Only permanently true things.** No weather, no light, no birds - those belong in
        an ambient message, because a description is read at noon and at midnight and in
        the rain, and a room that is always sunny is a room nobody believes.
    """
    voice = voice_for(race)
    opening = "%s runs underfoot, and %s stands against the wall." % (
        _sentence_case(rng.choice(voice["surface"])), rng.choice(voice["fixture"]))
    middle = "%s; the air is %s." % (
        _sentence_case(rng.choice(voice["sound"])), rng.choice(voice["smell"]))
    going = ways_sentence(exits)

    # **The third sentence is the culture's own, not a filler.** The first version padded
    # every short description with one fixed line, and the other three sentences come to
    # about thirty words - so that line appeared in essentially every room in the world.
    # Five thousand rooms sharing a sentence is worse than five thousand sharing a shape.
    details = list(voice.get("detail") or ("",))
    rng.shuffle(details)
    detail = details[0]
    text = " ".join(part for part in (opening, middle, detail, going) if part)
    # **Short is a refusal, so it is fixed here rather than reported there.** One area in a
    # hundred came out at thirty-three words against a floor of thirty-four and was thrown
    # away whole - fifty rooms discarded over one word. A second detail sentence costs
    # nothing and is the culture's own voice rather than filler.
    if len(text.split()) < band[0] and len(details) > 1:
        text = " ".join(part for part in (opening, middle, detail, details[1], going)
                        if part)
    # Trimmed against the same band the gate measures, so an area is never refused for a
    # sentence this file could have made the right length.
    if len(text.split()) > band[1]:
        text = " ".join([opening, middle, going])
    if len(text.split()) > band[1]:
        text = " ".join([opening, going])
    return text


def name_and_describe(area, race, rng, settled=True):
    """
    Fill an area's rooms with names and prose, in place, and name the area itself.

    Returns:
        area (dict): The same object, with `display_name` and every room's `key` and `desc`.
    """
    rooms = area.get("rooms") or []
    exits_by_room = {}
    for exit_ in area.get("exits") or ():
        exits_by_room.setdefault(exit_["source"], []).append(exit_["name"])
    for room, name in zip(rooms, room_names(race, len(rooms), rng, settled=settled)):
        room["key"] = name
        room["desc"] = describe(exits_by_room.get(room["id"], []), race, rng)
    if not area.get("display_name"):
        area["display_name"] = place_name(race, rng)
    return area
