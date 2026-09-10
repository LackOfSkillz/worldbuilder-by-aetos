"""A second pass over a finished world, with a language model at the desk.

**The templates write a world that is consistent and slightly the same.** Vocabularies,
rotations and draws-without-replacement got the generator past every prose law - median 48
words, no weather, no second person, no two rooms named alike - and a law-abiding room is
not the same thing as a room worth standing in. A player reads "a beat" and learns a
hunting term; they do not learn anything about *this* beat.

So this is a curator, not a generator. It reads a finished run and writes a second opinion
beside the first.

**Three rules govern the whole module.**

  * **It never overwrites.** The template's `key` and `desc` stay exactly where they are;
    the model's go in `key_ai` and `desc_ai`. The point of the exercise is to compare them,
    and a comparison whose left-hand side has been edited is not one.
  * **It never trusts.** Everything the model returns goes through the same prose laws the
    templates pass - word band, sentence band, no second person. What fails is thrown away
    and the template text stands, and the count of what failed is the honest answer to
    "is the model actually better".
  * **It can always be stopped.** The work list is written to disk before any of it is
    done, and every finished room is journalled immediately. Closing the laptop is a valid
    way to pause; re-running the same command is how you resume.

The model is reached over the OpenAI chat-completions protocol, which is what LM Studio,
vLLM, Ollama, OpenAI and Gemini all speak - so the backend is a URL, not a rewrite.
"""

import hashlib
import io
import json
import os
import re
import time
import urllib.error
import urllib.request
from concurrent import futures

from evennia_roundtrip import wares

#: The prose laws, in the words the model is held to. Copied from the game's own
#: `area_lint.DESC_WORD_BAND` and `DESC_SENTENCE_BAND` rather than invented here: a curator
#: judged by looser rules than the templates would win the comparison by cheating.
WORD_BAND = (34, 79)
SENTENCE_BAND = (2, 4)

#: How many times a room is re-asked before its template text is left alone.
#:
#: Low on purpose. A model that has missed the band twice is not going to find it on the
#: fifth attempt, and across twenty-four thousand rooms the retries are the run time.
ATTEMPTS = 3

#: How long to wait between attempts when the backend is unreachable, in seconds.
#:
#: **A laptop that has left the building is not an error.** This runs over days and travels;
#: losing the cluster is the expected case, not the exceptional one, so the run backs off
#: and then stops cleanly rather than burning the night failing.
BACKOFF = (5.0, 20.0, 60.0)

#: How many rooms are asked about at the same time.
#:
#: Matched to the server's own `max-num-seqs`, which is 6 on this cluster. Asking
#: for more than the server will run does not go faster: the extra requests queue
#: at the far end, where this side cannot see them and cannot stop them.
AT_ONCE = int(os.environ.get("WB_CURATOR_AT_ONCE", "6"))

#: The house style, locked, so every image in the world reads as one game.
#:
#: **Style drift comes from letting the style vary with the subject.** It does not vary
#: here: only the subject half of an image brief changes, and this half is the same text
#: for a dwarven forge and a fishing shack.
IMAGE_STYLE = ("painterly digital painting, warm lantern light, rich detail, "
               "wide banner composition, no text, no lettering, no watermark")

#: Every image, exactly this size. Not "about", not "roughly": exactly.
#:
#: **The client scales with `background-size: contain`, so a picture of a different shape
#: does not crop - it letterboxes, and the gap moves from room to room.** One odd image in
#: a thousand is the one a player notices. A single locked size also means the banner's
#: height can be set once in CSS instead of reserved generously for the worst case.
#:
#: Taken from the contrib's own stylesheet rather than measured off a screenshot. A room's
#: picture shares the map's window - `.maritime-interior-image` sits where
#: `.maritime-landmap-plot` would - and that window is `height: min(60vh, 640px)` with a
#: 320px floor, its width whatever the pane allows, the image fitted with `object-fit:
#: contain`. So 640 is the ceiling the client will ever draw, and 2.4:1 is the shape the
#: pane actually is.
#:
#: Both edges are multiples of 64 because a diffusion model given a size off its own grid
#: quietly rounds and hands back something else - the same fault arriving by another road.
IMAGE_SIZE = (1536, 640)

SYSTEM = """You write room descriptions for a text MUD, in the style of hand-authored areas.

Hard rules, every one of which is checked after you answer:
- Between {low} and {high} words. Aim for about 55.
- Between {few} and {many} sentences.
- Never address the reader. No "you", "your", "yours".
- Only permanently true things. No weather, no time of day, no season, no light level,
  no people coming or going. A description is read a thousand times over years; anything
  that could change between two readings belongs somewhere else.
- No proper nouns you invented. Use only names given to you.
- British spelling. Nothing out of period: no engines, no electricity, no gunpowder or
  firearms, no modern materials. Clockwork, springs and steam are allowed.
- Do not address or invite the reader even indirectly. No "beckon", "invite", "greet",
  "welcome", "await". The room does not know anybody is reading it.

Answer with a JSON object and nothing else:
{{"name": {name_rule}, "desc": "<the description>"}}"""

