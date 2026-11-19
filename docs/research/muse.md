# Muse Code: research briefing for a Cusco adapter

**Research · 18 November 2026**

Basis markers: (docs) official Muse Code developer docs, (source code) code read
on this machine, (local install) observed in the read-only inspection of this
Mac, (inference) my conclusion, not stated anywhere. Paths under the home folder
are written as `~`.

## 1. Product and surfaces

Muse Code is a terminal coding agent with a session protocol, SDKs (TypeScript,
Python, Rust) and a plugin and hook system (docs). It is made by Meta: the docs
live under the `meta-models` GitHub Pages site, and the launcher downloads
releases from Meta hosts and logs in through Meta's identity service (local
install).

**Versions.**
- Docs: `versions.json` lists `1.4` as `latest` and a `next` channel marked
  "Developer Preview" (docs). The `next` hook pages are identical to 1.4 apart
  from that banner (docs). The hook pages say every value was checked against
  Muse Code **1.3.0**, so they trail the binary (docs).
- Installed: `muse --version` prints `Muse Code 1.4.23 (1.4.23-R8977.1)`
  (local install). The launcher supports the `muse-stable` and `muse-canary`
  channels (source code).
- The wire protocol (MSP) is pre-1.0. It may change non-additively, and no
  protocol changelog is published yet (docs).

**Surfaces.** Every one of them runs the same binary and the same session
runtime, and "a hook fires the same way whether the session belongs to the
terminal UI, `muse exec` or `muse serve`" (docs).

| Surface | What it is | Hooks? | Scope |
| --- | --- | --- | --- |
| `muse` (TUI) | Interactive terminal agent. Runs its session server in-process (docs). | Yes (docs) | **In** |
| `muse exec` | Headless single prompt (docs, local `--help`). | Yes (docs) | **In** |
| `muse serve` | MSP host over stdio for SDK clients (editors, desktop apps, scripts). One client per host. Can host many sessions (docs). | Yes (docs) | **In** |
| `muse serve --listen` | Multi-client socket daemon on a well-known per-user socket (docs). | Yes (inference: same runtime) | **In**, shared-process case |
| `muse acp` | Agent Client Protocol server for Zed and JetBrains IDEs. Experimental, on by default. Each editor thread is one Muse session inside one `muse acp` process (docs). | Yes (inference: "runs the same Muse Code host as `muse serve`" (docs)) | **In**, shared-process case |
| `muse attach` | Attaches a terminal or an MSP stdio bridge to a session that is already running elsewhere (local `--help`). | No new hooks. The hosting process runs them (inference). | Out (no separate sessions) |
| `/Applications/Muse.app` | Meta's consumer "Muse" assistant: bundle `com.meta.endo`, version 12.1, a web shell called "Hatch" (local install). It contains no Muse Code strings, no `muse serve` reference and no hook config. Its session API is an HTTP `/api/session/start` against its own backend (local install). | No | **Out**: the separate "personal" Muse product |
| Muse Code desktop app | None installed. The docs mention desktop apps only as generic MSP clients (docs). | n/a | Out (would appear as `muse serve`) |
| Cloud | No cloud agent surface is documented (docs). | n/a | Out |

Maturity: the CLI is the primary product. `muse acp`, the socket daemon, the
`needsInput` session status and many MSP methods are explicitly experimental
(docs).

## 2. Extension mechanism

### Hook sources

There are four sources, and they share one event catalog, one stdin payload, one
output schema and one set of size caps (docs).

| Source | File | Trust step | Loads |
| --- | --- | --- | --- |
| Managed | the file named by `managed_hooks_path` in settings | none | at session start (docs) |
| **User** | the `hooks` member of `$XDG_CONFIG_HOME/muse/settings.json`, by default `~/.config/muse/settings.json` | **none** | at session start (docs) |
| Project | `<project-root>/.muse/hooks.json` | the workspace must be trusted (TUI prompt or `--trust-workspace`) | at session start (docs) |
| Plugin | the manifest's `capabilities.hooks` in a `.muse-plugin` package (Claude Code and Codex plugin formats are also imported) | each hook is approved with `muse plugins approve`. Any change returns it to review | at host start (docs) |

**Cusco should use the user source.** It has no trust step, unlike Codex
(inference from docs). Edits take effect **at the next session, not in the
running one** (docs). Sessions that are already running never report until they
are restarted (inference).

### Format (settings-level)

```json
{
  "schema_version": 1,
  "hooks": {
    "<EventName>": [
      { "matcher": "<optional>",
        "hooks": [ { "type": "command", "command": "<shell string>", "timeout": 5 } ] }
    ]
  }
}
```

- Handler fields form a closed set: `type` (must be `"command"`), `command`,
  `timeout` (seconds, default 600, 0 is raised to 1), `statusMessage`, `async`,
  `onFailure`, `commandWindows`, `outputCapabilities` (docs).
- A handler with an unknown key is skipped with a warning. A typed field with
  the wrong type, such as `"timeout": "5"`, **rejects the whole file**, so every
  hook in it is lost, including other tools' hooks. A malformed settings file
  (for example one without `schema_version`) **stops Muse from starting** (docs).
  `cusco setup` must therefore write exact, validated JSON.
