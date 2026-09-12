//! Plan 1b-3 (shores), Task 1: how is a water body's extent recorded?
//!
//! ```text
//! cargo run --release --no-default-features --bin shore_probe -- \
//!     SEED RADIUS_M PLATES LAND_FRACTION NODES [--ranges]
//! ```
//!
//! This binary is a one-off measurement, not a fixture: its figures live in
//! `docs/superpowers/reports/2026-09-12-water-1b3-outlines-design.md` and it is `git rm`ed in
//! the same task that wrote it (Task 1, Step 5). It exists because the question "how is a body's
//! extent recorded so `water_at` can answer spec §8.3 and the record still fits 2 MB?" has to be
//! answered on numbers, and the numbers need a 1,000,000-node bake.
//!
//! **No engine behaviour changes.** Nothing here is called by `bake`, `record_of` or `refine`;
//! it only reads `bake_stages`' output.
//!
//! # Method (every figure names its population, its method with parameters, and its host)
//!
//! - **Host:** the developer machine this task ran on, `cargo run --release
//!   --no-default-features`.
//! - **World:** `Surface::new(seed, radius_m, plate_count, land_fraction, None, None, tectonics)`,
//!   with `tectonics = Some(TectonicParams::ranges())` when `--ranges` is given and `None`
//!   otherwise. **No painted features and no forced outlets:** `FeatureInput` is `None` and
//!   `HydroParams::earth_like(nodes)` carries an empty `forced_outlets`. The owner's saved world
//!   is baked in the browser *with* its two painted features and one forced outlet, so this
//!   probe measures the unpainted landform of the same planet parameters, not that bake.
//! - **Params:** `HydroParams::earth_like(nodes)`, untouched.
//! - **Stages:** `hydrology::bake_stages(&surface, &params)` -- validation, `LandGraph::sample`,
//!   the flood, hollows and judging, `route`, `close_lakes`, `drainage_check`. Nothing is
//!   re-implemented here.
//! - **Bodies:** one per hollow whose `fate == Fate::Keep`, numbered 0.. in hollow order --
//!   exactly how `record_of` numbers `Body::id`, so an id printed here is the id on the wire.
//! - **`members`:** the count of nodes `i` with `routing.lake_of[i]` equal to that hollow's
//!   index (`routing::NO_LAKE` means none).
//! - **`collar`:** the distinct non-member nodes adjacent to a member through
//!   `LandGraph::neighbours` (the k-nearest graph, k = 8, `stream.rs::node_neighbours`), sorted
//!   and deduplicated.
//! - **`area_km2`:** the hollow's own `Hollow::area_m2` divided by 1e6. It is the flooded area
//!   `find_hollows` computed by summing `LandGraph::area_m2` over the hollow's members, not
//!   anything this probe re-derives, so a body's area here is the area `record_of` puts on the
//!   wire as `Body::area_m2`.
//! - **`shore_members`:** members with at least one collar neighbour. An interior member is one
//!   whose every graph neighbour is also a member.
//! - **`mc_edges` / `collar_not_above` / `mean_f` / `f>0.5`:** the member-collar graph edges of
//!   the body; how many of them have the collar end at or *below* the body's level (where Ruling
//!   S-1's premise "collar nodes stand above the body's level" fails outright -- the totals block
//!   splits those edges by what the collar node is, in a fixed precedence: a member of another
//!   kept body, an ocean node, or plain land at or below the level); and, over the
//!   rest, the mean and the past-halfway count of
//!   `f = (level_m - h_member) / (h_collar - h_member)` -- the fraction of the way from the
//!   member to the collar node at which the level contour crosses that edge, on the landform
//!   heights `LandGraph` carries (`Surface::structural_m`, linear along the edge). `f > 0.5` is a
//!   contour that lies outside Candidate B's halfway boundary.
//! - **`collar_parts/largest`:** the connected components of the collar subgraph (collar nodes
//!   joined where they are adjacent in the k-nearest graph) and the size of the largest. A body
//!   with an island carries the island's collar as its own component, and no single ring can hold
//!   two components at once.
//! - **`collar_span_km`:** the greatest great-circle distance between any two collar nodes. At
//!   most `SPAN_SAMPLE` collar nodes are used, taken evenly by index; a body that was sampled is
//!   marked `*` in the `span` column and counted in the totals.
//! - **`perim_graph_km`:** the sum over collar nodes of the mean great-circle distance from that
//!   collar node to its own adjacent members -- a graph-resolution stand-in for the shoreline's
//!   length.
//! - **`perim_circle_km`:** `2 * sqrt(pi * area_m2) / 1000`, the perimeter of a disc of the same
//!   area. A plainly separate second estimate, not a correction of the first.
//! - **`pts250_graph` / `pts250_circle`:** that perimeter in km times 4 -- the point count a
//!   250 m-spaced contour of it would carry (spec §6.6 as written).
//! - **Ring walks (the Candidate A risk check):** run on the three largest kept bodies by
//!   `area_m2` (ties to the lower body id) of each world. Two walks, because "does the ring
//!   close", "are consecutive steps adjacent in the graph" and "do any two ring edges cross" are
//!   not all non-trivial for the same walk:
//!   - **The body's centroid**, which both walks and every tangent projection here are built on,
//!     is the **area-weighted centroid of the body's members**: the sum of each member's unit
//!     position vector scaled by that member's `LandGraph::area_m2`, renormalised back onto the
//!     sphere. Collar nodes do not enter it. A body whose member vectors cancel to zero has no
//!     centroid and falls back to its first member's position, a fixed answer rather than a
//!     failure.
//!   - **Walks L and R, the minimum-turn edge walk.** Start at the collar node furthest from the body's
//!     area-weighted centroid (ties to the lower node id), with the incoming direction taken as
//!     the bearing from that node toward the centroid, so the body lies on one consistent side.
//!     At each step, project the current node's collar neighbours into `TangentFrame::at` that
//!     node, take each one's bearing by `atan2`, and step to the one whose bearing is the
//!     smallest positive turn from the reverse of the incoming direction -- the angular rule the
//!     plan's Ruling S-1 names. `L` measures that turn anticlockwise and `R` clockwise: the two
//!     are mirror images, and only one of them turns toward the side the body is on, so both are
//!     run rather than assuming which. The step back to where we came from is taken
//!     only when it is the only collar neighbour. Adjacency holds by construction; closure and
//!     coverage do not.
//!   - **Termination and revisits.** The walk stops on exactly three conditions: the chosen next
//!     node **is the start** (reported `closed`, and the start is not pushed a second time, so a
//!     closed ring of n points has n steps); the current node has **no collar neighbour at all**
//!     (reported `stuck`); or **`4 * collar` steps** have been taken (reported neither closed nor
//!     stuck -- it did not close). **Revisiting a node that is not the start is allowed and is
//!     not detected**: the walk may retrace or loop, and such a ring shows up as `ring_pts`
//!     exceeding the collar or as the step bound being hit rather than as its own outcome. That
//!     is deliberate -- suppressing revisits would be a repair of the walk, and the measurement
//!     is of the plain angular rule.
//!   - **Walk A, the angular sort.** Project every collar node into `TangentFrame::at` the
//!     body's centroid and sort by `atan2(y, x)`, ties by radius then node id. Closure holds by
//!     construction; adjacency does not.
//!   For each walk: whether it closed, how much of the collar the ring covers, how many
//!   consecutive ring steps are adjacent in the graph, how many pairs of ring edges cross, and
//!   `members_outside_ring` -- criterion 1, containment, measured directly as the number of the
//!   body's own member nodes that fall outside the ring polygon by a ray-crossing test in the
//!   tangent plane at the body's centroid. A member outside the ring is water the extent would
//!   not claim; the level contour lies further out than the members, so a ring that leaves any
//!   member out fails containment outright. A ring of fewer than 3 points contains nothing.
//! - **Crossings** are counted in the tangent plane at the body's centroid, on pairs of ring
//!   edges that share no ring index, by the sign-of-cross-product test, and only *proper*
//!   crossings count: a pair that merely touches at an endpoint or lies collinear is not a
//!   crossing. Every pair is tested (O(n^2)); no pair is sampled away.
//! - **Record words:** an outline point costs 2 words (`record.rs` writes `outline_len` then
//!   `lat, lon` per point), and a word is 8 bytes.

