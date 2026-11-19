//! The Claude Code adapter. See the design's "Claude Code" adapter section
//! and `docs/research/anthropic.md`.

use super::{
    Actor, Adapter, Reason, Sessions, Tracker, close, open_session, request_key, session_path,
};
use crate::record::HookRecord;
use crate::tree::{Attribution, Attrs, Client, Detection, Event, Millis, Observes, Outcome};
use std::collections::{HashMap, HashSet};

const CLIENT: Client = Client {
    name: "claude",
    observes: Observes {
        agents: true,
        approvals: Attribution::Actor,
        questions: true,
        mcp_input: true,
        interrupts: Detection::Inferred,
    },
};

#[derive(Default)]
pub struct ClaudeAdapter {
    sessions: Sessions,
    /// The session each Claude process is running.
    processes: HashMap<u32, String>,
}

impl ClaudeAdapter {
    pub fn new() -> ClaudeAdapter {
        ClaudeAdapter::default()
    }

    fn end(&mut self, session_id: &str) {
        self.sessions.end(session_id);
        self.processes.retain(|_, session| session != session_id);
    }
}

impl Adapter for ClaudeAdapter {
    fn client(&self) -> Client {
        CLIENT
    }

    fn handle(&mut self, record: &HookRecord, at: Millis) -> Vec<Event> {
        if !self.sessions.admits(record, &record.session_id) {
            return Vec::new();
        }
        let mut events = Vec::new();
        // A Claude process runs one session at a time, so a new session id
        // from the same process (`/clear`, `/resume`, a fork) ends the old one.
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
        let tracker = self.sessions.tracker(CLIENT.name, &record.session_id);
        handle_event(tracker, record, at, &mut events);
        events
    }

    fn forget_session(&mut self, session_id: &str) {
        self.end(session_id);
    }
}

fn handle_event(tracker: &mut Tracker, record: &HookRecord, at: Millis, events: &mut Vec<Event>) {
    let turn = record.turn_id.as_deref();
    let actor = match &record.agent_id {
        Some(agent) => tracker.agent(agent, turn),
        None => tracker.root(turn),
    };
    let tool_use_id = record.tool_use_id.as_deref();
    match record.event.as_str() {
        "UserPromptSubmit" => {
            // `source` tells a person's prompt from a turn the machine started,
            // such as a background task reporting back.
            let mut attrs = Attrs::new();
            if let Some(source) = &record.source {
                let origin = if matches!(source.as_str(), "user" | "sdk") {
                    "person"
                } else {
                    "machine"
                };
                attrs.insert("origin".into(), origin.into());
            }
            tracker.begin_turn(turn, attrs, events);
        }
        "Stop" => {
            tracker.end_turn(turn, Outcome::Ok, events);
            reconcile(tracker, record, events);
        }
        "StopFailure" => tracker.end_turn(turn, Outcome::Error, events),
        "Notification" => match record.notification_type.as_deref() {
            // Claude has no interrupt hook: waiting at the prompt means the
            // turn is over.
            Some("idle_prompt") => tracker.end_turn(None, Outcome::Cancelled, events),
            Some("permission_prompt") if tracker.has_requests() => tracker.mark_prompted(events),
            // A prompt no hook announced, such as a sandbox network approval.
            Some("permission_prompt") => {
                if let Some(actor) = &actor {
                    tracker.open_request(actor, "prompt", Reason::Approval, at, events);
                }
            }
            Some("elicitation_complete" | "elicitation_response") => {
                tracker.close_reason(None, Reason::McpInput, events);
            }
            _ => {}
        },
        "SubagentStart" => {
            if let Some(agent) = &record.agent_id {
                tracker.start_agent(agent, turn, events);
            }
        }
        "SubagentStop" | "TeammateIdle" => {
            if let Some(agent) = &record.agent_id {
                tracker.stop_agent(agent, Outcome::Ok, events);
            }
            reconcile(tracker, record, events);
        }
        "PreToolUse" => {
            if let Some(actor) = &actor {
                tracker.answer_approvals(actor, at, events);
                tracker.remember_call(actor, record.digest.as_deref(), tool_use_id);
                match (record.tool_name.as_deref(), tool_use_id) {
                    (Some("AskUserQuestion"), Some(id)) => {
                        tracker.open_request(actor, id, Reason::Question, at, events);
                    }
                    _ => tracker.enter(actor, events),
                }
            }
        }
        "PermissionRequest" => {
            if let Some(actor) = &actor {
                let id = approval_id(tracker, actor, record);
                tracker.open_request(actor, &id, Reason::Approval, at, events);
            }
        }
        "PostToolUse" | "PostToolUseFailure" => {
            close_call(tracker, record, Outcome::Ok, events);
            if record
                .tool_name
                .as_deref()
                .is_some_and(|name| name.starts_with("mcp__"))
            {
                tracker.close_reason(actor.as_ref(), Reason::McpInput, events);
            }
            if let Some(actor) = &actor {
                tracker.enter(actor, events);
            }
        }
        // The calls in a batch are resolved: granted and finished, or denied.
        "PostToolBatch" => {
            for id in record.tool_use_ids.iter().flatten() {
                tracker.close_request(id, Outcome::Ok, events);
                tracker.forget_call(id);
            }
        }
        // Fires only for auto-mode denials; a person's denial sends nothing.
        "PermissionDenied" => close_call(tracker, record, Outcome::Cancelled, events),
        "Elicitation" => {
            if let Some(actor) = &actor {
                let id = elicitation_id(record);
                tracker.open_request(actor, &id, Reason::McpInput, at, events);
            }
        }
        "ElicitationResult" => tracker.close_request(&elicitation_id(record), Outcome::Ok, events),
        _ => {}
    }
}

/// The request id for an approval: the call it belongs to when the input
/// matches (or the actor has only one open call), else the input digest.
pub(crate) fn approval_id(tracker: &Tracker, actor: &Actor, record: &HookRecord) -> String {
    tracker
        .match_call(actor, record.digest.as_deref())
        .or_else(|| record.digest.clone())
        .unwrap_or_else(|| "approval".to_string())
}

/// Closes whatever request a finished or denied call was holding open.
fn close_call(
    tracker: &mut Tracker,
    record: &HookRecord,
    outcome: Outcome,
    events: &mut Vec<Event>,
) {
    if let Some(id) = &record.tool_use_id {
        tracker.close_request(id, outcome, events);
        tracker.forget_call(id);
    }
    if let Some(digest) = &record.digest {
        tracker.close_request(digest, outcome, events);
    }
}

/// Elicitations may come without an id; then the server names the request.
fn elicitation_id(record: &HookRecord) -> String {
    record
        .elicitation_id
        .clone()
        .or_else(|| {
            record
                .mcp_server
                .as_deref()
                .map(|server| request_key("mcp", server))
        })
        .unwrap_or_else(|| "mcp_input".to_string())
}

/// `Stop` and `SubagentStop` list the background tasks still running.
fn reconcile(tracker: &mut Tracker, record: &HookRecord, events: &mut Vec<Event>) {
    if let Some(tasks) = &record.background_tasks {
        let running: HashSet<&str> = tasks.iter().map(|task| task.id.as_str()).collect();
        tracker.reconcile_agents(&running, events);
    }
}
