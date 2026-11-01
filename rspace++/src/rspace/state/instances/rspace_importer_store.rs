use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use shared::rust::ByteVector;
use shared::rust::store::key_value_store::KeyValueStore;

use crate::rspace::errors::AuthorityBackendMismatch;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::history::cold_store::{
    ColdLeafAddress, PersistedDataKind, PersistedDataView, put_cold_leaves_if_absent_verified,
};
use crate::rspace::history::instances::radix_history::RadixHistory;
use crate::rspace::history::radix_tree::{empty_node, encode};
use crate::rspace::serializers::cold_store_decode::ColdStoreDecode;
use crate::rspace::serializers::serializers::{decode_continuation, decode_datum, decode_join};
use crate::rspace::shared::trie_exporter::{AuthenticatedTraversalPage, NodePath, TrieNode};
use crate::rspace::state::closure_verifier::RecordValidator;
use crate::rspace::state::import_session::{
    SealedStateImport, StateImportError, StateImportPage, StateImportPageReceipt,
    StateImportPublication, StateImportPublicationWitness, StateImportSession, StateImportSpec,
};
use crate::rspace::state::rspace_exporter::RSpaceExporterInstance;
use crate::rspace::state::rspace_importer::RSpaceImporter;
use crate::rspace::state::rspace_state_authority::{
    RSpaceStateAuthority, SealedImportedRoots, StagedImport,
};

pub struct RSpaceImporterStore;

impl RSpaceImporterStore {
    pub fn create<C, P, A, K>(authority: Arc<RSpaceStateAuthority>) -> impl RSpaceImporter
    where
        C: Clone + ColdStoreDecode + 'static,
        P: Clone + ColdStoreDecode + 'static,
        A: Clone + ColdStoreDecode + 'static,
        K: Clone + ColdStoreDecode + 'static,
    {
        Self::create_with_validator(authority, record_validator::<C, P, A, K>())
    }

    pub(crate) fn create_with_validator(
        authority: Arc<RSpaceStateAuthority>,
        record_validator: RecordValidator,
    ) -> RSpaceImporterImpl {
        RSpaceImporterImpl {
            history_store: authority.history_store(),
            value_store: authority.cold_store(),
            authority,
            record_validator,
        }
    }
}

pub(crate) fn record_validator<C, P, A, K>() -> RecordValidator
where
    C: Clone + ColdStoreDecode + 'static,
    P: Clone + ColdStoreDecode + 'static,
    A: Clone + ColdStoreDecode + 'static,
    K: Clone + ColdStoreDecode + 'static,
{
    Arc::new(validate_record::<C, P, A, K>)
}

fn validate_record<C, P, A, K>(kind: PersistedDataKind, record: &[u8]) -> Result<(), String>
where
    C: Clone + ColdStoreDecode,
    P: Clone + ColdStoreDecode,
    A: Clone + ColdStoreDecode,
    K: Clone + ColdStoreDecode,
{
    match kind {
        PersistedDataKind::Data => decode_datum::<A>(record).map(drop),
        PersistedDataKind::Continuations => decode_continuation::<P, K>(record).map(drop),
        PersistedDataKind::Joins => decode_join::<C>(record).map(drop),
    }
    .map_err(|error| error.to_string())
}

#[derive(Clone)]
pub struct RSpaceImporterImpl {
    history_store: Result<Arc<dyn KeyValueStore>, AuthorityBackendMismatch>,
    value_store: Result<Arc<dyn KeyValueStore>, AuthorityBackendMismatch>,
    authority: Arc<RSpaceStateAuthority>,
    record_validator: RecordValidator,
}

