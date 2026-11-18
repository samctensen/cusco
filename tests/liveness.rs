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

/// The native Claude Code installer runs `~/.local/bin/claude`, a symlink to
/// a versioned binary, so the kernel's executable path never ends in
/// `claude`. The launch name still does.
#[test]
fn list_finds_process_launched_through_a_symlink() {
    let dir = short_temp_dir("symlink");
    let target = dir.path().join("2.1.301");
    signed_copy("/bin/sleep", &target);
    let link = dir.path().join("claude");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let mut child = std::process::Command::new(&link).arg("30").spawn().unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let launch_name = cusco::process::launch_name(child.id()).unwrap();
    assert!(launch_name.ends_with("/claude"), "{launch_name}");
    let found = cusco::process::list_processes(&[("claude", "claude".into())]);
    assert!(found.contains(&("claude", child.id())), "{found:?}");
    child.kill().unwrap();
    child.wait().unwrap();
}

/// Codex Desktop runs a supervisor whose child is the app server, both named
/// `codex`. Hooks only ever claim the nearer one, so the supervisor must not
/// become a process node that no session will ever claim.
#[test]
fn list_skips_a_match_that_is_an_ancestor_of_another() {
    let dir = short_temp_dir("nested");
    std::fs::create_dir(dir.path().join("outer")).unwrap();
    std::fs::create_dir(dir.path().join("inner")).unwrap();
    let outer = dir.path().join("outer/codex");
    let inner = dir.path().join("inner/codex");
    signed_copy("/bin/zsh", &outer);
    signed_copy("/bin/sleep", &inner);
    let mut parent = std::process::Command::new(&outer)
        .arg("-c")
        .arg(format!("{} 30; :", inner.display()))
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let found = cusco::process::list_processes(&[("codex", "codex".into())]);
    let child = found
        .iter()
        .map(|(_, pid)| *pid)
        .find(|pid| cusco::process::parent_pid(*pid) == Some(parent.id()));
    assert!(child.is_some(), "{found:?}");
    assert!(!found.contains(&("codex", parent.id())), "{found:?}");
    let _ = std::process::Command::new("kill")
        .arg(child.unwrap().to_string())
        .status();
    parent.kill().unwrap();
    parent.wait().unwrap();
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
