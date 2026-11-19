# Alert Latency Quick Wins Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Claude session shows `idle` the moment its registry file says so, and Claude Code and Grok Build run Cusco's hook with no shell.

**Architecture:** The tree gains an `end` event that closes a node without an outcome. The Claude adapter uses it when the registry goes idle, remembers the turn as unsettled, and writes `last_turn` once a `Stop` hook, a new prompt or the settle timer decides it. `cusco setup` writes `command` plus `args` for the two clients that support it, and bare `cusco` with piped input becomes a silent no-op.

**Tech Stack:** Rust (edition 2024), `serde_json`, `libc`. macOS only. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-11-18-alert-latency-design.md`

## Global Constraints

- Base the work on `origin/main` at `4c3ee8e` (Release 0.3.0) or newer, on a branch named `feat/alert-latency`. Line numbers below are for that commit. `docs/readme-cleanup` is already merged; commit nothing there.
- No new dependencies.
- The hook command never prints, always exits 0 and never waits on Cusco.
- Nothing new leaves the hook command: `HookRecord` gains no fields.
- `IDLE_SETTLE` stays `2_000` ms.
- `last_turn` keeps its shape, `{outcome, ended_at}`. API version 1 is otherwise unchanged.
- Tests stay lean: only the tests named in this plan, added to existing test functions where the plan says so, no fixture files.
- Every task ends with `cargo test`, `cargo clippy --all-targets` and `cargo fmt --check` clean.
- Commit messages use the repo's `feat:` / `docs:` prefixes and carry no AI attribution: no `Co-Authored-By` trailer, no "Generated with" line.
- No version bump and no release in this plan.

## Review Focus

1. **A settling idle is read again.** Any registry file changing makes Cusco re-read every session, so a session already idle is observed idle again. Its timer must neither restart nor vanish, or the outcome never arrives. *Test: Task 2, the `other.json` block in `claude_session_file_drives_state`.*
2. **Interrupt while a subagent runs.** The main turn ends, its own requests close, the subagent keeps the session `working`. *Test: Task 1, the block added to `closing_rules`.*
3. **Session ends while unsettled.** `SessionEnd` or the process exiting must drop the pending outcome; the timer firing later writes nothing. *Test: Task 2, the last block of `idle_closes_the_turn_before_its_outcome_is_known`.*
4. **An install path with a space.** The no-shell form stores the path exactly; the shell form still quotes it. *Test: Task 3, the `hook_entry` asserts in `setup_adds_and_removes_hooks`.*
5. **Another tool's `args` hook.** A hook that isn't Cusco's but also uses `args` must survive setup and removal untouched. *Test: Task 3, the `other-tool` entry in `setup_adds_and_removes_hooks`.*

---

## Before Task 1: the branch

The spec and this plan are untracked files. Carry them onto the new branch (in a worktree, copy both files in first):

```bash
git fetch origin
git switch --no-track -c feat/alert-latency origin/main
git add docs/superpowers/specs/2026-11-18-alert-latency-design.md docs/superpowers/plans/2026-11-18-alert-latency.md
git commit -m "docs: add the alert latency design and plan"
```

### Task 1: The `end` event

**Files:**
- Modify: `src/tree.rs:80` (`Event`), `src/tree.rs:270` (`apply`), `src/tree.rs:452` (`close`)
- Modify: `docs/design.md:213-222`
- Test: `tests/tree.rs:34` (`closing_rules`)

**Interfaces:**
- Produces: `Event::End { id: String }`.
- Produces: `pub(crate) fn last_turn(outcome: Outcome, ended_at: Millis) -> serde_json::Value` in `src/tree.rs`, returning `{"outcome": outcome, "ended_at": rfc3339(ended_at)}`.

- [ ] **Step 1: Write the failing test.** Append to the end of `closing_rules`:

```rust
    // An end closes like a close but records no outcome: that follows later.
    open(&mut t, "/claude/session:s2/turn:p1/request:r1", 7);
    open(&mut t, "/claude/session:s2/turn:p1/agent:a1", 7);
    t.apply(
        Event::End {
            id: "/claude/session:s2/turn:p1".into(),
        },
        8,
    );
    assert_eq!(
        t.state("/claude/session:s2"),
        Some(State::Working),
        "the subagent keeps running"
    );
    assert!(
        !t.contains("/claude/session:s2/turn:p1/request:r1"),
        "the turn's own request closed with it"
    );
    assert!(
        t.attrs("/claude/session:s2")
            .unwrap()
            .get("last_turn")
            .is_none()
    );