impl RSpaceImporter for RSpaceImporterImpl {
    fn begin_import(
        &self,
        spec: StateImportSpec,
    ) -> Result<Box<dyn StateImportSession>, StateImportError> {
        let history_store = self.history_store.clone()?;
        let value_store = self.value_store.clone()?;
        let staged = self.authority.begin_import(spec.clone())?;
        let pending_paths = spec
            .roots()
            .iter()
            .cloned()
            .map(|root| vec![(root, None)])
            .collect();
        Ok(Box::new(RSpaceImportSessionImpl {
            history_store,
            value_store,
            authority: Arc::clone(&self.authority),
            record_validator: Arc::clone(&self.record_validator),
            spec,
            staged,
            pending_paths,
        }))
    }

    fn get_history_item(
        &self,
        hash: Blake2b256Hash,
    ) -> Result<Option<ByteVector>, StateImportError> {
        Ok(self.history_store.clone()?.get_one(&hash.0)?)
    }
}

struct RSpaceImportSessionImpl {
    history_store: Arc<dyn KeyValueStore>,
    value_store: Arc<dyn KeyValueStore>,
    authority: Arc<RSpaceStateAuthority>,
    record_validator: RecordValidator,
    spec: StateImportSpec,
    staged: StagedImport,
    pending_paths: BTreeSet<NodePath>,
}

impl StateImportSession for RSpaceImportSessionImpl {
    fn spec(&self) -> &StateImportSpec { &self.spec }

    fn validate_and_stage_page(
        &mut self,
        page: StateImportPage,
    ) -> Result<StateImportPageReceipt, StateImportError> {
        validate_page_shape(&self.spec, &self.pending_paths, &page)?;

        let history = Arc::new(validate_history_objects(page.history_items)?);
        let traversal = authenticated_projection(
            Arc::clone(&self.history_store),
            Arc::clone(&history),
            page.start_path.clone(),
            page.skip,
            page.take,
        )?;
        validate_history_projection(&traversal.nodes, history.as_ref())?;
        let expected_leaf_addresses = expected_leaf_addresses(&traversal.nodes)?;
        if page.data_items.len() != expected_leaf_addresses.len() {
            return Err(StateImportError::DataProjectionMismatch);
        }
        let leaves = validate_cold_leaves(page.data_items, &self.record_validator)?;
        if !expected_leaf_addresses.iter().eq(leaves.keys()) {
            return Err(StateImportError::DataProjectionMismatch);
        }

        let authenticated_next_path = traversal.next_path;
        if page.advertised_next_path != authenticated_next_path {
            return Err(StateImportError::AdvertisedCursorMismatch {
                advertised: page.advertised_next_path,
                authenticated: authenticated_next_path,
            });
        }

        // Both batches are fully validated before either store is touched.
        // They are immutable content-addressed staging objects; root authority
        // remains unreachable until the session is sealed and published.
        let history_objects_staged = history.len();
        let cold_leaves_staged = leaves.len();
        let history = Arc::into_inner(history).ok_or_else(|| {
            StateImportError::InternalInvariant(
                "authenticated traversal retained its page-history overlay".to_string(),
            )
        })?;
        self.history_store.put_if_absent_or_verify_equal(
            history
                .into_iter()
                .map(|(hash, bytes)| (hash.0, bytes))
                .collect(),
        )?;
        put_cold_leaves_if_absent_verified(
            self.value_store.as_ref(),
            leaves.into_iter().collect(),
        )?;

        let terminal = traversal.terminal;
        self.pending_paths.remove(&page.start_path);
        if !terminal {
            self.pending_paths.insert(authenticated_next_path.clone());
        }
        self.authority.accept_import_page(
            &mut self.staged,
            &page.start_path,
            authenticated_next_path.clone(),
            terminal,
        )?;

        Ok(StateImportPageReceipt {
            next_path: authenticated_next_path,
            terminal,
            history_objects_staged,
            cold_leaves_staged,
        })
    }

