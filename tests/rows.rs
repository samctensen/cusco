mod common;

use common::*;
use cusco::row::Change;
use cusco::tree::Outcome;

fn upsert_ids(changes: &[Change]) -> Vec<String> {
    changes
        .iter()
        .filter_map(|change| match change {
            Change::Upsert(row) => Some(row.id.clone()),
            Change::Remove(_) => None,
        })
        .collect()
}

#[test]
fn first_changes_publish_machine_and_clients() {
    let mut t = tree();
    assert_eq!(upsert_ids(&t.take_changes()), vec!["/", "/claude"]);
    assert!(t.take_changes().is_empty());
}

#[test]
fn client_row_carries_cause_reason_and_counts() {
    let mut t = tree();
    open_with(
        &mut t,
        "/claude/session:s1/turn:p1/request:r1",
        serde_json::json!({"reason": "approval"}),
        1,
    );
    open(&mut t, "/claude/session:s2/turn:p2", 2);
    open(&mut t, "/claude/session:s3", 3);
    let row = t.rows().into_iter().find(|r| r.id == "/claude").unwrap();
    assert_eq!(
        serde_json::to_value(&row).unwrap(),
        serde_json::json!({
            "id": "/claude", "kind": "client", "state": "needs_input",
            "since": "1970-01-01T00:00:00.001Z", "reason": "approval",
            "cause": "/claude/session:s1/turn:p1/request:r1",
            "counts": {"sessions": 3, "needs_input": 1, "working": 1, "unknown": 0, "idle": 1},
            "attrs": {"observes": ["session", "turn", "agent", "request"]}
        })
    );
}

#[test]
fn session_rows_have_no_counts() {
    let mut t = tree();
    open(&mut t, "/claude/session:s1", 1);
    let row = t
        .rows()
        .into_iter()
        .find(|r| r.id == "/claude/session:s1")
        .unwrap();
    let value = serde_json::to_value(&row).unwrap();
    assert!(value.get("counts").is_none());
    assert!(value.get("cause").is_none());
    assert!(value.get("reason").is_none());
}

#[test]
fn removal_is_published_for_pruned_nodes() {
    let mut t = tree();
    open(&mut t, "/claude/session:s1/turn:p1", 1);
    t.take_changes();
    close(&mut t, "/claude/session:s1/turn:p1", Outcome::Ok, 2);
    assert!(
        t.take_changes()
            .iter()
            .any(|c| matches!(c, Change::Remove(id) if id == "/claude/session:s1/turn:p1"))
    );
}

#[test]
fn since_moves_only_when_state_changes() {
    let mut t = tree();
    open(&mut t, "/claude/session:s1/turn:p1", 1);
    open(&mut t, "/claude/session:s1/turn:p1/agent:a1", 2);
    let row = t.rows().into_iter().find(|r| r.id == "/claude").unwrap();
    assert_eq!(row.since, 1);
}

#[test]
fn closed_turn_sets_last_turn() {
    let mut t = tree();
    open(&mut t, "/claude/session:s1/turn:p1", 1);
    close(&mut t, "/claude/session:s1/turn:p1", Outcome::Error, 2);
    let attrs = t.attrs("/claude/session:s1").unwrap();
    assert_eq!(
        attrs["last_turn"],
        serde_json::json!({"outcome": "error", "ended_at": "1970-01-01T00:00:00.002Z"})
    );
}