- `hooks.state` is a reserved map for per-hook enablement (docs).
- Matchers do not apply to `UserPromptSubmit` and `Stop`: anything other than
  `*` skips the group (docs). Cusco should omit the matcher everywhere.
- `~/.config/muse/` also holds `.settings.json.lock`, which suggests Muse writes
  settings under a lock (local install). Its exact protocol is not documented.

### How a command hook runs (docs)

- **Command.** Settings hooks run as `$SHELL -c '<command>'`, falling back to
  `/bin/sh`. Plugin hooks get an argv array with no shell.
- **Stdin.** One JSON object, then the stream closes. A payload over **256 KiB
  is not delivered, and the hook is skipped** for that occurrence.
- **Working directory.** The payload's `cwd`: the session workspace for
  lifecycle events, the tool's effective directory for tool events.
- **Environment.** It is **cleared**. Only `HOME, PATH, USER, LOGNAME, TMPDIR,
  TEMP, TMP, SHELL, LANG, LC_ALL, TERM` pass through, plus
  `PROVENANCE_PROVIDER_SCOPE` when the launching process set it ("a label naming
  that surface"). Muse then sets **`MUSE_SESSION_ID`** (the top-level chat) and
  **`MUSE_THREAD_ID`** (the chat or subagent the hook fires for, equal to
  `MUSE_SESSION_ID` in a top-level chat). These two are applied last, so nothing
  can override them. A run with no real session gets neither. Plugin hooks also
  get `MUSE_PLUGIN_*` variables. The hook command therefore cannot rely on any
  `CUSCO_*` variable and must find its socket from `HOME`.
- **Concurrency.** "Every handler that matches an occurrence starts at the same
  time." Answers are applied in configured order (managed, then user, then
  project). `async: true` makes a hook observation-only, running alongside the
  turn.
- **Timeouts.** Settings hooks: `timeout` in seconds, default 600. Plugin hooks:
  `timeoutMs`, default 600000. A killed hook is recorded as `timed_out`, and the
  turn continues without it.
- **Caps.** stdout and stderr are capped at 16 KiB each. Exceeding a cap kills
  the process tree.

### What output and exit codes do (docs)

The output can change agent behavior, so **Cusco's hook must stay silent and
exit 0**:

| Exit | Effect |
| --- | --- |
| 0 | stdout is parsed as JSON if it starts with `{` or `[`. **Plain stdout is added to the model's context on `SessionStart`, `SubagentStart` and `UserPromptSubmit`.** |
| 2 with stderr | Blocks on events that can block. On `UserPromptSubmit` **the user's prompt is rejected**, and on `PreToolUse` the tool call is refused. |
| anything else | Recorded as `failed`. The TUI shows a failure notice for five seconds, and the session continues. |

The JSON output schema is closed and camelCase. It covers `decision`, `reason`,
`continue`, `stopReason`, `systemMessage`, and inside `hookSpecificOutput`:
`hookEventName`, `additionalContext`, `permissionDecision`, `updatedInput`, and
`decision` on `PermissionRequest` (docs). Two differences from Claude Code are
deliberate: a bare `permissionDecision: allow` is rejected, and the
`PermissionRequest` decision object allows only `behavior` and `message` (docs).

### Non-hook integration points

- **MSP (Muse Session Protocol).** JSON-RPC 2.0 over the stdio of a `muse serve`
  you spawn, over the socket daemon, or through `muse attach --stdio
  <SESSION_ID>`, which "bridges stdin/stdout to the session's host" (docs, local
  `--help`). It exposes exactly what Cusco derives by hand:
  - `session/statusChanged`, broadcast to every connection, carries `status`
    (`notLoaded | idle | running`, plus an experimental `needsInput`) and
    `attention` flags (`approvalPending`, `inputPending`, and experimental
    `admissionPending`) (docs).
  - `approval/requested` and `approval/resolved`, with an `ApprovalSubagentOrigin`
    (`childRunId`, `childSessionId`, `subagentId`) on child approvals projected
    onto the parent (docs).
  - `userInput/requested` and `userInput/settled` for questions (docs).
  - `turn/started` and `turn/completed`, with terminal `completed`, `failed` or
    `cancelled` (docs).
  - `serve/isIdle` (experimental) (docs).

  It is a much richer source than hooks, but a connection is a **driver** with
  full authority, not a passive observer. The host is unauthenticated by design
  (the "spawn boundary"), the TUI's attachable terminal is "off by default", and
  most of the useful fields are experimental (docs). It doesn't suit v0.1, but
  it is worth revisiting (inference).
- **`muse sessions --json [--watch]`** prints one entry per *running* session:
  `sessionId`, `name`, `workspaceLabel`, `openedBy`, `binaryVersion`,
  `attachable`, plus `sideOf` on a side chat's entry. It "reads the local session
  registry and connects to no session". `--watch` prints a new list whenever the
  registry changes (local `--help`). This is a documented, passive liveness and
  recovery feed (see section 6). I did not run it, to keep this inspection
  read-only.
- **SDK transcript helpers.** The Python SDK reference lists `load-transcript`,
  `transcript-dirs` and `read-session-durability` pages (docs). I did not read
  them.