```

- [ ] **Step 2: Run it.** `cargo test --test tree closing_rules`. Expected: does not compile, "no variant named `End`".

- [ ] **Step 3: Implement.** Add `Event::End { id }` after `Close`. `Tree::close` takes `outcome: Option<Outcome>` and writes `last_turn` only for `Some`, through the new `last_turn` helper; `Event::Close` and `process_exited` pass `Some`. `Event::End` is applied exactly as `Event::Close` is (confirm the parent, then close) with `None`.

- [ ] **Step 4: Run it.** `cargo test --test tree`. Expected: all pass. Then the three checks from Global Constraints.

- [ ] **Step 5: Update `docs/design.md`.** Line 213: "five event types" becomes "six event types". Add this row under the `close` row:

```markdown
| `end {id}` | A node ended and its outcome isn't known yet. It closes as `close` does; the outcome follows as an `attr`. |
```

Leave line 475 ("**v0.1:** the tree, the five events"): it records what v0.1 shipped.

- [ ] **Step 6: Commit.**

```bash
git add src/tree.rs tests/tree.rs docs/design.md
git commit -m "feat: let a node end before its outcome is known"
```

### Task 2: Idle at once, outcome later

**Files:**
- Modify: `src/adapter/mod.rs:20` (`Adapter`), `:63` (event helpers), `:207` (`Tracker::end_turn`)
- Modify: `src/adapter/claude.rs:29` (`ClaudeAdapter`), `:40` (`end`), `:62` (`handle`), `:88` (`observe`)
- Modify: `src/server.rs:56` (`IDLE_SETTLE`), `:197` (`settling`), `:226-290` (`run`, `observe_sessions`, `apply_settled`)
- Modify: `docs/design.md:451`, `docs/api.md:238`, `docs/api-v1.schema.json:251`
- Test: `tests/adapters.rs` (new test), `tests/service.rs:349` (`claude_session_file_drives_state`)

**Interfaces:**
- Consumes: `Event::End`, `tree::last_turn` (Task 1).
- Produces: `fn settle(&mut self, _session_id: &str, _at: Millis) -> Vec<Event>` on `Adapter`, defaulting to an empty vector.
- Produces: `pub(crate) fn end_turn_unsettled(&mut self, events: &mut Vec<Event>) -> Option<String>` on `Tracker`: ends the open turn with `Event::End` and returns its id, or returns `None` with no events when no turn is open.
- Produces: `struct Unsettled { turn: String, ended_at: Millis }` and `unsettled: HashMap<String, Unsettled>` (by session id) on `ClaudeAdapter`.

- [ ] **Step 1: Write the failing adapter test.** In `tests/adapters.rs`, add `use cusco::registry::{SessionState, Status};` and, after `interrupted_turn_is_cancelled`:

```rust
/// Claude's registry going idle closes the turn at once. How it ended is
/// settled afterwards: by the Stop hook, a new prompt, or silence.
#[test]
fn idle_closes_the_turn_before_its_outcome_is_known() {
    let session = "/claude/session:s1";
    let registry = |status| SessionState {
        session_id: "s1".into(),
        pid: 1,
        status,
        status_at: None,
        cwd: None,
        entrypoint: None,
    };
    let apply = |tree: &mut Tree, events: Vec<cusco::tree::Event>, at: Millis| {
        for event in events {
            tree.apply(event, at);
        }
    };
    // A turn open at 0, and the registry idle at one second.
    let interrupted = || {
        let mut adapter = ClaudeAdapter::new();
        let mut tree = Tree::new(&[adapter.client()], STALE_AFTER, 0);
        feed(&mut tree, &mut adapter, &("UserPromptSubmit", json!({})), 0);
        let events = adapter.observe(&registry(Status::Idle), 1_000);
        apply(&mut tree, events, 1_000);
        (adapter, tree)
    };
    let last_turn = |tree: &Tree| tree.attrs(session).unwrap().get("last_turn").cloned();

    let (mut adapter, mut tree) = interrupted();
    assert_eq!(tree.state(session), Some(State::Idle));
    assert_eq!(last_turn(&tree), None);
    let late = ("PostToolUse", json!({"tool_use_id": "tu1"}));
    feed(&mut tree, &mut adapter, &late, 1_100);
    assert_eq!(
        tree.state(session),
        Some(State::Idle),
        "a late tool hook can't reopen the turn"
    );
    feed(&mut tree, &mut adapter, &("Stop", json!({})), 1_200);
    assert_eq!(
        last_turn(&tree),
        Some(json!({"outcome": "ok", "ended_at": "1970-01-01T00:00:01.000Z"}))
    );

    let (mut adapter, mut tree) = interrupted();
    let next = ("UserPromptSubmit", json!({"turn_id": "p2"}));
    feed(&mut tree, &mut adapter, &next, 1_500);
    assert_eq!(last_outcome(&tree, session), "cancelled");
    assert!(tree.contains("/claude/session:s1/turn:p2"));

    let (mut adapter, mut tree) = interrupted();
    let events = adapter.settle("s1", 3_000);
    apply(&mut tree, events, 3_000);
    assert_eq!(last_outcome(&tree, session), "cancelled");

    // Busy again: the idle didn't hold, so nothing is recorded.
    let (mut adapter, mut tree) = interrupted();
    let events = adapter.observe(&registry(Status::Busy), 1_300);
    apply(&mut tree, events, 1_300);
    let events = adapter.settle("s1", 3_000);
    apply(&mut tree, events, 3_000);
    assert_eq!(tree.state(session), Some(State::Working));
    assert_eq!(last_turn(&tree), None);

    // A session that ends first gets no outcome afterwards.
    let (mut adapter, mut tree) = interrupted();
    feed(&mut tree, &mut adapter, &("SessionEnd", json!({})), 1_400);
    assert!(adapter.settle("s1", 3_000).is_empty());
}
```

- [ ] **Step 2: Run it.** `cargo test --test adapters idle_closes`. Expected: does not compile, "no method named `settle`".

- [ ] **Step 3: Implement `Tracker::end_turn_unsettled` and `Adapter::settle`** in `src/adapter/mod.rs`. `end_turn_unsettled` does `end_turn`'s bookkeeping (clear `open_turn`, drop that turn's main-thread requests, clear the main thread's calls) and emits `Event::End` where `end_turn` emits `Event::Close`.

- [ ] **Step 4: Implement the Claude adapter's rules** in `src/adapter/claude.rs`:

- `observe`, `Status::Busy` and `Status::Waiting`: remove the session's `unsettled` entry, then handle as today.
- `observe`, `Status::Idle`: call `end_turn_unsettled`; on `Some(turn)` insert `Unsettled { turn, ended_at: at }`.
- `handle`, after the `SessionEnd` return and before `handle_event`, when `record.agent_id` is `None` and the session is unsettled:

| `record.event` | Outcome | Then |
| --- | --- | --- |
| `Stop` | `Outcome::Ok` | continue into `handle_event` |
| `StopFailure` | `Outcome::Error` | continue into `handle_event` |
| `UserPromptSubmit`, `turn_id` differs from the unsettled turn (or is absent) | `Outcome::Cancelled` | continue into `handle_event` |
| `UserPromptSubmit`, same `turn_id` | none | continue into `handle_event` |
| anything else | none, entry kept | return the events gathered so far |

  In the first four rows remove the entry. Where there is an outcome, push `Event::Attr` on the session path with `{"last_turn": last_turn(outcome, ended_at)}` before `handle_event` runs.
- `settle`: remove the session's entry; if there was one, return one `Event::Attr` writing `last_turn` as `Outcome::Cancelled` with its `ended_at`.
- `end`: also remove the session's entry.

- [ ] **Step 5: Run it.** `cargo test --test adapters`. Expected: all pass.

- [ ] **Step 6: Update the service test.** In `claude_session_file_drives_state`, replace everything from `write("idle");` to the end of the function with:

```rust
    write("idle");
    let session = r#"{"v":1,"op":"get","path":"/claude/session:s1","depth":0}"#;
    let snapshot = wait_for_snapshot(dir.path(), session, |s| s["rows"][0]["state"] == "idle");
    assert!(
        snapshot["rows"][0]["attrs"].get("last_turn").is_none(),
        "idle is published before the outcome is known"
    );
    // The file says when the status changed, which can be before Cusco looked.
    assert_eq!(snapshot["rows"][0]["since"], "2026-11-18T11:06:40.000Z");

    // Another session's file changing makes Cusco read s1's idle again; the
    // outcome still arrives.
    let other = json!({"pid": std::os::unix::process::parent_id(), "sessionId": "s2",
                       "status": "idle"});
    std::fs::write(sessions.join("other.json"), other.to_string()).unwrap();
    wait_for_snapshot(dir.path(), session, |s| {
        s["rows"][0]["attrs"]["last_turn"]["outcome"] == "cancelled"
    });
}
```

- [ ] **Step 7: Run it.** `cargo test --test service claude_session_file`. Expected: FAIL, "timed out" waiting for `idle` without `last_turn` or for `cancelled`, because the core still holds the idle for two seconds and never calls `settle`.

- [ ] **Step 8: Implement the core change** in `src/server.rs`:

- `settling: HashMap<String, Millis>`: session id to when its idle settles.
- `observe_sessions`, per state: watch the pid; if `applied` already holds this status, skip it and leave `settling` alone; otherwise insert `at + IDLE_SETTLE` for `Status::Idle` or remove the entry for any other status, then call `apply_state`.
- `apply_settled(at)`: for each session that is due, remove it, call the Claude adapter's `settle(id, at)` and apply the events to the tree.
- `run`: the settle deadline is `settling.values().min()`.
- Rewrite `IDLE_SETTLE`'s comment: how long the outcome of a turn closed by the registry's `idle` may wait for a `Stop` hook before it is `cancelled`.

- [ ] **Step 9: Run it.** `cargo test`. Expected: all pass. Then `cargo clippy --all-targets` and `cargo fmt --check`.

- [ ] **Step 10: Update the documents.**

`docs/design.md:451`, replace the `idle` row with:

```markdown
| `idle` | Close a turn that is still open at once, with no outcome yet (`end`). For two seconds the outcome is unsettled: a `Stop` hook makes it `ok`, `StopFailure` makes it `error`, and a new prompt or two seconds of silence makes it `cancelled`. Until it settles no other main-thread hook reopens the turn; the registry going `busy` or `waiting` does. |
```

`docs/api.md:238`, the `last_turn` meaning becomes: "How the session's most recent turn ended. Missing until a turn ends. After a Claude Code interrupt it can follow the `idle` state by up to two seconds, while Cusco learns how the turn ended."

`docs/api-v1.schema.json:251`, the description becomes: "How the most recent turn ended. Missing until a turn ends; after an interrupt it can follow the idle state by up to two seconds."

- [ ] **Step 11: Commit.**

```bash
git add src/adapter/mod.rs src/adapter/claude.rs src/server.rs tests/adapters.rs tests/service.rs docs/design.md docs/api.md docs/api-v1.schema.json
git commit -m "feat: show a Claude session idle as soon as its registry says so"
```

### Task 3: Hooks without a shell

**Files:**
- Modify: `src/setup.rs:10` (`Target`), `:105` (`apply`), `:178` (`hook_entry`), `:184` (`update_cusco_hook`), `:233` (`is_cusco_hook`)
- Modify: `src/main.rs:5` (imports), `:43` (the `match`), `:86-88` (the `setup` loop)
- Modify: `docs/design.md:158`
- Test: `tests/service.rs:165` (`command_line`), `tests/service.rs:274` (`setup_adds_and_removes_hooks`)

**Interfaces:**
- Produces: `pub exec: bool` on `Target`: `true` for `claude` and `grok`, `false` for `codex` and `muse`.
- Produces: `pub fn hook_entry(executable: &Path, client: &str, exec: bool) -> Value`.
- Produces: `pub fn apply(target: &Target, executable: &Path, remove: bool) -> io::Result<Outcome>` (the second parameter was the command string).

- [ ] **Step 1: Write the failing tests.**

In `command_line`, after the existing "never fails and never prints" assert:

```rust
    // A client that ignored `args` would run bare `cusco`. That must not fail
    // a hook: exit code 2 blocks a tool call.
    let output = run_cusco(&[], dir.path(), payload);
    assert!(output.status.success() && output.stdout.is_empty() && output.stderr.is_empty());
