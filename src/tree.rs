//! The node tree: what Cusco has observed, and the states derived from it.
//!
//! Every node is identified by its path, for example
//! `/claude/session:7f3a/turn:12/agent:a2/request:t9`. The machine is `/` and
//! each client is `/<name>`; every deeper segment is `kind:id`.

use serde::Serialize;
use std::collections::BTreeMap;

/// Milliseconds since the Unix epoch.
pub type Millis = u64;

/// How long an open turn, agent or request may go without any event before
/// it becomes unconfirmed.
pub const STALE_AFTER: Millis = 3_600_000;

pub type Attrs = serde_json::Map<String, serde_json::Value>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Machine,
    Client,
    Process,
    Session,
    Turn,
    Agent,
    Request,
}

impl Kind {
    fn from_prefix(prefix: &str) -> Option<Kind> {
        Some(match prefix {
            "process" => Kind::Process,
            "session" => Kind::Session,
            "turn" => Kind::Turn,
            "agent" => Kind::Agent,
            "request" => Kind::Request,
            _ => return None,
        })
    }

    fn allows_child(self, child: Kind) -> bool {
        use Kind::*;
        matches!(
            (self, child),
            (Client, Process)
                | (Client, Session)
                | (Session, Turn)
                | (Turn, Agent)
                | (Turn, Request)
                | (Agent, Agent)
                | (Agent, Request)
        )
    }

    fn is_work(self) -> bool {
        matches!(self, Kind::Turn | Kind::Agent)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    NeedsInput,
    Working,
    Unknown,
    Idle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Ok,
    Error,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Open {
        id: String,
        attrs: Attrs,
    },
    Close {
        id: String,
        outcome: Outcome,
    },
    Attr {
        id: String,
        attrs: Attrs,
    },
    Lost {
        scope: String,
    },
    Sync {
        scope: String,
        nodes: Vec<(String, Attrs)>,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct Client {
    pub name: &'static str,
    pub observes: &'static [&'static str],
}

/// Returns the kind of node a path names, or `None` if the path is malformed.
pub fn kind_of(id: &str) -> Option<Kind> {
    if id == "/" {
        return Some(Kind::Machine);
    }
    let rest = id.strip_prefix('/')?;
    let mut segments = rest.split('/');
    let client = segments.next()?;
    if client.is_empty() || client.contains(':') {
        return None;
    }
    let mut kind = Kind::Client;
    for segment in segments {
        let (prefix, name) = segment.split_once(':')?;
        let child = Kind::from_prefix(prefix)?;
        if name.is_empty() || !kind.allows_child(child) {
            return None;
        }
        kind = child;
    }
    Some(kind)
}

fn parent_of(id: &str) -> Option<&str> {
    if id == "/" {
        return None;
    }
    match id.rfind('/') {
        Some(0) => Some("/"),
        Some(i) => Some(&id[..i]),
        None => None,
    }
}

fn depth_of(id: &str) -> usize {
    if id == "/" {
        0
    } else {
        id.matches('/').count()
    }
}

#[derive(Clone, Debug)]
struct Node {
    kind: Kind,
    open: bool,
    opened_at: Millis,
    attrs: Attrs,
}

pub struct Tree {
    nodes: BTreeMap<String, Node>,
    clients: Vec<Client>,
}

impl Tree {
    pub fn new(clients: &[Client], _stale_after: Millis, at: Millis) -> Tree {
        let mut nodes = BTreeMap::new();
        nodes.insert("/".to_string(), Node::new(Kind::Machine, at));
        for client in clients {
            nodes.insert(format!("/{}", client.name), Node::new(Kind::Client, at));
        }
        Tree {
            nodes,
            clients: clients.to_vec(),
        }
    }

    pub fn apply(&mut self, event: Event, at: Millis) {
        match event {
            Event::Open { id, attrs } => self.open(&id, attrs, at),
            Event::Close { id, outcome } => self.close(&id, outcome, at),
            Event::Attr { id, attrs } => {
                if let Some(node) = self.nodes.get_mut(&id) {
                    node.attrs.extend(attrs);
                }
            }
            Event::Lost { .. } | Event::Sync { .. } => {}
        }
        self.prune();
    }

    pub fn state(&self, id: &str) -> Option<State> {
        self.evaluate(id).map(|(state, _)| state)
    }

    pub fn cause(&self, id: &str) -> Option<String> {
        self.evaluate(id).and_then(|(_, cause)| cause)
    }

    pub fn contains(&self, id: &str) -> bool {
        self.nodes.contains_key(id)
    }

    pub fn attrs(&self, id: &str) -> Option<&Attrs> {
        self.nodes.get(id).map(|node| &node.attrs)
    }

    fn is_known_client(&self, id: &str) -> bool {
        let name = id[1..].split('/').next().unwrap_or("");
        self.clients.iter().any(|client| client.name == name)
    }

    fn open(&mut self, id: &str, attrs: Attrs, at: Millis) {
        let Some(kind) = kind_of(id) else { return };
        if matches!(kind, Kind::Machine | Kind::Client) || !self.is_known_client(id) {
            return;
        }
        let mut missing = Vec::new();
        let mut ancestor = parent_of(id);
        while let Some(path) = ancestor {
            if self.nodes.contains_key(path) {
                break;
            }
            missing.push(path.to_string());
            ancestor = parent_of(path);
        }
        for path in missing.into_iter().rev() {
            let kind = kind_of(&path).expect("ancestor of a valid path is valid");
            self.nodes.insert(path, Node::new(kind, at));
        }
        let node = self
            .nodes
            .entry(id.to_string())
            .or_insert_with(|| Node::new(kind, at));
        node.open = true;
        node.attrs.extend(attrs);
    }

    fn close(&mut self, id: &str, _outcome: Outcome, _at: Millis) {
        let Some(node) = self.nodes.get(id) else {
            return;
        };
        if !node.open || matches!(node.kind, Kind::Machine | Kind::Client) {
            return;
        }
        let kind = node.kind;
        let prefix = format!("{id}/");
        for (path, node) in self.nodes.range_mut(prefix.clone()..) {
            if !path.starts_with(&prefix) {
                break;
            }
            let direct_request = node.kind == Kind::Request && parent_of(path) == Some(id);
            if kind == Kind::Session || direct_request {
                node.open = false;
            }
        }
        if let Some(node) = self.nodes.get_mut(id) {
            node.open = false;
        }
    }

    /// Removes closed nodes that have nothing open beneath them.
    fn prune(&mut self) {
        let removable: Vec<String> = self
            .nodes
            .iter()
            .filter(|(path, node)| {
                !node.open
                    && !matches!(node.kind, Kind::Machine | Kind::Client)
                    && !self.descendants(path).any(|(_, child)| child.open)
            })
            .map(|(path, _)| path.clone())
            .collect();
        for path in removable {
            self.nodes.remove(&path);
        }
    }

    fn descendants<'a>(&'a self, id: &str) -> impl Iterator<Item = (&'a String, &'a Node)> + 'a {
        let prefix = if id == "/" {
            "/".to_string()
        } else {
            format!("{id}/")
        };
        let root = id == "/";
        self.nodes
            .range(prefix.clone()..)
            .take_while(move |(path, _)| path.starts_with(&prefix))
            .filter(move |(path, _)| !(root && path.as_str() == "/"))
    }

