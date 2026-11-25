//! Port of `third_party/puzzles-c/laydomino.c` (upstream 616da16).
//!
//! Random tiling of a rectangle with dominoes (2×1 tiles), used by
//! Dominosa and Magnets to lay out their grids. Which tiling comes out
//! decides the puzzle, so this is a faithful port (D5 exception): the same
//! shuffles and the same search as C, checked against it by the oracle
//! tests.

use crate::random::RandomState;

/// Lays dominoes over a `w` × `h` grid at random and returns, for each
/// square `y * w + x`, the square at the other end of its domino (C's
/// `domino_layout`, and `domino_layout_prealloc`, whose caller-supplied
/// buffers are an allocation detail).
///
/// If `w * h` is odd, one square is left over and refers to itself.
/// An empty grid (`w` or `h` is 0, where C asserts) gives an empty layout.
///
/// Dominoes are first dropped on the positions of a shuffled list of all
/// possible placements wherever both squares are free. The leftover
/// squares are then paired off: a breadth-first search from one of them
/// walks through dominoes (entering one end and leaving by the other)
/// until it is next to another leftover square, and every domino on that
/// path shifts along by one square. Each square's neighbours are tried in
/// a random order.
///
/// ```
/// use puzzles_core::laydomino::domino_layout;
/// use puzzles_core::random::RandomState;
///
/// let layout = domino_layout(3, 3, &mut RandomState::new(b"dominoes"));
/// let leftover = (0..9).filter(|&i| layout[i] == i).count();
/// assert_eq!(leftover, 1);
/// assert!((0..9).all(|i| layout[layout[i]] == i));
/// ```
///
/// # Panics
/// If `w * h` overflows `usize`, or the grid has `2^28` placements or more
/// (see [`RandomState::shuffle`]).
#[doc(alias = "domino_layout_prealloc")]
#[must_use]
pub fn domino_layout(w: usize, h: usize, rng: &mut RandomState) -> Vec<usize> {
    let wh = w.checked_mul(h).expect("grid size overflows usize");
    // grid[i] == i: square i is not covered yet (a singleton).
    let mut grid: Vec<usize> = (0..wh).collect();
    if wh == 0 {
        return grid;
    }

    // Every possible placement: 2 * i for the vertical domino with its top
    // at square i, 2 * i + 1 for the horizontal one with its left half at
    // square i. Verticals first, both in reading order.
    let mut placements = Vec::with_capacity(2 * wh - w - h);
    for y in 0..h - 1 {
        placements.extend((0..w).map(|x| 2 * (y * w + x)));
    }
    for y in 0..h {
        placements.extend((0..w - 1).map(|x| 2 * (y * w + x) + 1));
    }
    rng.shuffle(&mut placements);

    for &placement in &placements {
        let a = placement / 2;
        let b = a + if placement % 2 == 1 { 1 } else { w };
        if grid[a] == a && grid[b] == b {
            grid[a] = b;
            grid[b] = a;
        }
    }

    // Search state, one array as in C with two meanings: on a square where
    // the search leaves a domino (or starts), the distance from the start;
    // on a square where it enters one (or the target), the square it came
    // from. They never collide: in chessboard colours, the first kind all
    // share the start's colour and the second kind the other colour.
    let mut trail: Vec<Option<usize>> = vec![None; wh];
    let mut queue: Vec<usize> = Vec::with_capacity(wh);
    loop {
        // Start from the last singleton; stop when only the one an odd
        // area must have (or none) is left.
        let mut singletons = 0;
        let mut start = 0;
        for (square, &other) in grid.iter().enumerate() {
            if other == square {
                singletons += 1;
                start = square;
            }
        }
        if singletons == wh % 2 {
            break;
        }

        trail.fill(None);
        trail[start] = Some(0);
        queue.clear();
        queue.push(start);
        let mut done = 0;
        let mut target = None;
        'search: while done < queue.len() {
            let i = queue[done];
            done += 1;

            let (x, y) = (i % w, i / w);
            let mut neighbours = [0; 4];
            let mut count = 0;
            for (exists, square) in [
                (x > 0, i.wrapping_sub(1)),
                (x + 1 < w, i + 1),
                (y > 0, i.wrapping_sub(w)),
                (y + 1 < h, i + w),
            ] {
                if exists {
                    neighbours[count] = square;
                    count += 1;
                }
            }
            let neighbours = &mut neighbours[..count];
            // A random order, to avoid directional bias.
            rng.shuffle(neighbours);

            for &k in neighbours.iter() {
                if grid[k] == k {
                    trail[k] = Some(i);
                    target = Some(k);
                    break 'search;
                }
                // Through the domino k-m: k records where we came from, m
                // its distance, if that is new or shorter. (Never shorter:
                // the queue is in distance order. C checks anyway.)
                let m = grid[k];
                let distance = trail[i].expect("queued squares have a trail entry") + 1;
                if trail[m].is_none_or(|d| d > distance) {
                    trail[m] = Some(distance);
                    trail[k] = Some(i);
                    assert!(queue.len() < wh, "search queue overflow");
                    queue.push(m);
                }
            }
        }

        // Follow the trail back to the start, shifting each domino on it
        // along by one square.
        let mut i = target.expect("the search reaches another singleton");
        loop {
            let j = trail[i].expect("trail is unbroken");
            assert!(j < wh, "trail leaves the grid");
            let k = grid[j];
            grid[i] = j;
            grid[j] = i;
            if j == k {
                break;
            }
            i = k;
        }
    }
    grid
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every square is paired with an orthogonal neighbour that is paired
    /// back, and exactly `w * h % 2` squares are left over.
    fn check(w: usize, h: usize, layout: &[usize]) {
        assert_eq!(layout.len(), w * h);
        let mut leftover = 0;
        for (i, &j) in layout.iter().enumerate() {
            assert_eq!(layout[j], i, "{i} and {j} disagree");
            if i == j {
                leftover += 1;
            } else {
                let (x, y, x2, y2) = (i % w, i / w, j % w, j / w);
                assert_eq!(x.abs_diff(x2) + y.abs_diff(y2), 1, "{i}-{j} not adjacent");
            }
        }
        assert_eq!(leftover, w * h % 2);
    }

    #[test]
    fn empty_grid_gives_empty_layout() {
        let mut rng = RandomState::new(b"empty");
        assert_eq!(domino_layout(0, 5, &mut rng), Vec::<usize>::new());
        assert_eq!(domino_layout(5, 0, &mut rng), Vec::<usize>::new());
        assert_eq!(rng, RandomState::new(b"empty"));
    }

    #[test]
    fn single_square_is_left_over() {
        assert_eq!(domino_layout(1, 1, &mut RandomState::new(b"one")), [0]);
    }

    #[test]
    fn strips_are_tiled_end_to_end() {
        let mut rng = RandomState::new(b"strip");
        let layout = domino_layout(1, 6, &mut rng);
        assert_eq!(layout, [1, 0, 3, 2, 5, 4]);
        let layout = domino_layout(6, 1, &mut rng);
        assert_eq!(layout, [1, 0, 3, 2, 5, 4]);
    }

    #[test]
    fn layouts_are_valid_tilings() {
        let mut rng = RandomState::new(b"valid");
        for w in 1..=9 {
            for h in 1..=9 {
                for _ in 0..4 {
                    check(w, h, &domino_layout(w, h, &mut rng));
                }
            }
        }
    }

    #[test]
    fn large_grid_is_valid() {
        let mut rng = RandomState::new(b"large");
        check(41, 37, &domino_layout(41, 37, &mut rng));
    }

    #[test]
    fn layouts_are_cs() {
        // Golden values recorded from the C oracle: the layout, then the
        // generator's next 32 bits (so the draws used were the same).
        let mut rng = RandomState::new(b"golden");
        let layout = domino_layout(4, 3, &mut rng);
        assert_eq!(layout, [1, 0, 3, 2, 8, 9, 10, 11, 4, 5, 6, 7]);
        assert_eq!(rng.bits(32), 3_433_145_135);

        let mut rng = RandomState::new(b"golden");
        let layout = domino_layout(5, 5, &mut rng);
        assert_eq!(
            layout,
            [
                5, 2, 1, 4, 3, 0, 11, 12, 9, 8, 10, 6, 7, 14, 13, 20, 21, 22, 19, 18, 15, 16, 17,
                24, 23
            ]
        );
        assert_eq!(rng.bits(32), 3_293_923_641);
    }
}
