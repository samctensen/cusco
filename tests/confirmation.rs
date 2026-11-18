mod common;

use common::*;
use cusco::tree::{Event, STALE_AFTER, State};

#[test]
fn process_node_is_unknown() {
    let mut t = tree();
    open(&mut t, "/claude/process:42", 1);
    assert_eq!(t.state("/claude"), Some(State::Unknown));
    assert_eq!(t.cause("/claude").as_deref(), Some("/claude/process:42"));
}

#[test]
fn session_with_pid_replaces_process_node() {
    let mut t = tree();
    open(&mut t, "/claude/process:42", 1);
    open_with(
        &mut t,
        "/claude/session:s1",
        serde_json::json!({"pid": 42}),
        2,
    );
    assert!(!t.contains("/claude/process:42"));
    assert_eq!(t.state("/claude"), Some(State::Idle));
}

#[test]
fn process_exit_closes_its_sessions() {
    let mut t = tree();
    open(&mut t, "/claude/process:42", 1);
    open_with(
        &mut t,
        "/claude/session:s1",
        serde_json::json!({"pid": 42}),
        2,
    );
    open(&mut t, "/claude/session:s1/turn:p1", 3);
    open_with(
        &mut t,
        "/claude/session:s2",
        serde_json::json!({"pid": 43}),
        4,
    );
    assert_eq!(
        t.process_exited(42, 5),
        vec!["/claude/session:s1".to_string()]
    );
    assert!(!t.contains("/claude/session:s1"));
    assert!(t.contains("/claude/session:s2"));
}

#[test]
fn process_exit_removes_an_unlinked_process_node() {
    let mut t = tree();
    open(&mut t, "/claude/process:42", 1);
    assert!(t.process_exited(42, 2).is_empty());
    assert!(!t.contains("/claude/process:42"));
    assert_eq!(t.state("/claude"), Some(State::Idle));
}

#[test]
fn stale_work_becomes_unknown_never_idle() {
    let mut t = tree();
    open(&mut t, "/claude/session:s1/turn:p1", 0);
    t.expire(STALE_AFTER - 1);
    assert_eq!(t.state("/claude"), Some(State::Working));
    t.expire(STALE_AFTER);
    assert_eq!(t.state("/claude"), Some(State::Unknown));
}

#[test]
fn idle_session_never_goes_stale() {
    let mut t = tree();
    open(&mut t, "/claude/session:s1", 0);
    t.expire(10 * STALE_AFTER);
    assert_eq!(t.state("/claude"), Some(State::Idle));
}

#[test]
fn event_on_descendant_reconfirms() {
    let mut t = tree();
    open(&mut t, "/claude/session:s1/turn:p1", 0);
    t.expire(STALE_AFTER);
    open(
        &mut t,
        "/claude/session:s1/turn:p1/agent:a1",
        STALE_AFTER + 1,
    );
    assert_eq!(t.state("/claude"), Some(State::Working));
}

#[test]
fn next_deadline_tracks_oldest_open_work() {
    let mut t = tree();
    assert_eq!(t.next_deadline(), None);
    open(&mut t, "/claude/session:s1/turn:p1", 5);
    open(&mut t, "/claude/session:s2/turn:p2", 9);
    assert_eq!(t.next_deadline(), Some(5 + STALE_AFTER));
    t.expire(5 + STALE_AFTER);
    assert_eq!(t.next_deadline(), Some(9 + STALE_AFTER));
}

#[test]
fn lost_unconfirms_scope() {
    let mut t = tree();
    open(&mut t, "/claude/session:s1/turn:p1", 1);
    t.apply(
        Event::Lost {
            scope: "/claude/session:s1".into(),
        },
        2,
    );
    assert_eq!(t.state("/claude"), Some(State::Unknown));
}

#[test]
fn sync_replaces_subtree() {
    let mut t = tree();
    open(&mut t, "/claude/session:s1/turn:p1/agent:a1", 1);
    t.apply(
        Event::Sync {
            scope: "/claude/session:s1".into(),
            nodes: vec![("/claude/session:s1/turn:p2".into(), Default::default())],
        },
        2,
    );
    assert!(t.contains("/claude/session:s1"));
    assert!(!t.contains("/claude/session:s1/turn:p1"));
    assert!(t.contains("/claude/session:s1/turn:p2"));
    assert_eq!(t.state("/claude"), Some(State::Working));
}
