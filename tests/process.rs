mod common;

use common::*;
use eywa::process::{Hosting, ProcessRule};
use std::time::Duration;

/// The native Claude Code installer runs `~/.local/bin/claude`, a symlink to
/// a versioned binary, so the kernel's executable path never ends in
/// `claude`. Eywa also matches the name a process was launched as.
#[test]
fn finds_an_agent_launched_through_a_symlink() {
    let dir = temp_dir("symlink");
    let target = dir.path().join("2.1.301");
    signed_copy("/bin/sleep", &target);
    let link = dir.path().join("claude");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let mut child = std::process::Command::new(&link).arg("30").spawn().unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let rule = ProcessRule::new("claude", "claude", Hosting::Session);
    let found = eywa::process::list_processes(&[rule]);
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
    let _service = eywa::server::serve(eywa::server::Config {
        executables: vec![ProcessRule::new(
            "claude",
            agent.to_string_lossy(),
            Hosting::Session,
        )],
        ..config(dir.path())
    })
    .unwrap();
    let request = r#"{"v":1,"op":"get","path":"/claude","depth":0}"#;
    wait_for_snapshot(dir.path(), request, |s| s["rows"][0]["state"] == "unknown");
    child.kill().unwrap();
    child.wait().unwrap();
    wait_for_snapshot(dir.path(), request, |s| s["rows"][0]["state"] == "idle");
}

/// Muse runs as `muse-bin-<version>` behind a launcher script, and its
/// `serve` mode hosts many sessions, so only the plain process stands for an
/// unreported session.
#[test]
fn muse_matches_by_prefix_and_hosts_are_left_out() {
    let dir = temp_dir("muse");
    let binary = dir.path().join("muse-bin-1.4.23");
    signed_copy("/bin/zsh", &binary);
    let start = |extra: &[&str]| {
        std::process::Command::new(&binary)
            .args(["-c", "sleep 30; :"])
            .args(extra)
            .spawn()
            .unwrap()
    };
    let mut session = start(&[]);
    let mut host = start(&["serve"]);
    std::thread::sleep(Duration::from_millis(200));
    let rule = ProcessRule::new("muse", "muse-bin-*", Hosting::HostWhen(&["serve"]));
    let found = eywa::process::list_processes(&[rule]);
    for child in [&mut session, &mut host] {
        child.kill().unwrap();
        child.wait().unwrap();
    }
    assert!(found.contains(&("muse", session.id())), "{found:?}");
    assert!(!found.contains(&("muse", host.id())), "{found:?}");
}
