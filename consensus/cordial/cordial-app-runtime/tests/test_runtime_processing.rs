use std::collections::BTreeSet;

use cordial_app_runtime::{
    AppCursor, AppError, AppEvent, AppEventId, AppId, AppReceipt, AppReceiptStatus, AppRuntime,
    AppSnapshot, CordialApp,
};

struct RecordingApp {
    app_id: AppId,
    applied_payloads: Vec<Vec<u8>>,
    cursor: Option<AppCursor>,
    reject_event_types: BTreeSet<String>,
    fail_apply_event_types: BTreeSet<String>,
}

impl RecordingApp {
    fn new(app_id: &str) -> Self {
        Self {
            app_id: AppId(app_id.to_owned()),
            applied_payloads: Vec::new(),
            cursor: None,
            reject_event_types: BTreeSet::new(),
            fail_apply_event_types: BTreeSet::new(),
        }
    }

    fn reject_event_type(mut self, event_type: &str) -> Self {
        self.reject_event_types.insert(event_type.to_owned());
        self
    }

    fn fail_apply_event_type(mut self, event_type: &str) -> Self {
        self.fail_apply_event_types.insert(event_type.to_owned());
        self
    }

    fn state_root(&self) -> Option<Vec<u8>> {
        Some(vec![self.applied_payloads.len() as u8])
    }

    fn snapshot_payload(&self) -> Vec<u8> {
        let mut payload = Vec::new();
        for applied in &self.applied_payloads {
            payload.extend_from_slice(applied);
            payload.push(b'\n');
        }
        payload
    }
}

impl CordialApp for RecordingApp {
    fn app_id(&self) -> &AppId {
        &self.app_id
    }

    fn validate(&self, event: &AppEvent) -> Result<(), AppError> {
        if self.reject_event_types.contains(&event.event_type) {
            return Err(AppError::ValidationRejected {
                message: format!("{} rejected {}", self.app_id.0, event.event_type),
            });
        }

        Ok(())
    }

    fn apply(&mut self, event: AppEvent) -> Result<AppReceipt, AppError> {
        if self.fail_apply_event_types.contains(&event.event_type) {
            return Err(AppError::ApplyFailed {
                message: format!("{} failed {}", self.app_id.0, event.event_type),
            });
        }

        self.cursor = Some(AppCursor {
            finalized_anchor: event.finalized_anchor.clone(),
            ordered_index: event.ordered_index,
        });
        self.applied_payloads.push(event.payload);

        Ok(AppReceipt {
            app_id: event.app_id,
            event_id: event.event_id,
            ordered_index: event.ordered_index,
            status: AppReceiptStatus::Applied,
            state_root: self.state_root(),
            message: Some("applied".to_owned()),
        })
    }

    fn snapshot(&self) -> AppSnapshot {
        AppSnapshot {
            app_id: self.app_id.clone(),
            cursor: self.cursor.clone(),
            state_root: self.state_root(),
            payload: self.snapshot_payload(),
        }
    }
}

#[test]
fn registers_one_app_and_multiple_apps() {
    let mut runtime = AppRuntime::new();

    assert!(runtime.is_empty());
    assert_eq!(runtime.app_count(), 0);

    assert!(
        runtime
            .register_app(Box::new(RecordingApp::new("alpha")))
            .is_none()
    );
    assert!(
        runtime
            .register_app(Box::new(RecordingApp::new("beta")))
            .is_none()
    );

    assert!(!runtime.is_empty());
    assert_eq!(runtime.app_count(), 2);
    assert_eq!(
        runtime
            .snapshots()
            .into_iter()
            .map(|snapshot| snapshot.app_id)
            .collect::<Vec<_>>(),
        vec![app_id("alpha"), app_id("beta")]
    );
}

