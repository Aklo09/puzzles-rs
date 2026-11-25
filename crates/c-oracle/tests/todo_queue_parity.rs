//! Differential tests: the Rust `TodoQueue` against C's `tdq.c`.
//!
//! Pearl's generator pops elements in queue order, so the order must match
//! C exactly, including after the C ring buffer wraps around.

#![cfg(feature = "oracle")]

use c_oracle::CTdq;
use proptest::prelude::*;
use puzzles_core::todo_queue::TodoQueue;

const MAX_LEN: usize = 30;

#[derive(Clone, Debug)]
enum Op {
    /// Index reduced modulo the queue's capacity.
    Push(usize),
    Pop,
    Fill,
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        6 => (0..MAX_LEN).prop_map(Op::Push),
        5 => Just(Op::Pop),
        1 => Just(Op::Fill),
    ]
}

proptest! {
    #[test]
    fn any_operation_sequence_pops_like_c(
        len in 1..=MAX_LEN,
        ops in proptest::collection::vec(op(), 0..200),
    ) {
        let mut rust = TodoQueue::new(len);
        let mut c = CTdq::new(len);
        for op in &ops {
            match *op {
                Op::Push(k) => {
                    rust.push(k % len);
                    c.add(k % len);
                }
                Op::Pop => prop_assert_eq!(rust.pop(), c.remove()),
                Op::Fill => {
                    rust.fill();
                    c.fill();
                }
            }
        }
        // Drain both: the remaining contents and their order must match.
        let rust_rest: Vec<usize> = std::iter::from_fn(|| rust.pop()).collect();
        let c_rest: Vec<usize> = std::iter::from_fn(|| c.remove()).collect();
        prop_assert_eq!(rust_rest, c_rest);
    }
}

/// Keeps the queue full while cycling elements through it many times, so
/// the C ring buffer wraps around repeatedly.
#[test]
fn queue_kept_full_across_many_wraparounds_matches_c() {
    let len = 7;
    let mut rust = TodoQueue::new(len);
    let mut c = CTdq::new(len);
    rust.fill();
    c.fill();
    for step in 0..1000 {
        let (r, k) = (rust.pop(), c.remove());
        assert_eq!(r, k, "step {step}");
        let k = k.expect("queue is never empty here");
        // Re-add the popped element and one more (usually already queued).
        for add in [k, (k * 3 + step) % len] {
            rust.push(add);
            c.add(add);
        }
    }
}
