"""What a shop actually has on its shelves.

**A shop with no stock is a room with a sign on it.** The building laws make a shop a room
you walk into; this is what is inside when you get there, and it is what turns "eight shops"
in a tally into something a player can spend money in.

**Period-appropriate by construction, not by filtering.** Every ware below is a thing a
pre-industrial town actually sold, so the anachronism lint has nothing to catch - the same
argument the room descriptions make. A generator that invented stock from a general
vocabulary would put lamp oil and a car battery on the same shelf and only one of them would
be caught by eye.

**Counted, never invented.** The tally says how many items a world holds by adding up what
is on the shelves, so the number moves when the stock does.
"""

import re

#: What each kind of shop sells. Keyed by the marker `generate.TRADE_MARKERS` looks for, so
#: a room named "the weaponsmith" is stocked as one without a second lookup table.
WARES = {
    "inn": ("a bowl of barley broth", "a wedge of hard cheese", "brown bread",
            "a mug of small beer", "a jug of cider", "a bed for the night",
            "a plate of salt pork", "stabling for one horse", "a tallow candle",
            "a bundle of rushes for the floor"),
    "weaponsmith": ("a short sword", "a hunting spear", "a hand axe", "a war hammer",
                    "a bundle of arrows", "a yew bow", "a dagger in a plain sheath",
                    "a sword blank, unhilted", "a whetstone", "a spear ferrule",
                    "a boar spear with a crossbar", "a sling and a pouch of stones"),
    "armourer": ("a padded gambeson", "a mail shirt", "an iron cap", "a wooden shield",
                 "a pair of leather bracers", "a set of splinted greaves",
                 "a mail coif", "a buckler", "a roll of harness leather",
                 "a bag of mail rings"),
    "general store": ("a coil of hemp rope", "a tinderbox", "an iron cook pot",
                      "a woollen blanket", "a sack of flour", "a hank of twine",
                      "a horn spoon", "a leather bucket", "a bar of tallow soap",
                      "a bundle of tallow candles", "a fishing line and hooks",
                      "a folding knife"),
    "alchemist": ("a phial of thornwater", "a jar of leeches", "a pot of wound salve",
                  "dried feverfew", "a twist of willow bark", "a stoppered flask of spirit",
                  "powdered chalk", "a pot of beeswax", "oil of juniper",
                  "a phial of something the label does not name"),
    "counting house": ("a set of brass scales", "a tally stick", "a sealed letter of credit",
                       "a purse of clipped coin", "a ledger, half filled",
                       "a box of lead seals", "a moneychanger's touchstone"),
    "healer": ("clean linen bandage", "a splint of green wood", "a pot of bruise salve",
               "a bone needle and gut thread", "dried yarrow", "a draught for a fever",
               "a crutch cut to fit", "a jar of honey for dressing wounds"),
    "stables": ("a saddle blanket", "a bridle", "a bag of oats", "a hoof pick",
                "a curry comb", "a length of picket line", "a saddle, worn but sound",
                "a set of horseshoes"),
    "shrine": ("a wax votive", "a bundle of dried herbs", "a clay lamp",
               "a token cut from bone", "a length of prayer cord", "a phial of clean water"),
    "market stalls": ("a basket of turnips", "a string of onions", "a wheel of soft cheese",
                      "a brace of rabbits", "a bolt of undyed wool", "a crock of butter",
                      "a sack of dried peas", "a hen in a wicker cage",
                      "a bundle of firewood", "salted river fish"),
    "camp": ("a strip of dried meat", "a waterskin", "a bundle of snare wire",
             "a spare bowstring", "a flint and steel"),
    "cache": ("a sealed crock of grain", "a coil of spare line", "a wrapped spearhead",
              "a bundle of dry tinder"),
}

#: How many different wares a shop of each size carries.
#:
#: A village store is not a city market with fewer customers: it stocks less, and the
#: difference is what makes a city worth walking to.
DEPTH = {"city": (6, 10), "seat": (5, 9), "town": (5, 8), "village": (4, 7),
         "hamlet": (3, 5), "camp": (2, 4), "road": (2, 3)}


