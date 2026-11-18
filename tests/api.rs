use cusco::api::*;
use cusco::row::Change;

#[test]
fn parse_defaults_and_views() {
    let request = parse_request(r#"{"v":1,"op":"subscribe"}"#).unwrap();
    assert!(matches!(request.op, Op::Subscribe));
    assert!(matches!(request.selection, Selection::Tree { ref path, depth: 1 } if path == "/"));
    let request = parse_request(r#"{"v":1,"op":"get","path":"/claude","depth":3}"#).unwrap();
    assert!(matches!(request.op, Op::Get));
    assert!(
        matches!(request.selection, Selection::Tree { ref path, depth: 3 } if path == "/claude")
    );
    let request = parse_request(r#"{"v":1,"op":"get","view":"requests"}"#).unwrap();
    assert!(matches!(request.selection, Selection::Requests));
}

#[test]
fn parse_errors() {
    assert_eq!(parse_request("nope").unwrap_err().code, "bad_request");
    assert_eq!(
        parse_request(r#"{"v":2,"op":"get"}"#).unwrap_err().code,
        "unsupported_version"
    );
    assert_eq!(
        parse_request(r#"{"v":1,"op":"delete"}"#).unwrap_err().code,
        "bad_request"
    );
    assert_eq!(
        parse_request(r#"{"v":1,"op":"get","view":"x"}"#)
            .unwrap_err()
            .code,
        "bad_request"
    );
    assert_eq!(
        parse_request(r#"{"v":1,"op":"get","path":"claude"}"#)
            .unwrap_err()
            .code,
        "bad_request"
    );
    assert_eq!(
        parse_request(r#"{"v":1,"op":"get","depth":-1}"#)
            .unwrap_err()
            .code,
        "bad_request"
    );
}

#[test]
fn tree_selection_respects_depth() {
    let all = Selection::Tree {
        path: "/".into(),
        depth: 1,
    };
    assert!(all.includes("/"));
    assert!(all.includes("/claude"));
    assert!(!all.includes("/claude/session:s1"));
    let claude = Selection::Tree {
        path: "/claude".into(),
        depth: 0,
    };
    assert!(claude.includes("/claude"));
    assert!(!claude.includes("/codex"));
    assert!(!claude.includes("/claude/session:s1"));
    let deep = Selection::Tree {
        path: "/claude".into(),
        depth: 2,
    };
    assert!(deep.includes("/claude/session:s1/turn:p1"));
    assert!(!deep.includes("/claudex/session:s1"));
}

#[test]
fn requests_selection_matches_request_ids_only() {
    assert!(Selection::Requests.includes("/claude/session:s1/turn:p1/request:r1"));
    assert!(!Selection::Requests.includes("/claude/session:s1/turn:p1"));
}

#[test]
fn messages_have_the_wire_shape() {
    let remove = change_line(7, &Change::Remove("/claude/session:s1".into()));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&remove).unwrap(),
        serde_json::json!({"v": 1, "type": "remove", "rev": 7, "id": "/claude/session:s1"})
    );
    let snapshot = snapshot_line(3, &[]);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&snapshot).unwrap(),
        serde_json::json!({"v": 1, "type": "snapshot", "rev": 3, "rows": []})
    );
    let error = error_line(&ApiError {
        code: "bad_request",
        message: "unknown view".into(),
    });
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&error).unwrap(),
        serde_json::json!({"v": 1, "type": "error", "code": "bad_request", "message": "unknown view"})
    );
    assert!(!remove.ends_with('\n'));
}

#[test]
fn full_subscriber_is_dropped_others_keep_receiving() {
    let mut subscribers = Subscribers::new(2);
    let root = || Selection::Tree {
        path: "/".into(),
        depth: 1,
    };
    let slow = subscribers.add(root(), 0, &[]);
    let fast = subscribers.add(root(), 0, &[]);
    fast.recv().unwrap();
    for rev in 1..=3 {
        subscribers.publish(&[(rev, Change::Remove("/claude".into()))]);
        let line = fast.recv().unwrap();
        assert!(line.contains(&format!(r#""rev":{rev}"#)), "{line}");
    }
    assert_eq!(subscribers.len(), 1);
    drop(slow);
}

#[test]
fn subscribers_only_receive_matching_changes() {
    let mut subscribers = Subscribers::new(8);
    let requests = subscribers.add(Selection::Requests, 0, &[]);
    requests.recv().unwrap();
    subscribers.publish(&[
        (1, Change::Remove("/claude".into())),
        (
            2,
            Change::Remove("/claude/session:s1/turn:p1/request:r1".into()),
        ),
    ]);
    assert!(requests.recv().unwrap().contains(r#""rev":2"#));
    assert!(requests.try_recv().is_err());
}