use worldbuilder_engine::detmath as m;
use worldbuilder_engine::hydrology::hollows::Fate;
use worldbuilder_engine::hydrology::routing::NO_LAKE;
use worldbuilder_engine::hydrology::{bake_stages, HydroParams};
use worldbuilder_engine::hydrology::landgraph::LandGraph;
use worldbuilder_engine::sphere::SpherePoint;
use worldbuilder_engine::surface::Surface;
use worldbuilder_engine::tangent::TangentFrame;
use worldbuilder_engine::tectonics::TectonicParams;
use worldbuilder_engine::vectors::Vec3;

/// At most this many collar nodes enter the pairwise span search; see the module doc.
const SPAN_SAMPLE: usize = 2_000;

/// How many bodies get the ring-walk risk check, largest area first.
const RING_BODIES: usize = 3;

const PI: f64 = std::f64::consts::PI;
const TAU: f64 = 2.0 * PI;

const USAGE: &str = "usage: shore_probe SEED RADIUS_M PLATES LAND_FRACTION NODES [--ranges]";

struct Body {
    id: usize,
    hollow: usize,
    area_m2: f64,
    level_m: f64,
    enclosed: bool,
    members: Vec<u32>,
    collar: Vec<u32>,
    collar_span_m: f64,
    span_sampled: bool,
    perim_graph_m: f64,
    collar_parts: usize,
    collar_largest_part: usize,
    /// Members with at least one collar neighbour -- the body's shore ring of members.
    shore_members: usize,
    /// Member-collar graph edges, and of those how many have the collar end at or below the
    /// body's level (where Ruling S-1's containment premise fails outright).
    edges: usize,
    edges_collar_not_above: usize,
    /// The `edges_collar_not_above` edges split by what the collar node is, in the fixed
    /// precedence stated where they are counted: a member of another kept body, an ocean node,
    /// or plain land at or below the level. The three sum to `edges_collar_not_above`.
    not_above_other_body: usize,
    not_above_ocean: usize,
    not_above_land: usize,
    /// `not_above_land` split again: the collar node belongs to some hollow the judgement did
    /// **not** keep (a notched neighbour), or it belongs to no hollow at all.
    not_above_land_notched: usize,
    not_above_land_no_hollow: usize,
    /// Over the member-collar edges whose collar end *is* above the level: the mean of
    /// `f = (level - h_member) / (h_collar - h_member)`, the fraction of the way from the member
    /// to the collar at which the level contour crosses, and how many of those edges have
    /// `f > 0.5` -- a contour that Candidate B's halfway boundary leaves outside the extent.
    mean_contour_fraction: f64,
    edges_contour_past_half: usize,
}

