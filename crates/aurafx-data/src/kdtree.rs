//! A k-d tree over 3-D points: the k nearest points and the points within a radius.
//!
//! Built once (O(n log n)) by median splits over an index array; queries walk the tree and
//! skip halves that cannot hold a closer point. Used by the point interpolators, clustering,
//! neighbourhood statistics and point clouds.

/// A balanced k-d tree. The points are kept in tree order with their original indices.
#[derive(Debug, Clone, Default)]
pub struct KdTree {
    pts: Vec<[f64; 3]>,
    idx: Vec<usize>,
    /// Split axis of each node (implicit tree over `pts`: node = range [lo, hi), median at mid).
    axes: Vec<u8>,
}

impl KdTree {
    pub fn new(points: &[[f64; 3]]) -> Self {
        let mut items: Vec<(usize, [f64; 3])> = points.iter().copied().enumerate().collect();
        let mut axes = vec![0u8; items.len()];
        build(&mut items, 0, &mut axes, 0);
        KdTree { pts: items.iter().map(|i| i.1).collect(), idx: items.iter().map(|i| i.0).collect(), axes }
    }

    pub fn len(&self) -> usize {
        self.pts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pts.is_empty()
    }

    /// The `k` nearest points to `q`: (original index, squared distance), nearest first.
    pub fn nearest(&self, q: [f64; 3], k: usize) -> Vec<(usize, f64)> {
        if k == 0 || self.pts.is_empty() {
            return Vec::new();
        }
        let mut best: Vec<(usize, f64)> = Vec::with_capacity(k + 1);
        self.knn(0, self.pts.len(), q, k, &mut best);
        best.into_iter().map(|(i, d)| (self.idx[i], d)).collect()
    }

    /// The nearest point: (original index, squared distance).
    pub fn nearest_one(&self, q: [f64; 3]) -> Option<(usize, f64)> {
        self.nearest(q, 1).into_iter().next()
    }

    /// Every point within `r` of `q`: (original index, squared distance), in no order.
    pub fn within(&self, q: [f64; 3], r: f64) -> Vec<(usize, f64)> {
        let mut out = Vec::new();
        if !self.pts.is_empty() && r >= 0.0 {
            self.radius(0, self.pts.len(), q, r * r, &mut out);
        }
        out
    }

    fn knn(&self, lo: usize, hi: usize, q: [f64; 3], k: usize, best: &mut Vec<(usize, f64)>) {
        if lo >= hi {
            return;
        }
        let mid = (lo + hi) / 2;
        let p = self.pts[mid];
        let d = d2(p, q);
        if best.len() < k || d < best[best.len() - 1].1 {
            let pos = best.partition_point(|b| b.1 <= d);
            best.insert(pos, (mid, d));
            best.truncate(k);
        }
        let ax = self.axes[mid] as usize;
        let diff = q[ax] - p[ax];
        let (near, far) = if diff < 0.0 { ((lo, mid), (mid + 1, hi)) } else { ((mid + 1, hi), (lo, mid)) };
        self.knn(near.0, near.1, q, k, best);
        if best.len() < k || diff * diff < best[best.len() - 1].1 {
            self.knn(far.0, far.1, q, k, best);
        }
    }

    fn radius(&self, lo: usize, hi: usize, q: [f64; 3], r2: f64, out: &mut Vec<(usize, f64)>) {
        if lo >= hi {
            return;
        }
        let mid = (lo + hi) / 2;
        let p = self.pts[mid];
        let d = d2(p, q);
        if d <= r2 {
            out.push((self.idx[mid], d));
        }
        let ax = self.axes[mid] as usize;
        let diff = q[ax] - p[ax];
        if diff < 0.0 || diff * diff <= r2 {
            self.radius(lo, mid, q, r2, out);
        }
        if diff >= 0.0 || diff * diff <= r2 {
            self.radius(mid + 1, hi, q, r2, out);
        }
    }
}

#[inline]
fn d2(a: [f64; 3], b: [f64; 3]) -> f64 {
    let (x, y, z) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
    x * x + y * y + z * z
}

/// Arrange `items` so that every range's median splits it along the widest axis of the range.
fn build(items: &mut [(usize, [f64; 3])], offset: usize, axes: &mut [u8], depth: usize) {
    if items.len() <= 1 {
        // A leaf (or an empty range, whose `offset` belongs to another node: left alone).
        if items.len() == 1 {
            axes[offset] = 0;
        }
        return;
    }
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for (_, p) in items.iter() {
        for a in 0..3 {
            lo[a] = lo[a].min(p[a]);
            hi[a] = hi[a].max(p[a]);
        }
    }
    let ax = (0..3).max_by(|&a, &b| (hi[a] - lo[a]).total_cmp(&(hi[b] - lo[b]))).unwrap_or(depth % 3);
    let mid = items.len() / 2;
    items.select_nth_unstable_by(mid, |a, b| a.1[ax].total_cmp(&b.1[ax]));
    axes[offset + mid] = ax as u8;
    let (left, right) = items.split_at_mut(mid);
    build(left, offset, axes, depth + 1);
    build(&mut right[1..], offset + mid + 1, axes, depth + 1);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cloud(n: usize) -> Vec<[f64; 3]> {
        // A fixed pseudo-random cloud (xorshift).
        let mut s = 0x9E37_79B9_7F4A_7C15u64;
        let mut r = || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s >> 11) as f64 / (1u64 << 53) as f64
        };
        (0..n).map(|_| [r(), r() * 2.0, r() * 0.5]).collect()
    }

    #[test]
    fn matches_brute_force() {
        let pts = cloud(2000);
        let t = KdTree::new(&pts);
        for q in cloud(50).iter().map(|p| [p[0] * 1.2 - 0.1, p[1], p[2] + 0.1]) {
            let mut brute: Vec<(usize, f64)> = pts.iter().enumerate().map(|(i, p)| (i, d2(*p, q))).collect();
            brute.sort_by(|a, b| a.1.total_cmp(&b.1));
            let got = t.nearest(q, 7);
            assert_eq!(got.iter().map(|g| g.0).collect::<Vec<_>>(), brute[..7].iter().map(|b| b.0).collect::<Vec<_>>());
            let mut w: Vec<usize> = t.within(q, 0.2).into_iter().map(|g| g.0).collect();
            w.sort();
            let mut bw: Vec<usize> = brute.iter().filter(|b| b.1 <= 0.04).map(|b| b.0).collect();
            bw.sort();
            assert_eq!(w, bw);
        }
        assert!(KdTree::new(&[]).nearest([0.0; 3], 3).is_empty());
        assert_eq!(KdTree::new(&[[1.0, 2.0, 3.0]]).nearest_one([0.0; 3]).unwrap().0, 0);
    }
}
