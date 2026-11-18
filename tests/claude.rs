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

#[test]
fn stop_never_reconciles_away_an_agent_waiting_for_input() {
    assert_eq!(
        run("claude/reconcile_keeps_waiting_agent").state("/claude"),
        Some(State::NeedsInput)
    );
}

/// Feeds one record line to the adapter at a chosen time.
fn feed(t: &mut Tree, adapter: &mut ClaudeAdapter, line: &str, at: cusco::tree::Millis) {
    use cusco::adapter::Adapter;
    let record = serde_json::from_str(line).unwrap();
    for event in adapter.handle(&record, at) {
        t.apply(event, at);
    }
}

#[test]
fn sibling_tool_call_racing_an_approval_does_not_answer_it() {
    use cusco::adapter::Adapter;
    let mut adapter = ClaudeAdapter::new();
    let mut t = Tree::new(&[adapter.client()], cusco::tree::STALE_AFTER, 0);
    let turn = r#""client":"claude","session_id":"s1","turn_id":"p1""#;
    let tool = |id: &str, digest: &str| {
        format!(
            r#"{{{turn},"event":"PreToolUse","tool_name":"Read","tool_use_id":"{id}","digest":"{digest}"}}"#
        )
    };
    feed(
        &mut t,
        &mut adapter,
        &format!(r#"{{{turn},"event":"UserPromptSubmit"}}"#),
        0,
    );
    feed(&mut t, &mut adapter, &tool("tu1", "d1"), 10);
    feed(
        &mut t,
        &mut adapter,
        &format!(r#"{{{turn},"event":"PermissionRequest","tool_name":"Read","digest":"d1"}}"#),
        12,
    );
    // A parallel sibling's hook raced the approval and arrived just after it.
    feed(&mut t, &mut adapter, &tool("tu2", "d2"), 15);
    assert_eq!(t.state("/claude"), Some(State::NeedsInput));
    // A tool call after the user had time to deny means the agent moved on.
    feed(&mut t, &mut adapter, &tool("tu3", "d3"), 1_500);
    assert_eq!(t.state("/claude"), Some(State::Working));
}
