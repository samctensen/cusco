# Grok Build: research briefing for an Eywa adapter

**18 November 2026.** Basis tags: (docs) = Grok Build user guide; (source code) = the public
`xai-org/grok-build` repository; (local install) = files, binaries and `--help` output on this
Mac; (inference) = my reading, not stated anywhere.

Two caveats apply to everything below:

- The public repository is a periodic "Synced from monorepo" export. Its newest commit is
  `2bdd1d6a6369` from 29 September 2026. The installed CLI is 1.0.89 from 18 November, so the
  user guide that ships with it (`~/.grok/docs/user-guide/`) is newer than the repository. Where
  they differ, I followed the installed guide. Source-code claims describe the September snapshot.
- No Grok session has run on this Mac yet, because `~/.grok/sessions/` doesn't exist. Nothing here
  was observed from a live session. Section 8 lists what a recorded session must confirm.

## 1. Product and surfaces

**What it is.** Grok Build is xAI's agentic coding harness and full-screen TUI, written in Rust.
The repository describes it as "SpaceXAI's coding agent harness and TUI" (source code), and the
help banner reads "Grok Build TUI" (local install). It runs interactively, headless (`grok -p`), or
as an Agent Client Protocol (ACP) agent for editors and other hosts (`grok agent stdio|serve|
headless|leader`) (docs, local install).

**Lineage (inference).** It shares a lot with Cursor's CLI. The installer creates
`~/.grok/bin/agent` beside `~/.grok/bin/grok`, and `agent` is the Cursor CLI's command name. The
docs cover Cursor sign-in, hook compatibility with Cursor, opening "a session an earlier Grok or
the Cursor CLI saved", and `grok worker`, which runs this machine as a Cursor private worker.

| Surface | What it is | Version seen | Eywa relevance |
| --- | --- | --- | --- |
| CLI / TUI `grok` | Main interactive client | 1.0.89 (`28be5bb1bf65`) [stable] (local install) | Primary target |
| Headless `grok -p` | One prompt, prints and exits | same binary | Same hooks. Your own hooks run without folder trust (docs) |
| ACP agent `grok agent stdio` | Editors and hosts drive Grok over ACP (docs) | same binary | Hooks fire as usual. Surface is `ide` or `desktop` |
| `grok agent serve` / `headless` | WebSocket server, and a mode driven over xAI's relay (local install `--help`) | same binary | Remote-driven sessions on this Mac |
| Leader `grok agent leader` | One shared process serving several clients over `~/.grok/leader.sock`. Off by default (`cli.use_leader`) (docs) | same binary | One process for many sessions |
| "Daemon route" | A background service (`grok host-server`) that runs sessions. Per window by default, shared when `features.shared_daemon` is on (docs) | same binary | One process for many sessions. Different hook timing (section 2) |
| Grok.app (community desktop) | Tauri app `grok-app`, bundle `com.grokapp.desktop` 0.2.51 (local install) | 0.2.51 | Spawns the CLI as ACP agents under a **separate `GROK_HOME`** (section 5) |
| Cloud agents and `grok worker` | Cloud workspaces, and Cursor cloud agents running on this machine (docs) | — | Out of scope for v0.1 |
| Grok Bot.app | Electron app, bundle `com.anysphere.sand` 1.2.3, with a "Computer Use" helper (local install) | 1.2.3 | Consumer chat product. No local hook surface found. **Out of scope** |
| grok.com | Web chat | — | **Out of scope** |

**Maturity.** The 1.0.x line ships on stable, alpha and enterprise channels (docs). The hooks
guide is long and changes often, with explicit "behavior change" notes (docs). Hooks are a
first-class, Claude-compatible feature, plus one Grok-specific event, `StopCancelled` (docs).
The guide keeps distinguishing a "shell route" (also called the native route) from a
"daemon route", and some hook behavior differs between them. Nothing I read says which route a
default install uses (section 8).

## 2. Extension mechanism

**Hook sources.** All of these are merged (docs):

| Scope | Path | Gate |
| --- | --- | --- |
| Global | `$GROK_HOME/hooks/*.json` (default `~/.grok/hooks/*.json`) | Folder trust |
| Global, Claude compatibility | `~/.claude/settings.json` and `settings.local.json` | Folder trust. On by default (`[compat.claude] hooks = true`, `GROK_CLAUDE_HOOKS_ENABLED`) |
| Global, Cursor compatibility | `~/.cursor/hooks.json`, matcher-group entries only | Folder trust |
| Project | `<repo>/.grok/hooks/*.json`, `<repo>/.claude/settings*.json`, `<repo>/.cursor/hooks.json` | Project trust |
| Config | `~/.grok/config.toml` `[[hooks.<Event>]]` | Folder trust |
| Managed | `managed_config.toml` and `requirements.toml` (`$GROK_HOME`, `/etc/grok`, MDM) | Always run |
| Plugins and agent frontmatter | Inside plugins, and `hooks:` in agent definitions | Folder trust |
| ACP client hooks | Registered by the embedding client (an IDE or desktop app) over ACP | Not file-based |

