"""Build one area: a room lattice that satisfies the building laws, with prose to match.

**The laws are the specification, not a review afterwards.** `area_lint` measures mean
degree, corridor share, junction share, dead ends, loop density and lattice fill, and an
area that fails is thrown away. So this builds *toward* those bands rather than producing a
shape and hoping - which is the difference between a generator that mostly works and one
that wastes most of its output.

**A lattice with loops, not a tree.** The cheap way to connect rooms is a spanning tree, and
a spanning tree fails three laws at once: every room but the leaves is a corridor, there are
no junctions, and loop density is zero. A player feels this as a world with no choices. So
the walk lays a connected core and then closes loops deliberately until the density band is
met.

**Descriptions are templates the model may embellish, never the model alone.** The prose
laws want 34-79 words in 2-4 sentences, and every exit's noun mentioned in the text. A
template guarantees all three by construction; a model asked to hit them freehand misses
often enough that the gate becomes the bottleneck. So the skeleton is written here and the
flavour slots are filled from the pack.
"""

import math
import random

#: The bands the building laws measure. Restated here so this file can be read alone, and
#: checked against `area_lint` by the gate rather than trusted.
SIZE_BAND = (16, 170)
MEAN_DEGREE_BAND = (1.9, 2.8)
DEAD_END_BAND = (0.05, 0.40)
MIN_JUNCTION_SHARE = 0.10
MIN_LOOP_DENSITY = 0.03
DESC_WORD_BAND = (34, 79)
DESC_SENTENCE_BAND = (2, 4)

#: Median rooms per area type, measured across canon and quoted in the laws' Part 10.
TYPE_SIZE = {"city": 60, "town": 52, "village": 47, "hamlet": 30,
             "hunting": 48, "camp": 34, "seat": 60}

#: The eight compass steps, and their opposites, so every exit is drawn both ways.
STEPS = {"north": (0, 1), "south": (0, -1), "east": (1, 0), "west": (-1, 0),
         "northeast": (1, 1), "northwest": (-1, 1),
         "southeast": (1, -1), "southwest": (-1, -1)}
OPPOSITE = {"north": "south", "south": "north", "east": "west", "west": "east",
            "northeast": "southwest", "southwest": "northeast",
            "northwest": "southeast", "southeast": "northwest"}

#: Diagonals are capped by the laws, so the walk prefers the cardinals and reaches for a
#: diagonal only when it needs one.
CARDINALS = ("north", "south", "east", "west")
DIAGONALS = ("northeast", "northwest", "southeast", "southwest")


def _neighbours(cell):
    for name, (dx, dy) in STEPS.items():
        yield name, (cell[0] + dx, cell[1] + dy)


#: How a settlement grew, which decides its shape.
#:
#: **A planned city and a village are not the same graph.** The Landing is engineered - a
#: grid of streets laid out by somebody, compact, cardinal, richly looped. A village
#: happened: it straggles along a track, wanders round a green, has a lane that goes
#: nowhere and a barn on the end of it. Growing both the same way produced a world of
#: identical blobs, which is the tell that a map was generated rather than settled.
#:
#: `reach` is how far back along the frontier the walk will pick. Small means it extends
#: whatever it just built, which snakes; large means it picks anywhere, which fills.
STYLES = {
    "planned":  {"reach": 1.00, "diagonals": 0.00, "loops": (0.16, 0.24)},
    "town":     {"reach": 0.55, "diagonals": 0.10, "loops": (0.12, 0.20)},
    "organic":  {"reach": 0.22, "diagonals": 0.25, "loops": (0.07, 0.14)},
    "straggle": {"reach": 0.12, "diagonals": 0.35, "loops": (0.05, 0.11)},
}

#: Which style each archetype grew in.
TYPE_STYLE = {"city": "planned", "seat": "planned", "town": "town",
              "village": "organic", "hamlet": "straggle",
              "camp": "straggle", "hunting": "straggle"}