/// The result of one ring walk over a body's collar.
struct RingCheck {
    ring: Vec<u32>,
    closed: bool,
    stuck: bool,
    adjacent_steps: usize,
    crossings: u64,
    /// Criterion 1, containment: members of the body that fall outside the ring polygon.
    members_outside: usize,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}");
        return;
    }
    let mut ranges = false;
    let mut positional: Vec<String> = Vec::new();
    for arg in args {
        if arg == "--ranges" {
            ranges = true;
        } else {
            positional.push(arg);
        }
    }
    if positional.len() != 5 {
        eprintln!("{USAGE}");
        std::process::exit(2);
    }
    let seed: i64 = positional[0].replace('_', "").parse().expect("SEED must be an integer");
    let radius_m: f64 = positional[1].replace('_', "").parse().expect("RADIUS_M must be a number");
    let plates: usize = positional[2].parse().expect("PLATES must be an integer");
    let land: f64 = positional[3].parse().expect("LAND_FRACTION must be a number");
    let nodes: u32 = positional[4].replace('_', "").parse().expect("NODES must be an integer");

    let tectonics = if ranges { Some(TectonicParams::ranges()) } else { None };
    let surface = Surface::new(seed, radius_m, plates, land, None, None, tectonics);
    let params = HydroParams::earth_like(nodes);

    println!("# shore_probe");
    println!(
        "world: seed {seed}, radius {radius_m} m, {plates} plates, land {land}, ranges {ranges}"
    );
    println!("nodes: {nodes}; no painted features, no forced outlets; HydroParams::earth_like");

    let stages = match bake_stages(&surface, &params) {
        Ok(stages) => stages,
        Err(err) => {
            println!("bake_stages refused this world: {err:?}");
            return;
        }
    };
    let graph = &stages.graph;
    let hollows = &stages.hollows;
    let lake_of = &stages.routing.lake_of;

    // node -> hollow, inverted once: members by hollow index, in node order.
    let mut members_by_hollow: Vec<Vec<u32>> = vec![Vec::new(); hollows.len()];
    for i in 0..graph.len() {
        let lake = lake_of[i];
        if lake != NO_LAKE {
            members_by_hollow[lake as usize].push(i as u32); // cast-ok: a node index, bounded by stream::MAX_NODES
        }
    }
    // node -> a hollow that holds it, whatever that hollow's fate, so a collar node can be
    // asked "were you a hollow the judgement threw away?". A node can appear in more than one
    // hollow (`route` sub-floods a capped hollow and appends inner ones), and this map keeps the
    // FIRST in hollow order -- the count it feeds is "belongs to some hollow", not "belongs to
    // exactly this one", so which one is kept does not change it.
    let mut hollow_of_node: Vec<u32> = vec![u32::MAX; graph.len()];
    for (index, hollow) in hollows.iter().enumerate() {
        for &member in &hollow.members {
            if hollow_of_node[member as usize] == u32::MAX {
                hollow_of_node[member as usize] = index as u32; // cast-ok: a hollow index, bounded by hollows.len()
            }
        }
    }

    // Membership by node, so the collar test is a lookup rather than a search.
    let bodies = build_bodies(graph, hollows, lake_of, members_by_hollow, &hollow_of_node);

    print_table(&bodies);
    print_totals(graph, &bodies);
    print_ring_checks(graph, &bodies);
}

