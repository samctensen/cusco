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

/// Resolves symbolic links in a full-path executable, because the kernel
/// reports resolved paths (`/tmp/x` runs as `/private/tmp/x`). A bare name
/// is returned unchanged.
fn resolve_executable(executable: &str) -> String {
    if !executable.contains('/') {
        return executable.to_string();
    }
    std::fs::canonicalize(executable)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|_| executable.to_string())
}

/// Whether an executable path matches a resolved executable: by file name,
/// or by full path when it contains a `/`.
fn matches_executable(path: &Path, executable: &str) -> bool {
    if executable.contains('/') {
        path == Path::new(executable)
    } else {
        path.file_name() == Some(OsStr::new(executable))
    }
}

/// The nearest process at or above `start` running `executable`.
pub fn find_ancestor(start: u32, executable: &str) -> Option<u32> {
    let executable = resolve_executable(executable);
    let mut pid = start;
    while pid > 1 {
        if executable_path(pid).is_some_and(|path| matches_executable(&path, &executable)) {
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

/// Running processes whose executable matches one of `executables`, using
/// the same rule as `find_ancestor`.
pub fn list_processes(executables: &[(&'static str, String)]) -> Vec<(&'static str, u32)> {
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
    let executables: Vec<(&'static str, String)> = executables
        .iter()
        .map(|(client, executable)| (*client, resolve_executable(executable)))
        .collect();
    pids.into_iter()
        .filter(|pid| *pid > 1)
        .filter_map(|pid| {
            let pid = pid as u32;
            let path = executable_path(pid)?;
            executables
                .iter()
                .find(|(_, executable)| matches_executable(&path, executable))
                .map(|(client, _)| (*client, pid))
        })
        .collect()
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
