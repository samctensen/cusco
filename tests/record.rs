use cusco::record::{HookRecord, RecordError, tool_digest};
use serde_json::json;

#[test]
fn claude_payload_keeps_only_allowed_fields() {
    let payload = json!({
        "hook_event_name": "PreToolUse", "session_id": "s1", "prompt_id": "p1",
        "tool_name": "Bash", "tool_use_id": "tu1",
        "tool_input": {"command": "SECRET-COMMAND"},
        "prompt": "SECRET-PROMPT", "cwd": "/repo"
    });
    let record = HookRecord::from_payload("claude", &payload).unwrap();
    assert_eq!(record.turn_id.as_deref(), Some("p1"));
    assert_eq!(record.tool_use_id.as_deref(), Some("tu1"));
    assert_eq!(record.cwd.as_deref(), Some("/repo"));
    let digest = tool_digest("Bash", &json!({"command": "SECRET-COMMAND"}));
    assert_eq!(record.digest.as_deref(), Some(digest.as_str()));
    let wire = serde_json::to_string(&record).unwrap();
    assert!(!wire.contains("SECRET"), "{wire}");
}

#[test]
fn background_tasks_keep_id_type_and_status_only() {
    let payload = json!({
        "hook_event_name": "Stop", "session_id": "s1",
        "background_tasks": [{"id": "a1", "type": "subagent", "status": "running",
                              "description": "SECRET-DESCRIPTION", "command": "SECRET-COMMAND"}]
    });
    let record = HookRecord::from_payload("claude", &payload).unwrap();
    let tasks = record.background_tasks.as_ref().unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(
        (tasks[0].id.as_str(), tasks[0].kind.as_str()),
        ("a1", "subagent")
    );
    assert!(!serde_json::to_string(&record).unwrap().contains("SECRET"));
}

#[test]
fn codex_turn_id_maps_to_turn_id() {
    let payload = json!({"hook_event_name": "Stop", "session_id": "s1", "turn_id": "t7"});
    let record = HookRecord::from_payload("codex", &payload).unwrap();
    assert_eq!(record.turn_id.as_deref(), Some("t7"));
}

#[test]
fn digest_is_stable_across_key_order() {
    let a: serde_json::Value = serde_json::from_str(r#"{"a":1,"b":2}"#).unwrap();
    let b: serde_json::Value = serde_json::from_str(r#"{"b":2,"a":1}"#).unwrap();
    assert_eq!(tool_digest("Edit", &a), tool_digest("Edit", &b));
    assert_eq!(tool_digest("Edit", &a).len(), 32);
    assert_ne!(tool_digest("Edit", &a), tool_digest("Write", &a));
}

#[test]
fn ids_with_slash_or_too_long_are_rejected() {
    let slash = json!({"hook_event_name": "Stop", "session_id": "a/b"});
    assert_eq!(
        HookRecord::from_payload("claude", &slash),
        Err(RecordError::InvalidId("session_id"))
    );
    let long = json!({"hook_event_name": "Stop", "session_id": "x".repeat(129)});
    assert_eq!(
        HookRecord::from_payload("claude", &long),
        Err(RecordError::InvalidId("session_id"))
    );
    let agent = json!({"hook_event_name": "Stop", "session_id": "s1", "agent_id": "a/1"});
    assert_eq!(
        HookRecord::from_payload("claude", &agent),
        Err(RecordError::InvalidId("agent_id"))
    );
}

#[test]
fn missing_session_id_is_rejected() {
    assert_eq!(
        HookRecord::from_payload("claude", &json!({"hook_event_name": "Stop"})),
        Err(RecordError::MissingSessionId)
    );
}

#[test]
fn unknown_event_is_rejected() {
    let payload = json!({"hook_event_name": "FileChanged", "session_id": "s1"});
    assert_eq!(
        HookRecord::from_payload("claude", &payload),
        Err(RecordError::UnknownEvent("FileChanged".into()))
    );
}

#[test]
fn unknown_client_and_non_object_are_rejected() {
    let payload = json!({"hook_event_name": "Stop", "session_id": "s1"});
    assert_eq!(
        HookRecord::from_payload("gemini", &payload),
        Err(RecordError::UnknownClient)
    );
    assert_eq!(
        HookRecord::from_payload("claude", &json!([1, 2])),
        Err(RecordError::NotAnObject)
    );
}
