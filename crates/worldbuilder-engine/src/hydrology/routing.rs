//! Where each drop goes once hollows are decided: lakes flat at their level, notches cut so the
//! drained hollows run out, and every other node down its steepest slope.

use crate::hydrology::flood::{flood, Flood, NO_NODE};
use crate::hydrology::hollows::{find_hollows, forced_nodes, judge, Fate, Hollow};
use crate::hydrology::landgraph::LandGraph;
use crate::hydrology::HydroParams;

pub const NO_LAKE: u32 = u32::MAX;

/// How much each step of a notch drops below the one before, so the bed strictly falls.
const NOTCH_GRADE_M: f64 = 0.01;

#[derive(Debug, Clone, PartialEq)]
pub struct NotchRoute {
    pub nodes: Vec<u32>,
    pub bed_m: Vec<f64>,
}

#[derive(Debug, Clone)]
pub struct Routing {
    pub surface_m: Vec<f64>,
    pub receiver: Vec<u32>,
    pub lake_of: Vec<u32>,
    pub parent: Vec<u32>,
    pub notches: Vec<NotchRoute>,
    /// Nodes whose receiver was set by a cut (`cut_route` or `cut_path`). A committed node's
    /// receiver is always its `parent`, and a later cut that reaches one stops there.
    pub committed: Vec<bool>,
}

