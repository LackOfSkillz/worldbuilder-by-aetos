//! Streams, rivers and great rivers, cut from the flow wherever it passes the thresholds.

use crate::detmath as m;
use crate::hydrology::flood::NO_NODE;
use crate::hydrology::landgraph::LandGraph;
use crate::hydrology::routing::{Routing, NO_LAKE};
use crate::hydrology::HydroParams;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReachClass { Stream, River, Great }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Downstream { Reach(u32), Body(u32), Ocean, Sink }

#[derive(Debug, Clone, PartialEq)]
pub struct Reach {
    pub nodes: Vec<u32>,
    pub class: ReachClass,
    pub order: u32,
    pub downstream: Downstream,
}

pub fn width_m(flow_m2: f64, params: &HydroParams) -> f64 {
    3.0 * m::powf(flow_m2 / params.stream_flow_m2, 0.5)
}

pub fn depth_m(flow_m2: f64, params: &HydroParams) -> f64 {
    0.5 * m::powf(flow_m2 / params.stream_flow_m2, 0.4)
}

pub fn extract(graph: &LandGraph, routing: &Routing, flow: &[f64], params: &HydroParams) -> Vec<Reach> {
    let n = graph.len();
    let channel = |i: usize| !graph.ocean[i] && routing.lake_of[i] == NO_LAKE && flow[i] >= params.stream_flow_m2;
    let mut feeders = vec![0u32; n];
    for i in 0..n {
        if channel(i) {
            let r = routing.receiver[i];
            if r != NO_NODE {
                feeders[r as usize] += 1;
            }
        }
    }
    // A lake outlet starts a reach even with one channel feeder: the lake is the source.
    let mut outlet_of_lake = vec![false; n];
    for i in 0..n {
        if routing.lake_of[i] != NO_LAKE {
            let r = routing.receiver[i];
            if r != NO_NODE && routing.lake_of[r as usize] == NO_LAKE {
                outlet_of_lake[r as usize] = true;
            }
        }
    }
    let starts: Vec<u32> = (0..n)
        .filter(|&i| channel(i) && (feeders[i] != 1 || outlet_of_lake[i]))
        .map(|i| i as u32) // cast-ok: node index
        .collect();
    let mut reach_at = vec![u32::MAX; n];
    for (id, &s) in starts.iter().enumerate() {
        reach_at[s as usize] = id as u32; // cast-ok: at most one reach per node
    }
    let mut reaches = Vec::with_capacity(starts.len());
    for &start in &starts {
        let mut nodes = vec![start];
        let mut here = start;
        let downstream = loop {
            let next = routing.receiver[here as usize];
            if next == NO_NODE {
                break Downstream::Sink;
            }
            let j = next as usize;
            if graph.ocean[j] {
                nodes.push(next);
                break Downstream::Ocean;
            }
            if routing.lake_of[j] != NO_LAKE {
                nodes.push(next);
                break Downstream::Body(routing.lake_of[j]);
            }
            nodes.push(next);
            if reach_at[j] != u32::MAX {
                break Downstream::Reach(reach_at[j]);
            }
            here = next;
        };
        let last = *nodes.last().expect("a reach has nodes");
        let q = if graph.ocean[last as usize] || routing.lake_of[last as usize] != NO_LAKE {
            flow[nodes[nodes.len() - 2] as usize]
        } else {
            flow[last as usize]
        };
        let class = if q >= params.great_flow_m2 {
            ReachClass::Great
        } else if q >= params.river_flow_m2 {
            ReachClass::River
        } else {
            ReachClass::Stream
        };
        reaches.push(Reach { nodes, class, order: 0, downstream });
    }
    strahler(reaches)
}

/// Strahler order, in topological order of reaches (sources first).
pub fn strahler(mut reaches: Vec<Reach>) -> Vec<Reach> {
    let count = reaches.len();
    let mut inputs: Vec<Vec<u32>> = vec![Vec::new(); count];
    for (id, reach) in reaches.iter().enumerate() {
        if let Downstream::Reach(next) = reach.downstream {
            inputs[next as usize].push(id as u32); // cast-ok: reach index
        }
    }
    let mut pending: Vec<u32> = inputs.iter().map(|v| v.len() as u32).collect(); // cast-ok: feeder count
    let mut ready: Vec<usize> = (0..count).filter(|&i| pending[i] == 0).collect();
    let mut head = 0;
    while head < ready.len() {
        let id = ready[head];
        head += 1;
        let mut top = 0u32;
        let mut at_top = 0u32;
        for &input in &inputs[id] {
            let o = reaches[input as usize].order;
            if o > top {
                top = o;
                at_top = 1;
            } else if o == top {
                at_top += 1;
            }
        }
        reaches[id].order = if inputs[id].is_empty() { 1 } else if at_top >= 2 { top + 1 } else { top };
        if let Downstream::Reach(next) = reaches[id].downstream {
            let j = next as usize;
            pending[j] -= 1;
            if pending[j] == 0 {
                ready.push(j);
            }
        }
    }
    reaches
}

