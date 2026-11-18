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

/// Whether an executable path matches: by file name, or by full path when
/// `executable` contains a `/`.
pub(crate) fn matches_executable(path: &Path, executable: &str) -> bool {
    if executable.contains('/') {
        path == Path::new(executable)
    } else {
        path.file_name() == Some(OsStr::new(executable))
    }
}

/// The nearest process at or above `start` running `executable`.
pub fn find_ancestor(start: u32, executable: &str) -> Option<u32> {
    let mut pid = start;
    while pid > 1 {
        if executable_path(pid).is_some_and(|path| matches_executable(&path, executable)) {
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
