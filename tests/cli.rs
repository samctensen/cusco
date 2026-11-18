mod common;

use common::*;

fn lines(output: &std::process::Output) -> Vec<serde_json::Value> {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn status_prints_snapshot() {
    let dir = short_temp_dir("cli");
    let _server = start(dir.path());
    let output = run_cusco(&["status", "--json"], dir.path(), "");
    assert!(output.status.success());
    let messages = lines(&output);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["type"], "snapshot");
    assert_eq!(messages[0]["rows"][1]["id"], "/claude");
}

#[test]
fn status_accepts_path_depth_and_requests() {
    let dir = short_temp_dir("cli-args");
    let _server = start(dir.path());
    let output = run_cusco(
        &["status", "--path", "/codex", "--depth", "0"],
        dir.path(),
        "",
    );
    let rows = lines(&output)[0]["rows"].as_array().unwrap().clone();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], "/codex");
    let output = run_cusco(&["status", "--requests"], dir.path(), "");
    assert_eq!(lines(&output)[0]["rows"], serde_json::json!([]));
}

#[test]
fn status_reports_a_rejected_request() {
    let dir = short_temp_dir("cli-reject");
    let _server = start(dir.path());
    let output = run_cusco(&["status", "--path", "codex"], dir.path(), "");
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(lines(&output)[0]["type"], "error");
}

#[test]
fn status_without_service_fails_clearly() {
    let dir = short_temp_dir("cli-off");
    let output = run_cusco(&["status"], dir.path(), "");
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("cusco is not running"));
}

#[test]
fn hook_reaches_running_service() {
    let dir = short_temp_dir("cli-hook");
    let _server = start(dir.path());
    run_cusco(
        &["hook", "claude"],
        dir.path(),
        r#"{"hook_event_name":"UserPromptSubmit","session_id":"s1","prompt_id":"p1"}"#,
    );
    wait_for_snapshot(
        dir.path(),
        r#"{"v":1,"op":"get","path":"/claude","depth":0}"#,
        |s| s["rows"][0]["state"] == "working",
    );
}

#[test]
fn unknown_command_and_bad_flags_exit_2() {
    let dir = short_temp_dir("cli-bad");
    assert_eq!(
        run_cusco(&["frobnicate"], dir.path(), "").status.code(),
        Some(2)
    );
    assert_eq!(
        run_cusco(&["status", "--depth", "deep"], dir.path(), "")
            .status
            .code(),
        Some(2)
    );
}
