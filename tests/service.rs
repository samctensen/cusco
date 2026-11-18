mod common;

use common::*;
use cusco::hook::send_record;
use cusco::record::HookRecord;
use serde_json::json;
use std::os::unix::fs::PermissionsExt;

fn record(fields: serde_json::Value) -> HookRecord {
    let mut record = json!({"client": "claude", "session_id": "s1", "turn_id": "p1"});
    record
        .as_object_mut()
        .unwrap()
        .extend(fields.as_object().unwrap().clone());
    serde_json::from_value(record).unwrap()
}

fn send(dir: &std::path::Path, fields: serde_json::Value) {
    send_record(dir, &record(fields)).unwrap();
}

#[test]
fn get_returns_one_snapshot() {
    let dir = temp_dir("get");
    let _service = start(dir.path());
    let mut reader = connect(dir.path(), r#"{"v":1,"op":"get","path":"/","depth":1}"#);
    let snapshot = next_json(&mut reader);
    let ids: Vec<_> = snapshot["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["/", "/claude", "/codex"]);
    let mut rest = String::new();
    assert_eq!(
        std::io::BufRead::read_line(&mut reader, &mut rest).unwrap(),
        0
    );
}

#[test]
fn subscribers_see_who_needs_them() {
    let dir = temp_dir("demo");
    let _service = start(dir.path());
    let mut watcher = connect(
        dir.path(),
        r#"{"v":1,"op":"subscribe","path":"/","depth":1}"#,
    );
    send(dir.path(), json!({"event": "UserPromptSubmit"}));
    send(
        dir.path(),
        json!({"event": "PermissionRequest", "tool_name": "Bash", "digest": "d1"}),
    );
    send(
        dir.path(),
        json!({"client": "codex", "event": "UserPromptSubmit"}),
    );
    wait_for_states(&mut watcher, |states| {
        states.get("/claude").map(String::as_str) == Some("needs_input")
            && states.get("/codex").map(String::as_str) == Some("working")
    });

    // A consumer that reconnects gets the waiting request in its snapshot.
    let snapshot = next_json(&mut connect(
        dir.path(),
        r#"{"v":1,"op":"get","view":"requests"}"#,
    ));
    assert_eq!(
        snapshot["rows"][0]["id"],
        "/claude/session:s1/turn:p1/request:d1"
    );
}

#[test]
fn silence_turns_work_unknown() {
    let dir = temp_dir("stale");
    let _service = start(dir.path());
    let mut watcher = connect(
        dir.path(),
        r#"{"v":1,"op":"subscribe","path":"/claude","depth":0}"#,
    );
    send(dir.path(), json!({"event": "UserPromptSubmit"}));
    wait_for_states(&mut watcher, |states| {
        states.get("/claude").map(String::as_str) == Some("unknown")
    });
}

#[test]
fn bad_input_is_ignored() {
    let dir = temp_dir("bad");
    let _service = start(dir.path());
    let socket = std::os::unix::net::UnixDatagram::unbound().unwrap();
    socket
        .send_to(b"\xff not json", dir.path().join("ingest.sock"))
        .unwrap();
    // Sent straight to the socket: this id would inject a turn into the path.
    send(
        dir.path(),
        json!({"event": "UserPromptSubmit", "session_id": "s9/turn:x"}),
    );
    send(dir.path(), json!({"event": "UserPromptSubmit"}));
    let snapshot = wait_for_snapshot(
        dir.path(),
        r#"{"v":1,"op":"get","path":"/claude","depth":1}"#,
        |s| s["rows"][0]["state"] == "working",
    );
    assert_eq!(snapshot["rows"].as_array().unwrap().len(), 2, "{snapshot}");

    let reply = next_json(&mut connect(dir.path(), "not json"));
    assert_eq!(reply["type"], "error");
}

#[test]
fn one_service_per_directory() {
    let dir = temp_dir("sock");
    std::fs::write(dir.path().join("api.sock"), b"").unwrap(); // left behind by a crash
    let first = start(dir.path());
    let second = cusco::server::serve(config(dir.path()));
    assert_eq!(
        second.err().map(|error| error.kind()),
        Some(std::io::ErrorKind::AddrInUse)
    );
    first.shutdown();
    assert!(!dir.path().join("api.sock").exists());
}

#[test]
fn directory_must_be_private() {
    let dir = temp_dir("perm");
    let created = dir.path().join("cusco");
    drop(cusco::server::serve(config(&created)).unwrap());
    let mode = std::fs::metadata(&created).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);

    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    let refused = cusco::server::serve(config(dir.path()));
    assert_eq!(
        refused.err().map(|error| error.kind()),
        Some(std::io::ErrorKind::PermissionDenied)
    );
}

/// A subscriber that stops reading is dropped; the others keep receiving.
#[test]
fn slow_subscriber_is_dropped() {
    use cusco::api::{Selection, Subscribers};
    use cusco::row::Change;
    let mut subscribers = Subscribers::new(2);
    let root = || Selection::Tree {
        path: "/".into(),
        depth: 1,
    };
    let _slow = subscribers.add(root(), 0, &[]);
    let fast = subscribers.add(root(), 0, &[]);
    fast.recv().unwrap();
    for rev in 1..=3 {
        subscribers.publish(&[(rev, Change::Remove("/claude".into()))]);
        fast.recv().unwrap();
    }
    assert_eq!(subscribers.len(), 1);
}

#[test]
fn command_line() {
    let dir = temp_dir("cli");
    let output = run_cusco(&["status"], dir.path(), "");
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("cusco is not running"));

    // The hook command never fails and never prints, even with nothing listening.
    let payload = r#"{"hook_event_name":"UserPromptSubmit","session_id":"s1","prompt_id":"p1"}"#;
    let output = run_cusco(&["hook", "claude"], dir.path(), payload);
    assert!(output.status.success() && output.stdout.is_empty());

    let _service = start(dir.path());
    run_cusco(&["hook", "claude"], dir.path(), payload);
    wait_for_snapshot(
        dir.path(),
        r#"{"v":1,"op":"get","path":"/claude","depth":0}"#,
        |s| s["rows"][0]["state"] == "working",
    );
    let output = run_cusco(
        &["status", "--path", "/claude", "--depth", "0"],
        dir.path(),
        "",
    );
    let snapshot: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(snapshot["rows"][0]["state"], "working");
}
