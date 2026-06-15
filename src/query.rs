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
