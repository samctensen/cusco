# Google coding agents: Antigravity CLI, Gemini CLI, Antigravity desktop

**Research briefing · 18 November 2026**

Basis tags: (docs) = official documentation or changelog, (source code) = repository
source, (local install) = observed read-only on this Mac, (inference) = my conclusion,
not stated anywhere.

## 1. Product and surfaces

| Surface | What it is | Latest version seen | Status for Cusco |
| --- | --- | --- | --- |
| **Antigravity CLI** (`agy`) | Go terminal agent that shares its agent harness with the Antigravity desktop app (docs). Runs several agents in the background (docs). | 1.5.11 is listed in the changelog navigation. The newest dated entry is 1.5.10, 17 Nov 2026 (docs). | **Target first.** |
| **Antigravity App** (the 2.x desktop app, called "Hub" in changelog anchors) | Agent-first desktop app. Hosts many conversations, subagents, `/swarm`, automations and sidecars (docs). | 2.33.0, 17 Nov 2026 (docs) | Second. Uses the same `hooks.json` as the CLI (docs), but has no status line feed (see §4). |
| **Antigravity IDE** | Standalone editor, separate from the App (docs). The 1.x product installed here was a VS Code-style Electron fork (local install). | 2.7.0 per changelog navigation (docs) | Same hooks file as the App. Treat it like the App. |
| **Antigravity IDE extensions** (VS Code, JetBrains, Zed, Xcode and others) and `agy acp` | Editor integrations, partly through the Agent Client Protocol (`agy acp`) (docs) | n/a | They run the CLI harness under an editor process (inference). The CLI adapter applies, with surface `ide`. |
| **Gemini CLI** (`gemini`, npm `@google/gemini-cli`) | Node.js terminal agent. Consumer use (free tier and Google AI Pro/Ultra) moved to Antigravity CLI on **18 June 2026**. It stays available for Gemini Code Assist Standard/Enterprise and paid API keys (docs). It isn't archived and still ships releases (docs). | v0.68.0, 10 Nov 2026 (docs: GitHub releases API) | Legacy and enterprise-only. Its hooks are Claude Code–shaped and **incompatible** with Antigravity's. Support it later, if at all. |
| Jules, Remote Agents (`agy --remote-agent`) | Cloud execution (docs) | n/a | **Out of scope.** No local hooks are documented for cloud runs. |

**Transition** (docs: Google Developers Blog, Gemini CLI README, Antigravity migration page):
- Antigravity CLI became available to everyone on 19 May 2026. The blog says it keeps Gemini CLI's skills, hooks, subagents and extensions, with extensions renamed **plugins**, and warns there is no full feature parity.
- `agy plugin import gemini` converts legacy extensions and commands. On first launch, `agy` offers to migrate global config. It reads `GEMINI.md`, `AGENTS.md` and `~/.gemini/GEMINI.md` as they are. MCP servers move out of `~/.gemini/settings.json` into `~/.gemini/config/mcp_config.json`, and workspace MCP into `.agents/mcp_config.json`.
- The migration page doesn't mention hooks at all. Antigravity does not read Gemini CLI's `hooks` block in `settings.json`. Its hook file, event names and payload casing are all different (docs, compared in §2).
- `agy plugin import` converts **Claude Code** plugin hooks (`PreToolUse`, `PostToolUse`, `Stop`, startup `SessionStart`) and maps tool names, for example `Bash` to `run_command`. Hooks with no equivalent become warnings (docs: CLI 1.5.5). Nothing comparable is documented for Gemini CLI hooks.

**Which to target:** Antigravity CLI first, then the App and IDE with the same hook adapter
plus a reduced `observes`. Gemini CLI comes last. Its hooks are the easiest of all to map
because they mirror Claude Code (§4.4), but its consumer user base has moved away (inference).

**Local install** (local install):
- Only leftovers of the **1.x Antigravity IDE** remain. Its last language-server log, from February 2026, reports `appName=antigravity_desktop version=1.19.4`. Editor logs run until 20 April 2026.
- `/Applications/Antigravity.app` is gone.
- `~/.antigravity/antigravity/bin/agy` and `~/.antigravity/antigravity/bin/antigravity` are **broken symlinks** to `/Applications/Antigravity.app/Contents/Resources/app/bin/antigravity`. The 1.x IDE's `code`-style launcher was also called `agy`.
- `~/.local/bin/agy` doesn't exist, no `gemini` binary is installed, and there is no `~/.gemini/settings.json`.
- I therefore couldn't run `--version` or `--help` on any of these tools, and the current 2.x/CLI data layout is documented here only from the docs.

## 2. Extension mechanism

### 2.1 Antigravity hooks (CLI, App, IDE)

**Files** (docs):

