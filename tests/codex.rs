mod common;

use common::replay;
use cusco::adapter::codex::CodexAdapter;
use cusco::tree::{State, Tree};

fn run(fixture: &str) -> Tree {
    replay(&mut CodexAdapter::new(), fixture)
}

#[test]
fn approval_attaches_to_turn() {
    let t = run("codex/approval");
    assert_eq!(t.state("/codex"), Some(State::NeedsInput));
    assert_eq!(
        t.cause("/codex").as_deref(),
        Some("/codex/session:s1/turn:t1/request:d1")
    );
}

#[test]
fn next_tool_call_answers_approval() {
    assert_eq!(
        run("codex/approval_answered").state("/codex"),
        Some(State::Working)
    );
}

#[test]
fn unrelated_post_tool_use_keeps_approval() {
    assert_eq!(
        run("codex/parallel_post").state("/codex"),
        Some(State::NeedsInput)
    );
}

#[test]
fn interrupt_cancels_turn() {
    let t = run("codex/interrupt");
    assert_eq!(
        t.attrs("/codex/session:s1").unwrap()["last_turn"]["outcome"],
        "cancelled"
    );
    assert_eq!(t.state("/codex"), Some(State::Idle));
}

#[test]
fn subagent_outlives_turn() {
    assert_eq!(run("codex/subagent").state("/codex"), Some(State::Working));
}

#[test]
fn sessions_can_share_a_process() {
    let t = run("codex/shared_process");
    assert!(t.contains("/codex/session:s1"));
    assert!(t.contains("/codex/session:s2"));
}

#[test]
fn full_lifecycle_ends_idle_and_removed() {
    let t = run("codex/lifecycle");
    assert!(!t.contains("/codex/session:s1"));
    assert_eq!(t.state("/codex"), Some(State::Idle));
}

#[test]
fn client_row_reports_turn_level_requests() {
    let t = run("codex/approval");
    let row = t.rows().into_iter().find(|r| r.id == "/codex").unwrap();
    assert_eq!(
        row.attrs["observes"],
        serde_json::json!(["session", "turn", "agent", "request:turn"])
    );
}

#[test]
fn resumed_session_is_tracked_again() {
    assert_eq!(
        run("codex/resume_after_end").state("/codex"),
        Some(State::NeedsInput)
    );
}

#[test]
fn activity_opens_a_turn_seen_for_the_first_time() {
    assert_eq!(
        run("codex/restart_mid_turn").state("/codex"),
        Some(State::Working)
    );
}