#: What a street room is told about its own name.
#:
#: **A street name is geometry, not prose.** The rooms of one street share one name, and the
#: law says they must form a connected run - so "Nether Stair at Market Stair" shortened to
#: "Nether Stair" does not tidy a title, it silently merges two streets and breaks G1. The
#: model renamed 101 of 213 rooms on its first outing and had no way to know that, so it is
#: no longer asked to.
KEEP_NAME = '"<repeat the room\'s current name EXACTLY, character for character>"'

#: What an interior is told. A shop's name is constrained by nothing but its trade, and is
#: the place where a better name is actually worth having.
FREE_NAME = '"<short room name, 2-4 words, naming this shop>"'


def _fingerprint(payload):
    """A stable id for one request, so an answer is fetched once and replayed after."""
    return hashlib.sha256(
        json.dumps(payload, sort_keys=True, ensure_ascii=False).encode("utf-8")
    ).hexdigest()[:16]


def permanent_words(text):
    """
    Args:
        text (str): A description.

    Returns:
        words (int): How many words it runs, ignoring markup the player never reads.
    """
    return len(re.sub(r"\$\w+\([^)]*\)|\|\w{2}", " ", text or "").split())


def sentences(text):
    """How many sentences a description runs."""
    return len(re.findall(r"[.!?]", text or ""))


def doors_of(area, room):
    """
    The words a player must type to leave this room through a door.

    Returns:
        doors (list): `(noun, what is through it)` for every interior opening off here.

    Notes:
        Law T5: the noun a player types is the noun in the room description. The templates
        guaranteed it by construction; a curator rewriting the street does not, and on its
        first full pass 656 of 1,802 street rooms stopped mentioning their door - the Quiet-
        hall Inn's sign creaking over a room whose only way in is `go tavern`.
    """
    return [(inside["noun"], inside.get("key") or inside["noun"])
            for inside in area.get("rooms") or ()
            if inside.get("interior") and inside.get("noun")
            and inside.get("from") == room["id"]]


def judge(text, must_name=()):
    """
    Whether a description may stand, and why not when it may not.

    Args:
        text (str): What the model returned.
        must_name (iterable): Door nouns the text has to contain, word for word.

    Returns:
        fault (str or None): None when it passes; otherwise the law it broke.

    Notes:
        **The same laws, not similar ones.** These are the bands `prose_lint` measures the
        hand-built world against. A curator that graded itself would report an improvement
        whatever it wrote.
    """
    if not text or not text.strip():
        return "empty"
    words = permanent_words(text)
    if words < WORD_BAND[0]:
        return "thin (%d words)" % words
    if words > WORD_BAND[1]:
        return "fat (%d words)" % words
    count = sentences(text)
    if not SENTENCE_BAND[0] <= count <= SENTENCE_BAND[1]:
        return "%d sentences" % count
    if re.search(r"\b(you|your|yours|yourself)\b", text, re.I):
        return "second person"
    if re.search(r"\b(today|tonight|this morning|sunlight|moonlight|rain|snow|wind blows)\b",
                 text, re.I):
        return "weather or time of day"
    # **Second person without the word "you".** A colonnade whose arches "beckon" is
    # addressing a reader as surely as one that says "you see"; the pronoun check passed it
    # because the pronoun is implied rather than written. Found by reading the output, which
    # is the only way this class of fault is ever found.
    if re.search(r"\b(beckons?|beckoning|invites?|inviting|greets?|welcomes?|awaits?)\b",
                 text, re.I):
        return "addresses the reader"
    for noun in must_name:
        # Whole word: "inn" must not be satisfied by "inner", nor "stall" by "installed".
        if not re.search(r"\b%s\b" % re.escape(noun), text, re.I):
            return "door not named (%s)" % noun
    return None