pub fn bifurcation_ratios(reaches: &[Reach]) -> Vec<f64> {
    let top = reaches.iter().map(|r| r.order).max().unwrap_or(0) as usize; // cast-ok: order count fits usize
    let mut counts = vec![0.0f64; top + 2];
    let mut continues = vec![false; reaches.len()];
    for reach in reaches {
        if let Downstream::Reach(next) = reach.downstream {
            if reaches[next as usize].order == reach.order {
                continues[next as usize] = true;
            }
        }
    }
    for (id, reach) in reaches.iter().enumerate() {
        if !continues[id] {
            counts[reach.order as usize] += 1.0;
        }
    }
    let mut ratios = Vec::new();
    for k in 1..=top {
        if counts[k] >= 10.0 && counts[k + 1] >= 1.0 {
            ratios.push(counts[k] / counts[k + 1]);
        }
    }
    ratios
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hydrology::flood::{flood, ocean_seeds};
    use crate::hydrology::flow::close_lakes;
    use crate::hydrology::hollows::{find_hollows, judge};
    use crate::hydrology::landgraph::LandGraph;
    use crate::hydrology::routing::route;
    use crate::hydrology::HydroParams;

    fn baked(seed: i64, nodes: u32) -> (LandGraph, Routing, Vec<f64>, Vec<Reach>) {
        let surface = crate::surface::Surface::new(seed, 6_371_000.0, 12, 0.29, None, None, None);
        let g = LandGraph::sample(&surface, nodes, 500).expect("graph");
        let mut params = HydroParams::earth_like(nodes);
        // Coarse test graph: scale the thresholds to its node area so there are reaches to see.
        params.stream_flow_m2 = 3.0e10;
        params.river_flow_m2 = 3.0e11;
        params.great_flow_m2 = 3.0e12;
        let f = flood(&g, &ocean_seeds(&g), &|_| true);
        let mut hollows = find_hollows(&g, &f);
        judge(&mut hollows, &g, &params);
        let mut r = route(&g, &f, &mut hollows, &params);
        let (flow, _) = close_lakes(&g, &mut r, &hollows, &params);
        let reaches = extract(&g, &r, &flow, &params);
        (g, r, flow, reaches)
    }

    #[test]
    fn reaches_are_connected_and_acyclic() {
        let (_, _, _, reaches) = baked(20_260_904, 12_000);
        assert!(!reaches.is_empty());
        for (id, reach) in reaches.iter().enumerate() {
            if let Downstream::Reach(next) = reach.downstream {
                assert!((next as usize) < reaches.len());
                assert_ne!(next as usize, id);
                assert_eq!(reach.nodes.last(), reaches[next as usize].nodes.first(),
                           "a tributary shares its junction node with its receiver");
            }
        }
        // acyclic: following downstream never revisits
        for start in 0..reaches.len() {
            let mut seen = vec![false; reaches.len()];
            let mut here = start;
            while let Downstream::Reach(next) = reaches[here].downstream {
                assert!(!seen[here], "a cycle through {here}");
                seen[here] = true;
                here = next as usize;
            }
        }
    }

    #[test]
    fn flow_never_falls_along_a_reach_and_order_never_falls_downstream() {
        let (_, _, flow, reaches) = baked(20_260_904, 12_000);
        for reach in &reaches {
            for pair in reach.nodes.windows(2) {
                assert!(flow[pair[1] as usize] >= flow[pair[0] as usize]);
            }
            if let Downstream::Reach(next) = reach.downstream {
                assert!(reaches[next as usize].order >= reach.order);
            }
        }
    }

    #[test]
    fn width_and_depth_hit_their_anchors() {
        let params = HydroParams::earth_like(0);
        assert!((width_m(params.stream_flow_m2, &params) - 3.0).abs() < 1.0e-9);
        assert!((depth_m(params.stream_flow_m2, &params) - 0.5).abs() < 1.0e-9);
        assert!(width_m(params.great_flow_m2, &params) > width_m(params.river_flow_m2, &params));
    }

    /// Closes a coverage gap: `bifurcation_ratios` must count a Horton stream once even when
    /// it is split across more than one `Reach` (a same-order reach draining into another
    /// same-order reach, with a single input, is a continuation of the reach upstream of it,
    /// not a second stream).
    ///
    /// Twelve order-1 sources pair up into 6 order-2 junctions (ids 12..=17), each fed by two
    /// order-1 reaches (giving each an order of 2, since `at_top >= 2`). Five of the junctions
    /// (12..=16) drain straight to the ocean. The sixth (17) drains into a seventh order-2
    /// reach (id 18) that has only J17 as an input -- `at_top == 1` there, so `strahler` keeps
    /// its order at 2 rather than promoting it to 3, exactly the "continues" case
    /// `bifurcation_ratios` exists to fold back into its upstream reach's count.
    ///
    /// Horton's count of order-2 streams is therefore 6, not 7: reach 18 does not start a new
    /// order-2 stream, it continues J17's. `counts[order]` in `bifurcation_ratios` implements
    /// exactly this by never incrementing for a reach whose downstream neighbour marked it as
    /// a continuation -- so N2 = 6, and the ratio is 12.0 / 6.0 = 2.0.
    #[test]
    fn bifurcation_ratios_count_horton_streams_on_a_hand_network() {
        let mut reaches = Vec::new();
        // 12 order-1 sources (ids 0..=11), pairing into junctions 12..=17.
        for pair in 0..6u32 {
            let junction = 12 + pair;
            reaches.push(Reach {
                nodes: vec![2 * pair, 100 + pair],
                class: ReachClass::Stream,
                order: 0,
                downstream: Downstream::Reach(junction),
            });
            reaches.push(Reach {
                nodes: vec![2 * pair + 1, 100 + pair],
                class: ReachClass::Stream,
                order: 0,
                downstream: Downstream::Reach(junction),
            });
        }
        // Junctions 12..=16 drain straight to the ocean.
        for junction in 12..17u32 {
            reaches.push(Reach {
                nodes: vec![200 + junction, 201 + junction],
                class: ReachClass::Stream,
                order: 0,
                downstream: Downstream::Ocean,
            });
        }
        // Junction 17 continues into reach 18: a single input, same order once ordered.
        reaches.push(Reach {
            nodes: vec![217, 218],
            class: ReachClass::Stream,
            order: 0,
            downstream: Downstream::Reach(18),
        });
        reaches.push(Reach {
            nodes: vec![218, 219],
            class: ReachClass::Stream,
            order: 0,
            downstream: Downstream::Ocean,
        });

        assert_eq!(reaches.len(), 19);
        let ordered = strahler(reaches);
        let order_of = |id: usize| ordered[id].order;
        for id in 0..12 {
            assert_eq!(order_of(id), 1, "reach {id} is an order-1 source");
        }
        for id in 12..19 {
            assert_eq!(order_of(id), 2, "reach {id} is order 2 (never promoted past a single input)");
        }

        assert_eq!(bifurcation_ratios(&ordered), vec![12.0 / 6.0]);

        // Fewer than 10 order-1 streams: no ratio is reported at all.
        let small = vec![
            Reach { nodes: vec![0, 1], class: ReachClass::Stream, order: 0, downstream: Downstream::Ocean },
            Reach { nodes: vec![2, 3], class: ReachClass::Stream, order: 0, downstream: Downstream::Ocean },
        ];
        let small = strahler(small);
        assert!(bifurcation_ratios(&small).is_empty(), "fewer than 10 order-1 streams reports nothing");
    }

    #[test]
    fn a_hand_network_has_the_expected_strahler_orders() {
        // Two order-1 sources join (order 2), a third order-1 joins that (still 2).
        let reaches = vec![
            Reach { nodes: vec![0, 4], class: ReachClass::Stream, order: 0, downstream: Downstream::Reach(3) },
            Reach { nodes: vec![1, 4], class: ReachClass::Stream, order: 0, downstream: Downstream::Reach(3) },
            Reach { nodes: vec![2, 5], class: ReachClass::Stream, order: 0, downstream: Downstream::Reach(4) },
            Reach { nodes: vec![4, 5], class: ReachClass::Stream, order: 0, downstream: Downstream::Reach(4) },
            Reach { nodes: vec![5, 6], class: ReachClass::Stream, order: 0, downstream: Downstream::Ocean },
        ];
        let ordered = strahler(reaches);
        assert_eq!(ordered.iter().map(|r| r.order).collect::<Vec<_>>(), vec![1, 1, 1, 2, 2]);
    }
}
