#![allow(dead_code)]

use cusco::tree::{Client, Event, Millis, Outcome, STALE_AFTER, Tree};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

/// A short directory under `/tmp`, removed on drop. Unix socket paths must
/// stay under 104 bytes, so the system temp directory is too long.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn short_temp_dir(tag: &str) -> TempDir {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    let path = PathBuf::from(format!("/tmp/cusco-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    TempDir(path)
}

/// Replays `tests/fixtures/<fixture>.jsonl` through an adapter into a fresh
/// tree. Each line is one `HookRecord`; line n is applied at n seconds.
pub fn replay(adapter: &mut dyn cusco::adapter::Adapter, fixture: &str) -> Tree {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{fixture}.jsonl"));
    let text = std::fs::read_to_string(&path).unwrap();
    let mut tree = Tree::new(&[adapter.client()], STALE_AFTER, 0);
    for (n, line) in text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .enumerate()
    {
        let record: cusco::record::HookRecord = serde_json::from_str(line)
            .unwrap_or_else(|error| panic!("{fixture}:{}: {error}", n + 1));
        let at = n as Millis * 1000;
        for event in adapter.handle(&record, at) {
            tree.apply(event, at);
        }
    }
    tree
}

/// Runs the `cusco` binary with `CUSCO_DIR` set to `dir`.
pub fn run_cusco(args: &[&str], dir: &Path, stdin: &str) -> Output {
    run_cusco_env(args, dir, &[], stdin)
}

pub fn run_cusco_env(args: &[&str], dir: &Path, envs: &[(&str, &str)], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_cusco"))
        .args(args)
        .env("CUSCO_DIR", dir)
        .envs(envs.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

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