class Model(object):
    """One OpenAI-compatible chat endpoint.

    Notes:
        Written against the wire protocol with the standard library rather than against a
        vendor's client package. The protocol is four keys of JSON; a dependency to post it
        would be a dependency the whole project does not otherwise have, and it would still
        need this class to reach vLLM.
    """

    #: How long an address gets to answer a probe. Short on purpose: the request timeout is
    #: three minutes because a long description takes time to write, and a dead address does
    #: not refuse - it simply never answers - so without a probe a run started away from
    #: home would wait three minutes on the house LAN before trying anything else.
    PROBE_SECONDS = 5.0

    def __init__(self, base_url, name, key=None, timeout=180.0, thinking=None):
        urls = base_url if isinstance(base_url, (list, tuple)) else str(base_url).split(",")
        self.base_urls = [u.strip().rstrip("/") for u in urls if u.strip()]
        if not self.base_urls:
            raise ValueError("a model needs at least one address")
        self.current = 0
        self.name = name
        self.key = key
        self.timeout = timeout
        self.thinking = thinking

    @property
    def base_url(self):
        """The address in use."""
        return self.base_urls[self.current]

    def _headers(self):
        return {"content-type": "application/json",
                "authorization": "Bearer %s" % (self.key or "none")}

    def pick(self):
        """
        Use the first address that answers a quick probe.

        Returns:
            url (str or None): The address now in use, or None when nothing answered.

        Notes:
            **One backend, several ways to reach it.** The GX10 cluster is on the house LAN
            at home and on Tailscale away from it. This is asked at the start of a run and
            again whenever the address in use stops answering, so a laptop carried out of
            the house mid-run changes road rather than stopping.
        """
        for index, base in enumerate(self.base_urls):
            request = urllib.request.Request(base + "/models", headers=self._headers())
            try:
                with urllib.request.urlopen(request, timeout=self.PROBE_SECONDS) as answer:
                    answer.read()
            except (urllib.error.URLError, OSError, TimeoutError):
                continue
            self.current = index
            return base
        return None

    def ask(self, system, user, temperature=0.7):
        """
        Args:
            system (str): The standing instructions.
            user (str): This room's brief.
            temperature (float): How loose to let it be.

        Returns:
            text (str): The model's reply.

        Raises:
            urllib.error.URLError: When the backend cannot be reached at all, which the
                caller treats as "pause", not as "fail".
        """
        body = {"model": self.name, "temperature": temperature,
                "messages": [{"role": "system", "content": system},
                             {"role": "user", "content": user}]}
        if self.thinking is not None:
            # vLLM's DeepSeek template takes its reasoning budget here. Reasoning tokens
            # are the whole cost of a long batch, and a room description argued with itself
            # first is not a better room description.
            body["chat_template_kwargs"] = {"thinking": bool(self.thinking)}
        payload = json.dumps(body).encode("utf-8")
        try:
            return self._post(payload)
        except urllib.error.HTTPError as trouble:
            # **The backend answered, and said no.** That is one room's problem - a request
            # it disliked - and not a lost connection, so it is not a reason to pause the
            # run. HTTPError is a URLError, which is why it has to be caught first.
            raise ValueError("HTTP %s from the model" % trouble.code)
        except (urllib.error.URLError, OSError, TimeoutError):
            # The address in use stopped answering. Find one that does and try once more;
            # if none does, say so, and the caller backs off and eventually stops cleanly.
            if self.pick() is None:
                raise
            return self._post(payload)

    def _post(self, payload):
        request = urllib.request.Request(self.base_url + "/chat/completions", data=payload,
                                         headers=self._headers())
        with urllib.request.urlopen(request, timeout=self.timeout) as answer:
            got = json.loads(answer.read().decode("utf-8"))
        return got["choices"][0]["message"]["content"]


def unwrap(text):
    """
    The JSON object out of a reply that may be wrapped in prose or a code fence.

    Returns:
        got (dict or None): `{"name": ..., "desc": ...}`, or None when there is none.

    Notes:
        Models fence their JSON, preface it, and occasionally think out loud around it.
        Asking again costs a round trip; finding the object costs a regex.
    """
    if not text:
        return None
    fenced = re.search(r"```(?:json)?\s*(\{.*?\})\s*```", text, re.S)
    raw = fenced.group(1) if fenced else None
    if raw is None:
        brace = re.search(r"\{.*\}", text, re.S)
        raw = brace.group(0) if brace else None
    if raw is None:
        return None
    try:
        got = json.loads(raw)
    except ValueError:
        return None
    return got if isinstance(got, dict) else None


def brief_for(area, room, neighbours):
    """
    What the model is told about one room.

    Args:
        area (dict): The area it stands in.
        room (dict): The room itself.
        neighbours (list): The names of the rooms it opens onto.

    Returns:
        brief (str): The user half of the request.

    Notes:
        **Everything the templates knew, and nothing they did not.** The comparison is only
        fair if both sides are working from the same facts: the culture, the ground, the
        trade, and what is next door. What the model adds has to be craft, not information.
    """
    lines = [
        "Area: %s, a %s %s." % (area.get("display_name") or area.get("name"),
                                area.get("size") or "settlement",
                                area.get("purpose") or "home"),
        "People: %s." % (area.get("race") or area.get("voice") or "mixed"),
        "Culture: %s." % (area.get("culture") or "unstated"),
        "Ground: %s metres above the sea, %s km inland."
        % (round(room.get("elevation_m") or 0.0), area.get("inland_km", "?")),
        "Level band: %s." % ("-".join(str(n) for n in (area.get("level_band") or []))
                             or "unstated"),
        "",
        "This room is currently called: %s" % (room.get("key") or "unnamed"),
    ]
    if room.get("interior"):
        lines.append("It is the inside of a shop, entered from the street by "
                     "typing 'go %s'." % (room.get("noun") or "door"))
    if room.get("stock"):
        lines.append("It sells: %s." % ", ".join(list(room["stock"])[:6]))
    if room.get("people"):
        lines.append("Standing here: %s."
                     % ", ".join(p.get("name", "someone") for p in room["people"][:4]))
    if neighbours:
        lines.append("It opens onto: %s." % ", ".join(neighbours[:6]))
    doors = doors_of(area, room)
    if doors:
        # **Said as a requirement, not as context.** Listed among the neighbours by name,
        # the model wrote about the Quiethall Inn and never said "tavern" - which is the
        # only word that opens it.
        lines.append("")
        lines.append("REQUIRED: players enter the buildings here by typing a single word, "
                     "so the description MUST contain each of these exact words, and "
                     "should make clear it is a way in: %s."
                     % "; ".join("'%s' (%s)" % (noun, what) for noun, what in doors))
    lines += ["", "The template wrote this, which you are replacing:",
              (room.get("desc") or "").strip()]
    return "\n".join(lines)