| Scope | Path | Notes |
| --- | --- | --- |
| Global, shared by App, CLI and IDE | `~/.gemini/config/hooks.json` | One registration covers all three surfaces. CLI 1.0.8 fixed `/hooks` writing to `~/.gemini/antigravity-cli/hooks.json` instead of this file (docs: changelog). |
| Workspace | `<workspace>/.agents/hooks.json` | Loads only after the folder is trusted (`trustedWorkspaces` in CLI settings) (docs: CLI 1.1.1 changelog, CLI reference). |
| Plugin (CLI) | `hooks.json` at the root of an installed plugin. `hooks/hooks.json` is **not** read. | Plugin hooks run in the plugin's folder and receive `PLUGIN_ROOT` and `PLUGIN_DATA` env vars (docs: CLI 1.4.3, 1.5.3, 1.5.6) |
| Custom agent | A `hooks:` list in the agent's Markdown front matter | App 2.17.0, CLI 1.3.1 (docs) |
| Not loaded | Hooks inside `~/.gemini/antigravity-cli/settings.json` | (docs) |

**Format** (docs):
- The top level is a map from hook **name** to an object.
- Each object has an optional `"enabled"` (default `true`) and one or more event keys.
- `PreToolUse` and `PostToolUse` take `[{ "matcher": "<regex on tool name>", "hooks": [handler…] }]`. `""` or `"*"` matches all tools.
- The other events take a plain handler list.
- A handler is `{ "type": "command", "command": "<shell>", "timeout": <seconds, default 30> }`.
- A Claude Code–style top-level `"hooks"` key loads as one hook named `hooks` (docs: CLI 1.4.3).
- **Setup consequence:** Cusco can own one named entry, such as `"cusco"`, and add or remove it without touching anyone else's hooks (inference).

**Invocation:**
- The hook gets a JSON payload on stdin, with camelCase fields (docs).
- No environment variables are documented, apart from `PLUGIN_ROOT`/`PLUGIN_DATA` for plugin hooks (docs). The working directory isn't documented for non-plugin hooks (docs).
- No argv is passed. Cusco can put the event name into the command string itself, as in `cusco hook antigravity PreToolUse` (inference). That avoids depending on `hookEventName`, which only CLI 1.5.4 and later send (docs).

**Output and exit codes:** stdout JSON changes agent behavior. Exit-code semantics are **undocumented** (docs).

| Event | Output it may return (docs) |
| --- | --- |
| `SessionStart` | `injectSteps` |
| `PreToolUse` | `decision` (documented as required): `allow`, `auto_approve`, `deny`, `ask`, `force_ask`, `deny_unless_prior_grant`. Also `reason`, `overwrite` (merged into the tool args) and `permissionOverrides`. |
| `PostToolUse` | `{}` |
| `PreInvocation` | `injectSteps`, `terminationBehavior: "terminate"` |
| `PostInvocation` | `injectSteps`, `terminationBehavior: "force_continue"` or `"terminate"` |
| `Stop` | `decision` (documented as required): `"continue"` keeps the agent running, and anything else lets it stop |

Cusco's hook command must never print a `decision`. An empty decision string from a
pre-tool hook is "safely handled" (docs: CLI 1.0.16), so printing nothing is probably a
no-op (inference, **verify** §8).

**Failure handling** (docs: changelog):
- A failing `PostToolUse` hook (missing command, non-zero exit, timeout or invalid JSON) is logged, and the tool keeps its result (CLI 1.4.3).
- A failing hook no longer ends the session (App 2.12.0).
- A hook that times out stops at its timeout (App 2.6.0).
- Hooks run **synchronously** in the agent loop, since `PreToolUse` and `PreInvocation` can block or terminate the step (docs). Cusco's hook must return in milliseconds.

**Concurrency and ordering:**
- Undocumented between hooks (docs).
- `hooks.json` hooks run before the built-in termination checks (docs: CLI 1.1.10).
- Hooks fire only on tool steps for the tool events (docs: CLI 1.1.9).
- `/btw` side questions don't run hooks (docs: CLI 1.3.3).

**Trust:**
- Workspace hooks need a trusted folder (docs).
- No trust or approval step is documented for the global file (docs). **Verify** that nothing like Codex's hash-trust exists.
- Organization `admin_settings.json` can restrict plugins and permissions (docs: App 2.33.0 / CLI 1.5.10). Whether it can also disable user hooks is **verify**.

### 2.2 Non-hook integrations that could be better sources

