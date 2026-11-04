//! Attribute collections (point data, cell data, field data).

use crate::array::Array;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AttributeKind {
    Scalars,
    Vectors,
    Normals,
    TCoords,
    Tensors,
    GlobalIds,
    PedigreeIds,
}

/// Ordered map of named arrays with "active" attribute designations.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Attributes {
    pub arrays: IndexMap<String, Array>,
    pub active: IndexMap<AttributeKind, String>,
}

impl Attributes {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, array: Array) -> &mut Array {
        let name = array.name.clone();
        self.arrays.insert(name.clone(), array);
        self.arrays.get_mut(&name).unwrap()
    }

    pub fn insert_active(&mut self, kind: AttributeKind, array: Array) {
        self.active.insert(kind, array.name.clone());
        self.insert(array);
    }

    pub fn set_active(&mut self, kind: AttributeKind, name: &str) {
        if self.arrays.contains_key(name) {
            self.active.insert(kind, name.to_string());
        }
    }

    pub fn get(&self, name: &str) -> Option<&Array> {
        self.arrays.get(name)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut Array> {
        self.arrays.get_mut(name)
    }

    pub fn remove(&mut self, name: &str) -> Option<Array> {
        self.active.retain(|_, v| v != name);
        self.arrays.shift_remove(name)
    }

    pub fn active(&self, kind: AttributeKind) -> Option<&Array> {
        self.active.get(&kind).and_then(|n| self.arrays.get(n))
    }

    pub fn scalars(&self) -> Option<&Array> {
        self.active(AttributeKind::Scalars).or_else(|| self.arrays.values().find(|a| a.ncomp == 1 && a.scalar_type() != crate::ScalarType::String))
    }

    pub fn vectors(&self) -> Option<&Array> {
        self.active(AttributeKind::Vectors).or_else(|| self.arrays.values().find(|a| a.ncomp == 3))
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.arrays.keys().map(|s| s.as_str())
    }

    pub fn len(&self) -> usize {
        self.arrays.len()
    }

    pub fn is_empty(&self) -> bool {
        self.arrays.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Array> {
        self.arrays.values()
    }

    /// Build a new attribute set containing the given tuples of every array.
    pub fn gather(&self, idx: &[usize]) -> Attributes {
        Attributes { arrays: self.arrays.iter().map(|(k, a)| (k.clone(), a.gather(idx))).collect(), active: self.active.clone() }
    }

    /// Keep only arrays that pass `pred`.
    pub fn retain(&mut self, mut pred: impl FnMut(&Array) -> bool) {
        self.arrays.retain(|_, a| pred(a));
        let names: Vec<String> = self.arrays.keys().cloned().collect();
        self.active.retain(|_, v| names.contains(v));
    }

    pub fn n_bytes(&self) -> usize {
        self.arrays.values().map(|a| a.n_bytes()).sum()
    }
}