## 3. Event catalog

The reference lists **19** events (docs). The settings guide says Muse 1.3.0
accepts **17** names in settings-level hooks: the 19 minus `SessionFork` and
`ToolUseStart` (docs). Whether 1.4.23 accepts those two in `settings.json` is a
verify item.

### Common stdin keys (docs)

| Key | Notes |
| --- | --- |
| `hook_event_name` | PascalCase, as in the table below |
| `session_id` | "The session". Whether this is the child's id inside a subagent is a **verify** item. |
| `turn_id` | **on every event except `SessionStart` and `SessionEnd`** |
| `cwd` | session workspace, or the tool's effective directory on tool events |
| `transcript_path` | string or `null` ("when one exists"). It was `null` in the docs' echo-provider sample. |
| `model`, `permission_mode` | always present |
| `model_provider` | sometimes present |
| `agent_id`, `agent_type` | **only in child sessions, and only on `PostToolBatch` and `StopFailure`** |

The env vars `MUSE_SESSION_ID` (root) and `MUSE_THREAD_ID` (current thread) are
available on every event (docs).

### Events

| Event | Fires when (docs) | Event-specific keys (docs) | Can affect the agent (docs) |
| --- | --- | --- | --- |
| `SessionStart` | session starts, resumes, is cleared, is forked, or continues after compaction | `source`: `startup`, `resume`, `clear`, `compact` or `fork` | context, stop |
| `UserPromptSubmit` | a user prompt is submitted, before the model sees it | `prompt` | block (rejects the prompt), context |
| `PreToolUse` | a tool call is validated and about to run, **before approval** (`permissionDecision: ask` forces the prompt) | `tool_name`, `tool_input`, **`tool_use_id`** | block, rewrite input, context |
| `PermissionRequest` | a tool call is waiting for approval | `tool_name`, `tool_input`. **There is no `tool_use_id`.** | allow or deny |
| `ToolUseStart` | an **approved** tool call crosses its execution-start boundary, before it runs | **undocumented** | observation only |
| `PostToolUse` | a tool call finished successfully | `tool_name`, `tool_input`, `tool_use_id`, `tool_response` | feedback, context, stop |
| `PostToolUseFailure` | a tool call failed | `tool_name`, `tool_input`, `tool_use_id`, `error`, `is_interrupt` (currently always `false`), `duration_ms` | feedback, context, stop |
| `PostToolBatch` | one batch of tool results was committed | `tool_calls[]`, each with `tool_name`, `tool_input`, `tool_use_id`, `tool_response` | feedback, context, stop |
| `PreLLMCall` / `PostLLMCall` | a model request is about to be sent / a response arrived | `provider`, **`request_id`**, `attempt`, `step`, message and tool summaries, `status`, `response_id`, `usage`, `finish_reason`, … | block, context |
| `PreCompact` / `PostCompact` | compaction is about to run / has finished | `trigger` | `PreCompact` can veto |
| `SubagentStart` | a child agent session starts, inside a turn | **`subagent_id`, `child_session_id`** | context |
| `SubagentStop` | a child agent **wants to** stop | `subagent_id`, `child_session_id`, `stop_hook_active`, `last_assistant_message` | **block: the child keeps working** |
| `Stop` | the model **wants to** end the turn | `stop_hook_active`, `last_assistant_message` | **block: the model keeps working** (at most `max_consecutive_stop_hook_continuations`, default 8) |
| `StopFailure` | the turn ended because the model call failed | `error` (a category), `error_details`, `last_assistant_message` | observation only |
| `SessionEnd` | the session shuts down | `reason` (values undocumented) | observation only |
| `Notification` | **an approval prompt has been waiting for six seconds** | `notification_type` (only `permission_prompt`), `title`, `message` | observation only |
| `SessionFork` | a fork of the session is being opened | **undocumented** | observation only |

**Identifiers**

| Identifier | Where |
| --- | --- |
| Session | `session_id` on every payload. `MUSE_SESSION_ID` is always the top-level chat (docs). Session ids are UUIDv7s and double as the on-disk directory name (docs). |
| Thread | `MUSE_THREAD_ID` env only, never in the payload (docs). |
| Turn | `turn_id` on every payload except `SessionStart` and `SessionEnd` (docs). |
| Subagent / child | `subagent_id` and `child_session_id` on `SubagentStart` and `SubagentStop`, which fire in the parent. `agent_id` and `agent_type` appear only on `PostToolBatch` and `StopFailure` in a child (docs). How `subagent_id`, `child_session_id`, `agent_id` and the child's `MUSE_THREAD_ID` relate is a **verify** item. |
| Tool call | `tool_use_id` on `PreToolUse`, the post-tool events and `PostToolBatch` entries, **not on `PermissionRequest`** (docs). `ToolUseStart` is unknown. |
| Request | `request_id` on the LLM-call events only (docs). Approvals have an `approvalId` on MSP, but no hook carries it (docs). |
| Elicitation | No hook event exists (docs). |

**Hints from `wooting-glance` (source code), checked against the docs:**
- Claude-style PascalCase event names: **confirmed** (docs).
- `child_session_id` on parent lifecycle events: **confirmed** for
  `SubagentStart` and `SubagentStop`, alongside `subagent_id` (docs).