| Integration | Surfaces | Value for Cusco |
| --- | --- | --- |
| **Custom status line** (`statusLine` in `~/.gemini/antigravity-cli/settings.json`) | CLI, interactive only. It isn't started in `-p` print mode (docs: CLI 1.4.7). | **High.** A command run through `sh -c` with session-state JSON on stdin each time agent state changes, at most every 300 ms, and killed after 5 s. Its fields include `agent_state` (`idle`, `working`, `tool_use`, `reviewing`, `authenticating`, `initializing`, `error`), `tool_confirmation_pending`, `subagents[] {name, role, status}`, `pending_input_count` and `conversation_id` (docs). It is the **only documented signal for "waiting for approval."** Its stdout is displayed to the user. There is only one slot, so Cusco must chain to any existing user command and set `stack_with_default`. After 30 consecutive failures it disables itself for the session (docs). |
| `--output-format stream-json` | CLI headless only | Has `step_update` events with `conversation_id`, `step_index`, `state`, `tool_name` and `subagent_info.subagents[] {type_name, role, conversation_id, log_uri, workspace_uris}`, plus `result.status` (`SUCCESS`, `ERROR`, `CANCELED`, `INTERRUPTED`, `WAITING`, `RUNNING`, …) (docs). Cusco would have to launch the agent itself, so it can't observe sessions with this (inference). It is useful as a schema reference. |
| OSC 7501 terminal status | CLI | Reports working, waiting for input, done or failed to the terminal emulator (docs: CLI 1.4.0). Cusco can't read it, but it shows that the CLI tracks a "waiting for input" state internally. Controlled by `AGY_CLI_OSC7501`. |
| ACP (`agy acp`) | Editors | Cusco would have to be the ACP client, so it can't observe this way (inference). |
| Sidecars (`~/.gemini/config/sidecars/<id>/sidecar.json`) | App (only App is listed) | Long-running processes that the App supervises, with an `agentapi` CLI (`new-conversation`, `send-message`) (docs). No event feed is documented, so it's not useful for observation (docs). |
| Local language-server RPC | Desktop 1.x (local install). The 2.x/CLI harness is probably the same Go server (inference). | The 1.x IDE launched `…/Antigravity.app/Contents/Resources/app/extensions/antigravity/bin/language_server_macos_arm` with `--csrf_token`, `--extension_server_port`, `--extension_server_csrf_token`, `--app_data_dir antigravity` and `--cloud_code_endpoint`. It listened on 127.0.0.1 random HTTP/HTTPS ports, served the Connect RPC service `exa.language_server_pb.LanguageServerService`, and wrote a discovery file at `~/.gemini/antigravity/daemon/ls_<hash>.json` (local install). It is undocumented, CSRF-protected and private. Don't build on it. |
| SDK lifecycle hooks (`@hooks.pre_turn`, `@hooks.on_tool_error`) | Python SDK apps | Only for programs built with the SDK (docs). Not applicable. |

### 2.3 Gemini CLI hooks (legacy)

- **Config:** a `hooks` object in `settings.json`. Precedence is project `.gemini/settings.json`, then user `~/.gemini/settings.json`, then system `/etc/gemini-cli/settings.json`, then extensions (docs).
- **Shape:** `{ "<Event>": [{ "matcher", "sequential", "hooks": [{ "type": "command", "command", "name", "timeout" (ms, default 60000), "description" }] }] }`. `sequential: false` runs the group in parallel (docs).
- **Env vars:** `GEMINI_PROJECT_DIR`, `GEMINI_PLANS_DIR`, `GEMINI_SESSION_ID`, `GEMINI_CWD`, and `CLAUDE_PROJECT_DIR` as an alias (docs).
- **Exit codes:** `0` means stdout is parsed as JSON. `2` blocks, with stderr as the reason. Anything else is a non-fatal warning. Stdout must contain only the final JSON (docs).
- **Output fields:** `systemMessage`, `suppressOutput`, `continue`, `stopReason`, `decision`, `reason` and `hookSpecificOutput` (docs).
- **Trust:** project hooks are fingerprinted, and a changed name or command counts as a new, untrusted hook that warns before running (docs).
- **Management:** `/hooks panel`, `/hooks enable-all` and `/hooks disable-all` (docs).

## 3. Event catalog

### 3.1 Antigravity hooks (CLI, App, IDE share one schema per the docs)

Common stdin fields (docs):

| Field | Notes |
| --- | --- |
| `conversationId` | UUID. The only session identifier. |
| `workspacePaths` | Array of paths |
| `transcriptPath` | `<app_data_dir>/brain/<conversationId>/.system_generated/logs/transcript.jsonl` |
| `artifactDirectoryPath` | `<app_data_dir>/brain/<conversationId>` |
| `modelName` | The model in use |
| `lastUserInput` | Prompt text. Added in CLI 1.4.7 and App 2.28.0. **Must not cross Cusco's socket.** |
| `hookEventName`, `cwd` | CLI 1.5.4 and later. **Verify** for App and IDE. |

`<app_data_dir>` is `~/.gemini/antigravity` for the App, `~/.gemini/antigravity-cli` for
the CLI and `~/.gemini/antigravity-ide` for the IDE (docs).

| Event | When it fires (docs) | Event-specific fields (docs) |
| --- | --- | --- |
| `SessionStart` | Once per **new** conversation, before the first model call | none |
| `PreInvocation` | Before each model call | `invocationNum` ("0-indexed sequence number of the current model invocation"), `initialNumSteps` (steps already in the trajectory) |
| `PostInvocation` | Right after each model call completes | `invocationNum`, `initialNumSteps` |
| `PreToolUse` | Before a tool runs, matched by tool name | `toolCall {name, args, id}` (`id` since CLI 1.5.4), `stepIdx` (0-based step index) |
| `PostToolUse` | After a tool completes | `toolCall {name, args, id}`, `stepIdx`, `error` (empty on success). No tool result is included. |
| `Stop` | When the execution loop terminates | `executionNum` ("sequence number of the execution attempt"), `terminationReason` (examples only: `model_stop`, `max_steps_exceeded`, `error`), `error`, `fullyIdle` (true when "all background commands or asynchronous tasks have completed"), `continuation_active` (CLI 1.5.6 and later: true while running a continuation an earlier Stop hook requested) |

