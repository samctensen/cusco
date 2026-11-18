//! `cusco setup`: adds Cusco's hook to each client's settings, or removes it.
//! Other settings and other tools' hooks are left exactly as they were.

use crate::record::{CLAUDE_EVENTS, CODEX_EVENTS};
use serde_json::{Map, Value, json};
use std::io;
use std::path::{Path, PathBuf};

/// One client's hook settings file.
pub struct Target {
    pub client: &'static str,
    pub path: PathBuf,
    events: &'static [&'static str],
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Added,
    Removed,
    Unchanged,
}

/// Each supported client's settings file, honoring `CLAUDE_CONFIG_DIR` and
/// `CODEX_HOME`.
pub fn targets() -> Vec<Target> {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let dir = |variable: &str, default: &str| {
        std::env::var_os(variable).map_or_else(|| home.join(default), PathBuf::from)
    };
    vec![
        Target {
            client: "claude",
            path: dir("CLAUDE_CONFIG_DIR", ".claude").join("settings.json"),
            events: CLAUDE_EVENTS,
        },
        Target {
            client: "codex",
            path: dir("CODEX_HOME", ".codex").join("hooks.json"),
            events: CODEX_EVENTS,
        },
    ]
}

/// The path hooks should run. A Homebrew install runs from a versioned
/// Cellar directory, so hooks use the stable link in the prefix's `bin`
/// instead: upgrades then keep working, and Codex keeps trusting them.
pub fn hook_executable() -> io::Result<PathBuf> {
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

/// Adds the hook to every event the client supports, replacing any earlier
/// Cusco hook, or with `remove` takes Cusco's hooks out.
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
        _ => json!({}),
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
    remove_cusco_hooks(hooks);
    if !remove {
        for event in target.events {
            let group = json!({"hooks": [{"type": "command", "command": command, "timeout": 2}]});
            let groups = hooks.entry(*event).or_insert_with(|| json!([]));
            if let Some(groups) = groups.as_array_mut() {
                groups.push(group);
            }
        }
    }
    if hooks.is_empty() {
        root.remove("hooks");
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

/// Removes every hook that runs `cusco hook`, then any group or event left
/// empty by that.
fn remove_cusco_hooks(hooks: &mut Map<String, Value>) {
    for groups in hooks.values_mut() {
        let Some(groups) = groups.as_array_mut() else {
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
