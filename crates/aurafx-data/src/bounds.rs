use serde::{Deserialize, Serialize};

/// Axis-aligned bounding box.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Bounds {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

impl Bounds {
    pub const EMPTY: Bounds = Bounds { min: [f64::INFINITY; 3], max: [f64::NEG_INFINITY; 3] };

    pub fn new(min: [f64; 3], max: [f64; 3]) -> Self {
        Self { min, max }
    }

    pub fn is_empty(&self) -> bool {
        self.min[0] > self.max[0]
    }

    pub fn add_point(&mut self, p: [f64; 3]) {
        for i in 0..3 {
            if p[i] < self.min[i] {
                self.min[i] = p[i];
            }
            if p[i] > self.max[i] {
                self.max[i] = p[i];
            }
        }
    }

    pub fn union(&self, other: &Bounds) -> Bounds {
        if self.is_empty() {
            return *other;
        }
        if other.is_empty() {
            return *self;
        }
        let mut b = *self;
        b.add_point(other.min);
        b.add_point(other.max);
        b
    }

    pub fn center(&self) -> [f64; 3] {
        [0.5 * (self.min[0] + self.max[0]), 0.5 * (self.min[1] + self.max[1]), 0.5 * (self.min[2] + self.max[2])]
    }

    pub fn size(&self) -> [f64; 3] {
        [self.max[0] - self.min[0], self.max[1] - self.min[1], self.max[2] - self.min[2]]
    }

    pub fn diagonal(&self) -> f64 {
        if self.is_empty() {
            return 0.0;
        }
        let s = self.size();
        (s[0] * s[0] + s[1] * s[1] + s[2] * s[2]).sqrt()
    }
}

impl Default for Bounds {
    fn default() -> Self {
        Self::EMPTY
    }
}
