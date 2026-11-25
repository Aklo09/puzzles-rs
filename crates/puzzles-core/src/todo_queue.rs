//! Port of `third_party/puzzles-c/tdq.c` (upstream 616da16).
//!
//! A to-do queue for solvers: a FIFO of element indices `0..n` in which
//! each element appears at most once. Adding an element that is already
//! queued does nothing, so it keeps the position where it was *first*
//! added. Typical use: when a cell changes, queue its neighbours for
//! re-examination, and keep popping until the queue is empty.
//!
//! **Compatibility:** Pearl's generator uses this, so the pop order must
//! match C exactly. C hand-rolls a ring buffer; a [`VecDeque`] gives the
//! same FIFO order.

use std::collections::VecDeque;

/// De-duplicating FIFO of indices in `0..capacity` (C's `tdq`).
///
/// ```
/// use puzzles_core::todo_queue::TodoQueue;
///
/// let mut todo = TodoQueue::new(5);
/// todo.push(3);
/// todo.push(1);
/// todo.push(3); // already queued: ignored
/// assert_eq!(todo.pop(), Some(3));
/// assert_eq!(todo.pop(), Some(1));
/// assert_eq!(todo.pop(), None);
/// ```
#[doc(alias = "tdq")]
#[derive(Clone, Debug)]
pub struct TodoQueue {
    queue: VecDeque<usize>,
    /// Whether each element is currently queued.
    queued: Vec<bool>,
}

impl TodoQueue {
    /// Creates an empty queue for elements `0..capacity` (C's `tdq_new`).
    #[doc(alias = "tdq_new")]
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            queue: VecDeque::with_capacity(capacity),
            queued: vec![false; capacity],
        }
    }

    /// Adds `k` at the back, unless it is already queued (C's `tdq_add`).
    ///
    /// # Panics
    /// If `k` is not below the queue's capacity.
    #[doc(alias = "tdq_add")]
    pub fn push(&mut self, k: usize) {
        if !self.queued[k] {
            self.queued[k] = true;
            self.queue.push_back(k);
        }
    }

    /// Removes and returns the front element, or `None` if the queue is
    /// empty (C's `tdq_remove`, which returns -1 when empty).
    #[doc(alias = "tdq_remove")]
    pub fn pop(&mut self) -> Option<usize> {
        let k = self.queue.pop_front()?;
        self.queued[k] = false;
        Some(k)
    }

    /// Queues every element not already queued, in increasing order (C's
    /// `tdq_fill`).
    #[doc(alias = "tdq_fill")]
    pub fn fill(&mut self) {
        for k in 0..self.queued.len() {
            self.push(k);
        }
    }

    /// True if `k` is currently queued.
    ///
    /// # Panics
    /// If `k` is not below the queue's capacity.
    #[must_use]
    pub fn contains(&self, k: usize) -> bool {
        self.queued[k]
    }

    /// Number of queued elements.
    #[must_use]
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// True if nothing is queued.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(todo: &mut TodoQueue) -> Vec<usize> {
        std::iter::from_fn(|| todo.pop()).collect()
    }

    #[test]
    fn pops_in_insertion_order() {
        let mut todo = TodoQueue::new(10);
        for k in [7, 2, 9, 0] {
            todo.push(k);
        }
        assert_eq!(drain(&mut todo), [7, 2, 9, 0]);
    }

    #[test]
    fn readding_a_queued_element_keeps_its_first_position() {
        let mut todo = TodoQueue::new(4);
        for k in [1, 2, 1, 3, 2] {
            todo.push(k);
        }
        assert_eq!(drain(&mut todo), [1, 2, 3]);
    }

    #[test]
    fn element_can_be_requeued_after_popping() {
        let mut todo = TodoQueue::new(3);
        todo.push(1);
        todo.push(2);
        assert_eq!(todo.pop(), Some(1));
        todo.push(1);
        assert_eq!(drain(&mut todo), [2, 1]);
    }

    #[test]
    fn pop_on_empty_queue_returns_none() {
        assert_eq!(TodoQueue::new(3).pop(), None);
    }

    #[test]
    fn fill_appends_missing_elements_in_order() {
        let mut todo = TodoQueue::new(5);
        todo.push(3);
        todo.fill();
        assert_eq!(drain(&mut todo), [3, 0, 1, 2, 4]);
    }

    #[test]
    fn full_queue_then_cycle_keeps_fifo_order() {
        // The C ring buffer wraps around here; order must still be FIFO.
        let mut todo = TodoQueue::new(3);
        todo.fill();
        assert_eq!(todo.pop(), Some(0));
        todo.push(0);
        assert_eq!(todo.pop(), Some(1));
        todo.push(1);
        assert_eq!(drain(&mut todo), [2, 0, 1]);
    }

    #[test]
    fn len_and_contains_track_queued_elements() {
        let mut todo = TodoQueue::new(4);
        todo.push(2);
        todo.push(2);
        assert_eq!(
            (todo.len(), todo.contains(2), todo.contains(1)),
            (1, true, false)
        );
    }

    #[test]
    #[should_panic(expected = "out of bounds")]
    fn push_panics_on_out_of_range_element() {
        TodoQueue::new(3).push(3);
    }
}
