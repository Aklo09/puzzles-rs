//! Differential tests for the C code replaced by crates or small ports:
//!
//! * `combi.c` against `itertools::Itertools::combinations` (used as is,
//!   no wrapper): the same combinations in the same order;
//! * `sort.c`'s `arraysort` against `puzzles_core::sort::heap_sort_by`,
//!   with comparators that tie, so the order of equal elements is checked;
//! * `tree234.c` against `sorted_vec::SortedSet` (via
//!   `puzzles_core::sorted_set`) and `find_rel`: random operation
//!   sequences, comparing every result and the whole contents after each
//!   step. Elements are ordered by key only and carry a tag, so the tests
//!   also see which of two equal elements each side keeps.

#![cfg(feature = "oracle")]

use std::cmp::Ordering;

use c_oracle::{CTree, TreeItem, c_arraysort_by_key, c_combinations};
use itertools::Itertools as _;
use proptest::prelude::*;
use puzzles_core::sort::heap_sort_by;
use puzzles_core::sorted_set::{FindOrInsert, Relation, SortedSet, find_rel};

// ---------------------------------------------------------------------
// combi.c

/// Every `r` of every `n` up to 12 (the largest C's `long` count is safe
/// for on Windows), including `r = 0` and `r = n`.
#[test]
fn combinations_match_c() {
    for n in 1..=12 {
        for r in 0..=n {
            let rust: Vec<Vec<usize>> = (0..n).combinations(r).collect();
            assert_eq!(rust, c_combinations(r, n), "{r} of {n}");
        }
    }
}

// ---------------------------------------------------------------------
// sort.c

/// Sort keys: a total order, then coarser ones with more and more ties.
const KEYS: [fn(u32) -> u32; 5] = [|v| v, |v| v / 4, |v| v / 64, |v| v % 3, |_| 0];

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]

    #[test]
    fn heap_sort_matches_arraysort(
        items in proptest::collection::vec(0..256_u32, 0..80),
        key in 0..KEYS.len(),
    ) {
        let key = KEYS[key];
        let mut rust = items.clone();
        heap_sort_by(&mut rust, |a, b| key(*a).cmp(&key(*b)));
        let mut c = items;
        c_arraysort_by_key(&mut c, key);
        prop_assert_eq!(rust, c);
    }
}

/// Every length up to 40, all elements tied: the order is pure heapsort
/// mechanics.
#[test]
fn all_tied_matches_arraysort() {
    for len in 0..=40 {
        let items: Vec<u32> = (0..len).collect();
        let mut rust = items.clone();
        heap_sort_by(&mut rust, |_, _| Ordering::Equal);
        let mut c = items;
        c_arraysort_by_key(&mut c, |_| 0);
        assert_eq!(rust, c, "length {len}");
    }
}

// ---------------------------------------------------------------------
// tree234.c

/// A set element ordered by key only, like a C comparator on one field.
#[derive(Clone, Copy, Debug)]
struct Item {
    key: u32,
    tag: u8,
}