def image_brief(area, room, name):
    """
    The prompt the image model will be given for this point of interest.

    Notes:
        **Written by the model that understands the room, not by the one drawing it.** An
        image pass working from a room name alone is guessing; this way the description and
        the picture are two renderings of one understanding.

        The style half is `IMAGE_STYLE` and never varies. Only the subject does.
    """
    subject = "%s, a %s in a %s %s settlement" % (
        name, room.get("noun") or "place",
        area.get("race") or area.get("voice") or "human",
        area.get("size") or "town")
    return "%s, %s" % (subject, IMAGE_STYLE)


def work_list(document, areas=None, roads=True):
    """
    Every room to be curated, in a fixed order.

    Args:
        document (dict): A run's worldfile.
        areas (int, optional): Curate only this many areas, for a first look.
        roads (bool): Include the roads' rooms, after the areas'.

    Returns:
        jobs (list): `{"area": index, "room": id}` for an area's room, and the same with
            `"kind": "roads"` for a road's, in the order they will be done.

    Notes:
        **Written out before any of it is done.** A work list computed as it goes cannot be
        resumed, because nothing on disk says what "the rest" was.

        **Roads too.** They were left out at first, so a curated world kept the road
        template's "Lowrock lies back the way you came" in every road end it had.
    """
    jobs = []
    chosen = (document.get("areas") or [])[:areas] if areas else (document.get("areas") or [])
    for index, area in enumerate(chosen):
        for room in area.get("rooms") or ():
            jobs.append({"area": index, "room": room["id"]})
    if roads:
        for index, road in enumerate(document.get("roads") or ()):
            for room in road.get("rooms") or ():
                jobs.append({"kind": "roads", "area": index, "room": room["id"]})
    jobs.extend(ware_jobs(document, areas, roads))
    return jobs


def ware_jobs(document, areas=None, roads=True):
    """
    One job per shelf: every room with wares on it, areas first, then roads.

    Notes:
        After every room, not interleaved: the descriptions are what a player reads first,
        and a run paused halfway should have finished the rooms before starting the goods.
    """
    jobs = []
    kinds = [("areas", (document.get("areas") or [])[:areas] if areas
              else (document.get("areas") or []))]
    if roads:
        kinds.append(("roads", document.get("roads") or []))
    for kind, places in kinds:
        for index, place in enumerate(places):
            for room in place.get("rooms") or ():
                if room.get("stock"):
                    job = {"what": "wares", "area": index, "room": room["id"]}
                    if kind != "areas":
                        job["kind"] = kind
                    jobs.append(job)
    return jobs


def token(entry):
    """
    What a job or a journal record is known by.

    Notes:
        An area's room keeps the form every existing journal was written in; a road's room
        says so, because road 3 and area 3 are different places with the same index.
    """
    if entry.get("kind", "areas") == "roads":
        base = "roads:%s/%s" % (entry["area"], entry["room"])
    else:
        base = "%s/%s" % (entry["area"], entry["room"])
    # A shelf is its own job in the same room: the room's description and its goods are
    # asked for separately and journalled separately.
    return "wares:" + base if entry.get("what") == "wares" else base


def read_journal(path):
    """
    What has already been done.

    Returns:
        done (dict): room key -> the record written for it.

    Notes:
        **Append-only, and read forgivingly.** A laptop lid closing mid-write truncates the
        last line. One unreadable trailing line costs one room, which is re-asked; a
        half-written object in a single JSON file would cost the whole night.
    """
    done = {}
    if not os.path.exists(path):
        return done
    with io.open(path, encoding="utf-8") as journal:
        for line in journal:
            line = line.strip()
            if not line:
                continue
            try:
                record = json.loads(line)
            except ValueError:
                continue
            done[token(record)] = record
    return done


