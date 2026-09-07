"""Turn an area's exit graph into local coordinates, because the database has none worth using.

**This is the adapter the spec called `graph_inference`, and measurement made it the only
path rather than the fallback.** The plan assumed a game might carry room coordinates and
that reading them would be the easy road. The fixture does carry `map_x` and `map_y`, on
1,169 of 1,616 rooms - and in the first area inspected they read (0, 0), (0, 0), four times
None, then (178, 132). They are not one coordinate system; they are several, plus gaps.

So position is derived from the thing every MUD does keep consistent: which way you walk.
A `north` exit means the far room is north. Walk the graph from a root and the lattice
falls out.

**What this cannot do, stated up front.** A MUD map is not planar and does not close. Two
different paths between the same pair of rooms disagree about where the second one is
whenever the loop does not add up - and in a hand-built world it usually does not. This
lays rooms down breadth-first, keeps the first position it derives for a room, and
*counts every later disagreement* rather than averaging them away. That count is the
honest measure of how well an area suits a map, and it belongs in the worldfile next to
the coordinates it qualifies.
"""

import collections
from dataclasses import dataclass, field

from .evdb import COMPASS


@dataclass
class Layout:
    """Where an area's rooms sit relative to each other, and how much to trust it."""

    #: Room id to (x, y, z) in room-steps from the root. z is a floor, not a distance.
    cells: dict = field(default_factory=dict)
    #: The room everything was measured from.
    root: int = 0
    #: Rooms reached only by a named exit, whose position is a guess.
    inferred: set = field(default_factory=set)
    #: Rooms an exit contradicted after they were placed, with both positions.
    conflicts: list = field(default_factory=list)
    #: Rooms in the area no exit reaches from the root.
    unreached: set = field(default_factory=set)
    #: How many disconnected pieces the area's exit graph turned out to be.
    #:
    #: An area with no boundary exits still need not be one map. `spawn_smoke` has 179
    #: rooms, 294 internal exits and nothing crossing its edge, and its graph is 30-odd
    #: separate islands. Self-containment and connectivity are different properties and
    #: only the walk tells them apart.
    components: int = 0

    @property
    def agreement(self):
        """Fraction of direction exits whose two ends agree about where they are.

        One minus this is the share of the map that had to be forced. An area at 1.0 is
        a lattice; an area at 0.6 is a graph somebody drew without a ruler.
        """
        placed = len(self.cells)
        if not placed:
            return 0.0
        return 1.0 - len(self.conflicts) / max(1, placed + len(self.conflicts))

    def extent(self):
        """The bounding box, in room-steps: ((x0, y0, z0), (x1, y1, z1))."""
        if not self.cells:
            return ((0, 0, 0), (0, 0, 0))
        xs, ys, zs = zip(*self.cells.values())
        return ((min(xs), min(ys), min(zs)), (max(xs), max(ys), max(zs)))


def _root_of(area):
    """The best room to measure from: the one with the most direction exits.

    A well-connected room is near the middle of whatever the builder drew, which keeps the
    lattice's error spread out instead of piled up at one end.
    """
    degree = collections.Counter()
    for exit_ in area.exits:
        if exit_.name in COMPASS:
            degree[exit_.source] += 1
            degree[exit_.destination] += 1
    if degree:
        return degree.most_common(1)[0][0]
    return min(area.rooms) if area.rooms else 0


def build(area, root=None):
    """
    Lay an area's rooms out on a lattice by walking its exits.

    Args:
        area (Area): From `evdb.read`.
        root (int, optional): Room to start from. Defaults to the best-connected one.

    Returns:
        layout (Layout): Cells, and every way the walk had to compromise.

    """
    outgoing = collections.defaultdict(list)
    for exit_ in area.exits:
        outgoing[exit_.source].append(exit_)

    start = _root_of(area) if root is None else root
    layout = Layout(root=start)
    if not area.rooms:
        return layout

    # Each pass walks one connected piece. A second piece starts to the right of every
    # cell the earlier ones used, so the pieces sit side by side instead of overlapping -
    # a presentation choice, and the only honest one available, since nothing in the graph
    # says how two unconnected pieces of a map relate.
    remaining = list(area.rooms)
    remaining.sort(key=lambda room: (room != start, room))
    queue = collections.deque()
    for seed in remaining:
        if seed in layout.cells:
            continue
        layout.components += 1
        if layout.components == 1:
            layout.cells[seed] = (0, 0, 0)
        else:
            xs = [cell[0] for cell in layout.cells.values()]
            layout.cells[seed] = (max(xs) + 2, 0, 0)
        queue.append(seed)
        _walk(queue, outgoing, layout)

    layout.unreached = set(area.rooms) - set(layout.cells)
    return layout


def _walk(queue, outgoing, layout):
    """Breadth-first from everything already queued, filling in cells as it goes."""
    while queue:
        here = queue.popleft()
        x, y, z = layout.cells[here]
        for exit_ in outgoing[here]:
            step = COMPASS.get(exit_.name)
            if step is None:
                # A named way - a door, a gate, a staircase called "jail". It says the two
                # rooms are connected and nothing about where. Put it beside its source and
                # mark it, rather than dropping the room off the map entirely.
                target = (x, y, z)
                inferred = True
            else:
                target = (x + step[0], y + step[1], z + step[2])
                inferred = False

            there = exit_.destination
            if there in layout.cells:
                if layout.cells[there] != target and not inferred:
                    layout.conflicts.append((there, layout.cells[there], target))
                continue
            layout.cells[there] = target
            if inferred:
                layout.inferred.add(there)
            queue.append(there)


def spread(layout):
    """Push rooms sharing a cell apart, so a named-exit room is not on top of its neighbour.

    Rooms placed by a named exit inherit their source's cell, which means an area with
    several of them stacks rooms in one place. Nothing about the graph says which way they
    should go, so they are fanned around the cell in a fixed order - deterministic, and
    honest about being a presentation choice rather than a measurement.
    """
    occupied = collections.defaultdict(list)
    for room, cell in layout.cells.items():
        occupied[cell].append(room)

    fan = ((0, 0, 0), (1, 0, 0), (0, 1, 0), (-1, 0, 0), (0, -1, 0), (1, 1, 0), (-1, -1, 0))
    for cell, rooms in occupied.items():
        if len(rooms) < 2:
            continue
        for index, room in enumerate(sorted(rooms)):
            offset = fan[index % len(fan)]
            layout.cells[room] = (cell[0] + offset[0], cell[1] + offset[1], cell[2] + offset[2])
    return layout
