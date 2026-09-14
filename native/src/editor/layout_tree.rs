/// Heights are stored in fixed point: this many units per pixel.
///
/// A Fenwick tree accumulates *differences*, and floating-point differences
/// don't cancel exactly — after enough edits the prefix sums drift from the
/// heights they summarize, and lines slowly creep off their positions. Integer
/// units make every sum exact, and 1/256px is far below anything visible.
const UNITS_PER_PX: f32 = 256.0;

fn to_units(px: f32) -> i64 {
    if px.is_finite() {
        (px * UNITS_PER_PX).round() as i64
    } else {
        0
    }
}

fn to_px(units: i64) -> f32 {
    (units as f64 / UNITS_PER_PX as f64) as f32
}

/// Line heights with O(log n) prefix sums and y → line lookup: a Fenwick
/// (binary indexed) tree over fixed-point heights.
///
/// Sums are exact internally; the f32 values handed out are exact below
/// 2^24 units (65,536px) and within f32 spacing (0.0625px at 1,000,000px)
/// beyond.
#[derive(Default, Debug, Clone)]
pub struct HeightTree {
    tree: Vec<i64>,
    heights: Vec<i64>,
}

impl HeightTree {
    pub fn new(len: usize) -> Self {
        Self {
            tree: vec![0; len + 1],
            heights: vec![0; len],
        }
    }

    pub fn len(&self) -> usize {
        self.heights.len()
    }

    pub fn get_height(&self, idx: usize) -> f32 {
        self.heights.get(idx).copied().map_or(0.0, to_px)
    }

    pub fn update_height(&mut self, idx: usize, new_height: f32) {
        if idx >= self.heights.len() {
            return;
        }
        let new_units = to_units(new_height);
        let delta = new_units - self.heights[idx];
        if delta == 0 {
            return;
        }
        self.heights[idx] = new_units;
        let mut i = idx + 1;
        while i < self.tree.len() {
            self.tree[i] += delta;
            i += i.isolate_lowest_one();
        }
    }

    /// Sum of the heights of lines `0..idx`.
    pub fn prefix_sum(&self, idx: usize) -> f32 {
        let mut sum = 0;
        let mut i = idx.min(self.heights.len());
        while i > 0 {
            sum += self.tree[i];
            i -= i.isolate_lowest_one();
        }
        to_px(sum)
    }

    /// The line whose span `[prefix_sum(i), prefix_sum(i + 1))` contains `y`,
    /// clamped to the first and last lines. O(log n) by binary lifting.
    pub fn find_line_at_y(&self, y: f32) -> usize {
        if self.heights.is_empty() {
            return 0;
        }
        let target = to_units(y);
        let len = self.heights.len();
        let mut idx = 0;
        let mut sum = 0;
        let mut step = len.next_power_of_two();
        while step > 0 {
            let next = idx + step;
            if next <= len && sum + self.tree[next] <= target {
                idx = next;
                sum += self.tree[next];
            }
            step >>= 1;
        }
        idx.min(len - 1)
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
    fn sums_stay_exact_through_many_fractional_updates() {
        let n = 257;
        let mut tree = HeightTree::new(n);
        let mut state = 0x9E37_79B9_7F4A_7C15_u64;
        for _ in 0..20_000 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let idx = (state % n as u64) as usize;
            // Up to ~64px: sums stay below 2^24 units, where f32 is exact.
            let height = (state >> 40) as f32 / (1 << 24) as f32 * 64.0;
            tree.update_height(idx, height);
        }
        let mut sum = 0_i64;
        for i in 0..=n {
            assert_eq!(tree.prefix_sum(i), to_px(sum), "prefix {i}");
            if i < n {
                sum += tree.heights[i];
            }
        }
        for i in 0..n {
            let top = tree.prefix_sum(i);
            if tree.get_height(i) > 0.0 {
                assert_eq!(tree.find_line_at_y(top), i);
            }
        }
    }
}
