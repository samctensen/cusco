# Five agents, one model: what the research changes

**18 November 2026.** Cross-reading of the five briefings in this folder:
[anthropic](anthropic.md), [openai](openai.md), [grok](grok.md), [muse](muse.md) and
[google](google.md). It compares what each client exposes, lists where Eywa's design
(`docs/design.md`) holds up and where it doesn't, and proposes the v0.2 changes.

## At a glance

| | Claude Code | Codex | Grok Build | Muse Code | Antigravity |
| --- | --- | --- | --- | --- | --- |
| Hook file | `~/.claude/settings.json` | `~/.codex/hooks.json` | `~/.grok/hooks/*.json` (also reads Claude's) | `~/.config/muse/settings.json` | `~/.gemini/config/hooks.json` (named entries) |
| Trust step | Folder trust | Positional trust in `config.toml` | Folder trust | None | None documented |
| Turn id | `prompt_id` (optional) | `turn_id` (child's own turn inside subagents) | `promptId`, missing on tool events | `turn_id` on almost every event | None |
| Subagent id | `agent_id` on every event inside it | `agent_id` on every event inside it | `subagentId` = child `sessionId` | `MUSE_THREAD_ID` env var | Separate conversation, no parent link |
| Approval request | `PermissionRequest`, no tool id | `PermissionRequest`, no tool id | Notification only, no ids | `PermissionRequest`, no tool id | Status line only (CLI); none in App/IDE |
| Approval answered | `PostToolUse`/`PostToolBatch`; denial silent | `PostToolUse` on success only; denial silent | `stop_cancelled` on rejection | `ToolUseStart`; denial silent | Status line snapshot |
| Interrupt | None (inferred) | `Interrupt` | `stop_cancelled` | None (inferred) | None (inferred) |
| Questions | `AskUserQuestion` tool | `request_user_input` tool (verify) | `ask_user_question` tool | Ask tool (name to verify) | `ask_question` tool |
| MCP input | `Elicitation` hooks | Not observable | Not observable | Not observable | Not observable |
| Process model | One per session (CLI and Desktop) | Shared `codex app-server` daemon | TUI per session; leader/service shared | Launcher → `muse-bin-<version>`; `serve`/`acp` shared | `agy` per session; App and daemon shared |
| Live state outside hooks | `~/.claude/sessions/<pid>.json` (`status`, `waitingFor`) | `state_5.sqlite`, `thread_history_1.sqlite` | Sessions dir (unobserved) | `muse sessions --json --watch` | CLI status line snapshots |

## What holds up

- **The tree.** Session → turn → agent → request, with state derived from what's open
  beneath, fits all five. Clients that report less produce shallower trees, as designed.
- **Silence becomes `unknown`, never `idle`.** Every client has gaps where this matters.
- **Silent, always-successful hook command.** All five read hook stdout or exit codes in
  some events (Muse adds stdout to the model's context; Grok and Claude feed exit 2's
  stderr to the model). The hook must never print and always exit 0.
- **Digest matching for approvals.** No client puts a tool call id on its approval
  request, so matching the request to its `PreToolUse` by input digest is the general
  approach, not a Claude quirk.

## What has to change

### 1. Hook records are per client, not one Claude-shaped schema
Field names, casing and sources differ: Grok is camelCase with `hookEventName`;
Antigravity nests `toolCall.id`; Muse puts thread identity in environment variables;
Claude offers `CLAUDE_PID` and `CLAUDE_CODE_ENTRYPOINT`. The hook command needs a parser
per client that maps that client's payload *and environment* into the record, plus
per-client digest rules (Codex: drop `description` before hashing). The allowed-field
lists in the briefings become the parsers' specifications.

Grok runs `~/.claude/settings.json` hooks by default, so `eywa hook claude` receives
Grok payloads. The hook command detects `GROK_HOOK_EVENT`, relabels the record as
`grok`, and the adapter drops the duplicate by the envelope's `timestamp`.

### 2. "Never reopen" is wrong; per-thread order is the real guarantee
The design assumed hook records arrive out of order, so a closed turn or stopped agent
never reopens. The research contradicts both halves:
- **Order:** synchronous hooks block the agent, so one thread's events arrive in order
  (Codex source, Muse docs). Only different threads interleave.
- **Legitimate reopening:** a blocked `Stop` continues the same turn (Claude, Muse);
  `SubagentStart` repeats when a subagent resumes (Claude); `SubagentStop` fires per
  child turn, and the same agent runs again (Codex).

New rule: activity carrying a closed node's own id reopens it. Ended sessions keep the
current rule (only a new start or prompt reopens them). Grok's `stop_cancelled`, which
can arrive after the next prompt, is handled in its adapter.

### 3. Subagents attach to the root turn that spawned them
Codex reports a subagent's *own* `turn_id`, and the current adapter creates a phantom
root turn that never closes. Grok subagents are child sessions nested up to five deep.
Rule: a subagent's own turn ids never create root turns; agents attach to the spawning
root turn, using whatever the client provides (`agent_id`, `subagentId` with a
child-to-root map, `MUSE_THREAD_ID`).

### 4. `observes` becomes a capability record
A flat list can't say "approvals visible per actor on the CLI, only questions in the
App". Proposed shape, on the client row and on session rows when the surface differs:

```json
"observes": {"agents": true, "approvals": "actor", "questions": true, "mcp_input": false, "interrupts": "inferred"}
```

`approvals` is `actor`, `turn` or `none`; `interrupts` is `hook` or `inferred`.

### 5. Processes: name patterns, and hosts versus sessions
- **Names:** one name per client fails: Muse runs as `muse-bin-<version>`, Antigravity as
  `agy`, `Antigravity` or `language_server_macos_*`, Gemini CLI as `node`. Each client
  gets a list of executable patterns (exact, case-sensitive, or prefix).
- **Hosts:** Codex's app-server, Muse `serve`/`acp`, Grok's leader and background
  service, and Antigravity's App and daemon each run many sessions in one process. A
  host's exit closes all its sessions, but a running host is not evidence of an
  unreported session. So it gets no `process` placeholder, which today keeps Codex
  `unknown` forever after an Eywa restart. Process placeholders apply only to clients
  that run one session per process (Claude, Antigravity CLI, Grok TUI).
- **Pid:** use `CLAUDE_PID` where offered; keep the parent walk as the fallback.

### 6. State sources beside hooks
Every client but Grok has a place that states its current status directly. That fixes
the gaps hooks can't: interrupts (Claude, Muse, Antigravity), granted approvals on
long-running tools (Claude, Codex), and `unknown` after a restart.

| Client | Source | Gives |
| --- | --- | --- |
| Claude | `~/.claude/sessions/<pid>.json`, watched with kqueue | `busy`/`waiting`/`idle` per session, `waitingFor` |
| Codex | `thread_history_1.sqlite`, rollouts | Open turns and subagents (not pending approvals) |
| Muse | Session registry, `session.jsonl` | Turn ends, pending approvals |
| Antigravity | CLI status line wrapper | `agent_state`, pending confirmation, subagents |

These feed the existing `sync` event. The Claude registry is the cheapest and most
valuable: one directory, structured JSON, written on every status change.

### 7. Approval requests don't always mean a person is being asked
Codex's automatic reviewer and Muse's model reviewer decide some approvals after
`PermissionRequest` fires. Where a "prompt shown" notification exists (Claude and Muse
after about 6 seconds, Grok as its only signal), it confirms a person is being asked.

### 8. Setup per client, and positional trust
- Codex keys hook trust by position (file, event, group index, handler index). `eywa
  setup` currently removes its entry and appends a new one, which shifts any group
  added after it and silently untrusts another tool's hook. It must update its entry in
  place and only ever append new ones.
- Grok gets its own file (`$GROK_HOME/hooks/eywa.json`), plus Grok.app's separate
  agent home. Antigravity gets a named `"eywa"` entry. Muse needs exact JSON, because
  one wrong-typed field disables every hook in the file.
- `eywa doctor` checks trust (Codex positional, Claude and Grok folder trust) and
  reports hooks that are installed but silent.

### Smaller corrections already identified
Claude: handle `PostToolBatch` and `TeammateIdle`; elicitations without an id;
`UserPromptSubmit.source` marks machine-started turns (and answers "finished,
unseen"); surface from `CLAUDE_CODE_ENTRYPOINT`. Codex: there's no `StopFailure`;
`SessionStart` fires only for the root, lazily; install `PreToolUse`. Both adapters'
items are listed in section 7 of their briefings.

## Honest gaps
Some states can't be observed by any means found:
- MCP input requests in Codex, Grok, Muse and Antigravity.
- Approvals in the Antigravity App and IDE.
- Which approvals a person (not a reviewer) will see in Codex.

Each client's `observes` record should say so, rather than Eywa showing `idle` where
it can't see.

## Proposed v0.2

1. **Capture:** per-client parsers (payload and environment), relabeling Grok's Claude
   hooks, per-client digest rules.
2. **Model:** reopen by own id, root-turn attachment for subagents, the capability
   record for `observes`.
3. **Processes:** executable patterns per client; host versus session processes;
   `CLAUDE_PID`.
4. **Fix the Claude and Codex adapters** from their briefings.
5. **Add Grok, Muse and Antigravity adapters**, with `eywa setup` writers for each and
   the Codex positional fix.
6. **First state source:** the Claude session registry, as a `sync` feed.

Later: Codex and Muse state databases, the Antigravity status line, Gemini CLI.

## Decisions needed
1. **Reviewer-decided approvals:** show `needs_input` as soon as `PermissionRequest`
   fires (fast, occasionally a false alarm), or wait for a "prompt shown" notification
   where one exists (accurate, about 6 seconds late, unavailable for Codex)?
   Recommendation: immediately, with a `prompted` attribute once confirmed.
2. **Order of the new connectors:** recommendation is Grok and Muse with v0.2, since
   both have rich hooks and are installed here; Antigravity after a current install is
   available to verify against, since nothing current is installed on this Mac.
3. **Live verification:** each briefing lists 14 to 19 items only a recorded session
   can settle. That needs hooks installed on this Mac and real sessions recorded with
   `EYWA_RECORD`.
