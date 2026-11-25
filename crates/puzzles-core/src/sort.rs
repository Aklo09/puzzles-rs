//! Port of `third_party/puzzles-c/sort.c` (upstream 616da16).
//!
//! C's `arraysort` is a heapsort: not stable, so elements its comparator
//! calls equal come out in an order of its own, not the order
//! `slice::sort_by` or `sort_unstable_by` would give. Dominosa's solver
//! sorts with comparators that can tie, and what it deduces next depends
//! on that order, so the heapsort is ported (D5: `sort` call sites with
//! possible ties). Call sites whose comparator never ties can use the
//! standard sorts instead.

use std::cmp::Ordering;

/// Sorts `items` in increasing order by `cmp` with C's heapsort, so
/// elements comparing equal end up in the same order as in C (C's
/// `arraysort`, taking a closure instead of a comparator plus `void *ctx`).
///
/// ```
/// use puzzles_core::sort::heap_sort_by;
///
/// // Sort by tens digit only: ties come out in heapsort order.
/// let mut v = [31, 12, 35, 17, 33];
/// heap_sort_by(&mut v, |a, b| (a / 10).cmp(&(b / 10)));
/// assert_eq!(v, [12, 17, 35, 33, 31]);
/// ```
#[doc(alias("arraysort", "arraysort_fn"))]
pub fn heap_sort_by<T>(items: &mut [T], mut cmp: impl FnMut(&T, &T) -> Ordering) {
    let n = items.len();
    if n < 2 {
        return;
    }
    // Build a max-heap, downheaping from the last parent back to the root.
    for i in (0..=(n - 2) / 2).rev() {
        down_heap(items, n, i, &mut cmp);
    }
    // Repeatedly move the largest element to the end of the unsorted part.
    for end in (1..n).rev() {
        items.swap(0, end);
        down_heap(items, end, 0, &mut cmp);
    }
}

/// Sinks `items[i]` until it is no smaller than its children, within the
/// heap `items[..len]` (C's `downheap`). On a tie between a node and a
/// child the node stays; on a tie between the children the left one wins.
fn down_heap<T>(
    items: &mut [T],
    len: usize,
    mut i: usize,
    cmp: &mut impl FnMut(&T, &T) -> Ordering,
) {
    loop {
        let left = 2 * i + 1;
        if left >= len {
            return;
        }
        let right = left + 1;
        let mut largest = i;
        if cmp(&items[largest], &items[left]) == Ordering::Less {
            largest = left;
        }
        if right < len && cmp(&items[largest], &items[right]) == Ordering::Less {
            largest = right;
        }
        if largest == i {
            return;
        }
        items.swap(largest, i);
        i = largest;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_like_std_when_nothing_ties() {
        let mut state = 12_345_u32;
        for len in 0..50 {
            let mut v: Vec<u32> = (0..len)
                .map(|_| {
                    state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                    state >> 8
                })
                .collect();
            let mut expected = v.clone();
            expected.sort_unstable();
            heap_sort_by(&mut v, u32::cmp);
            assert_eq!(v, expected, "length {len}");
        }
    }

    #[test]
    fn ties_come_out_in_cs_order() {
        // Golden values recorded from the C oracle (`arraysort` on the
        // same keys, comparing `value / 10` only).
        let mut v = [31, 12, 35, 17, 33, 10, 39, 14];
        heap_sort_by(&mut v, |a, b| (a / 10).cmp(&(b / 10)));
        assert_eq!(v, [14, 10, 17, 12, 35, 39, 33, 31]);
    }

    #[test]
    fn short_slices_are_left_alone() {
        let mut empty: [u8; 0] = [];
        heap_sort_by(&mut empty, u8::cmp);
        let mut one = [7];
        heap_sort_by(&mut one, |_, _| panic!("no comparisons needed"));
        assert_eq!(one, [7]);
    }
}