- `MUSE_SESSION_ID` is the root session and `MUSE_THREAD_ID` the current thread:
  **confirmed** (docs).
- Its test assumes that events from inside a child carry the child's id in both
  `session_id` and `MUSE_THREAD_ID`, and that the child's thread id equals the
  `child_session_id` on `SubagentStart`. **Not stated in the docs: verify.**
- It registers 9 events in this Mac's user settings: `UserPromptSubmit`,
  `PermissionRequest`, `PostToolUse`, `PostToolUseFailure`, `Stop`, `StopFailure`,
  `SessionEnd`, `SubagentStart` and `SubagentStop` (local install).

## 4. Mapping to Cusco's model

| Cusco concept | Opened or created by | Ended by |
| --- | --- | --- |
| Session start / resume | `SessionStart`, with `source` `startup` or `resume`. A resume reuses the session id, because the id is the durable log identity (docs). | n/a |
| Session clear | `SessionStart` with `source=clear` (docs). Whether clear mints a new id or keeps it is unknown: **verify**. | as below |
| Session fork | `SessionStart` with `source=fork`, plus `SessionFork` (docs). A fork is a new session; the index records a fork source (local install). | as below |
| Session end | n/a | `SessionEnd` with `reason`, on orderly shutdown: closed stdin, SIGINT or SIGTERM (docs). **A hard kill writes nothing and fires nothing** (docs), so process liveness is still required. In `serve` and `acp`, whether idle unload fires `SessionEnd` is a **verify** item. |
| Turn start | `UserPromptSubmit` with `turn_id` (docs). `turn_id` is also on every later event, so any tool event re-confirms its turn. | n/a |
| Turn end | n/a | `Stop`, but it means "the model *wants* to end the turn", and another hook (a plugin, say) can block it and keep the model working (docs). A second `Stop` with `stop_hook_active: true` and the same `turn_id` reveals this. `turn/foregroundCompleted` shows that background "reminder" checks can hold a turn open after its foreground work is done (docs). **Verify** whether `Stop` fires before or after those checks. |
| Turn failure | n/a | `StopFailure` with `error` category, for model-call failures only (docs) |
| User interrupt | n/a | **No hook** (docs). MSP records interrupts as a `cancelled` terminal (docs), and the local logs have a `terminal` record with `terminal: completed/failed/cancelled` (local install). Whether `Stop` fires on interrupt is unknown: **verify**. Without it, an interrupted turn closes only on the next `UserPromptSubmit` with a new `turn_id`, on `SessionEnd`, or by going stale after an hour. **There is no `idle_prompt` equivalent**, because `Notification` only reports permission prompts (docs). |
| Subagent start / stop | `SubagentStart` in the parent, keyed by `child_session_id`, under the parent's `turn_id` (docs) | `SubagentStop`, which is also a blockable "wants to stop" event (docs) |
| Events inside a child | Attributed by `MUSE_THREAD_ID` ≠ `MUSE_SESSION_ID` (docs). Only `PostToolBatch` and `StopFailure` also carry `agent_id` (docs). **Attribution depends on the hook command capturing env vars.** Nested children: `MUSE_SESSION_ID` stays the root, and the grandchild's `SubagentStart` fires in the child's thread, so parentage is recoverable (inference). | n/a |
| Background work | Subagent tasks run in `foreground` or `background` `execution_mode` (local install, log records). "Background tasks started by the turn are not killed by either" stop lane (docs). `serve/isIdle` counts running subagents and monitors as running (docs). `Stop` has **no** `background_tasks` list (docs), so there is no reconciliation like Claude's. **Verify** whether `SubagentStop` fires for background children and whether internal "reminder agents" (skill, goal and verify reminders, which run as linked child sessions per the local logs) fire `SubagentStart`. | n/a |
| Tool approval requested | `PermissionRequest`, attributed by `MUSE_THREAD_ID`, then matched to the `tool_use_id` of the same thread's `PreToolUse` by input digest (docs plus the design's existing approach). `Notification` (`permission_prompt`) fires once a prompt has waited **six seconds** (docs). It carries `session_id`, `turn_id` and the thread env, but no tool id. **Caveat:** local logs show approvals decided by an automated reviewer (`presentation_phase: automated_reviewing`, `decision_source.kind: llm_judge`) (local install). **Verify** whether `PermissionRequest` fires before or during that review. If it does, it doesn't always mean a human is needed, and `Notification` is the better "you have the ball" signal. | n/a |
| Tool approval granted | n/a | `ToolUseStart` ("an approved tool call crosses its execution-start boundary") (docs). Its payload is undocumented: **verify** that it carries `tool_use_id`. Fallbacks are `PostToolUse` and `PostToolUseFailure` by `tool_use_id`. **Those are skipped when the payload exceeds 256 KiB** (docs), which is likely for big tool outputs. |
| Tool approval denied | n/a | **No documented hook** (Claude's `PermissionDenied` has no equivalent). **Verify** whether denial produces `PostToolUseFailure`. Otherwise the request closes on the same thread's next `PreToolUse` after one second, or when its turn or agent ends, as in the design. |
| Questions to the user | MSP has `userInput/requested` and `userInput/settled`. One to three questions, tied to a tool call, with an **auto-resolution timeout** (`autoResolutionMs`) (docs). For hooks, that means `PreToolUse` on the ask tool, whose name is **unknown: verify**. | `PostToolUse` or `PostToolUseFailure` of that `tool_use_id` |
| MCP input requests | **Not observable via hooks** (docs). | n/a |
| Idle / waiting notifications | Only `Notification` / `permission_prompt` (docs) | n/a |
| Ordering and concurrency | All matching handlers for one occurrence start together (docs). Parallel tool calls each fire their own hooks, so they presumably overlap (inference). `PreToolUse` precedes `PermissionRequest` for the same call (docs: `ask` forces the prompt). Records can arrive out of order exactly as with Claude (inference). | n/a |

## 5. Process and identity

- **Launcher, not symlink.** `~/.local/bin/muse` is a ~34 KB **bash script**,
  not a symbolic link (local install). It keeps the real binary *beside itself*
  as `~/.local/bin/muse-bin-<version>`, where `<version>` comes from
  `~/.local/bin/.muse-version`, currently `muse-bin-1.4.23-R8977.1`, a 470 MB
  Mach-O arm64 file (local install, source code). It can update itself and the
  binary in the background (source code).
- **What the process table shows.** The launcher ends with `exec "$binary" "$@"`,
  without `exec -a` (source code). So the running process has:
  - executable path `~/.local/bin/muse-bin-1.4.23-R8977.1`
  - **`argv[0]` = that same full path**, not `muse`
  - executable file name `muse-bin-1.4.23-R8977.1`. The kernel's short `comm` is
    truncated to `muse-bin-1.4.23-` (inference from macOS MAXCOMLEN).

  **Neither rule in Cusco's matcher ("file name is the client name, or `argv[0]`
  is") matches.** The Muse adapter needs a rule such as "file name starts with
  `muse-bin-`" (inference).
- **The process chain from a hook.** `cusco hook muse` ← `$SHELL -c` (zsh here)
  ← `muse-bin-…` (docs for the shell, inference for the rest). When a hook fires
  inside a subagent, the hosting process is the same one: subagents are child
  *sessions* inside the runtime, not separate processes (inference).
- **Surface from argv.** `argv[1]`:
  - none, a prompt, or `resume`: `cli`
  - `exec`: `cli`, headless
  - `acp`: `ide`
  - `serve`: an SDK host, possibly a desktop app
  - `serve --listen`: the shared daemon

  Mapping this way is inference. `PROVENANCE_PROVIDER_SCOPE` reaches hooks when
  the launching process set it and is documented as "a label naming that
  surface" (docs). Its values are unknown.
- **One process, or shared?** The TUI and `exec` run one process per session
  (inference). `muse serve`, `muse acp` and `muse serve --listen` host **many
  sessions in one process** (docs). Process exit then closes all of them, but
  process liveness can't tell those sessions apart. This is the same open
  question the design has for Claude Desktop.
- **No pid or surface in payloads** (docs). Each running session advertises
  itself in a registry with a socket lease that does carry a `pid` (section 6).
- **No Muse processes were running** during this inspection (local install).

## 6. Persistent state

Data directory: `$XDG_DATA_HOME/muse/`, by default `~/.local/share/muse/`. The
host reports the real path as `museHome` in the MSP `initialize` result (docs).
The docs call the folder layout an "implementation detail", so a program should
use reported paths (docs).

- **Session log (durable transcript).**
  `sessions/<YYYY>/<MM>/<DD>/<session id>/session.jsonl`, with a `.session.lock`
  writer lease beside it (docs). Locally, the session folder also holds
  `cron.db`, `approval-review/`, a `cli-<uuid>.log`, `session.peer-history.index`
  and crash-telemetry marker files (local install).
  - **Child sessions** live at
    `<parent session dir>/subagent/<child session id>/session.jsonl` (local
    install).
  - **Record envelope fields:** `id`, `sequence`, `stream {id, kind}` (kind
    `session`), `record_type` (`event`), `payload_type`, `payload`,
    `payload_schema_version`, `recorded_at`, `causation_id`, `durability`,
    `schema_version`. There are also `retained_marker` records and framed
    `children` transaction records (local install).
  - **Payload types useful to Cusco:** `session.started` (with
    `background_tasks`), `session.end` (`exit_reason`, `uptime_ms`, …),
    `approval_wait.effect.started` / `.terminal`, `tool_batch.effect.started` /
    `.terminal`, and `runtime.session`, whose `payload.event.kind` includes:
    - `started` and `terminal` (`terminal`: `completed | failed | cancelled`)
    - `requested` (approval: `pending_action_id`, `current_requirement_id`,
      `presentation_phase`, `run_stream`, …) and `decision_applied`
    - `automated_review_started` and `automated_review_completed`
    - `task_stream_linked` (`execution_mode`: `foreground | background`)
    - `memory_reminder_child_session_linked` (`child_session_id`,
      `parent_session_id`, …)
    - `hook_run_started` and `hook_run_terminal` (local install)

    Most other payloads carry prompt, response or reasoning text and must never
    be read.
  - **Recovery:** an open turn is a `started` record with no `terminal`, and a
    pending approval is a `requested` record with no `decision_applied` (local
    install for the shapes, docs for "the pending set is a log read"). After a
    crash the runtime re-opens pending approvals under **new** deterministic
    approval ids (docs).
- **Session index.** `session-index.db` is SQLite in WAL mode. Its `sessions`
  table includes:
  - `session_id`, `session_log_path`, `workspace_root`, `status`
  - `msp_kind` (`root | subagent`), `msp_parent_session_id`,
    `msp_spawned_by_parent_session_id`
  - `msp_last_activity_at_us`, `msp_turn_count`, `latest_segment_terminated`
  - `msp_created_by_client_name`, `msp_fork_source_session_id`

  It also has `title`, `first_user_prompt` and `search_text`, which **contain
  prompt text, so never select them**. All 6 local rows are `root` (local
  install), so children may not be indexed.
- **Running-session registry.** `runtime/muse/sessions/<session id>.json` has the
  fields `schema_version`, `session_id`, `session_name`, `endpoint_hint`,
  `workspace_label`, `target_eligibility` and `process_generation_hint`.
  `runtime/muse/ms-<hash>.sock` comes with an `ms-<hash>.sock.lease` holding
  `schema_version`, `endpoint_hint`, `process_generation_hint` and **`pid`**
  (local install). **All four leases on this Mac named dead pids, and one
  registry entry was left behind with no Muse process running** (local install).
  Stale entries are normal, so liveness must be checked: pid plus the lease's
  OS file lock (inference). `muse sessions --json --watch` is the documented
  reader of this registry (local `--help`).
- **Config.**
  - `~/.config/muse/settings.json` has top-level keys `schema_version`, `hooks`,
    `model`, `provider`, `tui` and others.
  - `trust.json` has `schema_version` and `projects`.
  - `auth.json` holds credentials and was not opened.
  - `.lock` files sit beside each (local install).
- **Do not read:** `tui-history.jsonl` (prompt history) and `local-tracing/`
  (private diagnostic logs) (local install, docs).

**What recovery could do (inference).** On start, `muse sessions --json` or the
registry, checked against live pids, lists running sessions and their ids. Each
session's `session.jsonl` then gives its open turn and pending approvals. That
is a full `sync` without waiting for the next hook.

## 7. Proposed adapter

The hook command adds two env vars to the record: `muse_session_id` and
`muse_thread_id`, taken from `MUSE_SESSION_ID` and `MUSE_THREAD_ID`. The adapter
keys each session by `muse_session_id`, falling back to `session_id`. The
**actor** is the main agent when `muse_thread_id == muse_session_id`, and
otherwise the subagent `muse_thread_id`.

| Hook | Cusco event |
| --- | --- |
| `SessionStart` | Main thread only: `open` session. `resume` reopens an ended session. `clear`: if the id changed, close the session previously running in the same process (**verify**). `fork`: a new session. `compact`: no change. |
| `UserPromptSubmit` | Main thread: `open` turn keyed by `turn_id`, and close any *other* open turn in the session as `cancelled`. A repeated `turn_id`, which a steer might produce, is a no-op (**verify** steer and queue timing). Child thread: confirm the agent. |
| `PreToolUse` | Remember `tool_use_id` against its input digest, per actor. Implicitly opens the turn (`turn_id`) and, in a child thread, the agent. If the tool is the ask-user tool (name to verify), `open` a request with reason `question`, keyed by `tool_use_id`. The design's one-second rule then answers an older approval from the same actor. |
| `PermissionRequest` | `open` a request with reason `approval` under the actor, keyed by the `tool_use_id` whose digest matches (the payload has none). |
| `Notification` (`permission_prompt`) | Set `attr {prompted: true}` on the actor's open approval. With none open, `open` one keyed `turn:<turn_id>` (verify). |
| `ToolUseStart` | `close` the approval for that call as `ok`, if the payload carries `tool_use_id` and settings accept the event (**verify both**). |
| `PostToolUse`, `PostToolUseFailure` | `close` the request with that `tool_use_id`, if one is open. |
| `SubagentStart` | `open` agent keyed by `child_session_id` under the event's `turn_id`. Record `subagent_id` and the parent thread as attributes. |
| `SubagentStop` | `close` that agent as `ok`. |
| `Stop` | Main thread: `close` turn `turn_id` as `ok`. Child thread: confirm the agent only (**verify** whether it fires there). |
| `StopFailure` | `close` the turn, or the agent when `agent_id` is present or the thread is a child, as `error`, with the `error` category. |
| `SessionEnd` | Main thread: `close` session, keeping `reason`. |

**Not registered:**
- `PostToolBatch`: its payload is the biggest, so the 256 KiB cap makes it the
  least reliable.
- `PreLLMCall`, `PostLLMCall`, `PreCompact`, `PostCompact`, `SessionFork`.

**Muse-specific rules:**
- **A closed turn can come back.** Because `Stop` and `SubagentStop` can be
  blocked by other hooks, a later event carrying the *same* `turn_id` (or the
  same child thread) is proof the turn or agent is still running. This
  contradicts the design's "never reopen a closed turn", which exists because
  Claude records lack a reliable turn id. Muse has one on every event. Either
  allow reopen-by-same-id for Muse, or accept a false `idle` until the next
  prompt.
- **Interrupted turns.** With no interrupt or idle hook, add transcript sync
  early for Muse: watch `transcript_path`, or the registry's session log, for
  `terminal` records.

**`observes`:** `["session", "turn", "agent", "request"]`. Approvals are
attributed per thread through `MUSE_THREAD_ID` once that is verified. Until
then, use `request:turn`. MCP input requests are not observable, and interrupts
are only inferred.

**Allowed fields for `cusco hook muse`:**
- From stdin: `hook_event_name`, `session_id`, `turn_id`, `cwd`,
  `transcript_path`, `source`, `reason`, `notification_type`, `subagent_id`,
  `child_session_id`, `agent_id`, `agent_type`, `tool_use_id`, `tool_name`,
  `error` (only on `StopFailure`, where it is a category).
- From the environment: `MUSE_SESSION_ID`, `MUSE_THREAD_ID`,
  `PROVENANCE_PROVIDER_SCOPE`.
- Never forward `prompt`, `tool_input` (send its digest), `tool_response`,
  `last_assistant_message`, `error_details`, or `Notification`'s
  `title`/`message`, which may quote the command being approved (inference).

**`cusco setup`** writes into the `hooks` member of
`${XDG_CONFIG_HOME:-~/.config}/muse/settings.json`. It keeps every existing key
and the other tools' handlers, keeps `schema_version`, writes atomically, and
uses one group with no matcher per event:

```json
{ "type": "command", "command": "/absolute/path/to/cusco hook muse", "timeout": 5 }
```

Events to register: `SessionStart`, `UserPromptSubmit`, `PreToolUse`,
`PermissionRequest`, `Notification`, `ToolUseStart` (only if the installed
version accepts it, see below), `PostToolUse`, `PostToolUseFailure`,
`SubagentStart`, `SubagentStop`, `Stop`, `StopFailure` and `SessionEnd`.

- Use an absolute command path: `PATH` is inherited from whatever launched Muse,
  which differs between a terminal, an editor and launchd (docs, inference).
- Leave `async` unset. Cusco's hook exits in milliseconds, and synchronous
  handlers keep event order closer to real order (inference).
- The hook must **print nothing and always exit 0**.
- An unknown event name skips only that event with a warning (docs), but a
  wrong-typed field kills the whole file. Setup should check the result with a
  dry run (`muse exec --provider echo` prints hook warnings at startup (docs)).
  That needs the user's approval, because it starts a session.
- `cusco doctor` should report "installed; running sessions started before
  install won't report until restarted."

## 8. Gaps and verify items

To record a session, put a logging hook in an isolated config: set
`XDG_CONFIG_HOME` and `XDG_DATA_HOME` to scratch folders, then in the hooks
member add a command such as
`{ cat; printf '\n%s %s\n' "$MUSE_SESSION_ID" "$MUSE_THREAD_ID"; } >> events.log`
on all 19 events. `muse exec --provider echo` fires `SessionStart`,
`UserPromptSubmit` and `Stop` without a model (docs). Everything involving
tools, approvals and subagents needs a real model run in the TUI.

1. **Child identity.** Inside a subagent, is `session_id` the child id or the
   root? Is the child's `MUSE_THREAD_ID` equal to `child_session_id`? How do
   `subagent_id` and `agent_id` relate to them? Spawn one subagent that runs one
   tool needing approval.
2. **Which events fire in a child thread.** `UserPromptSubmit`, `Stop` and
   `SessionStart` in particular.
3. **`ToolUseStart` and `SessionFork`.** Their payload keys, and whether
   `settings.json` accepts them in 1.4.23. The settings guide says 1.3.0 accepts
   only 17 names.
4. **Denial.** What fires when a user denies an approval: `PostToolUseFailure`
   with an error, or nothing?
5. **Automated reviewer.** Does `PermissionRequest` fire while the `llm_judge`
   reviewer decides? Does `Notification` fire only when a human prompt is
   actually shown? Compare `PermissionRequest` → `Notification` timing on an
   auto-approved call and on a human-approved call.
6. **Interrupt.** Press Esc mid-turn. Does `Stop`, `StopFailure` or anything
   fire? Does the next prompt carry a new `turn_id`?
7. **Queue and steer.** Submit while a turn runs. When does `UserPromptSubmit`
   fire, and with which `turn_id`? If it fires at submission with a new id, the
   "close the other open turn" rule would wrongly cancel the running turn.
8. **`/clear`.** Is the session id new or kept?
9. **`SessionEnd`.** Its `reason` values. Does it fire on idle unload in
   `serve` and `acp`, on SIGINT, and on terminal close (SIGHUP)?
10. **Background and reminder agents.** Does `SubagentStop` fire for background
    subagents? Do reminder agents (skill, goal, verify) fire `SubagentStart`? If
    they do, Cusco shows `working` after the user's turn looks done. Also check
    whether `Stop` fires at `turn/foregroundCompleted` or at the real terminal.
11. **Questions.** The ask-user tool's name, whether `PreToolUse` fires for it,
    and what happens on its auto-resolution timeout.
12. **MCP elicitation.** No hook is documented. Confirm it is invisible.
13. **256 KiB cap.** Run a tool with large output and confirm `PostToolUse` is
    skipped. This is the case that makes `ToolUseStart` important.
14. **Surface label.** `PROVENANCE_PROVIDER_SCOPE` values under the TUI,
    `acp` (Zed) and `serve` (SDK).
15. **Process tree.** Does `muse-bin-…` spawn helpers that are also
    `muse-bin-…` (sandbox, `mxp` executor, `attach`)? They would confuse the
    supervisor rule. How do updates behave with a session open: the next launch
    runs a new `muse-bin-<new>` while the old process keeps its own path?
16. **Registry staleness.** Does `muse sessions --json` filter the dead-pid
    entries seen here? Is it cheap enough to keep running with `--watch`?
17. **`transcript_path`.** When it is `null`: the echo provider showed `null`.
    Does it always point at `session.jsonl` for real sessions?
18. **Docs lag.** The hook reference was checked against 1.3.0, and 1.4.23 is
    installed. Re-check the 17 accepted names and the common keys against a
    recording.
19. **Settings lock.** How Muse uses `.settings.json.lock`, so that `cusco
    setup` doesn't race a running TUI that rewrites settings (for example on a
    model change).

