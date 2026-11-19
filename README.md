# Cusco

**Which of my coding agents needs me?**

A background service for macOS that watches Claude Code, Codex, Grok Build and
Muse Code, and tells any program which agents are working, idle or waiting on
you.

```console
$ cusco status --requests | jq -r '.rows[] | "\(.attrs.reason)\t\(.id)"'
approval	/claude/session:7f3a/turn:12/agent:a2/request:t9
question	/codex/session:019a/turn:4/request:call_8
```

Your agents' hooks report to Cusco, and anything can subscribe over a local
socket: the `cusco` CLI, a menu bar app, keyboard lights, a tmux status line.

## Install

```sh
brew install samctensen/tap/cusco
brew services start cusco   # runs Cusco now and at every login
cusco setup                 # adds Cusco's hook to each agent you have
```

Then:

- **Codex:** run `/hooks` and trust the new hooks.
- **Grok Build:** trust the folders you work in.
- Start a new session in each agent, so it loads the hooks.

`cusco status` should now list your agents. The hooks never slow down or block
an agent, even when Cusco isn't running.

`cusco setup` only adds its own entries to each agent's settings
(`~/.claude/settings.json`, `~/.codex/hooks.json`, `~/.grok/hooks/cusco.json`,
`~/.config/muse/settings.json`), and it's safe to run again.

### Update

```sh
brew upgrade cusco
brew services restart cusco
```

### Without Homebrew

```sh
cargo install --git https://github.com/samctensen/cusco   # needs Rust 1.88+
cusco setup
cusco serve   # runs in the foreground; stop it with ⌃C or cusco stop
```

### Uninstall

```sh
cusco setup --remove
brew services stop cusco
brew uninstall cusco
```

## Commands

| Command | What it does |
| --- | --- |
| `cusco setup [agent…]` | Adds Cusco's hook to the named agents (`claude`, `codex`, `grok`, `muse`), or to every one installed. |
| `cusco setup --remove [agent…]` | Takes the hooks out again. |
| `cusco serve` | Runs the service in the foreground. `brew services` does this for you. |
| `cusco stop` | Stops the service. Under `brew services`, it stays stopped until `brew services restart cusco` or your next login. |
| `cusco status` | Prints one snapshot and exits. |
| `cusco watch` | Prints a snapshot, then every change, until ⌃C or Cusco stops. |
| `cusco visualize` | A live, full-screen view of every agent. ⌃C quits. |
| `cusco --version` | Prints the version. |

`status` and `watch` print JSON lines in [the API's format](docs/api.md), and
take these options:

| Option | Meaning |
| --- | --- |
| `--path <id>` | Start at this node, for example `/claude`, instead of the whole machine. |
| `--depth <n>` | Levels below the path to include: machine, agent app, session, turn, subagent. `0` is the node alone; the default is `1`. |
| `--requests` | List every open request instead of the tree. |

`status` and `stop` exit with `1` if Cusco isn't running, and `2` on bad
options.

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
