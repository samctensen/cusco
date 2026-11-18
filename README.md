# Cusco

Cusco is a small background service that watches the coding agents running on
your Mac and answers one question for any program that asks: **which of my
agents needs me?**

You keep using Claude Code and Codex the way you do today. Their hooks report
lifecycle events to Cusco, which keeps one live picture of every session, turn,
subagent and pending approval. Any program can read that picture or subscribe
to it over a local socket: the `cusco` command line, a menu bar app, keyboard
lights, a Stream Deck plugin. Cusco defines the data; each consumer decides
how to show it.

The name comes from the Inca capital, the hub of a road network along which
relay runners carried messages. Here the hooks are the runners.

> **Status:** v0.1, macOS only. Supports Claude Code and Codex.

## How it works

Cusco keeps a tree of what it has observed:

```
/                                  machine
└─ /claude                         client
   └─ /claude/session:7f3a         session
      └─ …/turn:12                 turn
         └─ …/agent:a2             subagent
            └─ …/request:t9        approval, question or MCP input request
```

Every node gets a state derived from everything beneath it, checked in this
order:

| State | Meaning |
| --- | --- |
| `needs_input` | Something beneath is waiting on you. |
| `working` | An agent is doing work. |
| `unknown` | Cusco can't currently confirm what's happening. |
| `idle` | None of the above. |

So "is Claude busy?", "is this session waiting?" and "is that subagent
blocked?" are the same question asked at different depths. Elapsed time never
makes anything `idle`; silence only makes it `unknown`.

See [docs/design.md](docs/design.md) for the full design.

## Build and run

Requires Rust (edition 2024).

```sh
cargo build --release
./target/release/cusco serve
```

`cusco serve` runs in the foreground and listens in
`~/Library/Application Support/cusco/` (or `$CUSCO_DIR`).

## Connect your agents

Each client's hooks run `cusco hook <client>` once per event. The command
forwards a short list of identifiers and exits immediately. It never writes to
stdout, never fails, and never waits for Cusco, so your agents aren't slowed
down or blocked if Cusco isn't running.

Every hook entry has this shape, with the absolute path to your build:

```json
{ "hooks": [{ "type": "command", "command": "/path/to/cusco hook claude", "timeout": 2 }] }
```

**Claude Code** (`~/.claude/settings.json`, under `"hooks"`): add the entry,
with `hook claude`, to each of these events, keeping any handlers already
there: `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PermissionRequest`,
`PermissionDenied`, `PostToolUse`, `PostToolUseFailure`, `Elicitation`,
`ElicitationResult`, `Notification`, `SubagentStart`, `SubagentStop`, `Stop`,
`StopFailure`, `SessionEnd`.

**Codex** (`~/.codex/hooks.json`, under `"hooks"`): add the entry, with
`hook codex`, to each of these events: `SessionStart`, `UserPromptSubmit`,
`PreToolUse`, `PermissionRequest`, `PostToolUse`, `SubagentStart`,
`SubagentStop`, `Stop`, `Interrupt`, `SessionEnd`. Then review and trust the
hooks with `/hooks` in Codex; Codex skips hooks until they're trusted.

Start a new agent session after adding hooks.

## Commands

```sh
cusco status                       # one snapshot of the machine and each client
cusco status --path /claude --depth 2
cusco status --requests            # everything currently waiting on you
cusco watch                        # a snapshot, then every change as it happens
```

Output is one JSON object per line. `status` exits 1 if Cusco isn't running.

## The API

Any program can talk to Cusco directly. Connect to `api.sock` in the Cusco
directory and send one JSON line:

```jsonl
{"v":1,"op":"subscribe","path":"/","depth":1}
```

`get` returns one snapshot and closes; `subscribe` returns a snapshot and then
streams `upsert` and `remove` messages, each with an increasing `rev`. Add
`"view":"requests"` for a flat list of open requests. Every row summarizes its
whole subtree:

```json
{"id":"/claude","kind":"client","state":"needs_input","since":"2026-11-18T20:41:07.000Z",
 "reason":"approval","cause":"/claude/session:7f3a/turn:12/agent:a2/request:t9",
 "counts":{"sessions":4,"needs_input":1,"working":2,"unknown":0,"idle":1},
 "attrs":{"observes":["session","turn","agent","request"]}}
```

`cause` points at the node responsible for the state, so one more `get` tells
you which session and which subagent is waiting, and why. A subscriber that
falls behind is disconnected and should resubscribe for a fresh snapshot.

## Privacy

Prompts, responses, commands and tool arguments never leave the hook command.
Cusco receives identifiers, event names, tool names, the working directory and
the transcript path. Tool input is reduced to a hash, used only to match an
approval with the tool call it belongs to. Everything stays on your machine,
in a directory only you can read.

## Development

```sh
cargo test
cargo clippy --all-targets
cargo fmt --check
```

To see exactly what your agents send, set `CUSCO_RECORD=/path/to/file.jsonl` in
the environment they run in; the hook command appends each record (allowed
fields only) to that file.