def _ask_room(area, room, model, attempts=ATTEMPTS):
    """
    Ask the model about one room.

    Returns:
        answer (dict): `record` when it produced something the laws allow, `fault` when it
            did not, `stop` when the backend could not be reached at all.

    Notes:
        Split out of the loop so several rooms can be in flight at once. It touches nothing
        shared - no tally, no journal, no document - so everything it learns comes back as a
        return value. That is what makes it safe to run in a pool.
    """
    by_id = {r["id"]: r for r in area.get("rooms") or ()}
    neighbours = [by_id[e["destination"]].get("key")
                  for e in area.get("exits") or ()
                  if e["source"] == room["id"] and e["destination"] in by_id]
    system = SYSTEM.format(low=WORD_BAND[0], high=WORD_BAND[1],
                           few=SENTENCE_BAND[0], many=SENTENCE_BAND[1],
                           name_rule=(FREE_NAME if room.get("interior") else KEEP_NAME))
    brief = brief_for(area, room, [n for n in neighbours if n])
    doors = [noun for noun, _what in doors_of(area, room)]

    def check(got):
        fault = judge(got.get("desc"), must_name=doors)
        if fault:
            return None, fault
        return {"name": (got.get("name") or "").strip(), "desc": got.get("desc").strip()}, None

    return ask_until(model, system, brief, check, attempts)


def _ask_wares(area, room, model, attempts=ATTEMPTS):
    """
    Ask the model to rework the goods on one shop's shelves. See `wares`.

    Returns:
        answer (dict): As `_ask_room`, with `record["wares"]` the reworked goods.
    """
    place = area.get("display_name") or area.get("name") or ""
    brief = wares.brief(area, room, place)
    originals = list(room.get("stock") or ())

    def check(got):
        fault = wares.judge(originals, got.get("wares"), place, room.get("trade"),
                            area.get("look"))
        if fault:
            return None, fault
        return {"wares": [{"name": w["name"].strip(), "desc": w["desc"].strip()}
                          for w in got["wares"]]}, None

    return ask_until(model, wares.SYSTEM, brief, check, attempts)


def ask_until(model, system, brief, check, attempts=ATTEMPTS):
    """
    Ask, judge, and ask again warmer, until an answer passes or the attempts run out.

    Args:
        check (callable): `got -> (record, None)` when the answer may stand, or
            `(None, fault)` when it may not.

    Returns:
        answer (dict): `record` and `asked`, or `fault` and `asked`, or `stop` when the
            backend could not be reached at all.

    Notes:
        The one loop both rooms and wares go through, so a lesson learned about asking - a
        refusal is one item's problem, a lost connection is the whole run's - is learned
        once.
    """
    fault = None
    for attempt in range(attempts):
        try:
            # Warmer on a retry: the same temperature that missed the band once usually
            # misses it the same way twice.
            reply = model.ask(system, brief, temperature=0.7 + 0.15 * attempt)
        except (urllib.error.URLError, OSError, TimeoutError) as trouble:
            if attempt < len(BACKOFF):
                time.sleep(BACKOFF[attempt])
                continue
            return {"stop": "cannot reach %s: %s" % (model.base_url, trouble)}
        except (KeyError, ValueError) as trouble:
            fault = "bad reply: %s" % trouble
            continue
        got = unwrap(reply)
        if not got:
            fault = "no JSON in reply"
            continue
        record, fault = check(got)
        if record is not None:
            record.update(attempts=attempt + 1,
                          fingerprint=_fingerprint({"b": brief, "m": model.name}))
            return {"record": record, "asked": attempt + 1}
    return {"fault": fault or "unknown", "asked": attempts}


def replay(document, journal_path, jobs):
    """
    Put every journalled answer for `jobs` back onto the world, asking nothing.

    Returns:
        count (int): How many were applied.
    """
    done = read_journal(journal_path)
    count = 0
    for job in jobs:
        record = done.get(token(job))
        if record is None:
            continue
        place = (document.get(job.get("kind", "areas")) or [])[job["area"]]
        room = next((r for r in place.get("rooms") or () if r["id"] == job["room"]), None)
        if room is not None:
            _apply(place, room, record)
            count += 1
    return count


