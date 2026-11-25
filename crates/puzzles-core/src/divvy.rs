//! Port of `third_party/puzzles-c/divvy.c` (upstream 616da16).
//!
//! Random division of a rectangle into equally sized polyominoes, used by
//! Palisade for its regions and by Solo for jigsaw blocks. Which division
//! comes out decides the puzzle, so this is a faithful port (D5 exception):
//! the same shuffle, the same draws and the same search as C, checked
//! against it by the oracle tests.
//!
//! Every polyomino stays *simply connected* (it never surrounds a square
//! that isn't its own). That is what lets one local test, on the 8
//! neighbours of a square, decide both whether the square can join a
//! polyomino without enclosing a hole and whether it can leave one
//! without splitting it. The C file's header comment discusses what that
//! costs (for k ≥ 23 some divisions can never be produced).

use crate::dsf::{DisjointSets, Dsf};
use crate::random::RandomState;

/// Divides a `w` × `h` rectangle at random into polyominoes of `k`
/// squares each, retrying until an attempt succeeds (C's
/// `divvy_rectangle`). See [`try_divvy_rectangle`].
///
/// Each attempt fails only rarely, and retries keep drawing from `rng`,
/// so the result is still determined by the generator's state.
///
/// ```
/// use puzzles_core::divvy::divvy_rectangle;
/// use puzzles_core::dsf::DisjointSets;
/// use puzzles_core::random::RandomState;
///
/// let mut blocks = divvy_rectangle(6, 4, 4, &mut RandomState::new(b"regions"));
/// assert!((0..24).all(|i| blocks.set_size(i) == 4));
/// ```
///
/// # Panics
/// If `k` is 0 or doesn't divide `w * h` (C asserts), or `w * h`
/// overflows `usize`.
#[must_use]
pub fn divvy_rectangle(w: usize, h: usize, k: usize, rng: &mut RandomState) -> Dsf {
    loop {
        if let Some(dsf) = try_divvy_rectangle(w, h, k, rng) {
            return dsf;
        }
    }
}

/// Owner of a square while the rectangle is being divided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Owner {
    /// Not part of any polyomino yet.
    Unclaimed,
    /// Part of polyomino `n`.
    Omino(usize),
    /// Taken out of its polyomino for the moment, while the search checks
    /// how that polyomino could grow without it.
    Lifted,
}

/// How the breadth-first search over polyominoes reached one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Visit {
    Unvisited,
    /// The polyomino we are trying to grow.
    Start,
    /// Polyomino `from` grows by stealing `square` from this one.
    Stolen {
        from: usize,
        square: usize,
    },
}