impl PartialEq for Item {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl Eq for Item {}

impl PartialOrd for Item {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Item {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key.cmp(&other.key)
    }
}

fn pair(item: Item) -> (u32, u8) {
    (item.key, item.tag)
}

fn c_pair(item: TreeItem) -> (u32, u8) {
    (item.key, item.tag)
}

#[derive(Clone, Debug)]
enum Op {
    /// `add234`.
    Add(u32, u8),
    /// `del234` by key.
    Del(u32),
    /// `delpos234` (possibly out of range).
    DelPos(usize),
    /// `index234` (possibly out of range).
    Index(usize),
    /// `findrelpos234` with relation 0..=4 (EQ, LT, LE, GT, GE).
    FindRel(u32, u8),
}

fn op(keys: u32) -> impl Strategy<Value = Op> {
    prop_oneof![
        4 => (0..keys, any::<u8>()).prop_map(|(k, t)| Op::Add(k, t)),
        2 => (0..keys).prop_map(Op::Del),
        1 => (0..keys as usize + 2).prop_map(Op::DelPos),
        1 => (0..keys as usize + 2).prop_map(Op::Index),
        3 => (0..keys + 1, 0..5_u8).prop_map(|(k, r)| Op::FindRel(k, r)),
    ]
}

const RELATIONS: [Relation; 5] = [
    Relation::Eq,
    Relation::Lt,
    Relation::Le,
    Relation::Gt,
    Relation::Ge,
];

/// Applies `op` to both and compares the results.
fn apply(op: &Op, rust: &mut SortedSet<Item>, c: &mut CTree) -> Result<(), TestCaseError> {
    match *op {
        Op::Add(key, tag) => {
            let at = match rust.find_or_insert(Item { key, tag }) {
                FindOrInsert::Found(i) | FindOrInsert::Inserted(i) => i,
            };
            let kept = c.add(TreeItem { key, tag });
            prop_assert_eq!(pair(rust[at]), c_pair(kept), "{:?}", op);
        }
        Op::Del(key) => {
            let removed = rust.remove_item(&Item { key, tag: 0 }).map(pair);
            prop_assert_eq!(removed, c.del(key).map(c_pair), "{:?}", op);
        }
        Op::DelPos(i) => {
            let removed = (i < rust.len()).then(|| pair(rust.remove_index(i)));
            prop_assert_eq!(removed, c.del_pos(i).map(c_pair), "{:?}", op);
        }
        Op::Index(i) => {
            let found = rust.get(i).copied().map(pair);
            prop_assert_eq!(found, c.get(i).map(c_pair), "{:?}", op);
        }
        Op::FindRel(key, rel) => {
            let found = find_rel(rust, RELATIONS[usize::from(rel)], |e| e.key.cmp(&key))
                .map(|i| (pair(rust[i]), i));
            let c_found = c.find_rel(key, rel).map(|(item, i)| (c_pair(item), i));
            prop_assert_eq!(found, c_found, "{:?}", op);
        }
    }
    prop_assert_eq!(rust.len(), c.len(), "{:?}", op);
    let contents: Vec<_> = rust.iter().copied().map(pair).collect();
    let c_contents: Vec<_> = (0..c.len())
        .map(|i| c_pair(c.get(i).expect("in range")))
        .collect();
    prop_assert_eq!(contents, c_contents, "{:?}", op);
    Ok(())
}

fn compare(ops: &[Op]) -> Result<(), TestCaseError> {
    let mut rust = SortedSet::new();
    let mut c = CTree::new();
    for op in ops {
        apply(op, &mut rust, &mut c)?;
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Few keys: lots of duplicates, deletions of present keys, and
    /// searches landing on elements.
    #[test]
    fn small_key_space_matches_tree234(ops in proptest::collection::vec(op(16), 1..120)) {
        compare(&ops)?;
    }

    /// Many keys: trees deep enough to split and merge nodes at several
    /// levels.
    #[test]
    fn large_key_space_matches_tree234(ops in proptest::collection::vec(op(2000), 1..600)) {
        compare(&ops)?;
    }
}

/// Fill a tree in increasing, decreasing and interleaved orders, then
/// empty it from the front (the `while ((e = delpos234(t, 0)))` idiom),
/// from the back and from the middle.
#[test]
fn bulk_fill_and_drain_match_tree234() {
    let n = 500_u32;
    let orders: [Vec<u32>; 3] = [
        (0..n).collect(),
        (0..n).rev().collect(),
        (0..n).map(|i| i * 7 % n).collect(),
    ];
    for order in &orders {
        for drain in 0..3 {
            let mut ops: Vec<Op> = order.iter().map(|&k| Op::Add(k, (k % 251) as u8)).collect();
            ops.extend((0..n as usize).map(|i| match drain {
                0 => Op::DelPos(0),
                1 => Op::DelPos(n as usize - 1 - i),
                _ => Op::DelPos((n as usize - i) / 2),
            }));
            ops.push(Op::DelPos(0));
            compare(&ops).expect("parity");
        }
    }
}
