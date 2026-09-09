use std::ops::Bound::{Excluded, Unbounded};

use imbl::OrdSet;

pub struct OrderedSnapshot<K> {
    values: OrdSet<K>,
    previous: Option<K>,
    exhausted: bool,
}

impl<K: Ord + Clone> OrderedSnapshot<K> {
    pub fn new(values: OrdSet<K>) -> Self {
        Self {
            values,
            previous: None,
            exhausted: false,
        }
    }

    pub fn original_len(&self) -> usize { self.values.len() }

    pub(crate) fn shared_values(&self) -> OrdSet<K> { self.values.clone() }
}

impl<K: Ord + Clone> Iterator for OrderedSnapshot<K> {
    type Item = K;

    fn next(&mut self) -> Option<Self::Item> {
        if self.exhausted {
            return None;
        }
        let next = match self.previous.as_ref() {
            Some(previous) => self
                .values
                .range::<_, K>((Excluded(previous), Unbounded))
                .next(),
            None => self.values.get_min(),
        }
        .cloned();
        if let Some(key) = &next {
            self.previous = Some(key.clone());
        } else {
            self.exhausted = true;
        }
        next
    }
}

impl<K: Ord + Clone> std::iter::FusedIterator for OrderedSnapshot<K> {}
