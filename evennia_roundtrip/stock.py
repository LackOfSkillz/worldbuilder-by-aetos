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


def stock_for(room_key, size, rng, wares=None, depth=None):
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
    return sorted(available[:wanted])
