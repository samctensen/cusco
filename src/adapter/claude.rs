//! The Claude Code adapter. See the design's "Claude Code" adapter section.

use super::{
    Adapter, EndedSessions, Turns, agent_path, close, confirm, open, open_session, request_path,
    session_path, turn_path, with_reason,
};
use crate::record::HookRecord;
use crate::tree::{Attrs, Client, Event, Millis, Outcome};
use std::collections::{HashMap, HashSet, VecDeque};

const CLIENT: Client = Client {
    name: "claude",
    observes: &["session", "turn", "agent", "request"],
};

/// How many recent tool calls per actor are remembered for matching an
/// approval to its call.
const RECENT_CALLS: usize = 64;

#[derive(Default)]
pub struct ClaudeAdapter {
    sessions: HashMap<String, Session>,
    ended: EndedSessions,
    /// The session each Claude process is currently running.
    processes: HashMap<u32, String>,
}

impl ClaudeAdapter {
    pub fn new() -> ClaudeAdapter {
        ClaudeAdapter::default()
    }

    fn end(&mut self, session_id: &str) {
        self.sessions.remove(session_id);
        self.ended.insert(session_id);
        self.processes.retain(|_, session| session != session_id);
    }
}

impl Adapter for ClaudeAdapter {
    fn client(&self) -> Client {
        CLIENT
    }

    fn handle(&mut self, record: &HookRecord, _at: Millis) -> Vec<Event> {
        if self.ended.contains(&record.session_id) {
            return Vec::new();
        }
        let mut events = Vec::new();
        // One Claude process runs one session at a time, so a new session
        // from the same process means the previous one is over.
        if record.event == "SessionStart"
            && let Some(pid) = record.pid
            && let Some(previous) = self.processes.insert(pid, record.session_id.clone())
            && previous != record.session_id
        {
            events.push(close(session_path(CLIENT.name, &previous), Outcome::Ok));
            self.end(&previous);
        }
        let session = session_path(CLIENT.name, &record.session_id);
        events.push(open_session(&session, record));
        if record.event == "SessionEnd" {
            events.push(close(session, Outcome::Ok));
            self.end(&record.session_id);
            return events;
        }
        self.sessions
            .entry(record.session_id.clone())
            .or_default()
            .handle(&session, record, &mut events);
        events
    }

    fn forget_session(&mut self, session_id: &str) {
        self.end(session_id);
    }
}

#[derive(Default)]
struct Session {
    turns: Turns,
    /// Each known subagent and the turn that started it.
    agents: HashMap<String, String>,
    /// Recent `(digest, tool_use_id)` pairs per actor.
    calls: HashMap<String, VecDeque<(String, String)>>,
    /// Open requests by request id.
    requests: HashMap<String, Request>,
}

struct Request {
    actor: String,
    path: String,
    kind: RequestKind,
}

#[derive(Clone, Copy, PartialEq)]
enum RequestKind {
    Approval,
    Question,
    Input,
}

impl RequestKind {
    fn reason(self) -> &'static str {
        match self {
            RequestKind::Approval => "approval",
            RequestKind::Question => "question",
            RequestKind::Input => "mcp_input",
        }
    }
}

/// Whoever caused a record: the main thread of a turn, or a subagent.
struct Actor {
    /// The agent id, or empty for the main thread.
    key: String,
    turn: String,
    path: String,
    /// False when the record belongs to a turn that already closed: late
    /// records may close nodes but never open them.
    may_open: bool,
}

impl Session {
    fn handle(&mut self, session: &str, record: &HookRecord, events: &mut Vec<Event>) {
        match record.event.as_str() {
            "UserPromptSubmit" => self.start_turn(session, record, events),
            "Stop" => {
                self.end_turn(session, record.turn_id.clone(), Outcome::Ok, events);
                self.reconcile_agents(session, record, events);
            }
            "StopFailure" => self.end_turn(session, record.turn_id.clone(), Outcome::Error, events),
            "Notification" if record.notification_type.as_deref() == Some("idle_prompt") => {
                self.end_turn(session, record.turn_id.clone(), Outcome::Cancelled, events);
            }
            "SubagentStart" => {
                if let Some(actor) = self.actor(session, record)
                    && actor.may_open
                    && !actor.key.is_empty()
                {
                    events.push(open(actor.path, Attrs::new()));
                    self.agents.insert(actor.key, actor.turn);
                }
            }
            "SubagentStop" => {
                if let Some(agent) = &record.agent_id {
                    self.close_agent(session, agent, Outcome::Ok, events);
                }
            }
            "PreToolUse" => self.pre_tool_use(session, record, events),
            "PermissionRequest" => {
                if let Some(actor) = self.actor(session, record) {
                    let id = self.approval_id(&actor, record);
                    self.open_request(actor, id, RequestKind::Approval, events);
                }
            }
            "PostToolUse" | "PostToolUseFailure" => {
                self.close_tool_requests(record, Outcome::Ok, events);
                if let Some(actor) = self.actor(session, record) {
                    events.push(confirm(actor.path));
                }
            }
            "PermissionDenied" => self.close_tool_requests(record, Outcome::Cancelled, events),
            "Elicitation" => {
                if let (Some(actor), Some(id)) =
                    (self.actor(session, record), record.elicitation_id.clone())
                {
                    self.open_request(actor, id, RequestKind::Input, events);
                }
            }
            "ElicitationResult" => {
                if let Some(id) = &record.elicitation_id {
                    self.close_request(id, Outcome::Ok, events);
                }
            }
            _ => {}
        }
    }