#: The trades whose goods somebody would keep.
#:
#: **A souvenir is a thing that lasts.** You carry a dagger home from the badlands; you do
#: not carry home a bowl of barley broth, and dressing one up with a place name makes a
#: joke of both. So the local character goes on what a traveller would still own a year
#: later, and the inn goes on selling ordinary beer.
KEEPSAKES = ("weaponsmith", "armourer", "general store", "stables", "shrine")

#: What a place's own work looks like, by the culture that does it.
#:
#: **One look per place, not one per item.** Goods from a town read as being from that town
#: because they share a hand - a colour its dyers use, a material its country provides, a
#: mark its makers cut. Drawing those per item would make a shelf of unrelated oddments,
#: which is the opposite of a souvenir.
LOOKS = {
    "human": {"colour": ("green", "russet", "dun", "blue-black", "oxblood"),
              "material": ("oak", "horn", "brass", "boiled leather", "ash")},
    "dwarf": {"colour": ("iron-grey", "black", "deep red", "smoke-blue"),
              "material": ("iron", "bronze", "boar leather", "pewter", "black oak")},
    "elf": {"colour": ("moss-green", "silver", "pale gold", "birch-white"),
            "material": ("yew", "birch", "silk-wound", "green leather", "antler")},
    "gnome": {"colour": ("brass", "lacquer-red", "oiled black", "verdigris"),
              "material": ("brass", "tin", "spring steel", "waxed cord", "hardwood")},
    "saurathi": {"colour": ("reed-green", "mud-brown", "pale grey", "river-black"),
                 "material": ("fish leather", "river cane", "shell", "cypress")},
    "halfling": {"colour": ("wheat-gold", "apple-red", "hedge-green", "cream"),
                 "material": ("willow", "beech", "soft leather", "hazel")},
    "volgrin": {"colour": ("dust-red", "roan", "sun-bleached", "smoke-grey"),
                "material": ("horsehide", "horn", "rawhide", "bone")},
    "felari": {"colour": ("sea-green", "salt-white", "coral", "deep blue"),
               "material": ("driftwood", "sharkskin", "shell", "tarred cord", "copper")},
    "lunari": {"colour": ("slate", "moon-pale", "storm-grey", "heather"),
               "material": ("horn", "hill oak", "goat leather", "blackthorn")},
    "aethari": {"colour": ("white", "sky-blue", "gilt", "pale rose"),
                "material": ("cedar", "silver-inlaid", "fine leather", "olivewood")},
    "valran": {"colour": ("peat-brown", "fern-green", "grey", "bracken-red"),
               "material": ("bog oak", "sheepskin", "horn", "hill birch")},
    "goblin": {"colour": ("scavenged red", "tarnished", "mismatched", "soot-black"),
               "material": ("scrap iron", "cord", "hide", "found tin", "bone")},
}

#: Materials a ware may already name for itself. See `souvenir`.
MATERIALS = ("clay", "iron", "brass", "tin", "wooden", "oak", "leather", "bone", "horn",
             "wool", "woollen", "silver", "copper", "steel", "stone", "glass", "tallow",
             "wax", "hemp", "linen", "gut", "yew", "willow", "lead", "mail")

#: The look a place falls back on when its culture has none written down.
PLAIN_LOOK = {"colour": ("plain", "undyed", "weathered"),
              "material": ("oak", "horn", "leather", "iron")}


def look_of(race):
    """The colours and materials a people's work is known by."""
    return LOOKS.get(race or "", PLAIN_LOOK)


def signature(race, rng):
    """
    One place's own look: a colour and a material its makers use.

    Returns:
        look (dict): `colour` and `material`, chosen once for the whole settlement.
    """
    made = look_of(race)
    return {"colour": rng.choice(made["colour"]), "material": rng.choice(made["material"])}


