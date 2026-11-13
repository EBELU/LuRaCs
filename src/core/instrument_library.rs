//! Library of instruments (Unique or Generic), keyed by an opaque string.
//!
//! Direct port of `luracs.clients.instrument_library.InstrumentLibrary`.
//! Qt signals are replaced by a `VecDeque<InstrumentLibraryEvent>` the GUI
//! drains each frame (same pattern as `Settings`).
//!
//! Behavioural notes:
//! - `add_instrument` and `rename_instrument` emit events but do **not**
//!   mutate the registry. This mirrors the Python original, where those
//!   methods only fire `sigNewInstrumentAdded` / `sigInstrumentUpdated`
//!   and the caller's handler is expected to insert into
//!   `instrument_registry`. In particular, `rename_instrument` removes the
//!   old key but never re-inserts.
//! - `update_instrument_data` validates all field names before applying any
//!   change (atomic). The Python version applies fields incrementally, so a
//!   bad field halfway through leaves earlier updates applied — a silent
//!   partial-update bug we choose not to replicate.
//! - `get_key_from_attr` matches Python exactly: attribute lookup via
//!   `serde_json::to_value` and equality against a `serde_json::Value`.

use std::collections::{HashMap, VecDeque};

use serde::Serialize;
use serde_json::{Map, Value};
use thiserror::Error;

use crate::containers::{GenericInstrument, UniqueInstrument};
use crate::utils::{InstrumentData, XmlParser};

// ---------------------------------------------------------------------------
// Kind / Instrument
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryKind {
    Unique,
    Generic,
}

impl std::fmt::Display for LibraryKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            LibraryKind::Unique => "UniqueInstrument",
            LibraryKind::Generic => "GenericInstrument",
        })
    }
}

/// One stored instrument. Mirrors the Python `UniqueInstrument | GenericInstrument`.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum Instrument {
    Unique(UniqueInstrument),
    Generic(GenericInstrument),
}

impl Instrument {
    /// The field used as a "display name": `name` for Unique, `model` for Generic.
    pub fn name_or_model(&self) -> &str {
        match self {
            Instrument::Unique(u) => &u.name,
            Instrument::Generic(g) => &g.model,
        }
    }

    pub fn kind(&self) -> LibraryKind {
        match self {
            Instrument::Unique(_) => LibraryKind::Unique,
            Instrument::Generic(_) => LibraryKind::Generic,
        }
    }

    pub fn as_unique(&self) -> Option<&UniqueInstrument> {
        match self {
            Instrument::Unique(u) => Some(u),
            _ => None,
        }
    }