#[test]
fn unknown_app_id_is_reported_without_advancing_runtime_state() {
    let mut runtime = AppRuntime::new();
    let event = event("missing", "event-1", "create", 0, b"payload");

    let err = runtime.process_event(event.clone()).unwrap_err();

    assert_eq!(
        err,
        AppError::UnknownApp {
            app_id: event.app_id
        }
    );
    assert!(runtime.cursor().is_none());
    assert!(runtime.receipts().next().is_none());
}

#[test]
fn duplicate_event_id_is_rejected_deterministically() {
    let mut runtime = runtime_with_app(RecordingApp::new("alpha"));
    let first = event("alpha", "event-1", "create", 0, b"first");
    let duplicate = event("alpha", "event-1", "create", 1, b"second");

    let first_receipt = runtime.process_event(first.clone()).unwrap();
    let err = runtime.process_event(duplicate).unwrap_err();

    assert_eq!(
        err,
        AppError::DuplicateEvent {
            event_id: AppEventId("event-1".to_owned())
        }
    );
    assert_eq!(runtime.cursor(), Some(&cursor_from(&first)));
    assert_eq!(
        runtime.receipts().cloned().collect::<Vec<_>>(),
        vec![first_receipt]
    );
}

#[test]
fn valid_event_produces_applied_receipt_and_advances_cursor() {
    let mut runtime = runtime_with_app(RecordingApp::new("alpha"));
    let event = event("alpha", "event-1", "create", 4, b"payload");

    let receipt = runtime.process_event(event.clone()).unwrap();

    assert_eq!(receipt.status, AppReceiptStatus::Applied);
    assert_eq!(receipt.app_id, app_id("alpha"));
    assert_eq!(receipt.event_id, event_id("event-1"));
    assert_eq!(receipt.ordered_index, 4);
    assert_eq!(receipt.state_root, Some(vec![1]));
    assert_eq!(runtime.cursor(), Some(&cursor_from(&event)));
    assert_eq!(runtime.receipt(&event.event_id), Some(&receipt));
    assert_eq!(
        runtime.snapshot(&app_id("alpha")).unwrap().payload,
        b"payload\n".to_vec()
    );
}

#[test]
fn validation_failure_produces_rejected_receipt_without_applying_payload() {
    let mut runtime = runtime_with_app(RecordingApp::new("alpha").reject_event_type("reject"));
    let event = event("alpha", "event-1", "reject", 2, b"bad-payload");

    let receipt = runtime.process_event(event.clone()).unwrap();

    assert_eq!(receipt.status, AppReceiptStatus::Rejected);
    assert_eq!(receipt.state_root, Some(vec![0]));
    assert_eq!(receipt.message, Some("alpha rejected reject".to_owned()));
    assert_eq!(runtime.cursor(), Some(&cursor_from(&event)));
    assert_eq!(runtime.receipt(&event.event_id), Some(&receipt));
    assert_eq!(
        runtime.snapshot(&app_id("alpha")).unwrap().payload,
        Vec::<u8>::new()
    );
}

#[test]
fn app_apply_failure_is_surfaced_without_recording_receipt() {
    let mut runtime = runtime_with_app(RecordingApp::new("alpha").fail_apply_event_type("fail"));
    let event = event("alpha", "event-1", "fail", 0, b"payload");

    let err = runtime.process_event(event.clone()).unwrap_err();

    assert_eq!(
        err,
        AppError::ApplyFailed {
            message: "alpha failed fail".to_owned()
        }
    );
    assert!(runtime.cursor().is_none());
    assert!(runtime.receipt(&event.event_id).is_none());
}

#[test]
fn process_events_preserves_caller_order_and_cursor_follows_last_processed_event() {
    let mut runtime = runtime_with_app(RecordingApp::new("alpha"));
    let first = event("alpha", "event-1", "create", 7, b"first");
    let second = event("alpha", "event-2", "create", 3, b"second");

    let results = runtime.process_events(vec![first.clone(), second.clone()]);

    assert_eq!(results.len(), 2);
    assert!(results.iter().all(Result::is_ok));
    assert_eq!(
        runtime
            .receipts()
            .map(|receipt| receipt.ordered_index)
            .collect::<Vec<_>>(),
        vec![7, 3]
    );
    assert_eq!(runtime.cursor(), Some(&cursor_from(&second)));
}

