//! Adapters turn one client's hook records into tree events. Each keeps the
//! per-session state that mapping needs, such as which tool call an approval
//! belongs to.

pub mod claude;
pub mod codex;

use crate::record::HookRecord;
use crate::tree::{Attrs, Client, Event, Millis, Outcome};
use std::collections::{HashSet, VecDeque};

pub trait Adapter: Send {
    fn client(&self) -> Client;
    fn handle(&mut self, record: &HookRecord, at: Millis) -> Vec<Event>;
    /// Drops all state for a session that ended outside the adapter's view,
    /// for example because its process exited.
    fn forget_session(&mut self, session_id: &str);
}

/// Parallel tool calls run their hooks at the same moment, so a sibling's
/// `PreToolUse` can arrive just after another call's `PermissionRequest`. A
/// new tool call answers an approval only once the approval has been open
/// this long, which is less than anyone takes to respond to a prompt.
pub(crate) const SIBLING_WINDOW: Millis = 1_000;

pub fn session_path(client: &str, session: &str) -> String {
    format!("/{client}/session:{session}")
}

pub fn turn_path(session_path: &str, turn: &str) -> String {
    format!("{session_path}/turn:{turn}")
}

pub fn agent_path(turn_path: &str, agent: &str) -> String {
    format!("{turn_path}/agent:{agent}")
}

pub fn request_path(actor_path: &str, request: &str) -> String {
    format!("{actor_path}/request:{request}")
}

pub(crate) fn open(id: String, attrs: Attrs) -> Event {
    Event::Open { id, attrs }
}

pub(crate) fn close(id: String, outcome: Outcome) -> Event {
    Event::Close { id, outcome }
}

/// Opens (or refreshes) a session with the attributes a record carries.
pub(crate) fn open_session(path: &str, record: &HookRecord) -> Event {
    let mut attrs = Attrs::new();
    if let Some(cwd) = &record.cwd {
        attrs.insert("cwd".into(), cwd.clone().into());
    }
    if let Some(pid) = record.pid {
        attrs.insert("pid".into(), pid.into());
    }
    if let Some(surface) = &record.surface {
        attrs.insert("surface".into(), surface.clone().into());
    }
    open(path.to_string(), attrs)
}

pub(crate) fn with_reason(reason: &str) -> Attrs {
    let mut attrs = Attrs::new();
    attrs.insert("reason".into(), reason.into());
    attrs
}

/// One session's turns: which is open, and which have closed. Hooks run
/// concurrently, so a record can arrive after its turn closed; such a turn is
/// never reopened.
#[derive(Default)]
pub(crate) struct Turns {
    open: Option<String>,
    closed: HashSet<String>,
    numbered: u64,
}

impl Turns {
    /// The turn a record belongs to: its own turn id, else the open turn.
    pub(crate) fn of(&self, record: &HookRecord) -> Option<String> {
        record.turn_id.clone().or_else(|| self.open.clone())
    }

    pub(crate) fn is_closed(&self, turn: &str) -> bool {
        self.closed.contains(turn)
    }

    /// Starts the record's turn, numbering it when the record carries no
    /// turn id. Returns the new turn and the previously open turn, if any,
    /// or `None` when the record's turn already closed.
    pub(crate) fn begin(&mut self, record: &HookRecord) -> Option<(String, Option<String>)> {
        let turn = record.turn_id.clone().unwrap_or_else(|| {
            self.numbered += 1;
            format!("n{}", self.numbered)
        });
        if self.closed.contains(&turn) {
            return None;
        }
        let previous = self.open.replace(turn.clone()).filter(|open| *open != turn);
        Some((turn, previous))
    }

    /// Makes a turn seen for the first time the open turn, unless it closed
    /// or another turn is open.
    pub(crate) fn adopt(&mut self, turn: &str) {
        if self.open.is_none() && !self.closed.contains(turn) {
            self.open = Some(turn.to_string());
        }
    }

    /// Marks a turn closed (the open turn when `turn` is `None`). Returns it
    /// if this call closed it.
    pub(crate) fn finish(&mut self, turn: Option<String>) -> Option<String> {
        let turn = turn.or_else(|| self.open.clone())?;
        if !self.closed.insert(turn.clone()) {
            return None;
        }
        if self.open.as_deref() == Some(turn.as_str()) {
            self.open = None;
        }
        Some(turn)
    }
}

/// Sessions that have ended. Hooks run concurrently, so records can arrive
/// after the `SessionEnd` they preceded; those are ignored. Bounded so a
/// long-running service doesn't grow without limit.
#[derive(Default)]
pub(crate) struct EndedSessions {
    order: VecDeque<String>,
    set: HashSet<String>,
}

const ENDED_CAPACITY: usize = 1024;

impl EndedSessions {
    pub(crate) fn insert(&mut self, session_id: &str) {
        if self.set.insert(session_id.to_string()) {
            self.order.push_back(session_id.to_string());
            if self.order.len() > ENDED_CAPACITY
                && let Some(oldest) = self.order.pop_front()
            {
                self.set.remove(&oldest);
            }
        }
    }

    /// Whether to process a record. An ended session stays ended for late
    /// records, but a new start or prompt brings it back: a resumed session
    /// reuses its id.
    pub(crate) fn admits(&mut self, record: &HookRecord) -> bool {
        if !self.set.contains(&record.session_id) {
            return true;
        }
        if !matches!(record.event.as_str(), "SessionStart" | "UserPromptSubmit") {
            return false;
        }
        self.set.remove(&record.session_id);
        self.order.retain(|session| *session != record.session_id);
        true
    }
}
