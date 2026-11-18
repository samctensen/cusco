# Cusco

**Design · Draft v0.1 · 18 November 2026**

Cusco is a per-user background service that watches the coding agents running on
your machine and answers one question for any program that asks: *which of my
agents needs me?*

You keep launching Claude Code and Codex the way you do today. Their hooks report
lifecycle events to Cusco. Cusco keeps one live picture of everything that is
running and publishes it over a local socket. Consumers such as the `cusco`
command line, keyboard or desk lights, a menu bar app or a Stream Deck plugin
subscribe to that picture and decide how to show it. Cusco defines the data and
the API. It has no UI and knows nothing about any consumer.

The name comes from the Inca capital. Cusco sat at the center of a road network
of roughly 40,000 km, and relay runners called chasquis carried messages along it
to the capital. Here the hooks are the chasquis and the service is the capital.

## First demonstration

1. Claude Code and Codex run at the same time.
2. Claude's main turn finishes while one of its subagents waits for tool approval.
3. Start `cusco watch --json`. Its first message shows `claude` as `needs_input`,
   with `cause` pointing at the subagent's request, and `codex` as `working`.
4. Approve the subagent and let it finish. Only the `claude` row changes, to
   `idle`.

## Vocabulary

| Term | Meaning |
| --- | --- |
| client | A program that runs agents: `claude`, `codex`. |
| session | One conversation in a client. |
| turn | One prompt and the work done in response to it. |
| agent | A subagent started inside a turn. It can outlive the turn. |
| request | Something the agent is waiting on you for: a tool approval, a question or an MCP input request. |
| hook command | `cusco hook <client>`, which each client's hooks run once per event. |
| adapter | Client-specific code inside Cusco that turns hook records into Cusco events. It keeps per-session state, such as which tool call an approval belongs to. |
| consumer | Any program that reads from Cusco's API. |

## Data model

### One tree of nodes

Cusco stores what it has observed, never conclusions. It does not store "Claude
is working." It stores "turn 12 in session 7f3a is open" and "subagent a2 has an
open approval request." Every higher-level answer is computed from that.

```
/                                  machine
└─ /claude                         client
   ├─ /claude/process:4321         running process not yet linked to a session
   └─ /claude/session:7f3a         session (attrs: cwd, pid, surface)
      └─ …/turn:12                 turn
         └─ …/agent:a2             subagent
            └─ …/request:t9        approval, question or MCP input request
```

Each node has the same shape:

```json
{
  "id": "/claude/session:7f3a/turn:12/agent:a2/request:t9",
  "kind": "request",
  "open": true,
  "confirmed": true,
  "opened_at": "2026-11-18T20:41:07Z",
  "closed_at": null,
  "outcome": null,
  "attrs": { "reason": "approval" }
}
```

The id is the node's path, so the parent is implied. `outcome` is set on close:
`ok`, `error` or `cancelled`. `confirmed` is false when no source can currently
vouch for an open node.

Node kinds fall into three groups:

| Group | Kinds | An open node means |
| --- | --- | --- |
| container | client, process, session | It exists and is alive. Not that it is busy. |
| work | turn, agent (later: tool) | The agent has the ball. |
| attention | request | You have the ball. |

### Derived state

Every node, at every level, gets a state computed from its own subtree:

| State | Rule, checked in this order |
| --- | --- |
| `needs_input` | A confirmed open request exists in the subtree. |
| `working` | A confirmed open work node exists in the subtree. |
| `unknown` | An unconfirmed open node exists in the subtree. |
| `idle` | None of the above. |

One subagent's progress can't clear a sibling's approval, because each request is
its own node. A turn ending doesn't hide a subagent that keeps running, because
the subagent is still an open node beneath it. Elapsed time alone never produces
`idle`. It can only make an open node unconfirmed.

"Is Claude running?", "is this session busy?" and "is that subagent waiting?" are
the same question asked at different depths of the tree.

**Implementation.** When a node changes, Cusco recomputes the state of that node
and each of its ancestors from their children. Trees hold a few dozen nodes, so
this costs nothing and leaves no counters to drift. Cusco publishes a change only
for rows whose published fields actually changed, so detail deep in a session
doesn't create traffic at the client level.

### Closing rules

- Closing a node closes its open **requests**. A request can't outlive the actor
  that asked for it.
- Closing a turn does **not** close its subagents. Background subagents really do
  outlive their turn, and the first demonstration depends on that.
