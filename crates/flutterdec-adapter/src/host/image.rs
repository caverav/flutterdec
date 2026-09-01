//! The executable image one invocation runs, held as a descriptor.
//!
//! A pathname is not a promise. Anything the host verifies by reading a file and
//! then executes by naming a file is two different objects joined by a string
//! that a same-user process is free to re-point in between, and mode bits do not
//! close that gap: the owner of a `0500` file can `chmod` it back, rename it, or
//! unlink it and put another file in its place. The only way to make "the bytes
//! that ran are the bytes that were checked" true rather than likely is to stop
//! naming the file at all.
//!
//! So the verified bytes are turned into an inode here, a descriptor onto it is
//! held for the rest of the run, and every pathname to that inode is gone before
//! the caller does anything another process could synchronize against. On Linux
//! the inode is an anonymous `memfd` that never had a name, sealed so its
//! contents cannot change even for something that reaches the descriptor through
//! `/proc`. Elsewhere it is a file created inside the invocation's private
//! directory, opened, and unlinked before this module returns. The child is then
//! created with `execveat(AT_EMPTY_PATH)` — `execve("/dev/fd/N")` on platforms
//! without it — which resolves the descriptor, not a path.
//!
//! Scripts keep working, which is the reason the descriptor is inherited rather
//! than closed on exec: the kernel hands a `#!` interpreter `/dev/fd/N` as the
//! script to read, and that name resolves through the child's own descriptor
//! table. It is the image itself, read-only and sealed, so the child gaining a
//! second way to look at its own code costs nothing.

use super::HostError;
use std::ffi::{CString, OsStr};
use std::fs::File;
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::{AsRawFd, FromRawFd, OwnedFd};
use std::path::Path;

/// The empty path `execveat` needs alongside `AT_EMPTY_PATH`.
#[cfg(target_os = "linux")]
const EMPTY_PATH: &[u8] = b"\0";

/// `MFD_EXEC`, which only kernels that can also refuse an executable anonymous
/// file understand. Not in every `libc` release, and asking for it on an older
/// kernel is an error rather than a no-op, so it is tried and then dropped.
#[cfg(target_os = "linux")]
const MFD_EXEC: libc::c_uint = 0x0010;

/// A verified adapter image plus the exact `argv` and `envp` it is executed
/// with.
///
/// The argument and environment vectors are built here, before the fork,
/// because the process that finally calls `exec` runs between `fork` and `exec`
/// and may not allocate: everything it needs has to already exist.
pub(crate) struct ExecImage {
    fd: OwnedFd,
    /// Owns the bytes the pointer arrays below point at. Never read directly;
    /// the vectors of pointers are what `exec` passes to the kernel.
    _argv: Vec<CString>,
    _envp: Vec<CString>,
    argv_ptrs: Vec<*const libc::c_char>,
    envp_ptrs: Vec<*const libc::c_char>,
    #[cfg(not(target_os = "linux"))]
    fd_path: CString,
}

// The pointers are read-only views into `_argv`/`_envp`, which this value owns and
// never mutates after construction, so moving it between threads moves the whole
// graph together.
unsafe impl Send for ExecImage {}
unsafe impl Sync for ExecImage {}

impl ExecImage {
    /// Materialize `bytes` as an executable inode with no pathname.
    ///
    /// `scratch` is only used where anonymous files are unavailable, and even
    /// then the file it holds is unlinked before this returns.
    pub(crate) fn prepare(
        name: &str,
        bytes: &[u8],
        scratch: &Path,
        argv: Vec<CString>,
        envp: Vec<CString>,
    ) -> Result<Self, HostError> {
        let fd = open_image(name, bytes, scratch)?;
        #[cfg(not(target_os = "linux"))]
        let fd_path = CString::new(format!("/dev/fd/{}", fd.as_raw_fd()))
            .expect("a descriptor number has no NUL in it");
        Ok(Self {
            fd,
            argv_ptrs: pointers(&argv),
            envp_ptrs: pointers(&envp),
            _argv: argv,
            _envp: envp,
            #[cfg(not(target_os = "linux"))]
            fd_path,
        })
    }

    /// Replace the calling process with the image.
    ///
    /// Returns only when the execution failed, and then returns why.
    ///
    /// # Safety
    /// Called between `fork` and `exec`. Allocates nothing, takes no locks, and
    /// calls only async-signal-safe syscalls.
    pub(crate) unsafe fn exec(&self) -> std::io::Error {
        // The containment plan marks every inherited descriptor close-on-exec,
        // including this one, and a `#!` adapter needs it open on the other
        // side: the interpreter is handed `/dev/fd/N` and nothing else names the
        // script. Cleared last, so the sweep cannot undo it.
        libc::fcntl(self.fd.as_raw_fd(), libc::F_SETFD, 0);
        #[cfg(target_os = "linux")]
        libc::syscall(
            libc::SYS_execveat,
            self.fd.as_raw_fd(),
            EMPTY_PATH.as_ptr().cast::<libc::c_char>(),
            self.argv_ptrs.as_ptr(),
            self.envp_ptrs.as_ptr(),
            libc::AT_EMPTY_PATH,
        );
        #[cfg(not(target_os = "linux"))]
        libc::execve(
            self.fd_path.as_ptr(),
            self.argv_ptrs.as_ptr(),
            self.envp_ptrs.as_ptr(),
        );
        std::io::Error::last_os_error()
    }
}

