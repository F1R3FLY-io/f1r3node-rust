use std::sync::Arc;

use shared::rust::store::key_value_store::KeyValueStore;

use crate::rspace::errors::{AuthorityBackendMismatch, RootError};
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::history::cold_store::{ColdLeafAddress, load_cold_leaf};
use crate::rspace::history::instances::radix_history::RadixHistory;
use crate::rspace::shared::trie_exporter::{
    AuthenticatedTraversalPage, KeyHash, NodePath, StateExportError, TrieExporter, Value,
};
use crate::rspace::state::rspace_exporter::{RSpaceExporter, RSpaceExporterInstance};
use crate::rspace::state::rspace_state_authority::RSpaceStateAuthority;

// See rspace/src/main/scala/coop/rchain/rspace/state/instances/
// RSpaceExporterStore.scala
pub struct RSpaceExporterStore;

impl RSpaceExporterStore {
    pub fn create(authority: Arc<RSpaceStateAuthority>) -> impl RSpaceExporter {
        RSpaceExporterImpl {
            source_history_store: authority.history_store(),
            source_value_store: authority.cold_store(),
            authority,
        }
    }
}

pub struct RSpaceExporterImpl {
    source_history_store: Result<Arc<dyn KeyValueStore>, AuthorityBackendMismatch>,
    source_value_store: Result<Arc<dyn KeyValueStore>, AuthorityBackendMismatch>,
    authority: Arc<RSpaceStateAuthority>,
}

impl RSpaceExporterImpl {
    fn get_items(
        &self,
        store: Arc<dyn KeyValueStore>,
        keys: Vec<Blake2b256Hash>,
    ) -> Result<Vec<(Blake2b256Hash, Value)>, StateExportError> {
        let loaded = store.get(&keys.iter().map(|key| key.bytes()).collect())?;
        keys.into_iter()
            .zip(loaded)
            .map(|(expected, value)| {
                let value = value
                    .ok_or_else(|| StateExportError::MissingHistoryObject(expected.clone()))?;
                let actual = Blake2b256Hash::new(&value);
                if actual != expected {
                    return Err(StateExportError::CorruptHistoryObject { expected, actual });
                }
                Ok((expected, value))
            })
            .collect()
    }
}

impl RSpaceExporter for RSpaceExporterImpl {
    fn get_root(&self) -> Result<Blake2b256Hash, RootError> { self.authority.current_root() }
}

impl TrieExporter for RSpaceExporterImpl {
    fn get_page(
        &self,
        start_path: NodePath,
        skip: i32,
        take: i32,
    ) -> Result<AuthenticatedTraversalPage<KeyHash>, StateExportError> {
        let source_trie_store = RadixHistory::create_store(self.source_history_store.clone()?);

        RSpaceExporterInstance::traverse_history_page(
            start_path,
            skip,
            take,
            Arc::new(move |key| source_trie_store.get_one(key).map_err(Into::into)),
        )
    }

    fn get_history_items(
        &self,
        keys: Vec<Blake2b256Hash>,
    ) -> Result<Vec<(KeyHash, Value)>, StateExportError> {
        self.get_items(self.source_history_store.clone()?, keys)
    }

