//! The Codex adapter. See the design's "Codex" adapter section.
//!
//! Codex reports approvals per turn, not per subagent, and one Codex process
//! can host many sessions, so this adapter tracks less than the Claude one.

use super::{
    Adapter, EndedSessions, SIBLING_WINDOW, Turns, agent_path, close, open, open_session,
    request_path, session_path, turn_path, with_reason,
};
use crate::record::HookRecord;
use crate::tree::{Attribution, Attrs, Client, Detection, Event, Millis, Observes, Outcome};
use std::collections::HashMap;

const CLIENT: Client = Client {
    name: "codex",
    observes: Observes {
        agents: true,
        approvals: Attribution::Actor,
        questions: true,
        mcp_input: false,
        interrupts: Detection::Hook,
    },
};

#[derive(Default)]
pub struct CodexAdapter {
    sessions: HashMap<String, Session>,
    ended: EndedSessions,
}

impl CodexAdapter {
    pub fn new() -> CodexAdapter {
        CodexAdapter::default()
    }
}

impl Adapter for CodexAdapter {
    fn client(&self) -> Client {
        CLIENT
    }

    fn handle(&mut self, record: &HookRecord, at: Millis) -> Vec<Event> {
        if !self.ended.admits(record) {
            return Vec::new();
        }
        let session = session_path(CLIENT.name, &record.session_id);
        let mut events = vec![open_session(&session, record)];
        if record.event == "SessionEnd" {
            events.push(close(session, Outcome::Ok));
            self.forget_session(&record.session_id);
            return events;
        }
        self.sessions
            .entry(record.session_id.clone())
            .or_default()
            .handle(&session, record, at, &mut events);
        events
    }

    fn forget_session(&mut self, session_id: &str) {
        self.sessions.remove(session_id);
        self.ended.insert(session_id);
    }
}

#[derive(Default)]
struct Session {
    turns: Turns,
    /// Each known subagent and the turn that started it.
    agents: HashMap<String, String>,
    /// Open approvals per turn: request id and when it opened.
    approvals: HashMap<String, Vec<(String, Millis)>>,
}

impl Session {
    fn handle(&mut self, session: &str, record: &HookRecord, at: Millis, events: &mut Vec<Event>) {
        match record.event.as_str() {
            "UserPromptSubmit" => {
                if let Some((turn, previous)) = self.turns.begin(record) {
                    if previous.is_some() {
                        self.end_turn(session, previous, Outcome::Cancelled, events);
                    }
                    events.push(open(turn_path(session, &turn), Attrs::new()));
                }
            }
            "Stop" => self.end_turn(session, record.turn_id.clone(), Outcome::Ok, events),
            "Interrupt" => {
                self.end_turn(session, record.turn_id.clone(), Outcome::Cancelled, events);
            }
            "PermissionRequest" => {
                let Some(turn) = self.open_turn_of(record) else {
                    return;
                };
                let id = record.digest.clone().unwrap_or_else(|| "approval".into());
                self.enter_turn(session, &turn, events);
                let path = turn_path(session, &turn);
                events.push(open(request_path(&path, &id), with_reason("approval")));
                self.approvals.entry(turn).or_default().push((id, at));
            }
            // A new tool call in the turn means its approval was answered,
            // unless it falls within the sibling window of the approval.
            "PreToolUse" => {
                if let Some(turn) = self.turns.of(record)
                    && let Some(open_ids) = self.approvals.get_mut(&turn)
                {
                    let path = turn_path(session, &turn);
                    open_ids.retain(|(id, opened_at)| {
                        let answered = at.saturating_sub(*opened_at) >= SIBLING_WINDOW;
                        if answered {
                            events.push(close(request_path(&path, id), Outcome::Ok));
                        }
                        !answered
                    });
                }
                if let Some(turn) = self.open_turn_of(record) {
                    self.enter_turn(session, &turn, events);
                }
            }
            "PostToolUse" => {
                if let (Some(turn), Some(digest)) = (self.turns.of(record), &record.digest) {
                    let path = turn_path(session, &turn);
                    if let Some(open_ids) = self.approvals.get_mut(&turn)
                        && let Some(index) = open_ids.iter().position(|(id, _)| id == digest)
                    {
                        open_ids.remove(index);
                        events.push(close(request_path(&path, digest), Outcome::Ok));
                    }
                }
                if let Some(turn) = self.open_turn_of(record) {
                    self.enter_turn(session, &turn, events);
                }
            }
            "SubagentStart" => {
                if let (Some(agent), Some(turn)) = (&record.agent_id, self.open_turn_of(record)) {
                    self.enter_turn(session, &turn, events);
                    events.push(open(
                        agent_path(&turn_path(session, &turn), agent),
                        Attrs::new(),
                    ));
                    self.agents.insert(agent.clone(), turn);
                }
            }
            "SubagentStop" => {
                if let Some(agent) = &record.agent_id
                    && let Some(turn) = self.agents.remove(agent)
                {
                    events.push(close(
                        agent_path(&turn_path(session, &turn), agent),
                        Outcome::Ok,
                    ));
                }
            }
            _ => {}
        }
    }

    /// Opens the turn, or confirms it if it's already open. A turn Cusco
    /// hasn't seen (for example across a restart) is opened by its first
    /// activity.
    fn enter_turn(&mut self, session: &str, turn: &str, events: &mut Vec<Event>) {
        events.push(open(turn_path(session, turn), Attrs::new()));
        self.turns.adopt(turn);
    }

    /// The record's turn, unless it already closed: late records never open
    /// anything.
    fn open_turn_of(&self, record: &HookRecord) -> Option<String> {
        self.turns
            .of(record)
            .filter(|turn| !self.turns.is_closed(turn))
    }

    fn end_turn(
        &mut self,
        session: &str,
        turn: Option<String>,
        outcome: Outcome,
        events: &mut Vec<Event>,
    ) {
        if let Some(turn) = self.turns.finish(turn) {
            events.push(close(turn_path(session, &turn), outcome));
            self.approvals.remove(&turn);
        }
    }
}
