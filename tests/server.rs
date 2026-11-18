mod common;

use common::*;
use cusco::hook::send_record;
use cusco::record::HookRecord;

fn record(client: &str, event: &str, session: &str, turn: Option<&str>) -> HookRecord {
    HookRecord {
        client: client.into(),
        event: event.into(),
        session_id: session.into(),
        turn_id: turn.map(Into::into),
        ..Default::default()
    }
}

fn send_fixture(dir: &std::path::Path, fixture: &str) {
    for line in fixture.lines() {
        send_record(dir, &serde_json::from_str(line).unwrap()).unwrap();
    }
}

#[test]
fn get_returns_snapshot_and_closes() {
    let dir = short_temp_dir("get");
    let _server = start(dir.path());
    let mut reader = connect(dir.path(), r#"{"v":1,"op":"get","path":"/","depth":1}"#);
    let snapshot = next_json(&mut reader);
    assert_eq!(snapshot["type"], "snapshot");
    let ids: Vec<_> = snapshot["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["/", "/claude", "/codex"]);
    assert!(next_line_is_eof(&mut reader));
}

#[test]
fn first_demonstration_end_to_end() {
    let dir = short_temp_dir("demo");
    let _server = start(dir.path());
    let mut watcher = connect(
        dir.path(),
        r#"{"v":1,"op":"subscribe","path":"/","depth":1}"#,
    );
    next_json(&mut watcher);
    send_fixture(
        dir.path(),
        include_str!("fixtures/claude/first_demonstration.jsonl"),
    );
    send_record(
        dir.path(),
        &record("codex", "UserPromptSubmit", "c1", Some("t1")),
    )
    .unwrap();
    wait_for_rows(&mut watcher, |rows| {
        rows.get("/claude").map(String::as_str) == Some("needs_input")
            && rows.get("/codex").map(String::as_str) == Some("working")
    });
}

#[test]
fn revisions_increase() {
    let dir = short_temp_dir("rev");
    let _server = start(dir.path());
    let mut watcher = connect(
        dir.path(),
        r#"{"v":1,"op":"subscribe","path":"/","depth":3}"#,
    );
    let mut last = next_json(&mut watcher)["rev"].as_u64().unwrap();
    send_record(
        dir.path(),
        &record("claude", "UserPromptSubmit", "s1", Some("p1")),
    )
    .unwrap();
    for _ in 0..3 {
        let rev = next_json(&mut watcher)["rev"].as_u64().unwrap();
        assert!(rev > last, "{rev} after {last}");
        last = rev;
    }
}

#[test]
fn reconnect_snapshot_shows_waiting_client() {
    let dir = short_temp_dir("recon");
    let _server = start(dir.path());
    send_fixture(
        dir.path(),
        include_str!("fixtures/claude/first_demonstration.jsonl"),
    );
    let snapshot = wait_for_snapshot(dir.path(), r#"{"v":1,"op":"get","view":"requests"}"#, |s| {
        s["rows"].as_array().unwrap().len() == 1
    });
    assert_eq!(
        snapshot["rows"][0]["id"],
        "/claude/session:s1/turn:p1/agent:a1/request:tu1"
    );
}

#[test]
fn staleness_deadline_wakes_core() {
    let dir = short_temp_dir("stale");
    let _server = start(dir.path());
    let mut watcher = connect(
        dir.path(),
        r#"{"v":1,"op":"subscribe","path":"/claude","depth":0}"#,
    );
    next_json(&mut watcher);
    send_record(
        dir.path(),
        &record("claude", "UserPromptSubmit", "s1", Some("p1")),
    )
    .unwrap();
    wait_for_rows(&mut watcher, |rows| {
        rows.get("/claude").map(String::as_str) == Some("unknown")
    });
}

#[test]
fn bad_request_gets_error_line() {
    let dir = short_temp_dir("bad");
    let _server = start(dir.path());
    let mut reader = connect(dir.path(), "not json");
    assert_eq!(next_json(&mut reader)["type"], "error");
    assert!(next_line_is_eof(&mut reader));
}

#[test]
fn garbage_datagram_is_ignored() {
    let dir = short_temp_dir("garbage");
    let _server = start(dir.path());
    let socket = std::os::unix::net::UnixDatagram::unbound().unwrap();
    socket
        .send_to(b"\xff\xfe not json", dir.path().join("ingest.sock"))
        .unwrap();
    // Sent straight to the socket, bypassing the hook command's validation:
    // this id would otherwise inject a turn node into the path.
    let mut bad_ids = record("claude", "UserPromptSubmit", "s9/turn:x", Some("p1"));
    send_record(dir.path(), &bad_ids).unwrap();
    bad_ids.session_id = "s1".into();
    send_record(dir.path(), &bad_ids).unwrap();
    let snapshot = wait_for_snapshot(
        dir.path(),
        r#"{"v":1,"op":"get","path":"/claude","depth":1}"#,
        |s| s["rows"][0]["state"] == "working",
    );
    let ids: Vec<_> = snapshot["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["/claude", "/claude/session:s1"]);
}

#[test]
fn stale_socket_is_replaced_and_second_serve_refused() {
    let dir = short_temp_dir("sock");
    std::fs::write(dir.path().join("api.sock"), b"").unwrap();
    let first = start(dir.path());
    let second = cusco::server::serve(config(dir.path()));
    assert_eq!(
        second.err().map(|error| error.kind()),
        Some(std::io::ErrorKind::AddrInUse)
    );
    first.shutdown();
    assert!(!dir.path().join("api.sock").exists());
    assert!(!dir.path().join("ingest.sock").exists());
}

/// Other users must not reach the sockets, and serve must not silently
/// change permissions on a directory it didn't create: it refuses instead.
#[test]
fn serve_refuses_a_directory_others_can_reach() {
    use std::os::unix::fs::PermissionsExt;
    let dir = short_temp_dir("perm");
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    let refused = cusco::server::serve(config(dir.path()));
    assert_eq!(
        refused.err().map(|error| error.kind()),
        Some(std::io::ErrorKind::PermissionDenied)
    );
    let mode = std::fs::metadata(dir.path()).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o755);
    assert!(!dir.path().join("api.sock").exists());
}

#[test]
fn serve_creates_a_missing_directory_private() {
    use std::os::unix::fs::PermissionsExt;
    let parent = short_temp_dir("perm-new");
    let dir = parent.path().join("cusco");
    let _server = cusco::server::serve(config(&dir)).unwrap();
    let mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);
}