**Identifiers** (docs):

| Identifier | Field |
| --- | --- |
| Session | `conversationId`. The status line also reports `session_id`, which is identical to `conversation_id`. |
| Turn or prompt | None. `executionNum` appears only on `Stop`, and `invocationNum` counts model calls. |
| Subagent | None in hooks. A subagent is its own conversation with its own conversation ID (docs: subagents, stream-json). |
| Tool call | `toolCall.id` (CLI 1.5.4 and later), otherwise `stepIdx` |
| Request | None |

**Events that don't exist** (docs):
- Prompt submit
- Session end
- Interrupt
- Permission or approval request, grant or denial
- Notification or idle
- Subagent start or stop
- MCP elicitation

`ask_question`, `ask_permission`, `invoke_subagent`, `define_subagent`, `send_message` and
`manage_subagents` exist only as **tool names** that matchers can target (docs).

### 3.2 Antigravity CLI status line payload (CLI only)

| Group | Fields (docs) |
| --- | --- |
| Session | `session_id` (= `conversation_id`), `conversation_id`, `conversation_title` (generated from the prompt, so drop it), `transcript_path` |
| Location | `cwd`, `workspace {current_dir, project_dir}`, `vcs {type}` |
| State | `agent_state`, `tool_confirmation_pending` (true while a confirmation dialog shows, omitted otherwise), `pending_input_count` (queued messages, omitted when 0), `task_count`, `subagents[] {name, role, status: running or completed}` plus the model since 1.5.3, `cycle_mode` |
| Other | `agent`, `version`, `product`, `model`, `context_window`, `quota`, `cost`, `plan_tier`, **`email`** (must be dropped), `terminal_width`, `vim`, `sandbox`, `artifact_count` |

### 3.3 Gemini CLI hooks

- **Base fields** (docs): `session_id`, `transcript_path`, `cwd`, `hook_event_name`, `timestamp`.
- **Env vars** (docs): `GEMINI_SESSION_ID`.

| Event | When it fires (docs) | Fields (docs) |
| --- | --- | --- |
| `SessionStart` | Startup, resume or `/clear` | `source`: `startup`, `resume` or `clear` |
| `SessionEnd` | Exit or clear | `reason`: `exit`, `clear`, `logout`, `prompt_input_exit` or `other` |
| `BeforeAgent` | After a prompt is submitted, before planning | `prompt` |
| `AfterAgent` | Once per turn, after the final response | `prompt`, `prompt_response`, `stop_hook_active` |
| `BeforeModel`, `AfterModel`, `BeforeToolSelection` | Around model calls. `AfterModel` fires per chunk. | `llm_request`, `llm_response` |
| `BeforeTool`, `AfterTool` | Around a tool | `tool_name`, `tool_input`, `mcp_context`, `original_request_name`, and `tool_response` on `AfterTool` |
| `Notification` | System alert | `notification_type` (`ToolPermission`), `message`, `details` |
| `PreCompress` | Before history is summarized | `trigger`: `auto` or `manual` |

Gemini CLI has no turn id, no tool-call id and no subagent id in any payload (docs).

## 4. Mapping to Cusco's model

### 4.1 Antigravity CLI (hooks plus status line)

| Cusco concept | Signal that creates it | Signal that ends it |
| --- | --- | --- |
| Session | `SessionStart` (new conversation), or any event or status-line tick carrying a `conversationId` (implicit open) | No end event. Closes when the `agy` process exits. `/clear`, `/new` and `/fork` give a new conversation ID; `/fork` "allocates a new unique session ID" (docs). Nothing signals that the old conversation ended (docs), so it stays open and idle until the process exits (inference). |
| Resume | `agy -c` or `--conversation <id>` reuses the ID (docs). `SessionStart` fires only for **new** conversations (docs), so a resumed session reappears on its first event or status-line tick (inference). **Verify.** | n/a |
| Turn | No prompt event. Open on `PreInvocation` with `invocationNum == 0`, assuming the counter resets per execution (inference, **verify**). Any later invocation or tool event opens one implicitly. The status line's `agent_state` leaving `idle` also opens a turn. | `Stop`: `terminationReason` `model_stop` closes `ok`, `error` and `max_steps_exceeded` close `error`, other values close `cancelled` (inference, **verify** the full list). If another hook returns `continue`, the agent keeps going after Cusco closes the turn. The next `PreInvocation` reopens a new turn, and that `Stop` carries `continuation_active` (docs). |
| Turn failure | `Stop` with `terminationReason: "error"` or a non-empty `error` (docs) | n/a |
| User interrupt | Undocumented. Whether `Stop` fires on Esc or Ctrl+C is unknown. Fallback: the status line returns to `agent_state: "idle"`, which closes the turn as `cancelled` (inference). | n/a |
| Subagent | Each subagent is its own conversation (docs). Hooks inside a subagent probably carry the **subagent's** `conversationId`, with no link to the parent (inference, **verify**). Opening it from the parent's `PreToolUse` for `invoke_subagent` is possible, but subagents run asynchronously, so its `PostToolUse` likely fires at launch, not at completion (inference, **verify**). The status line's `subagents[] {name, role, status}` gives running/completed state without IDs (docs). | Status line `status: completed`. `manage_subagents` kill, or killed/idle states, have no hook (docs). |
| Background or parallel agents | `Stop.fullyIdle == false` means background commands or subagents remain (docs). `/swarm`, `/teamwork` and `/boost` create named workers (docs). These appear only as status-line `subagents[]` or as separate conversations (inference). | `fullyIdle == true` on a later `Stop` (**verify** a second `Stop` fires), or the status line shows no running subagents |
| Tool approval requested | **No hook.** Status line `tool_confirmation_pending: true` (docs). Subagent confirmations are relayed to the root conversation's UI (docs: CLI 1.1.1), so the request belongs to the turn, not the subagent. The agent-initiated `ask_permission` tool shows up as `PreToolUse` with `toolCall.name == "ask_permission"` (docs: tool name). | Status line `tool_confirmation_pending` absent or false. `PostToolUse` for the same `toolCall.id` closes an `ask_permission` request. A denial has no dedicated signal. |
| Question to the user | `PreToolUse` with `toolCall.name == "ask_question"` (docs: tool name). The CLI also raises terminal notifications for pending questions (docs: CLI 1.3.1). | `PostToolUse` with the same `toolCall.id` |
| MCP input request | Not documented | n/a |
| Idle or waiting notification | None. `Stop` with `fullyIdle: true`, or `agent_state: "idle"`. | n/a |
| Ordering and concurrency | Hooks are synchronous in the loop, so per-conversation order is probably sequential (inference). Parallel tool calls and hook concurrency are undocumented. The status line is debounced to 300 ms and is a snapshot, not an event stream, so it maps naturally to `sync {scope: session}` (inference). | n/a |

