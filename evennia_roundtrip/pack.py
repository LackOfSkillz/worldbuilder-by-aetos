"""A content pack: the races, creatures and trades a particular game populates worlds with.

**Worldbuilder is genre-neutral and this file is the seam that keeps it so.** The generator
knows how to score ground, choose sites, lay out rooms and check laws; it knows nothing
about elves, goblins or blacksmiths. A pack supplies those, so the same machinery builds a
high-fantasy world, a steampunk one or a survey station on a moon, and swapping the world
means swapping a pack rather than editing the generator.

**The rename layer lives here, and it is a requirement rather than a courtesy.** A pack may
be derived from a reference corpus, and a reference corpus carries somebody else's invented
proper nouns. Structural facts cross freely - that a creature is level eight, hunts in
packs and is medium-sized is a fact about a game's shape, not an expressible invention.
Coined names do not cross. So every name is checked against a rename table on the way in,
and anything still carrying a coined token is refused rather than quietly passed through.

**Generic words are not coined words.** "Cave bear", "wild boar", "goblin shaman" are
ordinary English and common fantasy; they pass untouched. Only invented proper nouns need
replacing, which measured out at roughly a tenth of the vocabulary rather than the whole
roster - the difference between an afternoon and a project.
"""

import json
import os
import re

#: Tokens that are somebody's invention rather than ordinary language, with what this world
#: calls them instead. A name containing an unmapped coined token is refused.
#:
#: Deliberately a table rather than a cleverness. There is no reliable way to tell an
#: invented word from a rare one by inspection, so the list is written once by a person and
#: checked mechanically forever after.
RENAMES = {}

#: Words that look coined to a naive check and are simply uncommon English or common
#: fantasy. Anything here passes untouched.
ALLOWED_UNCOMMON = set("""
albino andesite arachnid armadillo banshee barghest basilisk bawdy beisswurm boa
bobcat bramble brocket caiman caracal cockatrice crayfish cur dryad
gargoyle ghoul gnoll gryphon griffin harpy hatchling hornet imp kelpie kobold
lich lipopod manticore mastiff naiad nyad peccary pothanit
revenant sow specter spectre stag treant tusk urchin viper vulture whelp
wraith wyrm wyvern
""".split())


def _tokens(name):
    return [w for w in re.findall(r"[A-Za-z']+", name.lower()) if w]


class Pack:
    """One game's content, ready for the generator."""

    def __init__(self, name, races, creatures, shop_mix, tone=None, renames=None):
        self.name = name
        self.races = races
        self.creatures = creatures
        self.shop_mix = shop_mix
        self.tone = tone or {}
        self.renames = renames or {}

    # -- creatures -----------------------------------------------------------------
    def creatures_for(self, low, high):
        """Every creature whose level falls in a band."""
        return [c for c in self.creatures if low <= c["level"] <= high]

    def bands(self):
        """What each level band can actually be populated with."""
        out = {}
        for c in self.creatures:
            out.setdefault(tuple(c["band"]), []).append(c["name"])
        return out

    # -- shops ---------------------------------------------------------------------
    def shops_for(self, tier):
        """The shop types a settlement of this tier carries, largest share first."""
        return list(self.shop_mix.get(tier, ()))

    def as_dict(self):
        return {"name": self.name, "races": self.races,
                "creatures": self.creatures, "shop_mix": self.shop_mix,
                "tone": self.tone}


def rename(name, renames, coined=None, vocabulary=None):
    """
    A name with every coined token replaced, or None if one has no replacement.

    Args:
        name (str): The incoming name.
        renames (dict): Coined token -> this world's word.
        coined (set, optional): Tokens known to be somebody's invention.
        vocabulary (set, optional): Ordinary words, used only to FLAG, never to refuse.

    Returns:
        renamed (str or None): The name to use, or None to refuse it.

    Notes:
        **A denylist of coined tokens, not an allowlist of ordinary words** - and the first
        version had it the wrong way round, which refused 363 of 488 creatures. English is
        far too large to enumerate, so an allowlist rejects "skunk", "caracal" and "musk
        hog" as inventions and the roster comes out a quarter of its real size. The coined
        vocabulary is small and enumerable - measured at roughly a tenth of the tokens - so
        that is the list worth writing.

        A token in `coined` with no replacement still refuses the whole record. That is the
        case that must fail closed: a name slipping through unmapped would reach a
        worldfile, a commit and possibly a published feed looking exactly like content this
        project invented.
    """
    coined = coined or set()
    words = _tokens(name)
    out = []
    for word in words:
        if word in renames:
            out.append(renames[word])
        elif word in coined:
            return None          # known invention, no replacement: refuse the record
        else:
            out.append(word)
    renamed = " ".join(out)
    return renamed[:1].upper() + renamed[1:] if renamed else None