pub fn route(graph: &LandGraph, global: &Flood, hollows: &mut Vec<Hollow>, params: &HydroParams) -> Routing {
    let n = graph.len();
    let mut parent = global.parent.clone();

    // Rank of each node in the global flood's pop order, for picking each pocket's lake_entry.
    let mut global_rank = vec![u32::MAX; n];
    for (r, &node) in global.order.iter().enumerate() {
        global_rank[node as usize] = r as u32; // cast-ok: order length is bounded by the node count
    }
    let forced = forced_nodes(graph, params);

    // 1. Enclosed kept hollows: the ring above the datum drains into the basin, not over the rim,
    //    and each below-datum pocket inside becomes its own lake (Ruling W1).
    let enclosed: Vec<usize> = (0..hollows.len())
        .filter(|&h| hollows[h].enclosed && hollows[h].fate == Fate::Keep)
        .collect();
    // Indices of the nested (shore) hollows appended below, for Ruling C1-a.
    let mut nested_ids: Vec<usize> = Vec::new();
    for h in enclosed {
        let members = hollows[h].members.clone();

        // Sub-flood seeded from every submerged member, confined to H; shore nodes reached from
        // it get re-parented into the basin instead of over the rim.
        let seeds: Vec<(u32, f64)> = members
            .iter()
            .filter(|&&m| graph.height_m[m as usize] <= 0.0)
            .map(|&m| (m, 0.0))
            .collect();
        let inside = |node: u32| members.binary_search(&node).is_ok();
        let sub = flood(graph, &seeds, &inside);
        for &m in &members {
            if sub.reached[m as usize] && sub.parent[m as usize] != NO_NODE {
                parent[m as usize] = sub.parent[m as usize];
            }
        }

        // Nested hollows found in the sub-flood: a hollow that itself contains a submerged seed
        // member is one of the pockets below, not a real nested hollow (Ruling 1) - drop it.
        // The rest (shore-only pools above the datum) are judged as ordinary, non-enclosed
        // hollows.
        let mut nested = find_hollows(graph, &sub);
        nested.retain(|hollow| !hollow.members.iter().any(|&m| graph.height_m[m as usize] <= 0.0));
        for hollow in nested.iter_mut() {
            hollow.enclosed = false;
        }
        judge(&mut nested, graph, params);
        nested_ids.extend(hollows.len()..hollows.len() + nested.len());
        hollows.extend(nested);

        // Group the below-datum members into pockets, one per enclosed component id (Ruling 2).
        let mut keyed: Vec<(u32, u32)> = members
            .iter()
            .copied()
            .filter(|&m| graph.height_m[m as usize] <= 0.0)
            .map(|m| (graph.enclosed[m as usize], m))
            .collect();
        keyed.sort_unstable();

        let mut pockets: Vec<Hollow> = Vec::new();
        let mut i = 0;
        while i < keyed.len() {
            let component = keyed[i].0;
            let mut pocket_members = Vec::new();
            while i < keyed.len() && keyed[i].0 == component {
                pocket_members.push(keyed[i].1);
                i += 1;
            }
            pocket_members.sort_unstable();

            let mut floor = pocket_members[0];
            let mut entry = pocket_members[0];
            let mut area_m2 = 0.0;
            for &m in &pocket_members {
                area_m2 += graph.area_m2[m as usize];
                if graph.height_m[m as usize] < graph.height_m[floor as usize] {
                    floor = m;
                }
                if global_rank[m as usize] < global_rank[entry as usize] {
                    entry = m;
                }
            }
            let floor_m = graph.height_m[floor as usize];

            let raw_outlet = global.parent[entry as usize];
            let outlet = if raw_outlet == NO_NODE { entry } else { raw_outlet };

            let mut outlet_path = vec![entry];
            let mut cur = entry;
            while outlet_path.len() < graph.len() {
                let next = global.parent[cur as usize];
                if next == NO_NODE || graph.ocean[next as usize] {
                    break;
                }
                outlet_path.push(next);
                cur = next;
            }

            let is_forced = pocket_members.iter().any(|m| forced.binary_search(m).is_ok());

            pockets.push(Hollow {
                members: pocket_members,
                floor,
                floor_m,
                level_m: 0.0,
                depth_m: 0.0 - floor_m,
                area_m2,
                entry,
                outlet,
                enclosed: true,
                forced: is_forced,
                capped: false,
                fate: Fate::Keep,
                lake_entry: entry,
                outlet_path,
            });
        }

        // In-lake flood per pocket: every pocket member reaches its own entry without leaving the
        // water (without it, a submerged node's parent can be a shore node whose own descent
        // points back into the lake - a cycle).
        for pocket in &pockets {
            let under = |node: u32| pocket.members.binary_search(&node).is_ok();
            let inner = flood(graph, &[(pocket.lake_entry, 0.0)], &under);
            for &m in &pocket.members {
                parent[m as usize] = inner.parent[m as usize];
            }
        }

        // Replace hollows[h] with the first pocket (lowest component id); append the rest.
        let mut pockets = pockets.into_iter();
        if let Some(first) = pockets.next() {
            hollows[h] = first;
        }
        hollows.extend(pockets);
    }

    // 1b. Capped basins (Ruling 12b-5): a hollow too large to keep may still hold a real inner
    // basin, hidden because the single global flood submerges it along with everything else at
    // the outer basin's own, higher spill level. A sub-flood seeded at the floor -- the outer
    // basin's own local minimum -- finds any such inner basin on its own terms.
    //
    // Only a kept inner lake's own members get re-parented, onto the sub-flood's tree, so their
    // internal flow (and any lake member besides the entry) converges on that lake's own entry
    // and outlet instead of the outer basin's now-meaningless flat level. A notched inner hollow
    // needs nothing extra: it is ordinary terrain, cut by the minima pass like any other, along
    // the GLOBAL parent chain it already had. The floor is never a kept inner hollow's member
    // (it is the sub-flood's seed, so it is never raised in it), so it always keeps its GLOBAL
    // parent too -- its own way out, cut by the same minima pass, once judged Notch here leaves
    // it a local minimum. Reparenting every reached member instead (including ordinary, non-lake
    // ground) risks a two-node cycle: a member whose GLOBAL parent is another member that the
    // sub-flood, in turn, reaches through it.
    let capped: Vec<usize> = (0..hollows.len()).filter(|&h| hollows[h].capped).collect();
    for h in capped {
        let members = hollows[h].members.clone();
        let floor = hollows[h].floor;
        let floor_m = hollows[h].floor_m;

        let inside = |node: u32| members.binary_search(&node).is_ok();
        let sub = flood(graph, &[(floor, floor_m)], &inside);

        // The floor's own way out, restricted to this hollow's members: the very chain the
        // minima pass will cut, once this basin is judged Notch (as a capped one always is). A
        // member on it keeps its GLOBAL parent, unreparented, exactly as it would on any other
        // notched hollow -- and any hollow the sub-flood finds along it is notched below, never
        // kept. Ruling C1-a's kin: a kept lake there would need its own entry pointed back down
        // this same chain (its outlet, over its own rim), while the chain's own next member --
        // reached by the sub-flood *through* that lake -- still needs to carry on past it. Both
        // cannot hold without a two-node cycle at the seam.
        let mut escape = vec![floor];
        let mut cur = floor;
        while escape.len() < members.len() {
            let next = global.parent[cur as usize];
            if next == NO_NODE || !inside(next) {
                break;
            }
            escape.push(next);
            cur = next;
        }
        escape.sort_unstable();

        // Inner basins the sub-flood reveals: judged by the same rules as any other hollow (not
        // enclosed; one may itself be capped, and is then simply notched in its turn). One that
        // touches the escape chain is notched regardless of the ordinary rule's verdict.
        let mut inner = find_hollows(graph, &sub);
        judge(&mut inner, graph, params);
        for hollow in inner.iter_mut() {
            if hollow.fate == Fate::Keep && hollow.members.iter().any(|m| escape.binary_search(m).is_ok()) {
                hollow.fate = Fate::Notch;
            }
        }
        for &m in &members {
            if escape.binary_search(&m).is_ok() {
                continue;
            }
            if sub.reached[m as usize] && sub.parent[m as usize] != NO_NODE {
                parent[m as usize] = sub.parent[m as usize];
            }
        }
        hollows.extend(inner);
    }

    // Ruling C1-a: no kept lake on an outlet path. A fresh pocket's outlet cut (`close_lakes`,
    // `cut_path`) stops at the first lake member it meets, and a nested shore lake's own exit
    // leads back down toward the pocket it sits above -- so a kept nested hollow with a member
    // on ANY pocket's `outlet_path` would close a receiver cycle. Such a hollow is notched here,
    // before surfaces and receivers are set, so the minima pass drains it like any other.
    // Cost if wrong: a shore lake on the way out of an inland sea becomes drained ground even if
    // that sea later proves closed (its outlet never cut) -- the lake is lost, never the drainage.
    if !nested_ids.is_empty() {
        let mut on_outlet_path = vec![false; n];
        for hollow in hollows.iter() {
            if hollow.enclosed && hollow.fate == Fate::Keep {
                for &node in &hollow.outlet_path {
                    on_outlet_path[node as usize] = true;
                }
            }
        }
        for &id in &nested_ids {
            let hollow = &mut hollows[id];
            if hollow.fate == Fate::Keep && hollow.members.iter().any(|&m| on_outlet_path[m as usize]) {
                hollow.fate = Fate::Notch;
            }
        }
    }

    // 2. Kept hollows stand flat at their level.
    let mut surface_m = graph.height_m.clone();
    let mut lake_of = vec![NO_LAKE; n];
    for (id, hollow) in hollows.iter().enumerate() {
        if hollow.fate != Fate::Keep {
            continue;
        }
        for &m in &hollow.members {
            let h = graph.height_m[m as usize];
            let under = if hollow.enclosed { h <= 0.0 } else { h < hollow.level_m };
            if under {
                lake_of[m as usize] = id as u32; // cast-ok: hollow index
                surface_m[m as usize] = hollow.level_m;
            }
        }
    }

    let mut routing = Routing {
        surface_m,
        receiver: vec![NO_NODE; n],
        lake_of,
        parent,
        notches: Vec::new(),
        committed: vec![false; n],
    };

    // 3. Receivers.
    for node in 0..n {
        if graph.ocean[node] {
            continue;
        }
        routing.receiver[node] = if routing.lake_of[node] != NO_LAKE {
            routing.parent[node]
        } else {
            steepest(graph, &routing.surface_m, node as u32) // cast-ok: node index
        };
    }
    for hollow in hollows.iter() {
        if hollow.fate == Fate::Keep && !hollow.enclosed {
            routing.receiver[hollow.entry as usize] = hollow.outlet;
        }
    }
    for hollow in hollows.iter() {
        if hollow.fate == Fate::Keep && hollow.enclosed {
            set_sink(&mut routing, hollow);
        }
    }

    // 4. Local minima are cut out along their parent chain.
    for node in 0..n {
        let i = node;
        if graph.ocean[i] || routing.lake_of[i] != NO_LAKE || routing.receiver[i] != NO_NODE {
            continue;
        }
        let start_bed = routing.surface_m[i] - params.notch_fall_m;
        cut_route(&mut routing, graph, node as u32, start_bed); // cast-ok: node index
    }
    routing
}

