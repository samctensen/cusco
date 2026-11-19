//! Runs the service as a per-user launchd job, so it keeps running in the
//! background and comes back if it crashes. `eywa start` and `eywa stop`
//! load and unload the job. `eywa autostart on` puts a copy of its plist in
//! `~/Library/LaunchAgents`, which launchd loads at every login.
//!
//! There's one job per user, for the default directory. A copy of Eywa
//! pointed elsewhere with `EYWA_DIR` runs with `eywa serve` instead.

use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

pub const LABEL: &str = "io.github.samctensen.eywa";
/// Where the job writes its output, in Eywa's directory.
pub const LOG: &str = "eywa.log";
/// The plist `start` loads. It sits in Eywa's directory rather than in
/// `~/Library/LaunchAgents`, so starting Eywa doesn't also turn on autostart.
const PLIST: &str = "launchd.plist";

/// The job runs `eywa serve` on `dir`, and launchd runs it again unless it
/// exits cleanly, which `eywa stop` makes it do.
pub fn plist(executable: &Path, dir: &Path) -> String {
    let string = |path: &Path| format!("<string>{}</string>", escape(&path.to_string_lossy()));
    let log = string(&dir.join(LOG));
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    {executable}
    <string>serve</string>
  </array>
  <key>EnvironmentVariables</key>
  <dict>
    <key>EYWA_DIR</key>
    {dir}
  </dict>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <dict>
    <key>SuccessfulExit</key>
    <false/>
  </dict>
  <key>StandardOutPath</key>
  {log}
  <key>StandardErrorPath</key>
  {log}
</dict>
</plist>
"#,
        executable = string(executable),
        dir = string(dir),
    )
}

/// Loads the job from a fresh plist, after unloading any job already loaded,
/// so launchd starts `eywa serve` now. `dir` must exist.
pub fn start(executable: &Path, dir: &Path) -> io::Result<()> {
    let path = dir.join(PLIST);
    write_plist(&path, &plist(executable, dir))?;
    stop()?;
    let path = path.to_string_lossy();
    // launchd can still be tearing down a job it just unloaded.
    for _ in 0..20 {
        if launchctl(&["bootstrap", &domain(), &path]).is_ok() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    launchctl(&["bootstrap", &domain(), &path])
}

/// Unloads Eywa's job. launchd stops it if it's still running. Autostart
/// stays as it is.
pub fn stop() -> io::Result<()> {
    if loaded(LABEL) {
        launchctl(&["bootout", &format!("{}/{LABEL}", domain())])?;
    }
    Ok(())
}

/// Whether launchd has Eywa's job loaded, running or not.
pub fn loaded(label: &str) -> bool {
    Command::new("launchctl")
        .args(["print", &format!("{}/{label}", domain())])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Whether launchd starts Eywa at login.
pub fn autostart() -> bool {
    agent_plist(LABEL).exists()
}

/// Turns autostart on. A job already loaded keeps running as it is.
pub fn enable_autostart(executable: &Path, dir: &Path) -> io::Result<()> {
    let path = agent_plist(LABEL);
    if let Some(agents) = path.parent() {
        std::fs::create_dir_all(agents)?;
    }
    write_plist(&path, &plist(executable, dir))
}

/// Turns autostart off. A job already loaded keeps running until
/// `eywa stop` or logout.
pub fn disable_autostart() -> io::Result<()> {
    remove(&agent_plist(LABEL))
}

fn agent_plist(label: &str) -> PathBuf {
    let home = std::env::var_os("HOME").unwrap_or_default();
    PathBuf::from(home).join(format!("Library/LaunchAgents/{label}.plist"))
}

/// launchd refuses a plist that others can write.
fn write_plist(path: &Path, text: &str) -> io::Result<()> {
    std::fs::write(path, text)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644))
}

fn remove(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The launchd domain of the user's login session.
fn domain() -> String {
    // SAFETY: getuid has no preconditions and can't fail.
    format!("gui/{}", unsafe { libc::getuid() })
}

fn launchctl(args: &[&str]) -> io::Result<()> {
    let output = Command::new("launchctl").args(args).output()?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(io::Error::other(format!(
        "launchctl {}: {}",
        args.join(" "),
        stderr.trim()
    )))
}