`GROK_HOME` relocates everything (docs). This Mac already has `~/.grok/hooks/wooting-glance.json`,
written by another tool, so the one-file-per-tool layout is established (local install).

**File format.** The format is Claude's matcher-group shape (docs):

```json
{ "hooks": { "PreToolUse": [ { "matcher": "Bash", "hooks": [
  { "type": "command", "command": "/path/eywa", "args": ["hook", "grok"], "timeout": 2 } ] } ] } }
```

- Event keys accept PascalCase, snake_case and Cursor's camelCase. Unknown event names are skipped,
  so a shared Claude settings file still loads (docs, source code).
- Handler fields Grok reads: `type` (`command` or `http`), `command`, `args`, `url`, `timeout`,
  `failClosed` / `onFailure`, `env`. Claude's `if` makes Grok skip the handler. Grok ignores
  `async` and `shell` and runs the hook in the foreground (docs).
- When `args` is present, even as `[]`, Grok runs `command` directly with no shell. Without `args`,
  it runs the command under `sh -c` (docs). Eywa should use the `args` form so the shell doesn't
  sit between the hook and Grok in the process tree.

**Invocation.**

- The payload is one line of JSON on stdin, ending in a newline (docs, source code).
- The working directory is the workspace root (the git root), not the launch directory. The
  launch directory is the payload's `cwd` (docs).
- Grok sets these environment variables on every hook: `GROK_HOOK_EVENT` (snake_case, e.g.
  `pre_tool_use`), `GROK_HOOK_NAME`, `GROK_SESSION_ID`, `GROK_WORKSPACE_ROOT` and
  `CLAUDE_PROJECT_DIR` (docs, source code). A hook can't override them. None of Grok's
  credentials are passed (docs).
- Each hook gets its own process group (`setsid`). Closing the session kills the group (source
  code). The parent process is still Grok, so walking up from the hook still finds it (inference).
- `GROK_HOOK_DEBUG=1` logs every stdin payload, which helps when recording a session (source code).

**Timeouts.** The default is 5 s. `UserPromptSubmit` gets 30 s, and `Stop`, `SubagentStop` and
`PostToolUse` get 600 s. Fractional values like `1.5` are allowed, and `0` means the default
(docs). Teardown limits (docs):

- Queued turn-end reports get 0.5 s in total.
- Queued `Notification` hooks get 1.5 s.
- Each `SessionEnd` hook gets 1.5 s by default (`GROK_SESSION_END_HOOKS_TIMEOUT_MS`, up to 60 s).

**Exit codes and stdout.** Hook output can change agent behavior, so Eywa's hook must print
nothing and exit 0.

- `0` allows.
- `2` blocks or feeds back. It denies a `PreToolUse`, blocks a `UserPromptSubmit`, keeps a
  `Stop`/`SubagentStop` running, and sends stderr to the model on `PostToolUse` (docs).
- Any other code fails open, unless the handler sets `failClosed` (docs).
- stdout is parsed as JSON for `PreToolUse` (allow, deny, ask, defer, `updatedInput`,
  `continue:false`), `UserPromptSubmit` (block), `Stop`/`SubagentStop` (block), and also for
  `PostToolUse` (feedback, output replacement, `continue:false`). Observe-only events read only a
  top-level `systemMessage` (docs).

**Trust.** User hooks wait for folder trust, the same store that gates MCP and LSP
(`~/.grok/trusted_folders.toml`). In an untrusted folder, even `~/.grok/hooks/*` and the Claude
compatibility hooks are skipped silently. While any user hook exists, Grok asks for trust in every
folder except home and filesystem roots (docs). `grok -p` treats the folder as trusted for user
hooks (docs). Hooks can also be switched off:

- per hook in `/hooks`, saved in `~/.grok/disabled-hooks`;
- all user hooks, by the policy pin `allow_managed_hooks_only` (docs).

`grok inspect` and the `/hooks` tab list load errors and disabled state (docs). This plays the same
role as Codex's hook trust, so `eywa doctor` needs an "installed but folder not trusted" check.

**Concurrency and order.**

- Handlers for one event run **one after another**, in config order, and stop at the first
  `deny`. A handler that several sources name, with identical command, `args` and `env`, runs once
  per call (docs).
- `SubagentStart` and `Notification` are dispatched without blocking the agent (source code).
- On the shell route, `Notification` chains run up to 32 at a time per session (docs).
- On the daemon route, observe hooks for every session share one pool: 32 running and 64 waiting.
  Beyond that a dispatch **runs none of its hooks** (docs), so Eywa can lose events under load.
- `Stop`, `StopFailure` and `StopCancelled` go through one worker per session (docs).

## 3. Event catalog

**Common envelope.** Grok builds one envelope per firing and gives every handler the same copy
(source code, docs).

