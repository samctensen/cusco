#![allow(dead_code)]

use cusco::tree::{Client, Event, Millis, Outcome, STALE_AFTER, Tree};

pub const CLAUDE: Client = Client {
    name: "claude",
    observes: &["session", "turn", "agent", "request"],
};

pub fn tree() -> Tree {
    Tree::new(&[CLAUDE], STALE_AFTER, 0)
}

pub fn open(t: &mut Tree, id: &str, at: Millis) {
    open_with(t, id, serde_json::json!({}), at);
}

pub fn open_with(t: &mut Tree, id: &str, attrs: serde_json::Value, at: Millis) {
    let attrs = attrs.as_object().cloned().unwrap_or_default();
    t.apply(
        Event::Open {
            id: id.into(),
            attrs,
        },
        at,
    );
}

pub fn close(t: &mut Tree, id: &str, outcome: Outcome, at: Millis) {
    t.apply(
        Event::Close {
            id: id.into(),
            outcome,
        },
        at,
    );
}