    fn get_data_items(
        &self,
        addresses: Vec<ColdLeafAddress>,
    ) -> Result<Vec<(KeyHash, Value)>, StateExportError> {
        let source_value_store = self.source_value_store.clone()?;
        addresses
            .into_iter()
            .map(|address| {
                let loaded = load_cold_leaf(source_value_store.as_ref(), &address)?;
                Ok((address.into_digest(), loaded.into_bytes()))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use shared::rust::store::key_value_store::KeyValueStore;

    use super::RSpaceExporterImpl;
    use crate::rspace::errors::AuthorityBackendMismatch;
    use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
    use crate::rspace::history::cold_store::{
        ColdLeafAddress, ContinuationsLeaf, DataLeaf, JoinsLeaf, PersistedData, PersistedDataKind,
        PersistedDataView,
    };
    use crate::rspace::history::instances::radix_history::RadixHistory;
    use crate::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
    use crate::rspace::shared::trie_exporter::{StateExportError, TrieExporter};
    use crate::rspace::state::exporters::rspace_exporter_items::RSpaceExporterItems;
    use crate::rspace::state::rspace_state_authority::RSpaceStateAuthority;

    fn encoded(kind: PersistedDataKind, payload: &[u8]) -> (ColdLeafAddress, Vec<u8>) {
        let bytes = match kind {
            PersistedDataKind::Data => PersistedData::Data(DataLeaf {
                bytes: payload.to_vec(),
            }),
            PersistedDataKind::Continuations => PersistedData::Continuations(ContinuationsLeaf {
                bytes: payload.to_vec(),
            }),
            PersistedDataKind::Joins => PersistedData::Joins(JoinsLeaf {
                bytes: payload.to_vec(),
            }),
        }
        .into_legacy_bincode();
        let digest = Blake2b256Hash::new(
            PersistedDataView::parse_canonical(&bytes)
                .unwrap()
                .leaf_encoding(),
        );
        (ColdLeafAddress::new(kind, digest).unwrap(), bytes)
    }

    #[test]
    fn empty_root_exports_as_a_zero_object_terminal_page_without_materialization() {
        let exporter =
            Arc::new(super::RSpaceExporterStore::create(RSpaceStateAuthority::in_memory()));
        let start = vec![(
            crate::rspace::history::instances::radix_history::RadixHistory::empty_root_node_hash(),
            None,
        )];

        let (history_page, data_page) =
            RSpaceExporterItems::get_history_and_data(exporter, start.clone(), 0, 1)
                .expect("canonical empty root export");

        assert!(history_page.items.is_empty());
        assert!(data_page.items.is_empty());
        assert_eq!(history_page.last_path, start);
        assert_eq!(data_page.last_path, history_page.last_path);
    }

    #[test]
    fn data_export_prefers_typed_keys_and_validates_legacy_fallbacks() {
        let values = Arc::new(InMemoryKeyValueStore::new());
        let unused = Arc::new(InMemoryKeyValueStore::new());
        let (typed, typed_bytes) = encoded(
            PersistedDataKind::Data,
            &bincode::serialize(&vec![b"typed".to_vec()]).unwrap(),
        );
        let (raw, raw_bytes) =
            encoded(PersistedDataKind::Data, &bincode::serialize(&vec![b"raw".to_vec()]).unwrap());
        let (legacy, legacy_bytes) = encoded(
            PersistedDataKind::Data,
            &bincode::serialize(&vec![b"legacy".to_vec()]).unwrap(),
        );
        values
            .put(vec![
                (typed.encode(), typed_bytes.clone()),
                (raw.digest().bytes(), raw_bytes.clone()),
                (bincode::serialize(legacy.digest()).unwrap(), legacy_bytes.clone()),
            ])
            .expect("populate the leaf store");
        let exporter = RSpaceExporterImpl {
            source_history_store: Ok(unused.clone()),
            source_value_store: Ok(values),
            authority: RSpaceStateAuthority::in_memory(),
        };

        let exported = exporter
            .get_data_items(vec![typed.clone(), raw.clone(), legacy.clone()])
            .expect("typed and legacy export");
        assert_eq!(exported, vec![
            (typed.into_digest(), typed_bytes),
            (raw.into_digest(), raw_bytes),
            (legacy.into_digest(), legacy_bytes),
        ]);
    }

    #[test]
    fn equal_payloads_of_different_kinds_coexist_at_distinct_physical_addresses() {
        let values = Arc::new(InMemoryKeyValueStore::new());
        let unused = Arc::new(InMemoryKeyValueStore::new());
        let payload = bincode::serialize(&vec![b"same-record".to_vec()]).unwrap();
        let (data, data_bytes) = encoded(PersistedDataKind::Data, &payload);
        let (continuations, continuation_bytes) =
            encoded(PersistedDataKind::Continuations, &payload);
        assert_eq!(data.digest(), continuations.digest());
        assert_ne!(data.encode(), continuations.encode());
        values
            .put(vec![
                (data.encode(), data_bytes.clone()),
                (continuations.encode(), continuation_bytes.clone()),
            ])
            .unwrap();
        let exporter = RSpaceExporterImpl {
            source_history_store: Ok(unused.clone()),
            source_value_store: Ok(values),
            authority: RSpaceStateAuthority::in_memory(),
        };

        let exported = exporter
            .get_data_items(vec![data.clone(), continuations.clone()])
            .unwrap();
        assert_eq!(exported, vec![
            (data.into_digest(), data_bytes),
            (continuations.into_digest(), continuation_bytes),
        ]);
    }

    #[test]
    fn export_reports_missing_and_corrupt_history_objects() {
        let history = Arc::new(InMemoryKeyValueStore::new());
        let unused = Arc::new(InMemoryKeyValueStore::new());
        let missing = Blake2b256Hash::new(b"missing-history");
        let corrupt = Blake2b256Hash::new(b"expected-history");
        history
            .put_one(corrupt.bytes(), b"wrong-history-bytes".to_vec())
            .unwrap();
        let exporter = RSpaceExporterImpl {
            source_history_store: Ok(history),
            source_value_store: Ok(unused.clone()),
            authority: RSpaceStateAuthority::in_memory(),
        };

        assert!(matches!(
            exporter.get_history_items(vec![missing.clone()]),
            Err(StateExportError::MissingHistoryObject(actual)) if actual == missing
        ));
        assert!(matches!(
            exporter.get_history_items(vec![corrupt.clone()]),
            Err(StateExportError::CorruptHistoryObject { expected, .. }) if expected == corrupt
        ));
    }

    #[test]
    fn export_does_not_fall_back_after_a_typed_kind_mismatch() {
        let values = Arc::new(InMemoryKeyValueStore::new());
        let unused = Arc::new(InMemoryKeyValueStore::new());
        let payload = bincode::serialize(&vec![b"record".to_vec()]).unwrap();
        let (data, data_bytes) = encoded(PersistedDataKind::Data, &payload);
        let (continuations, continuation_bytes) =
            encoded(PersistedDataKind::Continuations, &payload);
        assert_eq!(data.digest(), continuations.digest());
        values
            .put(vec![(data.encode(), continuation_bytes), (data.digest().bytes(), data_bytes)])
            .unwrap();
        let exporter = RSpaceExporterImpl {
            source_history_store: Ok(unused.clone()),
            source_value_store: Ok(values),
            authority: RSpaceStateAuthority::in_memory(),
        };

        assert!(matches!(exporter.get_data_items(vec![data]), Err(StateExportError::ColdLeaf(_))));
    }

    #[test]
    fn every_export_operation_refuses_an_unavailable_legacy_facet() {
        let exporter = RSpaceExporterImpl {
            source_history_store: Err(AuthorityBackendMismatch::History),
            source_value_store: Err(AuthorityBackendMismatch::Cold),
            authority: RSpaceStateAuthority::in_memory(),
        };
        let root = RadixHistory::empty_root_node_hash();
        let history_refusal = StateExportError::BackendMismatch(AuthorityBackendMismatch::History);

        assert!(matches!(
            exporter.get_page(vec![(root.clone(), None)], 0, 1),
            Err(refusal) if refusal == history_refusal
        ));
        assert_eq!(exporter.get_history_items(Vec::new()), Err(history_refusal.clone()));
        assert_eq!(exporter.get_history_items(vec![root]), Err(history_refusal));
        assert_eq!(
            exporter.get_data_items(Vec::new()),
            Err(StateExportError::BackendMismatch(AuthorityBackendMismatch::Cold))
        );
    }
}