/// The steepest strictly-lower neighbour on `surface`, ties to the lower index.
fn steepest(graph: &LandGraph, surface: &[f64], node: u32) -> u32 {
    let here = surface[node as usize];
    let mut best = NO_NODE;
    let mut best_drop = 0.0;
    for &next in graph.neighbours(node) {
        let drop = here - surface[next as usize];
        if drop <= 0.0 {
            continue;
        }
        let run = graph.positions[node as usize].distance_to(&graph.positions[next as usize], graph.radius_m);
        let slope = drop / run;
        if best == NO_NODE || slope > best_drop {
            best = next;
            best_drop = slope;
        }
    }
    best
}

/// Cut a channel from `start` along its parent chain (the flood tree) until it reaches the sea,
/// a lake member, or a node an earlier cut committed -- and never sooner. Lower ground does not
/// stop it. Each step sets the node's receiver to its parent and commits it. The bed grades
/// down from `start_bed_m` by `NOTCH_GRADE_M`; where the ground is already lower, the bed
/// follows the ground and grades on from there. The node the cut stops at is never touched.
/// The `NotchRoute` lists only the nodes whose surface the cut actually lowered. A start node
/// that is ocean, a lake member, or already committed does nothing.
///
/// # Why no receiver cycle survives (Task 3)
///
/// Once `route` and `close_lakes` have run, every receiver edge `x -> r` is one of two kinds.
///
/// - **Steepest.** `route` sets these for non-lake nodes on the surface as it stood then (lakes
///   raised, nothing cut), so `r` is strictly lower. Cuts only ever lower a surface, and a cut
///   gives every node it lowers a cut receiver in the same call. So the tail of a surviving
///   steepest edge keeps its surface, and its head can only go down: the edge still falls
///   strictly on the final surface.
/// - **Parent.** `r == parent[x]` for the final `parent`. This covers every committed node, every
///   lake member (whose receiver `route` sets to its parent inside the lake), and a kept open
///   lake's `entry -> outlet` (`find_hollows` takes the outlet from the flood's parent of the
///   entry, and `route` copies that flood's parents into `parent`). `set_sink` only removes an
///   edge.
///
/// `parent` is a forest. Each flood's parents form one, and inside an enclosed basin `route`
/// points members only at other members, rooted at the pocket entries. Only `cut_path` changes
/// it afterwards, and it stays a forest (see there). So no cycle is made of parent edges alone,
/// and every cycle contains a steepest edge.
///
/// **Heights.** Around a cycle the surface must come back to where it started, so a cycle with
/// a strictly falling (steepest) edge needs an edge that rises. None of these rise:
/// - A cut step: the next node is lowered below this one's bed, or already stood lower and the
///   bed followed it down. A later cut that re-lowers a node also resets its receiver.
/// - An edge inside a lake: flat.
/// - A lake's way out, to its outlet or to a rim node at its level: never rises, because the
///   flood reached the lake from there, so that node's flood level (and ground) is at most the
///   lake's.
///
/// The one edge left is a cut's *last* step, into a lake or an earlier cut that stands higher
/// than the bed, where a notch is dug below what it runs into. That step is still a parent
/// edge, and so is everything after it until the water leaves a lake by its way out. Every edge
/// here is also non-increasing in the global flood's spill level. Parent edges are, by
/// construction. Steepest edges are too: a flood never raises a lower neighbour above its own
/// spill. So a cycle would stay on one spill level `S`. There, a kept open lake stands exactly
/// at `S`, and no steepest edge on the level can enter it, since that needs a node above `S`.
/// The water leaves through a node `o` at height `S` and can only come back to the cut through
/// a steepest edge `o -> w` into the cut's side. That side drains into the lake, which the flood
/// reached from `o`. So `w` was not yet reached when `o` was popped, and a flood gives such a
/// neighbour the popped node as its parent. The cut through `w` therefore walked on to `o` and
/// committed it, and `o` has no steepest edge. `every_small_world_drains` sweeps 48 real worlds
/// for this case, and `drainage_check` refuses any routing it misses.
///
/// Stopping at committed nodes keeps the total work of all `cut_route` calls O(n): each node
/// is walked on from at most once.
pub fn cut_route(routing: &mut Routing, graph: &LandGraph, start: u32, start_bed_m: f64) {
    let s = start as usize;
    if graph.ocean[s] || routing.lake_of[s] != NO_LAKE || routing.committed[s] {
        return;
    }
    let mut nodes = Vec::new();
    let mut beds = Vec::new();
    let bed = grade(routing, start, start_bed_m, &mut nodes, &mut beds);
    follow_parents(routing, graph, start, bed, &mut nodes, &mut beds);
    if !nodes.is_empty() {
        routing.notches.push(NotchRoute { nodes, bed_m: beds });
    }
}

