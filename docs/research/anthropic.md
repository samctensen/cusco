# Claude Code and Claude Desktop (Code tab): research briefing for Eywa

**18 November 2026.** Basis markers: (docs) = official Claude Code docs, read today;
(source code) = the JavaScript embedded in the Claude Code 2.1.325 binary that Claude
Desktop ships, read as text; (local install) = read-only inspection of this Mac;
(inference) = reasoning from the above, not observed.

## 1. Product and surfaces

| Surface | What runs | Versions seen | Runs hooks from `~/.claude/settings.json` |
| --- | --- | --- | --- |
| CLI, native installer | `~/.local/bin/claude` is a symlink to `~/.local/share/claude/versions/<version>` | 2.1.282, 2.1.294, 2.1.301 installed; the link points at 2.1.301 (local install) | Yes (docs), after the workspace trust dialog is accepted for the folder (docs) |
| Claude Desktop, Code tab (local sessions) | One `claude` process per session at `~/Library/Application Support/Claude/claude-code/<version>/<build-hash>/claude.app/Contents/MacOS/claude`. Note the extra `<build-hash>` directory (local install) | Bundled Claude Code 2.1.325, Agent SDK 0.3.325, Desktop app 5.11492.x (local install) | Yes (local install: Desktop transcripts contain `system` records with subtype `stop_hook_summary`, written when Stop hooks run). Desktop hosts Claude Code through the Agent SDK `canUseTool` callback (docs) |
| Desktop SSH sessions | `claude` runs on the remote host | not checked | Remote host's `~/.claude` (docs, by analogy with skills; inference for hooks). Not observable locally |
| VS Code extension | Hosts Claude Code through the Agent SDK like Desktop (docs). Bundled binary path not checked | not checked | Yes; the extension shows "hooks loaded in the session" (docs) |
| JetBrains | Runs the CLI in the IDE terminal (inference) | not checked | Yes, as CLI (inference) |
| Background sessions / agent view (`claude --bg`, `claude agents`) | Each session is its own Claude Code process under a supervisor process (`claude daemon`) (docs) | | Yes (docs: agent view rows report hook output) |
| Headless `claude -p` and Agent SDK | Same binary, non-interactive | | Yes, and with no trust dialog: a folder is treated as trusted (docs). `--bare` skips all hooks (docs) |
| Cloud sessions (claude.ai/code, `--cloud`) | Remote VM | | No: they don't read your local `~/.claude/settings.json` (docs). Not observable |
| Remote Control | A local session driven from claude.ai | | Yes, it is a local session; hook subprocesses get `CLAUDE_CODE_BRIDGE_SESSION_ID` (docs) |

Hooks are disabled for a run by `disableAllHooks` (respecting precedence),
`--settings '{"disableAllHooks": true}'`, `--bare`, or managed `allowManagedHooksOnly`
(blocks user, project, local and plugin hooks) (docs). In an interactive session in a
folder the user hasn't trusted, *all* settings-file hooks, including the user's own,
are held back until trust is accepted (docs). Each of these makes Eywa silently blind
for that session.

## 2. Extension mechanism

**Locations and precedence (docs).** Hooks can come from `~/.claude/settings.json`,
`.claude/settings.json`, `.claude/settings.local.json`, managed policy (file, MDM or
server-managed), plugin `hooks/hooks.json`, skill frontmatter (rest of session after
the skill runs) and subagent frontmatter (while that subagent runs). Hook entries
**merge** across levels instead of overriding; the same handler defined in several
settings files runs once, while plugin and skill copies stay separate. General settings
precedence is managed > command line > local project > shared project > user.
Directly edited settings are picked up by a file watcher (docs), so `eywa setup`
needs no restart. On this Mac, `~/.claude/settings.json` already contains command
hooks from another local tool on nine events (2 s timeouts), and there is no
managed-settings directory (local install). `eywa setup` must merge, not replace.

**Format (docs).** `hooks.<Event>[] = { matcher?, hooks: [handler…] }`. Handler types:
`command`, `http`, `mcp_tool`, `prompt`, `agent`. A `command` handler has `command`,
optional `args` (exec form: spawned directly with no shell), `async`, `asyncRewake`,
`shell`, `onFailure` (v2.1.295+), plus common `if`, `timeout`, `statusMessage`.

**Matchers (docs).** `*`, empty or absent matches all. A value of only letters,
digits, `_ - , |` and spaces is an exact-name list; anything else is an unanchored
JavaScript regex. Tool events match `tool_name`; `SessionStart` matches `source`;
`SessionEnd` matches `reason`; `Notification` matches `notification_type`;
`SubagentStart`/`SubagentStop` match `agent_type`; `StopFailure` matches `error`;
`Elicitation*` match the MCP server name. `UserPromptSubmit`, `PostToolBatch`, `Stop`,
`TeammateIdle`, `TaskCreated`, `TaskCompleted`, `Worktree*`, `MessageDisplay` have no
matcher. Eywa should omit matchers.

