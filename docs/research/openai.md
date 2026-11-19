# OpenAI Codex: research briefing for Cusco

**18 November 2026.** Covers the Codex CLI, the desktop app (shipped on this Mac as
`ChatGPT.app`), the IDE extension and the shared app-server daemon. Verified against
the hooks documentation, the `openai/codex` source at tag `rust-v0.170.0` (the
installed version) and a read-only look at `~/.codex`.

Each claim is tagged with its basis: **(docs)**, **(source code)**, **(local install)**
or **(inference)**.

---

## 1. Product and surfaces

| Surface | What runs locally | Version verified | Runs `~/.codex/hooks.json`? |
| --- | --- | --- | --- |
| `codex` CLI (TUI) | `~/.local/bin/codex` → `~/.codex/packages/standalone/current/bin/codex` → `releases/0.170.0-aarch64-apple-darwin/bin/codex` | `codex-cli 0.170.0` (local install) | Yes. Hooks run inside codex-core wherever the session is hosted (source code). |
| `codex exec` | Same binary, non-interactive | 0.170.0 (local install) | Yes. The source tree has `codex exec` hook tests in `codex-rs/exec/tests/suite/hooks.rs` (source code). |
| App-server daemon | Supervisor `~/.codex/packages/app-server-daemon/releases/<ver>/bin/codex app-server daemon pid-update-loop` (parent: launchd, pid 1) plus child `codex app-server --listen unix:// … --managed-daemon` | 0.170.0 running; 0.164.0 also installed (local install) | Yes. It hosts the sessions, so their hooks run as its children (source code, inference). |
| Desktop app | `/Applications/ChatGPT.app`, bundle id `com.openai.codex`, version 26.1111.21308, Electron. Bundles `Contents/Resources/codex-cli/bin/{codex,codex-exec-server,codex-code-mode-host}` at `0.169.0-alpha.11.1` | (local install) | Yes, through the app-server. The docs describe "Review hooks" in the composer and Settings › Hooks (docs). |
| IDE extension | Rollouts on this Mac record originator/source `vscode` (local install) | not verified | Yes if it hosts codex-core, which is the same engine (inference). |
| ChatGPT for Chrome | `~/.codex/plugins/cache/openai-bundled/chrome/latest/extension-host/…` native host (local install) | n/a | Out of scope: a browser plugin host, not a session host. |

**How they fit together.**

- On this Mac, the "Codex desktop app" and the ChatGPT desktop app are the same
  bundle. `ChatGPT.app` has bundle id `com.openai.codex` and ships a Codex framework,
  a Codex dock-tile plugin and a bundled `codex-cli` (local install). Its local coding
  agent is Codex, and it has no separate hook system. The ChatGPT chat features, such
  as connectors and agent mode, expose no local hook or API that I could find.
  **Out of scope** except through Codex (inference).
- Since 0.170 the interactive TUI connects to the shared app-server daemon by default.
  `codex-rs/tui/src/daemon_startup.rs` says: "automatic launches require a compatible
  shared server and a successful connection". `--no-daemon` forces embedded mode, and
  the TUI has an `AppServerTarget::Embedded` path for that (source code). So a CLI
  session usually runs inside the daemon's `codex app-server` process, not inside the
  `codex` TUI process (inference from source; **verify** with `ps` during a TUI
  session).
- The daemon is supervised by `codex app-server daemon pid-update-loop`, which launchd
  owns. It outlives the desktop app. At inspection time the app wasn't running but the
  daemon was (local install).
  - Its control socket is `~/.codex/app-server-control/app-server-control.sock`, a
    symlink to `/private/tmp/codex-daemon-<uid>/<hash>`.
  - State lives in `~/.codex/app-server-daemon/{daemon.pid, settings.json, daemon.stderr.log}`.
  - `daemon.pid` records pid, process start time, boot id and executable digests.
  - (local install)
- `codex app-server` speaks a JSON-RPC-style protocol over stdio, WebSocket or a Unix
  socket. It has thread, turn and item notifications and server-to-client approval
  requests (source code: `app-server-protocol/src/protocol/common.rs`). See §2.

---

## 2. Extension mechanism

### Where hooks are configured (docs)

| Layer | Location |
| --- | --- |
| System | `/etc/codex/hooks.json` |
| User | `$CODEX_HOME/hooks.json`, default `~/.codex/hooks.json` |
| Inline | A `[hooks]` table in any layer's `config.toml` |
| Project | `.codex/hooks.json`, loaded only from trusted project layers |
| Plugin | `hooks/hooks.json` in the plugin root, or the plugin manifest. Commands get `PLUGIN_ROOT`, `PLUGIN_DATA`, `CLAUDE_PLUGIN_ROOT` and `CLAUDE_PLUGIN_DATA`. |
| Managed | `requirements.toml`. `allow_managed_hooks_only = true` ignores user, project, session and plugin hooks. |
| Kill switch | `[features] hooks = false`. `codex_hooks` is a deprecated alias. |

Matching hooks from all layers run (docs).

**Format.** event → matcher groups → handlers (docs). Handler types are `command` and
`mcp_tool`; `prompt` and `agent` are parsed but skipped. Handler options are
`timeout`, `statusMessage`, `async`, `additionalContextLimit` and `commandWindows`
(docs).

