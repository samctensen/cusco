mod common;

use cusco::record::{BackgroundTask, HookRecord};
use std::os::unix::net::UnixDatagram;
use std::time::{Duration, Instant};

#[test]
fn large_record_is_delivered() {
    let dir = common::short_temp_dir("hook");
    let receiver = UnixDatagram::bind(dir.path().join("ingest.sock")).unwrap();
    let tasks = (0..300)
        .map(|i| BackgroundTask {
            id: format!("task{i}"),
            kind: "subagent".into(),
            status: "running".into(),
        })
        .collect();
    let record = HookRecord {
        client: "claude".into(),
        event: "Stop".into(),
        session_id: "s1".into(),
        background_tasks: Some(tasks),
        cwd: Some("/x".repeat(200)),
        ..Default::default()
    };
    cusco::hook::send_record(dir.path(), &record).unwrap();
    receiver
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut buffer = vec![0u8; 1 << 20];
    let n = receiver.recv(&mut buffer).unwrap();
    let received: HookRecord = serde_json::from_slice(&buffer[..n]).unwrap();
    assert_eq!(received.session_id, "s1");
    eprintln!(
        "large record delivered {} background tasks",
        received.background_tasks.map_or(0, |tasks| tasks.len())
    );
}

#[test]
fn hook_command_is_silent_without_service() {
    let dir = common::short_temp_dir("silent");
    let start = Instant::now();
    let output = common::run_cusco(
        &["hook", "claude"],
        dir.path(),
        r#"{"hook_event_name":"Stop","session_id":"s1"}"#,
    );
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(start.elapsed() < Duration::from_millis(500));
}

#[test]
fn hook_command_is_silent_on_garbage_input() {
    let dir = common::short_temp_dir("garbage-in");
    let output = common::run_cusco(&["hook", "claude"], dir.path(), "not json at all");
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
}

#[test]
fn record_env_appends_allowed_fields() {
    let dir = common::short_temp_dir("rec");
    let file = dir.path().join("records.jsonl");
    common::run_cusco_env(
        &["hook", "claude"],
        dir.path(),
        &[("CUSCO_RECORD", file.to_str().unwrap())],
        r#"{"hook_event_name":"Stop","session_id":"s1","last_assistant_message":"SECRET"}"#,
    );
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains(r#""session_id":"s1""#), "{text}");
    assert!(!text.contains("SECRET"), "{text}");
}

#[test]
fn ancestor_walk_finds_cargo() {
    let me = std::process::id();
    assert!(cusco::process::find_ancestor(me, "cargo").is_some());
    assert_eq!(
        cusco::process::find_ancestor(me, "no-such-executable"),
        None
    );
}
