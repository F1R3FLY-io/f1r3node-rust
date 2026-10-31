use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct LeftInSpace {
    data: HashMap<Vec<u8>, u32>,
    continuations: HashMap<Vec<u8>, u32>,
    taken: HashMap<Vec<u8>, u32>,
}

fn add(items: &mut HashMap<Vec<u8>, u32>, id: &[u8]) {
    *items.entry(id.to_vec()).or_insert(0) += 1;
}

fn take(items: &mut HashMap<Vec<u8>, u32>, id: &[u8]) -> bool {
    match items.get_mut(id) {
        Some(count) if *count > 1 => {
            *count -= 1;
            true
        }
        Some(_) => {
            items.remove(id);
            true
        }
        None => false,
    }
}

impl LeftInSpace {
    pub fn stored_datum(&mut self, id: &[u8]) { add(&mut self.data, id); }

    pub fn stored_continuation(&mut self, id: &[u8]) { add(&mut self.continuations, id); }

    pub fn removed_datum(&mut self, id: &[u8]) -> bool {
        if take(&mut self.data, id) {
            true
        } else {
            add(&mut self.taken, id);
            false
        }
    }

    pub fn removed_continuation(&mut self, id: &[u8]) -> bool {
        take(&mut self.continuations, id)
    }

    pub fn restores(&mut self, id: &[u8]) -> bool { take(&mut self.taken, id) }
}

#[cfg(test)]
mod tests {
    use super::LeftInSpace;

    #[test]
    fn only_items_the_run_stored_count_as_its_own() {
        let mut rule = LeftInSpace::default();
        rule.stored_datum(b"a");
        rule.stored_datum(b"a");
        assert!(rule.removed_datum(b"a"));
        assert!(rule.removed_datum(b"a"));
        assert!(!rule.removed_datum(b"a"));
        assert!(!rule.removed_datum(b"pre"));
        assert!(rule.restores(b"pre"));
        assert!(!rule.restores(b"pre"));
        rule.stored_continuation(b"k");
        assert!(rule.removed_continuation(b"k"));
        assert!(!rule.removed_continuation(b"k"));
    }
}