    fn seal(self: Box<Self>) -> Result<Box<dyn SealedStateImport>, StateImportError> {
        if !self.pending_paths.is_empty() {
            return Err(StateImportError::IncompletePagination(
                self.pending_paths.iter().cloned().collect(),
            ));
        }
        let sealed = self
            .authority
            .seal_import(self.staged, &self.record_validator)?;
        Ok(Box::new(SealedRSpaceImport {
            authority: self.authority,
            sealed,
        }))
    }
}

struct SealedRSpaceImport {
    authority: Arc<RSpaceStateAuthority>,
    sealed: SealedImportedRoots,
}

impl SealedStateImport for SealedRSpaceImport {
    fn spec(&self) -> &StateImportSpec { self.sealed.spec() }

    fn publish(
        self: Box<Self>,
        witness: &StateImportPublicationWitness,
    ) -> Result<StateImportPublication, StateImportError> {
        self.authority.publish_import(self.sealed, witness)
    }
}

fn validate_page_shape(
    spec: &StateImportSpec,
    pending_paths: &BTreeSet<NodePath>,
    page: &StateImportPage,
) -> Result<(), StateImportError> {
    if page.skip != 0 {
        return Err(StateImportError::InvalidPagination(format!(
            "affine cursor import requires skip 0, found {}",
            page.skip
        )));
    }
    if page.take <= 0 {
        return Err(StateImportError::InvalidPagination(format!(
            "take must be positive, found {}",
            page.take
        )));
    }
    if page.history_items.len() > page.take as usize {
        return Err(StateImportError::InvalidHistoryPageSize {
            take: page.take,
            received: page.history_items.len(),
        });
    }
    let Some((root, _)) = page.start_path.first() else {
        return Err(StateImportError::InvalidStartPath("cursor has no root component".to_string()));
    };
    if root.0.len() != 32 {
        return Err(StateImportError::InvalidStartPath(format!(
            "root component must contain 32 bytes, found {}",
            root.0.len()
        )));
    }
    if spec.roots().binary_search(root).is_err() {
        return Err(StateImportError::PageRootOutsideImport(root.clone()));
    }
    if !pending_paths.contains(&page.start_path) {
        return Err(StateImportError::UnexpectedStartPath(page.start_path.clone()));
    }
    Ok(())
}

fn validate_history_objects(
    items: Vec<(Blake2b256Hash, ByteVector)>,
) -> Result<BTreeMap<Blake2b256Hash, ByteVector>, StateImportError> {
    let mut validated = BTreeMap::new();
    for (expected, bytes) in items {
        if expected.0.len() != 32 {
            return Err(StateImportError::InvalidDigestWidth {
                field: "history object",
                available: expected.0.len(),
            });
        }
        let actual = Blake2b256Hash::new(&bytes);
        if actual != expected {
            return Err(StateImportError::HistoryDigestMismatch { expected, actual });
        }
        if validated.insert(expected.clone(), bytes).is_some() {
            return Err(StateImportError::DuplicateHistoryObject(expected));
        }
    }
    Ok(validated)
}

fn validate_cold_leaves(
    items: Vec<(Blake2b256Hash, ByteVector)>,
    validate_record: &RecordValidator,
) -> Result<BTreeMap<ColdLeafAddress, ByteVector>, StateImportError> {
    let mut validated = BTreeMap::new();
    for (expected, bytes) in items {
        if expected.0.len() != 32 {
            return Err(StateImportError::InvalidDigestWidth {
                field: "cold leaf",
                available: expected.0.len(),
            });
        }
        let envelope = PersistedDataView::parse_canonical(&bytes).map_err(|error| {
            crate::rspace::history::cold_store::ColdLeafStoreError::InvalidEnvelope(
                error.to_string(),
            )
        })?;
        let records = envelope
            .validate_records(envelope.kind(), None)
            .map_err(|error| {
                crate::rspace::history::cold_store::ColdLeafStoreError::InvalidEnvelope(
                    error.to_string(),
                )
            })?;
        let actual = Blake2b256Hash::new(records.leaf_encoding());
        if actual != expected {
            return Err(StateImportError::DataDigestMismatch { expected, actual });
        }
        let address = ColdLeafAddress::new(records.kind(), expected)?;
        for (index, record) in records.records().enumerate() {
            validate_record(address.kind(), record).map_err(|error| {
                StateImportError::MalformedRecord {
                    address: address.clone(),
                    index,
                    error,
                }
            })?;
        }
        if validated.insert(address.clone(), bytes).is_some() {
            return Err(StateImportError::DuplicateColdLeaf(address));
        }
    }
    Ok(validated)
}

