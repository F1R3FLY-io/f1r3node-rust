use super::KeyValueDagRepresentation;

pub struct DagLiveResidentDiagnostics {
    pub height_buckets: usize,
    pub height_entries: usize,
    pub minimum: Option<i64>,
    pub maximum: Option<i64>,
    pub cutoff: i64,
    pub max_height: Option<i64>,
    pub blocks: usize,
    pub edges: usize,
}

pub struct DagResidentDiagnostics {
    pub minimum: Option<i64>,
    pub maximum: Option<i64>,
    pub blocks: usize,
    pub edges: usize,
    pub child_keys: usize,
    pub heights: usize,
    pub block_numbers: usize,
    pub main_parents: usize,
    pub self_justifications: usize,
    pub finalized: usize,
    pub latest_messages: usize,
}

impl KeyValueDagRepresentation {
    pub fn resident_diagnostics(&self) -> DagResidentDiagnostics {
        assert_eq!(self.dag_set.len(), self.block_number_map.len());
        assert!(self
            .dag_set
            .iter()
            .all(|hash| self.block_number_map.contains_key(hash)));
        assert!(self
            .main_parent_map
            .keys()
            .all(|hash| self.dag_set.contains(hash)));
        assert!(self
            .self_justification_map
            .keys()
            .all(|hash| self.dag_set.contains(hash)));
        assert!(self
            .finalized_blocks_set
            .iter()
            .all(|hash| self.dag_set.contains(hash)));
        assert!(self.height_map.iter().all(|(height, hashes)| {
            !hashes.is_empty()
                && hashes
                    .iter()
                    .all(|hash| self.block_number_map.get(hash) == Some(height))
        }));
        assert!(self.child_map.iter().all(|(parent, children)| {
            (self.dag_set.contains(parent) || !children.is_empty())
                && children.iter().all(|child| self.dag_set.contains(child))
        }));
        DagResidentDiagnostics {
            minimum: self.block_number_map.values().copied().min(),
            maximum: self.block_number_map.values().copied().max(),
            blocks: self.dag_set.len(),
            edges: self.child_map.values().map(|children| children.len()).sum(),
            child_keys: self.child_map.len(),
            heights: self.height_map.len(),
            block_numbers: self.block_number_map.len(),
            main_parents: self.main_parent_map.len(),
            self_justifications: self.self_justification_map.len(),
            finalized: self.finalized_blocks_set.len(),
            latest_messages: self.latest_messages_map.len(),
        }
    }
}
