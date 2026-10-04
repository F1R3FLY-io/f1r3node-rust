use std::collections::{BTreeMap, BTreeSet};

use crate::error::AppError;
use crate::event::{AppCursor, AppEvent, AppEventId, AppId, AppSnapshot};
use crate::receipt::{AppReceipt, AppReceiptStatus};

/// Deterministic application state machine over finalized Cordial events.
pub trait CordialApp {
    fn app_id(&self) -> &AppId;

    fn validate(&self, event: &AppEvent) -> Result<(), AppError>;

    fn apply(&mut self, event: AppEvent) -> Result<AppReceipt, AppError>;

    fn snapshot(&self) -> AppSnapshot;
}

/// In-memory application runtime.
///
/// The runtime applies caller-provided finalized events in order, routes them
/// by application ID, records receipts, and advances a replay cursor for events
/// that reached an application decision.
#[derive(Default)]
pub struct AppRuntime {
    apps: BTreeMap<AppId, Box<dyn CordialApp>>,
    processed_event_ids: BTreeSet<AppEventId>,
    receipts: BTreeMap<AppEventId, AppReceipt>,
    receipt_order: Vec<AppEventId>,
    cursor: Option<AppCursor>,
}

impl AppRuntime {
    pub fn new() -> Self {
        Self {
            apps: BTreeMap::new(),
            processed_event_ids: BTreeSet::new(),
            receipts: BTreeMap::new(),
            receipt_order: Vec::new(),
            cursor: None,
        }
    }

    pub fn register_app(&mut self, app: Box<dyn CordialApp>) -> Option<Box<dyn CordialApp>> {
        self.apps.insert(app.app_id().clone(), app)
    }

    pub fn app_count(&self) -> usize {
        self.apps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.apps.is_empty()
    }

    pub fn process_event(&mut self, event: AppEvent) -> Result<AppReceipt, AppError> {
        if self.processed_event_ids.contains(&event.event_id) {
            return Err(AppError::DuplicateEvent {
                event_id: event.event_id,
            });
        }

        let receipt = {
            let app = self
                .apps
                .get_mut(&event.app_id)
                .ok_or_else(|| AppError::UnknownApp {
                    app_id: event.app_id.clone(),
                })?;

            match app.validate(&event) {
                Ok(()) => {
                    let receipt = app.apply(event.clone())?;
                    AppReceipt {
                        app_id: event.app_id.clone(),
                        event_id: event.event_id.clone(),
                        ordered_index: event.ordered_index,
                        status: AppReceiptStatus::Applied,
                        state_root: receipt.state_root,
                        message: receipt.message,
                    }
                }
                Err(error) => AppReceipt {
                    app_id: event.app_id.clone(),
                    event_id: event.event_id.clone(),
                    ordered_index: event.ordered_index,
                    status: AppReceiptStatus::Rejected,
                    state_root: app.snapshot().state_root,
                    message: Some(validation_rejection_message(error)),
                },
            }
        };

        self.record_processed_event(&event, receipt.clone());
        Ok(receipt)
    }

    pub fn process_events<I>(&mut self, events: I) -> Vec<Result<AppReceipt, AppError>>
    where
        I: IntoIterator<Item = AppEvent>,
    {
        events
            .into_iter()
            .map(|event| self.process_event(event))
            .collect()
    }

    pub fn cursor(&self) -> Option<&AppCursor> {
        self.cursor.as_ref()
    }

    pub fn receipt(&self, event_id: &AppEventId) -> Option<&AppReceipt> {
        self.receipts.get(event_id)
    }

    pub fn receipts(&self) -> impl Iterator<Item = &AppReceipt> {
        let receipts = &self.receipts;
        self.receipt_order
            .iter()
            .filter_map(move |event_id| receipts.get(event_id))
    }

    pub fn snapshot(&self, app_id: &AppId) -> Result<AppSnapshot, AppError> {
        self.apps
            .get(app_id)
            .map(|app| app.snapshot())
            .ok_or_else(|| AppError::UnknownApp {
                app_id: app_id.clone(),
            })
    }

    pub fn snapshots(&self) -> Vec<AppSnapshot> {
        self.apps.values().map(|app| app.snapshot()).collect()
    }

    fn record_processed_event(&mut self, event: &AppEvent, receipt: AppReceipt) {
        self.processed_event_ids.insert(event.event_id.clone());
        self.receipt_order.push(event.event_id.clone());
        self.receipts.insert(event.event_id.clone(), receipt);
        self.cursor = Some(AppCursor {
            finalized_anchor: event.finalized_anchor.clone(),
            ordered_index: event.ordered_index,
        });
    }
}

fn validation_rejection_message(error: AppError) -> String {
    match error {
        AppError::ValidationRejected { message } => message,
        error => error.to_string(),
    }
}
