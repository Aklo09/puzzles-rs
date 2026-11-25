//! Replacement for `third_party/puzzles-c/tree234.c` (upstream 616da16)
//! where the C code uses positions.
//!
//! tree234 is a counted 2-3-4 tree: a sorted set that can also find,
//! fetch and delete elements by position. Where a C file only adds,
//! deletes and finds, the port uses `BTreeSet`/`BTreeMap`. Where it uses
//! positions (`index234`, `delpos234`, `findrelpos234`: flip, loopgen,
//! net, netslide, pegs, penrose, spectre, untangle), it uses
//! [`SortedSet`] from the `sorted-vec` crate, re-exported here:
//!
//! | C | Rust |
//! |---|---|
//! | `newtree234(cmp)` | `SortedSet::new()`, with `cmp` as the element type's `Ord` |
//! | `add234(t, e)` | `set.find_or_insert(e)`: like C, an equal element already present is kept |
//! | `del234(t, e)` | `set.remove_item(&e)` |
//! | `index234(t, i)` | `set.get(i)` (the set derefs to a sorted slice) |
//! | `delpos234(t, i)` | `set.remove_index(i)` (check `i < set.len()` first: C returns NULL) |
//! | `count234(t)` | `set.len()` |
//! | `find234`, `findrel234`, `findrelpos234` | [`find_rel`] |
//!
//! A sorted set's order is fixed by its comparator, so positions match C
//! exactly (D5: a uniquely determined result, checked against `tree234.c`
//! by the oracle tests).

use std::cmp::Ordering;

pub use sorted_vec::{FindOrInsert, SortedSet};

/// Which element [`find_rel`] looks for, relative to the key (C's
/// `REL234_*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relation {
    /// The element equal to the key.
    #[doc(alias = "REL234_EQ")]
    Eq,
    /// The greatest element less than the key.
    #[doc(alias = "REL234_LT")]
    Lt,
    /// The element equal to the key, or else the greatest one less.
    #[doc(alias = "REL234_LE")]
    Le,
    /// The least element greater than the key.
    #[doc(alias = "REL234_GT")]
    Gt,
    /// The element equal to the key, or else the least one greater.
    #[doc(alias = "REL234_GE")]
    Ge,
}

/// Position in the sorted slice `items` of the element in `relation` to a
/// key (C's `findrelpos234`; with `Relation::Eq`, `find234` /
/// `findpos234`). `cmp(element)` orders an element against the key, as
/// `element.cmp(key)` would.
///
/// `cmp` must order the elements the way `items` is sorted, and at most
/// one element may compare equal to the key. That holds whenever the key
/// is compared with the set's own ordering, which is how every C caller
/// with positions uses it (they pass a NULL comparator). If several
/// elements compared equal, C would return whichever its tree descent met
/// first, which depends on the tree's shape.
///
/// C's NULL key (first or last element) is `items.first()` /
/// `items.last()`.
///
/// ```
/// use puzzles_core::sorted_set::{Relation, find_rel};
///
/// let items = [10, 20, 30];
/// assert_eq!(find_rel(&items, Relation::Lt, |e| e.cmp(&20)), Some(0));
/// assert_eq!(find_rel(&items, Relation::Le, |e| e.cmp(&25)), Some(1));
/// assert_eq!(find_rel(&items, Relation::Ge, |e| e.cmp(&25)), Some(2));
/// assert_eq!(find_rel(&items, Relation::Gt, |e| e.cmp(&30)), None);
/// assert_eq!(find_rel(&items, Relation::Eq, |e| e.cmp(&20)), Some(1));
/// ```
#[doc(alias("find234", "findpos234", "findrel234", "findrelpos234"))]
pub fn find_rel<T>(
    items: &[T],
    relation: Relation,
    mut cmp: impl FnMut(&T) -> Ordering,
) -> Option<usize> {
    // Elements before `below` are less than the key; before `upto`, not
    // greater.
    let below = items.partition_point(|e| cmp(e) == Ordering::Less);
    let upto = items.partition_point(|e| cmp(e) != Ordering::Greater);
    match relation {
        Relation::Eq => (below < upto).then_some(below),
        Relation::Lt => below.checked_sub(1),
        Relation::Le => upto.checked_sub(1),
        Relation::Gt => (upto < items.len()).then_some(upto),
        Relation::Ge => (below < items.len()).then_some(below),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relations_on_present_and_absent_keys() {
        let items = [10, 20, 30];
        let at = |rel, key: i32| find_rel(&items, rel, |e| e.cmp(&key));
        for (key, eq, lt, le, gt, ge) in [
            (5, None, None, None, Some(0), Some(0)),
            (10, Some(0), None, Some(0), Some(1), Some(0)),
            (15, None, Some(0), Some(0), Some(1), Some(1)),
            (20, Some(1), Some(0), Some(1), Some(2), Some(1)),
            (30, Some(2), Some(1), Some(2), None, Some(2)),
            (35, None, Some(2), Some(2), None, None),
        ] {
            assert_eq!(at(Relation::Eq, key), eq, "EQ {key}");
            assert_eq!(at(Relation::Lt, key), lt, "LT {key}");
            assert_eq!(at(Relation::Le, key), le, "LE {key}");
            assert_eq!(at(Relation::Gt, key), gt, "GT {key}");
            assert_eq!(at(Relation::Ge, key), ge, "GE {key}");
        }
    }

    #[test]
    fn empty_slice_finds_nothing() {
        let items: [i32; 0] = [];
        for rel in [
            Relation::Eq,
            Relation::Lt,
            Relation::Le,
            Relation::Gt,
            Relation::Ge,
        ] {
            assert_eq!(find_rel(&items, rel, |e| e.cmp(&1)), None);
        }
    }

    #[test]
    fn add_keeps_the_element_already_present() {
        // Ordered by key only, like a C comparator on one field.
        #[derive(Debug)]
        struct Item(u32, &'static str);
        impl PartialEq for Item {
            fn eq(&self, other: &Self) -> bool {
                self.0 == other.0
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
                self.0.cmp(&other.0)
            }
        }

        let mut set = SortedSet::new();
        assert_eq!(
            set.find_or_insert(Item(2, "first")),
            FindOrInsert::Inserted(0)
        );
        assert_eq!(
            set.find_or_insert(Item(1, "low")),
            FindOrInsert::Inserted(0)
        );
        assert_eq!(
            set.find_or_insert(Item(2, "second")),
            FindOrInsert::Found(1)
        );
        assert_eq!(set[1].1, "first");
        assert_eq!(set.len(), 2);
    }
}