| Field | Notes |
| --- | --- |
| `hookEventName` | Grok's snake_case name, e.g. `subagent_start` |
| `hook_event_name` | Claude's PascalCase name, e.g. `SubagentStart`. Always present |
| `sessionId` | **Session id.** A UUIDv7, or whatever a client supplied with `-s`. Inside a subagent this is the **child session's** id |
| `promptId` | **Turn id.** Optional. Missing on session-scoped events (`idle_prompt`, the session-end `Stop`) **and on tool events** (docs). Also missing on `SubagentStart` and every `Notification` (source code). Treat it as opaque and scoped to the session. On the daemon route, wake turns use `remote-turn-<turn id>` (docs). Background-subagent wakes use `subagent-completed-<subagentId>` (source code) |
| `cwd` | The launch directory, as typed |
| `workspaceRoot` | The git root |
| `timestamp` | RFC 3339, stamped at dispatch, not when the event happened (docs) |
| `transcriptPath` | Path to the session's `updates.jsonl`. **Not sent on the daemon route** (docs) |
| `permissionMode` | `default`, `auto`, `plan` or `bypassPermissions` |
| `clientIdentifier` | Declared in the envelope but always empty in the September snapshot (source code) |

**snake_case aliases.** Grok also copies some keys under Claude's snake_case names: `session_id`,
`transcript_path`, `permission_mode`, `tool_name`, `tool_input`, `tool_response`, `tool_use_id`,
`duration_ms`, `is_interrupt`, `stop_hook_active`, `last_assistant_message`, `error_details` and
`notification_type` (docs). The source snapshot copies fewer. **`promptId`, `subagentId` and
`subagentType` have no snake_case copy** (docs, source code).

**Events.** All from (docs) and (source code) `xai-grok-hooks/src/event.rs`:

| Event (`hookEventName`) | When it fires | Event-specific fields and identifiers |
| --- | --- | --- |
| `session_start` | A new session starts, including after `/new` or `/clear`, or a saved one reopens. **Never for a subagent's session.** Compaction doesn't fire it | `source` (`new` or `load`), `modelId`, `agentType` |
| `user_prompt_submit` | A prompt is submitted. Also, observe-only, for auto-wake turns (task or subagent completion, scheduler fire) and inside subagent sessions | `prompt` (text; drop it), `subagentType` in a child, `promptId` |
| `pre_tool_use` | Before a tool runs. Hooks run **before** the permission check | `toolName` (the real tool, `server__tool` for MCP), **`toolUseId`**, `toolInput`, `toolInputTruncated`, `subagentType` |
| `post_tool_use` | The tool ran, including a logical error such as a non-zero shell exit | `toolName`, `toolUseId`, `toolInput`, `toolResult`, `durationMs`, `isBackgrounded`, `subagentType` |
| `post_tool_use_failure` | The tool failed to dispatch, or an MCP tool returned an error | `toolName`, `toolUseId`, `error`, `durationMs`, `isInterrupt`, `subagentType` |
| `permission_denied` | "The permission system denies a tool call" | `toolName`, `toolUseId`, `toolInput`. **No `subagentType`** |
| `notification` | Something needs the user's attention | `notificationType`, `message` (display text), `title`, `level`. No tool or turn id |
| `subagent_start` | A subagent is spawned. Fires **in the spawning session** | **`subagentId`**, `subagentType`, `description` |
| `subagent_stop` | A subagent's turn ends. Fires once, **in the child session** | `phase` (`gate`), **`subagentId`** (the child's own session id), `subagentType`, `stopHookActive`, `lastAssistantMessage` |
| `stop` | The main agent's turn completes. Also fires once at session end | `reason` (`end_turn`; `channel_closed` or `shutdown` at session end), `stopHookActive`, `lastAssistantMessage`, `backgroundTasks[]` (`id`, `type` = `shell`/`monitor`/`subagent`, `status`, `description`, `command`, `agentType`), `sessionCrons[]` |
| `stop_failure` | The turn ended on an API error | `error` (`rate_limit`, `authentication_failed`, `invalid_request`, `server_error`, `max_output_tokens`, `unknown`), `errorDetails`, `subagentType` |
| `stop_cancelled` | Fires **instead of** `stop` when the turn didn't complete. Grok-specific | `reason` (`user_interrupt`, `permission_rejected`, `permission_cancelled`, `max_turns`, `no_progress`, `hook_blocked`, `hook_stopped`, `unknown`), `cancelledBy` (`user`, `runtime`, `unknown`), `cancelTrigger` (`ctrl_c`, `mouse`, `dashboard_stop`, …), `reasonDetails`, `subagentType` |
| `pre_compact` / `post_compact` | Around compaction | `source` (`manual` or `auto`) |
| `session_end` | The session ends. **Also fires for a child session** | `reason`, `turnCount`, `toolCallCount`, `subagentType` (only for a child) |

**`notificationType` values.**

- `permission_prompt`: a permission card is actually waiting. Also raised for plan approval and
  diff review (docs, source code). The message is always "Tool permission requested" or
  "Diff review requested" (source code).