/// `argv`/`envp` as the null-terminated vector `execve` expects.
fn pointers(values: &[CString]) -> Vec<*const libc::c_char> {
    values
        .iter()
        .map(|value| value.as_ptr())
        .chain(std::iter::once(std::ptr::null()))
        .collect()
}

/// One argument or environment entry, rejected rather than truncated if it
/// cannot survive the C boundary.
pub(crate) fn argument(value: &OsStr, what: &str) -> Result<CString, HostError> {
    CString::new(value.as_bytes()).map_err(|_| {
        HostError::Workspace(format!(
            "{what} contains a NUL byte and cannot be passed to a child"
        ))
    })
}

fn open_image(name: &str, bytes: &[u8], scratch: &Path) -> Result<OwnedFd, HostError> {
    #[cfg(target_os = "linux")]
    if let Some(fd) = anonymous_image(name, bytes) {
        return Ok(reserve(fd));
    }
    unlinked_image(name, bytes, scratch).map(reserve)
}

/// Keep the image off the descriptor numbers the child reassigns.
///
/// The containment plan moves its status pipe onto a fixed low descriptor and
/// the standard library moves the three standard streams onto theirs, both after
/// the fork and both with `dup2`, which silently replaces whatever was there. An
/// image that happened to be allocated one of those numbers would be executed as
/// a pipe.
fn reserve(fd: OwnedFd) -> OwnedFd {
    if fd.as_raw_fd() > crate::sandbox::STATUS_FD {
        return fd;
    }
    let moved = unsafe {
        libc::fcntl(
            fd.as_raw_fd(),
            libc::F_DUPFD_CLOEXEC,
            crate::sandbox::STATUS_FD + 1,
        )
    };
    if moved < 0 {
        return fd;
    }
    unsafe { OwnedFd::from_raw_fd(moved) }
}

/// An inode that never had a pathname and can never be written again.
///
/// `None` when the kernel will not provide one — an old kernel, or a seccomp
/// policy that refuses the call — so the caller can fall back to a file it
/// unlinks. Sealing is part of the contract rather than a nicety: a descriptor
/// is reachable through `/proc/<pid>/fd` by anything running as the same user,
/// and a seal is the one restriction that survives being reopened there.
#[cfg(target_os = "linux")]
fn anonymous_image(name: &str, bytes: &[u8]) -> Option<OwnedFd> {
    let label = CString::new(format!("flutterdec-adapter:{name}")).ok()?;
    let flags = libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING;
    let mut raw = unsafe { libc::memfd_create(label.as_ptr(), flags | MFD_EXEC) };
    if raw < 0 {
        raw = unsafe { libc::memfd_create(label.as_ptr(), flags) };
    }
    if raw < 0 {
        return None;
    }
    // Owned from here on, so every early return below closes it.
    let mut file = unsafe { File::from_raw_fd(raw) };
    file.write_all(bytes).ok()?;
    let seals = libc::F_SEAL_SEAL | libc::F_SEAL_SHRINK | libc::F_SEAL_GROW | libc::F_SEAL_WRITE;
    if unsafe { libc::fcntl(raw, libc::F_ADD_SEALS, seals) } != 0 {
        return None;
    }
    Some(OwnedFd::from(file))
}

/// A file that exists only long enough to be opened.
///
/// It is created inside the invocation's own directory, which nobody but the
/// owner can traverse, and it is unlinked before the descriptor is returned. The
/// window in which the name exists is entirely inside this function: no caller
/// has published the workspace, reached a rendezvous, or created a process yet.
fn unlinked_image(name: &str, bytes: &[u8], scratch: &Path) -> Result<OwnedFd, HostError> {
    use std::fs;
    use std::os::unix::fs::OpenOptionsExt;

    let path = scratch.join(format!(".image-{name}"));
    let fail = |what: &str, err: std::io::Error| {
        HostError::Workspace(format!("{what} {}: {err}", path.display()))
    };
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o500)
        .open(&path)
        .map_err(|err| fail("materialize", err))?;
    file.write_all(bytes).map_err(|err| fail("write", err))?;
    drop(file);
    let opened = File::open(&path).map_err(|err| fail("open", err))?;
    fs::remove_file(&path).map_err(|err| fail("unlink", err))?;
    Ok(OwnedFd::from(opened))
}