fn build_bodies(
    graph: &LandGraph,
    hollows: &[worldbuilder_engine::hydrology::hollows::Hollow],
    lake_of: &[u32],
    members_by_hollow: Vec<Vec<u32>>,
    hollow_of_node: &[u32],
) -> Vec<Body> {
    let mut bodies: Vec<Body> = Vec::new();
    let mut next_id = 0usize;
    let mut in_collar = vec![false; graph.len()];
    for (hollow_index, hollow) in hollows.iter().enumerate() {
        if hollow.fate != Fate::Keep {
            continue;
        }
        let id = next_id;
        next_id += 1;
        let members = members_by_hollow[hollow_index].clone();
        let mine = |node: u32| lake_of[node as usize] as usize == hollow_index;

        let mut collar: Vec<u32> = Vec::new();
        for &member in &members {
            for &next in graph.neighbours(member) {
                if !mine(next) {
                    collar.push(next);
                }
            }
        }
        collar.sort_unstable();
        collar.dedup();

        // The rough shoreline: each collar node's mean distance to the members it touches.
        let mut perim_graph_m = 0.0;
        for &c in &collar {
            let mut sum = 0.0;
            let mut count = 0usize;
            for &next in graph.neighbours(c) {
                if mine(next) {
                    sum += graph.positions[c as usize]
                        .distance_to(&graph.positions[next as usize], graph.radius_m);
                    count += 1;
                }
            }
            if count > 0 {
                perim_graph_m += sum / count as f64;
            }
        }

        // Where the level contour crosses each member-collar edge, and which members are on the
        // shore. Both are read off the landform heights the graph already carries.
        let mut shore_members = 0usize;
        let mut edges = 0usize;
        let mut edges_collar_not_above = 0usize;
        let mut not_above_other_body = 0usize;
        let mut not_above_ocean = 0usize;
        let mut not_above_land = 0usize;
        let mut not_above_land_notched = 0usize;
        let mut not_above_land_no_hollow = 0usize;
        let mut fraction_sum = 0.0;
        let mut fraction_count = 0usize;
        let mut edges_contour_past_half = 0usize;
        for &member in &members {
            let mut touches_collar = false;
            for &next in graph.neighbours(member) {
                if mine(next) {
                    continue;
                }
                touches_collar = true;
                edges += 1;
                let h_member = graph.height_m[member as usize];
                let h_collar = graph.height_m[next as usize];
                if !(h_collar > hollow.level_m) {
                    edges_collar_not_above += 1;
                    // Which kind of collar node sits at or below the body's level, in this
                    // fixed precedence so the three counts partition the edges exactly: a
                    // member of another kept body first, then an ocean node, then anything
                    // else -- plain land at or below the level, the category that exists only
                    // if it shows up.
                    if lake_of[next as usize] != NO_LAKE {
                        not_above_other_body += 1;
                    } else if graph.ocean[next as usize] {
                        not_above_ocean += 1;
                    } else {
                        not_above_land += 1;
                        if hollow_of_node[next as usize] != u32::MAX {
                            not_above_land_notched += 1;
                        } else {
                            not_above_land_no_hollow += 1;
                        }
                    }
                    continue;
                }
                // h_collar > level, and a member is at or below its lake's level, so the
                // denominator is strictly positive and f lands in (0, 1].
                let f = (hollow.level_m - h_member) / (h_collar - h_member);
                fraction_sum += f;
                fraction_count += 1;
                if f > 0.5 {
                    edges_contour_past_half += 1;
                }
            }
            if touches_collar {
                shore_members += 1;
            }
        }
        let mean_contour_fraction =
            if fraction_count == 0 { 0.0 } else { fraction_sum / fraction_count as f64 };

        let (collar_span_m, span_sampled) = collar_span(graph, &collar);

        for &c in &collar {
            in_collar[c as usize] = true;
        }
        let (collar_parts, collar_largest_part) = collar_components(graph, &collar, &in_collar);
        for &c in &collar {
            in_collar[c as usize] = false;
        }

        bodies.push(Body {
            id,
            hollow: hollow_index,
            area_m2: hollow.area_m2,
            level_m: hollow.level_m,
            enclosed: hollow.enclosed,
            members,
            collar,
            collar_span_m,
            span_sampled,
            perim_graph_m,
            collar_parts,
            collar_largest_part,
            shore_members,
            edges,
            edges_collar_not_above,
            not_above_other_body,
            not_above_ocean,
            not_above_land,
            not_above_land_notched,
            not_above_land_no_hollow,
            mean_contour_fraction,
            edges_contour_past_half,
        });
    }
    bodies
}

/// The greatest distance between any two collar nodes, over at most `SPAN_SAMPLE` of them taken
/// evenly by index. Returns `(span_m, sampled)`.
fn collar_span(graph: &LandGraph, collar: &[u32]) -> (f64, bool) {
    let sampled = collar.len() > SPAN_SAMPLE;
    let picked: Vec<u32> = if sampled {
        // Evenly by index: the i-th of SPAN_SAMPLE takes collar[i * len / SPAN_SAMPLE].
        (0..SPAN_SAMPLE).map(|i| collar[i * collar.len() / SPAN_SAMPLE]).collect()
    } else {
        collar.to_vec()
    };
    let mut span = 0.0;
    for (a, &left) in picked.iter().enumerate() {
        for &right in &picked[a + 1..] {
            let d = graph.positions[left as usize]
                .distance_to(&graph.positions[right as usize], graph.radius_m);
            if d > span {
                span = d;
            }
        }
    }
    (span, sampled)
}

