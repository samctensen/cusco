mod common;

use common::replay;
use cusco::adapter::claude::ClaudeAdapter;
use cusco::tree::{State, Tree};

const S1: &str = "/claude/session:s1";

fn run(fixture: &str) -> Tree {
    replay(&mut ClaudeAdapter::new(), fixture)
}

fn last_outcome(t: &Tree, session: &str) -> serde_json::Value {
    t.attrs(session).unwrap()["last_turn"]["outcome"].clone()
}

#[test]
fn first_demonstration() {
    let t = run("claude/first_demonstration");
    assert_eq!(t.state("/claude"), Some(State::NeedsInput));
    assert_eq!(
        t.cause("/claude").as_deref(),
        Some("/claude/session:s1/turn:p1/agent:a1/request:tu1")
    );
}

#[test]
fn finishing_the_subagent_turns_claude_idle() {
    let t = run("claude/first_demonstration_finish");
    assert_eq!(t.state("/claude"), Some(State::Idle));
}

#[test]
fn auto_mode_denial_closes_approval() {
    assert_eq!(
        run("claude/auto_denied").state("/claude"),
        Some(State::Working)
    );
}

#[test]
fn new_prompt_cancels_interrupted_turn() {
    let t = run("claude/interrupt_then_prompt");
    assert_eq!(last_outcome(&t, S1), "cancelled");
    assert!(t.contains("/claude/session:s1/turn:p2"));
    assert!(!t.contains("/claude/session:s1/turn:p1"));
}

#[test]
fn idle_prompt_cancels_interrupted_turn() {
    let t = run("claude/interrupt_then_idle_prompt");
    assert_eq!(t.state(S1), Some(State::Idle));
    assert_eq!(last_outcome(&t, S1), "cancelled");
}

#[test]
fn parallel_tool_does_not_clear_approval() {
    assert_eq!(
        run("claude/parallel_tools").state("/claude"),
        Some(State::NeedsInput)
    );
}

#[test]
fn next_tool_call_closes_denied_approval() {
    assert_eq!(
        run("claude/denied_approval").state("/claude"),
        Some(State::Working)
    );
}

#[test]
fn ask_user_question_is_a_request() {
    let t = run("claude/question");
    let row = t.rows().into_iter().find(|r| r.id == "/claude").unwrap();
    assert_eq!(row.reason.as_deref(), Some("question"));
}

#[test]
fn elicitation_opens_and_closes() {
    assert_eq!(
        run("claude/elicitation").state("/claude"),
        Some(State::Working)
    );
}

#[test]
fn stop_failure_records_error() {
    assert_eq!(last_outcome(&run("claude/stop_failure"), S1), "error");
}

#[test]
fn clear_closes_previous_session_in_process() {
    let t = run("claude/clear");
    assert!(!t.contains(S1));
    assert!(t.contains("/claude/session:s2"));
}

#[test]
fn late_events_do_not_reopen() {
    let t = run("claude/late_events");
    assert!(!t.contains(S1));
    assert_eq!(t.state("/claude"), Some(State::Idle));
}

#[test]
fn stop_reconciles_background_agents() {
    let t = run("claude/background_reconcile");
    assert!(t.contains("/claude/session:s1/turn:p1/agent:a1"));
    assert!(!t.contains("/claude/session:s1/turn:p1/agent:a2"));
}

#[test]
fn missing_background_tasks_skips_reconciliation() {
    let t = run("claude/no_background_list");
    assert!(t.contains("/claude/session:s1/turn:p1/agent:a1"));
}

#[test]
fn resumed_session_is_tracked_again() {
    assert_eq!(
        run("claude/resume_after_end").state("/claude"),
        Some(State::NeedsInput)
    );
}

#[test]
fn activity_opens_a_turn_seen_for_the_first_time() {
    let t = run("claude/restart_mid_turn");
    assert_eq!(t.state("/claude"), Some(State::Working));
    assert!(t.contains("/claude/session:s1/turn:p1"));
}

#[test]
fn approval_from_an_unseen_background_agent_is_shown() {
    let t = run("claude/restart_background_approval");
    assert_eq!(t.state("/claude"), Some(State::NeedsInput));
    assert_eq!(
        t.cause("/claude").as_deref(),
        Some("/claude/session:s1/turn:p1/agent:a7/request:d7")
    );
}

#[test]
fn unseen_background_agent_finishing_leaves_nothing_open() {
    let t = run("claude/restart_background_approval_done");
    assert_eq!(t.state("/claude"), Some(State::Idle));
    assert!(!t.contains("/claude/session:s1/turn:p1"));
}

#[test]
fn late_subagent_start_after_stop_opens_nothing() {
    let t = run("claude/late_subagent_start");
    assert_eq!(t.state("/claude"), Some(State::Idle));
    assert!(!t.contains("/claude/session:s1/turn:p1/agent:a9"));
}

#[test]
fn subagent_start_after_its_stop_opens_nothing() {
    let t = run("claude/stop_before_start");
    assert!(!t.contains("/claude/session:s1/turn:p1/agent:a1"));
}
