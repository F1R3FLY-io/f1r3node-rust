use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use models::rspace_plus_plus_types::state_import_result_proto::Outcome;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::state::import_session::StateImportActivation;

use super::*;

#[derive(Default)]
struct LifecycleCounts {
    pages: AtomicUsize,
    seals: AtomicUsize,
    publications: AtomicUsize,
    authoritative_publications: AtomicUsize,
    open_drops: AtomicUsize,
    sealed_drops: AtomicUsize,
}

#[derive(Clone, Copy)]
enum StageBehavior {
    Success,
    Error,
    Panic,
}

#[derive(Clone, Copy)]
enum SealBehavior {
    Success,
    Error,
    Panic,
}

#[derive(Clone, Copy)]
enum PublishBehavior {
    Success,
    Error,
    PanicBeforeEffect,
    PanicAfterEffect,
}

#[derive(Clone, Copy, Debug)]
enum BoundaryOperation {
    Begin,
    StagePage,
    Seal,
    Publish,
    AbortOpen,
    AbortSealed,
}

#[derive(Clone, Copy, Debug)]
enum BoundaryFaultPhase {
    BeforeOperation,
    BeforePrimaryEncoding,
    BeforeFallbackEncoding,
}

const BOUNDARY_OPERATIONS: [BoundaryOperation; 6] = [
    BoundaryOperation::Begin,
    BoundaryOperation::StagePage,
    BoundaryOperation::Seal,
    BoundaryOperation::Publish,
    BoundaryOperation::AbortOpen,
    BoundaryOperation::AbortSealed,
];

const BOUNDARY_FAULT_PHASES: [BoundaryFaultPhase; 3] = [
    BoundaryFaultPhase::BeforeOperation,
    BoundaryFaultPhase::BeforePrimaryEncoding,
    BoundaryFaultPhase::BeforeFallbackEncoding,
];

struct TestOpenSession {
    spec: StateImportSpec,
    counts: Arc<LifecycleCounts>,
    stage_behavior: StageBehavior,
    seal_behavior: SealBehavior,
}

impl Drop for TestOpenSession {
    fn drop(&mut self) { self.counts.open_drops.fetch_add(1, Ordering::SeqCst); }
}

impl StateImportSession for TestOpenSession {
    fn spec(&self) -> &StateImportSpec { &self.spec }

    fn validate_and_stage_page(
        &mut self,
        page: StateImportPage,
    ) -> Result<StateImportPageReceipt, StateImportError> {
        match self.stage_behavior {
            StageBehavior::Success => {
                self.counts.pages.fetch_add(1, Ordering::SeqCst);
                Ok(StateImportPageReceipt {
                    next_path: page.advertised_next_path,
                    terminal: true,
                    history_objects_staged: page.history_items.len(),
                    cold_leaves_staged: page.data_items.len(),
                })
            }
            StageBehavior::Error => {
                Err(StateImportError::InvalidPagination("test page rejected".to_string()))
            }
            StageBehavior::Panic => panic!("test stage panic"),
        }
    }

    fn seal(self: Box<Self>) -> Result<Box<dyn SealedStateImport>, StateImportError> {
        self.counts.seals.fetch_add(1, Ordering::SeqCst);
        match self.seal_behavior {
            SealBehavior::Error => {
                return Err(StateImportError::IncompletePagination(Vec::new()));
            }
            SealBehavior::Panic => panic!("test seal panic"),
            SealBehavior::Success => {}
        }
        Ok(Box::new(TestSealedSession {
            spec: self.spec.clone(),
            counts: Arc::clone(&self.counts),
            publish_behavior: PublishBehavior::Success,
        }))
    }
}

struct TestSealedSession {
    spec: StateImportSpec,
    counts: Arc<LifecycleCounts>,
    publish_behavior: PublishBehavior,
}

impl Drop for TestSealedSession {
    fn drop(&mut self) { self.counts.sealed_drops.fetch_add(1, Ordering::SeqCst); }
}

impl SealedStateImport for TestSealedSession {
    fn spec(&self) -> &StateImportSpec { &self.spec }