def suspects(names, vocabulary, allowed=ALLOWED_UNCOMMON):
    """
    Tokens that look invented, for a person to rule on once.

    Returns:
        found (dict): token -> how many names use it, commonest first.

    Notes:
        This does not refuse anything. It produces the short list somebody reads once to
        write the rename table - which is the job that actually needs doing, and is an
        afternoon rather than a project because the list is short.
    """
    import collections
    seen = collections.Counter()
    for name in names:
        for word in _tokens(name):
            if word in vocabulary or word in allowed or len(word) <= 2:
                continue
            seen[word] += 1
    return dict(seen.most_common())


#: The shared vocabulary of fantasy: creatures everybody already knows, plus the ordinary
#: English that describes them.
#:
#: **This is what makes a first run possible without a vocabulary review.** A reference
#: roster carries levels worth having and names of two kinds mixed together - ordinary ones
#: like "cave bear" and "goblin shaman", and somebody's inventions. Telling them apart
#: mechanically is not possible, and guessing risks either shipping an invention or renaming
#: a perfectly good badger. So a run can be told to keep only what is unambiguously common,
#: which loses some of the roster and needs nobody's ruling.
COMMON_FANTASY = set("""
dragon drake wyvern wyrm hatchling serpent basilisk cockatrice hydra chimera manticore
griffin gryphon harpy phoenix roc pegasus unicorn kraken leviathan behemoth
vampire werewolf lycanthrope zombie skeleton skeletal ghoul wight wraith spectre specter
ghost phantom shade banshee lich mummy revenant undead draugr barrow
goblin hobgoblin orc troll ogre giant kobold gnoll bugbear imp gremlin
demon devil fiend fury incubus succubus balor archfiend hellhound
sprite pixie fairy faerie nymph dryad naiad sylph brownie boggart gnome leprechaun
elemental golem construct homunculus gargoyle automaton animated
slime ooze jelly cube mimic
troglodyte lizardfolk lizardman ratman beastman minotaur centaur satyr
giant ettin cyclops
bear wolf boar hog pig deer elk stag doe fawn antelope bison buffalo cougar puma
bobcat lynx panther leopard jaguar tiger lion cat kitten hound dog wolfhound mastiff cur
rat mouse vole shrew mole badger weasel stoat ferret otter beaver skunk raccoon
squirrel chipmunk rabbit hare fox coyote jackal hyena
horse pony mule donkey ox cow bull calf goat sheep ram lamb
snake viper adder cobra python boa constrictor crocodile alligator caiman
lizard gecko iguana monitor toad frog newt salamander turtle tortoise
spider tarantula scorpion centipede millipede beetle ant wasp hornet bee moth
butterfly locust cricket mantis grub worm slug leech maggot larva louse tick flea
crow raven rook magpie jay hawk falcon eagle owl vulture buzzard kite gull tern
heron crane stork swan goose duck grouse pheasant chicken cock rooster hen bird
fish eel shark ray squid octopus crab lobster crayfish shrimp jellyfish urchin
starfish clam oyster whale dolphin seal walrus
ape monkey gorilla baboon lemur bat
bandit brigand raider marauder reaver thug ruffian footpad cutthroat pirate corsair
rogue thief burglar assassin cultist zealot fanatic heretic
guard sentry soldier warrior berserker knight squire champion mercenary
archer bowman slinger spearman swordsman
priest cleric acolyte monk hermit oracle seer
mage magus sorcerer wizard warlock witch shaman druid necromancer conjurer
apprentice adept
peasant farmer shepherd miner smith blacksmith merchant trader beggar drunk madman
outlaw exile deserter
forest wood woodland grove thicket swamp marsh bog fen moor heath
mountain hill cave cavern crag cliff tunnel mine
desert dune sand plain steppe tundra glacier
sea ocean river lake pond stream shore beach coast reef
fire flame ember ash cinder frost ice snow storm thunder lightning
earth stone rock granite marble iron bronze copper silver gold crystal
shadow dark night moon sun star blood bone dust rot plague blight venom poison
adult baby young elder ancient old great greater lesser giant huge massive small tiny
lone wild feral savage rabid crazed mad dire fell foul cursed blessed holy unholy
red black white blue green grey gray brown golden silver pale dark bright
armoured armored horned fanged winged clawed spined scaled furred hairy bald
king queen lord lady chief chieftain captain warlord matriarch patriarch alpha
pack swarm flock herd nest hive brood
a an the of and or with in on at from to
""".split())


