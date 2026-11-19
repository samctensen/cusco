//! Claude Code's session registry: one JSON file per running process in
//! `~/.claude/sessions/`, with the session's id, process id and status
//! (`busy`, `waiting` or `idle`). It states what hooks can only imply: an
//! interrupted turn, a granted approval, and every session's state after
//! Cusco restarts.
//!
//! Claude rewrites the files in place, so the watcher holds a kqueue watch on
//! each file as well as on the folder. It reads the folder again whenever
//! anything changes; nothing polls.

use std::collections::HashMap;
use std::ffi::{CString, c_int};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

/// One running Claude Code process, as its registry file describes it.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionState {
    pub session_id: String,
    pub pid: u32,
    pub status: Status,
    pub cwd: Option<String>,
    pub entrypoint: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Busy,
    /// Waiting on the user, with what for, when the file says.
    Waiting(Option<String>),
    Idle,
}

/// `$CLAUDE_CONFIG_DIR/sessions`, or `~/.claude/sessions`.
pub fn default_dir() -> PathBuf {
    let config = std::env::var_os("CLAUDE_CONFIG_DIR").map_or_else(
        || PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".claude"),
        PathBuf::from,
    );
    config.join("sessions")
}

/// Every readable registry file in `dir`. A file caught mid-write fails to
/// parse and is skipped; its next change is read again.
pub fn read_states(dir: &Path) -> Vec<SessionState> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .filter_map(|path| parse(&std::fs::read(path).ok()?))
        .collect()
}

fn parse(bytes: &[u8]) -> Option<SessionState> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let text = |key: &str| value.get(key).and_then(|v| v.as_str()).map(String::from);
    let session_id = text("sessionId").filter(|id| {
        !id.is_empty() && id.len() <= crate::record::MAX_ID_BYTES && !id.contains('/')
    })?;
    let pid = u32::try_from(value.get("pid")?.as_u64()?).ok()?;
    let status = match text("status")?.as_str() {
        "busy" => Status::Busy,
        "idle" => Status::Idle,
        "waiting" => Status::Waiting(value.get("waitingFor").map(|reason| match reason {
            serde_json::Value::String(reason) => reason.clone(),
            other => other.to_string(),
        })),
        _ => return None,
    };
    Some(SessionState {
        session_id,
        pid,
        status,
        cwd: text("cwd"),
        entrypoint: text("entrypoint"),
    })
}

/// Calls `deliver` with every session's state now, and again whenever a
/// registry file or the folder changes. Returns without watching if the
/// folder doesn't exist.
pub fn watch(dir: PathBuf, deliver: impl Fn(Vec<SessionState>) + Send + 'static) {
    std::thread::spawn(move || {
        // SAFETY: kqueue takes no arguments; a negative result is an error.
        let queue = unsafe { libc::kqueue() };
        if queue < 0 {
            return;
        }
        let Some(folder) = watch_path(queue, &dir) else {
            // SAFETY: `queue` is a descriptor this thread opened.
            unsafe { libc::close(queue) };
            return;
        };
        let mut files: HashMap<PathBuf, c_int> = HashMap::new();
        loop {
            refresh_watches(queue, &dir, &mut files);
            deliver(read_states(&dir));
            // SAFETY: kevent is plain data, so all-zero bytes are valid.
            let mut event: libc::kevent = unsafe { std::mem::zeroed() };
            // SAFETY: `event` has room for the one event requested.
            let received = unsafe {
                libc::kevent(queue, std::ptr::null(), 0, &mut event, 1, std::ptr::null())
            };
            if received < 0
                && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted
            {
                break;
            }
        }
        for descriptor in files.into_values().chain([folder, queue]) {
            // SAFETY: each descriptor was opened by this thread.
            unsafe { libc::close(descriptor) };
        }
    });
}

/// Opens `path` for change notification only and adds it to the queue.
fn watch_path(queue: c_int, path: &Path) -> Option<c_int> {
    let name = CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: `name` is a valid NUL-terminated path.
    let descriptor = unsafe { libc::open(name.as_ptr(), libc::O_EVTONLY) };
    if descriptor < 0 {
        return None;
    }
    let change = libc::kevent {
        ident: descriptor as usize,
        filter: libc::EVFILT_VNODE,
        flags: libc::EV_ADD | libc::EV_CLEAR,
        fflags: libc::NOTE_WRITE
            | libc::NOTE_EXTEND
            | libc::NOTE_ATTRIB
            | libc::NOTE_DELETE
            | libc::NOTE_RENAME,
        data: 0,
        udata: std::ptr::null_mut(),
    };
    // SAFETY: `change` is one valid kevent; no events are requested back.
    let added =
        unsafe { libc::kevent(queue, &change, 1, std::ptr::null_mut(), 0, std::ptr::null()) };
    if added < 0 {
        // SAFETY: `descriptor` was just opened here.
        unsafe { libc::close(descriptor) };
        return None;
    }
    Some(descriptor)
}

/// Watches files that appeared and stops watching files that went away.
fn refresh_watches(queue: c_int, dir: &Path, files: &mut HashMap<PathBuf, c_int>) {
    let present: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
                .collect()
        })
        .unwrap_or_default();
    files.retain(|path, descriptor| {
        let keep = present.contains(path);
        if !keep {
            // SAFETY: the descriptor was opened by `watch_path`.
            unsafe { libc::close(*descriptor) };
        }
        keep
    });
    for path in present {
        if !files.contains_key(&path)
            && let Some(descriptor) = watch_path(queue, &path)
        {
            files.insert(path, descriptor);
        }
    }
}