The matcher is a regex, and `*`, empty or absent matches everything. These events
ignore it: `UserPromptSubmit`, `Stop`, `Interrupt`, `PreModelRequest` and
`PostModelRequest`. Matcher subjects by event (docs):

- `SessionStart`: the source.
- `SessionEnd`: `other`.
- `PreCompact` and `PostCompact`: `manual` or `auto`.
- Tool events: the tool name. `apply_patch` also matches `Edit` and `Write`.
- `SubagentStart` and `SubagentStop`: the agent type.

The user `hooks.json` on this Mac belongs to another local tool. It registers eight
events with `timeout: 2`: `UserPromptSubmit`, `PermissionRequest`, `PostToolUse`,
`Stop`, `SessionEnd`, `Interrupt`, `SubagentStart` and `SubagentStop`. It doesn't
register `PreToolUse` or `SessionStart`. Cusco isn't installed yet (local install).

### How a command hook runs (source code: `hooks/src/engine/command_runner.rs`)

- Codex spawns `$SHELL -lc "<command>"`, falling back to `/bin/sh`. That is a
  **login** shell, so `.zprofile` and `.zlogin` run on every hook (source code). A
  configured shell program uses its own arguments instead.
- The environment starts empty (`env_clear`). Codex then replays the session's
  shell-environment snapshot and adds the hook-specific variables, minus restricted
  names (source code). I found no environment variable that identifies the session or
  thread (source code; docs list only the plugin variables).
- The command gets its own process session (`ProcessMode::NewSession`, i.e. `setsid`).
  Its parent pid is still the Codex process that hosts the session (source code).
- The working directory is the session's `cwd`. One JSON object is written to stdin,
  and the timeout covers the stdin write. On timeout Codex kills the process group
  (source code).
- Environment hooks, which run in a remote or exec-server environment, go through an
  `environment_executor` and not a local spawn (source code). That isn't relevant to
  local user hooks.

**Timeouts.** The default is 600 s. `SessionEnd` and `Interrupt` default to 1 s, with
a 3 s maximum (docs).

**Exit codes and stdout** (docs):

- Exit 0 with no output means success.
- Exit 2 blocks, with the reason read from stderr, on `PreToolUse`, `PostToolUse`
  (as feedback), `UserPromptSubmit`, `Stop` and `SubagentStop` (as continuation).
- Plain stdout becomes developer context on `SessionStart`, `UserPromptSubmit` and
  `SubagentStart`. It's ignored on the tool and compact events.
- `Stop`, `SubagentStop` and `Interrupt` require JSON on stdout. Plain text is
  invalid there.
- `PermissionRequest` decides through `hookSpecificOutput.decision.behavior: "allow"`
  or `"deny"` with an optional `message`. Any deny wins. Returning `updatedInput`,
  `updatedPermissions` or `interrupt` fails closed (docs; source code:
  `events/permission_request.rs` "any deny wins").
- **For Cusco:** exit 0 with empty stdout is a no-op on every event.

**Concurrency.**

- All matching command handlers for one event start concurrently (docs; source code:
  `dispatcher.rs` uses `FuturesUnordered`).
- Codex awaits synchronous hooks before it continues (source code). Handlers with
  `async: true` run in the background instead, at most 8 per session
  (`MAX_CONCURRENT_ASYNC_HOOKS = 8`), and can't block anything (docs, source code).
- **Consequence:** with synchronous hooks, events from one thread arrive in step
  order, as long as the hook command delivers its datagram before it exits. Records
  can interleave only across threads (parent and subagents run concurrently) and
  across parallel tool calls in one thread (inference from source). Cusco should
  **not** set `async`.

### Trust

- Non-managed hooks are skipped until reviewed, through `/hooks` in the CLI or Settings
  › Hooks in the app. `--dangerously-bypass-hook-trust` skips the check for one
  invocation, and managed hooks are trusted by policy (docs).
- Trust is stored in `config.toml` (local install; source code: `discovery.rs`
  `hook_key`, `hook_hash`):
  ```toml
  [hooks.state."<source path>:<event_snake_case>:<group_index>:<handler_index>"]
  trusted_hash = "sha256:<hex>"
  ```
- The key is the **file path, event, matcher-group index and handler index**. The hash
  covers a normalized identity: the event name plus the matcher group with that single
  handler, i.e. command, matcher, timeout, async and so on (source code). Plugin hooks
  also store `trusted_content_hash` for bundled scripts (local install).
- **Consequences for `cusco setup`:**
  - Inserting Cusco's group ahead of another tool's group in the same event array
    shifts that tool's index and untrusts it.
  - Changing Cusco's command string or timeout untrusts Cusco.
  - So append new groups, keep them in a fixed position, and keep the command stable
    (inference).
- `~/.codex/.hook-trust/` holds files named by a 64-hex hash, each containing
  `sha256:<hash>`. Their purpose is unverified; see §8 (local install).

### Integrations other than hooks

