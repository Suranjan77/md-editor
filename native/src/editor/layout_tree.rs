#[derive(Default, Debug, Clone)]
pub(crate) struct HeightTree {
    tree: Vec<f32>,
    heights: Vec<f32>,
}

impl HeightTree {
    pub(crate) fn new(len: usize) -> Self {
        Self {
            tree: vec![0.0; len + 1],
            heights: vec![0.0; len],
        }
    }

    pub(crate) fn resize(&mut self, len: usize) {
        self.tree.resize(len + 1, 0.0);
        self.heights.resize(len, 0.0);
        self.tree.fill(0.0);
        self.heights.fill(0.0);
    }

    pub(crate) fn len(&self) -> usize {
        self.heights.len()
    }

    pub(crate) fn get_height(&self, idx: usize) -> f32 {
        self.heights.get(idx).copied().unwrap_or(0.0)
    }

    pub(crate) fn update_height(&mut self, idx: usize, new_height: f32) {
        if idx >= self.heights.len() {
            return;
        }
        let old_height = self.heights[idx];
        let delta = new_height - old_height;
        if delta.abs() < 1e-5 {
            return;
        }
        self.heights[idx] = new_height;
        let mut i = idx + 1;
        while i < self.tree.len() {
            self.tree[i] += delta;
            // i & -i equivalent: i & (!i + 1)
            let step = i & (!i + 1);
            i += step;
        }
    }

    /// Prefix sum of heights from line 0 up to idx (exclusive)
    pub(crate) fn prefix_sum(&self, idx: usize) -> f32 {
        self.prefix_sum_with_steps(idx).0
    }

    fn prefix_sum_with_steps(&self, idx: usize) -> (f32, usize) {
        let mut sum = 0.0;
        let mut i = idx;
        let mut steps = 0;
        while i > 0 {
            sum += self.tree[i];
            let step = i & (!i + 1);
            i -= step;
            steps += 1;
        }
        (sum, steps)
    }

    /// Finds the line index whose visual range [start_y, start_y + height] contains y.
    /// This uses O(log N) binary lifting on the Fenwick tree.
    pub(crate) fn find_line_at_y(&self, y: f32) -> usize {
        self.find_line_at_y_with_steps(y).0
    }

    fn find_line_at_y_with_steps(&self, y: f32) -> (usize, usize) {
        if self.heights.is_empty() {
            return (0, 0);
        }
        let len = self.heights.len();
        let mut idx = 0;
        let mut sum = 0.0;
        let mut steps_taken = 0;

        let mut step = 1;
        while step <= len {
            step <<= 1;
        }
        step >>= 1;

        while step > 0 {
            let next_idx = idx + step;
            if next_idx <= len {
                let val = self.tree[next_idx];
                if sum + val <= y {
                    idx = next_idx;
                    sum += val;
                }
            }
            step >>= 1;
            steps_taken += 1;
        }

        (idx.min(len - 1), steps_taken)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_height_tree_basics() {
        let mut tree = HeightTree::new(3);
        tree.update_height(0, 10.0);
        tree.update_height(1, 20.0);
        tree.update_height(2, 30.0);

        assert_eq!(tree.prefix_sum(0), 0.0);
        assert_eq!(tree.prefix_sum(1), 10.0);
        assert_eq!(tree.prefix_sum(2), 30.0);
        assert_eq!(tree.prefix_sum(3), 60.0);

        // find_line_at_y testing
        assert_eq!(tree.find_line_at_y(-5.0), 0);
        assert_eq!(tree.find_line_at_y(0.0), 0);
        assert_eq!(tree.find_line_at_y(5.0), 0);
        assert_eq!(tree.find_line_at_y(10.0), 1);
        assert_eq!(tree.find_line_at_y(15.0), 1);
        assert_eq!(tree.find_line_at_y(30.0), 2);
        assert_eq!(tree.find_line_at_y(59.9), 2);
        assert_eq!(tree.find_line_at_y(65.0), 2);
    }

    #[test]
    fn large_height_tree_queries_stay_logarithmic() {
        let mut tree = HeightTree::new(50_000);
        for idx in 0..50_000 {
            tree.update_height(idx, 24.0);
        }

        let (prefix, prefix_steps) = tree.prefix_sum_with_steps(49_999);
        let (line, find_steps) = tree.find_line_at_y_with_steps(prefix);

        assert_eq!(line, 49_999);
        assert!(
            prefix_steps <= 32,
            "prefix sum should remain logarithmic, got {prefix_steps}"
        );
        assert!(
            find_steps <= 32,
            "line lookup should remain logarithmic, got {find_steps}"
        );
    }
}