- `idle_prompt`: about a minute after the session settles following **any** turn end, including
  interrupted or failed turns. Cancelled if you send another message first (docs).
- `task_complete` (docs).
- `agent_error`: retries or auto-recovery ran out (source code).

**Other identifiers.**

- **Subagent id equals child session id.** `spawn.rs` sets
  `child_session_id = SessionId::new(request.id)`, `subagent_id` is `request.id`, and
  `SubagentStop` sends `subagent_id = self.session_id_string()` (source code).
- **Request id: none.** No event carries an approval or request id (source code).
- **Tool call id: `toolUseId`,** on the four tool events only (source code).

## 4. Mapping to Eywa's model

**Session.**

- **Start:** `session_start`. With `source: load` it is a resume (reopen the session).
- **Clear:** `/new` and `/clear` send `session_start` with `source: new` (docs), presumably under a
  new `sessionId` (inference). It isn't documented whether the old session gets a `session_end`.
  Until a recording shows, use Claude's rule: a `source: new` start closes the session previously
  seen from the same process (inference).
- **End:** `session_end` without `subagentType`.
- **Process exit:** the same as end, but `session_end` may be cut off by the 1.5 s teardown budget
  (docs).

**Turn.**

- **Start:** `user_prompt_submit` without `subagentType`, keyed by `promptId`.
- **End:**
  - `stop` with `reason: end_turn` closes the turn as `ok`.
  - `stop_failure` closes it as `error`.
  - `stop_cancelled` closes it as `cancelled`.

  A turn reports **at most one** of the three (docs), with one exception: an interrupt while a
  `Stop` hook is still running produces `stop` followed by `stop_cancelled`.
- **No report at all:** a completed bash-mode (`!`) command, a completed built-in slash command,
  a rewind, cancel-and-send, and a turn superseded before its report was built (docs).
- **Backstop:** `notification`/`idle_prompt` (no `promptId`) closes whatever turn is still open
  (docs).
- **Prompts not typed by you:** `user_prompt_submit` also fires for auto-wake turns. Grok
  distinguishes some of them by `promptId` prefix: `subagent-completed-…` (source code) and
  `remote-turn-…` (docs, daemon route).

**User interrupt.** Grok has an interrupt hook: `stop_cancelled` with `reason: user_interrupt` and
`cancelledBy: user` (docs). However, the cancelled report is dispatched off the session's command
loop, so it **can arrive after the next turn's `user_prompt_submit`** (docs). Key it on
`promptId`, and treat a report for a turn that isn't the newest as a close-only no-op.
Cancel-and-send (typing while a turn runs) fires nothing, so the next `user_prompt_submit` has to
close the previous turn (docs).

**Subagent.**

- **Start:** `subagent_start` in the parent session. Its `sessionId` is the parent and
  `subagentId` is the child session id (source code). It has no `promptId`, so attach the agent
  to the parent's currently open turn (inference).
