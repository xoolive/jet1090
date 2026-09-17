use std::collections::{BTreeMap, VecDeque};
use std::fmt::Write;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Local};
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer};

const MAX_EVENTS: usize = 100;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceStatus {
    Connecting,
    Healthy,
    Reconnecting,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Severity {
    Warning,
    Error,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Warning => "WARN",
            Self::Error => "ERROR",
        }
    }
}

#[derive(Clone, Debug)]
pub struct HealthEvent {
    pub timestamp: DateTime<Local>,
    pub severity: Severity,
    pub source: String,
    pub message: String,
    pub occurrences: u32,
}

#[derive(Debug, Default)]
pub struct HealthState {
    pub sources: BTreeMap<String, SourceStatus>,
    pub events: VecDeque<HealthEvent>,
    pub unread_errors: usize,
}

pub type SharedHealth = Arc<Mutex<HealthState>>;

pub fn new_health() -> SharedHealth {
    Arc::new(Mutex::new(HealthState::default()))
}

pub fn register_source(health: &SharedHealth, source: String) {
    let mut health = health.lock().expect("health state lock poisoned");
    health
        .sources
        .entry(source)
        .or_insert(SourceStatus::Connecting);
}

pub fn set_source_status(
    health: &SharedHealth,
    source: &str,
    status: SourceStatus,
) {
    let mut health = health.lock().expect("health state lock poisoned");
    health.sources.insert(source.to_string(), status);
}

pub fn mark_events_read(health: &SharedHealth) {
    health
        .lock()
        .expect("health state lock poisoned")
        .unread_errors = 0;
}

/// Print the retained operational events after the alternate screen is restored.
/// This preserves diagnostics when a graceful TUI shutdown prevents opening the
/// in-app error overlay.
pub fn print_report(health: &SharedHealth) {
    let health = health.lock().expect("health state lock poisoned");
    if health.events.is_empty() {
        return;
    }

    eprintln!("jet1090 recent warnings and errors:");
    for event in &health.events {
        let repeats = if event.occurrences > 1 {
            format!(" (×{})", event.occurrences)
        } else {
            String::new()
        };
        eprintln!(
            "{} {} {}: {}{}",
            event.timestamp.format("%H:%M:%S"),
            event.severity.label(),
            event.source,
            event.message,
            repeats,
        );
    }
}

fn record_event(
    health: &SharedHealth,
    severity: Severity,
    source: String,
    message: String,
) {
    let mut health = health.lock().expect("health state lock poisoned");
    if let Some(status) = health.sources.get_mut(&source) {
        *status = match severity {
            Severity::Warning => SourceStatus::Reconnecting,
            Severity::Error => SourceStatus::Failed,
        };
    }
    if let Some(last) = health.events.back_mut() {
        if last.severity == severity
            && last.source == source
            && last.message == message
        {
            last.timestamp = Local::now();
            last.occurrences += 1;
            if severity == Severity::Error {
                health.unread_errors += 1;
            }
            return;
        }
    }

    if health.events.len() == MAX_EVENTS {
        health.events.pop_front();
    }
    health.events.push_back(HealthEvent {
        timestamp: Local::now(),
        severity,
        source,
        message,
        occurrences: 1,
    });
    if severity == Severity::Error {
        health.unread_errors += 1;
    }
}

pub struct UiEventLayer {
    health: SharedHealth,
}

impl UiEventLayer {
    pub fn new(health: SharedHealth) -> Self {
        Self { health }
    }
}

impl<S> Layer<S> for UiEventLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        let severity = match *event.metadata().level() {
            Level::ERROR => Severity::Error,
            Level::WARN => Severity::Warning,
            _ => return,
        };
        let mut visitor = EventVisitor::default();
        event.record(&mut visitor);
        let source = visitor
            .source
            .unwrap_or_else(|| event.metadata().target().to_string());
        let mut message = if visitor.message.is_empty() {
            event.metadata().name().to_string()
        } else {
            visitor.message
        };
        if let Some(error) = visitor.error {
            message.push_str(&format!(": {error}"));
        }
        record_event(&self.health, severity, source, message);
    }
}

#[derive(Default)]
struct EventVisitor {
    source: Option<String>,
    message: String,
    error: Option<String>,
}

impl Visit for EventVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "source" {
            self.source =
                Some(format!("{value:?}").trim_matches('"').to_string());
        } else if field.name() == "message" {
            let _ = write!(self.message, "{value:?}");
            self.message = self.message.trim_matches('"').to_string();
        } else if field.name() == "error" {
            self.error =
                Some(format!("{value:?}").trim_matches('"').to_string());
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "source" {
            self.source = Some(value.to_string());
        } else if field.name() == "message" {
            self.message = value.to_string();
        } else if field.name() == "error" {
            self.error = Some(value.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retains_a_bounded_event_history() {
        let health = new_health();
        for index in 0..=MAX_EVENTS {
            record_event(
                &health,
                Severity::Warning,
                "test".to_string(),
                index.to_string(),
            );
        }
        let health = health.lock().unwrap();
        assert_eq!(health.events.len(), MAX_EVENTS);
        assert_eq!(health.events.front().unwrap().message, "1");
    }

    #[test]
    fn coalesces_repeated_errors_and_marks_source_failed() {
        let health = new_health();
        register_source(&health, "Delft".to_string());
        record_event(
            &health,
            Severity::Error,
            "Delft".to_string(),
            "connection lost".to_string(),
        );
        record_event(
            &health,
            Severity::Error,
            "Delft".to_string(),
            "connection lost".to_string(),
        );
        let health = health.lock().unwrap();
        assert_eq!(health.events.len(), 1);
        assert_eq!(health.events[0].occurrences, 2);
        assert_eq!(health.unread_errors, 2);
        assert_eq!(health.sources["Delft"], SourceStatus::Failed);
    }
}