/// The 8 neighbours of a square in cyclic order, starting west and going
/// clockwise (C's `dir` 0–7 in `addremcommon`). The even entries are the
/// 4 orthogonal neighbours.
const AROUND: [(isize, isize); 8] = [
    (-1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
];

/// The 4 orthogonal neighbours: west, east, north, south.
const ORTHOGONAL: [(isize, isize); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];

/// The square at offset `(dx, dy)` from `(x, y)`, if it is inside the grid.
fn neighbour(w: usize, h: usize, x: usize, y: usize, (dx, dy): (isize, isize)) -> Option<usize> {
    let sx = x.checked_add_signed(dx).filter(|&sx| sx < w)?;
    let sy = y.checked_add_signed(dy).filter(|&sy| sy < h)?;
    Some(sy * w + sx)
}

/// Whether square `(x, y)` borders polyomino `omino` in a way that lets it
/// join without enclosing a hole, or equally (for a square of `omino`)
/// leave without splitting it (C's `addremcommon`).
///
/// The square must be orthogonally adjacent to `omino`, and going round
/// its 8 neighbours in cyclic order must cross between `omino` and
/// not-`omino` exactly twice.
fn can_add_or_remove(w: usize, h: usize, x: usize, y: usize, own: &[Owner], omino: usize) -> bool {
    let ours = AROUND.map(|offset| {
        neighbour(w, h, x, y, offset).is_some_and(|sq| own[sq] == Owner::Omino(omino))
    });
    if !(ours[0] || ours[2] || ours[4] || ours[6]) {
        return false;
    }
    let crossings = (0..8).filter(|&d| ours[d] != ours[(d + 1) % 8]).count();
    crossings == 2
}

/// One attempt at dividing a `w` × `h` rectangle at random into
/// polyominoes of `k` squares each (C's `divvy_rectangle_attempt`).
/// Returns `None` if this attempt got stuck; [`divvy_rectangle`] retries
/// until one succeeds.
///
/// The result has one set per polyomino; its representatives are the same
/// elements C's `dsf_canonify` returns.
///
/// The squares are shuffled once, and that order is used for every scan
/// of the grid. The first `w * h / k` squares in it seed one polyomino
/// each. Then, until all are complete, a random incomplete polyomino is
/// grown by one square: a breadth-first search over polyominoes looks for
/// a free square it can take, or else a square it can steal from a
/// polyomino not yet searched, which must then grow in turn. The chain of
/// steals found ends in a free square, so each round claims one more.
/// The attempt fails if the search runs out of polyominoes.
///
/// An empty rectangle (`w` or `h` is 0, where C asserts) gives an empty
/// [`Dsf`] without drawing from `rng`.
///
/// # Panics
/// If `k` is 0 or doesn't divide `w * h` (C asserts), or `w * h`
/// overflows `usize`.
#[doc(alias = "divvy_rectangle_attempt")]
#[must_use]
pub fn try_divvy_rectangle(w: usize, h: usize, k: usize, rng: &mut RandomState) -> Option<Dsf> {
    let wh = w.checked_mul(h).expect("grid size overflows usize");
    assert!(k > 0, "polyomino size must be positive");
    let n = wh / k;
    assert_eq!(wh, k * n, "polyomino size {k} doesn't divide {w}x{h}");
    if wh == 0 {
        return Some(Dsf::new(0));
    }

    // A random order for every scan of the grid: no directional bias.
    let mut order: Vec<usize> = (0..wh).collect();
    rng.shuffle(&mut order);

    let mut own = vec![Owner::Unclaimed; wh];
    for (omino, &square) in order[..n].iter().enumerate() {
        own[square] = Owner::Omino(omino);
    }
    let mut sizes = vec![1; n];

    // Per round: which squares may leave their polyomino, and for each
    // square and direction, the neighbouring polyomino it may join.
    let mut removable = vec![false; wh];
    let mut addable = vec![[None::<usize>; 4]; wh];
    let mut visits = vec![Visit::Unvisited; n];
    let mut queue = Vec::with_capacity(n);
    let mut incomplete = Vec::with_capacity(n);
    loop {
        // Ignores other polyominoes, so a square is often both removable
        // from one and addable to another: it can be stolen.
        for y in 0..h {
            for x in 0..w {
                let yx = y * w + x;
                removable[yx] = match own[yx] {
                    Owner::Omino(o) if sizes[o] == 1 => true,
                    Owner::Omino(o) => can_add_or_remove(w, h, x, y, &own, o),
                    Owner::Unclaimed | Owner::Lifted => false,
                };
                addable[yx] = ORTHOGONAL.map(|offset| {
                    let Owner::Omino(other) = own[neighbour(w, h, x, y, offset)?] else {
                        return None;
                    };
                    (own[yx] != Owner::Omino(other) && can_add_or_remove(w, h, x, y, &own, other))
                        .then_some(other)
                });
            }
        }

        incomplete.clear();
        incomplete.extend((0..n).filter(|&o| sizes[o] < k));
        if incomplete.is_empty() {
            break;
        }
        let limit = u32::try_from(incomplete.len()).expect("too many polyominoes");
        let start = incomplete[rng.upto(limit) as usize];

        // Is `square` still addable to `omino` now that the square stolen
        // from it (if any) is lifted? The top-of-round table says whether
        // it borders `omino` suitably; the neighbourhood test is redone.
        let addable_now = |own: &[Owner], square: usize, omino: usize| {
            addable[square].contains(&Some(omino))
                && can_add_or_remove(w, h, square % w, square / w, own, omino)
        };

        visits.fill(Visit::Unvisited);
        visits[start] = Visit::Start;
        queue.clear();
        queue.push(start);
        let mut head = 0;
        let mut grown = false;
        while head < queue.len() {
            let omino = queue[head];
            head += 1;

            // If we got here by `omino` losing a square, lift that square
            // so the adjacency tests see `omino` without it.
            let stolen = match visits[omino] {
                Visit::Stolen { square, .. } => {
                    assert_eq!(own[square], Owner::Omino(omino));
                    own[square] = Owner::Lifted;
                    Some(square)
                }
                Visit::Start | Visit::Unvisited => None,
            };

            // A free square `omino` can take ends the search. A singleton
            // that lost its square is empty, so it can take any.
            let free = order.iter().copied().find(|&sq| {
                own[sq] == Owner::Unclaimed
                    && ((sizes[omino] == 1 && stolen.is_some()) || addable_now(&own, sq, omino))
            });
            if let Some(mut square) = free {
                if let Some(stolen) = stolen {
                    own[stolen] = Owner::Omino(omino);
                }
                // Walk the chain back to the start: each polyomino takes
                // the square it was offered, having lost one to the next.
                let mut taker = omino;
                loop {
                    own[square] = Owner::Omino(taker);
                    match visits[taker] {
                        Visit::Start => break,
                        Visit::Stolen { from, square: lost } => {
                            square = lost;
                            taker = from;
                        }
                        Visit::Unvisited => unreachable!("chain runs through visited polyominoes"),
                    }
                }
                sizes[taker] += 1;
                grown = true;
                break;
            }

            // Otherwise queue each unsearched polyomino with a square that
            // can move to `omino` (the first such square in `order`).
            for &sq in &order {
                let Owner::Omino(victim) = own[sq] else {
                    continue;
                };
                if visits[victim] == Visit::Unvisited
                    && removable[sq]
                    && addable_now(&own, sq, omino)
                {
                    assert!(queue.len() < n, "search queue overflow");
                    queue.push(victim);
                    visits[victim] = Visit::Stolen {
                        from: omino,
                        square: sq,
                    };
                }
            }

            if let Some(stolen) = stolen {
                own[stolen] = Owner::Omino(omino);
            }
        }
        if !grown {
            return None;
        }
    }

    let owners: Vec<usize> = own
        .iter()
        .map(|&o| match o {
            Owner::Omino(o) => o,
            Owner::Unclaimed | Owner::Lifted => unreachable!("every square is owned"),
        })
        .collect();

    // Join each square to the last square of its polyomino, in C's order,
    // so the representatives come out as in C.
    let mut last = vec![0; n];
    for (square, &o) in owners.iter().enumerate() {
        last[o] = square;
    }
    let mut dsf = Dsf::new(wh);
    for (square, &o) in owners.iter().enumerate() {
        dsf.union(square, last[o]);
    }

    // Check again from adjacency that no polyomino came out in pieces.
    let mut pieces = Dsf::new(wh);
    for y in 0..h {
        for x in 0..w {
            let yx = y * w + x;
            if x + 1 < w && owners[yx] == owners[yx + 1] {
                pieces.union(yx, yx + 1);
            }
            if y + 1 < h && owners[yx] == owners[yx + w] {
                pieces.union(yx, yx + w);
            }
        }
    }
    for square in 0..wh {
        let root = dsf.find(square);
        assert!(pieces.same_set(root, square), "polyomino split in two");
    }
    Some(dsf)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every set has `k` squares and is connected orthogonally.
    fn check(w: usize, h: usize, k: usize, dsf: &mut Dsf) {
        assert_eq!(dsf.len(), w * h);
        let mut pieces = Dsf::new(w * h);
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                assert_eq!(dsf.set_size(i), k, "{w}x{h}/{k}: square {i}");
                if x + 1 < w && dsf.same_set(i, i + 1) {
                    pieces.union(i, i + 1);
                }
                if y + 1 < h && dsf.same_set(i, i + w) {
                    pieces.union(i, i + w);
                }
            }
        }
        for i in 0..w * h {
            assert_eq!(
                pieces.set_size(i),
                k,
                "{w}x{h}/{k}: square {i} disconnected"
            );
        }
    }

    #[test]
    fn empty_rectangle_gives_empty_dsf() {
        let mut rng = RandomState::new(b"empty");
        assert!(divvy_rectangle(0, 4, 2, &mut rng).is_empty());
        assert!(divvy_rectangle(4, 0, 3, &mut rng).is_empty());
        assert_eq!(rng, RandomState::new(b"empty"));
    }

    #[test]
    fn size_one_gives_singletons() {
        let mut dsf = divvy_rectangle(3, 2, 1, &mut RandomState::new(b"one"));
        assert!((0..6).all(|i| dsf.find(i) == i));
    }

    #[test]
    fn whole_rectangle_is_one_set() {
        let mut dsf = divvy_rectangle(4, 3, 12, &mut RandomState::new(b"all"));
        assert!((0..12).all(|i| dsf.set_size(i) == 12));
    }

    #[test]
    #[should_panic(expected = "doesn't divide")]
    fn size_must_divide_area() {
        let _ = divvy_rectangle(3, 3, 2, &mut RandomState::new(b"odd"));
    }

    #[test]
    #[should_panic(expected = "must be positive")]
    fn size_zero_panics() {
        let _ = divvy_rectangle(3, 3, 0, &mut RandomState::new(b"zero"));
    }

    #[test]
    fn divisions_are_valid() {
        let mut rng = RandomState::new(b"valid");
        for w in 1..=8_usize {
            for h in 1..=8 {
                for k in (1..=w * h).filter(|&k| (w * h).is_multiple_of(k)) {
                    check(w, h, k, &mut divvy_rectangle(w, h, k, &mut rng));
                }
            }
        }
    }

    #[test]
    fn divisions_are_cs() {
        // Golden values recorded from the C oracle: every square's
        // representative, then the generator's next 32 bits (so the draws
        // used were the same).
        let reps = |dsf: &mut Dsf| (0..dsf.len()).map(|i| dsf.find(i)).collect::<Vec<_>>();

        let mut rng = RandomState::new(b"golden");
        let mut dsf = divvy_rectangle(4, 3, 4, &mut rng);
        assert_eq!(reps(&mut dsf), [8, 7, 7, 7, 8, 8, 11, 7, 8, 11, 11, 11]);
        assert_eq!(rng.bits(32), 3_463_172_180);

        let mut rng = RandomState::new(b"golden");
        let mut dsf = divvy_rectangle(5, 5, 5, &mut rng);
        assert_eq!(
            reps(&mut dsf),
            [
                6, 6, 6, 13, 24, 6, 6, 13, 13, 24, 21, 21, 13, 13, 24, 21, 23, 23, 23, 24, 21, 21,
                23, 23, 24
            ]
        );
        assert_eq!(rng.bits(32), 1_140_769_779);
    }

    #[test]
    fn jigsaw_and_palisade_sizes_are_valid() {
        // Solo's jigsaw blocks (cr x cr into cr-ominoes) and a large
        // Palisade grid.
        let mut rng = RandomState::new(b"jigsaw");
        for cr in [4, 6, 9, 12] {
            check(cr, cr, cr, &mut divvy_rectangle(cr, cr, cr, &mut rng));
        }
        check(15, 12, 5, &mut divvy_rectangle(15, 12, 5, &mut rng));
    }
}
