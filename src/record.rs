//! Hook records: the allowed fields of a client's hook payload.
//!
//! Prompts, responses, commands and tool arguments never reach a record.
//! Tool input is reduced to a digest so an approval can be matched with the
//! tool call it belongs to.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

pub const MAX_ID_BYTES: usize = 128;

pub const CLAUDE_EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PermissionRequest",
    "PermissionDenied",
    "PostToolUse",
    "PostToolUseFailure",
    "Elicitation",
    "ElicitationResult",
    "Notification",
    "SubagentStart",
    "SubagentStop",
    "Stop",
    "StopFailure",
    "SessionEnd",
];

pub const CODEX_EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PermissionRequest",
    "PostToolUse",
    "SubagentStart",
    "SubagentStop",
    "Stop",
    "Interrupt",
    "SessionEnd",
];

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct HookRecord {
    pub client: String,
    pub event: String,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notification_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elicitation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background_tasks: Option<Vec<BackgroundTask>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surface: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct BackgroundTask {
    pub id: String,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

#[derive(Debug, PartialEq)]
pub enum RecordError {
    NotAnObject,
    UnknownClient,
    UnknownEvent(String),
    MissingSessionId,
    InvalidId(&'static str),
}

impl HookRecord {
    /// Builds a record from a hook payload, keeping only the allowed fields.
    pub fn from_payload(client: &str, payload: &Value) -> Result<HookRecord, RecordError> {
        let object = payload.as_object().ok_or(RecordError::NotAnObject)?;
        // Claude calls its turn id `prompt_id`; Codex calls it `turn_id`.
        let (events, turn_field) = match client {
            "claude" => (CLAUDE_EVENTS, "prompt_id"),
            "codex" => (CODEX_EVENTS, "turn_id"),
            _ => return Err(RecordError::UnknownClient),
        };
        let event = text(object, "hook_event_name").unwrap_or_default();
        if !events.contains(&event.as_str()) {
            return Err(RecordError::UnknownEvent(event));
        }
        let session_id = id(object, "session_id")?.ok_or(RecordError::MissingSessionId)?;
        let turn_id = id(object, turn_field)?;
        let tool_name = text(object, "tool_name");
        let digest = match (&tool_name, object.get("tool_input")) {
            (Some(name), Some(input)) => Some(tool_digest(name, input)),
            _ => None,
        };
        Ok(HookRecord {
            client: client.to_string(),
            event,
            session_id,
            turn_id,
            agent_id: id(object, "agent_id")?,
            agent_type: text(object, "agent_type"),
            tool_use_id: id(object, "tool_use_id")?,
            tool_name,
            notification_type: text(object, "notification_type"),
            source: text(object, "source"),
            elicitation_id: id(object, "elicitation_id")?,
            error: text(object, "error"),
            background_tasks: background_tasks(object),
            cwd: text(object, "cwd"),
            transcript_path: text(object, "transcript_path"),
            digest,
            pid: None,
            surface: None,
        })
    }
}

impl HookRecord {
    /// Whether every id that can become part of a node path is valid. The
    /// service checks this on each datagram, since anything running as the
    /// user can write to the ingest socket.
    pub fn has_valid_ids(&self) -> bool {
        is_valid_id(&self.session_id)
            && [
                &self.turn_id,
                &self.agent_id,
                &self.tool_use_id,
                &self.elicitation_id,
                &self.digest,
            ]
            .into_iter()
            .flatten()
            .all(|id| is_valid_id(id))
    }
}

/// SHA-256 over the tool name, a newline and the tool input serialized with
/// sorted keys. Returns the first 16 bytes as lowercase hex.
pub fn tool_digest(tool_name: &str, tool_input: &Value) -> String {
    let mut hasher = Sha256::new();
    hasher.update(tool_name.as_bytes());
    hasher.update(b"\n");
    hasher.update(tool_input.to_string().as_bytes());
    hasher.finalize()[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn text(object: &Map<String, Value>, field: &str) -> Option<String> {
    object.get(field)?.as_str().map(String::from)
}

/// An id becomes part of a node path, so it must be short and contain no `/`.
fn is_valid_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_ID_BYTES && !value.contains('/')
}

fn id(object: &Map<String, Value>, field: &'static str) -> Result<Option<String>, RecordError> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if is_valid_id(value) => Ok(Some(value.clone())),
        Some(_) => Err(RecordError::InvalidId(field)),
    }
}

/// The background task list, or `None` if any entry lacks a usable id. A
/// partly read list must not look like "these tasks finished".
fn background_tasks(object: &Map<String, Value>) -> Option<Vec<BackgroundTask>> {
    object
        .get("background_tasks")?
        .as_array()?
        .iter()
        .map(|task| {
            let task = task.as_object()?;
            Some(BackgroundTask {
                id: text(task, "id").filter(|id| is_valid_id(id))?,
                kind: text(task, "type"),
                status: text(task, "status"),
            })
        })
        .collect()
}
