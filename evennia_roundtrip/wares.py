"""The curator's second job: the goods on a shop's shelves.

**The generator's wares are correct and plain.** "a dust-red rawhide-hilted dagger in a plain
sheath from Farsteading" is a real object, made of something that can make it, from a place
that is the only place it could be bought. It is also the fortieth dagger that reads exactly
like that. The model is asked to give each ware a better name and a line of description - and
is then held to everything the generator already got right:

  * **The same kind of thing.** A dagger stays a dagger. Every ware is traced back to the
    generator's own table and its head noun must still be in the new name - "a bundle of
    arrows" may become "a sheaf of goose-fletched arrows", never "a bundle of bolts". The
    owner's rule: "the items have to make some logical sense".
  * **The same place.** A keepsake ends "from Farsteading", and that is what makes it unique
    to the town: no two towns share a name, so no two sell the same keepsake. It must end
    that way still, exactly.
  * **No invented proper nouns.** The only capitalised name allowed is the place's own.
  * **A description a player can read in a breath**: one or two sentences, six to thirty-five
    words, no second person, no weather or hour, nothing out of period.
  * **Same count, same order**, so each new name is known to replace the one it replaces.

What fails keeps the generator's wares, and the reason is counted.
"""

import re

from evennia_roundtrip import period, stock

SYSTEM = """You name and describe goods for sale in one shop of a text MUD.

For each item you are given, write a better name and a one-line description.

Each item is either a staple, given with its name, or a thing from the town, given only as
what it is. A staple keeps its name exactly; the description does the work. A thing from the
town you name yourself. Food, drink, fodder, herbs and water keep a plain name. A made thing
always gains at least one particular word beyond what it is - its make, its shape, its
finish, a maker's mark - so "a hunting spear" becomes "a broad-bladed hunting spear", never
just "a hunting spear" again. Two of the same made thing in one shop need two different names.

For example, in a town called Longmire whose work is known for moss-green colour and ash:
  given: a dagger in a plain sheath, from Longmire
  name:  a narrow dagger with a moss-green ash grip, in a plain sheath from Longmire
  given: a spear ferrule, from Longmire
  name:  a spear ferrule of dark forged iron from Longmire
  given: a bag of oats - a staple: keep this name exactly
  name:  a bag of oats

Hard rules, every one of which is checked after you answer:
- Keep every item the SAME KIND of thing: a dagger stays a dagger, arrows stay arrows. You
  may add detail - maker's marks, wear, decoration, material - but not change what it is.
- An item ending "from <Place>" must still end with exactly "from <Place>". That says where
  it was made and must not change - staples included: "a bag of oats from Longmire" stays
  "a bag of oats from Longmire".
- Materials must be able to make what they are part of: no silk blades, no clay armour.
- The town's material goes only where that material is really used. Wood makes hafts, grips,
  stocks, bows, boxes and handles; it never makes nails, bits, blades, buckles or rings.
- The town's colour is a dye, paint, glaze or thread on leather, cloth, wood or pottery. Bare
  metal - a blade, a horseshoe, a bit - is never coloured.
- Not every item needs the town's colour or material. One item in three is plenty; the
  particular word for the rest comes from how it is made, not from the town's look.
- The town's colour and material belong on what it MAKES. Never dye food, drink, animals or
  plants: no blue oats, no ribboned rabbits.
- No proper nouns except the place name you are given. Invent no people, guilds or brands.
- Each description is one or two sentences, 6 to 35 words, and never addresses the reader.
- No weather, no time of day. British spelling. Nothing out of period: no engines, no
  electricity, no gunpowder or firearms, no modern materials. Clockwork, springs and steam
  are allowed - this world is discovering them.
- Return exactly as many items as you were given, in the same order.

Answer with a JSON object and nothing else:
{"wares": [{"name": "<the item, starting with a or an or none>", "desc": "<description>"}, ...]}"""

#: What a ware is, and the words that are only what it comes in, live with the wares
#: themselves: see `stock.head_noun`.
CONTAINERS = stock.CONTAINERS
head_noun = stock.head_noun

#: How long a ware's description may run, in words.
DESC_WORDS = (6, 35)


def _plain(entry):
    for article in ("a ", "an ", "the "):
        if entry.startswith(article):
            return entry[len(article):]
    return entry


def base_ware(name, trade=None, table=None):
    """
    Which of the generator's wares a shelf item was made from.

    Returns:
        base (str or None): The table's entry, e.g. "a dagger in a plain sheath".

    Notes:
        A keepsake has a colour, a material and a place wrapped round its table entry, but the
        entry itself survives whole inside it - so the longest entry found inside the name is
        the one it was made from.
    """
    table = table or stock.WARES
    pools = [table[trade]] if trade in table else list(table.values())
    found = [entry for pool in pools for entry in pool if _plain(entry) in name]
    return max(found, key=lambda entry: len(_plain(entry))) if found else None


def _mentions(text, noun):
    """Whether `text` names `noun`, as a whole word, singular or plural."""
    stem = noun[:-1] if noun.endswith("s") and len(noun) > 3 else noun
    return bool(re.search(r"\b%s\w{0,2}\b" % re.escape(stem), text.lower()))


def _stray_names(text, place):
    """Capitalised words that are neither the place nor the start of a sentence."""
    allowed = set(place.split()) if place else set()
    stray = []
    for sentence in re.split(r"(?<=[.!?])\s+", text.strip()):
        for word in re.findall(r"[A-Za-z][\w'-]*", sentence)[1:]:
            if word[0].isupper() and word not in allowed:
                stray.append(word)
    return stray