    fn publish(
        self: Box<Self>,
        witness: &StateImportPublicationWitness,
    ) -> Result<StateImportPublication, StateImportError> {
        self.counts.publications.fetch_add(1, Ordering::SeqCst);
        match self.publish_behavior {
            PublishBehavior::Error => {
                return Err(StateImportError::InternalInvariant(
                    "test publication rejected".to_string(),
                ));
            }
            PublishBehavior::PanicBeforeEffect => panic!("test pre-publication panic"),
            PublishBehavior::PanicAfterEffect => {
                self.counts
                    .authoritative_publications
                    .fetch_add(1, Ordering::SeqCst);
                witness.record_authoritative_publication();
                panic!("test post-publication panic");
            }
            PublishBehavior::Success => {
                self.counts
                    .authoritative_publications
                    .fetch_add(1, Ordering::SeqCst);
                witness.record_authoritative_publication();
            }
        }
        let activation = match self.spec.publication_mode() {
            StateImportPublicationMode::PreserveCurrent => None,
            StateImportPublicationMode::Activate(root) => Some(StateImportActivation {
                root: root.clone(),
                generation: 7,
            }),
        };
        Ok(StateImportPublication {
            verified_roots: self.spec.roots().to_vec(),
            activation,
        })
    }
}

fn digest(byte: u8) -> Blake2b256Hash { Blake2b256Hash::from_bytes(vec![byte; 32]) }

fn activating_spec(root: &Blake2b256Hash) -> StateImportSpec {
    StateImportSpec::new([root.clone()], StateImportPublicationMode::Activate(root.clone()))
        .expect("valid test import spec")
}

fn protobuf_path(root: &Blake2b256Hash) -> Vec<PathElement> {
    vec![PathElement {
        key_hash: root.bytes(),
        optional_byte: Vec::new(),
    }]
}

fn protobuf_page(root: &Blake2b256Hash) -> Vec<u8> {
    StateImportPageProto {
        start_path: protobuf_path(root),
        advertised_next_path: protobuf_path(root),
        skip: 0,
        take: 1,
        history_items: Vec::new(),
        data_items: Vec::new(),
    }
    .encode_to_vec()
}

fn protobuf_spec(root: &Blake2b256Hash) -> Vec<u8> {
    StateImportSpecProto {
        target_roots: vec![root.bytes()],
        publication_mode: StateImportPublicationModeProto::StateImportPublicationModeActivate
            as i32,
        activation_root: root.bytes(),
    }
    .encode_to_vec()
}

fn in_memory_space() -> *mut Space {
    let mut manager = InMemoryStoreManager::new();
    let store = blocking_runtime()
        .block_on(manager.r_space_stores())
        .expect("in-memory RSpace stores");
    let rspace = RSpace::create(store, Arc::new(Box::new(Matcher))).expect("in-memory RSpace");
    Box::into_raw(Box::new(Space {
        rspace: Mutex::new(rspace),
    }))
}

fn open_handle(
    spec: StateImportSpec,
    counts: Arc<LifecycleCounts>,
    stage_behavior: StageBehavior,
    seal_behavior: SealBehavior,
) -> *mut OpenStateImportHandle {
    Box::into_raw(Box::new(OpenStateImportHandle {
        session: Mutex::new(Some(Box::new(TestOpenSession {
            spec,
            counts,
            stage_behavior,
            seal_behavior,
        }))),
    }))
}

fn sealed_handle(
    spec: StateImportSpec,
    counts: Arc<LifecycleCounts>,
    publish_behavior: PublishBehavior,
) -> *mut SealedStateImportHandle {
    Box::into_raw(Box::new(SealedStateImportHandle {
        session: Mutex::new(Some(Box::new(TestSealedSession {
            spec,
            counts,
            publish_behavior,
        }))),
    }))
}

fn decode_and_free(pointer: *const u8) -> StateImportResultProto {
    assert!(!pointer.is_null(), "state-import FFI must return a structured result");
    // SAFETY: every state-import extern returns a four-byte length followed
    // by exactly that many protobuf bytes allocated by this crate.
    let header = unsafe { std::slice::from_raw_parts(pointer, 4) };
    let protobuf_len = u32::from_le_bytes(header.try_into().expect("four-byte header")) as usize;
    let protobuf_bytes = unsafe { std::slice::from_raw_parts(pointer.add(4), protobuf_len) };
    let protobuf_result =
        StateImportResultProto::decode(protobuf_bytes).expect("decode FFI result");
    deallocate_memory(pointer.cast_mut(), protobuf_len + 4);
    protobuf_result
}

fn success(result: StateImportResultProto) -> StateImportSuccessProto {
    match result.outcome.expect("result outcome") {
        Outcome::Success(success) => success,
        Outcome::Error(error) => panic!("unexpected state-import FFI error: {error:?}"),
    }
}