/// `2 * sqrt(pi * area)`, the perimeter of a disc of the same area.
fn circle_perimeter_m(area_m2: f64) -> f64 {
    2.0 * m::sqrt(PI * area_m2)
}

/// A 250 m-spaced contour of a perimeter in km carries this many points.
fn contour_points(perimeter_km: f64) -> f64 {
    perimeter_km * 4.0
}

fn print_table(bodies: &[Body]) {
    println!();
    println!("## bodies, in body-id order");
    println!(
        "id  hollow  area_km2  level_m  encl  members  shore_members  collar  \
         collar_parts/largest  collar_span_km  perim_graph_km  perim_circle_km  \
         pts250_graph  pts250_circle  mc_edges  collar_not_above  mean_f  f>0.5"
    );
    for b in bodies {
        let perim_graph_km = b.perim_graph_m / 1_000.0;
        let perim_circle_km = circle_perimeter_m(b.area_m2) / 1_000.0;
        let span_km = b.collar_span_m / 1_000.0;
        let mark = if b.span_sampled { "*" } else { "" };
        println!(
            "{}  {}  {:.3}  {:.2}  {}  {}  {}  {}  {}/{}  {:.3}{}  {:.3}  {:.3}  {:.0}  {:.0}  \
             {}  {}  {:.3}  {}",
            b.id,
            b.hollow,
            b.area_m2 / 1.0e6,
            b.level_m,
            if b.enclosed { "y" } else { "n" },
            b.members.len(),
            b.shore_members,
            b.collar.len(),
            b.collar_parts,
            b.collar_largest_part,
            span_km,
            mark,
            perim_graph_km,
            perim_circle_km,
            contour_points(perim_graph_km),
            contour_points(perim_circle_km),
            b.edges,
            b.edges_collar_not_above,
            b.mean_contour_fraction,
            b.edges_contour_past_half,
        );
    }
}

fn print_totals(graph: &LandGraph, bodies: &[Body]) {
    let total_members: usize = bodies.iter().map(|b| b.members.len()).sum();
    let total_collar: usize = bodies.iter().map(|b| b.collar.len()).sum();
    let sampled = bodies.iter().filter(|b| b.span_sampled).count();
    let perim_graph_km: f64 = bodies.iter().map(|b| b.perim_graph_m / 1_000.0).sum();
    let perim_circle_km: f64 = bodies.iter().map(|b| circle_perimeter_m(b.area_m2) / 1_000.0).sum();
    // Round the point count to a whole number ONCE, then derive words and bytes from that
    // integer. Printing a rounded count beside `count * 2` computed on the unrounded float
    // produced an odd word count for an even number of words per point, which read like two
    // different runs (Task 1 review, critical 1).
    let pts_graph = round_to_usize(contour_points(perim_graph_km));
    let pts_circle = round_to_usize(contour_points(perim_circle_km));

    let total_shore: usize = bodies.iter().map(|b| b.shore_members).sum();
    let total_edges: usize = bodies.iter().map(|b| b.edges).sum();
    let total_not_above: usize = bodies.iter().map(|b| b.edges_collar_not_above).sum();
    let total_past_half: usize = bodies.iter().map(|b| b.edges_contour_past_half).sum();
    let total_other_body: usize = bodies.iter().map(|b| b.not_above_other_body).sum();
    let total_ocean: usize = bodies.iter().map(|b| b.not_above_ocean).sum();
    let total_land: usize = bodies.iter().map(|b| b.not_above_land).sum();
    // The mean of f weighted by each body's usable edge count, so it is the mean over EDGES and
    // not over bodies -- an unweighted mean of per-body means lets the long tail of one- and
    // two-member hollows outvote the great lake (Task 1 review, minors).
    let usable_edges = total_edges - total_not_above;
    let weighted_f_sum: f64 = bodies
        .iter()
        .map(|b| b.mean_contour_fraction * (b.edges - b.edges_collar_not_above) as f64)
        .sum();
    let mean_f_over_edges =
        if usable_edges == 0 { 0.0 } else { weighted_f_sum / usable_edges as f64 };

    // An outline point is 2 words; a word is 8 bytes.
    let collar_words = 2 * total_collar;
    let both_words = 2 * (total_members + total_collar);
    let shore_words = 2 * (total_shore + total_collar);

    println!();
    println!("## totals");
    println!("land nodes: {}", graph.ocean.iter().filter(|o| !**o).count());
    println!("bodies kept: {}", bodies.len());
    println!("members, all bodies: {total_members}");
    println!("collar nodes, all bodies: {total_collar}");
    println!("bodies whose span was sampled: {sampled} (of {})", bodies.len());
    println!(
        "bodies whose collar is in more than one piece: {} (of {})",
        bodies.iter().filter(|b| b.collar_parts > 1).count(),
        bodies.len()
    );
    println!(
        "A, collar only: {collar_words} words = {} bytes = {:.3} MB",
        collar_words * 8,
        (collar_words * 8) as f64 / 1.0e6
    );
    println!(
        "B, members + collar: {both_words} words = {} bytes = {:.3} MB",
        both_words * 8,
        (both_words * 8) as f64 / 1.0e6
    );
    println!("shore members (a member touching the collar), all bodies: {total_shore}");
    println!(
        "B trimmed to shore members + collar: {shore_words} words = {} bytes = {:.3} MB",
        shore_words * 8,
        (shore_words * 8) as f64 / 1.0e6
    );
    println!("member-collar edges, all bodies: {total_edges}");
    println!(
        "  of those, collar end NOT above the body's level: {total_not_above} ({:.3}%)",
        100.0 * total_not_above as f64 / total_edges as f64
    );
    println!(
        "    of those, the collar node is a member of another kept body: {total_other_body}; \
         an ocean node: {total_ocean}; plain land at or below the level: {total_land}"
    );
    println!(
        "      of that plain land, in a hollow the judgement did not keep: {}; in no hollow at \
         all: {}",
        bodies.iter().map(|b| b.not_above_land_notched).sum::<usize>(),
        bodies.iter().map(|b| b.not_above_land_no_hollow).sum::<usize>()
    );
    println!(
        "  usable edges (collar end above the level): {usable_edges}; of those, contour crosses \
         past halfway (f > 0.5): {total_past_half} ({:.3}%)",
        100.0 * total_past_half as f64 / usable_edges as f64
    );
    println!("  mean contour fraction f over usable edges: {mean_f_over_edges:.4}");
    println!(
        "C, 250 m contour of perim_graph: {pts_graph} points = {} words = {:.3} MB",
        pts_graph * 2,
        (pts_graph * 2 * 8) as f64 / 1.0e6
    );
    println!(
        "C, 250 m contour of perim_circle: {pts_circle} points = {} words = {:.3} MB",
        pts_circle * 2,
        (pts_circle * 2 * 8) as f64 / 1.0e6
    );
}