    fn subtree<'a>(&'a self, id: &str) -> impl Iterator<Item = (&'a String, &'a Node)> + 'a {
        self.nodes
            .get_key_value(id)
            .into_iter()
            .chain(self.descendants(id))
    }

    /// The derived state of a node and the node responsible for it.
    fn evaluate(&self, id: &str) -> Option<(State, Option<String>)> {
        self.nodes.get(id)?;
        let mut best: [Option<(&String, &Node)>; 2] = [None, None];
        for (path, node) in self.subtree(id).filter(|(_, node)| node.open) {
            let category = match node.kind {
                Kind::Request => 0,
                kind if kind.is_work() => 1,
                _ => continue,
            };
            let slot = &mut best[category];
            if slot.is_none_or(|current| outranks(path, node, current)) {
                *slot = Some((path, node));
            }
        }
        let states = [State::NeedsInput, State::Working];
        for (state, candidate) in states.into_iter().zip(best) {
            if let Some((path, _)) = candidate {
                return Some((state, Some(path.clone())));
            }
        }
        Some((State::Idle, None))
    }
}

/// Whether `(path, node)` is a better cause than `current`: deeper first,
/// then opened earlier, then the lower path.
fn outranks(path: &str, node: &Node, current: (&String, &Node)) -> bool {
    let (current_path, current_node) = current;
    (
        depth_of(path),
        std::cmp::Reverse(node.opened_at),
        std::cmp::Reverse(path),
    ) > (
        depth_of(current_path),
        std::cmp::Reverse(current_node.opened_at),
        std::cmp::Reverse(current_path.as_str()),
    )
}

impl Node {
    fn new(kind: Kind, at: Millis) -> Node {
        Node {
            kind,
            open: true,
            opened_at: at,
            attrs: Attrs::new(),
        }
    }
}
