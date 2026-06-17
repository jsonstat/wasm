//! Core index calculation and combinatorial utilities for JSON-stat queries.

/// Calculates the 1D flat array index from a list of category indices (row-major order).
///
/// `indices`: The category index for each dimension.
/// `sizes`: The size (number of categories) of each dimension.
///
/// Example:
/// A(3) B(2) C(4)
/// indices = [1, 1, 2] (A: 1 out of 3, B: 1 out of 2, C: 2 out of 4)
/// sizes = [3, 2, 4]
/// index = 1*(2*4) + 1*(4) + 2*(1) = 8 + 4 + 2 = 14
pub fn calculate_index(indices: &[usize], sizes: &[usize]) -> Option<usize> {
    if indices.len() != sizes.len() {
        return None;
    }

    let mut multiplier = 1;
    let mut flat_index = 0;

    for i in (0..sizes.len()).rev() {
        if indices[i] >= sizes[i] {
            return None; // Out of bounds
        }
        flat_index += indices[i] * multiplier;
        multiplier *= sizes[i];
    }

    Some(flat_index)
}

/// Calculates dimension indices from a flat index (inverse of `calculate_index`).
pub fn calculate_indices(flat: usize, sizes: &[usize]) -> Option<Vec<usize>> {
    let total: usize = sizes.iter().product();
    if flat >= total {
        return None;
    }
    let n = sizes.len();
    let mut indices = vec![0; n];
    let mut remaining = flat;
    for i in 0..n {
        let stride: usize = if i + 1 < n {
            sizes[i + 1..].iter().product()
        } else {
            1
        };
        indices[i] = remaining / stride;
        remaining %= stride;
    }
    Some(indices)
}

/// Iterator over all index combinations for given sizes (row-major order).
/// Yields one `Vec<usize>` per cell without materializing all combinations
/// at once. The n-th yielded combination corresponds to flat index n.
pub struct IndexIter {
    sizes: Vec<usize>,
    current: Vec<usize>,
    done: bool,
}

impl Iterator for IndexIter {
    type Item = Vec<usize>;

    fn next(&mut self) -> Option<Vec<usize>> {
        if self.done {
            return None;
        }
        let item = self.current.clone();
        // Increment odometer (rightmost dimension fastest)
        let mut i = self.sizes.len();
        loop {
            if i == 0 {
                self.done = true;
                break;
            }
            i -= 1;
            self.current[i] += 1;
            if self.current[i] < self.sizes[i] {
                break;
            }
            self.current[i] = 0;
        }
        Some(item)
    }
}

/// Creates a row-major iterator over all index combinations for given sizes.
pub fn index_iter(sizes: &[usize]) -> IndexIter {
    IndexIter {
        sizes: sizes.to_vec(),
        current: vec![0; sizes.len()],
        done: sizes.contains(&0),
    }
}

/// Allocation-free row-major odometer over all index combinations.
///
/// Unlike [`IndexIter`] (which clones a fresh `Vec<usize>` per step), the
/// odometer mutates a single reusable buffer in place and exposes it via
/// [`Odometer::current`]. Callers drive it with a `loop { … if !o.advance() {
/// break } }` pattern, so the hot transform/dice loops avoid one heap
/// allocation **per cell** — the dominant per-cell cost when emitting large
/// tables.
///
/// The n-th visited combination corresponds to flat (row-major) index n, so
/// callers can track the flat index with a simple counter instead of calling
/// [`calculate_index`] again.
pub struct Odometer {
    sizes: Vec<usize>,
    current: Vec<usize>,
    /// `true` once the space is exhausted (or empty to begin with).
    done: bool,
}

impl Odometer {
    /// Create an odometer positioned at the first combination (all zeros).
    /// If any dimension has size 0 the space is empty and [`Odometer::done`]
    /// is immediately `true`.
    pub fn new(sizes: &[usize]) -> Self {
        Odometer {
            sizes: sizes.to_vec(),
            current: vec![0; sizes.len()],
            done: sizes.contains(&0),
        }
    }

    /// The current combination (read-only borrow, no allocation).
    #[inline]
    pub fn current(&self) -> &[usize] {
        &self.current
    }