- **Stop:** one of these, from the **child** session (docs, source code):
  - `subagent_stop`, with `subagentId` equal to `sessionId`, is genuine completion;
  - `stop_failure` or `stop_cancelled` carrying `subagentType` (`user_interrupt` never fires in a
    child; the parent's report covers it);
  - `session_end` carrying `subagentType`, which is the child's teardown.
- **Attribution inside a subagent:** everything from the child session carries the child's
  `sessionId` and `subagentType`. The adapter maps `child sessionId → (root session, turn,
  agent)` from `subagent_start` (inference).
- **Nesting:** a subagent can spawn its own subagents. The default depth is 5 on the daemon route
  and 1 on the shell route (docs). Eywa nests agents only under a turn, so nested agents would
  need flattening under the root turn (inference).
- **Late starts:** for a child whose `subagent_start` Eywa never saw, nothing in the payload names
  its parent (gap; section 8).

**Background work.**

- Subagents run in the background by default (`run_in_background` defaults to `true`) (docs). So
  in Grok, unlike Claude, outliving the turn is the normal case.
- The `stop` payload's `backgroundTasks` lists in-flight shell, monitor and subagent tasks (docs).
- When a background subagent completes, it wakes the parent with an auto-wake turn whose `promptId`
  is `subagent-completed-<subagentId>`, which fires `user_prompt_submit` (source code, docs).
- `/loop` and scheduler fires run as detached background subagents (docs).

**Tool approval requested.** Grok has **no `PermissionRequest` event.** The only signal is
`notification` with `notificationType: permission_prompt`. It has no `toolUseId`, no `toolName`
and no `promptId` (source code). It fires only when a permission card is actually waiting (docs).

- The source snapshot says only the session that owns the permission manager may raise it, and a
  subagent shares its parent's manager. A subagent's approval would then be reported from the
  **root** session, not the child (source code comment, inference).
- So approvals attach to the session's open turn, as Codex's do.

**Tool approval answered.**

- **Granted:** the tool runs and sends `post_tool_use` or `post_tool_use_failure`. Which
  `toolUseId` belongs to the prompt can only be inferred: `pre_tool_use` fires before the
  permission check (docs), so take the latest `pre_tool_use` still running in that session when
  the prompt arrived (inference).
- **Rejected or dismissed:** this ends the turn with `stop_cancelled`, reason
  `permission_rejected` or `permission_cancelled` (docs). That is a real denial signal, better
  than Claude's.
- **`permission_denied`:** covers denials by the permission system (rules or auto mode). It is not
  documented to fire when you reject a card.
- The approve dialog also offers "reject once … optionally with a message back to the model"
  (docs). Whether that ends the turn is unconfirmed (section 8).

**Questions to the user.** The question tool is `ask_user_question` on the shell route and
`AskQuestion` on the daemon route (docs). Open a `question` request on `pre_tool_use` for either
name, keyed by `toolUseId`. Close it on the matching `post_tool_use` or `post_tool_use_failure`
(inference). It isn't known whether the question card also raises `permission_prompt`.

**MCP input requests.** Grok has an MCP elicitation card (`x.ai/mcp/elicit`) (docs), but **no hook
event and no `Notification` mapping** for it in the source snapshot (source code). Eywa can't see
it.

**Idle and waiting notifications.** `idle_prompt` arrives about 60 s after settling, with no
`promptId` (docs). The docs' own recipe for a busy/idle indicator uses five registrations:
`UserPromptSubmit`, `Stop`, `StopFailure`, `StopCancelled` and `Notification[idle_prompt]` (docs).
`task_complete` and `agent_error` carry no state that Eywa needs (inference).

**Event ordering and concurrency.**

- `timestamp` is stamped at dispatch and reports can queue behind slow hooks, so correlate by
  `promptId`, not by the clock (docs).
- A third-party blocking `Stop` gate makes `stop` fire again for every continuation round. Eywa
  can't tell those fires from the final one, because `stopHookActive` is true for both (docs). The
  `idle_prompt` backstop then corrects it.
- The daemon route's shared pool can drop dispatches (section 2).

## 5. Process and identity

**Executables (local install).**

- `~/.grok/bin/grok` and `~/.grok/bin/agent` are both symlinks to
  `~/.grok/downloads/grok-1.0.89-macos-aarch64`, a 190 MB arm64 binary.
- So the kernel reports a **versioned** executable name, and `argv[0]` is `grok`,
  `~/.grok/bin/grok`, or `agent`.
- Eywa's existing rule (executable file name equals the client name, or the `argv[0]` basename
  does) matches the common case through `argv[0]` (inference). `agent` collides with the Cursor
  CLI's command name, so match `agent` only when the executable lives under `~/.grok/downloads/`
  (inference).
- `version.json` records `version` and `stable_version`.

**Process shapes.**

| Mode | Shape | Liveness consequence |
| --- | --- | --- |
| Shell route TUI | One `grok` per window. The agent runs in the same process. The sandbox doc says "the agent runs in-process, not through the shared leader" (docs) | One process per window. `/new` and `/resume` reuse it for further sessions |
| Daemon route | A background service (`grok host-server`), per window by default or shared across windows with `features.shared_daemon` (docs). Headless runs each start their own daemon (docs) | Hooks run as children of the daemon. A shared daemon hosts many sessions |
| Leader | `grok agent leader` on `~/.grok/leader.sock`. Clients use `--leader` or `cli.use_leader`. Off by default (docs) | One process for many sessions |
| Grok.app | Spawns `$HOME/.grok/bin/grok agent stdio` per chat over ACP. Keeps a pool of agents ("live_shell", "bg", "parked", "prewarm"). Can start `nohup grok agent leader --no-exit-on-disconnect …` (local install, strings in the `grok-app` binary and its log) | Prewarmed `grok` processes have no session yet. A `nohup` leader leaves the app's process tree, so a parent walk from its hooks never reaches `grok-app` |

**Grok.app's separate home.** Grok.app runs its agents with
`GROK_HOME=~/Library/Application Support/com.grokapp.grok-app/agent-home`, which holds only
`auth.json` and `config.toml` today (local install). Two consequences, both (inference):

- **`~/.grok/hooks/` does not load for Grok.app sessions.** To cover them, Eywa would have to
  write `agent-home/hooks/eywa.json` too.
- `~/.claude/settings.json` hooks still load there, because Claude compatibility reads `~/.claude`
  whatever `GROK_HOME` says. The app sets only the MCP compatibility variables.

**What else Grok.app has (local install).**

- A loopback, token-gated "session api" (`/v1/sessions`, `/v1/sessions/{id}/turns`).
- `logs/host_runtime.json`, with the app's `pid` and a heartbeat.
- Desktop notifications of its own.

These are possible future signals but are not hooks.

**Identity in payloads.** No pid and no surface. `clientIdentifier` is declared but empty (source
code). The parent walk has to supply both:

- an ancestor `grok-app` means `desktop`;
- `grok agent stdio` under any other parent means `ide`;
- otherwise `cli`.