/// Nearest whole number, for a count that is reported and then multiplied. There is no `round`
/// in `detmath` and `.round(` is banned, so this is `floor(x + 0.5)` on a value the caller has
/// already established is a non-negative count.
fn round_to_usize(x: f64) -> usize {
    m::floor(x + 0.5) as usize // cast-ok: a non-negative point count, floored to a whole number
}

/// The area-weighted centroid of a body's members, back on the sphere.
fn centroid(graph: &LandGraph, members: &[u32]) -> SpherePoint {
    let mut sum = Vec3::new(0.0, 0.0, 0.0);
    for &node in members {
        let v = graph.positions[node as usize].vector;
        sum = sum.add(&v.scaled(graph.area_m2[node as usize]));
    }
    match SpherePoint::from_vector(&sum) {
        Some(p) => p,
        // A body whose members cancel out has no centroid; its first member is a fixed answer.
        None => graph.positions[members[0] as usize],
    }
}

/// The collar nodes adjacent to `node` in the graph, in graph order.
fn collar_neighbours(graph: &LandGraph, node: u32, in_collar: &[bool]) -> Vec<u32> {
    graph.neighbours(node).iter().copied().filter(|&n| in_collar[n as usize]).collect()
}

/// `bearing_b - bearing_a`, folded into `[0, TAU)`.
fn turn_from(bearing_a: f64, bearing_b: f64) -> f64 {
    let mut t = bearing_b - bearing_a;
    while t < 0.0 {
        t += TAU;
    }
    while t >= TAU {
        t -= TAU;
    }
    t
}

