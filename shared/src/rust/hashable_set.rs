use std::cmp::Ordering;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::iter::FromIterator;

#[derive(Debug, Clone)]
pub struct HashableSet<T>(pub HashSet<T>);

impl<T: Eq + Hash> HashableSet<T> {
    pub fn new() -> Self { Self(HashSet::new()) }
}

impl<T: Eq + Hash> PartialEq for HashableSet<T> {
    fn eq(&self, other: &Self) -> bool { self.0 == other.0 }
}

impl<T: Eq + Hash> Eq for HashableSet<T> {}

// Implement PartialOrd for HashableSet with T that implements Ord
impl<T: Eq + Hash + Ord> PartialOrd for HashableSet<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}

// Implement Ord for HashableSet with T that implements Ord
impl<T: Eq + Hash + Ord> Ord for HashableSet<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        // Sort both sets for consistent comparison
        let mut self_vec: Vec<&T> = self.0.iter().collect();
        let mut other_vec: Vec<&T> = other.0.iter().collect();

        self_vec.sort();
        other_vec.sort();

        // First compare by length
        let len_cmp = self.0.len().cmp(&other.0.len());
        if len_cmp != Ordering::Equal {
            return len_cmp;
        }

        // Then lexicographically
        for (a, b) in self_vec.iter().zip(other_vec.iter()) {
            match a.cmp(b) {
                Ordering::Equal => continue,
                non_eq => return non_eq,
            }
        }

        Ordering::Equal
    }
}

impl<T: Eq + Hash> Hash for HashableSet<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Collect the element hashes and sort for order-independent hashing
        let mut element_hashes: Vec<u64> = self
            .0
            .iter()
            .map(|item| {
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                item.hash(&mut hasher);
                hasher.finish()
            })
            .collect();

        element_hashes.sort_unstable(); // Ensure same order regardless of insertion
        for h in element_hashes {
            h.hash(state);
        }
    }
}

// Implement IntoIterator for HashableSet
impl<T: Eq + Hash> IntoIterator for HashableSet<T> {
    type Item = T;
    type IntoIter = std::collections::hash_set::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter { self.0.into_iter() }
}

// Implement IntoIterator for &HashableSet
impl<'a, T: Eq + Hash> IntoIterator for &'a HashableSet<T> {
    type Item = &'a T;
    type IntoIter = std::collections::hash_set::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter { self.0.iter() }
}

// Implement FromIterator for HashableSet
impl<T: Eq + Hash> FromIterator<T> for HashableSet<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        HashableSet(HashSet::from_iter(iter))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::hash_map::DefaultHasher;

    use super::*;

    fn hash(value: &HashableSet<i32>) -> u64 {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn compares_and_hashes_sets_independently_of_insertion_order() {
        let first: HashableSet<_> = [3, 1, 2].into_iter().collect();
        let second: HashableSet<_> = [2, 3, 1].into_iter().collect();
        let shorter: HashableSet<_> = [1, 2].into_iter().collect();
        let different: HashableSet<_> = [1, 2, 4].into_iter().collect();

        assert_eq!(first, second);
        assert_eq!(hash(&first), hash(&second));
        assert!(shorter < first);
        assert!(first < different);
        assert_eq!(first.partial_cmp(&second), Some(Ordering::Equal));
    }

    #[test]
    fn iterates_owned_and_borrowed_values() {
        let empty = HashableSet::<i32>::new();
        assert!(empty.0.is_empty());

        let values: HashableSet<_> = [1, 2, 3].into_iter().collect();
        let mut borrowed: Vec<_> = (&values).into_iter().copied().collect();
        borrowed.sort();
        assert_eq!(borrowed, vec![1, 2, 3]);

        let mut owned: Vec<_> = values.into_iter().collect();
        owned.sort();
        assert_eq!(owned, vec![1, 2, 3]);
    }
}