- Closing a session, or the session's process exiting, closes everything beneath it.

### Clients that report less

Clients differ in what they report. A client that only reports turn start and end
produces shallower trees, and the same rules still work. Each client row says what
its adapter can observe, so a consumer can tell "really idle" from "can't see any
deeper":

```json
{ "id": "/codex", "attrs": { "observes": ["session", "turn", "agent", "request:turn"] } }
```

`request:turn` means approvals are attributed to the turn, not to the subagent
that asked.

## Event flow

```
 Claude Code hooks ──┐
 Codex hooks ────────┴─► cusco hook <client>    keeps allowed fields, hashes tool input,
                                │               sends one datagram, exits
                                ▼  ingest.sock
                         adapter, per client    stateful: matches approvals to tool calls,
                                │               closes interrupted turns
 process table ─► liveness ─────┤
 transcripts (later) ─► sync ───┤  open · close · attr · lost · sync
                                ▼
                               core             tree → derived states → changed rows
                                │
                                ▼  api.sock
          cusco status / watch · status lights · menu bar (requests view)
```

### Capture

Each client's hook configuration runs `cusco hook <client>`. The hook command
reads the payload from stdin, keeps only the allowed fields, sends them as one
datagram to Cusco's ingest socket and exits. If Cusco isn't running, it exits
immediately with status 0. The agent never waits on Cusco.

The hook command does no mapping. Mapping needs memory across events, such as
which tool call an approval belongs to and whether the previous turn is still
open, so it runs in the adapter inside Cusco.

**Allowed fields:** event name, session id, turn or prompt id, agent id and type,
tool use id, tool name, notification type, session start source, elicitation id,
stop error code, background task ids, types and statuses, `cwd` and the
transcript path. Prompts, responses, commands and tool arguments never cross the
socket or touch disk.

**Tool input digest.** To match an approval with its tool call, the hook command
computes SHA-256 over the tool name and the tool input re-serialized with sorted
keys, and sends the first 16 bytes as hex. The input itself stays in the hook
command.

**Process id.** No payload includes one. The hook command walks up its parent
processes to the client's process and sends that id, so Cusco can tell when a
session's process exits. The same walk gives the surface (`cli`, `desktop` or
`ide`) on a best-effort basis.

### Normalized events

Adapters produce five event types. The core knows nothing about any specific
client.

| Event | Effect |
| --- | --- |
| `open {node}` | A node appeared. |
| `close {id, outcome}` | A node ended: `ok`, `error` or `cancelled`. |
| `attr {id, attrs}` | A node's attributes changed. |
| `lost {scope}` | The open nodes in `scope` become unconfirmed. |
| `sync {scope, nodes}` | Authoritative replacement of a subtree, used for recovery. Cusco diffs it against what it has and publishes the changes. |

Any event that mentions a node also confirms that node and its ancestors. An event
for a node Cusco hasn't seen opens it implicitly, so a session that started before
Cusco did is picked up on its next event.

## Adapters

Hook behavior below comes from the Claude Code hooks reference and the Codex hooks
documentation, read on 18 November 2026. Items marked **verify** aren't documented
and need a recorded session to confirm before the adapter relies on them.

### Claude Code

Every Claude payload carries `session_id`, `prompt_id` (the turn id) and `cwd`.
Events inside a subagent also carry `agent_id` and `agent_type`, so every tool
event and approval can be attributed to the subagent that caused it.

| Hook | Cusco event |
| --- | --- |
| `SessionStart` | `open` session. With source `clear`, close the session previously running in the same process. |
| `UserPromptSubmit` | `open` turn, keyed by `prompt_id`. Close any turn in the session that is still open as `cancelled`. |
| `PreToolUse` | Remember the call's `tool_use_id` against its input digest, per actor. If the tool is `AskUserQuestion`, `open` a request with reason `question`, keyed by `tool_use_id`. |
| `PermissionRequest` | `open` a request with reason `approval` under the asking actor, keyed by the `tool_use_id` whose digest matches. |
| `PostToolUse`, `PostToolUseFailure` | `close` the request with that `tool_use_id`, if one is open. |
| `PermissionDenied` | `close` the request with that `tool_use_id` as `cancelled`. This hook fires only for auto-mode denials. |
| `Elicitation` / `ElicitationResult` | `open` / `close` a request with reason `mcp_input`, keyed by `elicitation_id`. |
| `Notification` (`idle_prompt`) | Claude is waiting at the prompt. `close` any open turn as `cancelled`. |
| `SubagentStart` / `SubagentStop` | `open` / `close` agent, keyed by `agent_id`, under the current turn. |
| `Stop` | `close` turn as `ok`. Reconcile the session's open agents against `background_tasks`. |
| `StopFailure` | `close` turn as `error`, with the error code. |
| `SessionEnd` | `close` session. |

