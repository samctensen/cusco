//! `cusco setup`: adds Cusco's hook to each client's settings, or removes it.
//! Other settings and other tools' hooks are left exactly as they were.

use crate::record::{CLAUDE_EVENTS, CODEX_EVENTS, GROK_EVENTS, MUSE_EVENTS};
use serde_json::{Map, Value, json};
use std::io;
use std::path::{Path, PathBuf};

/// One client's hook settings file.
pub struct Target {
    pub client: &'static str,
    /// The client's configuration folder; its presence means the client is installed.
    pub home: PathBuf,
    pub path: PathBuf,
    events: &'static [&'static str],
    /// The settings a new file starts with.
    seed: fn() -> Value,
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Added,
    Removed,
    Unchanged,
}

/// Each supported client's settings file, honoring `CLAUDE_CONFIG_DIR`,
/// `CODEX_HOME`, `GROK_HOME` and `XDG_CONFIG_HOME`.
pub fn targets() -> Vec<Target> {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let dir = |variable: &str, default: &str| {
        std::env::var_os(variable).map_or_else(|| home.join(default), PathBuf::from)
    };
    let claude = dir("CLAUDE_CONFIG_DIR", ".claude");
    let codex = dir("CODEX_HOME", ".codex");
    let grok = dir("GROK_HOME", ".grok");
    let muse = dir("XDG_CONFIG_HOME", ".config").join("muse");
    let empty = || json!({});
    vec![
        Target {
            client: "claude",
            path: claude.join("settings.json"),
            home: claude,
            events: CLAUDE_EVENTS,
            seed: empty,
        },
        Target {
            client: "codex",
            path: codex.join("hooks.json"),
            home: codex,
            events: CODEX_EVENTS,
            seed: empty,
        },
        // Grok reads every file in its hooks folder, so Cusco gets its own.
        Target {
            client: "grok",
            path: grok.join("hooks").join("cusco.json"),
            home: grok,
            events: GROK_EVENTS,
            seed: empty,
        },
        Target {
            client: "muse",
            path: muse.join("settings.json"),
            home: muse,
            events: MUSE_EVENTS,
            seed: || json!({"schema_version": 1}),
        },
    ]
}

/// The path hooks and the launchd job run Cusco by. A Homebrew install runs
/// from a versioned Cellar directory, so they use the stable link in the
/// prefix's `bin` instead: upgrades then keep working, and Codex keeps
/// trusting the hooks.
pub fn stable_executable() -> io::Result<PathBuf> {
    let executable = std::env::current_exe()?;
    let text = executable.to_string_lossy();
    if let Some(index) = text.find("/Cellar/cusco/") {
        let linked = Path::new(&text[..index]).join("bin/cusco");
        if linked.exists() {
            return Ok(linked);
        }
    }
    Ok(executable)
}

/// The hook command for a client, quoting the path if it needs it.
pub fn hook_command(executable: &Path, client: &str) -> String {
    let path = executable.to_string_lossy();
    let plain = path
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-+".contains(c));
    if plain {
        format!("{path} hook {client}")
    } else {
        format!("'{}' hook {client}", path.replace('\'', r"'\''"))
    }
}

/// Adds the hook to every event the client supports, or with `remove` takes
/// Cusco's hooks out. A Cusco hook already present is updated where it sits:
/// Codex trusts hooks by their position, so moving one would untrust the
/// hooks after it.
pub fn apply(target: &Target, command: &str, remove: bool) -> io::Result<Outcome> {
    let original = match std::fs::read_to_string(&target.path) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    if remove && original.is_none() {
        return Ok(Outcome::Unchanged);
    }
    let mut settings: Value = match &original {
        Some(text) if !text.trim().is_empty() => serde_json::from_str(text).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{} is not valid JSON: {error}", target.path.display()),
            )
        })?,
        _ => (target.seed)(),
    };
    let Some(root) = settings.as_object_mut() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{} is not a JSON object", target.path.display()),
        ));
    };
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()));
    let Some(hooks) = hooks.as_object_mut() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("\"hooks\" in {} is not an object", target.path.display()),
        ));
    };
    remove_cusco_hooks(hooks, |event| remove || !target.events.contains(&event));
    if !remove {
        for event in target.events {
            let groups = hooks.entry(*event).or_insert_with(|| json!([]));
            if let Some(groups) = groups.as_array_mut()
                && !update_cusco_hook(groups, command)
            {
                groups.push(json!({"hooks": [hook_entry(command)]}));
            }
        }
    }
    if hooks.is_empty() {
        root.remove("hooks");
    }
    // A file left with nothing in it was Cusco's alone.
    if remove && root.is_empty() {
        std::fs::remove_file(&target.path)?;
        return Ok(Outcome::Removed);
    }
    let mut text = serde_json::to_string_pretty(&settings)?;
    text.push('\n');
    if original.as_deref() == Some(text.as_str()) {
        return Ok(Outcome::Unchanged);
    }
    if !remove
        && original.is_none()
        && let Some(parent) = target.path.parent()
    {
        std::fs::create_dir_all(parent)?;
    }
    let staging = target.path.with_extension("json.cusco-tmp");
    std::fs::write(&staging, text)?;
    std::fs::rename(&staging, &target.path)?;
    Ok(if remove {
        Outcome::Removed
    } else {
        Outcome::Added
    })
}

fn hook_entry(command: &str) -> Value {
    json!({"type": "command", "command": command, "timeout": 2})
}

/// Points the event's first Cusco hook at `command`, in place, and drops any
/// duplicates. Returns whether there was one.
fn update_cusco_hook(groups: &mut Vec<Value>, command: &str) -> bool {
    let mut found = false;
    for group in groups.iter_mut() {
        let Some(entries) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
            continue;
        };
        entries.retain_mut(|entry| {
            if !is_cusco_hook(entry) {
                return true;
            }
            if found {
                return false;
            }
            found = true;
            *entry = hook_entry(command);
            true
        });
    }
    groups.retain(|group| {
        group
            .get("hooks")
            .and_then(Value::as_array)
            .is_none_or(|entries| !entries.is_empty())
    });
    found
}

/// Removes every hook that runs `cusco hook` from the chosen events, then
/// any group or event left empty by that.
fn remove_cusco_hooks(hooks: &mut Map<String, Value>, chosen: impl Fn(&str) -> bool) {
    for (event, groups) in hooks.iter_mut() {
        let Some(groups) = groups.as_array_mut().filter(|_| chosen(event)) else {
            continue;
        };
        for group in groups.iter_mut() {
            if let Some(entries) = group.get_mut("hooks").and_then(Value::as_array_mut) {
                entries.retain(|entry| !is_cusco_hook(entry));
            }
        }
        groups.retain(|group| {
            group
                .get("hooks")
                .and_then(Value::as_array)
                .is_none_or(|entries| !entries.is_empty())
        });
    }
    hooks.retain(|_, groups| groups.as_array().is_none_or(|groups| !groups.is_empty()));
}

fn is_cusco_hook(entry: &Value) -> bool {
    let Some(command) = entry.get("command").and_then(Value::as_str) else {
        return false;
    };
    let Some((program, arguments)) = command.rsplit_once(" hook ") else {
        return false;
    };
    let program = program.trim_matches('\'');
    !arguments.contains(' ') && Path::new(program).file_name() == Some("cusco".as_ref())
}
