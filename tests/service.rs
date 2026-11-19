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
    assert_eq!(ids, ["/", "/claude", "/codex", "/grok", "/muse"]);
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

/// Setup adds the hook to every event and is safe to repeat; removing it
/// leaves other settings and other tools' hooks exactly as they were.
#[test]
fn setup_adds_and_removes_hooks() {
    let dir = temp_dir("setup");
    let [claude, codex, grok, config] = ["claude", "codex", "grok", "config"].map(|name| {
        let path = dir.path().join(name);
        std::fs::create_dir_all(&path).unwrap();
        path
    });
    std::fs::create_dir_all(config.join("muse")).unwrap();
    let settings = claude.join("settings.json");
    let original = "{\n  \"theme\": \"dark\",\n  \"hooks\": {\n    \"Stop\": [\n      {\n        \"hooks\": [\n          {\n            \"type\": \"command\",\n            \"command\": \"other-tool\"\n          }\n        ]\n      }\n    ]\n  },\n  \"model\": \"opus\"\n}\n";
    std::fs::write(&settings, original).unwrap();
    // Codex trusts hooks by position: an existing Cusco hook between two
    // other hooks must stay where it is.
    let group = |command: &str| json!({"hooks": [{"type": "command", "command": command}]});
    let codex_hooks = codex.join("hooks.json");
    let stop = json!([
        group("first"),
        group("/old/path/cusco hook codex"),
        group("last")
    ]);
    std::fs::write(&codex_hooks, json!({"hooks": {"Stop": stop}}).to_string()).unwrap();
    let envs = [
        ("CLAUDE_CONFIG_DIR", claude.as_path()),
        ("CODEX_HOME", codex.as_path()),
        ("GROK_HOME", grok.as_path()),
        ("XDG_CONFIG_HOME", config.as_path()),
    ];
    let setup = |args: &[&str]| run_cusco_env(args, dir.path(), &envs, "");
    let read = |path: &std::path::Path| -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    };

    assert!(setup(&["setup"]).status.success());
    let added = std::fs::read_to_string(&settings).unwrap();
    let parsed = read(&settings);
    let keys: Vec<_> = parsed.as_object().unwrap().keys().cloned().collect();
    assert_eq!(keys, ["theme", "hooks", "model"], "key order is kept");
    assert_eq!(parsed["hooks"]["Stop"].as_array().unwrap().len(), 2);
    let command =
        |value: &serde_json::Value| value["hooks"][0]["command"].as_str().unwrap().to_string();
    assert!(command(&parsed["hooks"]["PermissionRequest"][0]).ends_with("cusco hook claude"));
    let stop = read(&codex_hooks)["hooks"]["Stop"].clone();
    assert_eq!(command(&stop[0]), "first");
    assert!(
        command(&stop[1]).ends_with("cusco hook codex") && !command(&stop[1]).starts_with("/old")
    );
    assert_eq!(command(&stop[2]), "last");
    assert!(read(&grok.join("hooks/cusco.json"))["hooks"]["StopCancelled"].is_array());
    assert_eq!(
        read(&config.join("muse/settings.json"))["schema_version"],
        1
    );

    assert!(setup(&["setup"]).status.success());
    assert_eq!(
        std::fs::read_to_string(&settings).unwrap(),
        added,
        "repeating changes nothing"
    );

    assert!(setup(&["setup", "--remove"]).status.success());
    assert_eq!(
        read(&settings),
        serde_json::from_str::<serde_json::Value>(original).unwrap()
    );
    assert!(
        !grok.join("hooks/cusco.json").exists(),
        "Cusco's own file goes"
    );
}

