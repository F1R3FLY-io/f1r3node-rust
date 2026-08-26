use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use models::rspace_plus_plus_types::state_import_result_proto::Outcome;
use rspace_plus_plus::rspace::state::import_session::StateImportActivation;

use super::*;

#[derive(Default)]
struct LifecycleCounts {
    pages: AtomicUsize,
    seals: AtomicUsize,
    publications: AtomicUsize,
}

#[derive(Clone, Copy)]
enum StageBehavior {
    Success,
    Error,
    Panic,
}

struct TestOpenSession {
    spec: StateImportSpec,
    counts: Arc<LifecycleCounts>,
    stage_behavior: StageBehavior,
    seal_error: bool,
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
        if self.seal_error {
            return Err(StateImportError::IncompletePagination(Vec::new()));
        }
        Ok(Box::new(TestSealedSession {
            spec: self.spec.clone(),
            counts: Arc::clone(&self.counts),
            publish_error: false,
        }))
    }
}

struct TestSealedSession {
    spec: StateImportSpec,
    counts: Arc<LifecycleCounts>,
    publish_error: bool,
}

impl SealedStateImport for TestSealedSession {
    fn spec(&self) -> &StateImportSpec { &self.spec }

    fn publish(self: Box<Self>) -> Result<StateImportPublication, StateImportError> {
        self.counts.publications.fetch_add(1, Ordering::SeqCst);
        if self.publish_error {
            return Err(StateImportError::InternalInvariant(
                "test publication rejected".to_string(),
            ));
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

fn open_handle(
    spec: StateImportSpec,
    counts: Arc<LifecycleCounts>,
    stage_behavior: StageBehavior,
    seal_error: bool,
) -> *mut OpenStateImportHandle {
    Box::into_raw(Box::new(OpenStateImportHandle {
        session: Mutex::new(Some(Box::new(TestOpenSession {
            spec,
            counts,
            stage_behavior,
            seal_error,
        }))),
    }))
}

fn sealed_handle(
    spec: StateImportSpec,
    counts: Arc<LifecycleCounts>,
    publish_error: bool,
) -> *mut SealedStateImportHandle {
    Box::into_raw(Box::new(SealedStateImportHandle {
        session: Mutex::new(Some(Box::new(TestSealedSession {
            spec,
            counts,
            publish_error,
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

#[test]
fn open_page_seal_publish_is_affine_and_returns_typed_receipts() {
    let root = digest(9);
    let counts = Arc::new(LifecycleCounts::default());
    let open =
        open_handle(activating_spec(&root), Arc::clone(&counts), StageBehavior::Success, false);
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
    let open =
        open_handle(activating_spec(&root), Arc::clone(&counts), StageBehavior::Error, false);
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
    let decode_open =
        open_handle(activating_spec(&root), Arc::clone(&counts), StageBehavior::Success, false);
    let malformed = [0xff];
    let decode_error = error(decode_and_free(unsafe {
        state_import_stage_page(decode_open, malformed.as_ptr(), malformed.len())
    }));
    assert_eq!(decode_error.code, PROTOBUF_STATE_IMPORT_ERROR_DECODE);
    success(decode_and_free(unsafe { state_import_abort_open(decode_open) }));

    let length_open =
        open_handle(activating_spec(&root), Arc::clone(&counts), StageBehavior::Success, false);
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

    let panic_open =
        open_handle(activating_spec(&root), Arc::clone(&counts), StageBehavior::Panic, false);
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
    let open =
        open_handle(activating_spec(&root), Arc::clone(&counts), StageBehavior::Success, true);
    let mut sealed = 1_usize as *mut SealedStateImportHandle;
    let seal_error = error(decode_and_free(unsafe { state_import_seal(open, &mut sealed) }));
    assert_eq!(seal_error.code, PROTOBUF_STATE_IMPORT_ERROR_IMPORT);
    assert!(sealed.is_null(), "failed seal must leave no publication capability");

    let sealed = sealed_handle(activating_spec(&root), Arc::clone(&counts), true);
    let publish_error = error(decode_and_free(unsafe { state_import_publish(sealed) }));
    assert_eq!(publish_error.code, PROTOBUF_STATE_IMPORT_ERROR_IMPORT);
    assert_eq!(counts.seals.load(Ordering::SeqCst), 1);
    assert_eq!(counts.publications.load(Ordering::SeqCst), 1);
}

#[test]
fn abort_sealed_consumes_capability_without_publishing() {
    let root = digest(13);
    let counts = Arc::new(LifecycleCounts::default());
    let sealed = sealed_handle(activating_spec(&root), Arc::clone(&counts), false);
    let aborted = success(decode_and_free(unsafe { state_import_abort_sealed(sealed) }));
    assert_eq!(aborted.kind, PROTOBUF_STATE_IMPORT_SUCCESS_ABORTED);
    assert_eq!(counts.publications.load(Ordering::SeqCst), 0);
}