- **App-server protocol** (source code: `app-server-protocol/src/protocol/common.rs`,
  `schema/typescript/v2/*`). This is the richest source.
  - **Notifications:**
    - `thread/started`, `thread/closed`, `thread/archived`.
    - `thread/status/changed {threadId, status}`, where `status` is `notLoaded`,
      `idle`, `systemError` or `active {activeFlags}`. The flags are
      `waitingOnApproval` and `waitingOnUserInput`.
    - `turn/started`, `turn/completed`, `item/started`, `item/completed`.
    - `item/autoApprovalReview/started` and `item/autoApprovalReview/completed`.
    - `serverRequest/resolved`, `hook/started`, `hook/completed`.
  - **Server-to-client requests:** `item/commandExecution/requestApproval`,
    `item/fileChange/requestApproval`, `item/permissions/requestApproval`,
    `item/tool/requestUserInput` and `mcpServer/elicitation/request`.
  - `thread/status/changed` is almost exactly Cusco's derived state.
  - **Limits:**
    - Delivery is per connection and per thread. The README says threads a
      connection started, resumed or forked receive events, and "thread/unsubscribe
      or disconnect stops delivery" (source code: `app-server/README.md`).
    - A passive observer would have to attach to the daemon's private socket and
      resume each thread. That may keep threads loaded, which interferes with the
      30-minute idle `SessionEnd`, and may make Cusco a target for approval requests
      (inference).
    - The protocol is marked experimental in `codex --help` (local install), and
      third-party clients aren't documented.
  - **Verdict:** not a replacement for hooks in v0.1. It's a good later source for
    `sync`, through `thread/loaded/list` and `thread/read`, which are listed methods
    (source code).
- **Legacy `notify`.** A program in `config.toml` gets a JSON argument
  `{"type":"agent-turn-complete","thread-id","turn-id","cwd","client","input-messages","last-assistant-message"}`
  at turn end (source code: `hooks/src/legacy_notify.rs`). It carries prompt text and
  only covers turn completion, so it's inferior to `Stop`. On this Mac `notify` is
  already used by the bundled Computer Use client (local install).
- **OpenTelemetry.** Managed OTel configuration exists (source code: README heading
  "Managed OpenTelemetry configuration"). Not evaluated; see §8.

---

## 3. Event catalog

Every payload is one JSON object on stdin. It comes from
`codex-rs/hooks/schema/generated/<event>.command.input.schema.json` at
`rust-v0.170.0` (source code), cross-checked with the docs.

**Common fields:** `session_id`, `transcript_path` (nullable), `cwd`,
`hook_event_name` and `model`. `SessionEnd` has no `model`. `permission_mode` is one
of `default`, `acceptEdits`, `plan`, `dontAsk` or `bypassPermissions`. In the table,
`?` marks an optional field.

| Event | Fires when | Event-specific fields | Identifiers |
| --- | --- | --- | --- |
| `SessionStart` | **Root thread only, lazily before its first turn**, ahead of that turn's `UserPromptSubmit` (source code: `session/turn.rs` runs `run_pending_session_start_hooks` before `run_hooks_and_record_inputs`). A thread that is opened but never prompted fires nothing (inference). | `source`: `startup`, `resume`, `clear`, `compact` or `fork`; `permission_mode` | `session_id`. No `turn_id`. |
| `UserPromptSubmit` | Each user input item accepted into a turn, including **steering** input typed mid-turn, which reuses the running turn's id (source code: `SteeredUserInput` path). Not for turns started by agent mail, durable sleep or wake-on-agent-message (source code: `tasks/mod.rs` `maybe_start_turn_for_pending_work`). | `prompt`, `permission_mode`, `agent_id?`, `agent_type?` | `session_id`, `turn_id`, plus `agent_id` when a subagent thread receives user input |
| `PreToolUse` | Before a tool handler runs, for tools that expose a hook payload. Function tools get one by default (source code: `tools/registry.rs`). | `tool_name`, `tool_input`, `tool_use_id`, `permission_mode`, `agent_id?`, `agent_type?` | `tool_use_id` (the model call id), `turn_id`, `agent_id?` |
| `PermissionRequest` | Inside the tool handler, **after** that call's `PreToolUse`, just before an approval prompt (source code: `tools/approvals.rs`). Covers shell escalation, sandbox-denial retry (internal run id `<call_id>:retry`), `write_stdin`, `apply_patch`, MCP tool approval, network access and `request_permissions`. Also fires when the decision will go to the automatic reviewer (Guardian) and not to the user (source code: `request_reviewer_approval`). | `tool_name`, `tool_input`, `permission_mode`, `agent_id?`, `agent_type?`. **No `tool_use_id`.** For shell, `tool_input` is `{command, description?}`, where `description` is the model's justification or `network-access <target>` (source code: `sandboxing.rs` `PermissionRequestPayload::bash`). | `turn_id`, `agent_id?` |
| `PostToolUse` | After the tool returns, **only when it succeeded** (source code: `registry.rs` builds `post_tool_use_payload` only `if success`). | `tool_name`, `tool_input`, `tool_use_id`, `tool_response`, `permission_mode`, `agent_id?`, `agent_type?` | `tool_use_id`, `turn_id`, `agent_id?` |
| `PreCompact` / `PostCompact` | Around context compaction | `trigger`: `manual` or `auto`; `agent_id?`, `agent_type?` | `turn_id`, `agent_id?` |
| `SubagentStart` | First turn of a **thread-spawned** subagent whose start source is `startup` or `fork`. A resumed subagent fires nothing, and internal subagents such as Guardian fire nothing (source code: `hook_runtime.rs`). | `agent_id`, `agent_type` (the role; the default role name if unset), `permission_mode` | `session_id` (**root** thread id), `agent_id` (the subagent's thread id), `turn_id` (**the subagent's own turn**) |
| `SubagentStop` | **End of each successfully completed turn** of a thread-spawned subagent, which is the child's `Stop` (source code: "Root turns run Stop; thread-spawned child turns run SubagentStop"). Not on abort or error. | `agent_id`, `agent_type`, `agent_transcript_path`, `stop_hook_active`, `last_assistant_message`, `permission_mode`. `transcript_path` is the **parent's**. | as above |
| `Stop` | A root turn finishes normally, meaning no follow-up is needed. **Not** fired when the turn errors or is aborted (source code: `turn.rs`, where error paths return before `run_turn_stop_hooks`). | `stop_hook_active`, `last_assistant_message`, `permission_mode` | `session_id`, `turn_id` |
| `Interrupt` | A **root** turn aborted with reason `Interrupted`. Not for `Replaced` or `BudgetLimited` aborts, and not for subagents (source code: `tasks/mod.rs`, `hook_runtime.rs`; docs: "main thread only"). Advisory only. | `permission_mode` | `session_id`, `turn_id` |
| `SessionEnd` | Root thread only. Fires on archive or delete of an open thread, a normal close, or 30 minutes idle with no connected client. Not on `thread/unsubscribe` or switching away (docs). The rollout is flushed first (source code). | `reason`, always `"other"` (schema `const`; docs) | `session_id`. The command input has no `turn_id` (schema). |
| `PreModelRequest` / `PostModelRequest` | Each model request attempt (docs). The 0.170.0 tree has one `model-request` input schema with no `hook_event_name`, `status` or `error` (source code). | Docs: `PostModelRequest` adds `status`, `error` and `token_usage`; `agent_id` and `agent_type` "when available". | `turn_id`, `agent_id?` |