All three are (inference). `GROK_SESSION_ID` in the hook's environment duplicates `sessionId`.

**Claude-compatibility double fire.** This is the most important identity issue.

- Grok runs the hooks in `~/.claude/settings.json` (docs). Eywa's Claude setup would therefore
  run `eywa hook claude` **inside Grok sessions, with Grok-shaped payloads**.
- Those payloads have `session_id` but **no `prompt_id` and no `agent_id`**, which the Claude
  adapter depends on.
- The two registrations are different commands, so Grok's handler dedupe doesn't merge them, and
  both run (docs).
- The hook command can recognize Grok in two ways: `GROK_HOOK_EVENT` is set in its environment,
  and the payload has a camelCase `hookEventName` (docs, source code).

## 6. Persistent state

Field names only. Nothing below was read from a real session. Path layout (docs):

```
$GROK_HOME/sessions/<url-encoded cwd>/<session-id>/
  summary.json          info{id,cwd}, generated_title, session_summary, title_is_manual,
                        created_at, updated_at, num_messages, num_chat_messages,
                        current_model_id, parent_session_id (fork/restore), agent_name,
                        last_turn_summary, last_recap
  updates.jsonl         one ACP update per line: timestamp, timestamp_us,
                        method ("session/update" | "_x.ai/session/update"),
                        params{…, _meta.eventId "<session-id>-<counter>", _meta.agentTimestampMs}
  chat_history.jsonl, system_prompt.txt, prompt_context.json, tool_definitions.json,
  plan.json, rewind_points.jsonl, signals.json (token/tool/turn counters),
  feedback.jsonl, compaction_checkpoints/, subagents/ (per-subagent meta.json)
```

- If the encoded directory name exceeds 255 bytes, Grok uses a slug and hash instead and adds a
  `.cwd` file (docs).
- Child sessions live in the same tree as top-level sessions (docs).
- Grok.app's code suggests `summary.json` also has a `session_kind`, whose value starts with
  `subagent` for children (local install, strings in `grok-app`).

**Recovery value.**

- `updates.jsonl` is append-only and in event order, and it is the authority for `/resume` (docs).
  A later `sync` could replay it to find a turn left open or a pending permission card. The
  update kinds for "turn started", "turn ended" and "permission requested" still have to be
  identified from a recording (section 8).
- `transcriptPath` gives the path on the shell route but is missing on the daemon route (docs), so
  Eywa would have to rebuild it from `cwd` and `sessionId`.
- Grok.app keeps its own journal under `~/Library/Application Support/com.grokapp.grok-app/` and
  CLI sessions under `…/agent-home/sessions/` (local install, strings).
- Other local state:
  - `~/.grok/logs/unified.jsonl`: a process log with keys `ts`, `lvl`, `src`, `msg`, `pid`, `ctx`
    (local install).
  - `~/.grok/leader.log`, `~/.grok/leader.sock` (docs).

## 7. Proposed adapter

Turns are keyed by `promptId` and agents by `subagentId`, which is the child session id. Question
requests are keyed by `toolUseId`. Approvals get a synthetic id per session. The adapter keeps
three tables:

- `child → root session`;
- each session's newest `promptId`;
- each actor's in-flight tool calls (`toolUseId` and digest).

"Actor" means the agent when `sessionId` is a known child (or the payload has `subagentType`),
and otherwise the session's open turn.

| Hook (`hookEventName`) | Eywa event |
| --- | --- |
| `session_start` | `open` session. With `source: new`, close the session previously seen from the same process (**verify**). With `source: load`, reopen. |
| `user_prompt_submit` (main) | `open` turn keyed by `promptId`. Close any other open turn in the session as `cancelled` (cancel-and-send reports nothing). Set attr `wake: subagent` for the `subagent-completed-` prefix and `wake: remote` for `remote-turn-`. |
| `user_prompt_submit` (child) | Confirm or open the agent. Opens no turn. |
| `pre_tool_use` | Remember `toolUseId` and its digest for the actor. For `ask_user_question` or `AskQuestion`, `open` a request with reason `question` keyed by `toolUseId`. If the session's approval has been open for at least 1 s, `close` it (answered). |
| `post_tool_use`, `post_tool_use_failure` | `close` the `question` request with that `toolUseId`. If the session's approval was matched to this `toolUseId`, `close` it. Drop the call from in-flight. |
| `permission_denied` | `close` the session's open approval as `cancelled` if it was matched to this `toolUseId`. |
| `notification` `permission_prompt` | `open` a request with reason `approval` under the session's open turn, or under the session if no turn is open (plan or diff review). Match it to the newest in-flight `pre_tool_use`. |
| `notification` `idle_prompt` | `close` every open turn in the session as `cancelled`. Agents stay open. |
| `subagent_start` | `open` agent `subagentId` under the spawning session's open turn. Resolve the spawner to its root first when the spawner is itself a child. Record `subagentId → root`. |
| `subagent_stop` | `close` agent `sessionId` as `ok`. |
| `stop` with `reason: end_turn` | `close` turn `promptId` as `ok`. Reconcile `backgroundTasks` entries of type `subagent` against open agents, using the same rule as Claude (**verify** that the ids match `subagentId`). |
| `stop` with `reason` `channel_closed` or `shutdown` | `close` the session's open turns as `cancelled`. Session end follows. |
| `stop_failure` | Main session: `close` turn as `error` with `error`. Child: `close` agent as `error`. |
| `stop_cancelled` | Main session: `close` turn as `cancelled` with `reason` and `cancelledBy`; this also closes its approval. Child: `close` agent as `cancelled`. Never reopens a turn. |
| `session_end` | Main session: `close` session. Child (`subagentType` present): `close` agent if still open. |
| `pre_compact`, `post_compact` | Ignore. |

