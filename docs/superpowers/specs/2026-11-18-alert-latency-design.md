# Alert latency: quick wins

Status: proposed, 18 November 2026. A small change.

## Why

Cusco exists to say which agent needs you. The time between an agent changing
state and a subscriber hearing about it should be as short as Cusco can make it.

Measured on one Mac (median of 100 runs, event to subscriber line, the hook
spawned by a test harness in place of the agent):

| Path | Latency |
| --- | --- |
| Service alone: datagram in, subscriber line out | 0.06 ms |
| Hook run directly, no shell | 2.2 ms |
| Hook through `sh -c` (Claude Code, Grok Build as set up today) | 4.3 ms |
| Hook through `$SHELL -c` (Muse Code) | 4.6 ms |
| Hook through `$SHELL -lc` (Codex, a login shell) | 18.3 ms |
| Claude session registry change | 0.6 to 1.5 ms |
| Claude interrupt, or a denied approval that stops the agent | 2,004 ms |

The service is not the bottleneck. Two things Cusco controls are: a fixed
two-second wait before an interrupted Claude turn shows as `idle`, and a shell
that starts before every Claude and Grok hook.

## Goals

1. A Claude session whose registry file goes `idle` shows `idle` at once, in
   about a millisecond.
2. Claude Code and Grok Build run the hook with no shell in between.

## Not in this change

- Codex's login shell. Codex has no shell-free form, and changing its entry
  makes the user trust the hook again.
- The transitions Codex and Muse never report (a granted approval, a failed
  turn, an interrupt). Those need a second source per client.
- HTTP hooks. They would save under 2 ms and send whole payloads to the service.
- The length of the settle time, and the one-second sibling window.

## What must not happen

- A session stuck on `working` because a late hook reopened a turn that ended.
- A turn that finished normally reported as `cancelled`, even briefly.
- A hook that blocks, slows or fails an agent.
- More data leaving the hook command.

## Approaches

**The two-second wait**

| Approach | Trade-off |
| --- | --- |
| **State now, outcome later (chosen).** Close the turn the moment the registry says `idle`; decide `ok`, `error` or `cancelled` within the settle time. | `state` is immediate. `last_turn` trails it by up to two seconds after an interrupt. If Claude's registry ever flickers to `idle` mid-turn, subscribers see the flicker. |
| Shorten the wait to a few hundred milliseconds. | Still a wait, and the safe length depends on how long after `idle` a `Stop` hook can arrive, which isn't measured. |
| Write `cancelled` at once and correct it if `Stop` arrives. | Every normal turn end could flash `cancelled`. Ruled out above. |

**The shell**

| Approach | Trade-off |
| --- | --- |
| **`command` plus `args` for Claude Code and Grok Build (chosen).** Both document it as spawning the executable directly. | Saves about 2 ms per event. A client version that ignores `args` would run bare `cusco`; see the guard below. |
| Leave the shell form. | No risk, no gain. |

## Design

### Idle at once, outcome later

When Claude's registry says a session went `idle` and a turn is still open:

- The turn closes immediately. As today, the main thread's open requests close
  with it and its subagents keep running. `last_turn` is cleared: the turn
  before no longer says how the latest one ended, so `idle` with no
  `last_turn` means the outcome is still to come.
- For the settle time (still two seconds), the session's turn is *unsettled*:

| What arrives | Effect |
| --- | --- |
| Main-thread `Stop` | `last_turn` is `ok`. |
| Main-thread `StopFailure` | `last_turn` is `error`. |
| `UserPromptSubmit` with a different turn id | `last_turn` is `cancelled`; the new turn opens as usual. |
| `UserPromptSubmit` with the same turn id | The turn continues: it reopens and nothing is written. |
| Any other main-thread record | Ignored, so a late tool hook can't reopen the turn. |
| A subagent's record | Handled as usual. |
| The registry says `busy` or `waiting` | The idle didn't hold: the turn reopens and nothing is written. |
| Nothing, for the settle time | `last_turn` is `cancelled`. |

`last_turn.ended_at` is when the registry went idle, whichever row decides the
outcome. An `idle` seen for the first time, at startup, behaves as it does now.

How it is built (my calls, open to change in the plan):

- The tree gains a sixth event, `end {id}`: close a node with no outcome. The
  outcome arrives as an `attr` on the session, and one helper builds the
  `last_turn` value for both paths.
- The Claude adapter remembers each session's unsettled turn and applies the
  table. The core keeps the timer and tells the adapter when it runs out.

### No shell for Claude Code and Grok Build

- `cusco setup` writes
  `{"type": "command", "command": "<path>", "args": ["hook", "<client>"], "timeout": 2}`
  for Claude Code and Grok Build. Codex and Muse Code keep the string form.
- A Cusco entry in the old form is rewritten where it sits on the next
  `cusco setup`, and `--remove` takes out either form.
- **Guard (my call):** `cusco` run with no arguments and with input that isn't a
  terminal exits 0 and prints nothing. A client that ignored `args` would
  otherwise get the usage error, and exit code 2 blocks a tool call on the
  events that can block. Typed at a terminal, bare `cusco` still prints usage.

## Tests

Kept few, by behavior:

- The registry test: after `idle`, the session is `idle` with no `last_turn`
  from that turn, and then `cancelled`.
- One adapter replay: `idle` then `Stop` gives `ok`; `idle` then a late tool
  record stays `idle`; `idle` then a new prompt gives `cancelled`.
- The setup test: Claude and Grok entries carry `args`, Codex and Muse don't,
  and an old-form entry is replaced in place.
- The command-line test: bare `cusco` with piped input exits 0, silently.

## Documents to update

- `docs/design.md`: the event table, the registry's `idle` row, and the Capture
  paragraph on how hooks are run.
- `docs/api.md` and the schema's description: `last_turn` can trail `state` by
  up to two seconds after an interrupt.

## Checked against real clients

Checked on 18 November 2026 with Claude Code 2.1.301 (the CLI) and 2.1.325 (the
build bundled with the desktop app), and Grok Build 1.0.89. The samples are
small.

1. **`args` is honored.** A headless session of each Claude Code build and of
   Grok Build, given the entry `cusco setup` writes for it, delivered
   `SessionStart`, `UserPromptSubmit`, `Stop` and `SessionEnd` records through
   it. A shell-form hook beside it confirmed that hooks ran.
2. **`idle` and the `Stop` hook arrive in either order.** Over three turn ends
   on live desktop sessions, the `Stop` record reached Cusco 19 ms and 18 ms
   before the registry's `idle`, and once 179 ms after it. In the headless
   sessions it came first. When `idle` is first, a subscriber sees `idle` with
   no `last_turn`, then the outcome.
3. **The registry does go `idle` and back within a moment.** Twice in five
   minutes a session's status went `idle` and then `busy`, 42 ms and 122 ms
   apart. Why isn't known. Subscribers now see that as a brief `idle`; the
   two-second wait used to hide it unless a `Stop` hook had closed the turn.
4. **At a turn's start the registry's `busy` beat the `UserPromptSubmit`
   hook** in the headless sessions, by 29 ms and 332 ms. Replacing the
   `running` placeholder then writes `last_turn: ok` for a turn that has only
   started. That is older than this change and is not fixed by it.

Still to check: the oldest version of either client that honors `args`.

## Rollout

Existing installs get the new hook form by running `cusco setup` again.