/// Lowers `node` to `bed_m` if it stands higher, recording it. Returns the bed the cut carries
/// on from, which is the node's surface after the step: `bed_m`, or the lower ground the bed
/// follows.
fn grade(routing: &mut Routing, node: u32, bed_m: f64, nodes: &mut Vec<u32>, beds: &mut Vec<f64>) -> f64 {
    let i = node as usize;
    if routing.surface_m[i] > bed_m {
        routing.surface_m[i] = bed_m;
        nodes.push(node);
        beds.push(bed_m);
    }
    routing.surface_m[i]
}

/// Carries a cut on from `start`, already graded to `bed_m`, along its parent chain. It stops at
/// the sea, a lake member, a committed node or the chain's end, and nowhere else.
fn follow_parents(routing: &mut Routing, graph: &LandGraph, start: u32, bed_m: f64, nodes: &mut Vec<u32>, beds: &mut Vec<f64>) {
    let mut here = start;
    let mut bed = bed_m;
    loop {
        let next = routing.parent[here as usize];
        if next == NO_NODE {
            break;
        }
        let i = next as usize;
        let stops = graph.ocean[i] || routing.lake_of[i] != NO_LAKE || routing.committed[i];
        routing.receiver[here as usize] = next;
        routing.committed[here as usize] = true;
        if stops {
            break;
        }
        bed = grade(routing, next, bed - NOTCH_GRADE_M, nodes, beds);
        here = next;
    }
}