/// Claude's per-process session file states what hooks can only imply. Claude
/// rewrites it in place; going idle with a turn open means an interrupt.
#[test]
fn claude_session_file_drives_state() {
    let dir = temp_dir("registry");
    let sessions = dir.path().join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    let file = sessions.join(format!("{}.json", std::process::id()));
    let write = |status: &str| {
        let state = json!({"pid": std::process::id(), "sessionId": "s1", "status": status,
                           "cwd": "/repo", "entrypoint": "cli"});
        std::fs::write(&file, state.to_string()).unwrap();
    };
    write("busy");
    let _service = cusco::server::serve(cusco::server::Config {
        claude_sessions: Some(sessions.clone()),
        ..config(dir.path())
    })
    .unwrap();
    let request = r#"{"v":1,"op":"get","path":"/claude","depth":0}"#;
    wait_for_snapshot(dir.path(), request, |s| s["rows"][0]["state"] == "working");

    write("waiting");
    wait_for_snapshot(dir.path(), request, |s| {
        s["rows"][0]["state"] == "needs_input"
    });

    // Back to busy: the approval was answered, which no hook reports.
    write("busy");
    wait_for_snapshot(dir.path(), request, |s| s["rows"][0]["state"] == "working");

    write("idle");
    let snapshot = wait_for_snapshot(
        dir.path(),
        r#"{"v":1,"op":"get","path":"/claude/session:s1","depth":0}"#,
        |s| s["rows"][0]["state"] == "idle",
    );
    assert_eq!(
        snapshot["rows"][0]["attrs"]["last_turn"]["outcome"],
        "cancelled"
    );
}

/// The live view lists each client's sessions, the one that needs you first,
/// and expands sessions at work down to the request that's waiting.
#[test]
fn visualize_draws_the_tree() {
    let row = |id: &str, kind: &str, state: &str, attrs: serde_json::Value| {
        let row = json!({"id": id, "kind": kind, "state": state,
                         "since": "2026-11-18T20:00:00.000Z", "attrs": attrs});
        (id.to_string(), row)
    };
    let counts = json!({"sessions": 2, "needs_input": 1, "working": 0, "unknown": 0, "idle": 1});
    let mut machine = row("/", "machine", "needs_input", json!({}));
    machine.1["counts"] = counts;
    let rows = [
        machine,
        row("/claude", "client", "needs_input", json!({})),
        row(
            "/claude/session:s2",
            "session",
            "idle",
            json!({"cwd": "/src/ago\u{7}ra"}),
        ),
        row("/claude/session:s2/turn:old", "turn", "idle", json!({})),
        row(
            "/claude/session:s9",
            "session",
            "needs_input",
            json!({"cwd": "/src/cusco"}),
        ),
        row(
            "/claude/session:s9/turn:p1",
            "turn",
            "needs_input",
            json!({}),
        ),
        row(
            "/claude/session:s9/turn:p1/request:t9",
            "request",
            "needs_input",
            json!({"reason": "approval"}),
        ),
        row("/codex", "client", "idle", json!({})),
    ]
    .into_iter()
    .collect();
    let now = humantime::parse_rfc3339("2026-11-18T20:02:05Z").unwrap();
    let now = now
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    let screen = cusco::visualize::render(&rows, now, 60);
    let mut plain = String::new();
    let mut escape = false;
    for c in screen.chars() {
        match (escape, c) {
            (false, '\x1b') => escape = true,
            (true, 'm') => escape = false,
            (false, c) => plain.push(c),
            _ => {}
        }
    }
    let lines: Vec<&str> = plain.lines().collect();
    let line = |text: &str| lines.iter().position(|line| line.contains(text)).unwrap();
    assert!(lines[line("cusco ·")].contains("2 sessions"), "{plain}");
    assert!(
        lines[line("cusco s9")].contains("needs you 2m 05s"),
        "{plain}"
    );
    assert!(line("cusco s9") < line("approval") && line("approval") < line("agora"));
    assert!(!plain.contains("turn old"), "idle sessions stay collapsed");
    assert!(lines[line("codex")].contains("no sessions"));
    assert!(lines.iter().all(|line| line.chars().count() <= 60));
}