**Interrupts.** Claude has no interrupt hook, and `Stop` doesn't fire when you
interrupt a turn. An interrupted turn is closed by whichever comes first: the next
`UserPromptSubmit`, an `idle_prompt` notification, or `SessionEnd`.

**Answered approvals.** No hook fires when you grant or deny a prompt. A granted
approval closes when its tool's `PostToolUse` or `PostToolUseFailure` arrives,
matched by `tool_use_id`. A denied one closes on the same actor's next
`PreToolUse`, because a new tool call means the agent has moved on, or when the
actor's turn or subagent ends. **Verify** what a manual denial sends.

**Background work.** `UserPromptSubmit` also fires when a background subagent
reports back, so it doesn't always mean you typed. **Verify** whether
`SubagentStop` fires for background subagents, and whether `background_tasks` ids
match `agent_id`.

### Codex

Codex payloads carry `session_id`, `turn_id` and `cwd`. Subagent hooks reuse the
parent's `session_id`.

| Hook | Cusco event |
| --- | --- |
| `SessionStart` | `open` session. |
| `UserPromptSubmit` | `open` turn, keyed by `turn_id`. |
| `PermissionRequest` | `open` a request with reason `approval` under the turn. |
| `PreToolUse` | `close` the turn's open approval: a new tool call means it was answered. |
| `PostToolUse` | `close` the turn's open approval if its tool input digest matches. |
| `SubagentStart` / `SubagentStop` | `open` / `close` agent, keyed by `agent_id`, under the turn. |
| `Stop` | `close` turn as `ok`. |
| `Interrupt` | `close` turn as `cancelled`. Fires for the main thread only. |
| `SessionEnd` | `close` session. Fires on archive, delete, normal exit, or after 30 idle minutes with no connected client, so it isn't the same as the process exiting. |

**Attribution.** **Verify** whether tool events and approvals inside a Codex
subagent carry `agent_id`. Until they do, Codex approvals attach to the turn, and
the client row says `request:turn` in `observes`.

**Trust.** Codex skips hooks until you review and trust them in `/hooks`, and
trust is tied to the hook's hash. The installer must keep the hook command stable,
and `cusco doctor` must report "installed but not trusted" when a client's hooks
are present but silent.

## Local API v1

### Sockets

Cusco listens on two Unix sockets in `~/Library/Application Support/cusco/`. The
directory is readable only by the current user.

- `ingest.sock`: datagram socket for the hook command. One JSON record per datagram.
- `api.sock`: stream socket for consumers. One UTF-8 JSON object per line.

### Requests

Consumers select part of the tree by path and depth. Depth counts levels below
the path: `0` means only that node.

```jsonl
{"v":1,"op":"subscribe","path":"/","depth":1}
{"v":1,"op":"get","path":"/claude","depth":2}
{"v":1,"op":"get","view":"requests"}
```

| Operation | Behavior |
| --- | --- |
| `get` | Send one snapshot, then close the connection. Used by `cusco status` and scripts. |
| `subscribe` | Send a snapshot, then stream changes until the consumer disconnects. |

| View | Contents |
| --- | --- |
| `tree` (default) | Nodes under `path`, down to `depth`. |
| `requests` | Every open request across all clients, as a flat list. This is the direct answer to "which agents need me?" |

### Rows

Every row summarizes its entire subtree, whatever depth the consumer asked for.
Depth only limits which rows are sent.

```json
{
  "id": "/claude",
  "kind": "client",
  "state": "needs_input",
  "since": "2026-11-18T20:41:07Z",
  "reason": "approval",
  "cause": "/claude/session:7f3a/turn:12/agent:a2/request:t9",
  "counts": { "sessions": 4, "needs_input": 1, "working": 2, "unknown": 0, "idle": 1 },
  "attrs": { "observes": ["session", "turn", "agent", "request"] }
}
```

- `state` is the derived state. `since` is when it last changed.
- `cause` is the deepest node responsible for the current state. One `get` on it
  answers "which session, which subagent and why."