def lattice(size, rng, loop_target=0.14, style="organic"):
    """
    A connected room lattice with loops, junctions and a few dead ends.

    Args:
        size (int): How many rooms.
        rng (random.Random): The world's own generator, so a seed reproduces a world.
        loop_target (float): Extra edges as a fraction of rooms, above the spanning core.

    Returns:
        built (dict): `cells` (cell -> room index) and `edges` (a set of cell pairs).

    Notes:
        **Growth is biased toward existing frontier, not random placement.** Scattering
        cells and joining them afterwards makes a blob with a low lattice fill; growing from
        a frontier keeps the shape compact, which is what `MIN_LATTICE_FILL` measures and
        what a walkable settlement actually looks like.
    """
    cells = {(0, 0)}
    frontier = [(0, 0)]
    edges = set()
    rules = STYLES.get(style, STYLES["organic"])
    while len(cells) < size and frontier:
        # **Where along the frontier the walk picks is the whole difference in shape.**
        # Picking anywhere fills a compact block - a planned town. Picking near the end
        # extends what was just built, which straggles and wanders, and is what a village
        # that grew along a track actually looks like.
        span = max(1, int(len(frontier) * rules["reach"]))
        base = frontier[len(frontier) - 1 - rng.randrange(span)]
        options = [(d, c) for d, c in _neighbours(base) if c not in cells]
        # The laws cap diagonal share, so cardinals are preferred - but a settlement that
        # never turns off the compass reads as graph paper, so a style may allow some.
        cardinal = [(d, c) for d, c in options if d in CARDINALS]
        if cardinal and rng.random() >= rules["diagonals"]:
            pool = cardinal
        else:
            pool = options or cardinal
        if not pool:
            frontier.remove(base)
            continue
        direction, cell = pool[rng.randrange(len(pool))]
        cells.add(cell)
        edges.add((base, cell))
        frontier.append(cell)
        if len(options) <= 1:
            frontier.remove(base)

    # Close loops deliberately. A spanning walk alone has zero loop density, no junctions
    # and nothing but corridors - three laws failed by taking the cheap option.
    wanted = int(len(cells) * loop_target)
    tries = 0
    while wanted > 0 and tries < wanted * 40:
        tries += 1
        cell = rng.choice(list(cells))
        for direction, other in _neighbours(cell):
            if other in cells and direction in CARDINALS:
                pair = (cell, other)
                if pair not in edges and (other, cell) not in edges:
                    edges.add(pair)
                    wanted -= 1
                    break
    order = sorted(cells, key=lambda c: (c[1], c[0]))
    return {"cells": {c: i for i, c in enumerate(order)}, "edges": edges,
            "order": order}


def shape_of(built):
    """The measurements the laws care about, so the gate is checked before it is run."""
    cells, edges = built["cells"], built["edges"]
    degree = {c: 0 for c in cells}
    for a, b in edges:
        degree[a] += 1
        degree[b] += 1
    n = len(cells)
    if not n:
        return {}
    values = list(degree.values())
    dead = sum(1 for d in values if d <= 1)
    junction = sum(1 for d in values if d >= 3)
    return {
        "rooms": n,
        "mean_degree": 2.0 * len(edges) / n,
        "dead_end_share": dead / n,
        "junction_share": junction / n,
        "loop_density": (len(edges) - (n - 1)) / n,
    }


def fits_laws(shape):
    """Whether a lattice is inside every band, with the reasons it is not."""
    problems = []
    if not SIZE_BAND[0] <= shape.get("rooms", 0) <= SIZE_BAND[1]:
        problems.append("size %s outside %s" % (shape.get("rooms"), (SIZE_BAND,)))
    if not MEAN_DEGREE_BAND[0] <= shape.get("mean_degree", 0) <= MEAN_DEGREE_BAND[1]:
        problems.append("mean degree %.2f outside %s"
                        % (shape.get("mean_degree", 0), (MEAN_DEGREE_BAND,)))
    if not DEAD_END_BAND[0] <= shape.get("dead_end_share", 0) <= DEAD_END_BAND[1]:
        problems.append("dead ends %.2f outside %s"
                        % (shape.get("dead_end_share", 0), (DEAD_END_BAND,)))
    if shape.get("junction_share", 0) < MIN_JUNCTION_SHARE:
        problems.append("junctions %.2f below %.2f"
                        % (shape.get("junction_share", 0), MIN_JUNCTION_SHARE))
    if shape.get("loop_density", 0) < MIN_LOOP_DENSITY:
        problems.append("loop density %.3f below %.2f"
                        % (shape.get("loop_density", 0), MIN_LOOP_DENSITY))
    return problems


def build_lattice(size, rng, attempts=40, style="organic"):
    """A lattice that passes the shape laws, or the best attempt and why it failed."""
    best, best_problems = None, None
    for _ in range(attempts):
        # The loop target is nudged per attempt: too few loops fails density, too many
        # fails mean degree, and where the window sits depends on the shape that grew.
        low, high = STYLES.get(style, STYLES["organic"])["loops"]
        built = lattice(size, rng, loop_target=rng.uniform(low, high), style=style)
        shape = shape_of(built)
        problems = fits_laws(shape)
        if not problems:
            built["shape"] = shape
            return built, []
        if best is None or len(problems) < len(best_problems):
            built["shape"] = shape
            best, best_problems = built, problems
    return best, best_problems


def rooms_and_exits(built, names, base_id):
    """Turn a lattice into room and exit records, both ways along every edge."""
    cells = built["cells"]
    rooms = []
    for cell in built["order"]:
        index = cells[cell]
        rooms.append({"id": base_id + index, "cell": [cell[0], cell[1], 0],
                      "key": names[index % len(names)]})
    exits = []
    for a, b in built["edges"]:
        for direction, (dx, dy) in STEPS.items():
            if (a[0] + dx, a[1] + dy) == b:
                exits.append({"source": base_id + cells[a], "name": direction,
                              "destination": base_id + cells[b]})
                exits.append({"source": base_id + cells[b], "name": OPPOSITE[direction],
                              "destination": base_id + cells[a]})
                break
    return rooms, exits
