//! Tracing setup with an in-memory ring buffer that the Log tab reads.
//!
//! Two layers:
//!
//! - `tracing_subscriber::fmt` → console output (colour, targets)
//! - `BufferLayer` → pushes each event's formatted line into an `Arc<LogBuffer>`
//!
//! The GUI's `log_tab` reads `LogBuffer::snapshot()` every frame and
//! `LogBuffer::clear()` on a "Clear" button. There is nothing else in this
//! module; logging is otherwise standard `tracing` (`info!`, `warn!`, …).
//!
//! ## Level control
//!
//! `init_logging(debug)` sets the default filter to `INFO` or `DEBUG`.
//! `RUST_LOG=…` overrides both if set, so `RUST_LOG=luracs=debug cargo run`
//! works without changing code.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::prelude::*;

// ---------------------------------------------------------------------------
// LogBuffer
// ---------------------------------------------------------------------------

/// Bounded ring buffer of formatted log lines. Cheap to share across the
/// async runtime and the GUI thread.
pub struct LogBuffer {
    capacity: usize,
    messages: Mutex<VecDeque<String>>,
}

impl LogBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            messages: Mutex::new(VecDeque::with_capacity(capacity)),
        }
    }

    /// Append a line, dropping the oldest if the buffer is full.
    pub fn push(&self, line: String) {
        let mut buf = self.messages.lock().unwrap();
        while buf.len() >= self.capacity {
            buf.pop_front();
        }
        buf.push_back(line);
    }

    /// Snapshot every line currently buffered (oldest first).
    pub fn snapshot(&self) -> Vec<String> {
        self.messages.lock().unwrap().iter().cloned().collect()
    }

    /// Clear the buffer (called from the Log tab's "Clear" button).
    pub fn clear(&self) {
        self.messages.lock().unwrap().clear();
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.messages.lock().unwrap().len()
    }
}

// ---------------------------------------------------------------------------
// BufferLayer
// ---------------------------------------------------------------------------

struct BufferLayer(Arc<LogBuffer>);

struct MsgVisitor {
    text: String,
    fields: Vec<(String, String)>,
}

impl MsgVisitor {
    fn new() -> Self {
        Self {
            text: String::new(),
            fields: Vec::new(),
        }
    }
}

impl Visit for MsgVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            if self.text.is_empty() {
                self.text = value.to_string();
            } else {
                self.text.push(' ');
                self.text.push_str(value);
            }
        } else {
            self.fields
                .push((field.name().to_string(), value.to_string()));
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let rendered = format!("{value:?}");
        if field.name() == "message" {
            // Debug-format of the message is normally the string without
            // quotes, but wrap safely regardless.
            if self.text.is_empty() {
                self.text = rendered;
            } else {
                self.text.push(' ');
                self.text.push_str(&rendered);
            }
        } else {
            self.fields.push((field.name().to_string(), rendered));
        }
    }
}

impl<S: Subscriber> Layer<S> for BufferLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = MsgVisitor::new();
        event.record(&mut visitor);

        let meta = event.metadata();

        let level_str = match *meta.level() {
            Level::ERROR => "ERROR",
            Level::WARN => "WARN ",
            Level::INFO => "INFO ",
            Level::DEBUG => "DEBUG",
            Level::TRACE => "TRACE",
        };

        let mut line = format!("[{}] {}: {}", level_str, meta.target(), visitor.text);

        if !visitor.fields.is_empty() {
            let kv = visitor
                .fields
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(" ");
            line.push_str(" | ");
            line.push_str(&kv);
        }

        self.0.push(line);
    }
}

// ---------------------------------------------------------------------------
// Init
// ---------------------------------------------------------------------------

/// Install the subscriber. Returns the shared buffer for the GUI's Log tab.
///
/// `RUST_LOG` overrides `debug` if set, so either works:
///     RUST_LOG=luracs=debug cargo run
///     cargo run -- --debug
pub fn init_logging(debug: bool) -> Result<Arc<LogBuffer>, Box<dyn std::error::Error>> {
    let buffer = Arc::new(LogBuffer::new(4096));

    let default_level = if debug { Level::DEBUG } else { Level::INFO };

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::from_default_env().add_directive(default_level.into()));

    let console = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_level(true)
        .with_ansi(true);

    tracing_subscriber::registry()
        .with(filter)
        .with(console)
        .with(BufferLayer(Arc::clone(&buffer)))
        .init();

    Ok(buffer)
}
