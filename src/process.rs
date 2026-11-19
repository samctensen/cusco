//! macOS process inspection: executable paths and parent processes.

use std::ffi::{OsStr, OsString, c_int, c_void};
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};

pub fn executable_path(pid: u32) -> Option<PathBuf> {
    let mut buffer = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    // SAFETY: the buffer is valid for `buffer.len()` bytes.
    let length = unsafe {
        libc::proc_pidpath(
            pid as c_int,
            buffer.as_mut_ptr().cast::<c_void>(),
            buffer.len() as u32,
        )
    };
    if length <= 0 {
        return None;
    }
    buffer.truncate(length as usize);
    Some(PathBuf::from(OsString::from_vec(buffer)))
}

pub fn parent_pid(pid: u32) -> Option<u32> {
    // SAFETY: proc_bsdinfo is plain data, so all-zero bytes are a valid value.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as c_int;
    // SAFETY: `info` is valid for `size` bytes.
    let written = unsafe {
        libc::proc_pidinfo(
            pid as c_int,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast::<c_void>(),
            size,
        )
    };
    (written == size).then_some(info.pbi_ppid)
}

/// How Cusco recognizes a client's processes.
#[derive(Clone, Debug)]
pub struct ProcessRule {
    pub client: &'static str,
    /// An exact file name, a file-name prefix ending in `*`, or a full path.
    pub executable: String,
    pub hosting: Hosting,
}

/// Whether a client's process runs one session or hosts many.
#[derive(Clone, Copy, Debug)]
pub enum Hosting {
    /// One session per process, so a running process that hasn't reported
    /// is an unconfirmed session.
    Session,
    /// Every process hosts many sessions.
    Host,
    /// Hosts many sessions when launched with any of these arguments.
    HostWhen(&'static [&'static str]),
}

impl ProcessRule {
    pub fn new(
        client: &'static str,
        executable: impl Into<String>,
        hosting: Hosting,
    ) -> ProcessRule {
        ProcessRule {
            client,
            executable: executable.into(),
            hosting,
        }
    }

    /// The rules for the clients Cusco supports. See `docs/research/`.
    pub fn defaults() -> Vec<ProcessRule> {
        vec![
            ProcessRule::new("claude", "claude", Hosting::Session),
            // The CLI and the desktop app both run hooks from the shared app server.
            ProcessRule::new("codex", "codex", Hosting::Host),
            ProcessRule::new("grok", "grok", Hosting::HostWhen(&["agent"])),
            // A launcher script runs `muse-bin-<version>`.
            ProcessRule::new("muse", "muse-bin-*", Hosting::HostWhen(&["serve", "acp"])),
        ]
    }

    pub fn for_client(client: &str) -> Option<ProcessRule> {
        ProcessRule::defaults()
            .into_iter()
            .find(|rule| rule.client == client)
    }

    /// The rule with symbolic links resolved in a full-path executable,
    /// because the kernel reports resolved paths (`/tmp/x` runs as
    /// `/private/tmp/x`).
    fn resolved(&self) -> ProcessRule {
        let mut rule = self.clone();
        if rule.executable.contains('/')
            && let Ok(path) = std::fs::canonicalize(&rule.executable)
        {
            rule.executable = path.to_string_lossy().into_owned();
        }
        rule
    }

    /// Whether process `pid`, running `path`, belongs to this client. A name
    /// also matches the name the process was launched as, for installs that
    /// run a versioned binary through a symbolic link.
    fn matches(&self, pid: u32, path: &Path) -> bool {
        if self.executable.contains('/') {
            return path == Path::new(&self.executable);
        }
        let named = |path: &Path| {
            path.file_name()
                .and_then(OsStr::to_str)
                .is_some_and(|name| name_matches(name, &self.executable))
        };
        named(path) || launch_name(pid).is_some_and(|name| named(Path::new(&name)))
    }

    fn hosts(&self, pid: u32) -> bool {
        match self.hosting {
            Hosting::Session => false,
            Hosting::Host => true,
            Hosting::HostWhen(markers) => arguments(pid).is_some_and(|args| {
                args.iter()
                    .skip(1)
                    .any(|arg| markers.contains(&arg.as_str()))
            }),
        }
    }
}

fn name_matches(name: &str, pattern: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => name.starts_with(prefix),
        None => name == pattern,
    }
}