```

In `setup_adds_and_removes_hooks`:

- In `original`, give the `other-tool` hook an `"args": ["hook", "claude"]` member. It is not Cusco's and must come through untouched; the existing `Stop` length assert and the final comparison with `original` prove it.
- After `group` is defined, seed Grok with an entry in the old form:

```rust
    std::fs::create_dir_all(grok.join("hooks")).unwrap();
    let old = json!({"hooks": {"Stop": [group("/old/path/cusco hook grok")]}});
    std::fs::write(grok.join("hooks/cusco.json"), old.to_string()).unwrap();
```

- Replace the `PermissionRequest` assert (line 314) with:

```rust
    let entry = |value: &serde_json::Value| value["hooks"][0].clone();
    let claude_entry = entry(&parsed["hooks"]["PermissionRequest"][0]);
    assert!(claude_entry["command"].as_str().unwrap().ends_with("/cusco"));
    assert_eq!(claude_entry["args"], json!(["hook", "claude"]));
```

- After the Codex asserts, add:

```rust
    assert!(entry(&stop[1]).get("args").is_none(), "Codex has no shell-free form");
    let grok_stop = read(&grok.join("hooks/cusco.json"))["hooks"]["Stop"].clone();
    assert_eq!(grok_stop.as_array().unwrap().len(), 1, "replaced where it sat");
    assert_eq!(entry(&grok_stop[0])["args"], json!(["hook", "grok"]));
    assert!(!command(&grok_stop[0]).starts_with("/old"));
    let muse_stop = read(&config.join("muse/settings.json"))["hooks"]["Stop"].clone();
    assert!(entry(&muse_stop[0]).get("args").is_none());
    // Without a shell the path is stored as it is; with one it is quoted.
    let spaced = std::path::Path::new("/a b/cusco");
    assert_eq!(
        cusco::setup::hook_entry(spaced, "claude", true)["command"],
        "/a b/cusco"
    );
    assert_eq!(
        cusco::setup::hook_entry(spaced, "codex", false)["command"],
        "'/a b/cusco' hook codex"
    );