fn error(result: StateImportResultProto) -> StateImportErrorProto {
    match result.outcome.expect("result outcome") {
        Outcome::Error(error) => error,
        Outcome::Success(success) => panic!("unexpected state-import FFI success: {success:?}"),
    }
}

fn decode_if_present(pointer: *const u8) -> Option<StateImportResultProto> {
    (!pointer.is_null()).then(|| decode_and_free(pointer))
}

fn boundary_fault_guard(phase: BoundaryFaultPhase) -> StateImportFfiTestPlanGuard {
    match phase {
        BoundaryFaultPhase::BeforeOperation => {
            StateImportFfiTestPlanGuard::install(&[StateImportFfiTestPhase::BeforeOperation])
        }
        BoundaryFaultPhase::BeforePrimaryEncoding => {
            StateImportFfiTestPlanGuard::install(&[StateImportFfiTestPhase::BeforePrimaryEncoding])
        }
        BoundaryFaultPhase::BeforeFallbackEncoding => StateImportFfiTestPlanGuard::install(&[
            StateImportFfiTestPhase::BeforeOperation,
            StateImportFfiTestPhase::BeforeFallbackEncoding,
        ]),
    }
}

fn assert_boundary_fault_response(
    pointer: *const u8,
    operation: BoundaryOperation,
    phase: BoundaryFaultPhase,
) {
    let must_be_null = matches!(phase, BoundaryFaultPhase::BeforeFallbackEncoding) ||
        matches!(
            (operation, phase),
            (BoundaryOperation::Publish, BoundaryFaultPhase::BeforePrimaryEncoding)
        );
    if must_be_null {
        assert!(pointer.is_null(), "{operation:?}/{phase:?} must return null");
    } else {
        let result = decode_if_present(pointer)
            .unwrap_or_else(|| panic!("{operation:?}/{phase:?} must return a panic result"));
        assert_eq!(
            error(result).code,
            PROTOBUF_STATE_IMPORT_ERROR_PANIC,
            "{operation:?}/{phase:?}",
        );
    }
}