**Identifier semantics** (source code):

- `session_id` is `Session::session_id()`: "the identity shared by the root thread and
  all descendant threads", equal to the root thread's id (`core/src/session/session.rs`,
  `core/src/agent/control.rs`).
- `agent_id` is the subagent's own `thread_id` (`subagent_hook_context`).
  `agent_id` and `agent_type` appear only for `SubAgentSource::ThreadSpawn`. Internal
  subagents (Guardian, memory consolidation) send no `agent_id`.
- `turn_id` is the `sub_id` of the turn context whose thread raised the event, so
  inside a subagent it is **the subagent's turn**, never the parent's.
  - The rollout files record a `root_turn_id` linking a child turn to the root turn
    that spawned it, but hooks don't carry it (local install, source code).
  - Turns started by pending work get a fresh `uuid::Uuid::new_v4()` id (source code).
- `tool_use_id` is the model's call id. It appears on `PreToolUse` and `PostToolUse`
  only.
- There is no request id, elicitation id, pid or surface field in any payload (source
  code).

---

## 4. Mapping to Cusco's model

| Cusco concept | Created by | Ended by |
| --- | --- | --- |
| Session start / resume / fork | `SessionStart`. The source distinguishes `startup`, `resume` (reuses the id), `fork` (new thread, new id) and `clear` (new thread). `compact` fires mid-session after compaction and should only confirm the session (docs, source code). The first record of any kind implicitly opens it, because `SessionStart` is lazy. | `SessionEnd`. Whether `/clear` ends the old thread immediately or only after the idle unload is **unknown** (verify). |
| Turn start | `UserPromptSubmit` without `agent_id`. A repeat with the **same** `turn_id` is steering: confirm the turn and don't cancel it (source code). Turns woken by agent mail or durable sleep have no `UserPromptSubmit`; their first sign is `PreToolUse`, `PermissionRequest` or `Stop` with an unseen `turn_id` (source code). | `Stop` → `ok` |
| Turn failure | No hook. There is no `StopFailure`; an errored turn just never sends `Stop` (source code). `PostModelRequest` with `error` might stand in, but retries make one failure not terminal, and the 0.170 schema lacks the fields (verify). | Next `UserPromptSubmit` (close as `cancelled` or `error`), `SessionEnd`, or staleness |
| User interrupt | `Interrupt`, root only. Esc or `turn/interrupt` (docs, source code) | close turn `cancelled` |
| Subagent start | `SubagentStart`: `agent_id` and `agent_type`. **Attach to the session's currently open root turn**, not to `turn_id`. If no root turn is open (background), attach to the most recent root turn (inference). | `SubagentStop` ends the child's *turn*, not necessarily the agent. A parent can send it more work later, which runs another child turn with no new `SubagentStart` (source code). Aborted or errored child turns send no `SubagentStop`. |
| Subagent attribution | Every tool, approval, prompt and compact event from a thread-spawned subagent carries `agent_id` (schema and source code). Subagent work is fully attributable: `observes` can drop `request:turn` for `request`. | — |
| Background work | Subagents run concurrently with the parent and outlive its turn. With wake-on-agent-messages, a child's report starts a new root turn with no `UserPromptSubmit` (source code). | — |
| Tool approval requested | `PermissionRequest` (+`agent_id?`). Match it to the actor's open `PreToolUse` by digest, with `description` removed from `tool_input` (see §7). | — |
| Approval granted | `PostToolUse` with that `tool_use_id` (success only), else the actor's next `PreToolUse` after the 1 s window, or the actor's turn ending (`Stop`, `SubagentStop`, `Interrupt`). | close `ok` |
| Approval denied | **Nothing fires for the denial.** The tool fails, so no `PostToolUse`, and the model continues to the actor's next `PreToolUse` or `Stop` (source code, inference). "Deny and abort" cancels the turn and, for the root, should fire `Interrupt` (verify). A hook `deny` from `PermissionRequest` never shows a prompt. | as above |
| Approval answered by Guardian | `PermissionRequest` fires, then the automatic reviewer decides without the user (source code). Hooks can't tell the cases apart: `permission_mode` reflects the approval policy, not the reviewer. Cusco will show `needs_input` for the review's duration (gap). | `PostToolUse`, or the next `PreToolUse` |
| Questions to the user | The `request_user_input` tool. The app-server exposes `item/tool/requestUserInput` (source code). As a function tool it should get `PreToolUse` and `PostToolUse` with that tool name (inference, **verify** the exact `tool_name`). Open a `question` request on its `PreToolUse` and close it on `PostToolUse` or the next actor event. | — |
| MCP input requests | Elicitation exists (`mcpServer/elicitation/request`) but **no hook fires** (source code: no elicitation event). Not observable by hooks. | — |
| Idle / waiting | No idle notification. A root session with no open turn is idle at the prompt. App-server `thread/status/changed` is the only explicit idle signal (source code). | — |
| Ordering and concurrency | Synchronous hooks are awaited, so one thread's records arrive in order. Parallel tool calls within a thread run concurrently (source code: `tools/parallel.rs` takes a read lock for parallel-capable tools, a write lock otherwise). Parent and child threads run concurrently. `PreToolUse` → `PermissionRequest` → (`PostToolUse` if success) for one call (source code). | — |