```

- [ ] **Step 2: Run them.** `cargo test --test service -- command_line setup_adds`. Expected: does not compile, "function `hook_entry` is private".

- [ ] **Step 3: Add the guard** in `src/main.rs`. Before `_ => usage()`:

```rust
        None if !std::io::stdin().is_terminal() => ExitCode::SUCCESS,
```

with `IsTerminal` added to the `std::io` import. It prints nothing and does not read its input.

- [ ] **Step 4: Implement the hook forms** in `src/setup.rs`:

- `hook_entry` with `exec` returns, in this key order, `{"type": "command", "command": <the path, never quoted>, "args": ["hook", client], "timeout": 2}`. Without `exec` it returns today's entry, whose `command` is `hook_command(executable, client)`.
- `apply` builds the entry from `target.exec`; `update_cusco_hook` takes the entry in place of the command string. `main.rs` passes `&executable` and no longer calls `hook_command`.
- `is_cusco_hook` also accepts the no-shell form: `command`'s file name is `cusco` and `args[0]` is `"hook"`. The shell-form check is unchanged, so an old entry is still found and rewritten where it sits.

- [ ] **Step 5: Run them.** `cargo test`. Expected: all pass. Then `cargo clippy --all-targets` and `cargo fmt --check`.

- [ ] **Step 6: Update `docs/design.md:158`.** After the first sentence of the Capture paragraph add: "Where a client can run a command with no shell (Claude Code and Grok Build, through `args`), `cusco setup` writes that form; Codex and Muse Code run the command through a shell. Run with no arguments and piped input, `cusco` exits 0 silently, so a client that ignores `args` can't fail a hook."

- [ ] **Step 7: Commit.**

```bash
git add src/setup.rs src/main.rs tests/service.rs docs/design.md
git commit -m "feat: run Claude and Grok hooks without a shell"
```

## Final check

Build the release binary and rerun the benchmark that produced the spec's table. The benchmark is a script kept outside the repo: it starts a throwaway service in its own directory, subscribes, fires each kind of event 100 times and times it to the subscriber's line.

Expected: `registry: busy -> idle` under 5 ms where it was 2,004 ms; `exec form (no shell): event -> subscriber` still about 2 ms.

Result, 18 November 2026: `registry: busy -> idle` 0.15 ms median; `exec form (no shell): event -> subscriber` 2.2 ms median.