### 4.2 Antigravity App and IDE (hooks only)

The same hook rows apply, but there is no status line. So **tool approvals and subagent
state are invisible.** Only `ask_question` and `ask_permission` tool calls can produce
`needs_input`. The App can't tell an approval wait from a long-running tool, because both
look like `PreToolUse` without `PostToolUse` (inference). Elapsed time must not turn that
into `needs_input`.

Sessions never get an end signal, and one App process hosts many conversations (docs:
multiple concurrent sessions). Process liveness closes them only when the whole App quits,
the same problem as the Claude Desktop open question (inference).

### 4.3 What challenges Cusco's current model

1. **`needs_input` for approvals requires a non-hook source.** Antigravity has no
   `PermissionRequest` hook. Cusco needs a second capture path, the status line command,
   and it exists only in the interactive CLI. Its snapshot shape fits `sync` better than
   `open` and `close`.
2. **Subagents are separate conversations.** Without a parent ID in hook payloads, Cusco
   would show a subagent as a sibling session, not as `…/turn/agent:` under its parent.
3. **No turn id.** Turns must be synthesized from `PreInvocation`, `invocationNum` and
   `Stop`. `executionNum` arrives only at the end.
4. **`observes` needs new words.** For App and IDE: approvals are not observable, but
   questions are. Today's vocabulary (`request`, `request:turn`) can't say that.
5. **Process naming.** The client is "antigravity" but the binary is `agy`. The desktop
   runs `Antigravity` plus `language_server_macos_arm`. Gemini CLI runs as `node` (§5).
   The "executable or `argv[0]` equals client name" rule needs a per-client list of names.

### 4.4 Gemini CLI

| Cusco concept | Signal (docs) |
| --- | --- |
| Session start, resume, clear | `SessionStart` with `source` `startup`, `resume` or `clear`. On `clear`, close the session previously running in that process, as Cusco does for Claude Code. |
| Session end | `SessionEnd` |
| Turn start and end | `BeforeAgent` and `AfterAgent`. There is no turn id, so key the turn by a per-session counter (inference). |
| Approval requested | `Notification` with `notification_type: "ToolPermission"` (docs). Whether `details` carries the tool name and input is **verify**. |
| Approval answered | `AfterTool` for the matching tool input digest, or the next `BeforeTool` after one second, as with Codex (inference) |
| Interrupt | No hook. Close the turn on the next `BeforeAgent` or on `SessionEnd`, as with Claude Code (inference). |
| Subagents | No events (docs) |

`observes` for Gemini CLI: `["session", "turn", "request:turn"]`.

## 5. Process and identity

| Surface | Executable and argv | Process model |
| --- | --- | --- |
| Antigravity CLI | `~/.local/bin/agy`, installed by `curl -fsSL https://antigravity.google/cli/install.sh \| bash` (docs). Whether it is a symlink into a versioned directory is unknown (**verify**: `readlink`, `ps -o comm,args`). | One `agy` per terminal session (inference). The CLI runs its own "language server" (docs: CLI 1.1.14), probably a child process (inference). So hooks may be grandchildren of `agy`, and Cusco's parent walk must pass through it (**verify**). |
| CLI daemon modes | `agy remote-control start` registers a background daemon with the OS service manager (launchd on macOS), and it persists across logouts (docs: CLI 1.2.0). `agy acp` runs under an editor (docs). | The daemon hosts conversations without a terminal: a shared process with many sessions and one pid (inference) |
| Antigravity App, 2.x | `/Applications/Antigravity.app` from the DMG (docs) | Electron app hosting many conversations (docs, inference). The 1.x desktop spawned a Go `language_server_macos_arm` per window: two language-server PIDs started in the same second (local install). Under Cusco's recovery rule the App would be a supervisor of the language server (inference). |
| Antigravity IDE | Separate download; its app data directory is `antigravity-ide` (docs) | Like the App (inference) |
| Gemini CLI | `gemini` from npm, Homebrew or MacPorts (docs). It's a Node script, so the kernel reports `node` as the executable, and the `gemini` path is in `argv[1]` (inference). | One process per session (inference) |

