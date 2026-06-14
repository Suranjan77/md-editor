//! Height sum-tree (plan §3.2): a monoid tree over per-line heights giving
//! O(log n) offset queries, O(log n) point updates, and O(log n)
//! insert/remove — the data structure that makes "a line changed height"
//! a cheap, *total* invalidation of every subsequent offset.
//!
//! Implementation: an implicit treap (tree keyed by line index) where each
//! node caches its subtree's element count and height sum. Priorities come
//! from a deterministic xorshift stream, so tree shape — and therefore
//! performance — is reproducible.

/// Error for out-of-range line indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("line index {index} out of bounds (len {len})")]
pub struct OutOfBounds {
    pub index: usize,
    pub len: usize,
}

#[derive(Debug)]
struct Node {
    pri: u64,
    height: f64,
    count: usize,
    sum: f64,
    left: Option<Box<Node>>,
    right: Option<Box<Node>>,
}

impl Node {
    fn new(height: f64, pri: u64) -> Box<Node> {
        Box::new(Node {
            pri,
            height,
            count: 1,
            sum: height,
            left: None,
            right: None,
        })
    }

    fn refresh(&mut self) {
        self.count = 1 + count(&self.left) + count(&self.right);
        self.sum = self.height + sum(&self.left) + sum(&self.right);
    }
}

fn count(n: &Option<Box<Node>>) -> usize {
    n.as_ref().map_or(0, |n| n.count)
}

fn sum(n: &Option<Box<Node>>) -> f64 {
    n.as_ref().map_or(0.0, |n| n.sum)
}

fn merge(a: Option<Box<Node>>, b: Option<Box<Node>>) -> Option<Box<Node>> {
    match (a, b) {
        (None, b) => b,
        (a, None) => a,
        (Some(mut a), Some(mut b)) => {
            if a.pri >= b.pri {
                let right = a.right.take();
                a.right = merge(right, Some(b));
                a.refresh();
                Some(a)
            } else {
                let left = b.left.take();
                b.left = merge(Some(a), left);
                b.refresh();
                Some(b)
            }
        }
    }
}

/// Split into (first `k` elements, the rest).
fn split(n: Option<Box<Node>>, k: usize) -> (Option<Box<Node>>, Option<Box<Node>>) {
    match n {
        None => (None, None),
        Some(mut node) => {
            let left_count = count(&node.left);
            if k <= left_count {
                let left = node.left.take();
                let (a, b) = split(left, k);
                node.left = b;
                node.refresh();
                (a, Some(node))
            } else {
                let right = node.right.take();
                let (a, b) = split(right, k - left_count - 1);
                node.right = a;
                node.refresh();
                (Some(node), b)
            }
        }
    }
}

#[derive(Debug)]
pub struct HeightTree {
    root: Option<Box<Node>>,
    rng: u64,
}

impl Default for HeightTree {
    fn default() -> HeightTree {
        HeightTree::new()
    }
}

impl HeightTree {
    pub fn new() -> HeightTree {
        HeightTree {
            root: None,
            rng: 0x9E37_79B9_7F4A_7C15,
        }
    }

