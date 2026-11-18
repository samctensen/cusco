mod common;

use common::*;
use std::sync::mpsc;
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(5);

#[test]
fn list_finds_named_process() {
    let dir = short_temp_dir("list");
    let mut child = fake_agent(dir.path(), "claude");
    let path = dir.path().join("claude").to_str().unwrap().to_string();
    let found = cusco::process::list_processes(&[("claude", path)]);
    assert!(found.contains(&("claude", child.id())), "{found:?}");
    child.kill().unwrap();
    child.wait().unwrap();
}

#[test]
fn watch_exit_fires_on_kill_and_on_dead_pid() {
    let dir = short_temp_dir("exit");
    let mut child = fake_agent(dir.path(), "claude");
    let (sender, exits) = mpsc::channel();
    let dead = sender.clone();
    cusco::process::watch_exit(child.id(), move || sender.send("killed").unwrap());
    std::thread::sleep(Duration::from_millis(100));
    child.kill().unwrap();
    child.wait().unwrap();
    assert_eq!(exits.recv_timeout(WAIT).unwrap(), "killed");
    cusco::process::watch_exit(child.id(), move || dead.send("dead").unwrap());
    assert_eq!(exits.recv_timeout(WAIT).unwrap(), "dead");
}

#[test]
fn running_agent_shows_unknown_until_it_exits() {
    let dir = short_temp_dir("live");
    let mut child = fake_agent(dir.path(), "claude");
    let path = dir.path().join("claude").to_str().unwrap().to_string();
    let _server = cusco::server::serve(cusco::server::Config {
        executables: vec![("claude", path)],
        ..config(dir.path())
    })
    .unwrap();
    let request = r#"{"v":1,"op":"get","path":"/claude","depth":1}"#;
    let snapshot = wait_for_snapshot(dir.path(), request, |s| s["rows"][0]["state"] == "unknown");
    assert_eq!(
        snapshot["rows"][1]["id"],
        format!("/claude/process:{}", child.id())
    );
    child.kill().unwrap();
    child.wait().unwrap();
    wait_for_snapshot(dir.path(), request, |s| s["rows"][0]["state"] == "idle");
}

#[test]
fn session_pid_exit_closes_session() {
    let dir = short_temp_dir("pid");
    let mut child = fake_agent(dir.path(), "agent");
    let _server = start(dir.path());
    let record = cusco::record::HookRecord {
        client: "claude".into(),
        event: "UserPromptSubmit".into(),
        session_id: "s1".into(),
        turn_id: Some("p1".into()),
        pid: Some(child.id()),
        ..Default::default()
    };
    cusco::hook::send_record(dir.path(), &record).unwrap();
    let request = r#"{"v":1,"op":"get","path":"/claude","depth":0}"#;
    wait_for_snapshot(dir.path(), request, |s| s["rows"][0]["state"] == "working");
    child.kill().unwrap();
    child.wait().unwrap();
    wait_for_snapshot(dir.path(), request, |s| s["rows"][0]["state"] == "idle");
}