**Client row.** `"observes": ["session", "turn", "agent", "request:turn"]`. Approvals are
attributed to the turn, not the subagent, because `permission_prompt` names no actor and the
source suggests the root session raises it. Questions are attributed to the actor. MCP input is
not observed.

**`eywa setup` registrations.**

- **What:** write `$GROK_HOME/hooks/eywa.json` with one matcher group per event:
  `SessionStart`, `SessionEnd`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`,
  `PostToolUseFailure`, `PermissionDenied`, `Notification`, `SubagentStart`, `SubagentStop`,
  `Stop`, `StopFailure`, `StopCancelled`.
- **Handler:** `{"type":"command","command":"<abs path to eywa>","args":["hook","grok"],
  "timeout":2}` with no matcher.
- **Why a short timeout:** `PreToolUse`, `UserPromptSubmit`, `Stop` and `SubagentStop` are gates
  on the agent's critical path, and their defaults run to 30–600 s.
- **Where:** write the file for `~/.grok`, and offer to write it for Grok.app's `agent-home`.
- **Hook command rules:**
  1. Print nothing and exit 0, always. stdout is read even for `PostToolUse`, and exit 2 feeds
     stderr to the model.
  2. When invoked as `eywa hook claude` with `GROK_HOOK_EVENT` set, relabel the record as
     `grok`. Don't drop it: Grok.app sessions reach Eywa only through `~/.claude/settings.json`.
  3. Send the envelope's `timestamp`, so the adapter can drop the second copy of a firing that
     reached it through both registrations. Both handlers receive the same envelope, with an
     identical `timestamp`, `hookEventName`, `sessionId` and `toolUseId` (source code).
- **Allowed fields to add for Grok:** `hookEventName`, `promptId`, `subagentId`, `subagentType`,
  `notificationType`, `source`, `reason`, `cancelledBy`, `error`, `isBackgrounded`, `timestamp`,
  and `backgroundTasks[]` restricted to `{id, type, status, agentType}`.
- **Fields to drop:** `prompt`, `toolInput` (send the digest only), `toolResult`,
  `lastAssistantMessage`, `message`, `title`, `description`, `errorDetails`, `reasonDetails`,
  `backgroundTasks[].command` and `.description`, and `sessionCrons`.

## 8. Gaps and verify items

**How to record a session.** Use a scratch `~/.grok/hooks/record.json` whose command writes
`jq -c '{e:.hookEventName, s:.sessionId, p:.promptId, t:.toolUseId, n:.toolName, sa:.subagentId,
st:.subagentType, nt:.notificationType, r:.reason, cb:.cancelledBy, src:.source, ts:.timestamp,
bg:[.backgroundTasks[]?|{id,type,status}]}'` plus `$PPID` and `ps -o comm=,args= -p $PPID` to a
log. Run Grok with `GROK_HOOK_DEBUG=1` in a trusted folder. No content fields are captured.
Confirm the following:

1. **Which route a stock install uses**, and how `grok host-server` relates to the TUI process: a
   child, a sibling, or launchd-owned. Also whether a TUI on the daemon route still has a `grok`
   ancestor in the hook's parent chain. Nothing in the docs states the default.
2. **Subagent approvals.** Does `permission_prompt` for a subagent's tool arrive with the root's
   `sessionId` or the child's? The source comment says the root. The sibling project
   wooting-glance assumes the child.
3. **Approval matching.** Is the prompted call always the newest unfinished `pre_tool_use`? Do
   permission cards serialize when several parallel calls need approval?
4. **Manual rejection.** Does "reject once with a message" end the turn
   (`stop_cancelled: permission_rejected`), or does the turn continue? Does `permission_denied`
   fire on a manual reject?
5. **The question tool.** Does `ask_user_question` raise `permission_prompt` as well as
   `pre_tool_use`?
6. **`/new` and `/clear`.** Is the `sessionId` new, and does the old session get `session_end`
   (and with which `reason`)? List the `session_end` `reason` values; they aren't documented.
7. **`backgroundTasks[].id`.** For `type: subagent`, does it equal `subagentId`?
8. **Unknown children.** For a child session first seen after Eywa started, how do we find its
   root? Check `subagents/*/meta.json` in the parent's directory, the child's `summary.json`, and
   `session_kind`.
