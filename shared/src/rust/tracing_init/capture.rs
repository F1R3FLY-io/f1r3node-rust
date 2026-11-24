use std::fmt;
use std::sync::{Arc, Mutex};

use tracing::field::{Field, Visit};
use tracing::subscriber::DefaultGuard;
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::Layer;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedEvent {
    pub level: Level,
    pub target: String,
    pub message: String,
}

#[derive(Clone, Default)]
pub struct EventCapture {
    events: Arc<Mutex<Vec<CapturedEvent>>>,
}

impl EventCapture {
    pub fn new() -> Self { Self::default() }

    pub fn install(&self) -> DefaultGuard {
        tracing::subscriber::set_default(tracing_subscriber::registry().with(self.clone()))
    }

    pub fn events(&self) -> Vec<CapturedEvent> {
        self.events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn with_message(&self, fragment: &str) -> Vec<CapturedEvent> {
        self.events()
            .into_iter()
            .filter(|event| event.message.contains(fragment))
            .collect()
    }

    pub fn at_or_above(&self, level: Level) -> Vec<CapturedEvent> {
        self.events()
            .into_iter()
            .filter(|event| event.level <= level)
            .collect()
    }
}

impl<S: Subscriber> Layer<S> for EventCapture {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut message = MessageVisitor::default();
        event.record(&mut message);
        self.events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(CapturedEvent {
                level: *event.metadata().level(),
                target: event.metadata().target().to_string(),
                message: message.0,
            });
    }
}

#[derive(Default)]
struct MessageVisitor(String);

impl Visit for MessageVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.0 = value.to_string();
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_level_target_and_formatted_message_of_each_event() {
        let capture = EventCapture::new();
        {
            let _guard = capture.install();
            tracing::warn!(target: "capture_test", peer = "p1", "first {}", 1);
            tracing::debug!("second");
        }
        tracing::error!("after the guard is dropped");

        assert_eq!(capture.events(), vec![
            CapturedEvent {
                level: Level::WARN,
                target: "capture_test".to_string(),
                message: "first 1".to_string(),
            },
            CapturedEvent {
                level: Level::DEBUG,
                target: module_path!().to_string(),
                message: "second".to_string(),
            },
        ]);
        assert_eq!(capture.at_or_above(Level::WARN).len(), 1);
        assert_eq!(capture.with_message("second").len(), 1);
    }
}