---

## 5. Process and identity

| Surface | Nearest ancestor a hook sees | `argv[0]` / executable |
| --- | --- | --- |
| CLI TUI (default) | The daemon's `codex app-server --listen unix:// … --managed-daemon` process (inference from `daemon_startup.rs`; **verify**) | `~/.codex/packages/app-server-daemon/releases/<ver>/bin/codex` |
| CLI TUI `--no-daemon`, `codex exec` | The `codex` process itself (embedded) (inference) | `~/.codex/packages/standalone/releases/<ver>/bin/codex`, reached through the `~/.local/bin/codex` symlink |
| Desktop app | The same daemon `codex app-server` child (local install shows no app-owned app-server while the app is closed; inference) | as above. The app's bundled `…/ChatGPT.app/Contents/Resources/codex-cli/bin/codex` may be used if no daemon is available (**verify**). |
| IDE extension | Probably its own `codex app-server` over stdio (inference; **verify**) | the extension's bundled `codex` |

Between the hook and Codex sits `$SHELL -lc`, which zsh may replace with the command
through `exec` (source code, inference).

**Consequences:**

- **One process hosts many sessions.** Process exit tells Cusco nothing per session.
  When the daemon exits, every session it hosted is gone, but the daemon is
  long-lived and launchd-owned.
- **Cusco's recovery rule produces permanent `unknown`.** The supervisor
  (`codex app-server daemon pid-update-loop`) is correctly skipped. But the
  always-running `codex app-server` child gets an unconfirmed process node, so Codex
  shows `unknown` until some session reports, which may be never. Treat a
  `--managed-daemon` app-server as a host that is never claimed by a process node.
  Rely on `sync` from persisted state for recovery (§6).
- **No pid or surface in any payload** (source code).
  - The surface can be recovered from the thread's persisted originator: rollout
    `session_meta.originator` (`codex-tui`, `Codex Desktop`) or `state_5.sqlite`
    `threads.originator` (local install).
  - Both are looked up from `session_id`, or from `transcript_path` when it isn't
    null.
  - The parent walk alone reports "app-server" for both the TUI and the desktop app.

---

## 6. Persistent state

All observations below are field names only (local install).

- **Rollouts:** `~/.codex/sessions/YYYY/MM/DD/rollout-<timestamp>-<thread id>.jsonl`,
  with archived ones in `~/.codex/archived_sessions/`.
  - Each line is `{timestamp, type, payload, ordinal, metadata?}`.
  - **Record types:**

    | `type` | Notable fields |
    | --- | --- |
    | `session_meta` | `id`, `session_id`, `parent_thread_id`, `forked_from_id`, `source`, `originator`, `cwd`, `cli_version`, `agent_path`, `agent_nickname`, `thread_source`, `history_mode`, … |
    | `turn_context` | `turn_id`, `root_turn_id`, `approval_policy`, `approvals_reviewer`, `cwd`, `model`, `sandbox_policy`, … |
    | `event_msg` | `payload.type` is one of the events below |
    | `response_item` | `message`, `function_call {call_id, name, …}`, `custom_tool_call`, `custom_tool_call_output`, `reasoning`, `agent_message {author, recipient}`, `additional_tools` |
    | other | `token_usage_record {session_id, thread_id, turn_id, root_turn_id}`, `world_state`, `inter_agent_communication_metadata {trigger_turn}` |

  - **`event_msg` events:**

    | Event | Fields |
    | --- | --- |
    | `task_started` | `turn_id`, `root_turn_id`, `started_at` |
    | `task_complete` | `turn_id`, `completed_at`, `duration_ms` |
    | `turn_aborted` | `turn_id`, `reason` |
    | `item_completed` | `thread_id`, `turn_id`, `item` |
    | others | `token_count`, `thread_settings_applied` |

  - `session_meta.source` values seen: `"vscode"` for root sessions,
    `{"subagent":{"thread_spawn":{"parent_thread_id":…}}}` and
    `{"subagent":{"other":"guardian"}}`.
