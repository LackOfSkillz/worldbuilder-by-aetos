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
        "beside": "at the water's edge",
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
        "beside": "against the hedge",
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
        "beside": "in the open",
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
        "beside": "above the tide line",
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
        "beside": "under the trees",
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
        "beside": "against the weather",
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
    "road": {
        "detail": (
                   "The verge is cut back a spear's length on either side.",
                   "Cart ruts have worn down to the stone beneath.",
                   "A milestone leans here, its face weathered past reading.",
                   "The way was made by use and only later by anybody's hand.",
                   "Somebody has piled stones at the turning as a mark.",
        ),
        "head": ("Long", "Old", "High", "Stone", "Green", "Winter"),
        "tail": ("road", "way", "track", "drove", "reach", "mile"),
        "street": ("road", "way", "track", "verge", "crossing", "milestone"),
        "surface": ("rutted track", "packed gravel", "worn stone", "grass grown over ruts"),
        "fixture": ("a milestone", "a horse trough beside the way",
                    "a cairn at the turning", "a bench cut from a fallen trunk",
                    "a wayside shrine no bigger than a kennel"),
        "sound": ("the road is quiet in both directions",
                  "wind moves in the trees along the verge",
                  "something is coming, a long way off"),
        "smell": ("dust and horse", "wet grass", "clean and cold"),
        "beside": "beside the way",
    },
    "game": {
        "detail": (
                   "Droppings and slot marks show what uses this ground.",
                   "A salt lick has been worn hollow by patient tongues.",
                   "The grass is cropped short in a wide, careful circle.",
                   "Feathers lie scattered where a covey was flushed.",
                   "A hide of woven branches faces down the clearing.",
        ),
        "head": ("Deer", "Fallow", "Covert", "Fowl", "Hare", "Quail", "Elk"),
        "tail": ("covert", "moor", "meadow", "marsh", "run", "wood", "lease"),
        "street": ("game trail", "ride", "covert edge", "meadow", "flight line"),
        "surface": ("cropped turf", "leaf litter and moss", "trodden bracken"),
        "fixture": ("a hide of woven branches", "a salt lick worn hollow",
                    "a feed trough somebody keeps filled", "a stile over the fence"),
        "sound": ("something moves off through the undergrowth, unhurried",
                  "a cock pheasant calls twice and stops",
                  "duck get up off the water somewhere ahead"),
        "smell": ("crushed grass", "leaf mould", "cold water and reed"),
        "beside": "at the edge of the ride",
    },
    "game_wood": {
        "detail": (
                   "Slot marks cut deep where deer cross to the water.",
                   "A hide of woven branches faces down the ride.",
                   "Bark is frayed at knee height where a buck has been fraying.",
                   "Feathers lie scattered where a covey was flushed.",
                   "Beech mast lies thick, and something has been turning it over.",
        ),
        "head": ("Fallow", "Roe", "Covert", "Hazel", "Buck", "Hart"),
        "tail": ("covert", "wood", "ride", "lease", "chase", "holt"),
        "street": ("ride", "game trail", "covert edge", "beat", "deer path"),
        "surface": ("leaf litter and moss", "trodden bracken", "beech mast"),
        "fixture": ("a hide of woven branches", "a salt lick worn hollow",
                    "a high seat lashed into a fork", "a stile over the deer fence"),
        "sound": ("a roe barks once, away in the thicket",
                  "a cock pheasant clatters up and glides off",
                  "something heavy moves off through the bracken, unhurried",
                  "a jay screams a warning further down the ride"),
        "smell": ("leaf mould and crushed fern", "wet bark", "fox"),
        "beside": "at the edge of the ride",
    },
    "game_marsh": {
        "detail": (
                   "Duck have been feeding here; the weed is torn and floating.",
                   "A punt lies drawn up in the reeds, half full of rain.",
                   "Otter have slid the bank into a smooth chute.",
                   "Snipe workings pit the soft ground in hundreds.",
                   "A line of decoys is stacked under sacking.",
        ),
        "head": ("Fowl", "Teal", "Heron", "Wigeon", "Reed", "Otter"),
        "tail": ("marsh", "flight", "fen", "water", "lead", "shallow"),
        "street": ("flight line", "reed cut", "bank", "causeway", "lead"),
        "surface": ("soft black silt", "trodden reed", "wet peat"),
        "fixture": ("a punt drawn up in the reeds", "a stack of decoys under sacking",
                    "a hide sunk into the bank", "a withy trap staked in the shallows"),
        "sound": ("duck get up off the water somewhere ahead",
                  "a heron lifts, complaining, and beats away",
                  "snipe zigzag up out of the rushes",
                  "frogs stop all at once, and then start again"),
        "smell": ("cold water and reed", "silt", "wet feather"),
        "beside": "at the water's edge",
    },
    "game_moor": {
        "detail": (
                   "Grouse butts are dug in a line along the contour.",
                   "The heather has been burned in strips to bring on new growth.",
                   "Hare runs cut white lines through the older heather.",
                   "A ram has rubbed the peat hag smooth against its horn.",
                   "Droppings show where the herd came down off the tops.",
        ),
        "head": ("Grouse", "Hare", "Ram", "Whin", "Heather", "Fell"),
        "tail": ("moor", "fell", "lease", "tops", "brae", "ground"),
        "street": ("sheep track", "butt line", "peat road", "ridge", "gully"),
        "surface": ("heather and peat", "cropped turf and stone", "wet moss"),
        "fixture": ("a grouse butt of turf and stone", "a cairn on the skyline",
                    "a salt lick set on a flat rock", "a shooting stick left leaning"),
        "sound": ("a grouse goes off low and fast, complaining",
                  "a ram stands off on the skyline and watches",
                  "hare break in three directions at once",
                  "wind is the only thing moving up here"),
        "smell": ("peat and bruised heather", "cold rain", "sheep"),
        "beside": "on the open ground",
    },
    "wild_marsh": {
        "detail": (
                   "Something long slid off the bank as you came up.",
                   "Bones of something large lie half in the water.",
                   "The reeds are flattened in a wide, deliberate trail.",
                   "Nothing sings here, and the quiet is not restful.",
                   "A slide worn into the mud is wider than a man.",
        ),
        "head": ("Black", "Drowned", "Fever", "Still", "Rot", "Deep"),
        "tail": ("mire", "water", "slough", "bank", "shallow", "hole"),
        "street": ("board walk", "reed way", "mud bank", "causeway"),
        "surface": ("black sucking mud", "rotten boards", "matted reed"),
        "fixture": ("a slide worn into the bank", "a ribcage picked clean",
                    "a nest mound of rotting weed", "a drowned tree stripped white"),
        "sound": ("something long slides into the water behind you",
                  "the frogs have stopped, all of them",
                  "water moves against the current, and keeps moving"),
        "smell": ("rot and standing water", "old meat", "fever"),
        "beside": "at the water's edge",
    },
    "wild_upland": {
        "detail": (
                   "Scat on the rock is fresh and full of hair.",
                   "Something has been sharpening its claws on the scree.",
                   "A kill was dragged up here and finished at leisure.",
                   "The ravens are waiting, which means something else is too.",
                   "A cave mouth breathes cold air out of the hillside.",
        ),
        "head": ("Grey", "Bare", "Wolf", "Bone", "Cold", "Crag"),
        "tail": ("scree", "crag", "waste", "reach", "howl", "fell"),
        "street": ("scree run", "goat track", "gully", "ridge line"),
        "surface": ("loose scree", "frost-split rock", "bare stone"),
        "fixture": ("a cave mouth breathing cold air", "a kill dragged into cover",
                    "a slab scored with claw marks", "a cairn nobody built for luck"),
        "sound": ("something answers from higher up, and it is not an echo",
                  "stones come down the scree that nothing kicked",
                  "the ravens have gone quiet"),
        "smell": ("cold stone and musk", "old blood", "wet fur"),
        "beside": "against the rock",
    },
    "wild_shore": {
        "detail": (
                   "The tideline is a mess of picked shells and claws.",
                   "Something dragged itself up the sand and back again.",
                   "Wreck timber has been gnawed at the waterline.",
                   "Gulls will not settle on this stretch of beach.",
                   "A slick of something dark comes and goes with the water.",
        ),
        "head": ("Wreck", "Drowned", "Grey", "Salt", "Shark", "Bone"),
        "tail": ("strand", "shore", "cove", "reach", "skerry", "bar"),
        "street": ("tideline", "shingle", "rock shelf", "strand"),
        "surface": ("wet shingle", "weed-slick rock", "coarse grey sand"),
        "fixture": ("a wreck timber gnawed at the waterline",
                    "a drag mark up the sand and back", "a midden of picked shells",
                    "a rock shelf scoured bare"),
        "sound": ("something breaks the surface further out and is gone",
                  "the gulls will not come down to this stretch",
                  "water sucks back off the shingle and takes something with it"),
        "smell": ("salt and rotting weed", "old fish", "cold brine"),
        "beside": "above the tideline",
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
        "fixture": ("a game trail worn deep", "a ribcage picked clean",
                    "a thorn brake nothing has forced", "a wallow churned to mud"),
        "sound": ("nothing moves, and that is worse", "something large shifts its weight",
                  "birds went quiet a moment ago"),
        "smell": ("rot and wet leaf", "musk", "cold earth"),
        "beside": "at the edge of the trail",
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
    # **Not every room has a wall.** "A cairn at the turning stands against the wall" is
    # what a template written for streets says when it is handed a road, and it is the kind
    # of wrong that a word-count band cannot see. Outdoor voices say where their fixture
    # stands in their own terms.
    fixture = rng.choice(voice["fixture"])
    opening = "%s runs underfoot, and %s stands %s." % (
        _sentence_case(rng.choice(voice["surface"])), fixture,
        voice.get("beside", "against the wall"))
    middle = "%s; the air is %s." % (
        _sentence_case(rng.choice(voice["sound"])), rng.choice(voice["smell"]))
    going = ways_sentence(exits)

    # **The third sentence is the culture's own, not a filler.** The first version padded
    # every short description with one fixed line, and the other three sentences come to
    # about thirty words - so that line appeared in essentially every room in the world.
    # Five thousand rooms sharing a sentence is worse than five thousand sharing a shape.
    details = list(voice.get("detail") or ("",))
    rng.shuffle(details)
    # **Do not name the same object twice in three sentences.** The fixture and the detail
    # are drawn from lists that describe the same country, so both can land on the hide, or
    # both on the salt lick - and "a hide of woven branches stands at the edge of the ride.
    # A hide of woven branches faces down the ride" reads as a stutter rather than a room.
    keyword = " ".join(fixture.split()[1:3]).rstrip(",.")
    detail = next((line for line in details if keyword and keyword not in line.lower()),
                  details[0])
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


def retell_exits(area):
    """
    Rewrite each room's last sentence so it names the exits the room actually has.

    Notes:
        **Roads are laid after the prose is written, and that silently breaks a law.** A
        description names every way out; a settlement room that later gains a road exit is
        then a room whose prose hides a door - and it is hidden in exactly the way the law
        exists to prevent. Measured on a hundred-area run: seven hundred and thirty-two
        exits went unnamed once the roads were in.

        Only the closing sentence is replaced, because only the closing sentence is about
        the ways out. A room whose description this module did not write is left alone: it
        is not this function's prose to rewrite.
    """
    by_room = {}
    for exit_ in area.get("exits") or ():
        by_room.setdefault(exit_["source"], []).append(exit_["name"])
    for room in area.get("rooms") or ():
        text = room.get("desc")
        if not text:
            continue
        head, _, tail = text.rpartition(". ")
        if not head or not _is_ways_sentence(tail):
            continue
        room["desc"] = "%s. %s" % (head, ways_sentence(by_room.get(room["id"], [])))
    return area


def _is_ways_sentence(text):
    """Whether a sentence is one this module wrote about the ways out."""
    return text.startswith(("Ways lead ", "The only way on lies ", "There is no way on"))
