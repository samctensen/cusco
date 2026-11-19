# Rename Cusco to Eywa

## Why

Cusco read as a joke. The service keeps one tree of everything it has observed,
and every node's state sums up everything beneath it. Eywa, the great tree in
Avatar that connects every living thing on Pandora, names that directly.

## Decisions

- **Clean break.** Eywa contains no code that finds, migrates or cleans up a
  Cusco install. Anyone moving over runs `cusco setup --remove`, stops Cusco
  and uninstalls it first.
- **Already done:** the GitHub repo is `samctensen/eywa`. The Cusco v0.2.0,
  v0.2.1 and v0.3.0 releases and tags are deleted. The `cusco` formula is
  deleted from `samctensen/homebrew-tap` (tap PR #4), and tap PR #3 is closed.

## Approaches

1. **Clean break** (chosen): rename everything; no compatibility code.
2. **Eywa cleans up after Cusco:** `eywa setup` would also match `cusco hook`
   entries, and `eywa start` would take over the `io.github.samctensen.cusco`
   job the way it takes over `brew services`. It eases upgrades, but it is
   compatibility code for a handful of installs, and deleting the releases
   already ends the upgrade path.

## What changes

| Cusco | Eywa |
| --- | --- |
| crate and binary `cusco` | `eywa` |
| "Cusco" in prose | "Eywa" |
| `CUSCO_DIR`, `CUSCO_RECORD` | `EYWA_DIR`, `EYWA_RECORD` |
| `~/Library/Application Support/cusco` | `~/Library/Application Support/eywa` |
| `cusco.log` | `eywa.log` |
| launchd `io.github.samctensen.cusco` | `io.github.samctensen.eywa` |
| `homebrew.mxcl.cusco`, `sh.brew.cusco` | `homebrew.mxcl.eywa`, `sh.brew.eywa` |
| `~/.grok/hooks/cusco.json` | `~/.grok/hooks/eywa.json` |
| `/Cellar/cusco/` | `/Cellar/eywa/` |
| `.json.cusco-tmp` staging files | `.json.eywa-tmp` |
| `is_cusco_hook` and friends | `is_eywa_hook` and friends |
| `github.com/samctensen/cusco` URLs | `github.com/samctensen/eywa` |

The paragraph in `docs/design.md` on where the name comes from is rewritten
around the tree.

## What stays

- The API: every message, field and value, and protocol version `1`.
- Every behavior. The rename changes names only.
- Dated plans and specs under `docs/superpowers/` written before this one keep
  "cusco", since they record past work. (Claude's call.)

## Left to the builder

- The first Eywa release is v0.4.0, published through the tap's PR and
  `brew pr-pull` flow. (Claude's call.)
- This branch starts from `feat/alert-latency`, whose unmerged commits touch
  the same files. (Claude's call.)
- After it merges, move this Mac over: `cusco setup --remove`,
  `brew services stop cusco`, `brew uninstall cusco`, then install Eywa,
  `eywa setup` and `eywa start`.