**Invocation (docs).** The JSON payload arrives on stdin. Command hooks run in the
session's current directory with Claude Code's environment, in their own session with
no controlling terminal. Environment variables set in hook subprocesses:
`CLAUDE_PROJECT_DIR`, `CLAUDECODE=1`, `CLAUDE_CODE_CHILD_SESSION=1`,
`CLAUDE_CODE_SESSION_ID` (matches the payload's `session_id`, updated on `/clear`),
**`CLAUDE_PID` (the Claude Code process id, v2.1.214+)**, `CLAUDE_EFFORT`,
`CLAUDE_CODE_REMOTE=true` in cloud sessions, `CLAUDE_CODE_BRIDGE_SESSION_ID` under
Remote Control, `CLAUDE_JOB_DIR` in background sessions. `CLAUDE_ENV_FILE` only on
SessionStart, Setup, CwdChanged and FileChanged. Undocumented but present in a Desktop
session's subprocess environment: `CLAUDE_CODE_ENTRYPOINT=claude-desktop`,
`CLAUDE_CODE_EXECPATH`, `CLAUDE_CODE_HOST_SESSION_ID`, `AI_AGENT` (local install).

**Timeouts (docs).** Default 600 s for `command`; 30 s on `UserPromptSubmit`,
`PreModelSwitch`, `PostModelSwitch`; 10 s on `MessageDisplay`; `SessionEnd` hooks share
a 1.5 s budget. Async hooks have no enforced timeout.

**Concurrency (docs).** All matching handlers for one event run in parallel.
`PostToolUse` fires concurrently for parallel tool calls. Synchronous hooks block the
action that triggered them, so hooks of successive phases (PreToolUse, then
PostToolUse, then Stop) are ordered (inference). `Notification`, `CwdChanged`,
`FileChanged`, `ConfigChange`, `InstructionsLoaded`, `Worktree*`, `PostModelSwitch` are
"standalone async events" and can interleave anywhere (docs, lifecycle diagram).
Background subagents and in-process teammates produce events concurrently with the
main thread (inference).

**Output and exit codes (docs).** Exit 0: stdout is parsed as one JSON object, else
treated as plain text. Exit 2: blocking error on events that can block. Other codes:
non-blocking error. Events where output changes behavior: `PreToolUse` (allow, deny,
ask, defer, `updatedInput`), `PermissionRequest` (decision object; exit 2 ignored),
`UserPromptSubmit` (block, `additionalContext`), `UserPromptExpansion`, `Stop` and
`SubagentStop` (block to continue), `TeammateIdle`, `TaskCreated`, `TaskCompleted`,
`ConfigChange`, `PostToolBatch` (stop the loop), `PreCompact`, `PreModelSwitch`,
`Elicitation` and `ElicitationResult` (answer or decline), `WorktreeCreate`/`Remove`,
`SessionStart` and `SubagentStart` (`additionalContext`), `PermissionDenied`
(`retry`), `MessageDisplay` (rewrite text). Observational only: `PostToolUse`,
`PostToolUseFailure` (stderr to Claude on exit 2), `Notification`, `StopFailure`,
`SessionEnd`, `CwdChanged`, `PostCompact`, `PostModelSwitch`, `InstructionsLoaded`.
Eywa's hook must print nothing and exit 0 on every path (inference from the above).
It should be synchronous (not `async`) so cross-phase ordering holds, use exec form
(`command` + `args`) to avoid a shell, and set a short `timeout`.

**Trust step (docs).** Interactive sessions hold back every settings-file hook until
the folder is trusted. There is no per-hook trust or hash pinning as in Codex.

**Non-hook integrations.**
- `claude agents --json` (docs): "the supported way to read session state from
  outside Claude Code." Per live session: `cwd`, `kind` (`interactive`/`background`),
  `startedAt`, `pid`, `status` (`busy`/`waiting`/`idle`), `waitingFor`
  (`permission prompt`, `input needed`, `sandbox request`, `worker request`,
  `dialog open`), `sessionId`, `name`; background sessions add `id` and `state`
  (`working`/`blocked`/`done`/`failed`/`stopped`).
- Its backing store appears to be `~/.claude/sessions/<pid>.json` (undocumented;
  local install): one file per live process with `pid`, `sessionId`, `cwd`,
  `startedAt`, `procStart`, `version`, `kind`, `entrypoint`, `status`,
  `statusUpdatedAt`, `updatedAt`, `name`, `nameSource`, and others. All seven Desktop
  sessions on this Mac had a file named after their pid. Sibling `*.key` files are
  secrets; never read them.
- OpenTelemetry (docs): events `claude_code.user_prompt`, `tool_decision`
  (`tool_use_id`, `decision` accept/reject, `source` config/hook/user_temporary/…),
  span `claude_code.tool.blocked_on_user` (`decision` accept/reject/cancelled,
  `duration_ms`), `subagent_completed` (`is_async`), all with `prompt.id`. This is the
  only documented source that reports a *manual* approval or denial with its
  `tool_use_id`, but it needs the user to export telemetry to a local OTLP receiver.
- Headless `stream-json` with `--include-hook-events` (docs) only covers processes
  Eywa itself launches. Not useful for observing the user's sessions.
- Status line (docs): a per-session script fed session JSON on stdin. Terminal UI
  only; not a reliable event source.

## 3. Event catalog

Common fields on every payload (docs; source code for optionality): `session_id`,
`transcript_path`, `cwd`, `hook_event_name`, optional `prompt_id`, `scratchpad_dir`
(v2.1.257+), `permission_mode` (some events), `effort` (events with a tool-use
context), `agent_id` and `agent_type` (inside a subagent or in-process teammate).

| Event | Fires when | Event-specific fields (docs unless marked) |
| --- | --- | --- |
| `SessionStart` | Session begins, resumes, clears, compacts or forks | `source` (`startup`/`resume`/`clear`/`compact`/`fork`), `model?`, `agent_type?` (with `--agent`), `session_title?`; on resume/fork also `seconds_since_last_response`, `context_tokens`, `prompt_cache_likely_expired`, `estimated_cache_write_usd` |
| `Setup` | `--init-only`, or `--init`/`--maintenance` with `-p` | `trigger` (source code) |
| `InstructionsLoaded` | A CLAUDE.md or rules file is loaded | load reason (fields not reviewed) |
| `UserPromptSubmit` | A prompt is submitted, including scheduled tasks, `/loop`, **background subagents reporting back**, cross-session messages, Code Mode output | `prompt` (never forward), `session_title?`; **`source`**: `user`, `sdk`, `system`, `loop_wakeup`, `schedule_wakeup`, `poll_event` (source code; undocumented, "payloads may omit it while the field rolls out") |
| `UserPromptExpansion` | A typed slash command expands | `expansion_type`, `command_name`, `command_args`, `command_source?`, `prompt` (source code) |
| `MessageDisplay` | Assistant text streams to the screen | message text (fields not reviewed) |
| `PreToolUse` | Before every tool call, permission needed or not (not `EndConversation`) | `tool_name`, `tool_input`, **`tool_use_id`**, `mcp_server?` |
| `PermissionRequest` | A permission dialog is about to show, or a call that can't prompt would be auto-denied | `tool_name`, `tool_input`, `mcp_server?`, `permission_suggestions?`. **No `tool_use_id`** |
| `PostToolUse` | A tool call **completed** successfully | `tool_name`, `tool_input`, `tool_response`, `tool_use_id`, `duration_ms?`. For `Agent`: `tool_response.status` (`completed`/`async_launched`) and `tool_response.agentId` |
| `PostToolUseFailure` | A tool that started executing failed | `tool_name`, `tool_input`, `tool_use_id`, `error` (**tool output text**), `is_interrupt?`, `duration_ms?`. Not fired for validation errors or permission denials |
| `PostToolBatch` | Once after every call in a batch resolved, before the next model request | `tool_calls[]` of `{tool_name, tool_input, tool_use_id, tool_response}` |
| `PermissionDenied` | **Auto mode** denies a call (not manual denials, not deny rules, not PreToolUse blocks) | `tool_name`, `tool_input`, `tool_use_id`, `reason` |
| `Notification` | Claude Code raises a notification | `message`, `title?`, `notification_type`: `permission_prompt` (~6 s after a prompt appears), `idle_prompt` (~60 s after finishing, never while a background agent runs), `elicitation_dialog`, `elicitation_url_dialog`, `elicitation_complete`, `elicitation_response`, `agent_needs_input`, `agent_completed`, `auth_*`, `quota_auto_resume_*`, `plugin_notification` |
| `SubagentStart` | A subagent is spawned, **resumed**, or an in-process teammate handles **each new message** | `agent_id`, `agent_type` |
| `SubagentStop` | A subagent finished responding; also internal agents (prompt suggestions, `/btw`) | `agent_id`, `agent_type` (empty or the session's `--agent` for internal agents), `agent_transcript_path`, `last_assistant_message`, `stop_hook_active`, `background_tasks`, `session_crons` (both scoped to the parent session) |
| `TaskCreated` / `TaskCompleted` | A TaskCreate/TaskUpdate to-do item is created / completed | `task_id`, `task_subject`, `task_description?`, `teammate_name?`, `team_name?` (deprecated), `agent_id?` (v2.1.290+). These are to-do items, not background tasks |
| `Stop` | The main agent finished responding. **Not on user interrupt**; API errors fire `StopFailure` | `stop_hook_active`, `last_assistant_message`, `background_tasks[]` (`id`, `type`, `status`, `description`, plus `command`/`agent_type`/`server`/`tool`/`name` by type), `session_crons[]` |
| `StopFailure` | The turn ended on an API error | `error` (enum: `rate_limit`, `authentication_failed`, …, `unknown`), `error_details?`, `last_assistant_message?` |
| `TeammateIdle` | An agent-team teammate is about to go idle | `teammate_name`, `team_name` (deprecated), `agent_id?` (v2.1.290+) |
| `ConfigChange` | A settings file changes mid-session | configuration source |
| `CwdChanged` | Working directory changes | (fields not reviewed) |
| `DirectoryAdded` | `/add-dir` or SDK `register_repo_root` | how added |
| `FileChanged` | A watched file changes | (fields not reviewed) |
| `WorktreeCreate` / `WorktreeRemove` | Worktree created / removed | (fields not reviewed) |
| `PreCompact` / `PostCompact` | Around compaction | trigger `manual`/`auto` |
| `PreModelSwitch` / `PostModelSwitch` | Around a model change | `from_model`, `to_model` |
| `SessionEnd` | Session terminates | `reason`: `clear`, `resume`, `logout`, `prompt_input_exit`, `other` |
| `Elicitation` | An MCP server requests input during a tool call | `mcp_server_name`, `message`, `mode?` (`form`/`url`), `url?`, **`elicitation_id?` (optional; absent in the documented form example)**, `requested_schema?` |
| `ElicitationResult` | The user answered an elicitation (not when a hook answered it) | `mcp_server_name`, `action`, `mode?`, `elicitation_id?`, `content?` (never forward) |

**Identifiers.**
- `session_id`: a UUID. For a call served on behalf of a remote caller the source
  builds `served:<caller session id or "unknown">` with an empty `transcript_path`
  (source code).
- `prompt_id`: a UUID, the same value as OTel `prompt.id`. It is process-global:
  "absent until the first user input of the process lifetime" (source code), and
  every event, including a background subagent's, carries the *current* prompt's id,
  not the id of the turn that launched the agent (source code: one global getter).
  Each background-agent report gets a new prompt id (local install: every
  `<task-notification>` user record in recent transcripts had a fresh `promptId`).
- `agent_id`: "present only when the hook fires from within a subagent … absent for
  the main thread, even in `--agent` sessions" (source code); also identifies
  in-process teammates (docs). Docs examples look like `a4d2c8f1e0b3a297`.
- `tool_use_id`: `toolu_…`, on PreToolUse, PostToolUse, PostToolUseFailure,
  PermissionDenied, PostToolBatch entries. Not on PermissionRequest.
- `background_tasks[].id`: for subagent tasks this **is the `agent_id`** (source code:
  the task record is created with `id` set to the agent id). A backgrounded main
  session appears as a `subagent`-typed task with agent type `main-session` and an id
  of the form `s` + 8 characters (source code).
- `elicitation_id` (optional), `task_id` (to-do items), `session_crons[].id`,
  `teammate_name`. No request id reaches hooks; OTel carries the API `request_id`.

## 4. Mapping to Eywa's model

**Session.**
- Start: `SessionStart`. `startup` = new id. `resume` (`--resume`, `--continue`,
  `/resume`) **reuses** the resumed session's id (docs: transcript per session id;
  `--fork-session` exists to get a new id instead). `clear` = **new** id: the previous
  session is saved and resumable, and `CLAUDE_CODE_SESSION_ID` "is updated on /clear"
  (docs). `compact` = same id (local install: `compact_boundary` records sit inside
  the same transcript file). `fork` (`--fork-session`, `/branch`, `/fork`, moving to
  the background) = new id (docs).
- End: `SessionEnd` (`clear` and in-session `/resume` end the old session in the same
  process; `prompt_input_exit`, `logout`, `other`). Killed processes send nothing, so
  process exit stays necessary. A background session's process may be stopped by the
  supervisor after about an hour idle, and restarted on reply or after a crash (docs).
  Process exit then doesn't mean the conversation ended, only that it's idle (inference).

**Turn.**
- Start: `UserPromptSubmit` keyed by `prompt_id`. Use `source` to label the turn's
  origin: `user` (CLI composer) and `sdk` (Desktop, VS Code and `-p`; local install:
  Desktop prompts are recorded with `promptSource: sdk`) versus `system`,
  `loop_wakeup`, `schedule_wakeup`, `poll_event` (machine-initiated).
- End: `Stop` (ok) or `StopFailure` (error). **Stop is not final**: another Stop hook
  or `/goal` can block it, and the turn continues with the same `prompt_id`; the next
  `Stop` then has `stop_hook_active: true` (docs).
- Interrupt: no hook (docs). Closers: the next `UserPromptSubmit`; `idle_prompt` about
  60 s later, but only if no background agent is running (docs) and possibly never in
  SDK-hosted sessions such as Desktop (inference, verify); `SessionEnd`. A better
  closer is the session registry's `status` turning `idle` (section 2).
  `PostToolUseFailure.is_interrupt` is only sometimes set (docs).

**Subagent.**
- Start: `SubagentStart` (`agent_id`). It fires again when a subagent resumes and each
  time an in-process teammate takes a message (docs), so one `agent_id` can start and
  stop several times.
- Stop: `SubagentStop`. It fires for background subagents too (source code: Stop and
  SubagentStop come from the same turn-end routine of each agent loop, with no
  foreground/background condition). `PostToolUse` on `Agent` with
  `tool_response.status: completed` also ends a foreground agent; `async_launched` means
  it went to the background (docs).
- Attribution: events built inside a subagent carry `agent_id` (source code: the common
  payload builder copies the tool-use context's agent id). Nesting (`parentAgentId`,
  `spawnDepth`) isn't in hook payloads (source code); only the innermost `agent_id`.
  Subagents launched from the main conversation start in the background by default
  (docs), so agents outliving their turn is the common case, not the exception.

**Background tasks and teammates.** `Stop` and `SubagentStop` list in-flight
`background_tasks`; a subagent task's `id` equals its `agent_id` (source code).
Teammates: in-process teammates have an `agent_id` and fire `SubagentStart` per
message, `SubagentStop`, `TeammateIdle` (docs). Split-pane teammates (tmux or iTerm2)
are separate `claude` processes, each with its own session (inference). Eywa sees
them as unrelated sessions.

**Tool approvals.**
- Requested: `PermissionRequest` (immediate; no `tool_use_id`, so match by input digest
  against the actor's recent `PreToolUse`). It also fires when a call can't prompt and
  will be auto-denied, for example background subagents in non-interactive mode (docs).
  Other PermissionRequest hooks can answer it at the same moment (docs). Sandboxed
  network requests fire only `Notification`/`permission_prompt` (docs).
- Granted: no hook at the moment of the grant. `PostToolUse`/`PostToolUseFailure` fire
  when the tool **finishes** (docs), so a granted long-running command still shows as
  waiting for its whole run.
- Denied manually: no hook (docs state PermissionDenied doesn't fire). The transcript
  records the rejection (`toolUseResult` "User rejected tool use", `toolDenialKind:
  user-rejected`) and usually a `[Request interrupted by user for tool use]` marker
  (local install), so a plain "No" ends the turn without `Stop` (inference). A denial
  with a comment lets the model continue, so the next `PreToolUse` or `PostToolBatch`
  follows (inference).
- Auto mode: `PermissionDenied` with `tool_use_id` (docs).

**Questions.** `AskUserQuestion` (and plan approval `ExitPlanMode`) via `PreToolUse`
keyed by `tool_use_id`, answered by that call's `PostToolUse` (docs: the answer
arrives as the tool's result). Whether these tools also fire `PermissionRequest` is
unverified.

**MCP input.** `Elicitation` → `ElicitationResult`, keyed by `elicitation_id` when
present. An elicitation always happens inside an MCP tool call (docs), so that call's
`PostToolUse`/`PostToolUseFailure` is a safe closer. `Notification` types
`elicitation_complete`/`elicitation_response` are extra closers (docs).

**Idle and waiting notifications.** `permission_prompt` and `elicitation_*_dialog`
fire after about 6 s, only when the user seems away in a terminal (docs); they confirm
but don't open requests. `agent_needs_input` and `agent_completed` concern agent-view
background sessions (docs); which `session_id` they carry is unknown.

**Ordering.** See section 2. Records for a closed turn can still arrive: PostToolUse
after an interrupt can run for up to 10 s (docs), and background-agent activity
arrives with a newer `prompt_id` (source code).

## 5. Process and identity

- **CLI:** the executable is `~/.local/share/claude/versions/<version>` (file name is
  the version string), launched as `claude` (local install). So the `argv[0]` rule in
  the design is required.
- **Desktop:** `/Applications/Claude.app/Contents/MacOS/Claude` →
  `Contents/Helpers/disclaimer` → `…/claude-code/<version>/<build-hash>/claude.app/Contents/MacOS/claude`,
  one `claude` process per session (local install: seven processes, seven registry
  files with distinct pids). This answers the design's open question 3: Desktop doesn't
  share one process across sessions. The Electron app's own executable is `Claude` and
  its helpers are `Claude Helper…`, so name matching must be case-sensitive (local
  install).
- **Supervisor:** background sessions run as child `claude` processes of a `claude`
  supervisor (docs), the design's "supervisor" case.
- **Pid in payloads:** none. But **`CLAUDE_PID`** in the hook's environment is the
  Claude Code process id (docs, v2.1.214+). Prefer it over walking the parents, and
  keep the walk for older versions.
- **Surface:** `CLAUDE_CODE_ENTRYPOINT` in the hook's environment (undocumented;
  `claude-desktop` locally). OTel docs name `claude-desktop`, `claude-desktop-3p`,
  `local-agent`. Other values such as `cli`, `claude-vscode`, `sdk-cli`, `sdk-ts`
  (inference, verify). The same value is written to transcripts and the session
  registry as `entrypoint` (local install). The current path heuristic in
  `process::surface` reports VS Code sessions as `cli`.

## 6. Persistent state

- **Transcripts (docs, local install):** `~/.claude/projects/<cwd with
  non-alphanumerics replaced by '-'>/<session-id>.jsonl`. Subagents:
  `<session-id>/subagents/agent-<agent_id>.jsonl` plus `agent-<agent_id>.meta.json`
  (`agentType`, `description`, `toolUseId` of the launching `Agent` call, `spawnDepth`,
  `finished`, `model`, …). Also `<session-id>/tool-results/`. The docs say the format
  is internal and changes between versions. Retention is 30 days by default.
- **Record structure (field names only, local install, 2.1.325):** one JSON object per
  line with `type` among `user`, `assistant`, `system`, `attachment`,
  `queue-operation`, `custom-title`, `agent-name`, `mode`, `last-prompt`, `pr-link`,
  `cost-state`, `file-history-*`, `api-request*`. Message records carry `uuid`,
  `parentUuid`, `sessionId`, `timestamp`, `isSidechain`, `agentId` (subagent files),
  `entrypoint`, `cwd`, `version`, `message.content[]` blocks (`text`, `thinking`,
  `tool_use` with `id`, `tool_result` with `tool_use_id`). `user` records add
  `promptId`, `origin.kind` (`human`, `task-notification`, `peer`), `turnOrigin`,
  `promptSource`, `permissionMode`, `toolUseResult`, `toolDenialKind`
  (`user-rejected`, `automode-blocked`, `automode-unavailable`), `permissionDecision`
  `{decision, source, reasonType, …}`, `interruptedByShutdown`. `system` subtypes seen:
  `stop_hook_summary`, `api_error`, `compact_boundary`, `informational`.
  `queue-operation` has `operation` (`enqueue`, `dequeue`, `remove`) and `reason`
  (`absorbed_mid_turn`, `delivered_to_agent`).
- **Live registry:** `~/.claude/sessions/<pid>.json` (section 2), and `claude agents
  --json` as its supported reader.

**Rebuilding after a restart (inference).** For each running `claude` process, read its
registry file (or `claude agents --json`) for `sessionId`, `cwd`, `status`,
`waitingFor`. `idle` → session with no open nodes (removes the design's false
`unknown` for idle sessions). `busy` → open, confirmed turn keyed by the transcript's
last `promptId`. `waiting` + `waitingFor` → an open request with that reason, attached
to the turn (actor unknown). Then refine from the transcript tail: open agents are
`meta.json` files with `finished: false` (verify meaning) whose `toolUseId` belongs to
this session. A pending approval is a `tool_use` block with no matching
`tool_result`, but the transcript alone can't tell "running" from "awaiting
approval"; the registry's `waitingFor` can. Emit the result as one `sync`.

## 7. Proposed adapter

| Hook | Eywa event |
| --- | --- |
| `SessionStart` | `open` session. If `CLAUDE_PID` (or the walk) maps the process to a different session, close that one (covers `clear`, in-session `resume`, `/branch`). `compact` keeps the id: no close. |
| `UserPromptSubmit` | `open` turn keyed by `prompt_id`, attr `origin` from `source` (`user`/`sdk` = person, anything else = machine). Close a different open turn as `cancelled`. |
| `PreToolUse` | Remember `tool_use_id` against the input digest per actor. `AskUserQuestion`: `open` request `question` keyed by `tool_use_id`. Main-thread activity for a turn closed by `Stop` **reopens** it (a blocked Stop continued the turn). Otherwise open or confirm the actor. Close the actor's approvals open for ≥ 1 s. |
| `PermissionRequest` | `open` request `approval` under the actor, keyed by the digest-matched `tool_use_id`. Don't replace an open `question` with the same id. |
| `PostToolUse`, `PostToolUseFailure` | `close` the request with that `tool_use_id`; close the actor's open `mcp_input` requests when the tool is an MCP tool. For `Agent` with `tool_response.status: completed`, close agent `tool_response.agentId`. |
| `PostToolBatch` (new) | Close every approval or question whose `tool_use_id` is in `tool_calls[]`: the call resolved, granted or denied. Forward only the ids. |
| `PermissionDenied` | `close` that `tool_use_id` as `cancelled` (auto mode only). |
| `Elicitation` / `ElicitationResult` | `open` / `close` request `mcp_input`, keyed by `elicitation_id`, else by `mcp_server_name`. |
| `Notification` | `idle_prompt`: close the open turn as `cancelled`. `permission_prompt` with no open request in the session: open an approval on the open turn (sandbox network prompts). `elicitation_complete`/`elicitation_response`: close the session's `mcp_input` requests. |
| `SubagentStart` | `open` agent keyed by `agent_id`, **reopening a stopped agent** (resume, teammate message), under the agent's original turn, else the current turn. |
| `SubagentStop` | `close` agent. Reconcile against `background_tasks` as on `Stop`. |
| `TeammateIdle` (new) | `close` agent `agent_id` when present. |
| `Stop` | `close` turn as `ok`; remember it as Stop-closed. Reconcile agents against `background_tasks` (ids are agent ids). |
| `StopFailure` | `close` turn as `error` with the `error` enum. |
| `SessionEnd` | `close` session. |
| Session registry (new, non-hook) | Watch `~/.claude/sessions/` for `status` changes (kqueue, no polling). `idle` closes an open turn as `cancelled` (interrupt) and closes stale approvals; `waiting` confirms a request; seeds recovery `sync`. |

**Changes from the current design and adapter, and why.**
1. **Privacy bug:** `record.rs` keeps `error` on every event, but
   `PostToolUseFailure.error` is tool output ("Exit code 1" plus stdout and stderr)
   (docs). Keep `error` only on `StopFailure`, where it's an enum.
2. **Pid:** read `CLAUDE_PID` (docs) instead of walking parents; keep the walk as a
   fallback. **Surface:** read `CLAUDE_CODE_ENTRYPOINT` (undocumented) instead of the
   path heuristic, which reports VS Code as `cli`.
3. **Re-starting subagents:** `SubagentStart` repeats for resumed subagents and
   in-process teammates (docs). The adapter's `stopped` set and the design's "a late
   `SubagentStart` opens nothing" would hide a resumed agent's work. SubagentStart is
   synchronous and fired by the agent itself, so it isn't late (inference).
4. **Background report turns:** answered by source code. `UserPromptSubmit.source` is
   `system` for task notifications, and each report gets a new `prompt_id`. Record it
   as turn `origin`; this also gives open question 1 its acknowledgment signal
   (`user`/`sdk` prompts only).
5. **Blocked Stop:** a Stop hook or `/goal` can keep the turn going after `Stop`
   (docs). Today the adapter ignores the continuation as late records, so the session
   shows `idle` while working. Main-thread records for a Stop-closed `prompt_id` should
   reopen that turn.
6. **`background_tasks` ids match `agent_id`** (source code). The verify item is
   resolved, and `SubagentStop` carries the same list, so reconcile there too.
7. **`SubagentStop` fires for background subagents** (source code, high confidence).
   It also fires for internal agents (prompt suggestions, `/btw`) whose `agent_type`
   is empty or the session's agent (docs). These are harmless only if no
   `SubagentStart` opened them (verify).
8. **Elicitations:** `elicitation_id` is optional (docs) and the adapter opens nothing
   without it. Fall back to the server name; close on the enclosing MCP tool's
   `PostToolUse*`, since `ElicitationResult` doesn't fire when a hook answered.
9. **Approval answers:** add `PostToolBatch` as a deterministic closer, keeping the
   1 s rule for actors that continue without a batch. State plainly that a granted
   long-running tool keeps `needs_input` until it finishes. Only the session registry
   (`status`) or OTel `tool_decision` sees the grant itself.
10. **Interrupts:** `idle_prompt` doesn't fire while a background agent runs (docs) and
    may not fire in Desktop. Add the registry `status → idle` closer.
11. **Headless false positives:** in `-p` without a permission host, `PermissionRequest`
    precedes an automatic denial (docs). The request closes on the next `PreToolUse` or
    at turn end. Consider not opening approvals for non-interactive entrypoints once
    their values are confirmed.
12. **`permission_prompt`** is the only signal for sandbox network prompts (docs).
13. **Teammates:** handle `TeammateIdle`; split-pane teammates are separate sessions.
14. **Design text:** `prompt_id` is optional (absent before the first prompt of the
    process) and process-global (source code). The Desktop path includes a build-hash
    directory. Desktop runs one process per session, which answers open question 3.
    Process-name matching must be case-sensitive.
15. **Recovery:** seed from `claude agents --json` or `~/.claude/sessions/<pid>.json`
    rather than leaving every pre-existing process `unknown`.
16. **Setup:** sync exec-form command hooks, short `timeout`, no stdout, no matchers,
    merged into existing hooks. `eywa doctor` should flag `disableAllHooks`,
    `allowManagedHooksOnly`, untrusted folders (hooks silent while the process lives)
    and `--bare`.

## 8. Gaps and verify items

Undocumented or unconfirmed, each with a check to run in a recorded session (log
payloads with a capture-only hook that writes field names and ids, no content):

1. **Manual denial.** Deny a Bash prompt with plain "No" and again with a comment.
   Record which of `PostToolBatch`, `Stop`, `PostToolUseFailure` fire, and whether
   `tool_calls[]` includes the denied id. Do the same in Desktop.
2. **`idle_prompt` in Desktop.** Interrupt a Desktop turn and wait 90 s. Does
   `idle_prompt` fire? Does the registry `status` flip to `idle`?
3. **`source` on `UserPromptSubmit`.** Confirm it's present on CLI 2.1.301 and Desktop
   2.1.325 for typed prompts (`user` vs `sdk`) and background reports (`system`).
4. **AskUserQuestion/ExitPlanMode.** Do they also fire `PermissionRequest`? What closes
   a dismissed question?
5. **Internal agents.** Do prompt suggestions or `/btw` fire `SubagentStart`?
6. **`/fork` and backgrounding.** In which process does `SessionStart(fork)` run? If
   it runs in the original process, the pid-based close would wrongly end the
   original session.
7. **Elicitation attribution.** Do `Elicitation` payloads carry `agent_id` and
   `prompt_id`?
8. **Entrypoint values** for CLI, VS Code, JetBrains, `-p`, agent-view background
   sessions; and whether a `claude` started from inside a Desktop session inherits
   `claude-desktop`.
9. **`claude agents --json` side effects.** Does running it start the supervisor? (Not
   run here.) Does it list Desktop sessions? (The registry files say yes.)
10. **Registry semantics.** Exact `status` and `waitingFor` transitions in the
    registry file versus `claude agents --json`; whether the file updates on `/clear`
    (new `sessionId`).
11. **`agent_needs_input` / `agent_completed`.** Which `session_id` do they carry?
12. **Split-pane teammates.** Confirm each is a separate session with its own
    `SessionStart`.
13. **Queued messages.** A message typed mid-turn and `absorbed_mid_turn`: does it fire
    `UserPromptSubmit`, and with a new `prompt_id`? (Transcripts show some consecutive
    prompts sharing a `promptId`.)
14. **`meta.json` `finished`** as a reliable recovery signal for open subagents.
15. The Agent SDK overview page returned HTML instead of Markdown and was not read. The
    hooks guide was downloaded but not relied on.

## 9. Sources

Documentation, all read 18 November 2026 (Markdown form, `<url>.md`):
- https://code.claude.com/docs/en/hooks
- https://code.claude.com/docs/en/settings
- https://code.claude.com/docs/en/env-vars
- https://code.claude.com/docs/en/sub-agents
- https://code.claude.com/docs/en/agent-teams
- https://code.claude.com/docs/en/agent-view
- https://code.claude.com/docs/en/sessions
- https://code.claude.com/docs/en/headless
- https://code.claude.com/docs/en/cli-reference
- https://code.claude.com/docs/en/monitoring-usage
- https://code.claude.com/docs/en/permissions
- https://code.claude.com/docs/en/interactive-mode
- https://code.claude.com/docs/en/statusline
- https://code.claude.com/docs/en/desktop
- https://code.claude.com/docs/en/vs-code
- https://code.claude.com/docs/en/claude-code-on-the-web
- https://code.claude.com/docs/en/remote-control
- https://code.claude.com/docs/en/hooks-guide (downloaded, not relied on)
- https://code.claude.com/docs/en/agent-sdk/overview (not readable as Markdown)

Local paths (read-only):
- `~/.local/bin/claude`, `~/.local/share/claude/versions/`
- `~/Library/Application Support/Claude/claude-code/2.1.325/<build-hash>/claude.app/Contents/MacOS/claude`
  (embedded source: hook input schemas, background task list builder, Stop/SubagentStop
  dispatch, common payload builder)
- `~/.claude/settings.json`, `~/.claude/settings.local.json` (keys and hook event
  names only)
- `~/.claude/sessions/*.json` (field names and enum values only)
- `~/.claude/projects/*/*.jsonl`, `~/.claude/projects/*/*/subagents/` (record types,
  field names, enum values, counts; no content)
- `ps` process table (executable paths only)
- This repository: `docs/design.md`, `src/adapter/claude.rs`, `src/adapter/mod.rs`,
  `src/record.rs`, `src/process.rs`