- `counts` appears on machine and client rows. It counts the open sessions beneath
  the row, in total and by state. `counts.sessions` answers "is it running at
  all?" and `state` answers "is it busy?"
- Session rows include `cwd`, `pid`, `surface` and
  `last_turn: { outcome, ended_at }`. Session titles are left out because clients
  generate them from prompts.

### Messages

```jsonl
{"v":1,"type":"snapshot","rev":41,"rows":[ … ]}
{"v":1,"type":"upsert","rev":44,"row":{ … }}
{"v":1,"type":"remove","rev":45,"id":"/claude/session:7f3a"}
{"v":1,"type":"error","code":"bad_request","message":"unknown view"}
```

- The snapshot's revision and the subscriber's registration happen in one step of
  the core, so no change falls between them.
- Revisions increase with every published change during one run of Cusco. A
  filtered subscriber sees gaps. After any reconnect, including after Cusco
  restarts, the new snapshot replaces the consumer's view entirely.
- Each subscriber has a bounded queue. If a subscriber falls behind, Cusco closes
  its connection and the subscriber resubscribes. Hooks and other subscribers are
  never held up.

## Recovery

Cusco holds no state across restarts in v0.1. On startup it lists the running
processes of each enabled client:

- **No process for a client:** that client is `idle`, with no sessions. A missing
  process is proof that nothing is running.
- **Each running process:** becomes an unconfirmed `process` node under its
  client, so the client shows `unknown`. The node is replaced by a session when an
  event arrives carrying that process id, and removed when the process exits.

`unknown` therefore means exactly "these processes haven't reported since Cusco
started." It clears on its own as sessions are used or closed.

During normal running:

- A session whose process exits is closed.
- An open turn, agent or request that has heard nothing for an hour while its
  process is alive becomes unconfirmed. Sessions with nothing open never go stale,
  because sitting at the prompt is genuinely idle.

Later, adapters can rebuild open nodes from client transcripts and send a `sync`.
Claude's payloads already include the transcript path. That would also let an idle
session that started before Cusco show `idle` instead of `unknown`. `sync` is the
single recovery mechanism: startup, missed hooks and restarts all go through it.
If persistence turns out to be needed, Cusco can append normalized events to a log
and replay them on start. The same log then doubles as test fixtures. It would
store events, never derived state.

## v0.1 scope

**In:** the tree, the five events, the core, the Claude Code and Codex adapters,
the process liveness check, `api.sock` with `get` and `subscribe`, the `tree` and
`requests` views, `cusco status --json` and `cusco watch --json`.

Cusco blocks on its sockets and wakes only for real work: an incoming event, a
subscriber, a process exit, or a staleness deadline. It runs no polling loop.

**Acceptance:**

1. The first demonstration passes.
2. With agents active, disconnect a subscriber, trigger an approval and
   reconnect. The first snapshot shows the waiting client.
3. Restart Cusco during an approval. The client shows `unknown`, never `idle`,
   until that session's next event. A false `idle` fails.
4. Interrupt a Claude turn. The turn closes as `cancelled` no later than the next
   prompt or `idle_prompt` notification.
5. Run two tools in parallel in one Claude turn, one needing approval. The other
   tool finishing does not clear the approval.
6. Recorded hook sequences replay as tests without a live agent.

**Next:** transcript `sync` for Claude, `cusco doctor`, a launchd agent and
installer, Grok Build and Muse Code adapters, then measured latency and
idle-overhead targets.

**Later:** OpenCode, Cursor, Antigravity CLI, tool nodes, a `group_by: cwd` view
("which repo is blocked?"), Linux.

## Open questions

1. **"Finished, unseen."** An agent needs you when it's blocked mid-turn and also
   when it has finished and is waiting for your next prompt. v0.1 only publishes
   `last_turn`. A consumer `ack` operation would make "finished and not yet seen"
   a real state. `UserPromptSubmit` can't serve as the acknowledgment, because
   background subagents also trigger it. Is `ack` worth adding to the core, or
   should consumers decide by time?
2. **Tool nodes.** `PreToolUse` and `PostToolUse` would give dashboards per-tool
   detail, at the cost of much more event traffic. Worth it, and for which views?
3. **Claude Desktop.** One app process can host many sessions. How does the hook
   command's parent-process walk look there, and does process-level liveness still
   tell sessions apart?
4. **Codex as the neutrality test.** If the Codex adapter needs a sixth event
   type, the vocabulary is shaped too closely around Claude. Revisit the five
   events after the Codex adapter lands.