**Payload identity:**
- No Antigravity or Gemini payload carries a pid (docs).
- Antigravity payloads carry no surface field. The status line's `product` is just `"antigravity"`.
- The surface can be derived from `transcriptPath`/`artifactDirectoryPath`, whose middle component is `antigravity`, `antigravity-cli` or `antigravity-ide` (inference from docs).
- The status-line docs' own example shows a CLI transcript under `~/.gemini/antigravity/…`, which contradicts this (docs). **Verify.**

## 6. Persistent state

| Location | Contents (field names and structure only) |
| --- | --- |
| `<app_data_dir>/brain/<conversationId>/.system_generated/logs/transcript.jsonl` | Per-conversation JSONL transcript (docs). Schema undocumented. Not present locally because it is pre-2.x. Subagent transcripts "remain readable in JSONL logs" (docs). |
| `<app_data_dir>/brain/<conversationId>/` | Artifacts. Locally (1.x): `task.md`, `implementation_plan.md` and `walkthrough.md`, each with `.metadata.json` (keys `artifactType`, `summary`, `updatedAt`, `version`), `.resolved` and `.resolved.N` versions, and a `scratch/` folder (local install) |
| CLI conversation store | SQLite `.db` (plus `.db-wal`) is "CLI's conversation format", with a "shared SQLite summary store" (docs: CLI 1.0.4, 1.0.5, 1.0.16, 1.2.2). Conversations are scoped to the working directory where `agy` was launched (docs). The exact path is undocumented. Deleting a conversation removes a database file, which suggests one file per conversation (inference). |
| `~/.gemini/antigravity/conversations/<uuid>.pb` | 1.x desktop conversation store, one file per conversation. High-entropy bytes with no protobuf structure visible, so probably encrypted (local install, inference). Not usable for recovery. |
| `~/.gemini/antigravity/annotations/<uuid>.pbtxt` | Text protobuf with a single field `last_user_view_time` (local install). Relevant to Cusco's "finished, unseen" open question. |
| `~/.gemini/antigravity/implicit/<uuid>.pb`, `code_tracker/{active,history}`, `knowledge/`, `context_state/`, `user_settings.pb`, `installation_id`, `mcp_config.json` (`mcpServers`), `browserAllowlist.txt` | 1.x desktop state (local install) |
| `~/.gemini/antigravity/daemon/ls_<hash>.log` and `.json` | Language-server logs and the discovery file holding ports and CSRF token (local install). Don't read the `.json`. |
| `~/.gemini/antigravity-cli/settings.json` | CLI settings: `agentMode`, `toolPermission`, `permissions {allow, deny, ask}`, `trustedWorkspaces`, `statusLine`, `modelProvider` (docs) |
| `~/.gemini/config/` | Shared: `hooks.json`, `mcp_config.json`, `agents/`, `sidecars/`, `config.json` (docs) |
| `~/.antigravity/` | 1.x IDE: `argv.json` (`crash-reporter-id`, `enable-crash-reporter`), `extensions/` and the `antigravity/bin/` launcher symlinks (local install) |
| `~/Library/Application Support/Antigravity/` | Electron user data in VS Code layout: `User/`, `logs/<timestamp>/{ls-main.log, main.log, window*/exthost, …}` (local install) |
| Gemini CLI | Hooks receive `transcript_path` (docs). The usual on-disk layout, `~/.gemini/tmp/<project-hash>/chats/`, is from prior knowledge and wasn't checked this session (inference, **verify**). |

**Recovery:**
- A transcript-based `sync` would work only if the 2.x `transcript.jsonl` records step states such as a pending confirmation or a running subagent. Its schema is undocumented (**verify**).
- For the CLI, the first status-line tick after Cusco restarts is itself an authoritative `sync`, but only while a terminal is rendering (inference).

## 7. Proposed adapter

Two capture paths share one adapter, `antigravity`:
- `cusco hook antigravity <Event>`, registered in `~/.gemini/config/hooks.json`. It covers the CLI, App and IDE.
- `cusco statusline antigravity`, registered in the CLI's `statusLine`. It sends a snapshot datagram, then runs the user's previous status-line command, if any, and prints that command's output unchanged. Otherwise it prints nothing.

The surface comes from the `app_data_dir` component of `transcriptPath`, falling back to the
parent-process walk.