/// A closed lake keeps its water: its entry drains nowhere.
pub fn set_sink(routing: &mut Routing, hollow: &Hollow) {
    routing.receiver[hollow.lake_entry as usize] = NO_NODE;
}

/// Cut a fresh pocket's outlet: along the explicit `path` (lake entry, up the shore, over the
/// rim), then on along the parent chain of its last node. The rule is `cut_route`'s, and so is
/// the proof there. `path[0]` keeps its surface, and `path[1]` is cut to `start_bed_m`.
///
/// Along the explicit path, each node's receiver *and parent* become the next path node, and
/// only the sea or a lake member stops the cut. A node an earlier cut committed does not stop
/// it: it is re-pointed along the path. Inside an enclosed basin `parent` leads back down to
/// the pocket, and `path` (the global flood's chain) leads out. Stopping at such a node would
/// leave `entry -> ... -> node -> ... -> entry`, a cycle. After the path, committed nodes stop
/// the cut, as in `cut_route`.
///
/// **`parent` stays a forest.** A cycle would have to run from the re-pointed nodes, through
/// the node the cut stopped at, and back along its old parent chain. That stop is the sea, the
/// path's end (whose parent is the sea), or a lake member. A lake member's old chain leads to
/// its entry, then on along global-flood parents. That is a pocket whose own entry the flood
/// reached earlier, since the entry is the pocket's first-reached member and this chain reached
/// one of its members first. Or it is an open lake, and C1-a leaves no nested lake on any
/// outlet path. Global-flood parents only lead to nodes popped earlier. Since Task 2's
/// tie-break, the flood pops a raised hollow's members in one unbroken run, so the chain never
/// comes back into the basin it left, or to anything the path had already crossed.
///
/// A path shorter than 2 does nothing. If nothing is lowered, no `NotchRoute` is pushed.
pub fn cut_path(routing: &mut Routing, graph: &LandGraph, path: &[u32], start_bed_m: f64) {
    if path.len() < 2 {
        return;
    }
    let mut nodes = Vec::new();
    let mut beds = Vec::new();
    let mut bed = start_bed_m;
    let mut stopped = false;
    for k in 1..path.len() {
        let (here, next) = (path[k - 1], path[k]);
        routing.receiver[here as usize] = next;
        routing.parent[here as usize] = next;
        routing.committed[here as usize] = true;
        if graph.ocean[next as usize] || routing.lake_of[next as usize] != NO_LAKE {
            // The sea or a lake member stops the cut here; its surface is never touched.
            stopped = true;
            break;
        }
        let target = if k == 1 { start_bed_m } else { bed - NOTCH_GRADE_M };
        bed = grade(routing, next, target, &mut nodes, &mut beds);
    }
    if !stopped {
        follow_parents(routing, graph, path[path.len() - 1], bed, &mut nodes, &mut beds);
    }
    if !nodes.is_empty() {
        routing.notches.push(NotchRoute { nodes, bed_m: beds });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hydrology::flood::{flood, ocean_seeds};
    use crate::hydrology::hollows::{find_hollows, judge};
    use crate::hydrology::landgraph::LandGraph;
    use crate::hydrology::HydroParams;
    use crate::sphere::SpherePoint;

    fn line(heights: &[f64], area: f64) -> LandGraph {
        let n = heights.len();
        let positions: Vec<SpherePoint> =
            (0..n).map(|i| SpherePoint::from_latlon(0.0, i as f64 * 0.5)).collect();
        let directed: Vec<Vec<u32>> = (0..n as u32) // cast-ok: tiny fixture
            .map(|i| {
                let mut v = Vec::new();
                if i > 0 { v.push(i - 1); }
                if (i as usize) + 1 < n { v.push(i + 1); }
                v
            })
            .collect();
        LandGraph::from_parts(6_371_000.0, positions, heights.to_vec(), vec![area; n], &directed,
                              vec![0.5; n])
    }

    fn routed(g: &LandGraph) -> (Vec<crate::hydrology::hollows::Hollow>, Routing) {
        let params = HydroParams::earth_like(0);
        let f = flood(g, &ocean_seeds(g), &|_| true);
        let mut hollows = find_hollows(g, &f);
        judge(&mut hollows, g, &params);
        let r = route(g, &f, &mut hollows, &params);
        (hollows, r)
    }

    /// Follows receivers from `node`; returns where it ends.
    fn terminus(r: &Routing, node: u32) -> u32 {
        let mut here = node;
        let mut steps = 0;
        while r.receiver[here as usize] != NO_NODE {
            here = r.receiver[here as usize];
            steps += 1;
            assert!(steps < 10_000, "a cycle");
        }
        here
    }

    #[test]
    fn a_notched_hollow_drains_and_its_route_only_falls() {
        let g = line(&[-50.0, 30.0, 26.0, 27.0, 28.0, 70.0], 2.0e6);
        let (_, r) = routed(&g);
        assert_eq!(terminus(&r, 2), 0, "the notched pit drains to the ocean");
        assert_eq!(r.notches.len(), 1);
        let beds = &r.notches[0].bed_m;
        assert!(beds.windows(2).all(|w| w[1] < w[0]), "a notch bed only falls: {beds:?}");
    }

    #[test]
    fn a_kept_lake_flows_out_through_its_outlet() {
        let g = line(&[-50.0, 40.0, 5.0, 12.0, 25.0, 70.0], 2.0e6);
        let (hollows, r) = routed(&g);
        assert_eq!(r.lake_of[2], 0);
        assert_eq!(r.surface_m[2], 40.0);
        assert_eq!(terminus(&r, 3), 0, "lake water leaves over the outlet and reaches the sea");
        assert_eq!(hollows[0].outlet, 1);
        assert!(r.notches.is_empty(), "a kept lake above the datum needs no cut");
    }

    #[test]
    fn an_enclosed_basin_is_a_sink_until_judged_fresh() {
        let g = line(&[-50.0, -40.0, -30.0, 39.0, -5.0, 10.0, 60.0], 1.0e6);
        let (_, r) = routed(&g);
        assert_eq!(r.surface_m[4], 0.0);
        assert_eq!(terminus(&r, 5), 4, "the shore above the datum drains into the basin");
    }

    #[test]
    fn seed_pockets_are_not_nested_hollows_and_nothing_loops() {
        let g = line(&[60.0, 10.0, -12.0, -10.0, 39.0, -30.0, -40.0, -50.0], 1.0e6);
        let (_, r) = routed(&g);
        for node in 0..g.len() as u32 { // cast-ok: node index
            if g.ocean[node as usize] {
                continue;
            }
            let end = terminus(&r, node);
            assert!(g.ocean[end as usize] || r.lake_of[end as usize] != NO_LAKE,
                    "node {node} ends at {end}, neither sea nor lake");
        }
    }

    #[test]
    fn every_below_datum_pocket_is_its_own_lake() {
        let g = line(&[60.0, -5.0, 5.0, -5.0, 39.0, -30.0, -40.0, -50.0], 1.0e6);
        let (hollows, r) = routed(&g);
        assert_ne!(r.lake_of[1], NO_LAKE, "node 1's pocket is a lake");
        assert_ne!(r.lake_of[3], NO_LAKE, "node 3's pocket is a lake");
        assert_ne!(r.lake_of[1], r.lake_of[3], "the two pockets are different lakes");
        for node in 0..g.len() as u32 { // cast-ok: node index
            if g.ocean[node as usize] {
                continue;
            }
            let end = terminus(&r, node);
            assert!(g.ocean[end as usize] || r.lake_of[end as usize] != NO_LAKE,
                    "node {node} ends at {end}, neither sea nor lake");
        }
        for hollow in hollows.iter().filter(|h| h.enclosed && h.fate == Fate::Keep) {
            assert_eq!(hollow.outlet_path[0], hollow.lake_entry,
                       "each pocket lake's outlet_path starts at its own lake_entry");
        }
    }

    #[test]
    fn a_notch_never_cuts_a_lake() {
        let g = line(&[-50.0, 25.0, 12.0, 30.0, 24.0, 70.0], 2.0e6);
        let (hollows, r) = routed(&g);
        for hollow in hollows.iter().filter(|h| h.fate == Fate::Keep) {
            for &m in &hollow.members {
                if r.lake_of[m as usize] != NO_LAKE {
                    assert_eq!(r.surface_m[m as usize], hollow.level_m,
                               "a notch must never cut through a kept lake member");
                }
            }
        }
    }

    #[test]
    fn the_enclosed_fixture_has_exactly_one_lake() {
        let g = line(&[-50.0, -40.0, -30.0, 39.0, -5.0, 10.0, 60.0], 1.0e6);
        let (hollows, _r) = routed(&g);
        let kept_enclosed: Vec<&crate::hydrology::hollows::Hollow> =
            hollows.iter().filter(|h| h.enclosed && h.fate == Fate::Keep).collect();
        assert_eq!(kept_enclosed.len(), 1, "exactly one enclosed lake");
        assert_eq!(kept_enclosed[0].lake_entry, 4);
        assert_eq!(kept_enclosed[0].outlet_path, vec![4, 3]);
    }

    /// Ruling C1-a: the shore pool at node 4 (a nested hollow at 20 m, deep and wide enough to
    /// keep) sits on the pocket's outlet path `[6, 5, 4, 3, 2]`, so it is notched, not kept.
    #[test]
    fn a_shore_lake_on_a_pockets_outlet_path_is_notched() {
        let g = line(&[-50.0, -40.0, 39.0, 30.0, 5.0, 20.0, -5.0, 60.0], 1.0e6);
        let (hollows, r) = routed(&g);
        let pocket = hollows.iter().find(|h| h.enclosed && h.fate == Fate::Keep).expect("the pocket");
        assert_eq!(pocket.outlet_path, vec![6, 5, 4, 3, 2]);
        let shore = hollows.iter().find(|h| !h.enclosed && h.members == vec![4]).expect("the nested shore pool");
        assert!(shore.depth_m >= 8.0 && shore.area_m2 >= 1.0e6, "sanity: the keep rule alone would keep it");
        assert_eq!(shore.fate, Fate::Notch);
        assert_eq!(r.lake_of[4], NO_LAKE);
        assert_eq!(terminus(&r, 4), 6, "the notched pool drains into the pocket");
    }

    #[test]
    fn on_a_real_world_every_land_node_ends_in_the_ocean_or_a_lake() {
        let surface = crate::surface::Surface::new(20_260_904, 6_371_000.0, 12, 0.29, None, None, None);
        let g = LandGraph::sample(&surface, 8_000, 400).expect("graph");
        let (_, r) = routed(&g);
        for node in 0..g.len() as u32 { // cast-ok: node index
            if g.ocean[node as usize] { continue; }
            let end = terminus(&r, node);
            assert!(g.ocean[end as usize] || r.lake_of[end as usize] != NO_LAKE,
                    "node {node} ends at {end}, neither sea nor lake");
        }
    }
}