    pub fn as_generic(&self) -> Option<&GenericInstrument> {
        match self {
            Instrument::Generic(g) => Some(g),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Events (replace Qt signals)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum InstrumentLibraryEvent {
    /// In-place update. `changes` mirrors the Python `gui_logger.debug(...)`
    /// payload: `["field=value", …]`.
    InstrumentUpdated {
        key: String,
        instrument: Instrument,
        changes: Vec<String>,
    },
    /// Emitted before removal, carrying the removed instrument.
    InstrumentRemoved { instrument: Instrument },
    /// Emitted after removal, carrying the key.
    RemoveInstrument { key: String },
    /// Emitted by `add_instrument`. The listener is expected to insert.
    NewInstrumentAdded { instrument: Instrument },
    /// Declared by the Python `sigRegistryUpdated`. Never emitted there;
    /// kept here for parity so future code can start emitting it.
    RegistryUpdated,
    /// Emitted by `rename_instrument`. The listener is expected to insert
    /// `new_instrument` under the new key.
    InstrumentRenamed {
        old_key: String,
        new_instrument: Instrument,
    },
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum InstrumentLibraryError {
    #[error("parser has no instrument attached")]
    NoInstrument,

    #[error("expected a {expected} instance, got a {got}")]
    WrongKind {
        expected: LibraryKind,
        got: LibraryKind,
    },

    #[error("instrument key not found: {0:?}")]
    NotFound(String),

    #[error("unknown field {field:?} on instrument")]
    UnknownField { field: String },

    #[error("instrument JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type InstrumentLibraryResult<T> = std::result::Result<T, InstrumentLibraryError>;

// ---------------------------------------------------------------------------
// Library
// ---------------------------------------------------------------------------

pub struct InstrumentLibrary {
    registry: HashMap<String, Instrument>,
    kind: LibraryKind,
    events: VecDeque<InstrumentLibraryEvent>,
}

impl InstrumentLibrary {
    pub fn new(kind: LibraryKind) -> Self {
        Self {
            registry: HashMap::new(),
            kind,
            events: VecDeque::new(),
        }
    }

    /// Insert directly without emitting. Useful for event handlers that react to
    /// `NewInstrumentAdded` / `InstrumentRenamed` and must complete the insert
    /// the notification methods only announced.
    pub fn set_raw(&mut self, key: impl Into<String>, instrument: Instrument) -> InstrumentLibraryResult<()> {
        self.check_kind(&instrument)?;
        self.registry.insert(key.into(), instrument);
        Ok(())
    }

    // ---- Accessors ------------------------------------------------------

    pub fn kind(&self) -> LibraryKind {
        self.kind
    }

    pub fn len(&self) -> usize {
        self.registry.len()
    }

    pub fn is_empty(&self) -> bool {
        self.registry.is_empty()
    }

    pub fn contains(&self, key: &str) -> bool {
        self.registry.contains_key(key)
    }

    pub fn get(&self, key: &str) -> Option<&Instrument> {
        self.registry.get(key)
    }

    /// Iteration in registry order (unordered; matches Python dict iteration
    /// which is insertion-ordered only in 3.7+ — we don't replicate that).
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Instrument)> {
        self.registry.iter()
    }

    // ---- Event queue ----------------------------------------------------

    pub fn drain_events(&mut self) -> Vec<InstrumentLibraryEvent> {
        self.events.drain(..).collect()
    }

    fn emit(&mut self, ev: InstrumentLibraryEvent) {
        self.events.push_back(ev);
    }

    fn check_kind(&self, inst: &Instrument) -> InstrumentLibraryResult<()> {
        if inst.kind() != self.kind {
            return Err(InstrumentLibraryError::WrongKind {
                expected: self.kind,
                got: inst.kind(),
            });
        }
        Ok(())
    }

    // ---- Mutating / notification methods --------------------------------

    /// Insert an instrument parsed from an N42 XML file. Replaces Python's
    /// `assert isinstance(instrument, self._instrument_class)` with a
    /// returned error.
    pub fn set_instrument_from_parser(
        &mut self,
        key: &str,
        parser: &XmlParser,
    ) -> InstrumentLibraryResult<()> {
        let data = parser
            .get_instrument()
            .ok_or(InstrumentLibraryError::NoInstrument)?;

        let instrument = match (self.kind, data) {
            (LibraryKind::Unique, InstrumentData::Unique(u)) => Instrument::Unique(u.clone()),
            (LibraryKind::Generic, InstrumentData::Generic(g)) => Instrument::Generic(g.clone()),
            (expected, got) => {
                let got = match got {
                    InstrumentData::Unique(_) => LibraryKind::Unique,
                    InstrumentData::Generic(_) => LibraryKind::Generic,
                };
                return Err(InstrumentLibraryError::WrongKind { expected, got });
            }
        };

        self.registry.insert(key.to_string(), instrument);
        Ok(())
    }

    /// Remove a key. Emits `InstrumentRemoved` then `RemoveInstrument`.
    /// Returns the removed instrument, or `None` if the key was unknown.
    pub fn remove_instrument(&mut self, key: &str) -> Option<Instrument> {
        let removed = self.registry.remove(key)?;
        self.emit(InstrumentLibraryEvent::InstrumentRemoved {
            instrument: removed.clone(),
        });
        self.emit(InstrumentLibraryEvent::RemoveInstrument {
            key: key.to_string(),
        });
        Some(removed)
    }

    /// Notification-only: emit `NewInstrumentAdded` without inserting.
    /// The caller's event handler is expected to do the insert. Matches
    /// the Python behaviour exactly.
    pub fn add_instrument(&mut self, new_instrument: Instrument) -> InstrumentLibraryResult<()> {
        self.check_kind(&new_instrument)?;
        self.emit(InstrumentLibraryEvent::NewInstrumentAdded {
            instrument: new_instrument,
        });
        Ok(())
    }

    /// Rename: removes the old key and emits `InstrumentRenamed`. The
    /// caller's handler is expected to re-insert `new_instrument` under the
    /// new key. Matches the Python behaviour (the docstring says "removes
    /// and re-adds" but the implementation only removes + signals).
    pub fn rename_instrument(
        &mut self,
        old_key: &str,
        new_instrument: Instrument,
    ) -> InstrumentLibraryResult<()> {
        self.check_kind(&new_instrument)?;
        self.remove_instrument(old_key);
        self.emit(InstrumentLibraryEvent::InstrumentRenamed {
            old_key: old_key.to_string(),
            new_instrument,
        });
        Ok(())
    }

    // ---- Getters --------------------------------------------------------

    /// Look up by `name` (Unique) or `model` (Generic).
    pub fn get_instrument_by_name(&self, name: &str) -> Option<&Instrument> {
        self.registry
            .values()
            .find(|inst| inst.name_or_model() == name)
    }

    /// Names in registry order. `name` for Unique, `model` for Generic.
    pub fn get_instrument_names(&self) -> Vec<String> {
        self.registry
            .values()
            .map(|inst| inst.name_or_model().to_string())
            .collect()
    }

    /// Match by attribute name via serde JSON. Mirrors Python's
    /// `getattr(item, attr) == value` — including its behaviour of matching
    /// **any** serialised field (not just `name` / `model`). Slower than a
    /// direct field check but generic.
    pub fn get_key_from_attr(&self, attr: &str, value: &Value) -> Option<&String> {
        for (key, inst) in &self.registry {
            let Ok(serialized) = serde_json::to_value(inst) else {
                continue;
            };
            if serialized.get(attr) == Some(value) {
                return Some(key);
            }
        }
        None
    }

    /// Ergonomic Rust alternative to `get_key_from_attr` for the common
    /// "match by name or model" case:
    pub fn get_key_by_name(&self, name: &str) -> Option<&String> {
        self.registry
            .iter()
            .find(|(_, inst)| inst.name_or_model() == name)
            .map(|(k, _)| k)
    }

    // ---- Updates --------------------------------------------------------

    /// Update an instrument in place by JSON round-trip.
    ///
    /// All keys are validated *before* any change is applied (atomic),
    /// unlike the Python which applies fields incrementally and can leave
    /// partial updates on error.
    pub fn update_instrument_data(
        &mut self,
        key: &str,
        updates: Map<String, Value>,
    ) -> InstrumentLibraryResult<()> {
        // Build the change list before consuming `updates`
        let changes: Vec<String> = updates
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();

        // Apply atomically, then clone out for the event
        let instrument_clone: Instrument = {
            let instrument = self
                .registry
                .get_mut(key)
                .ok_or_else(|| InstrumentLibraryError::NotFound(key.to_string()))?;

            match instrument {
                Instrument::Unique(u) => update_via_json(u, updates)?,
                Instrument::Generic(g) => update_via_json(g, updates)?,
            }

            instrument.clone()
        };

        // Optional logging — drop in `tracing::debug!` if you use tracing:
        //   tracing::debug!(
        //       name = %instrument_clone.name_or_model(),
        //       changes = %changes.join(", "),
        //       "Instrument updated"
        //   );

        self.emit(InstrumentLibraryEvent::InstrumentUpdated {
            key: key.to_string(),
            instrument: instrument_clone,
            changes,
        });
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Generic JSON round-trip updater
// ---------------------------------------------------------------------------

fn update_via_json<T>(target: &mut T, updates: Map<String, Value>) -> InstrumentLibraryResult<()>
where
    T: Serialize + serde::de::DeserializeOwned,
{
    let mut val = serde_json::to_value(&*target)?;
    let obj = val.as_object_mut().ok_or_else(|| {
        InstrumentLibraryError::Json(serde_json::Error::io(std::io::Error::other(
            "serialized instrument is not a JSON object",
        )))
    })?;

    // Validate every key up front (atomic)
    for k in updates.keys() {
        if !obj.contains_key(k) {
            return Err(InstrumentLibraryError::UnknownField { field: k.clone() });
        }
    }

    // Apply
    for (k, v) in updates {
        obj.insert(k, v);
    }

    *target = serde_json::from_value(val)?;
    Ok(())
}