- **SQLite** (WAL mode; the version is part of the file name, so expect churn):
  - `~/.codex/state_5.sqlite`:
    - `threads (id, rollout_path, source, cwd, archived, agent_path, agent_role, originator, updated_at_ms, …)`
    - `thread_spawn_edges (parent_thread_id, child_thread_id, status)`
  - `~/.codex/thread_history_1.sqlite`:
    - `thread_turns (thread_id, turn_id, status, started_at, completed_at, error_json, root_turn_id, …)`
  - `codex migrate-rollouts` moves "legacy local sessions to paginated thread
    history", so storage is moving from JSONL to SQLite, and `transcript_path` is
    nullable (local install, source code).
- **Approval requests are not persisted.** Across 105 recent rollouts the only
  `event_msg` types were `item_completed`, `token_count`, `task_started`,
  `task_complete`, `thread_settings_applied` and `turn_aborted` (local install).

**Rebuilding after a restart** (inference):

1. **Candidates.** Read the non-archived threads from `state_5.sqlite`, ordered by
   recency.
2. **Open turns.** Take the latest `thread_turns` row whose `completed_at` is null or
   whose status is in progress (**verify** the status vocabulary). Alternatively, scan
   the rollout tail for a `task_started` with no `task_complete` or `turn_aborted`
   carrying the same `turn_id`.
3. **Subagents.** Use `thread_spawn_edges` with an open status, or child rollouts
   whose `task_started.root_turn_id` names the root turn. The edges give the parent
   thread and the root turn, which hooks never send.
4. **In-flight tools.** A `function_call` or `custom_tool_call` without a matching
   `*_output` means a tool is running or waiting for approval. Persisted state can't
   tell which, so send those turns as **unconfirmed**. That satisfies acceptance #3
   (never a false `idle`), but can't recreate request nodes.
5. **Loaded or not.** Disk doesn't say whether a thread is still loaded in the daemon.
   Only the app-server (`thread/loaded/list`, `thread/status/changed`) can.

---

## 7. Proposed adapter