#: What wood cannot be, and what a colour cannot be put on. The model was told both and
#: still sold "a bridle with an olivewood bit" and "olivewood buckles at the calf".
_NOT_WOOD = r"(nails?|bits?|blades?|buckles?|rings?|horseshoes?|rivets?)"
_BARE_METAL = r"(blades?|steel|iron|horseshoes?|bits?)"


def _misused_look(text, look, original=""):
    """
    A fault when the town's material is made into what it cannot make, or its colour is put
    on bare metal. None otherwise, and None for anything the generator itself already said.
    """
    lowered = text.lower()
    material = str((look or {}).get("material") or "").lower()
    colour = str((look or {}).get("colour") or "").lower()
    if material and set(material.split()) & set(stock.WOODS):
        found = re.search(r"\b%s[\w-]* %s\b" % (re.escape(material), _NOT_WOOD), lowered)
        if found and found.group(0) not in original.lower():
            return "%s cannot make that (%s)" % (material, found.group(0))
    if colour:
        found = re.search(r"\b%s[\w-]* %s\b" % (re.escape(colour), _BARE_METAL), lowered)
        if found and found.group(0) not in original.lower():
            return "coloured bare metal (%s)" % found.group(0)
    return None


def judge(originals, got, place, trade=None, look=None):
    """
    Whether a reworked shelf may replace the generator's, and why not when it may not.

    Args:
        originals (list): The generator's wares, in order.
        got (list): The model's `[{"name", "desc"}, ...]`.
        place (str): The town's name, the one proper noun allowed.
        trade (str, optional): The shop's trade, to find each ware's table entry faster.
        look (dict, optional): The town's `colour` and `material`, which belong on what it
            makes and never on its oats.

    Returns:
        fault (str or None): None when every ware passes.
    """
    if not isinstance(got, list):
        return "no list of wares"
    if len(got) != len(originals):
        return "%d wares for %d" % (len(got), len(originals))
    names = set()
    for original, item in zip(originals, got):
        if not isinstance(item, dict) or not str(item.get("name") or "").strip():
            return "a ware with no name"
        name = str(item["name"]).strip()
        desc = str(item.get("desc") or "").strip()
        if name.lower() in names:
            return "two wares called %r" % name
        names.add(name.lower())
        # A ware the tables do not know - an older run's, a hand-written one - is still held
        # to its own head noun, not waved through.
        base = base_ware(original, trade)
        noun = head_noun(base or original)
        if noun and not _mentions(name, noun):
            return "%r no longer says what it is (%s)" % (name, noun)
        keepsake = bool(place) and original.rstrip().endswith("from %s" % place)
        if keepsake and not name.rstrip().endswith("from %s" % place):
            return "%r lost where it came from" % name
        # **The town's colour on what it makes, never on what it eats.** The first run
        # brought back "sky-blue oats" and "a brace of sky-blue ribboned rabbits": right in
        # kind, wrong in sense. A staple that gains the colour is refused.
        colour = str((look or {}).get("colour") or "").lower()
        if colour and not keepsake and colour in name.lower() \
                and colour not in original.lower():
            return "dyed %r the town's %s" % (name, colour)
        misused = _misused_look(name + " " + desc, look, original)
        if misused:
            return misused
        stray = _stray_names(name, place) + _stray_names(desc, place)
        if stray:
            return "invented a name (%s)" % stray[0]
        words = len(desc.split())
        if not DESC_WORDS[0] <= words <= DESC_WORDS[1]:
            return "a description of %d words" % words
        if len(re.findall(r"[.!?]", desc)) > 2:
            return "a description of more than two sentences"
        if re.search(r"\b(you|your|yours|yourself)\b", desc, re.I):
            return "a description addressed to the reader"
        if re.search(r"\b(today|tonight|this morning|rain|snow|sunlight|moonlight)\b", desc, re.I):
            return "weather or time of day"
        offences = period.offences(name + " " + desc)
        if offences:
            return "out of period (%s)" % ", ".join(word for _kind, word in offences[:2])
    return None


def brief(area, room, place):
    """What the model is told about one shop's shelves."""
    look = area.get("look") or {}
    lines = [
        "Town: %s, a %s of %s." % (place, area.get("size") or "settlement",
                                   area.get("race") or area.get("voice") or "people"),
        "Shop: %s (%s)." % (room.get("key"), room.get("trade") or "a shop"),
    ]
    # Only a shop that sells the town's own work is told the town's colour: said to the inn
    # and the market too, it came back as sky-blue oats.
    makes = any((ware or "").rstrip().endswith("from %s" % place)
                for ware in room.get("stock") or ())
    if look and makes:
        lines.append("Things made here are known for their %s colour and their %s."
                     % (look.get("colour") or "colour", look.get("material") or "material"))
    lines.append("")
    lines.append("The items, in order:")
    for number, ware in enumerate(room.get("stock") or (), 1):
        lines.append("%d. %s" % (number, _brief_line(ware, place, room.get("trade"))))
    return "\n".join(lines)


def _brief_line(ware, place, trade=None):
    """
    How one ware is put to the model.

    Notes:
        **A made thing is described, not drafted.** Shown "a sky-blue olivewood-hilted dagger
        in a plain sheath from Greystair", the model handed it back unchanged ninety-three
        times in a hundred: a finished-looking name is an answer, and it took it. Told only
        "a dagger in a plain sheath, from Greystair", it has to write the name itself.
        A staple is shown whole and told to keep it, which is what it did anyway.
    """
    made = bool(place) and ware.rstrip().endswith("from %s" % place)
    if not made:
        return "%s - a staple: keep this name exactly" % ware
    base = base_ware(ware, trade)
    what = base or ware[:-len(" from %s" % place)]
    return "%s, from %s - write its name, ending \"from %s\"" % (what, place, place)