def curate(document, model, journal_path, jobs, on_room=None, attempts=ATTEMPTS,
           at_once=AT_ONCE):
    """
    Walk the work list, asking the model about each room, journalling as it goes.

    Args:
        document (dict): The run's worldfile, modified in place.
        model (Model): Where to ask.
        journal_path (str): The append-only record of what is done.
        jobs (list): From `work_list`.
        on_room (callable, optional): `(done, total, record)` after each room.
        attempts (int): How many times one room is re-asked before it is left alone.
        at_once (int): How many rooms are in flight together.

    Returns:
        tally (dict): What happened, for the run summary and for `--status`.

    Notes:
        **Several rooms in flight, one thread writing.** The first outing asked one room at
        a time and the cluster reported `Running: 1 reqs` throughout - three seconds a room,
        which is twenty-one hours for a four-hundred-area world, on hardware sized to hold
        far more than one request. The asking is parallel; the journal, the tally and the
        document are touched only here, on this thread, so there is nothing to lock and
        nothing to race.

        **Stopping is not failing.** When the backend cannot be reached at all - the laptop
        has left the building, the cluster is off, ComfyUI has the card - this gives up and
        returns what it has. The journal is on disk either way, so the next run continues
        rather than restarts.
    """
    done = read_journal(journal_path)
    tally = {"asked": 0, "kept": 0, "rejected": 0, "replayed": 0, "left": 0,
             "faults": {}, "stopped": None}

    def locate(job):
        area = (document.get(job.get("kind", "areas")) or [])[job["area"]]
        room = next((r for r in area.get("rooms") or () if r["id"] == job["room"]), None)
        return area, room

    # Everything already answered is replayed first, in order and without asking: it is
    # nearly free, and doing it up front means the pool below is only ever real work.
    pending = []
    for job in jobs:
        area, room = locate(job)
        if room is None:
            continue
        if token(job) in done:
            tally["replayed"] += 1
            _apply(area, room, done[token(job)])
            continue
        pending.append(job)

    counted = tally["replayed"]
    if on_room and counted:
        on_room(counted, len(jobs), {"replayed": counted})
    if not pending:
        return tally

    with io.open(journal_path, "a", encoding="utf-8") as journal:
        with futures.ThreadPoolExecutor(max_workers=max(1, at_once)) as pool:
            sent = {}
            for job in pending:
                area, room = locate(job)
                ask = _ask_wares if job.get("what") == "wares" else _ask_room
                sent[pool.submit(ask, area, room, model, attempts)] = job
            for finished in futures.as_completed(sent):
                job = sent[finished]
                area, room = locate(job)
                answer = finished.result()
                counted += 1
                tally["asked"] += answer.get("asked", 0)

                if answer.get("stop"):
                    # **Say so once and stop counting it.** Every other request in flight
                    # fails the same way, and letting each report it turns one lost
                    # connection into four hundred copies of the same line.
                    if tally["stopped"] is None:
                        tally["stopped"] = answer["stop"]
                    continue

                where = {"area": job["area"], "room": job["room"]}
                if job.get("kind", "areas") != "areas":
                    where["kind"] = job["kind"]
                if job.get("what") == "wares":
                    where["what"] = "wares"
                if answer.get("record"):
                    record = dict(answer["record"], **where)
                    tally["kept"] += 1
                    if job.get("what") != "wares":
                        record["image"] = image_brief(area, room,
                                                      record["name"] or room.get("key"))
                        record["image_size"] = list(IMAGE_SIZE)
                    _apply(area, room, record)
                else:
                    # **The template's text stands, and the reason is written down.** A
                    # curator that quietly dropped what it could not improve would report a
                    # world it had not made.
                    fault = answer.get("fault") or "unknown"
                    tally["left"] += 1
                    tally["rejected"] += 1
                    tally["faults"][fault] = tally["faults"].get(fault, 0) + 1
                    record = dict(where, left=True, fault=fault)

                journal.write(json.dumps(record, ensure_ascii=False) + chr(10))
                journal.flush()
                os.fsync(journal.fileno())
                if on_room:
                    on_room(counted, len(jobs), record)
    return tally


def _apply(area, room, record):
    """Hang the model's answer on the room, beside the template's, never over it."""
    if record.get("left"):
        return
    if record.get("what") == "wares":
        # The reworked goods, aligned with `stock` item for item. Never in place of it.
        room["stock_ai"] = record.get("wares") or []
        return
    if record.get("name"):
        room["key_ai"] = record["name"]
    if record.get("desc"):
        room["desc_ai"] = record["desc"]
    if record.get("image"):
        room["image_prompt"] = record["image"]
        room["image_size"] = record.get("image_size") or list(IMAGE_SIZE)


def compare(document, areas=None, limit=6):
    """
    A side-by-side of what the templates wrote and what the model wrote.

    Notes:
        **The whole reason the curator does not overwrite.** Read them against each other
        and decide; a report that showed only the new text would be asking whether the
        prose is good, which is a different and much easier question than whether it is
        better.
    """
    lines = []
    for area in (document.get("areas") or [])[:areas or 1]:
        lines.append("=" * 78)
        lines.append("%s  (%s %s, %s)" % (area.get("display_name") or area.get("name"),
                                          area.get("size"), area.get("purpose"),
                                          area.get("race") or area.get("voice")))
        shown = 0
        for room in area.get("rooms") or ():
            if not room.get("desc_ai") or shown >= limit:
                continue
            shown += 1
            lines.append("")
            lines.append("-- TEMPLATE ---------------------------------------------------")
            lines.append("%s" % room.get("key"))
            lines.append((room.get("desc") or "").strip())
            lines.append("-- CURATED ----------------------------------------------------")
            lines.append("%s" % room.get("key_ai"))
            lines.append((room.get("desc_ai") or "").strip())
    return chr(10).join(lines)