    fn actor(&self, session: &str, record: &HookRecord) -> Option<Actor> {
        match &record.agent_id {
            Some(agent) => {
                // A known agent stays under the turn that started it, even if
                // later records carry a newer prompt id.
                let known = self.agents.get(agent);
                let turn = known.cloned().or_else(|| self.turns.of(record))?;
                let may_open = known.is_some() || !self.turns.is_closed(&turn);
                let path = agent_path(&turn_path(session, &turn), agent);
                Some(Actor {
                    key: agent.clone(),
                    turn,
                    path,
                    may_open,
                })
            }
            None => {
                let turn = self.turns.of(record)?;
                let may_open = !self.turns.is_closed(&turn);
                let path = turn_path(session, &turn);
                Some(Actor {
                    key: String::new(),
                    turn,
                    path,
                    may_open,
                })
            }
        }
    }

    fn start_turn(&mut self, session: &str, record: &HookRecord, events: &mut Vec<Event>) {
        let Some((turn, previous)) = self.turns.begin(record) else {
            return;
        };
        // Claude has no interrupt hook: a new prompt while a turn is still
        // open means that turn was interrupted.
        if previous.is_some() {
            self.end_turn(session, previous, Outcome::Cancelled, events);
        }
        events.push(open(turn_path(session, &turn), Attrs::new()));
    }

    fn end_turn(
        &mut self,
        session: &str,
        turn: Option<String>,
        outcome: Outcome,
        events: &mut Vec<Event>,
    ) {
        let Some(turn) = self.turns.finish(turn) else {
            return;
        };
        let path = turn_path(session, &turn);
        events.push(close(path.clone(), outcome));
        // The tree closes the turn's own requests; forget them here too.
        let prefix = format!("{path}/request:");
        self.requests
            .retain(|_, request| !request.path.starts_with(&prefix));
    }

    /// Closes the session's agents that a `Stop` no longer lists as
    /// background tasks. A `Stop` without the list tells us nothing.
    fn reconcile_agents(&mut self, session: &str, record: &HookRecord, events: &mut Vec<Event>) {
        let Some(tasks) = &record.background_tasks else {
            return;
        };
        let listed: HashSet<&str> = tasks.iter().map(|task| task.id.as_str()).collect();
        let finished: Vec<String> = self
            .agents
            .keys()
            .filter(|agent| !listed.contains(agent.as_str()))
            .cloned()
            .collect();
        for agent in finished {
            self.close_agent(session, &agent, Outcome::Ok, events);
        }
    }

    fn close_agent(
        &mut self,
        session: &str,
        agent: &str,
        outcome: Outcome,
        events: &mut Vec<Event>,
    ) {
        if let Some(turn) = self.agents.remove(agent) {
            events.push(close(
                agent_path(&turn_path(session, &turn), agent),
                outcome,
            ));
            self.requests.retain(|_, request| request.actor != agent);
            self.calls.remove(agent);
        }
    }

    fn pre_tool_use(&mut self, session: &str, record: &HookRecord, events: &mut Vec<Event>) {
        let Some(actor) = self.actor(session, record) else {
            return;
        };
        // A new tool call means the actor has moved on, so any approval it
        // was waiting on has been answered. No hook reports a manual denial.
        let answered: Vec<String> = self
            .requests
            .iter()
            .filter(|(_, request)| {
                request.actor == actor.key && request.kind == RequestKind::Approval
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in answered {
            self.close_request(&id, Outcome::Ok, events);
        }
        if let (Some(digest), Some(tool_use_id)) = (&record.digest, &record.tool_use_id) {
            let calls = self.calls.entry(actor.key.clone()).or_default();
            calls.push_back((digest.clone(), tool_use_id.clone()));
            if calls.len() > RECENT_CALLS {
                calls.pop_front();
            }
        }
        match (&record.tool_name, &record.tool_use_id) {
            (Some(tool), Some(tool_use_id)) if tool == "AskUserQuestion" => {
                let id = tool_use_id.clone();
                self.open_request(actor, id, RequestKind::Question, events);
            }
            _ => events.push(confirm(actor.path)),
        }
    }

    /// The `tool_use_id` of the actor's most recent call with this input, or
    /// the digest itself, or a fixed id when neither is known.
    fn approval_id(&self, actor: &Actor, record: &HookRecord) -> String {
        let Some(digest) = &record.digest else {
            return "approval".to_string();
        };
        self.calls
            .get(&actor.key)
            .and_then(|calls| calls.iter().rev().find(|(seen, _)| seen == digest))
            .map_or_else(|| digest.clone(), |(_, tool_use_id)| tool_use_id.clone())
    }

    fn open_request(
        &mut self,
        actor: Actor,
        id: String,
        kind: RequestKind,
        events: &mut Vec<Event>,
    ) {
        if !actor.may_open {
            return;
        }
        let path = request_path(&actor.path, &id);
        events.push(open(path.clone(), with_reason(kind.reason())));
        if !actor.key.is_empty() {
            self.agents.entry(actor.key.clone()).or_insert(actor.turn);
        }
        self.requests.insert(
            id,
            Request {
                actor: actor.key,
                path,
                kind,
            },
        );
    }

    fn close_request(&mut self, id: &str, outcome: Outcome, events: &mut Vec<Event>) {
        if let Some(request) = self.requests.remove(id) {
            events.push(close(request.path, outcome));
        }
    }

    /// Closes the request a finished or denied tool call was waiting on,
    /// matched by `tool_use_id` or, failing that, by input digest.
    fn close_tool_requests(
        &mut self,
        record: &HookRecord,
        outcome: Outcome,
        events: &mut Vec<Event>,
    ) {
        for id in [&record.tool_use_id, &record.digest].into_iter().flatten() {
            self.close_request(id, outcome, events);
        }
    }
}