/// A process's arguments, `argv[0]` first.
pub fn arguments(pid: u32) -> Option<Vec<String>> {
    let mut argmax: c_int = 0;
    let mut size = std::mem::size_of::<c_int>();
    let mut name = [libc::CTL_KERN, libc::KERN_ARGMAX];
    // SAFETY: `argmax` is valid for `size` bytes.
    let read = unsafe {
        libc::sysctl(
            name.as_mut_ptr(),
            2,
            (&mut argmax as *mut c_int).cast::<c_void>(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if read != 0 || argmax <= 0 {
        return None;
    }
    let mut buffer = vec![0u8; argmax as usize];
    let mut size = buffer.len();
    let mut name = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid as c_int];
    // SAFETY: the buffer is valid for `size` bytes.
    let read = unsafe {
        libc::sysctl(
            name.as_mut_ptr(),
            3,
            buffer.as_mut_ptr().cast::<c_void>(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if read != 0 {
        return None;
    }
    // Layout: argc, the executable path, NUL padding, then argc strings.
    let count_bytes = buffer.get(..std::mem::size_of::<c_int>())?;
    let count = c_int::from_ne_bytes(count_bytes.try_into().ok()?);
    let after_argc = buffer.get(std::mem::size_of::<c_int>()..size)?;
    let after_path = &after_argc[after_argc.iter().position(|byte| *byte == 0)?..];
    let mut rest = &after_path[after_path.iter().position(|byte| *byte != 0)?..];
    let mut args = Vec::new();
    for _ in 0..count.max(0) {
        let end = rest
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(rest.len());
        args.push(String::from_utf8_lossy(&rest[..end]).into_owned());
        rest = rest.get(end + 1..).unwrap_or_default();
    }
    Some(args)
}

/// The name a process was launched as: its `argv[0]`. Unlike the executable
/// path, it keeps the name of a symbolic link the process was started
/// through, such as `~/.local/bin/claude` for `…/claude/versions/2.1.301`.
pub fn launch_name(pid: u32) -> Option<String> {
    arguments(pid)?.into_iter().next()
}

/// The nearest process at or above `start` that belongs to the rule's client.
pub fn find_ancestor(start: u32, rule: &ProcessRule) -> Option<u32> {
    let rule = rule.resolved();
    let mut pid = start;
    while pid > 1 {
        if executable_path(pid).is_some_and(|path| rule.matches(pid, &path)) {
            return Some(pid);
        }
        pid = parent_pid(pid)?;
    }
    None
}

/// Best-effort guess at where a client process runs.
pub fn surface(pid: u32) -> &'static str {
    let path = executable_path(pid)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    if path.contains("/Library/Application Support/Claude/") || path.contains("/app-server-daemon/")
    {
        "desktop"
    } else {
        "cli"
    }
}

/// Running processes that each run one session of a client: the processes
/// that, if they haven't reported, stand for an unconfirmed session. Hosts
/// and supervisors are left out.
pub fn list_processes(rules: &[ProcessRule]) -> Vec<(&'static str, u32)> {
    // SAFETY: a null buffer asks only for the number of processes.
    let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    if count <= 0 {
        return Vec::new();
    }
    // Leave room for processes started between the two calls.
    let mut pids = vec![0 as c_int; count as usize + 64];
    let size = (pids.len() * std::mem::size_of::<c_int>()) as c_int;
    // SAFETY: the buffer is valid for `size` bytes.
    let listed = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast::<c_void>(), size) };
    if listed <= 0 {
        return Vec::new();
    }
    pids.truncate(listed as usize);
    let rules: Vec<ProcessRule> = rules.iter().map(ProcessRule::resolved).collect();
    let matches: Vec<(&'static str, u32, bool)> = pids
        .into_iter()
        .filter(|pid| *pid > 1)
        .filter_map(|pid| {
            let pid = pid as u32;
            let path = executable_path(pid)?;
            let rule = rules.iter().find(|rule| rule.matches(pid, &path))?;
            Some((rule.client, pid, rule.hosts(pid)))
        })
        .collect();
    // Hooks claim the nearest matching ancestor, so a match that is itself
    // an ancestor of another match (a supervisor) would never be claimed.
    let supervisors: std::collections::HashSet<(&'static str, u32)> = matches
        .iter()
        .flat_map(|(client, pid, _)| {
            ancestors(*pid)
                .into_iter()
                .map(move |above| (*client, above))
        })
        .collect();
    matches
        .into_iter()
        .filter(|(client, pid, host)| !host && !supervisors.contains(&(*client, *pid)))
        .map(|(client, pid, _)| (client, pid))
        .collect()
}

fn ancestors(pid: u32) -> Vec<u32> {
    let mut found = Vec::new();
    let mut current = pid;
    while let Some(parent) = parent_pid(current).filter(|parent| *parent > 1) {
        found.push(parent);
        current = parent;
    }
    found
}

/// Calls `on_exit` from a new thread when process `pid` exits, or right away
/// if it has already exited.
pub fn watch_exit(pid: u32, on_exit: impl FnOnce() + Send + 'static) {
    std::thread::spawn(move || {
        if wait_for_exit(pid) {
            on_exit();
        }
    });
}

/// Blocks until `pid` exits. Returns false only if the wait itself failed.
fn wait_for_exit(pid: u32) -> bool {
    // SAFETY: kqueue takes no arguments; a negative result is an error.
    let queue = unsafe { libc::kqueue() };
    if queue < 0 {
        return false;
    }
    let change = libc::kevent {
        ident: pid as usize,
        filter: libc::EVFILT_PROC,
        flags: libc::EV_ADD | libc::EV_ONESHOT,
        fflags: libc::NOTE_EXIT,
        data: 0,
        udata: std::ptr::null_mut(),
    };
    // SAFETY: `change` is one valid kevent; no events are requested back.
    let registered =
        unsafe { libc::kevent(queue, &change, 1, std::ptr::null_mut(), 0, std::ptr::null()) };
    let exited = if registered < 0 {
        // ESRCH: the process is already gone.
        std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
    } else {
        loop {
            // SAFETY: kevent is plain data, so all-zero bytes are valid.
            let mut event: libc::kevent = unsafe { std::mem::zeroed() };
            // SAFETY: `event` has room for the one event requested.
            let received = unsafe {
                libc::kevent(queue, std::ptr::null(), 0, &mut event, 1, std::ptr::null())
            };
            if received > 0 {
                break true;
            }
            if received < 0
                && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted
            {
                break false;
            }
        }
    };
    // SAFETY: `queue` is a descriptor this function opened.
    unsafe { libc::close(queue) };
    exited
}