Codex payloads carry `session_id` (the **root thread id**, shared by the whole agent
tree), `cwd` and, on turn-scoped events, `turn_id`. `turn_id` is the turn of the
thread that raised the event. Inside a subagent it is the subagent's own turn, and
`agent_id` (the subagent's thread id) is present.

The adapter tracks one **actor** per event: the root (no `agent_id`) or a subagent
(`agent_id`). Root events use `turn_id` as the turn key. Subagent events never create
turns.

| Hook | Cusco event |
| --- | --- |
| `SessionStart` | `open` session. `resume` reopens an ended session. `compact` only confirms it. |
| `UserPromptSubmit` (root) | `open` turn keyed by `turn_id`. A different, still-open root turn closes as `cancelled`. The same `turn_id` is steering and only confirms. |
| `UserPromptSubmit` (`agent_id`) | Confirm or open that agent under its known turn. No root turn. |
| `PreToolUse` | Record `tool_use_id` against its digest for this actor. Close the actor's approvals that have been open for at least 1 s. If `tool_name` is `request_user_input` (**verify**), `open` a request with reason `question`, keyed by `tool_use_id`. Root: implicitly opens an unseen `turn_id`. Agent: opens or reopens the agent. |
| `PermissionRequest` | `open` a request with reason `approval` under the actor (the agent if `agent_id` is set, else the root turn). Key it by the `tool_use_id` of the actor's open call whose digest matches. Otherwise, if the actor has exactly one open call, use that one. Otherwise key it by digest. |
| `PostToolUse` | `close` the request with that `tool_use_id` (`ok`) and forget the call. It fires only on success. |
| `SubagentStart` | `open` agent `agent_id` under the session's open root turn, or the latest root turn if none is open. **Ignore `turn_id`** for placement. |
| `SubagentStop` | `close` the agent (`ok`), but allow later activity with the same `agent_id` to reopen it, because Codex subagents run several turns. |
| `Stop` | `close` the root turn `turn_id` (`ok`). Its requests close; its agents don't. |
| `Interrupt` | `close` the root turn `turn_id` (`cancelled`). |
| `SessionEnd` | `close` the session. Root only. Not the same as the process exiting. |
| `PreCompact`, `PostCompact`, `*ModelRequest` | Ignored in v0.1. Could later confirm turns and agents. |

**Hook command change.** For Codex, compute the tool input digest over `tool_input`
with the top-level `description` key removed. `PermissionRequest` adds
`description` (the justification or `network-access <target>`) to the shell input,
which `PreToolUse` and `PostToolUse` don't have (source code). Network-access approvals
arise mid-command and may still not match. The "exactly one open call" fallback covers
them.

**Client row:** `observes: ["session", "turn", "agent", "request"]`. Drop
`request:turn`.

### Changes from `docs/design.md` and `src/adapter/codex.rs`, and why

1. **`SubagentStart` is placed under the wrong turn.**
   - What happens now: the adapter puts the agent under `turn_path(record.turn_id)`
     and calls `enter_turn` with it.
   - Why it's wrong: that `turn_id` is the subagent's own turn (source code), so this
     creates a phantom root turn. No `Stop` ever closes it, because child turns end
     with `SubagentStop`, and `turns.adopt` can make it the session's open turn.
     The session shows `working` until staleness.
   - **Fix:** place the agent under the open root turn.
2. **Tool events and approvals ignore `agent_id`.**
   - What happens now: `PreToolUse`, `PermissionRequest` and `PostToolUse` inside a
     subagent go to `turn_path(child turn_id)`, which opens more phantom turns.
   - Why it's wrong: `agent_id` is present on all of them (schema). The design's
     "verify" item resolves to **yes**.
   - **Fix:** attribute to the agent, keep a separate approval list per actor, and
     switch `observes` to `request`.
3. **Approvals are answered across actors.**
   - What happens now: with per-turn bookkeeping, root and child approvals share no
     list, but only because of the phantom turns. The design text ("close the turn's
     open approval") would let one actor's `PreToolUse` close another's approval once
     agent-attributed events land under the root turn.
   - **Fix:** key approvals by actor.
4. **The `PostToolUse` digest match fails for shell approvals that carry a
   justification.**
   - Why: the `PermissionRequest` input is `{command, description}`, while the
     `PreToolUse` and `PostToolUse` input is `{command}` (source code).
   - **Fix:** strip `description` before hashing, and key the request by
     `tool_use_id` through the matching `PreToolUse`. `PostToolUse` then closes it by
     `tool_use_id`, which it carries.
5. **`PostToolUse` doesn't fire for failed tools** (source code). The design
   assumes it closes granted approvals. A granted command that fails leaves the
   request to the 1-second `PreToolUse` rule or the end of the turn. Document that.
6. **`SubagentStop` is per child turn, not per agent** (source code).
   - The design's rule "a late record never reopens a stopped subagent" is wrong for
     Codex. A follow-up to the same agent runs a new child turn with no
     `SubagentStart`.
   - Synchronous hooks keep one thread's events in order, so later activity from a
     stopped agent is real, not late.
   - **Fix:** allow reopening for Codex.
7. **There is no `StopFailure`, and errored turns get no `Stop`** (source code). The
   design doesn't say how Codex closes a failed turn. Close it on the next
   `UserPromptSubmit` (already implemented), `SessionEnd` or staleness. Consider
   `PostModelRequest` once its fields ship (verify).
8. **`SessionStart` is lazy and root-only.** It fires just before the first turn's
   `UserPromptSubmit`, not when a thread is created. Subagents get `SubagentStart`
   instead, and resumed subagents get nothing (source code). The design only says
   "`open` session".
9. **`SessionEnd` carries no `turn_id`** and its `reason` is always `other` (schema,
   docs). The design implies every payload has `turn_id`.
10. **Not every turn starts with `UserPromptSubmit`.** Steering repeats an existing
    `turn_id`, which the current `Turns::begin` correctly treats as no change. Turns
    woken by agent mail or durable sleep start with no prompt hook (source code).
    The adapter already opens turns implicitly from tool events. A text-only woken
    turn is invisible until its `Stop`.
11. **`PermissionRequest` may not mean the user is being asked.** Guardian
    auto-review runs after the hook (source code). Note this in `observes` docs or
    as a known false `needs_input`.
12. **Questions and MCP elicitation.**
    - `request_user_input` can be observed through `PreToolUse` (verify the tool
      name). Add it.
    - MCP elicitation has no hook. The design's `request` vocabulary should say
      Codex can't observe `mcp_input`.
13. **Process identity and recovery.**
    - With the 0.170 TUI daemon default, the CLI and the desktop app both hook from
      the shared `codex app-server`.
    - Per-session pid liveness doesn't apply. The design's supervisor rule skips
      only the supervisor, so the always-on app-server leaves Codex `unknown` forever
      after a Cusco restart.
    - **Fix:** never create process nodes for `--managed-daemon` app-servers, and
      recover Codex through `sync` from `state_5.sqlite` and `thread_history_1.sqlite`
      or the rollouts. Derive `surface` from the persisted originator.
14. **Trust key is positional.**
    - The design says trust is "tied to the hook's hash". It's keyed by
      *path:event:group:handler index* and checked against a hash of the normalized
      group and handler (source code).
    - The installer must append and never reorder, or it will untrust other tools'
      hooks.
    - `cusco doctor` should read `[hooks.state]` to report "installed but not
      trusted", and check `allow_managed_hooks_only` and `features.hooks`.
15. **Install `PreToolUse`.** The digest and actor-matching scheme depends on it.
    Other hook installations seen locally omit it.

---

## 8. Gaps and verify items

Each item names the recorded session that would settle it. Record with a capture-only
hook that logs `hook_event_name`, the identifier fields and a digest. Never log
`prompt`, `tool_input` or `tool_response`.

1. **CLI process topology.** Is a default `codex` TUI session hosted in the daemon's
   app-server, so that the hook's parent chain reaches `codex app-server`?
   Compare with `--no-daemon` and `codex exec`. Use `ps -o pid,ppid,args` on the hook's
   ancestors.
2. **Does the shell `PermissionRequest`'s `command` equal `PreToolUse`'s `command`?**
   Request an escalated command with and without a justification and compare digests
   with `description` stripped.
3. **Denial.**
   - Deny an approval and confirm that no hook fires until the next `PreToolUse` or
     `Stop`.
   - Choose "deny and stop" (abort) and check whether `Interrupt` fires and whether a
     subagent's abort sends anything.
4. **Root interrupt with running subagents.** Are child turns aborted? Does any
   `SubagentStop` arrive? Otherwise agent nodes leak until staleness.
5. **Follow-up to a stopped subagent.** Send a second message to a spawned agent.
   Confirm there is no second `SubagentStart`, and that tool events carry the same
   `agent_id` with a new `turn_id`.
6. **`request_user_input`.** Confirm `PreToolUse` fires and its exact `tool_name`, in
   Plan mode or wherever the tool is offered.
7. **Guardian / auto-review.** With automatic review enabled, confirm
   `PermissionRequest` fires and find any signal that distinguishes it. Do the
   Guardian subagent's own events (no `agent_id`) leak in as root events with an
   unknown `turn_id`, and with which `session_id`?
8. **`PreModelRequest` / `PostModelRequest`.** The docs list them with `status` and
   `error`, but the 0.170.0 schema has neither. Check whether they're
   registrable in 0.170 and whether a failed turn's last `PostModelRequest` is a
   usable failure signal.
9. **`/clear` and `fork`.** Does the old thread get `SessionEnd` at once or only
   after the 30-minute idle unload? Does `fork` send `SessionStart` with source `fork`
   under the new id before its first turn?
10. **`thread_turns.status` vocabulary and `thread_spawn_edges.status` values**, for
    `sync`.
11. **`~/.codex/.hook-trust/` files.** What do the file names hash (project layers?
    plugin trust?), and do they matter for user-level hooks?
12. **IDE extension and desktop app fallback.** Which `codex` binary hosts sessions,
    and do they share the daemon?
13. **OpenTelemetry.** Not evaluated. Check whether OTel events carry turn and
    approval lifecycle without prompt content, as a possible second source.
14. **Docs fidelity.** The docs page was read through a summarizing fetch, not
    verbatim. Every payload field above was confirmed against the generated schemas
    and source. Exit-code, timeout and matcher details rest on the docs summary alone.

---

## 9. Sources

Web, read 18 November 2026:

- https://developers.openai.com/codex/hooks (308 redirect to the next URL)
- https://learn.chatgpt.com/docs/hooks (the full page loaded; its last section was
  "Schemas")
- https://github.com/openai/codex at tag `rust-v0.170.0`, read through the GitHub API.
  `main` was also listed, for the file inventory.

`openai/codex` source files, at `rust-v0.170.0`:

- `codex-rs/hooks/schema/generated/{session-start,session-end,user-prompt-submit,pre-tool-use,permission-request,post-tool-use,pre-compact,post-compact,subagent-start,subagent-stop,stop,interrupt,model-request}.command.input.schema.json`
- `codex-rs/hooks/src/engine/command_runner.rs`
- `codex-rs/hooks/src/engine/dispatcher.rs`
- `codex-rs/hooks/src/engine/discovery.rs`
- `codex-rs/hooks/src/events/{common,interrupt,permission_request,session_end}.rs`
- `codex-rs/hooks/src/legacy_notify.rs`
- `codex-rs/core/src/hook_runtime.rs`
- `codex-rs/core/src/session/{session,turn,handlers}.rs`
- `codex-rs/core/src/tasks/mod.rs`
- `codex-rs/core/src/agent/control.rs`
- `codex-rs/core/src/tools/{approvals,sandboxing,registry,parallel}.rs`
- `codex-rs/core/src/tools/handlers/unified_exec/exec_command.rs`
- `codex-rs/app-server/README.md`
- `codex-rs/app-server-protocol/src/protocol/common.rs`
- `codex-rs/app-server-protocol/schema/typescript/v2/{ThreadStatus,ThreadActiveFlag,ThreadStatusChangedNotification,ToolRequestUserInputParams}.ts`
- `codex-rs/tui/src/{daemon_startup,app_server_connection}.rs`

Local paths, all read-only; for files with content, only structure and field names
were read:

- `~/.local/bin/codex`, `~/.codex/packages/standalone/`,
  `~/.codex/packages/app-server-daemon/releases/`, `~/.codex/version.json`
- `~/.codex/hooks.json` (structure and events only), `~/.codex/config.toml` (table
  headers, `[hooks.state]` keys, `notify` shape), `~/.codex/.hook-trust/` (names and
  sizes)
- `~/.codex/app-server-daemon/{daemon.pid,settings.json}` (keys),
  `~/.codex/app-server-control/`, `~/.codex/ipc/`
- `~/.codex/sessions/2026/11/*/rollout-*.jsonl` (record types and key names only)
- `~/.codex/state_5.sqlite`, `~/.codex/thread_history_1.sqlite` (table and column
  names only)
- `/Applications/ChatGPT.app/Contents/Info.plist`,
  `/Applications/ChatGPT.app/Contents/Resources/codex-cli/codex-package.json`
- Process table (`ps`) for the running daemon and app-server
- Cusco: `docs/design.md`, `src/adapter/codex.rs`, `src/adapter/mod.rs`, `src/record.rs`