    /// Whether the space is exhausted (or was empty from the start).
    #[inline]
    pub fn done(&self) -> bool {
        self.done
    }

    /// Advance to the next combination (rightmost dimension fastest).
    /// Returns `false` when there is no next combination.
    #[inline]
    pub fn advance(&mut self) -> bool {
        if self.done {
            return false;
        }
        let mut i = self.sizes.len();
        loop {
            if i == 0 {
                self.done = true;
                return false;
            }
            i -= 1;
            self.current[i] += 1;
            if self.current[i] < self.sizes[i] {
                return true;
            }
            self.current[i] = 0;
        }
    }
}

/// Format a `usize` into the provided stack buffer and return it as `&str`.
///
/// Sparse JSON-stat `value`/`status` are objects keyed by the decimal flat
/// index (`"0"`, `"1"`, …). Looking those up with `index.to_string()` heap-
/// allocates a `String` on every cell. This writes the digits into a caller-
/// owned `[u8; 20]` (enough for `usize::MAX`) and borrows it for the lookup,
/// so the hot per-cell path allocates nothing.
#[inline]
pub fn usize_key(buf: &mut [u8; 20], mut n: usize) -> &str {
    if n == 0 {
        buf[0] = b'0';
        // Digits are always valid ASCII/UTF-8.
        return std::str::from_utf8(&buf[..1]).unwrap();
    }
    let mut i = buf.len();
    while n > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    std::str::from_utf8(&buf[i..]).unwrap()
}

/// Row-major strides for a list of dimension sizes: `strides[i]` is the flat
/// distance between consecutive categories of dimension `i`. Lets callers
/// update a flat index incrementally instead of recomputing it per cell.
///
/// Example: sizes `[3, 2, 4]` → strides `[8, 4, 1]`.
pub fn strides(sizes: &[usize]) -> Vec<usize> {
    let n = sizes.len();
    let mut strides = vec![1usize; n];
    for i in (0..n.saturating_sub(1)).rev() {
        strides[i] = strides[i + 1] * sizes[i + 1];
    }
    strides
}

/// Generates all index combinations for given sizes (row-major order).
pub fn all_indices(sizes: &[usize]) -> Vec<Vec<usize>> {
    index_iter(sizes).collect()
}