#: What part of a trade's goods a local material actually makes.
#:
#: **A material has to be the part it could be.** Pushed in front of the whole item it
#: writes "a spider silk short sword" and "a marble dust cook pot", which are not things.
#: A hilt, a binding, a handle - those a place's own leather and horn and oak really do make,
#: and it is what somebody means when they say a dagger is from the badlands.
#:
#: **By the thing, not by the shop.** Each trade once had a single part and stamped it on
#: everything it sold, so a weaponsmith sold "an olivewood-hilted spear ferrule", "an
#: olivewood-hilted sword blank, unhilted" and "an olivewood-hilted hand axe" - a ferrule and
#: a blank have no hilt, and an axe has a haft. A part now belongs to the goods that have one.
PARTS = {
    "weaponsmith": {"sword": "hilted", "dagger": "hilted", "knife": "hilted",
                    "axe": "hafted", "hammer": "hafted", "spear": "hafted"},
    "armourer": {"shield": "bound", "buckler": "bound"},
    "general store": {"knife": "handled", "bucket": "handled", "spoon": "handled"},
    "stables": {"bridle": "stitched", "blanket": "stitched", "saddle": "stitched"},
    "shrine": {"token": "carved"},
}

#: What a part may be made of, by the words a material is named with. **And by the
#: material, not only the thing:** a place's one material has to be able to make the part -
#: "olivewood-hilted" is a grip, "olivewood-stitched" is nonsense.
_WOOD = ("oak", "ash", "yew", "birch", "cedar", "olivewood", "hazel", "willow", "beech",
         "cypress", "hardwood", "blackthorn", "driftwood", "cane", "wood")
_SOFT = ("leather", "hide", "skin", "cord", "silk", "gut", "wool", "linen")
_HARD = ("horn", "bone", "antler", "shell", "ivory", "stone")
_METAL = ("iron", "bronze", "brass", "pewter", "tin", "copper", "steel", "silver", "gold")
#: The woods, by name: what a place's material has to be for it to make a haft or a grip.
WOODS = _WOOD
PART_MATERIALS = {
    "hilted": _WOOD + _SOFT + _HARD + _METAL,
    "hafted": _WOOD + ("iron", "steel", "bronze"),
    "bound": _SOFT + _METAL + ("horn",),
    "handled": _WOOD + _SOFT + _HARD + _METAL,
    "stitched": _SOFT,
    "carved": _WOOD + _HARD,
}

#: Measures and vessels: the thing a ware comes in, never the thing it is.
CONTAINERS = {"bundle", "bag", "sack", "jar", "pot", "phial", "bowl", "mug", "jug", "plate",
              "wedge", "coil", "hank", "bar", "string", "brace", "bolt", "crock", "basket",
              "box", "roll", "length", "strip", "twist", "purse", "flask", "pair", "set",
              "wheel"}
_ARTICLES = {"a", "an", "the", "one"}
_PHRASE = re.compile(r",| of | in | with | and | for | from | cut ")


def head_noun(ware):
    """
    What a ware IS: the last word of its first phrase that is not a container.

    "a bundle of arrows" -> arrows; "a dagger in a plain sheath" -> dagger;
    "a sword blank, unhilted" -> blank; "a token cut from bone" -> token.
    """
    text = ware.lower()
    for article in ("a ", "an ", "the "):
        if text.startswith(article):
            text = text[len(article):]
            break
    last_container = None
    for words in (seg.split() for seg in _PHRASE.split(text)):
        words = [word for word in words if word not in _ARTICLES]
        if not words:
            continue
        if len(words) > 3:
            # "something the label does not name": the first word is the noun.
            return words[0]
        if words[-1] in CONTAINERS:
            last_container = words[-1]
            continue
        return words[-1]
    return last_container


def part_for(trade, ware, material):
    """
    The part a place's material makes of this ware, or None when it makes none.

    Both halves have to fit: the ware has to have the part, and the material has to be able
    to be it.
    """
    part = (PARTS.get(trade) or {}).get(head_noun(ware) or "")
    if not part:
        return None
    fits = PART_MATERIALS.get(part, ())
    words = set(re.findall(r"[a-z]+", (material or "").lower()))
    return part if words & set(fits) else None