/// Walks L and R: the minimum-turn edge walk over the collar subgraph, anticlockwise (`L`,
/// `anticlockwise = true`) or clockwise (`R`). The two differ only in which side of the incoming
/// direction the rule turns toward, and one of them is the side the body is on. See the module
/// doc.
fn walk_min_turn(graph: &LandGraph, body: &Body, in_collar: &[bool], anticlockwise: bool) -> RingCheck {
    let hub = centroid(graph, &body.members);
    // Start at the collar node furthest from the centroid, ties to the lower node id.
    let mut start = body.collar[0];
    let mut best = -1.0;
    for &c in &body.collar {
        let d = hub.distance_to(&graph.positions[c as usize], graph.radius_m);
        if d > best {
            best = d;
            start = c;
        }
    }

    let mut ring: Vec<u32> = vec![start];
    let mut here = start;
    // The first "reverse of the incoming direction" is the bearing from the start toward the
    // centroid, which puts the body on one consistent side of the walk.
    let mut back_bearing = {
        let frame = TangentFrame::at(&graph.positions[start as usize], graph.radius_m);
        let (x, y) = frame.sphere_to_local(&hub);
        m::atan2(y, x)
    };
    let mut previous: Option<u32> = None;
    let bound = 4 * body.collar.len();
    let mut closed = false;
    let mut stuck = false;

    for _ in 0..bound {
        let options = collar_neighbours(graph, here, in_collar);
        if options.is_empty() {
            stuck = true;
            break;
        }
        let frame = TangentFrame::at(&graph.positions[here as usize], graph.radius_m);
        let mut pick: Option<(f64, u32)> = None;
        for &candidate in &options {
            if Some(candidate) == previous && options.len() > 1 {
                continue;
            }
            let (x, y) = frame.sphere_to_local(&graph.positions[candidate as usize]);
            let anticlockwise_turn = turn_from(back_bearing, m::atan2(y, x));
            // The clockwise walk ranks the same turns from the other side.
            let turn = if anticlockwise { anticlockwise_turn } else { TAU - anticlockwise_turn };
            let better = match pick {
                None => true,
                // Ties to the lower node id, so the walk is deterministic.
                Some((best_turn, best_node)) => {
                    turn < best_turn || (turn == best_turn && candidate < best_node)
                }
            };
            if better {
                pick = Some((turn, candidate));
            }
        }
        let (_, next) = match pick {
            Some(p) => p,
            None => {
                stuck = true;
                break;
            }
        };
        if next == start {
            closed = true;
            break;
        }
        // The new reverse-of-incoming: the bearing from `next` back to `here`.
        let next_frame = TangentFrame::at(&graph.positions[next as usize], graph.radius_m);
        let (bx, by) = next_frame.sphere_to_local(&graph.positions[here as usize]);
        back_bearing = m::atan2(by, bx);
        previous = Some(here);
        here = next;
        ring.push(next);
    }

    finish_check(graph, body, ring, closed, stuck)
}

/// Walk A: the angular sort about the body's centroid. See the module doc.
fn walk_angular(graph: &LandGraph, body: &Body) -> RingCheck {
    let hub = centroid(graph, &body.members);
    let frame = TangentFrame::at(&hub, graph.radius_m);
    let mut keyed: Vec<(f64, f64, u32)> = body
        .collar
        .iter()
        .map(|&c| {
            let (x, y) = frame.sphere_to_local(&graph.positions[c as usize]);
            (m::atan2(y, x), m::hypot(x, y), c)
        })
        .collect();
    // Total and explicit: bearing, then radius, then node id.
    keyed.sort_by(|a, b| {
        a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.cmp(&b.2))
    });
    let ring: Vec<u32> = keyed.into_iter().map(|(_, _, c)| c).collect();
    finish_check(graph, body, ring, true, false)
}

/// Counts the adjacent consecutive steps and the crossing ring-edge pairs of a finished ring.
fn finish_check(
    graph: &LandGraph,
    body: &Body,
    ring: Vec<u32>,
    closed: bool,
    stuck: bool,
) -> RingCheck {
    let mut adjacent_steps = 0usize;
    if ring.len() > 1 {
        let steps = if closed { ring.len() } else { ring.len() - 1 };
        for i in 0..steps {
            let a = ring[i];
            let b = ring[(i + 1) % ring.len()];
            if graph.neighbours(a).contains(&b) {
                adjacent_steps += 1;
            }
        }
    }
    let crossings = count_crossings(graph, body, &ring, closed);
    let members_outside = members_outside_ring(graph, body, &ring);
    RingCheck { ring, closed, stuck, adjacent_steps, crossings, members_outside }
}

/// Criterion 1, containment, measured directly: how many of the body's own member nodes fall
/// outside the ring polygon, by the ray-crossing test in the tangent plane at the body's
/// centroid. A member outside the ring is water the recorded extent would not claim, so a ring
/// that leaves any member out fails containment outright -- the level contour lies further out
/// still. Boundary cases fall where the ray test puts them; no tolerance is applied, and a ring
/// of fewer than 3 points is reported as containing nothing.
fn members_outside_ring(graph: &LandGraph, body: &Body, ring: &[u32]) -> usize {
    if ring.len() < 3 {
        return body.members.len();
    }
    let hub = centroid(graph, &body.members);
    let frame = TangentFrame::at(&hub, graph.radius_m);
    let poly: Vec<(f64, f64)> =
        ring.iter().map(|&c| frame.sphere_to_local(&graph.positions[c as usize])).collect();
    let mut outside = 0usize;
    for &member in &body.members {
        let (x, y) = frame.sphere_to_local(&graph.positions[member as usize]);
        let mut inside = false;
        let mut j = poly.len() - 1;
        for i in 0..poly.len() {
            let (xi, yi) = poly[i];
            let (xj, yj) = poly[j];
            if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
                inside = !inside;
            }
            j = i;
        }
        if !inside {
            outside += 1;
        }
    }
    outside
}

