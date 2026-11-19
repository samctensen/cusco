#!/bin/sh
# Updates Eywa the way it was installed: with Homebrew, or with cargo from the
# same git repository or checkout (a checkout is built as it is; pull first).
# If that changed the binary, it restarts the service when `eywa start` runs
# it, and updates the hooks of each agent that already has them. With nothing
# new, it changes nothing.
#
#   curl -fsSL https://raw.githubusercontent.com/samctensen/eywa/main/scripts/update.sh | sh

set -eu

say() { printf '%s\n' "$*"; }
die() {
    printf 'eywa update: %s\n' "$*" >&2
    exit 1
}

main() {
    [ $# -eq 0 ] || die "takes no arguments"
    # The launchd job serves the default directory; that's the copy to update.
    unset EYWA_DIR
    eywa=$(command -v eywa) || die "eywa isn't installed, or isn't on your PATH"
    old=$("$eywa" --version)
    old=${old#eywa }
    before=$(cksum <"$eywa")

    cargo_home=${CARGO_HOME:-$HOME/.cargo}
    case $(readlink -f "$eywa") in
        */Cellar/eywa/*)
            say "Updating eywa $old with Homebrew"
            brew upgrade eywa
            ;;
        *)
            [ "$eywa" -ef "$cargo_home/bin/eywa" ] ||
                die "$eywa wasn't installed by Homebrew or cargo; update it the way you installed it"
            cargo_install "$cargo_home/.crates.toml"
            ;;
    esac

    if [ "$(cksum <"$eywa")" = "$before" ]; then
        say "eywa $old is up to date"
        return
    fi
    new=$("$eywa" --version)
    new=${new#eywa }
    if [ "$new" = "$old" ]; then
        say "Rebuilt eywa $new"
    else
        say "Updated eywa from $old to $new"
    fi

    if [ "$("$eywa" autostart)" = on ]; then
        "$eywa" autostart on >/dev/null
    fi
    restart
    update_hooks
}

# Runs cargo install again from the source in cargo's install record.
cargo_install() {
    from=$(sed -n 's/^"eywa [^ ]* (\(.*\))" = .*/\1/p' "$1" 2>/dev/null) || true
    case $from in
        git+*)
            url=${from#git+}
            url=${url%%#*}
            repo=${url%%[?]*}
            case $url in
                "$repo") set -- --git "$repo" ;;
                *'?branch='*) set -- --git "$repo" --branch "${url#*[?]branch=}" ;;
                *) die "eywa is pinned to ${url#*[?]}; reinstall it with cargo install yourself" ;;
            esac
            say "Updating eywa $old with cargo from $repo"
            ;;
        path+file://*)
            path=${from#path+file://}
            [ -d "$path" ] || die "can't find $path, the checkout eywa was installed from"
            commit=$(git -C "$path" log -1 --format=' (%h %s)' 2>/dev/null) || commit=
            say "Updating eywa $old with cargo from $path$commit"
            set -- --path "$path"
            ;;
        *) die "can't tell where cargo installed eywa from; reinstall it with cargo install yourself" ;;
    esac
    cargo install --locked "$@"
}

# The running service still runs the old binary. launchd's job is restarted;
# an `eywa serve` someone started by hand is left to them.
restart() {
    "$eywa" status --depth 0 >/dev/null 2>&1 || return 0
    # The label is `LABEL` in src/launchd.rs.
    if launchctl print "gui/$(id -u)/io.github.samctensen.eywa" >/dev/null 2>&1; then
        "$eywa" stop >/dev/null
        "$eywa" start >/dev/null
        say "Restarted the service"
    else
        say "Restart your eywa serve to run the new version"
    fi
}

# Runs `eywa setup` for each agent that already has Eywa's hook, so a new
# version's hooks replace the old ones without adding any to other agents.
update_hooks() {
    set --
    has_hook "${CLAUDE_CONFIG_DIR:-$HOME/.claude}/settings.json" && set -- "$@" claude
    has_hook "${CODEX_HOME:-$HOME/.codex}/hooks.json" && set -- "$@" codex
    has_hook "${GROK_HOME:-$HOME/.grok}/hooks/eywa.json" && set -- "$@" grok
    has_hook "${XDG_CONFIG_HOME:-$HOME/.config}/muse/settings.json" && set -- "$@" muse
    [ $# -gt 0 ] || return 0
    output=$("$eywa" setup "$@") || {
        say "$output"
        exit 1
    }
    say "$output"
    case $output in
        *"added hooks"*) say "Start a new session in each agent whose hooks changed, so it loads them" ;;
    esac
}

# `eywa setup` writes a hook whose command is Eywa's absolute path, alone or
# followed by `hook <agent>`.
has_hook() {
    [ -f "$1" ] && grep -Eq "\"command\": *\"'?/[^\"]*/eywa('? hook [a-z]+)?\"" "$1"
}

# Called last, so a download cut short runs nothing.
main "$@"
