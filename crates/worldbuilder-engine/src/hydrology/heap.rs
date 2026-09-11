//! A min-priority queue keyed by height, with one fixed order on every build.
//!
//! **Float keys are integers here.** A heap that compares `f64`s through `partial_cmp` has
//! to decide what NaN means, and one that compares them at all leaves the order to the
//! instruction stream. `sortable` maps every finite `f64` to a `u64` that orders the way the
//! number does, negatives included, and the node index breaks every tie. Two builds, native
//! and wasm, therefore pop the same node at the same moment.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// An `f64` as a `u64` that sorts the way the number does. Callers never pass NaN.
pub fn sortable(value: f64) -> u64 {
    let bits = value.to_bits();
    if bits >> 63 == 1 {
        !bits
    } else {
        bits | (1u64 << 63)
    }
}

/// The inverse of `sortable`.
pub fn unsortable(key: u64) -> f64 {
    let bits = if key >> 63 == 1 { key & !(1u64 << 63) } else { !key };
    f64::from_bits(bits)
}

/// Lowest level first; equal levels pop in ascending tie order, then ascending node order.
#[derive(Debug, Default)]
pub struct FloodQueue {
    heap: BinaryHeap<Reverse<(u64, u64, u32)>>,
}

impl FloodQueue {
    pub fn new() -> Self {
        Self { heap: BinaryHeap::new() }
    }

    /// Push with an explicit tie-break key, compared after `level_m` and before `node`.
    pub fn push_tied(&mut self, level_m: f64, tie_m: f64, node: u32) {
        self.heap.push(Reverse((sortable(level_m), sortable(tie_m), node)));
    }

    pub fn push(&mut self, level_m: f64, node: u32) {
        self.push_tied(level_m, level_m, node);
    }

    pub fn pop(&mut self) -> Option<(f64, u32)> {
        self.heap.pop().map(|Reverse((key, _tie, node))| (unsortable(key), node))
    }

    pub fn len(&self) -> usize {
        self.heap.len()
    }

    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sortable_keys_order_like_the_numbers() {
        let values = [-1.0e9, -1.0, -1.0e-300, 0.0, 1.0e-300, 1.0, 1.0e9];
        for pair in values.windows(2) {
            assert!(sortable(pair[0]) < sortable(pair[1]), "{} < {}", pair[0], pair[1]);
        }
    }

    #[test]
    fn sortable_round_trips_bit_for_bit() {
        for value in [-1234.5678, -0.0, 0.0, 3.25, f64::MAX, f64::MIN, 1.0e-310] {
            assert_eq!(unsortable(sortable(value)).to_bits(), value.to_bits());
        }
    }

    #[test]
    fn the_queue_pops_lowest_first_and_breaks_ties_by_node() {
        let mut queue = FloodQueue::new();
        queue.push(5.0, 7);
        queue.push(-2.0, 9);
        queue.push(5.0, 3);
        queue.push(1.0, 1);
        let order: Vec<(f64, u32)> = std::iter::from_fn(|| queue.pop()).collect();
        assert_eq!(order, vec![(-2.0, 9), (1.0, 1), (5.0, 3), (5.0, 7)]);
        assert!(queue.is_empty());
    }

    #[test]
    fn ties_break_by_ground_then_node() {
        let mut queue = FloodQueue::new();
        queue.push_tied(5.0, 3.0, 7);
        queue.push_tied(5.0, 1.0, 9);
        queue.push_tied(5.0, 1.0, 2);
        let order: Vec<u32> = std::iter::from_fn(|| queue.pop()).map(|(_, node)| node).collect();
        assert_eq!(order, vec![2, 9, 7]);
    }
}