#: Where a run is told to look for the model when nothing else says: the GX10 cluster on the
#: house LAN, then on Tailscale. Tried in order.
DEFAULT_URLS = "http://192.168.1.200:8888/v1,http://100.92.130.112:8888/v1"

#: How often, at most, `status.json` is rewritten. A watcher polls it every couple of seconds;
#: writing it for every room of twenty-four thousand would be most of the disk traffic.
STATUS_EVERY_SECONDS = 2.0


def write_status(path, status):
    """
    Replace `status.json` whole, so a reader never sees half of one.

    Notes:
        Advisory, so a failure to write it never stops the curation it describes: on Windows
        a reader holding the file open for an instant can refuse the replace, and the next
        write will land.
    """
    temporary = path + ".tmp"
    try:
        with io.open(temporary, "w", encoding="utf-8") as handle:
            json.dump(status, handle)
        os.replace(temporary, path)
    except OSError:
        pass


def main(argv=None):
    """Curate a finished run, resumably."""
    import argparse
    parser = argparse.ArgumentParser(description="A second opinion on a generated world.")
    parser.add_argument("--run", required=True,
                        help="A run directory, or a worldfile.json inside one.")
    parser.add_argument("--areas", type=int, default=None,
                        help="Only the first N areas, and no roads. Leave off for the whole "
                             "world.")
    parser.add_argument("--base-url", default=os.environ.get("WB_CURATOR_URL", DEFAULT_URLS),
                        help="One address or several, comma-separated, tried in order.")
    parser.add_argument("--model", default=os.environ.get(
        "WB_CURATOR_MODEL", "deepseek-v4-flash-0731"))
    parser.add_argument("--key", default=os.environ.get("WB_CURATOR_KEY"))
    parser.add_argument("--thinking", default=None,
                        help="Set to 'on' to let the model reason first. Off by default: "
                             "reasoning tokens are the whole cost of a long batch.")
    parser.add_argument("--at-once", type=int, default=AT_ONCE,
                        help="How many rooms to ask about at the same time. Match the "
                             "server's max-num-seqs; more only queues at the far end.")
    parser.add_argument("--status", action="store_true",
                        help="Say how far along it is and stop, doing no work.")
    parser.add_argument("--compare", action="store_true",
                        help="Print the side-by-side and stop.")
    parser.add_argument("--no-wares", action="store_true",
                        help="Curate rooms only, and leave every shelf as the generator made it.")
    parser.add_argument("--quiet", action="store_true",
                        help="No line per room. The studio uses this and reads status.json.")
    args = parser.parse_args(argv)

    run_dir = args.run
    if os.path.isfile(run_dir):
        run_dir = os.path.dirname(run_dir)
    worldfile = os.path.join(run_dir, "worldfile.json")
    with io.open(worldfile, encoding="utf-8") as handle:
        document = json.load(handle)

    where = os.path.join(run_dir, "curate")
    if not os.path.isdir(where):
        os.makedirs(where)
    journal_path = os.path.join(where, "done.jsonl")
    jobs_path = os.path.join(where, "job.json")
    status_path = os.path.join(where, "status.json")

    # **The work list is written once and re-read after.** A list recomputed on resume is a
    # different list whenever anything upstream changed, and the journal would then be
    # skipping rooms by index that are no longer the same rooms.
    # **The stored list is always the WHOLE world; `--areas` slices it.** Storing the
    # narrowed list pinned the scope to whatever the first run asked for, so a later
    # `--areas 3` re-read a one-area list and quietly did one area - the resume machinery
    # working exactly as built and doing the wrong thing.
    if os.path.exists(jobs_path):
        with io.open(jobs_path, encoding="utf-8") as handle:
            stored = json.load(handle)
        jobs = stored["jobs"]
        # **A run curated before wares were a job gains them, appended.** Every existing job
        # keeps its place and its journal key, so nothing already curated is asked again.
        if not any(job.get("what") == "wares" for job in jobs):
            extra = ware_jobs(document)
            if extra:
                jobs = jobs + extra
                stored["jobs"] = jobs
                with io.open(jobs_path, "w", encoding="utf-8") as handle:
                    json.dump(stored, handle)
    else:
        jobs = work_list(document)
        with io.open(jobs_path, "w", encoding="utf-8") as handle:
            json.dump({"jobs": jobs, "model": args.model}, handle)
    # The whole world's work, kept before slicing: what is written out at the end is the
    # whole world, whatever part of it this run was asked to work on.
    every_job = list(jobs)
    if args.areas:
        jobs = [job for job in jobs
                if job.get("kind", "areas") == "areas" and job["area"] < args.areas]
    if args.no_wares:
        jobs = [job for job in jobs if job.get("what") != "wares"]

    done = read_journal(journal_path)
    if args.status or args.compare:
        for job in jobs:
            if token(job) not in done:
                continue
            place = (document.get(job.get("kind", "areas")) or [])[job["area"]]
            room = next((r for r in place.get("rooms") or () if r["id"] == job["room"]), None)
            if room is not None:
                _apply(place, room, done[token(job)])
        if args.compare:
            print(compare(document, args.areas))
        else:
            finished = sum(1 for job in jobs if token(job) in done)
            left = sum(1 for job in jobs if done.get(token(job), {}).get("left"))
            print(json.dumps({"rooms": len(jobs), "done": finished,
                              "left_as_template": left,
                              "remaining": len(jobs) - finished}))
        return 0

    model = Model(args.base_url, args.model, key=args.key,
                  thinking=(args.thinking == "on") if args.thinking else False)
    # Kept and left are the run's totals, not this process's: started from what the journal
    # already holds, or a resumed run tells its watcher it has kept 21 rooms of 209 done.
    # The rate stays this process's own - it is a measure of now, not of the whole run.
    earlier = [done[token(job)] for job in jobs if token(job) in done]
    seen = {"asked": 0, "kept": sum(1 for r in earlier if not r.get("left")),
            "left": sum(1 for r in earlier if r.get("left")), "last": 0.0}
    started = time.time()
    status = {"state": "starting", "run": os.path.basename(os.path.normpath(run_dir)),
              "model": args.model, "total": len(jobs),
              "done": sum(1 for job in jobs if token(job) in done),
              "kept": seen["kept"], "left": seen["left"], "asked": 0, "rate_per_min": None,
              "eta_seconds": None,
              "url": None, "started_at": started, "updated_at": started, "stopped": None,
              "faults": {},
              # Rooms first, then shelves (see `ware_jobs`): which the dial is counting now.
              "doing": "rooms" if any(token(job) not in done for job in jobs
                                      if job.get("what") != "wares") else "shelves"}
    write_status(status_path, status)

    # **Find a road to the model before promising anything.** An address that is not there
    # does not refuse, it just never answers; probing first means a run started away from
    # home uses Tailscale at once instead of waiting three minutes on the house LAN.
    url = model.pick()
    if url is None:
        status.update(state="stopped", updated_at=time.time(),
                      stopped="no address answered: %s" % ", ".join(model.base_urls))
        write_status(status_path, status)
        print(json.dumps({"stopped": status["stopped"]}), flush=True)
        return 2
    status.update(state="running", url=url)
    write_status(status_path, status)


    def progress(count, total, record):
        if "replayed" in record:
            return
        seen["asked"] += 1
        seen["left" if record.get("left") else "kept"] += 1
        status["doing"] = "shelves" if record.get("what") == "wares" else "rooms"
        if record.get("left"):
            status["faults"][record.get("fault", "unknown")] = \
                status["faults"].get(record.get("fault", "unknown"), 0) + 1
        if not args.quiet:
            print(json.dumps({"room": count, "of": total,
                              "how": "left" if record.get("left") else "kept",
                              "name": record.get("name") or record.get("fault")}),
                  flush=True)
        now = time.time()
        if now - seen["last"] < STATUS_EVERY_SECONDS and count < total:
            return
        seen["last"] = now
        minutes = max(1e-6, (now - started) / 60.0)
        rate = seen["asked"] / minutes
        remaining = total - count
        status.update(done=count, kept=seen["kept"], left=seen["left"], asked=seen["asked"],
                      rate_per_min=round(rate, 1), url=model.base_url, updated_at=now,
                      eta_seconds=round(remaining / rate * 60.0) if rate > 0 else None)
        write_status(status_path, status)

    tally = curate(document, model, journal_path, jobs, on_room=progress,
                   at_once=args.at_once)

    # **Everything the journal holds, not only this run's slice.** A run told `--areas 1`
    # once wrote a curated.json holding one area's curation and silently dropped the other
    # hundred and twenty-nine - the journal still had them; the file did not.
    replay(document, journal_path, every_job)

    # The curated world is written before the status says "done", so whoever sees "done"
    # can open it at once.
    with io.open(os.path.join(where, "curated.json"), "w", encoding="utf-8") as handle:
        json.dump(document, handle, ensure_ascii=False)
    with io.open(os.path.join(where, "compare.txt"), "w", encoding="utf-8") as handle:
        handle.write(compare(document, args.areas))
    finished = sum(1 for job in jobs if token(job) in read_journal(journal_path))
    status.update(state="stopped" if tally["stopped"] else "done", done=finished,
                  stopped=tally["stopped"], updated_at=time.time(), eta_seconds=0,
                  kept=seen["kept"], left=seen["left"], asked=seen["asked"])
    write_status(status_path, status)
    print(json.dumps(tally), flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