**Allowed fields** (hook command):
- `conversationId`, `hookEventName` (or the argv event), `cwd` or `workspacePaths[0]`, `transcriptPath`
- `toolCall.name`, `toolCall.id`, `stepIdx`, `invocationNum`, `initialNumSteps`
- `executionNum`, `terminationReason`, whether `error` is non-empty, `fullyIdle`, `continuation_active`
- A digest of `toolCall.name` plus `toolCall.args`

Drop `lastUserInput`, `toolCall.args` and `modelName`.

**Allowed fields** (status line):
- `conversation_id`, `cwd`, `agent_state`, `tool_confirmation_pending`, `pending_input_count`, `task_count`
- `subagents[].{name, role, status}`, `version`, `transcript_path`

Drop `email`, `conversation_title`, `quota`, `cost`, `plan_tier` and `model`.

| Source | Cusco event |
| --- | --- |
| `SessionStart` | `open` session keyed by `conversationId`, with attrs `cwd` and `surface` |
| `PreInvocation` | If no turn is open in the session, `open` a turn keyed by a per-session counter. `invocationNum == 0` while a turn is open means a new execution: close the old turn as `cancelled` and open a new one (**verify** the counter resets per execution). |
| `PreToolUse` | Open the turn implicitly. Remember `toolCall.id` (or `stepIdx`) with its digest. If the tool is `ask_question`, `open` a request with reason `question` keyed by `toolCall.id`. If it is `ask_permission`, `open` a request with reason `approval` keyed the same way. If it is `invoke_subagent`, note the call for subagent linking (§8). |
| `PostToolUse` | `close` the request with that `toolCall.id`, if one is open |
| `Stop` | `close` the turn: `model_stop` closes `ok`, `error` (or a non-empty `error` field) closes `error` with the reason, anything else closes `cancelled`. If `fullyIdle` is false, `open` agent `background` under that turn. Close it on a later `Stop` with `fullyIdle: true`, or on a status-line `idle` with no running subagents. |
| status line tick (CLI) | `sync {scope: session}`. `agent_state` in `working`, `tool_use`, `reviewing` or `initializing` means an open turn; `idle` means none; `error` closes the turn as `error`. `tool_confirmation_pending: true` means an open request with reason `approval` under the turn, keyed `confirm`. Each `subagents[]` entry with `status: running` becomes an open agent keyed `name:role`. |
| `agy` process exit | `close` session (liveness check) |

**`observes`:**

| Surface | Value |
| --- | --- |
| CLI with status line | `["session", "turn", "agent", "request:turn"]`. Approvals are attributed to the turn because subagent confirmations bubble up to the root. |
| CLI without status line, and App/IDE | `["session", "turn", "request:question"]`. This needs a new token meaning only questions are visible, not approvals (§4.3). |

**Registration by `cusco setup`:**

1. In `~/.gemini/config/hooks.json`, add or replace the one top-level entry `"cusco"`:
   ```json
   {
     "cusco": {
       "SessionStart":  [ { "type": "command", "command": "cusco hook antigravity SessionStart",  "timeout": 5 } ],
       "PreInvocation": [ { "type": "command", "command": "cusco hook antigravity PreInvocation", "timeout": 5 } ],
       "PreToolUse":  [ { "matcher": "", "hooks": [ { "type": "command", "command": "cusco hook antigravity PreToolUse",  "timeout": 5 } ] } ],
       "PostToolUse": [ { "matcher": "", "hooks": [ { "type": "command", "command": "cusco hook antigravity PostToolUse", "timeout": 5 } ] } ],
       "Stop":          [ { "type": "command", "command": "cusco hook antigravity Stop",          "timeout": 5 } ]
     }
   }
   ```
   - `PostInvocation` isn't needed.
   - The `PreToolUse`/`PostToolUse` matcher could narrow to `ask_question|ask_permission|invoke_subagent` to cut traffic. Keep `""` if tool activity should confirm turns.
   - The hook must print nothing and exit 0, never a `decision`.
2. In `~/.gemini/antigravity-cli/settings.json`, set `statusLine: { "type": "command", "command": "cusco statusline antigravity", "stack_with_default": true }`.
   - Save any existing `statusLine.command` so the wrapper can chain to it and `cusco setup --remove` can restore it.
3. Gemini CLI, if supported later, gets a separate `gemini` adapter. Register `SessionStart`, `SessionEnd`, `BeforeAgent`, `AfterAgent`, `BeforeTool`, `AfterTool` and `Notification` in `~/.gemini/settings.json` `hooks`. Mapping per §4.4. Process names: `node` plus `argv[1]` ending in `gemini`.

**Process names for the `antigravity` client:** `agy`, `Antigravity` (the App or IDE main
executable, **verify**) and `language_server_macos_arm`/`language_server_macos_x64`
(**verify** the 2.x and CLI names).

## 8. Gaps and verify items

Each item can be settled with a recorder: a temporary global hook entry and status-line
command that append the raw JSON shape (keys and IDs only) and `ps -o pid,ppid,comm,args`
of the parent chain to a scratch file. Run each scenario once on the CLI and once in the App.

1. **Subagent hooks:**
   - Do hooks fire inside subagents, and with whose `conversationId`?
   - Does the parent's `invoke_subagent` `PostToolUse` fire at launch or at completion?
   - Can a subagent's conversation ID be linked to its parent? Possible sources: `toolCall.args` of `send_message` or `manage_subagents`, or the parent transcript.
   - Scenario: invoke one subagent that needs an approval.