fn assert_boundary_fault_cell(operation: BoundaryOperation, phase: BoundaryFaultPhase, seed: u8) {
    let root = digest(seed);
    match operation {
        BoundaryOperation::Begin => {
            let spec = protobuf_spec(&root);
            let space = in_memory_space();
            let mut open = 1_usize as *mut OpenStateImportHandle;
            let guard = boundary_fault_guard(phase);
            let pointer =
                unsafe { state_import_begin(space, spec.as_ptr(), spec.len(), &mut open) };
            assert_boundary_fault_response(pointer, operation, phase);
            assert!(open.is_null(), "{operation:?}/{phase:?} exposed an open capability");
            drop(guard);
            // SAFETY: this test uniquely owns the allocation returned by in_memory_space.
            unsafe { drop(Box::from_raw(space)) };
        }
        BoundaryOperation::StagePage => {
            let counts = Arc::new(LifecycleCounts::default());
            let open = open_handle(
                activating_spec(&root),
                Arc::clone(&counts),
                StageBehavior::Success,
                SealBehavior::Success,
            );
            let page = protobuf_page(&root);
            let guard = boundary_fault_guard(phase);
            let pointer = unsafe { state_import_stage_page(open, page.as_ptr(), page.len()) };
            assert_boundary_fault_response(pointer, operation, phase);
            drop(guard);

            if matches!(phase, BoundaryFaultPhase::BeforePrimaryEncoding) {
                assert_eq!(counts.pages.load(Ordering::SeqCst), 1);
                let abort = error(decode_and_free(unsafe { state_import_abort_open(open) }));
                assert_eq!(abort.code, PROTOBUF_STATE_IMPORT_ERROR_INVALID_HANDLE_STATE);
            } else {
                assert_eq!(counts.pages.load(Ordering::SeqCst), 0);
                success(decode_and_free(unsafe { state_import_abort_open(open) }));
            }
            assert_eq!(counts.open_drops.load(Ordering::SeqCst), 1);
        }
        BoundaryOperation::Seal => {
            let counts = Arc::new(LifecycleCounts::default());
            let open = open_handle(
                activating_spec(&root),
                Arc::clone(&counts),
                StageBehavior::Success,
                SealBehavior::Success,
            );
            let mut sealed = 1_usize as *mut SealedStateImportHandle;
            let guard = boundary_fault_guard(phase);
            let pointer = unsafe { state_import_seal(open, &mut sealed) };
            assert_boundary_fault_response(pointer, operation, phase);
            assert!(sealed.is_null(), "{operation:?}/{phase:?} exposed a sealed capability");
            drop(guard);

            if matches!(phase, BoundaryFaultPhase::BeforePrimaryEncoding) {
                assert_eq!(counts.seals.load(Ordering::SeqCst), 1);
                assert_eq!(counts.open_drops.load(Ordering::SeqCst), 1);
                assert_eq!(counts.sealed_drops.load(Ordering::SeqCst), 1);
            } else {
                assert_eq!(counts.seals.load(Ordering::SeqCst), 0);
                success(decode_and_free(unsafe { state_import_abort_open(open) }));
                assert_eq!(counts.open_drops.load(Ordering::SeqCst), 1);
                assert_eq!(counts.sealed_drops.load(Ordering::SeqCst), 0);
            }
        }
        BoundaryOperation::Publish => {
            let counts = Arc::new(LifecycleCounts::default());
            let sealed = sealed_handle(
                activating_spec(&root),
                Arc::clone(&counts),
                PublishBehavior::Success,
            );
            let guard = boundary_fault_guard(phase);
            let pointer = unsafe { state_import_publish(sealed) };
            assert_boundary_fault_response(pointer, operation, phase);
            drop(guard);

            if matches!(phase, BoundaryFaultPhase::BeforePrimaryEncoding) {
                assert_eq!(counts.publications.load(Ordering::SeqCst), 1);
                assert_eq!(counts.authoritative_publications.load(Ordering::SeqCst), 1);
            } else {
                assert_eq!(counts.publications.load(Ordering::SeqCst), 0);
                assert_eq!(counts.authoritative_publications.load(Ordering::SeqCst), 0);
                success(decode_and_free(unsafe { state_import_abort_sealed(sealed) }));
            }
            assert_eq!(counts.sealed_drops.load(Ordering::SeqCst), 1);
        }
        BoundaryOperation::AbortOpen => {
            let counts = Arc::new(LifecycleCounts::default());
            let open = open_handle(
                activating_spec(&root),
                Arc::clone(&counts),
                StageBehavior::Success,
                SealBehavior::Success,
            );
            let guard = boundary_fault_guard(phase);
            let pointer = unsafe { state_import_abort_open(open) };
            assert_boundary_fault_response(pointer, operation, phase);
            drop(guard);

            if !matches!(phase, BoundaryFaultPhase::BeforePrimaryEncoding) {
                success(decode_and_free(unsafe { state_import_abort_open(open) }));
            }
            assert_eq!(counts.open_drops.load(Ordering::SeqCst), 1);
            assert_eq!(counts.publications.load(Ordering::SeqCst), 0);
        }
        BoundaryOperation::AbortSealed => {
            let counts = Arc::new(LifecycleCounts::default());
            let sealed = sealed_handle(
                activating_spec(&root),
                Arc::clone(&counts),
                PublishBehavior::Success,
            );
            let guard = boundary_fault_guard(phase);
            let pointer = unsafe { state_import_abort_sealed(sealed) };
            assert_boundary_fault_response(pointer, operation, phase);
            drop(guard);

            if !matches!(phase, BoundaryFaultPhase::BeforePrimaryEncoding) {
                success(decode_and_free(unsafe { state_import_abort_sealed(sealed) }));
            }
            assert_eq!(counts.sealed_drops.load(Ordering::SeqCst), 1);
            assert_eq!(counts.publications.load(Ordering::SeqCst), 0);
        }
    }
}

#[test]
fn all_six_operations_satisfy_the_exhaustive_boundary_fault_matrix() {
    let mut seed = 40_u8;
    for operation in BOUNDARY_OPERATIONS {
        for phase in BOUNDARY_FAULT_PHASES {
            assert_boundary_fault_cell(operation, phase, seed);
            seed = seed.wrapping_add(1);
        }
    }
}

