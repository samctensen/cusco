# Cusco

**Which of my coding agents needs me?**

Cusco is a small background service for macOS that watches the coding agents
on your machine (Claude Code, Codex, Grok Build and Muse Code) and tells any
program which ones are working, which are idle and which are waiting on you.

```console
$ cusco status --requests | jq -r '.rows[] | "\(.attrs.reason)\t\(.id)"'
approval	/claude/session:7f3a/turn:12/agent:a2/request:t9
question	/codex/session:019a/turn:4/request:call_8
```

You keep using your agents the way you do today. Their hooks report lifecycle
events to Cusco, which keeps one live picture of every session, turn, subagent
and pending approval. Any program can read that picture or subscribe to it over
a local socket: the `cusco` command line, a menu bar app, keyboard lights, a
Stream Deck key, a tmux status line. Cusco defines the data, and each consumer
decides how to show it.

The name comes from the Inca capital, the hub of a road network along which
relay runners carried messages. Here the hooks are the runners.

> **Status:** v0.2, macOS only. Supports Claude Code, Codex, Grok Build and Muse
> Code.

## Install

Install with Homebrew, start the service and connect your agents:

```sh
brew install samctensen/tap/cusco
brew services start cusco
cusco setup
```

- `brew services` runs Cusco in the background, starts it at login and restarts
  it if it crashes. `cusco stop` stops it until `brew services restart cusco`
  or your next login.
- `cusco setup` adds Cusco's hook to each agent it finds on your machine. It
  keeps your other settings and other tools' hooks, and it's safe to run again.

| Agent | Hook file |
| --- | --- |
| Claude Code | `~/.claude/settings.json` |
| Codex | `~/.codex/hooks.json` |
| Grok Build | `~/.grok/hooks/cusco.json` |
| Muse Code | `~/.config/muse/settings.json` |

Then finish up in your agents:

1. **Codex:** run `/hooks` and trust the new hooks. Codex skips hooks until
   they're trusted.
2. **Grok Build:** trust the folders you work in. Grok skips hooks in untrusted
   folders.
3. Start a new session in each agent, so it picks up the hooks.

Check that it's working:

```sh
cusco status
```

The hook command forwards a short list of identifiers and exits immediately. It
never writes output, never fails and never waits for Cusco, so your agents
aren't slowed down or blocked even when Cusco isn't running.

### Update

```sh
brew upgrade cusco
brew services restart cusco
```

The hooks point at Homebrew's stable `bin/cusco` link, so they keep working
across upgrades and Codex keeps trusting them.

### Without Homebrew

With a Rust toolchain (1.88 or later):

```sh
cargo install --git https://github.com/samctensen/cusco
cusco setup
cusco serve
```

`cusco serve` runs in the foreground; keep it running however you like, and
stop it with ⌃C or `cusco stop`.

### Uninstall

```sh
cusco setup --remove
brew services stop cusco
brew uninstall cusco
```

## Commands

| Command | What it does |
| --- | --- |
| `cusco setup` | Adds Cusco's hook to every supported agent on this machine. |
| `cusco setup claude codex` | Adds it to the named agents only: `claude`, `codex`, `grok`, `muse`. |
| `cusco setup --remove` | Takes Cusco's hooks out again. Accepts agent names too. |
| `cusco serve` | Runs the service in the foreground. `brew services` does this for you. |
| `cusco stop` | Stops the running service and waits until it has. Under `brew services`, it stays stopped until `brew services restart cusco` or your next login. |
| `cusco status` | Prints one snapshot and exits. |
| `cusco watch` | Prints a snapshot, then every change as it happens, until you press ⌃C or Cusco stops. |
| `cusco visualize` | A live, full-screen view of every agent in the terminal. ⌃C quits. |
| `cusco --version` | Prints the version. |

`status` and `watch` print JSON, one message per line, in the format described
in [the API reference](docs/api.md). They take the same options:

| Option | Meaning |
| --- | --- |
| `--path <id>` | Start at this node instead of the whole machine, for example `/claude`. |
| `--depth <n>` | How many levels below the path to include. `0` is the node alone. The default is `1`. |
| `--requests` | List every open request across all agents instead of the tree. |

The levels are machine (`/`), agent app (`/claude`), session, turn and
subagent, so `--depth 2` stops at sessions.

`status` and `stop` exit with `1` if Cusco isn't running, and `2` if the
options are wrong.

### Recipes

These use [jq](https://jqlang.org).

```sh
# What's waiting on me, and why
cusco status --requests | jq -r '.rows[] | "\(.attrs.reason)\t\(.id)"'

# One line for a status bar
cusco status --depth 0 | jq -r '.rows[0].counts | "\(.needs_input) waiting · \(.working) working"'

# Every Claude Code session, its state and its folder
cusco status --path /claude | jq -r '.rows[] | select(.kind == "session") | "\(.state)\t\(.attrs.cwd)"'

# Follow Codex as it changes
cusco watch --path /codex --depth 2
```

## Build on Cusco

Cusco has no UI of its own. Anything that can open a Unix socket can show your
agents' state. Connect to `api.sock` in `~/Library/Application Support/cusco/`,
send one JSON line and read JSON lines back:

```sh
echo '{"v":1,"op":"subscribe","path":"/","depth":1}' \
  | nc -U "$HOME/Library/Application Support/cusco/api.sock"
```

```jsonl
{"v":1,"type":"snapshot","rev":41,"rows":[{"id":"/","kind":"machine","state":"needs_input",…},{"id":"/claude",…},{"id":"/codex",…}]}
{"v":1,"type":"upsert","rev":44,"row":{"id":"/claude","kind":"client","state":"idle",…}}
```

Keep a map of rows by `id`. The snapshot replaces the map, `upsert` sets a row
and `remove` deletes one. When the connection closes, reconnect and subscribe
again. Every row already summarizes everything beneath it, so a status light
needs only the machine row.

- **[API reference](docs/api.md):** every field and its type, when each one is
  present, how to subscribe correctly, and complete subscribers in Python and
  TypeScript.
- **[JSON Schema](docs/api-v1.schema.json):** the same contract in
  machine-readable form, to validate messages or generate types.

## How it works

Cusco keeps a tree of what it has observed:

```
/                                  machine
└─ /claude                         client (agent app)
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

## Privacy

Prompts, responses, commands, tool arguments and tool output never leave the
hook command. Cusco receives identifiers, event names, tool names, the working
directory and the transcript path. Tool input is reduced to a hash, used only
to match an approval with the tool call it belongs to. Cusco also reads Claude
Code's session status files in `~/.claude/sessions` (session ids, process ids
and status), never transcripts. Everything stays on your machine, in a
directory only you can read.

## Development

```sh
cargo test
cargo clippy --all-targets
cargo fmt --check
```

To see exactly what your agents send, set `CUSCO_RECORD=/path/to/file.jsonl`
in the environment they run in; the hook command appends each record (allowed
fields only) to that file. To run a second copy of Cusco alongside your own,
point both it and your agents at another directory with `CUSCO_DIR`.

## License

[MIT](LICENSE)
