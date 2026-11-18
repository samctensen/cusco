mod common;

use cusco::record::{BackgroundTask, HookRecord, RecordError, tool_digest};
use serde_json::json;

#[test]
fn only_allowed_fields_leave_the_hook() {
    let payload = json!({
        "hook_event_name": "PreToolUse", "session_id": "s1", "prompt_id": "p1",
        "tool_name": "Bash", "tool_use_id": "tu1", "cwd": "/repo",
        "tool_input": {"command": "SECRET"}, "prompt": "SECRET",
        "background_tasks": [{"id": "a1", "description": "SECRET"}]
    });
    let record = HookRecord::from_payload("claude", &payload).unwrap();
    assert_eq!(record.turn_id.as_deref(), Some("p1"));
    assert_eq!(
        record.digest,
        Some(tool_digest("Bash", &json!({"command": "SECRET"})))
    );
    let wire = serde_json::to_string(&record).unwrap();
    assert!(!wire.contains("SECRET"), "{wire}");
}

#[test]
fn digest_ignores_key_order() {
    let a: serde_json::Value = serde_json::from_str(r#"{"a":1,"b":2}"#).unwrap();
    let b: serde_json::Value = serde_json::from_str(r#"{"b":2,"a":1}"#).unwrap();
    assert_eq!(tool_digest("Edit", &a), tool_digest("Edit", &b));
}

#[test]
fn bad_payloads_are_rejected() {
    let reject = |client: &str, payload: serde_json::Value| {
        HookRecord::from_payload(client, &payload).unwrap_err()
    };
    let stop =
        |session: serde_json::Value| json!({"hook_event_name": "Stop", "session_id": session});
    assert_eq!(
        reject("claude", stop(json!("s9/turn:x"))),
        RecordError::InvalidId("session_id")
    );
    assert_eq!(
        reject("claude", stop(json!("x".repeat(129)))),
        RecordError::InvalidId("session_id")
    );
    assert_eq!(
        reject("claude", json!({"hook_event_name": "Stop"})),
        RecordError::MissingSessionId
    );
    assert_eq!(
        reject(
            "claude",
            json!({"hook_event_name": "FileChanged", "session_id": "s1"})
        ),
        RecordError::UnknownEvent("FileChanged".into())
    );
    assert_eq!(
        reject("gemini", stop(json!("s1"))),
        RecordError::UnknownClient
    );
}

/// A partly readable task list must not look like "these tasks finished".
#[test]
fn background_task_list_is_all_or_nothing() {
    let tasks = |list: serde_json::Value| {
        let payload =
            json!({"hook_event_name": "Stop", "session_id": "s1", "background_tasks": list});
        HookRecord::from_payload("claude", &payload)
            .unwrap()
            .background_tasks
    };
    assert_eq!(tasks(json!([{"id": "a1"}])).unwrap()[0].id, "a1");
    assert_eq!(tasks(json!([{"id": "a1"}, {"type": "subagent"}])), None);
}

/// macOS caps local datagrams well below a large record, so the hook resends
/// it without its task list rather than losing it.
#[test]
fn oversized_record_is_still_delivered() {
    let dir = common::temp_dir("big");
    let receiver = std::os::unix::net::UnixDatagram::bind(dir.path().join("ingest.sock")).unwrap();
    let task = |i| BackgroundTask {
        id: format!("task{i}"),
        kind: None,
        status: None,
    };
    let record = HookRecord {
        client: "claude".into(),
        event: "Stop".into(),
        session_id: "s1".into(),
        background_tasks: Some((0..500).map(task).collect()),
        ..Default::default()
    };
    cusco::hook::send_record(dir.path(), &record).unwrap();
    let mut buffer = vec![0u8; 1 << 20];
    let length = receiver.recv(&mut buffer).unwrap();
    let received: HookRecord = serde_json::from_slice(&buffer[..length]).unwrap();
    assert_eq!(received.session_id, "s1");
}