#[test]
fn two_sequential_publications_are_distinct_and_each_exactly_once() {
    let root = digest(70);
    let counts = Arc::new(LifecycleCounts::default());
    for _ in 0..2 {
        let sealed =
            sealed_handle(activating_spec(&root), Arc::clone(&counts), PublishBehavior::Success);
        let result = success(decode_and_free(unsafe { state_import_publish(sealed) }));
        assert_eq!(result.kind, PROTOBUF_STATE_IMPORT_SUCCESS_PUBLISHED);
    }
    assert_eq!(counts.publications.load(Ordering::SeqCst), 2);
    assert_eq!(counts.authoritative_publications.load(Ordering::SeqCst), 2);
    assert_eq!(counts.sealed_drops.load(Ordering::SeqCst), 2);
}

#[test]
fn open_page_seal_publish_is_affine_and_returns_typed_receipts() {
    let root = digest(9);
    let counts = Arc::new(LifecycleCounts::default());
    let open = open_handle(
        activating_spec(&root),
        Arc::clone(&counts),
        StageBehavior::Success,
        SealBehavior::Success,
    );
    let page = protobuf_page(&root);

    let staged = success(decode_and_free(unsafe {
        state_import_stage_page(open, page.as_ptr(), page.len())
    }));
    assert_eq!(staged.kind, PROTOBUF_STATE_IMPORT_SUCCESS_PAGE_STAGED);
    let receipt = staged.page_receipt.expect("page receipt");
    assert!(receipt.terminal);
    assert_eq!(receipt.authenticated_next_path, protobuf_path(&root));

    let mut sealed = std::ptr::null_mut();
    let sealed_result = success(decode_and_free(unsafe { state_import_seal(open, &mut sealed) }));
    assert_eq!(sealed_result.kind, PROTOBUF_STATE_IMPORT_SUCCESS_SEALED);
    assert!(!sealed.is_null());

    let published = success(decode_and_free(unsafe { state_import_publish(sealed) }));
    assert_eq!(published.kind, PROTOBUF_STATE_IMPORT_SUCCESS_PUBLISHED);
    let publication = published.publication.expect("publication receipt");
    assert_eq!(publication.verified_roots, vec![root.bytes()]);
    assert_eq!(publication.activation.expect("activation").generation, 7);
    assert_eq!(counts.pages.load(Ordering::SeqCst), 1);
    assert_eq!(counts.seals.load(Ordering::SeqCst), 1);
    assert_eq!(counts.publications.load(Ordering::SeqCst), 1);
}

#[test]
fn validation_failure_does_not_publish_and_open_handle_can_be_aborted() {
    let root = digest(10);
    let counts = Arc::new(LifecycleCounts::default());
    let open = open_handle(
        activating_spec(&root),
        Arc::clone(&counts),
        StageBehavior::Error,
        SealBehavior::Success,
    );
    let page = protobuf_page(&root);

    let rejected =
        error(decode_and_free(unsafe { state_import_stage_page(open, page.as_ptr(), page.len()) }));
    assert_eq!(rejected.code, PROTOBUF_STATE_IMPORT_ERROR_IMPORT);
    let aborted = success(decode_and_free(unsafe { state_import_abort_open(open) }));
    assert_eq!(aborted.kind, PROTOBUF_STATE_IMPORT_SUCCESS_ABORTED);
    assert_eq!(counts.seals.load(Ordering::SeqCst), 0);
    assert_eq!(counts.publications.load(Ordering::SeqCst), 0);
}

