//! Adapters turn one client's hook records into tree events. Each keeps the
//! per-session state that mapping needs, such as which tool call an approval
//! belongs to.

pub mod claude;

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

/// Confirms an existing node without ever creating one.
pub(crate) fn confirm(id: String) -> Event {
    Event::Attr {
        id,
        attrs: Attrs::new(),
    }
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

    pub(crate) fn contains(&self, session_id: &str) -> bool {
        self.set.contains(session_id)
    }
}