9. **Grok.app.** Do its agents load `~/.claude/settings.json` hooks? Does it prompt for folder
   trust, or pass `--trust`? Does it register ACP client hooks that hide or duplicate events? What
   process tree do its pooled or prewarmed agents show?
10. **Event shape.** Do `post_compact`, `task_complete` and `agent_error` appear in normal use?
    Does `idle_prompt` really arrive about 60 s after every turn end?
11. **Turn-start records in `updates.jsonl`.** Which update kinds mark turn start, turn end and a
    pending permission? They are needed for a later `sync`.
12. **MCP elicitation.** Not observable through hooks. Confirm on 1.0.89, which is newer than the
    source snapshot.

The public repository lags the installed binary by about seven weeks. Recheck every
source-code claim against a recording before the adapter relies on it.

**The sibling project's claims, checked** (`~/Developer/wooting-glance/src/agents.rs`):

| Claim | Result |
| --- | --- |
| snake_case `hookEventName` (`subagent_start`, `notification`) | **Confirmed** |
| `notificationType: permission_prompt` | **Confirmed**. Plan and diff review also produce it |
| camelCase `sessionId`, `hookEventName`, `subagentId` | **Confirmed**. `subagentId` appears only on `subagent_start` and `subagent_stop`, and it equals the child's `sessionId` |
| PascalCase `hook_event_name` alongside the snake_case name | **Confirmed**, on every payload |
| Grok imports Claude Code's hooks, so Claude-configured hooks fire for Grok sessions | **Confirmed**, global and project, on by default. **Correction:** they receive Grok's envelope, not Claude's payload. There is no `prompt_id` or `agent_id`, and Claude-only events (`PermissionRequest`, `Elicitation`, …) are skipped |

## 9. Sources

All read on 18 November 2026.

**Web**

- https://github.com/xai-org/grok-build: repository metadata. Newest commit `2bdd1d6a6369`,
  29 September 2026.
- https://github.com/xai-org/grok-build/blob/main/crates/codegen/xai-grok-pager/docs/user-guide/10-hooks.md:
  the hooks guide. I read the newer copy bundled with the install (below).
- https://raw.githubusercontent.com/xai-org/grok-build/main/crates/codegen/xai-grok-hooks/src/event.rs
- https://raw.githubusercontent.com/xai-org/grok-build/main/crates/codegen/xai-grok-hooks/src/runner/command.rs
- https://raw.githubusercontent.com/xai-org/grok-build/main/crates/codegen/xai-grok-shell/src/session/acp_session/hooks.rs
- https://raw.githubusercontent.com/xai-org/grok-build/main/crates/codegen/xai-grok-shell/src/session/acp_session_impl/updates.rs
- https://raw.githubusercontent.com/xai-org/grok-build/main/crates/codegen/xai-grok-shell/src/session/acp_session_impl/hook_dispatch.rs
- https://raw.githubusercontent.com/xai-org/grok-build/main/crates/codegen/xai-grok-shell/src/session/acp_session_impl/stop_gate.rs
- https://raw.githubusercontent.com/xai-org/grok-build/main/crates/codegen/xai-grok-shell/src/agent/subagent/spawn.rs
- https://raw.githubusercontent.com/xai-org/grok-build/main/crates/codegen/xai-grok-shell/src/agent/mvp_agent/subagent_spawn.rs
- GitHub code search over `xai-org/grok-build`, used to locate the files above. It hit the rate
  limit partway through, so I fetched the remaining files from raw.githubusercontent.com.

**Local**

- `~/Developer/eywa/docs/design.md`
- `~/.grok/` layout, `~/.grok/version.json`, `~/.grok/bin/{grok,agent}` symlinks,
  `~/.grok/downloads/grok-1.0.89-macos-aarch64` (`--version`, `--help`, `leader --help`,
  `agent --help`, `strings`), and `~/.grok/logs/unified.jsonl` (keys only).
- `~/.grok/hooks/wooting-glance.json`
- `~/.grok/docs/user-guide/`: `05-configuration.md`, `10-hooks.md`, `14-headless-mode.md`,
  `16-subagents.md`, `17-sessions.md`, `18-sandbox.md`, `19-plan-mode.md`,
  `20-background-tasks.md`, `22-permissions-and-safety.md`, `25-status-line.md`,
  `26-config-reference.md`, `02-authentication.md`, `03-keyboard-shortcuts.md`,
  `15-agent-mode.md` (targeted reads and searches; `10-hooks.md` read in full).
- `/Applications/Grok.app` (`Info.plist`, `Contents/MacOS/grok-app` strings).
- `/Applications/Grok Bot.app` (`Info.plist`, bundle layout).
- `~/Library/Application Support/com.grokapp.grok-app/`: layout,
  `agent-home/config.toml`, `logs/host_runtime.json`, `logs/app.log.2026-11-18`.
- `~/Developer/wooting-glance/src/agents.rs`
- Process table (`ps`): no Grok processes were running during the research.