#[test]
fn null_decode_length_import_and_panic_failures_are_structured() {
    let begin_root = digest(8);
    let protobuf_spec = StateImportSpecProto {
        target_roots: vec![begin_root.bytes()],
        publication_mode: StateImportPublicationModeProto::StateImportPublicationModeActivate
            as i32,
        activation_root: begin_root.bytes(),
    }
    .encode_to_vec();
    let mut open_output = 1_usize as *mut OpenStateImportHandle;
    let begin_error = error(decode_and_free(unsafe {
        state_import_begin(
            std::ptr::null_mut(),
            protobuf_spec.as_ptr(),
            protobuf_spec.len(),
            &mut open_output,
        )
    }));
    assert_eq!(begin_error.code, PROTOBUF_STATE_IMPORT_ERROR_NULL_POINTER);
    assert!(open_output.is_null(), "failed begin must leave no open capability");

    let null_handle = error(decode_and_free(unsafe {
        state_import_stage_page(std::ptr::null_mut(), std::ptr::null(), 0)
    }));
    assert_eq!(null_handle.code, PROTOBUF_STATE_IMPORT_ERROR_NULL_POINTER);

    let root = digest(11);
    let counts = Arc::new(LifecycleCounts::default());
    let decode_open = open_handle(
        activating_spec(&root),
        Arc::clone(&counts),
        StageBehavior::Success,
        SealBehavior::Success,
    );
    let malformed = [0xff];
    let decode_error = error(decode_and_free(unsafe {
        state_import_stage_page(decode_open, malformed.as_ptr(), malformed.len())
    }));
    assert_eq!(decode_error.code, PROTOBUF_STATE_IMPORT_ERROR_DECODE);
    success(decode_and_free(unsafe { state_import_abort_open(decode_open) }));

    let length_open = open_handle(
        activating_spec(&root),
        Arc::clone(&counts),
        StageBehavior::Success,
        SealBehavior::Success,
    );
    let invalid_path = StateImportPageProto {
        start_path: vec![PathElement {
            key_hash: vec![0; 31],
            optional_byte: Vec::new(),
        }],
        advertised_next_path: protobuf_path(&root),
        skip: 0,
        take: 1,
        history_items: Vec::new(),
        data_items: Vec::new(),
    }
    .encode_to_vec();
    let length_error = error(decode_and_free(unsafe {
        state_import_stage_page(length_open, invalid_path.as_ptr(), invalid_path.len())
    }));
    assert_eq!(length_error.code, PROTOBUF_STATE_IMPORT_ERROR_INVALID_LENGTH);
    success(decode_and_free(unsafe { state_import_abort_open(length_open) }));

    let panic_open = open_handle(
        activating_spec(&root),
        Arc::clone(&counts),
        StageBehavior::Panic,
        SealBehavior::Success,
    );
    let page = protobuf_page(&root);
    let panic_error = error(decode_and_free(unsafe {
        state_import_stage_page(panic_open, page.as_ptr(), page.len())
    }));
    assert_eq!(panic_error.code, PROTOBUF_STATE_IMPORT_ERROR_PANIC);
    let poisoned_abort = error(decode_and_free(unsafe { state_import_abort_open(panic_open) }));
    assert_eq!(poisoned_abort.code, PROTOBUF_STATE_IMPORT_ERROR_INVALID_HANDLE_STATE);
    assert_eq!(counts.publications.load(Ordering::SeqCst), 0);
}

#[test]
fn failed_seal_or_publish_consumes_authority_without_publication_receipt() {
    let root = digest(12);
    let counts = Arc::new(LifecycleCounts::default());
    let open = open_handle(
        activating_spec(&root),
        Arc::clone(&counts),
        StageBehavior::Success,
        SealBehavior::Error,
    );
    let mut sealed = 1_usize as *mut SealedStateImportHandle;
    let seal_error = error(decode_and_free(unsafe { state_import_seal(open, &mut sealed) }));
    assert_eq!(seal_error.code, PROTOBUF_STATE_IMPORT_ERROR_IMPORT);
    assert!(sealed.is_null(), "failed seal must leave no publication capability");

    let sealed = sealed_handle(activating_spec(&root), Arc::clone(&counts), PublishBehavior::Error);
    let publish_error = error(decode_and_free(unsafe { state_import_publish(sealed) }));
    assert_eq!(publish_error.code, PROTOBUF_STATE_IMPORT_ERROR_IMPORT);
    assert_eq!(counts.seals.load(Ordering::SeqCst), 1);
    assert_eq!(counts.publications.load(Ordering::SeqCst), 1);
}

#[test]
fn abort_sealed_consumes_capability_without_publishing() {
    let root = digest(13);
    let counts = Arc::new(LifecycleCounts::default());
    let sealed =
        sealed_handle(activating_spec(&root), Arc::clone(&counts), PublishBehavior::Success);
    let aborted = success(decode_and_free(unsafe { state_import_abort_sealed(sealed) }));
    assert_eq!(aborted.kind, PROTOBUF_STATE_IMPORT_SUCCESS_ABORTED);
    assert_eq!(counts.publications.load(Ordering::SeqCst), 0);
}