/// Generates all combinations from a vector of per-dimension index lists.
/// E.g., [[0, 1], [0, 1, 2]] → [[0,0], [0,1], [0,2], [1,0], [1,1], [1,2]]
pub fn all_combinations(ranges: &[Vec<usize>]) -> Vec<Vec<usize>> {
    if ranges.is_empty() {
        return vec![vec![]];
    }
    let mut result = vec![vec![]];
    for range in ranges {
        let mut new_result = Vec::new();
        for prefix in &result {
            for &val in range {
                let mut combo = prefix.clone();
                combo.push(val);
                new_result.push(combo);
            }
        }
        result = new_result;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_row_major_order() {
        let sizes = vec![3, 2, 4];
        let indices = vec![1, 1, 2];
        assert_eq!(calculate_index(&indices, &sizes), Some(14));

        let sizes2 = vec![1, 36, 12];
        let indices2 = vec![0, 0, 0];
        assert_eq!(calculate_index(&indices2, &sizes2), Some(0));
    }

    #[test]
    fn test_calculate_indices_roundtrip() {
        let sizes = vec![3, 2, 4];
        for flat in 0..24 {
            let indices = calculate_indices(flat, &sizes).unwrap();
            assert_eq!(calculate_index(&indices, &sizes), Some(flat));
        }
    }

    #[test]
    fn test_calculate_indices_single_dim() {
        let sizes = vec![5];
        assert_eq!(calculate_indices(0, &sizes), Some(vec![0]));
        assert_eq!(calculate_indices(4, &sizes), Some(vec![4]));
        assert_eq!(calculate_indices(5, &sizes), None);
    }

    #[test]
    fn test_all_indices() {
        let sizes = vec![2, 3];
        let all = all_indices(&sizes);
        assert_eq!(all.len(), 6);
        assert_eq!(all[0], vec![0, 0]);
        assert_eq!(all[5], vec![1, 2]);
    }

    #[test]
    fn test_all_combinations() {
        let ranges = vec![vec![0, 1], vec![0, 1, 2]];
        let combos = all_combinations(&ranges);
        assert_eq!(combos.len(), 6);
        assert_eq!(combos[0], vec![0, 0]);
        assert_eq!(combos[5], vec![1, 2]);
    }

    #[test]
    fn test_index_iter_matches_all_indices() {
        let sizes = vec![3, 2, 4];
        let iterated: Vec<Vec<usize>> = index_iter(&sizes).collect();
        assert_eq!(iterated.len(), 24);
        for (flat, indices) in iterated.iter().enumerate() {
            assert_eq!(calculate_index(indices, &sizes), Some(flat));
        }
        // Empty sizes → single empty combination
        let empty: Vec<Vec<usize>> = index_iter(&[]).collect();
        assert_eq!(empty, vec![Vec::<usize>::new()]);
        // Zero-sized dimension → no combinations
        let none: Vec<Vec<usize>> = index_iter(&[2, 0]).collect();
        assert!(none.is_empty());
    }

    #[test]
    fn test_odometer_matches_index_iter() {
        let sizes = vec![3, 2, 4];
        let mut o = Odometer::new(&sizes);
        let mut flat = 0usize;
        loop {
            // The current combination must map back to the flat counter.
            assert_eq!(calculate_index(o.current(), &sizes), Some(flat));
            if !o.advance() {
                break;
            }
            flat += 1;
        }
        // Visited every cell exactly once.
        assert_eq!(flat + 1, sizes.iter().product::<usize>());
    }

    #[test]
    fn test_odometer_empty_and_zero() {
        // No dimensions → a single empty combination, then exhausted.
        let mut o = Odometer::new(&[]);
        assert!(!o.done());
        assert_eq!(o.current(), &[] as &[usize]);
        assert!(!o.advance());
        // Zero-sized dimension → immediately done, no combinations.
        let mut z = Odometer::new(&[2, 0]);
        assert!(z.done());
        assert!(!z.advance());
    }

    #[test]
    fn test_usize_key() {
        let mut buf = [0u8; 20];
        assert_eq!(usize_key(&mut buf, 0), "0");
        assert_eq!(usize_key(&mut buf, 7), "7");
        assert_eq!(usize_key(&mut buf, 10), "10");
        assert_eq!(usize_key(&mut buf, 1234567890), "1234567890");
        assert_eq!(usize_key(&mut buf, usize::MAX), usize::MAX.to_string());
        // Reused buffer must not leak stale digits across calls.
        assert_eq!(usize_key(&mut buf, 9), "9");
    }

    #[test]
    fn test_strides() {
        assert_eq!(strides(&[3, 2, 4]), vec![8, 4, 1]);
        assert_eq!(strides(&[5]), vec![1]);
        assert_eq!(strides(&[]), Vec::<usize>::new());
        // strides[i] equals the flat delta for advancing dimension i by one.
        let sizes = vec![3, 2, 4];
        let st = strides(&sizes);
        for d in 0..sizes.len() {
            if sizes[d] < 2 {
                continue;
            }
            let a = vec![0usize; sizes.len()];
            let mut b = a.clone();
            b[d] = 1;
            let fa = calculate_index(&a, &sizes).unwrap();
            let fb = calculate_index(&b, &sizes).unwrap();
            assert_eq!(fb - fa, st[d]);
        }
    }

    #[test]
    fn test_all_combinations_empty() {
        let combos = all_combinations(&[]);
        let empty: Vec<Vec<usize>> = vec![vec![]];
        assert_eq!(combos, empty);
    }

    #[test]
    fn test_empty_sizes() {
        assert_eq!(calculate_index(&[], &[]), Some(0));
        assert_eq!(calculate_indices(0, &[]), Some(vec![]));
    }

    #[test]
    fn test_out_of_bounds() {
        assert_eq!(calculate_index(&[5], &[3]), None);
        assert_eq!(calculate_index(&[1, 5], &[3, 4]), None);
        assert_eq!(calculate_index(&[1], &[3, 4]), None); // mismatched lengths
    }
}