2. **Approval timeline:**
   - Does `PreToolUse` fire before the confirmation dialog?
   - Does the status line flip `tool_confirmation_pending` promptly, including for a subagent's request?
   - What is seen on deny: `PostToolUse` with `error`, or nothing?
3. **Interrupt:** does Esc or Ctrl+C mid-turn fire `Stop`, and with which `terminationReason`? Collect the full list of `terminationReason` values.
4. **Background work:** after `Stop` with `fullyIdle: false`, does a second `Stop` fire when the work ends?
5. **Session boundaries:** does `SessionStart` fire on `agy -c`, `--conversation <id>`, `/resume`, `/clear`, `/new` and `/fork`? Do the old conversation's hooks stop?
6. **Hook output:** confirm that empty stdout and exit 0 leave `PreToolUse` and `Stop` behavior unchanged, and that a non-zero exit (Cusco down) changes nothing. The docs say `decision` is "required" (docs).
7. **App and IDE parity:**
   - Do the desktop surfaces send `hookEventName`, `cwd` and `toolCall.id`? Only CLI 1.5.4 documents them.
   - Is there any approval signal in the App?
   - Is `transcriptPath` really surface-specific? The status-line example contradicts this.
8. **Concurrency:** parallel tool calls and hook ordering across the global, workspace and plugin hook files.
9. **Process tree:**
   - Is `~/.local/bin/agy` a symlink to a versioned binary?
   - Is the hook's parent `agy` or a language-server child?
   - What does the remote-control daemon's tree look like?
   - What are the App's executable names in 2.x?
10. **Global hook trust:** confirm there is no approval step for `~/.gemini/config/hooks.json`, and check whether an organization `admin_settings.json` can disable user hooks.
11. **Persistent state:** the schema of `transcript.jsonl` and the CLI's SQLite conversation path, to evaluate transcript `sync`.
12. **Gemini CLI:**
    - Does `Notification` `details` carry the tool name and input?
    - Does `AfterAgent` fire on interrupt?
    - Do hooks run in parallel by default? The default value of `sequential` isn't stated.
    - Is the on-disk session layout as described in §6?
13. **Pages that didn't give what I needed:**
    - `/docs/install` is an authentication page, so install details came from `/docs/getting-started`.
    - `/docs/gcli-migration` is only a redirect, so I read `/docs/cli/gcli-migration`.
    - The changelog was too long for the fetch tool, so CLI entries were extracted from the page's HTML.
    - Not read: `/docs/cli/commands/resume`, `/docs/remote-control`, `/docs/cli/acp`.
14. **Local install:** the installed data is from the 1.x desktop, and no current binary is present. Everything about 2.x and CLI layout comes from the docs.

## 9. Sources

All URLs were read on 18 November 2026.

**Web**
- https://developers.googleblog.com/an-important-update-transitioning-gemini-cli-to-antigravity-cli/
- https://antigravity.google/docs/hooks?tab=cli and https://antigravity.google/docs/hooks (App, CLI and IDE tabs)
- https://antigravity.google/docs/gcli-migration (redirect) and https://antigravity.google/docs/cli/gcli-migration
- https://antigravity.google/docs/subagents
- https://antigravity.google/docs/permissions
- https://antigravity.google/docs/install
- https://antigravity.google/docs/getting-started
- https://antigravity.google/docs/cli/conversations
- https://antigravity.google/docs/cli/reference
- https://antigravity.google/docs/cli/statusline
- https://antigravity.google/docs/cli/headless
- https://antigravity.google/docs/sidecars
- https://antigravity.google/docs/sdk/lifecycle
- https://antigravity.google/docs/changelog and https://antigravity.google/docs/changelog?tab=cli (App 2.x, CLI 1.0.0–1.5.11, IDE anchors)
- https://github.com/google-gemini/gemini-cli (README)
- https://github.com/google-gemini/gemini-cli/blob/main/docs/hooks/reference.md
- https://github.com/google-gemini/gemini-cli/blob/main/docs/hooks/index.md
- GitHub API `repos/google-gemini/gemini-cli/releases/latest`

**Local paths** (read-only; structure and field names only)
- `~/.antigravity/` (`argv.json` keys, `extensions/`, `antigravity/bin/{agy,antigravity}` symlink targets)
- `~/.gemini/` (`GEMINI.md` exists and is empty) and `~/.gemini/antigravity/`: `conversations/`, `implicit/`, `annotations/`, `brain/`, `daemon/`, `code_tracker/`, `knowledge/`, `context_state/`, `mcp_config.json` (top-level keys only)
- `~/.gemini/antigravity/daemon/ls_*.log` (startup lines, version strings and flag names; tokens redacted)
- `~/Library/Application Support/Antigravity/` (directory listing, `logs/*/ls-main.log` flag names, version strings)
- Absent: `/Applications/Antigravity.app`, `~/.local/bin/agy`, `~/.gemini/settings.json`, `~/.gemini/config/`, any `gemini` binary
- `docs/design.md` (this repository)