#[test]
fn begin_primary_encoding_failure_exposes_no_open_capability() {
    let root = digest(14);
    let spec = protobuf_spec(&root);
    let space = in_memory_space();
    let mut open = std::ptr::null_mut();
    let guard =
        StateImportFfiTestPlanGuard::install(&[StateImportFfiTestPhase::BeforePrimaryEncoding]);

    let pointer = unsafe { state_import_begin(space, spec.as_ptr(), spec.len(), &mut open) };
    let result = decode_if_present(pointer).expect("the fallback panic result must encode");
    assert_eq!(error(result).code, PROTOBUF_STATE_IMPORT_ERROR_PANIC);
    drop(guard);

    let exposed = !open.is_null();
    if exposed {
        let _ = decode_and_free(unsafe { state_import_abort_open(open) });
    }
    // SAFETY: this test uniquely owns the allocation returned by in_memory_space.
    unsafe { drop(Box::from_raw(space)) };
    assert!(!exposed, "FFI-03/04: begin exposed an unencoded open capability");
}

#[test]
fn pre_operation_unwind_nulls_outputs_without_consuming_input_capabilities() {
    let root = digest(21);
    let spec = protobuf_spec(&root);
    let space = in_memory_space();
    let mut open_output = 1_usize as *mut OpenStateImportHandle;
    let begin_guard =
        StateImportFfiTestPlanGuard::install(&[StateImportFfiTestPhase::BeforeOperation]);
    let begin_result = error(decode_and_free(unsafe {
        state_import_begin(space, spec.as_ptr(), spec.len(), &mut open_output)
    }));
    assert_eq!(begin_result.code, PROTOBUF_STATE_IMPORT_ERROR_PANIC);
    assert!(open_output.is_null(), "FFI-05: failed begin left a capability output");
    drop(begin_guard);
    // SAFETY: this test uniquely owns the allocation returned by in_memory_space.
    unsafe { drop(Box::from_raw(space)) };

    let counts = Arc::new(LifecycleCounts::default());
    let open = open_handle(
        activating_spec(&root),
        Arc::clone(&counts),
        StageBehavior::Success,
        SealBehavior::Success,
    );
    let mut sealed_output = 1_usize as *mut SealedStateImportHandle;
    let seal_guard =
        StateImportFfiTestPlanGuard::install(&[StateImportFfiTestPhase::BeforeOperation]);
    let seal_result =
        error(decode_and_free(unsafe { state_import_seal(open, &mut sealed_output) }));
    assert_eq!(seal_result.code, PROTOBUF_STATE_IMPORT_ERROR_PANIC);
    assert!(sealed_output.is_null(), "FFI-05: failed seal left a capability output");
    drop(seal_guard);

    let aborted = success(decode_and_free(unsafe { state_import_abort_open(open) }));
    assert_eq!(aborted.kind, PROTOBUF_STATE_IMPORT_SUCCESS_ABORTED);
    assert_eq!(counts.seals.load(Ordering::SeqCst), 0);
    assert_eq!(counts.publications.load(Ordering::SeqCst), 0);
}

#[test]
fn seal_primary_encoding_failure_exposes_no_sealed_capability() {
    let root = digest(15);
    let counts = Arc::new(LifecycleCounts::default());
    let open = open_handle(
        activating_spec(&root),
        Arc::clone(&counts),
        StageBehavior::Success,
        SealBehavior::Success,
    );
    let mut sealed = std::ptr::null_mut();
    let guard =
        StateImportFfiTestPlanGuard::install(&[StateImportFfiTestPhase::BeforePrimaryEncoding]);

    let pointer = unsafe { state_import_seal(open, &mut sealed) };
    let result = decode_if_present(pointer).expect("the fallback panic result must encode");
    assert_eq!(error(result).code, PROTOBUF_STATE_IMPORT_ERROR_PANIC);
    drop(guard);

    let exposed = !sealed.is_null();
    if exposed {
        let _ = decode_and_free(unsafe { state_import_abort_sealed(sealed) });
    }
    assert!(!exposed, "FFI-02/03/04: seal exposed an unencoded sealed capability");
    assert_eq!(counts.publications.load(Ordering::SeqCst), 0);
}

#[test]
fn seal_operation_panic_consumes_open_and_exposes_no_sealed_capability() {
    let root = digest(22);
    let counts = Arc::new(LifecycleCounts::default());
    let open = open_handle(
        activating_spec(&root),
        Arc::clone(&counts),
        StageBehavior::Success,
        SealBehavior::Panic,
    );
    let mut sealed = 1_usize as *mut SealedStateImportHandle;

    let panic_result = error(decode_and_free(unsafe { state_import_seal(open, &mut sealed) }));
    assert_eq!(panic_result.code, PROTOBUF_STATE_IMPORT_ERROR_PANIC);
    assert!(sealed.is_null(), "FFI-05/06: panicking seal exposed a capability");
    assert_eq!(counts.seals.load(Ordering::SeqCst), 1);
    assert_eq!(counts.publications.load(Ordering::SeqCst), 0);
}