#[test]
fn replaying_same_ordered_prefix_produces_identical_receipts_and_snapshots() {
    let events = vec![
        event("alpha", "event-1", "create", 0, b"one"),
        event("alpha", "event-2", "reject", 1, b"two"),
        event("beta", "event-3", "create", 2, b"three"),
    ];

    let mut first_runtime = runtime_with_apps(vec![
        RecordingApp::new("alpha").reject_event_type("reject"),
        RecordingApp::new("beta"),
    ]);
    let mut second_runtime = runtime_with_apps(vec![
        RecordingApp::new("alpha").reject_event_type("reject"),
        RecordingApp::new("beta"),
    ]);

    let first_results = first_runtime.process_events(events.clone());
    let second_results = second_runtime.process_events(events);

    assert_eq!(first_results, second_results);
    assert_eq!(
        first_runtime.receipts().cloned().collect::<Vec<_>>(),
        second_runtime.receipts().cloned().collect::<Vec<_>>()
    );
    assert_eq!(first_runtime.cursor(), second_runtime.cursor());
    assert_eq!(first_runtime.snapshots(), second_runtime.snapshots());
}

#[test]
fn failure_in_one_app_does_not_block_unrelated_app() {
    let mut runtime = runtime_with_apps(vec![
        RecordingApp::new("alpha").fail_apply_event_type("fail"),
        RecordingApp::new("beta"),
    ]);
    let alpha_failure = event("alpha", "event-1", "fail", 0, b"alpha");
    let beta_success = event("beta", "event-2", "create", 1, b"beta");

    let results = runtime.process_events(vec![alpha_failure.clone(), beta_success.clone()]);

    assert_eq!(
        results[0],
        Err(AppError::ApplyFailed {
            message: "alpha failed fail".to_owned()
        })
    );
    assert_eq!(
        results[1].as_ref().unwrap().status,
        AppReceiptStatus::Applied
    );
    assert!(runtime.receipt(&alpha_failure.event_id).is_none());
    assert!(runtime.receipt(&beta_success.event_id).is_some());
    assert_eq!(runtime.cursor(), Some(&cursor_from(&beta_success)));
    assert_eq!(
        runtime.snapshot(&app_id("beta")).unwrap().payload,
        b"beta\n".to_vec()
    );
}

fn runtime_with_app(app: RecordingApp) -> AppRuntime {
    runtime_with_apps(vec![app])
}

fn runtime_with_apps(apps: Vec<RecordingApp>) -> AppRuntime {
    let mut runtime = AppRuntime::new();
    for app in apps {
        runtime.register_app(Box::new(app));
    }
    runtime
}

fn event(
    app_id: &str,
    event_id: &str,
    event_type: &str,
    ordered_index: u64,
    payload: &[u8],
) -> AppEvent {
    AppEvent {
        event_id: AppEventId(event_id.to_owned()),
        app_id: AppId(app_id.to_owned()),
        event_type: event_type.to_owned(),
        payload: payload.to_vec(),
        submitter: vec![ordered_index as u8, 1],
        ordered_index,
        block_hash: vec![ordered_index as u8, 2],
        deploy_signature: Some(vec![ordered_index as u8, 3]),
        finalized_anchor: vec![ordered_index as u8, 4],
    }
}

fn app_id(value: &str) -> AppId {
    AppId(value.to_owned())
}

fn event_id(value: &str) -> AppEventId {
    AppEventId(value.to_owned())
}

fn cursor_from(event: &AppEvent) -> AppCursor {
    AppCursor {
        finalized_anchor: event.finalized_anchor.clone(),
        ordered_index: event.ordered_index,
    }
}