fn authenticated_projection(
    history_store: Arc<dyn KeyValueStore>,
    page_history: Arc<BTreeMap<Blake2b256Hash, ByteVector>>,
    start_path: NodePath,
    skip: i32,
    take: i32,
) -> Result<AuthenticatedTraversalPage<Blake2b256Hash>, StateImportError> {
    let empty_root = RadixHistory::empty_root_node_hash();
    if skip == 0 && start_path.as_slice() == [(empty_root.clone(), None)] && page_history.is_empty()
    {
        // The canonical empty radix root is a protocol constant, not a
        // transferred object. Its only page is therefore the zero-object
        // terminal page; closure verification independently re-derives and
        // recognizes the same root digest.
        return Ok(AuthenticatedTraversalPage::from_raw_cursor(
            start_path.clone(),
            Vec::new(),
            start_path,
        )?);
    }
    Ok(RSpaceExporterInstance::traverse_history_page(
        start_path,
        skip,
        take,
        Arc::new(move |raw_hash| {
            let hash = Blake2b256Hash::from_bytes(raw_hash.clone());
            if let Some(bytes) = page_history.get(&hash) {
                return Ok(Some(bytes.clone()));
            }
            match history_store.get_one(raw_hash)? {
                Some(bytes) => Ok(Some(bytes)),
                None if hash == empty_root => Ok(Some(encode(&empty_node()))),
                None => Ok(None),
            }
        }),
    )?)
}

fn validate_history_projection(
    nodes: &[TrieNode<Blake2b256Hash>],
    history: &BTreeMap<Blake2b256Hash, ByteVector>,
) -> Result<(), StateImportError> {
    let expected_history = nodes
        .iter()
        .filter(|node| !node.is_leaf)
        .map(|node| node.hash.clone())
        .collect::<BTreeSet<_>>();
    if expected_history.len() != history.len() || !expected_history.iter().eq(history.keys()) {
        return Err(StateImportError::HistoryProjectionMismatch);
    }
    Ok(())
}