#[test]
fn stage_primary_encoding_failure_poisons_the_mutated_open_session() {
    let root = digest(16);
    let counts = Arc::new(LifecycleCounts::default());
    let open = open_handle(
        activating_spec(&root),
        Arc::clone(&counts),
        StageBehavior::Success,
        SealBehavior::Success,
    );
    let page = protobuf_page(&root);
    let guard =
        StateImportFfiTestPlanGuard::install(&[StateImportFfiTestPhase::BeforePrimaryEncoding]);

    let pointer = unsafe { state_import_stage_page(open, page.as_ptr(), page.len()) };
    let result = decode_if_present(pointer).expect("the fallback panic result must encode");
    assert_eq!(error(result).code, PROTOBUF_STATE_IMPORT_ERROR_PANIC);
    drop(guard);

    let abort = error(decode_and_free(unsafe { state_import_abort_open(open) }));
    assert_eq!(
        abort.code, PROTOBUF_STATE_IMPORT_ERROR_INVALID_HANDLE_STATE,
        "FFI-11: an unacknowledged staged mutation left its session usable",
    );
    assert_eq!(counts.pages.load(Ordering::SeqCst), 1);
    assert_eq!(counts.publications.load(Ordering::SeqCst), 0);
}

#[test]
fn publish_primary_encoding_failure_returns_null_after_authority_effect() {
    let root = digest(17);
    let counts = Arc::new(LifecycleCounts::default());
    let sealed =
        sealed_handle(activating_spec(&root), Arc::clone(&counts), PublishBehavior::Success);
    let _guard =
        StateImportFfiTestPlanGuard::install(&[StateImportFfiTestPhase::BeforePrimaryEncoding]);

    let pointer = unsafe { state_import_publish(sealed) };
    let response = decode_if_present(pointer);
    assert_eq!(counts.authoritative_publications.load(Ordering::SeqCst), 1);
    assert!(
        response.is_none(),
        "FFI-07/08: an authoritative publication with an unencoded receipt must return null",
    );
}

#[test]
fn post_publication_operation_panic_returns_null_not_unpublished_error() {
    let root = digest(18);
    let counts = Arc::new(LifecycleCounts::default());
    let sealed = sealed_handle(
        activating_spec(&root),
        Arc::clone(&counts),
        PublishBehavior::PanicAfterEffect,
    );

    let pointer = unsafe { state_import_publish(sealed) };
    let response = decode_if_present(pointer);
    assert_eq!(counts.authoritative_publications.load(Ordering::SeqCst), 1);
    assert!(
        response.is_none(),
        "FFI-08/09: post-publication panic was misclassified as unpublished failure",
    );
}

#[test]
fn pre_publication_operation_panic_is_structured_and_does_not_invent_publication() {
    let root = digest(19);
    let counts = Arc::new(LifecycleCounts::default());
    let sealed = sealed_handle(
        activating_spec(&root),
        Arc::clone(&counts),
        PublishBehavior::PanicBeforeEffect,
    );

    let result = error(decode_and_free(unsafe { state_import_publish(sealed) }));
    assert_eq!(result.code, PROTOBUF_STATE_IMPORT_ERROR_PANIC);
    assert_eq!(counts.authoritative_publications.load(Ordering::SeqCst), 0);
}

#[test]
fn primary_and_fallback_encoding_failure_returns_null_without_consuming_operation() {
    let root = digest(20);
    let counts = Arc::new(LifecycleCounts::default());
    let open = open_handle(
        activating_spec(&root),
        Arc::clone(&counts),
        StageBehavior::Success,
        SealBehavior::Success,
    );
    let guard = StateImportFfiTestPlanGuard::install(&[
        StateImportFfiTestPhase::BeforeOperation,
        StateImportFfiTestPhase::BeforeFallbackEncoding,
    ]);

    let pointer = unsafe { state_import_abort_open(open) };
    assert!(pointer.is_null(), "FFI-05/06: fallback unwind must return null");
    drop(guard);

    let aborted = success(decode_and_free(unsafe { state_import_abort_open(open) }));
    assert_eq!(aborted.kind, PROTOBUF_STATE_IMPORT_SUCCESS_ABORTED);
    assert_eq!(counts.publications.load(Ordering::SeqCst), 0);
}
