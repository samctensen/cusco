mod common;

use common::*;
use cusco::tree::{Event, Kind, Outcome, State, kind_of};

#[test]
fn empty_client_is_idle() {
    let t = tree();
    assert_eq!(t.state("/"), Some(State::Idle));
    assert_eq!(t.state("/claude"), Some(State::Idle));
}

#[test]
fn open_turn_makes_ancestors_working() {
    let mut t = tree();
    open(&mut t, "/claude/session:s1/turn:p1", 1);
    assert!(t.contains("/claude/session:s1"));
    for id in ["/", "/claude", "/claude/session:s1"] {
        assert_eq!(t.state(id), Some(State::Working), "{id}");
    }
}

#[test]
fn open_session_alone_is_idle() {
    let mut t = tree();
    open(&mut t, "/claude/session:s1", 1);
    assert_eq!(t.state("/claude"), Some(State::Idle));
}

#[test]
fn request_outranks_work() {
    let mut t = tree();
    open(&mut t, "/claude/session:s1/turn:p1/agent:a1/request:r1", 1);
    assert_eq!(t.state("/claude"), Some(State::NeedsInput));
    assert_eq!(
        t.cause("/claude").as_deref(),
        Some("/claude/session:s1/turn:p1/agent:a1/request:r1")
    );
}

#[test]
fn sibling_progress_does_not_clear_request() {
    let mut t = tree();
    open(&mut t, "/claude/session:s1/turn:p1/agent:a1/request:r1", 1);
    open(&mut t, "/claude/session:s1/turn:p1/agent:a2", 2);
    close(
        &mut t,
        "/claude/session:s1/turn:p1/agent:a2",
        Outcome::Ok,
        3,
    );
    assert_eq!(t.state("/claude"), Some(State::NeedsInput));
}

#[test]
fn closing_actor_closes_its_requests() {
    let mut t = tree();
    open(&mut t, "/claude/session:s1/turn:p1/agent:a1/request:r1", 1);
    close(
        &mut t,
        "/claude/session:s1/turn:p1/agent:a1",
        Outcome::Ok,
        2,
    );
    assert!(!t.contains("/claude/session:s1/turn:p1/agent:a1/request:r1"));
    assert_eq!(t.state("/claude"), Some(State::Working));
}

#[test]
fn closing_turn_keeps_subagents() {
    let mut t = tree();
    open(&mut t, "/claude/session:s1/turn:p1/agent:a1", 1);
    close(&mut t, "/claude/session:s1/turn:p1", Outcome::Ok, 2);
    assert!(t.contains("/claude/session:s1/turn:p1"));
    assert_eq!(t.state("/claude/session:s1"), Some(State::Working));
    close(
        &mut t,
        "/claude/session:s1/turn:p1/agent:a1",
        Outcome::Ok,
        3,
    );
    assert!(!t.contains("/claude/session:s1/turn:p1"));
    assert_eq!(t.state("/claude/session:s1"), Some(State::Idle));
}

#[test]
fn closing_session_removes_everything_beneath() {
    let mut t = tree();
    open(&mut t, "/claude/session:s1/turn:p1/agent:a1/request:r1", 1);
    close(&mut t, "/claude/session:s1", Outcome::Ok, 2);
    assert!(!t.contains("/claude/session:s1"));
    assert!(!t.contains("/claude/session:s1/turn:p1"));
    assert_eq!(t.state("/claude"), Some(State::Idle));
}

#[test]
fn open_is_idempotent_and_merges_attrs() {
    let mut t = tree();
    open_with(
        &mut t,
        "/claude/session:s1",
        serde_json::json!({"cwd": "/a"}),
        1,
    );
    open_with(
        &mut t,
        "/claude/session:s1",
        serde_json::json!({"pid": 42}),
        2,
    );
    let a = t.attrs("/claude/session:s1").unwrap();
    assert_eq!(a["cwd"], "/a");
    assert_eq!(a["pid"], 42);
}

#[test]
fn attr_merges_into_existing_and_ignores_unknown() {
    let mut t = tree();
    open(&mut t, "/claude/session:s1", 1);
    let attrs = serde_json::json!({"cwd": "/b"})
        .as_object()
        .unwrap()
        .clone();
    t.apply(
        Event::Attr {
            id: "/claude/session:s1".into(),
            attrs,
        },
        2,
    );
    t.apply(
        Event::Attr {
            id: "/claude/session:s9".into(),
            attrs: Default::default(),
        },
        3,
    );
    assert_eq!(t.attrs("/claude/session:s1").unwrap()["cwd"], "/b");
    assert!(!t.contains("/claude/session:s9"));
}

#[test]
fn close_of_unknown_node_is_ignored() {
    let mut t = tree();
    close(&mut t, "/claude/session:nope", Outcome::Ok, 1);
    assert_eq!(t.state("/claude"), Some(State::Idle));
}

#[test]
fn invalid_paths_are_ignored() {
    let mut t = tree();
    open(&mut t, "/claude/turn:p1", 1);
    open(&mut t, "/gemini/session:s1", 1);
    assert_eq!(t.state("/"), Some(State::Idle));
    assert!(!t.contains("/gemini"));
    assert_eq!(
        kind_of("/claude/session:s1/turn:p1/agent:a1/agent:a2"),
        Some(Kind::Agent)
    );
    assert_eq!(kind_of("/claude/bogus:x"), None);
}

#[test]
fn cause_prefers_deepest_then_earliest() {
    let mut t = tree();
    open(&mut t, "/claude/session:s1/turn:p1/request:r1", 1);
    open(&mut t, "/claude/session:s2/turn:p2/agent:a1/request:r2", 2);
    open(&mut t, "/claude/session:s3/turn:p3/agent:a1/request:r3", 3);
    assert_eq!(
        t.cause("/claude").as_deref(),
        Some("/claude/session:s2/turn:p2/agent:a1/request:r2")
    );
}