def souvenir(ware, look, place, part=None):
    """
    One ware as the work of a particular place.

    Args:
        ware (str): The plain ware, as `WARES` lists it.
        look (dict): That settlement's `colour` and `material`.
        place (str): What the place is called.
        part (str, optional): What the local material makes of this trade's goods -
            `hilted`, `bound`, `handled`. Without one the ware takes only its place.

    Returns:
        named (str): "a green leather-hilted dagger from Longmire".

    Notes:
        **The article stays where it was.** "a dagger in a plain sheath" becomes "a green
        oak dagger in a plain sheath from Longmire" and not "green oak a dagger..." - the
        leading article is lifted off, the look goes in behind it, and the place goes on the
        end where a maker's mark would be.
    """
    said = ware
    lead = ""
    for article in ("a ", "an ", "the "):
        if said.startswith(article):
            lead, said = article, said[len(article):]
            break
    # **Only a plain noun takes the adjectives.** "a silver antler bundle of arrows" is what
    # happens when a colour and a material are pushed in front of a collective noun, and
    # "a dust-red horn boar spear with a crossbar" is what happens with a compound one. A
    # ware that already carries a phrase keeps its own shape and takes only the place, which
    # is all that uniqueness actually needs.
    # **A ware that already names its material keeps it.** "a sky-blue fine leather clay
    # lamp" is two materials arguing; the colour is welcome and the second material is not.
    # By whole word: "tin" is inside "hunting", and "a hunting spear" was taken for tinware.
    words = set(re.findall(r"[a-z]+", said.lower()))
    told = any(set(word.split()) <= words for word in MATERIALS)
    plain = not any(part in said for part in (" of ", " with ", " and "))
    if not plain or not part:
        # A phrase keeps its own shape, and a trade with no part for a material to be -
        # an inn, a healer - takes the place and nothing else.
        body = said
    elif told:
        # The ware names its own material, so only the colour is added: "a green clay lamp",
        # never "a green leather clay lamp".
        body = "%s %s" % (look["colour"], said)
    else:
        body = "%s %s-%s %s" % (look["colour"], look["material"], part, said)
    # **The article is chosen for the word that now comes first.** Keeping the ware's own
    # gave "an sky-blue iron cook pot": the "an" belonged to "iron", which is no longer the
    # word after it.
    if lead in ("a ", "an "):
        lead = "an " if body[:1].lower() in "aeiou" else "a "
    dressed = "%s%s" % (lead, body)
    return "%s from %s" % (dressed, place)


def trade_of(room_key, markers=None):
    """
    Which trade a room is, by its name, or None if it is not a shop.

    The room names are written by `naming.TRADES`, so the marker that matched when the tally
    counted this room as a shop is the same one that stocks it - one table, one answer.
    """
    lowered = (room_key or "").lower()
    for trade in (markers or WARES):
        if trade in lowered:
            return trade
    return None


def stock_for(room_key, size, rng, wares=None, depth=None, look=None, place=None):
    """
    What is on the shelves of one shop.

    Args:
        room_key (str): The room's name, which says what kind of shop it is.
        size (str): The settlement's size, which says how deep the stock goes.
        rng (random.Random): The world's own generator, so a seed reproduces a world.

    Returns:
        stock (list): Distinct wares, or an empty list when the room is not a shop.
    """
    table = wares or WARES
    trade = trade_of(room_key, table)
    if trade is None:
        return []
    available = list(table[trade])
    low, high = (depth or DEPTH).get(size, (3, 6))
    wanted = min(len(available), rng.randint(low, high))
    rng.shuffle(available)
    chosen = sorted(available[:wanted])
    # **A keepsake says where it came from; a meal does not.** See `KEEPSAKES`. With the
    # place on it no two settlements sell the same thing, which is the point: a hundred and
    # two wares cannot fill nineteen thousand shelves without repeating, and a maker's mark
    # is how a real economy told one town's work from another's.
    if look and place and trade in KEEPSAKES:
        return [souvenir(one, look, place, part_for(trade, one, look.get("material")))
                for one in chosen]
    return chosen