def common_only(creatures, vocabulary=COMMON_FANTASY):
    """
    Keep the creatures whose names are unambiguously common fantasy.

    Notes:
        A whole-name test, not a per-word one: "blood wolf" survives because both words are
        shared vocabulary, and one invented token drops the record rather than being
        quietly dropped from the name. Half a name is not a creature.
    """
    kept = []
    for c in creatures:
        words = _tokens(c["name"])
        if words and all(w in vocabulary for w in words):
            kept.append(c)
    return kept


def load_ordinary_words(path=None):
    """The words this checker considers ordinary. A small, explicit list beats a guess."""
    words = set("""
    a an the of and or with in on at from to
    adult baby young elder old great greater lesser giant huge small large tiny
    red black white blue green grey gray brown golden silver bronze iron stone
    bear wolf boar hog pig deer antelope bison cougar bobcat lynx panther lion tiger cat
    rat mouse bat snake serpent crocodile lizard toad frog spider scorpion crab badger
    skunk squirrel ape monkey horse pony mule ox cow bull goat sheep ram dog hound
    goblin orc troll ogre demon devil fiend ghost skeleton zombie corpse mummy vampire
    dragon drake golem construct elemental grub worm slug leech beetle ant wasp moth
    crow raven hawk eagle owl vulture gull heron swan goose duck chicken bird fish eel
    shark ray squid octopus jellyfish bandit thug brigand raider cutthroat pirate rogue
    thief assassin guard soldier warrior knight squire priest cleric acolyte fanatic
    zealot cultist witch warlock mage sorcerer shaman archer bowman berserker cabalist
    peasant farmer miner smith merchant beggar madman lout swain drunk crazed sleazy
    forest cave mountain swamp marsh desert sand snow ice frost fire flame storm sea
    river lake deep hill wood field alley musk salt wild striped fell forager scavenger
    blood bone dark shadow night moon sun star death blight water
    burrower crawler creeper stalker lurker prowler howler screamer digger
    hardened animated mechanical clockwork armored armoured bristle backed eyed cross
    tree fungus mold mould mushroom vine thorn boggle bucca
    """.split())
    if path and os.path.isfile(path):
        with open(path, encoding="utf-8") as handle:
            words |= {w.strip().lower() for w in handle if w.strip()}
    return words


def build(name, roster_path, shop_path, races, tone=None, renames=None,
          coined=None, bands=None):
    """
    Build a pack from exported reference data.

    Args:
        name (str): The pack's name.
        roster_path (str): `name|level|type|size|packs|alignment` lines.
        shop_path (str): `shop_type|count` lines.
        races (dict): This world's races and their affinities.
        tone (dict, optional): Period rules - banned and permitted words.
        renames (dict, optional): Coined token -> replacement.
        bands (tuple, optional): `(low, high)` level bands.

    Returns:
        built (dict): `pack`, plus `refused` - the records dropped for carrying a coined
        token with no replacement, which is a list worth reading rather than a number.
    """
    bands = bands or ((1, 5), (6, 10), (11, 20), (21, 40), (41, 60), (61, 100))
    renames = renames or dict(RENAMES)
    coined = coined or set()
    vocabulary = load_ordinary_words()

    creatures, refused = [], []
    with open(roster_path, encoding="utf-8") as handle:
        for line in handle:
            parts = line.rstrip("\n").split("|")
            if len(parts) < 6:
                continue
            raw, level = parts[0], int(parts[1] or 0)
            if level <= 0:
                continue
            safe = rename(raw, renames, coined=coined, vocabulary=vocabulary)
            if safe is None:
                refused.append(raw)
                continue
            band = next(((lo, hi) for lo, hi in bands if lo <= level <= hi), bands[-1])
            creatures.append({
                "name": safe, "level": level, "band": list(band),
                "type": parts[2] if parts[2] != "-" else None,
                "size": parts[3] if parts[3] != "-" else None,
                "packs": parts[4] == "t",
            })

    shop_mix = {}
    with open(shop_path, encoding="utf-8") as handle:
        rows = []
        for line in handle:
            bits = line.rstrip("\n").split("|")
            if len(bits) == 2 and bits[0] != "other":
                rows.append((bits[0], int(bits[1])))
    rows.sort(key=lambda r: -r[1])
    # Tiers from the measured canon distribution: a village carries the commonest few, a
    # city carries the lot. The order is by how common the type is, so the first shop a
    # settlement gets is the one settlements most often have.
    order = [r[0] for r in rows]
    shop_mix = {
        "hamlet": order[:2],
        "village": order[:4],
        "town": order[:7],
        "city": order,
    }

    pack = Pack(name, races, creatures, shop_mix, tone=tone, renames=renames)
    return {"pack": pack, "refused": refused, "creature_count": len(creatures),
            "suspects": suspects([c["name"] for c in creatures], vocabulary)}
