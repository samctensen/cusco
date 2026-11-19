mod common;

use common::*;
use std::time::Duration;

/// The native Claude Code installer runs `~/.local/bin/claude`, a symlink to
/// a versioned binary, so the kernel's executable path never ends in
/// `claude`. Cusco also matches the name a process was launched as.
#[test]
fn finds_an_agent_launched_through_a_symlink() {
    let dir = temp_dir("symlink");
    let target = dir.path().join("2.1.301");
    signed_copy("/bin/sleep", &target);
    let link = dir.path().join("claude");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let mut child = std::process::Command::new(&link).arg("30").spawn().unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let found = cusco::process::list_processes(&[("claude", "claude".into())]);
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(found.contains(&("claude", child.id())), "{found:?}");
}

/// A running agent that hasn't reported shows `unknown`, and its exit is
/// noticed without polling.
#[test]
fn running_agent_is_unknown_until_it_exits() {
    let dir = temp_dir("live");
    let agent = dir.path().join("claude");
    signed_copy("/bin/sleep", &agent);
    let mut child = std::process::Command::new(&agent)
        .arg("30")
        .spawn()
        .unwrap();
    let _service = cusco::server::serve(cusco::server::Config {
        executables: vec![("claude", agent.to_string_lossy().into_owned())],
        ..config(dir.path())
    })
    .unwrap();
    let request = r#"{"v":1,"op":"get","path":"/claude","depth":0}"#;
    wait_for_snapshot(dir.path(), request, |s| s["rows"][0]["state"] == "unknown");
    child.kill().unwrap();
    child.wait().unwrap();
    wait_for_snapshot(dir.path(), request, |s| s["rows"][0]["state"] == "idle");
}