fn expected_leaf_addresses(
    nodes: &[TrieNode<Blake2b256Hash>],
) -> Result<BTreeSet<ColdLeafAddress>, StateImportError> {
    nodes
        .iter()
        .filter(|node| node.is_leaf)
        .map(|node| {
            node.leaf_address
                .clone()
                .ok_or(StateImportError::InvalidHistoryKeyPath { available: 0 })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::rspace::history::instances::radix_history::RadixHistory;
    use crate::rspace::state::import_session::{
        StateImportActivation, StateImportPublicationMode, StateImportPublicationWitness,
    };

    fn empty_importer() -> RSpaceImporterImpl {
        let authority = RSpaceStateAuthority::in_memory();
        RSpaceImporterImpl {
            history_store: authority.history_store(),
            value_store: authority.cold_store(),
            authority,
            record_validator: Arc::new(|_, _| Ok(())),
        }
    }

    #[test]
    fn empty_root_requires_a_complete_page_before_atomic_publication() {
        let importer = empty_importer();
        let root = RadixHistory::empty_root_node_hash();
        let spec = StateImportSpec::new(
            [root.clone()],
            StateImportPublicationMode::Activate(root.clone()),
        )
        .unwrap();
        let session = importer.begin_import(spec).unwrap();
        assert!(matches!(session.seal(), Err(StateImportError::IncompletePagination(_))));

        let mut session = importer
            .begin_import(
                StateImportSpec::new(
                    [root.clone()],
                    StateImportPublicationMode::Activate(root.clone()),
                )
                .unwrap(),
            )
            .unwrap();
        let start_path = vec![(root.clone(), None)];
        let receipt = session
            .validate_and_stage_page(StateImportPage {
                start_path: start_path.clone(),
                advertised_next_path: start_path,
                skip: 0,
                take: 64,
                history_items: Vec::new(),
                data_items: Vec::new(),
            })
            .unwrap();
        assert!(receipt.terminal);
        let publication = session
            .seal()
            .unwrap()
            .publish(&StateImportPublicationWitness::new())
            .unwrap();
        assert_eq!(
            publication.activation,
            Some(StateImportActivation {
                root,
                generation: 1,
            })
        );
    }

    #[test]
    fn malformed_page_cannot_mutate_any_store_or_publish_a_root() {
        let importer = empty_importer();
        let root = RadixHistory::empty_root_node_hash();
        let mut session = importer
            .begin_import(
                StateImportSpec::new(
                    [root.clone()],
                    StateImportPublicationMode::Activate(root.clone()),
                )
                .unwrap(),
            )
            .unwrap();
        let start_path = vec![(root.clone(), None)];
        let malformed = vec![9, 8, 7];
        let wrong_hash = Blake2b256Hash::new(b"different");
        let current_before = importer.authority.current_verified_root().unwrap();
        assert!(matches!(
            session.validate_and_stage_page(StateImportPage {
                start_path: start_path.clone(),
                advertised_next_path: start_path,
                skip: 0,
                take: 64,
                history_items: vec![(wrong_hash, malformed)],
                data_items: Vec::new(),
            }),
            Err(StateImportError::HistoryDigestMismatch { .. })
        ));
        assert!(
            !importer
                .history_store
                .as_ref()
                .expect("legacy history facet")
                .non_empty()
                .expect("history facet reads")
        );
        assert!(
            !importer
                .value_store
                .as_ref()
                .expect("legacy cold facet")
                .non_empty()
                .expect("cold facet reads")
        );
        assert_eq!(importer.authority.current_verified_root().unwrap(), current_before);
    }

    #[test]
    fn unauthenticated_cold_leaf_payload_is_rejected_before_record_decoding() {
        let importer = empty_importer();
        let root = RadixHistory::empty_root_node_hash();
        let mut session = importer
            .begin_import(
                StateImportSpec::new(
                    [root.clone()],
                    StateImportPublicationMode::Activate(root.clone()),
                )
                .unwrap(),
            )
            .unwrap();
        let start_path = vec![(root.clone(), None)];
        assert_eq!(
            session.validate_and_stage_page(StateImportPage {
                start_path: start_path.clone(),
                advertised_next_path: start_path.clone(),
                skip: 0,
                take: 64,
                history_items: Vec::new(),
                data_items: vec![(Blake2b256Hash::new(b"not authenticated"), vec![0xff])],
            }),
            Err(StateImportError::DataProjectionMismatch)
        );
        assert!(
            !importer
                .value_store
                .as_ref()
                .expect("legacy cold facet")
                .non_empty()
                .expect("cold facet reads")
        );

        // Rejection is affine but non-consuming: the exact requested cursor
        // remains retryable with its authenticated zero-object page.
        session
            .validate_and_stage_page(StateImportPage {
                start_path: start_path.clone(),
                advertised_next_path: start_path,
                skip: 0,
                take: 64,
                history_items: Vec::new(),
                data_items: Vec::new(),
            })
            .unwrap();
        assert_eq!(
            session
                .seal()
                .unwrap()
                .publish(&StateImportPublicationWitness::new())
                .unwrap()
                .verified_roots,
            vec![root]
        );
    }

    fn stage_empty_root(
        importer: &RSpaceImporterImpl,
        mode: StateImportPublicationMode,
    ) -> Box<dyn SealedStateImport> {
        let root = RadixHistory::empty_root_node_hash();
        let mut session = importer
            .begin_import(StateImportSpec::new([root.clone()], mode).unwrap())
            .unwrap();
        let start_path = vec![(root, None)];
        session
            .validate_and_stage_page(StateImportPage {
                start_path: start_path.clone(),
                advertised_next_path: start_path,
                skip: 0,
                take: 64,
                history_items: Vec::new(),
                data_items: Vec::new(),
            })
            .unwrap();
        session.seal().unwrap()
    }

    #[test]
    fn concurrent_activation_sessions_use_generation_cas_at_publication() {
        let importer = empty_importer();
        let root = RadixHistory::empty_root_node_hash();
        let first = stage_empty_root(&importer, StateImportPublicationMode::Activate(root.clone()));
        let stale = stage_empty_root(&importer, StateImportPublicationMode::Activate(root.clone()));
        let first_witness = StateImportPublicationWitness::new();
        assert_eq!(
            first
                .publish(&first_witness)
                .unwrap()
                .activation
                .unwrap()
                .generation,
            1
        );
        assert!(first_witness.authoritative_publication_observed());
        let stale_witness = StateImportPublicationWitness::new();
        assert!(matches!(
            stale.publish(&stale_witness),
            Err(StateImportError::Root(
                crate::rspace::errors::RootError::RootCatalogConflict { .. }
            ))
        ));
        assert!(!stale_witness.authoritative_publication_observed());
    }

    #[test]
    fn preserve_current_publication_reports_no_activation_and_cannot_move_current() {
        let importer = empty_importer();
        let current = importer.authority.current_verified_root().unwrap().unwrap();

        let witness = StateImportPublicationWitness::new();
        let publication = stage_empty_root(&importer, StateImportPublicationMode::PreserveCurrent)
            .publish(&witness)
            .unwrap();
        assert!(witness.authoritative_publication_observed());
        assert_eq!(publication.activation, None);
        assert_eq!(importer.authority.current_verified_root().unwrap().unwrap(), current);
        assert!(
            importer
                .authority
                .contains_verified_root(&RadixHistory::empty_root_node_hash())
                .unwrap()
        );
    }

    #[test]
    fn every_import_operation_refuses_an_unavailable_legacy_facet() {
        let authority = RSpaceStateAuthority::in_memory();
        let current_before = authority
            .current_verified_root()
            .expect("current root reads");
        let history_refused = RSpaceImporterImpl {
            history_store: Err(AuthorityBackendMismatch::History),
            value_store: authority.cold_store(),
            authority: Arc::clone(&authority),
            record_validator: Arc::new(|_, _| Ok(())),
        };
        let cold_refused = RSpaceImporterImpl {
            history_store: authority.history_store(),
            value_store: Err(AuthorityBackendMismatch::Cold),
            authority: Arc::clone(&authority),
            record_validator: Arc::new(|_, _| Ok(())),
        };
        let root = RadixHistory::empty_root_node_hash();
        let spec = StateImportSpec::new(
            [root.clone()],
            StateImportPublicationMode::Activate(root.clone()),
        )
        .expect("activation import spec");

        assert!(matches!(
            history_refused.begin_import(spec.clone()),
            Err(StateImportError::BackendMismatch(AuthorityBackendMismatch::History))
        ));
        assert!(matches!(
            cold_refused.begin_import(spec),
            Err(StateImportError::BackendMismatch(AuthorityBackendMismatch::Cold))
        ));
        assert_eq!(
            history_refused.get_history_item(root),
            Err(StateImportError::BackendMismatch(AuthorityBackendMismatch::History))
        );
        assert_eq!(
            authority
                .current_verified_root()
                .expect("current root reads"),
            current_before
        );
    }
}