/// The connected components of the collar subgraph -- collar nodes joined where they are
/// adjacent in the k-nearest graph. Returns `(components, largest)`. A body with an island has
/// the island's own collar as a separate component from its outer shore's, and no single ring
/// can hold both.
fn collar_components(graph: &LandGraph, collar: &[u32], in_collar: &[bool]) -> (usize, usize) {
    let mut seen = vec![false; collar.len()];
    // collar is sorted, so `binary_search` is the index of a node within it.
    let mut components = 0usize;
    let mut largest = 0usize;
    let mut stack: Vec<u32> = Vec::new();
    for (start, &node) in collar.iter().enumerate() {
        if seen[start] {
            continue;
        }
        components += 1;
        seen[start] = true;
        stack.push(node);
        let mut size = 0usize;
        while let Some(here) = stack.pop() {
            size += 1;
            for &next in graph.neighbours(here) {
                if !in_collar[next as usize] {
                    continue;
                }
                let at = collar.binary_search(&next).expect("a collar node is in the collar");
                if !seen[at] {
                    seen[at] = true;
                    stack.push(next);
                }
            }
        }
        if size > largest {
            largest = size;
        }
    }
    (components, largest)
}

/// Pairs of ring edges that properly cross, in the tangent plane at the body's centroid. Every
/// pair sharing no ring index is tested.
fn count_crossings(graph: &LandGraph, body: &Body, ring: &[u32], closed: bool) -> u64 {
    if ring.len() < 4 {
        return 0;
    }
    let hub = centroid(graph, &body.members);
    let frame = TangentFrame::at(&hub, graph.radius_m);
    let flat: Vec<(f64, f64)> =
        ring.iter().map(|&c| frame.sphere_to_local(&graph.positions[c as usize])).collect();
    let edge_count = if closed { ring.len() } else { ring.len() - 1 };
    let mut crossings = 0u64;
    for i in 0..edge_count {
        let a = flat[i];
        let b = flat[(i + 1) % ring.len()];
        for j in (i + 2)..edge_count {
            // An edge pair that shares a ring index is not a crossing.
            if i == 0 && j == edge_count - 1 && closed {
                continue;
            }
            let c = flat[j];
            let d = flat[(j + 1) % ring.len()];
            if segments_cross(a, b, c, d) {
                crossings += 1;
            }
        }
    }
    crossings
}

/// Proper crossing only: a pair that touches at an endpoint or lies collinear is not counted.
fn segments_cross(a: (f64, f64), b: (f64, f64), c: (f64, f64), d: (f64, f64)) -> bool {
    let side = |p: (f64, f64), q: (f64, f64), r: (f64, f64)| {
        (q.0 - p.0) * (r.1 - p.1) - (q.1 - p.1) * (r.0 - p.0)
    };
    let d1 = side(a, b, c);
    let d2 = side(a, b, d);
    let d3 = side(c, d, a);
    let d4 = side(c, d, b);
    let straddles_ab = (d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0);
    let straddles_cd = (d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0);
    straddles_ab && straddles_cd
}

fn print_ring_checks(graph: &LandGraph, bodies: &[Body]) {
    println!();
    println!("## Candidate A's risk check: the {RING_BODIES} largest bodies by area");
    if bodies.is_empty() {
        println!("no kept bodies");
        return;
    }
    let mut order: Vec<usize> = (0..bodies.len()).collect();
    // Largest area first, ties to the lower body id.
    order.sort_by(|&a, &b| {
        bodies[b].area_m2.total_cmp(&bodies[a].area_m2).then(bodies[a].id.cmp(&bodies[b].id))
    });
    println!(
        "walk  body  area_km2  members  collar  collar_parts/largest  ring_pts  closed  stuck  \
         covers_collar  adjacent_steps/steps  crossings  members_outside_ring"
    );
    let mut in_collar = vec![false; graph.len()];
    for &which in order.iter().take(RING_BODIES) {
        let body = &bodies[which];
        for &c in &body.collar {
            in_collar[c as usize] = true;
        }
        let (parts, largest) = collar_components(graph, &body.collar, &in_collar);
        let left = walk_min_turn(graph, body, &in_collar, true);
        let right = walk_min_turn(graph, body, &in_collar, false);
        let angular = walk_angular(graph, body);
        for (name, check) in [("L", &left), ("R", &right), ("A", &angular)] {
            let steps = if check.closed { check.ring.len() } else { check.ring.len().max(1) - 1 };
            println!(
                "{}  {}  {:.3}  {}  {}  {}/{}  {}  {}  {}  {:.1}%  {}/{}  {}  {} of {}",
                name,
                body.id,
                body.area_m2 / 1.0e6,
                body.members.len(),
                body.collar.len(),
                parts,
                largest,
                check.ring.len(),
                if check.closed { "yes" } else { "no" },
                if check.stuck { "yes" } else { "no" },
                100.0 * check.ring.len() as f64 / body.collar.len() as f64,
                check.adjacent_steps,
                steps,
                check.crossings,
                check.members_outside,
                body.members.len(),
            );
        }
        for &c in &body.collar {
            in_collar[c as usize] = false;
        }
    }
}