## 9. Sources

All retrieved on 18 November 2026.

**Official docs**
- https://meta-models.github.io/muse-code-sdk/ (redirects to `latest/`, which redirects to `1.4/`)
- https://meta-models.github.io/muse-code-sdk/versions.json
- https://meta-models.github.io/muse-code-sdk/1.4/guides/plugins/reference/hook-events/
- https://meta-models.github.io/muse-code-sdk/1.4/guides/extend/hooks/
- https://meta-models.github.io/muse-code-sdk/1.4/guides/plugins/concepts/hooks/
- https://meta-models.github.io/muse-code-sdk/1.4/guides/plugins/concepts/trust-review-and-scopes/
- https://meta-models.github.io/muse-code-sdk/1.4/guides/plugins/concepts/plugins-in-sdk-sessions/
- https://meta-models.github.io/muse-code-sdk/1.4/guides/msp-concepts/
- https://meta-models.github.io/muse-code-sdk/1.4/guides/msp-concepts/sessions-and-turns/
- https://meta-models.github.io/muse-code-sdk/1.4/guides/msp-concepts/approvals/
- https://meta-models.github.io/muse-code-sdk/1.4/guides/host-operations/
- https://meta-models.github.io/muse-code-sdk/1.4/guides/editors-acp/
- https://meta-models.github.io/muse-code-sdk/1.4/guides/quickstart/
- https://meta-models.github.io/muse-code-sdk/1.4/reference/
- https://meta-models.github.io/muse-code-sdk/1.4/generated/changelog/
- https://meta-models.github.io/muse-code-sdk/1.4/generated/msp/types/sessionkind/
- https://meta-models.github.io/muse-code-sdk/1.4/generated/msp/types/sessionspawnedby/
- https://meta-models.github.io/muse-code-sdk/1.4/generated/msp/types/approvalsubagentorigin/
- https://meta-models.github.io/muse-code-sdk/1.4/generated/msp/types/attentionflag/
- https://meta-models.github.io/muse-code-sdk/1.4/generated/msp/types/sessionstatus/
- https://meta-models.github.io/muse-code-sdk/1.4/generated/msp/notifications/session-statuschanged/
- https://meta-models.github.io/muse-code-sdk/1.4/generated/msp/notifications/userinput-requested/
- https://meta-models.github.io/muse-code-sdk/1.4/generated/msp/notifications/turn-foregroundcompleted/
- https://meta-models.github.io/muse-code-sdk/1.4/generated/msp/methods/serve-isidle/
- https://meta-models.github.io/muse-code-sdk/next/guides/plugins/reference/hook-events/ (compared with 1.4; identical apart from the "Developer Preview" banner)
- https://meta-models.github.io/muse-code-sdk/next/guides/extend/hooks/ (same)

**Local, read-only**
- `muse --version`, `muse --help`, `muse serve --help`, `muse sessions --help`, `muse attach --help`
- `~/.local/bin/muse` (the launcher script: `active_binary`, `resolve_install_dir`, `main`), `~/.local/bin/.muse-version`, `~/.local/bin/muse-bin-1.4.23-R8977.1` (file type only)
- `~/.config/muse/` (listing; `settings.json` keys and hook structure only; `trust.json` shape only; `auth.json` not opened)
- `~/.local/share/muse/` (listing; `sessions/**/session.jsonl` key names and kind values only; `session-index.db` schema and enumerated columns only; `runtime/muse/sessions/*.json` and `*.sock.lease` key names, plus a pid liveness check)
- `/Applications/Muse.app/Contents/Info.plist`, `Contents/MacOS/Muse` and `Contents/Resources/hatch/index.html` (string searches only); `~/Library/Application Support/Muse/` (listing)
- `~/Developer/wooting-glance/src/agents.rs` (the `Hook` struct, `muse_event` and the `muse_child_approval_survives_parent_stop_and_clears_on_teardown` test)
- `~/Developer/cusco/docs/design.md`