    fn next_pri(&mut self) -> u64 {
        // xorshift64* — deterministic, cheap, good enough for treap balance.
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub fn len(&self) -> usize {
        count(&self.root)
    }

    pub fn is_empty(&self) -> bool {
        self.root.is_none()
    }

    /// Sum of all line heights — total document height.
    pub fn total_height(&self) -> f64 {
        sum(&self.root)
    }

    /// Insert a line of `height` so it becomes line `index` (0 ≤ index ≤ len).
    pub fn insert(&mut self, index: usize, height: f64) -> Result<(), OutOfBounds> {
        let len = self.len();
        if index > len {
            return Err(OutOfBounds { index, len });
        }
        let pri = self.next_pri();
        let (a, b) = split(self.root.take(), index);
        self.root = merge(merge(a, Some(Node::new(height, pri))), b);
        Ok(())
    }

    pub fn push(&mut self, height: f64) {
        let pri = self.next_pri();
        let root = self.root.take();
        self.root = merge(root, Some(Node::new(height, pri)));
    }

    /// Remove line `index`, returning its height.
    pub fn remove(&mut self, index: usize) -> Result<f64, OutOfBounds> {
        let len = self.len();
        if index >= len {
            return Err(OutOfBounds { index, len });
        }
        let (a, bc) = split(self.root.take(), index);
        let (b, c) = split(bc, 1);
        let removed = b.map_or(0.0, |n| n.height);
        self.root = merge(a, c);
        Ok(removed)
    }

    /// Set line `index` to `height`, returning the old height. This is the
    /// whole invalidation protocol: after this call every offset query below
    /// `index` is already correct.
    pub fn set(&mut self, index: usize, height: f64) -> Result<f64, OutOfBounds> {
        let old = self.remove(index)?;
        // Insert at the same index cannot fail: we just removed from there.
        self.insert(index, height).map_err(|_| OutOfBounds {
            index,
            len: self.len(),
        })?;
        Ok(old)
    }

    pub fn get(&self, index: usize) -> Option<f64> {
        let mut cur = self.root.as_deref();
        let mut index = index;
        while let Some(n) = cur {
            let lc = count(&n.left);
            if index < lc {
                cur = n.left.as_deref();
            } else if index == lc {
                return Some(n.height);
            } else {
                index -= lc + 1;
                cur = n.right.as_deref();
            }
        }
        None
    }

    /// Vertical offset of the top of line `index`: the sum of heights of all
    /// lines before it. `offset_of(len)` is the total height. O(log n).
    pub fn offset_of(&self, index: usize) -> Result<f64, OutOfBounds> {
        let len = self.len();
        if index > len {
            return Err(OutOfBounds { index, len });
        }
        let mut acc = 0.0;
        let mut k = index;
        let mut cur = self.root.as_deref();
        while let Some(n) = cur {
            let lc = count(&n.left);
            if k <= lc {
                cur = n.left.as_deref();
            } else {
                acc += sum(&n.left) + n.height;
                k -= lc + 1;
                cur = n.right.as_deref();
            }
        }
        Ok(acc)
    }

    /// Which line contains vertical offset `y`? Clamps: negative `y` → line 0,
    /// `y` past the end → last line. `None` only when the tree is empty.
    pub fn line_at_offset(&self, y: f64) -> Option<usize> {
        let len = self.len();
        if len == 0 {
            return None;
        }
        if y <= 0.0 {
            return Some(0);
        }
        let mut y = y;
        let mut base = 0;
        let mut cur = self.root.as_deref();
        while let Some(n) = cur {
            let left_sum = sum(&n.left);
            if y < left_sum {
                cur = n.left.as_deref();
            } else {
                y -= left_sum;
                let idx = base + count(&n.left);
                if y < n.height {
                    return Some(idx);
                }
                y -= n.height;
                base = idx + 1;
                cur = n.right.as_deref();
            }
        }
        Some(len - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok<T>(r: Result<T, OutOfBounds>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => panic!("{e}"),
        }
    }

    #[test]
    fn offsets_are_prefix_sums() {
        let mut tree = HeightTree::new();
        for h in [16.0, 32.0, 16.0, 48.0] {
            tree.push(h);
        }
        assert_eq!(ok(tree.offset_of(0)), 0.0);
        assert_eq!(ok(tree.offset_of(1)), 16.0);
        assert_eq!(ok(tree.offset_of(2)), 48.0);
        assert_eq!(ok(tree.offset_of(3)), 64.0);
        assert_eq!(ok(tree.offset_of(4)), 112.0);
        assert_eq!(tree.total_height(), 112.0);
    }

    #[test]
    fn set_updates_all_subsequent_offsets_at_once() {
        let mut tree = HeightTree::new();
        for _ in 0..100 {
            tree.push(16.0);
        }
        assert_eq!(ok(tree.offset_of(50)), 800.0);
        let old = ok(tree.set(10, 64.0));
        assert_eq!(old, 16.0);
        assert_eq!(
            ok(tree.offset_of(50)),
            848.0,
            "offset below the change reflects it immediately"
        );
        assert_eq!(
            ok(tree.offset_of(10)),
            160.0,
            "offset above the change is untouched"
        );
    }

    #[test]
    fn line_at_offset_inverts_offset_of() {
        let mut tree = HeightTree::new();
        for h in [10.0, 20.0, 30.0, 40.0] {
            tree.push(h);
        }
        assert_eq!(tree.line_at_offset(-5.0), Some(0));
        assert_eq!(tree.line_at_offset(0.0), Some(0));
        assert_eq!(tree.line_at_offset(9.9), Some(0));
        assert_eq!(tree.line_at_offset(10.0), Some(1));
        assert_eq!(tree.line_at_offset(59.9), Some(2));
        assert_eq!(tree.line_at_offset(60.0), Some(3));
        assert_eq!(tree.line_at_offset(1e9), Some(3), "clamps past the end");
        assert_eq!(HeightTree::new().line_at_offset(0.0), None);
    }

    #[test]
    fn out_of_bounds_is_an_error_not_a_panic() {
        let mut tree = HeightTree::new();
        tree.push(16.0);
        assert_eq!(tree.remove(1), Err(OutOfBounds { index: 1, len: 1 }));
        assert_eq!(tree.insert(5, 1.0), Err(OutOfBounds { index: 5, len: 1 }));
        assert_eq!(tree.offset_of(2), Err(OutOfBounds { index: 2, len: 1 }));
        assert_eq!(tree.get(1), None);
    }

    /// Randomized differential test: the treap must agree with a naive Vec
    /// model under thousands of mixed operations. Deterministic seed.
    #[test]
    fn agrees_with_naive_model_under_random_ops() {
        let mut tree = HeightTree::new();
        let mut model: Vec<f64> = Vec::new();
        let mut rng: u64 = 42;
        let mut next = || {
            rng ^= rng >> 12;
            rng ^= rng << 25;
            rng ^= rng >> 27;
            rng.wrapping_mul(0x2545_F491_4F6C_DD1D)
        };
        for _ in 0..4000 {
            let op = next() % 4;
            match op {
                0 => {
                    let idx = (next() as usize) % (model.len() + 1);
                    let h = ((next() % 64) + 1) as f64;
                    ok(tree.insert(idx, h));
                    model.insert(idx, h);
                }
                1 if !model.is_empty() => {
                    let idx = (next() as usize) % model.len();
                    let got = ok(tree.remove(idx));
                    let want = model.remove(idx);
                    assert_eq!(got, want);
                }
                2 if !model.is_empty() => {
                    let idx = (next() as usize) % model.len();
                    let h = ((next() % 64) + 1) as f64;
                    ok(tree.set(idx, h));
                    model[idx] = h;
                }
                _ if !model.is_empty() => {
                    let idx = (next() as usize) % (model.len() + 1);
                    let want: f64 = model[..idx].iter().sum();
                    let got = ok(tree.offset_of(idx));
                    assert!(
                        (got - want).abs() < 1e-6,
                        "offset_of({idx}): {got} != {want}"
                    );
                }
                _ => {}
            }
            assert_eq!(tree.len(), model.len());
        }
        let want_total: f64 = model.iter().sum();
        assert!((tree.total_height() - want_total).abs() < 1e-6);
    }
}
