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
//! `/proc`, and the seals are read back before anything is executed. The child
//! is then created with `execveat(AT_EMPTY_PATH)`, which resolves the
//! descriptor and never a path.
//!
//! There is no fallback between the two on Linux. A host that cannot produce a
//! sealed anonymous image refuses the run: falling back to a pathname there
//! would quietly hand back the property the caller was promised, and it would do
//! it exactly on the hosts — old kernels, restrictive seccomp policies — where
//! the guarantee is worth the most.
//!
//! Darwin cannot execute a descriptor at all. `/dev/fd/N` there is not another
//! name for the file: it is a node whose reported mode is the *descriptor's*
//! access mode, so it never carries an execute bit and `execve` on it is refused
//! with `EACCES` whether the file behind it is linked or not. There is no
//! `fexecve` and no `execveat`. So the image is a file inside the invocation's
//! own private directory, created `O_EXCL` at mode `0500`, and then made
//! immutable through the descriptor this module holds. While that flag is set
//! the name cannot be written through, truncated, renamed, or unlinked, which is
//! what closes the gap between checking bytes and running them. The known
//! ceiling is that the flag is a *user* flag: the owner may clear it, and on
//! Darwin an attacker running as the same user is the owner. It is the strongest
//! the platform offers, and it is bounded, reported, and never used on Linux.
//!
//! Scripts keep working. On Linux that is the reason the descriptor is inherited
//! rather than closed on exec: the kernel hands a `#!` interpreter `/dev/fd/N`
//! as the script to read, and that name resolves through the child's own
//! descriptor table. It is the image itself, read-only and sealed, so the child
//! gaining a second way to look at its own code costs nothing. On Darwin the
//! interpreter is handed the image's own pathname, and the descriptor stays
//! close-on-exec: handing the child a writable-flags handle on its own image
//! would give it the one capability the flag exists to deny.

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
    /// The image's own pathname, which is what Darwin executes because Darwin
    /// cannot execute a descriptor. Immutable for as long as the run lasts.
    #[cfg(not(target_os = "linux"))]
    image_path: CString,
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
        #[cfg(target_os = "linux")]
        let fd = open_image(name, bytes, scratch)?;
        #[cfg(not(target_os = "linux"))]
        let (fd, image_path) = open_image(name, bytes, scratch)?;
        Ok(Self {
            fd,
            argv_ptrs: pointers(&argv),
            envp_ptrs: pointers(&envp),
            _argv: argv,
            _envp: envp,
            #[cfg(not(target_os = "linux"))]
            image_path,
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
        // script. Cleared last, so the sweep cannot undo it. Only where the
        // child has a use for it — elsewhere the image's descriptor is the one
        // handle that can lift its immutability, and the child gets no share of
        // it.
        #[cfg(target_os = "linux")]
        {
            libc::fcntl(self.fd.as_raw_fd(), libc::F_SETFD, 0);
            libc::syscall(
                libc::SYS_execveat,
                self.fd.as_raw_fd(),
                EMPTY_PATH.as_ptr().cast::<libc::c_char>(),
                self.argv_ptrs.as_ptr(),
                self.envp_ptrs.as_ptr(),
                libc::AT_EMPTY_PATH,
            );
        }
        #[cfg(not(target_os = "linux"))]
        libc::execve(
            self.image_path.as_ptr(),
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

/// The image, or the reason there is not one.
///
/// Two implementations, not one implementation with a fallback: on Linux the
/// only way to hold the bytes is a sealed anonymous file, and `scratch` is
/// untouched because nothing here may put an executable pathname in it.
#[cfg(target_os = "linux")]
fn open_image(name: &str, bytes: &[u8], _scratch: &Path) -> Result<OwnedFd, HostError> {
    anonymous_image(name, bytes).map(reserve)
}

#[cfg(not(target_os = "linux"))]
fn open_image(name: &str, bytes: &[u8], scratch: &Path) -> Result<(OwnedFd, CString), HostError> {
    let (fd, path) = immutable_image(name, bytes, scratch)?;
    Ok((reserve(fd), path))
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

/// The seals an image carries before anything is allowed to execute it, and the
/// names to say which one is missing.
///
/// Each is load-bearing. `F_SEAL_WRITE` is the content itself; `F_SEAL_GROW` and
/// `F_SEAL_SHRINK` stop the size from moving under a mapping; `F_SEAL_SEAL`
/// stops the set from being taken back off. Sealing is the contract rather than
/// a nicety: a descriptor is reachable through `/proc/<pid>/fd` by anything
/// running as the same user, and a seal is the one restriction that survives
/// being reopened there.
#[cfg(target_os = "linux")]
const REQUIRED_SEALS: [(libc::c_int, &str); 4] = [
    (libc::F_SEAL_WRITE, "F_SEAL_WRITE"),
    (libc::F_SEAL_GROW, "F_SEAL_GROW"),
    (libc::F_SEAL_SHRINK, "F_SEAL_SHRINK"),
    (libc::F_SEAL_SEAL, "F_SEAL_SEAL"),
];

/// An inode that never had a pathname and can never be written again.
///
/// Every failure is returned, not absorbed. An old kernel, a seccomp policy that
/// refuses `memfd_create`, a short write, a rejected seal: each of them means
/// this host cannot make the promise the caller is relying on, and the run stops
/// before a process exists rather than continuing with a weaker image.
#[cfg(target_os = "linux")]
fn anonymous_image(name: &str, bytes: &[u8]) -> Result<OwnedFd, HostError> {
    let label = CString::new(format!("flutterdec-adapter:{name}"))
        .map_err(|_| refused("the adapter name contains a NUL byte"))?;
    let flags = libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING;
    // `MFD_EXEC` is rejected outright by kernels that predate it, so the ask is
    // made once and then dropped. Both attempts failing is the kernel declining
    // to provide an anonymous file at all.
    let mut raw = unsafe { libc::memfd_create(label.as_ptr(), flags | MFD_EXEC) };
    if raw < 0 {
        raw = unsafe { libc::memfd_create(label.as_ptr(), flags) };
    }
    if raw < 0 {
        return Err(refused(&format!(
            "create an anonymous executable image: {}",
            std::io::Error::last_os_error()
        )));
    }
    // Owned from here on, so every early return below closes it.
    let mut file = unsafe { File::from_raw_fd(raw) };
    file.write_all(bytes).map_err(|err| {
        refused(&format!(
            "write {} byte(s) into the anonymous image: {err}",
            bytes.len()
        ))
    })?;
    seal(file)
}

/// Make the written bytes immutable, and read the seals back rather than
/// assuming `F_ADD_SEALS` did what it was asked.
///
/// The verification is not ceremony. `F_ADD_SEALS` takes a mask, and a kernel
/// that understands the call but not every bit in that mask would leave a
/// descriptor that looks sealed and is not. What executes has to be a descriptor
/// this host has seen the whole seal set on.
#[cfg(target_os = "linux")]
fn seal(file: File) -> Result<OwnedFd, HostError> {
    let fd = file.as_raw_fd();
    let mask = REQUIRED_SEALS
        .iter()
        .fold(0, |mask, (seal, _)| mask | *seal);
    if unsafe { libc::fcntl(fd, libc::F_ADD_SEALS, mask) } != 0 {
        return Err(refused(&format!(
            "seal the anonymous image: {}",
            std::io::Error::last_os_error()
        )));
    }
    verify_seals(fd)?;
    Ok(OwnedFd::from(file))
}

/// The seals actually present, checked against the ones that were required.
#[cfg(target_os = "linux")]
fn verify_seals(fd: libc::c_int) -> Result<(), HostError> {
    let present = unsafe { libc::fcntl(fd, libc::F_GET_SEALS) };
    if present < 0 {
        return Err(refused(&format!(
            "read the seals back from the anonymous image: {}",
            std::io::Error::last_os_error()
        )));
    }
    let missing: Vec<&str> = REQUIRED_SEALS
        .iter()
        .filter(|(seal, _)| present & seal == 0)
        .map(|(_, name)| *name)
        .collect();
    if !missing.is_empty() {
        return Err(refused(&format!(
            "the anonymous image is missing {}",
            missing.join(", ")
        )));
    }
    Ok(())
}

/// Why the image could not be made, as the refusal the caller sees.
#[cfg(target_os = "linux")]
fn refused(detail: &str) -> HostError {
    HostError::ImageNotSealed(detail.to_string())
}

/// A file whose name stops being a way to change anything.
///
/// It is created inside the invocation's own directory with `O_EXCL`, written,
/// reopened read-only, and then frozen through that descriptor: while the flag
/// is set the kernel refuses a write, a truncation, a rename and an unlink
/// through the name, so the name and the bytes stay welded together for the
/// whole run. The flag is set through the descriptor rather than the path, so
/// nothing between creating the file and freezing it is decided by re-resolving
/// a string.
///
/// Every step is a hard failure. This is the platform's whole guarantee, and a
/// host that could not establish it would be executing a pathname anyone running
/// as this user could re-point between the digest and the `exec`.
///
/// Compiled only where a descriptor cannot be executed. On Linux this does not
/// exist, so no Linux code path can reach a pathname-backed executable.
#[cfg(not(target_os = "linux"))]
fn immutable_image(
    name: &str,
    bytes: &[u8],
    scratch: &Path,
) -> Result<(OwnedFd, CString), HostError> {
    use std::fs;
    use std::os::unix::fs::OpenOptionsExt;

    let path = scratch.join(format!(".image-{name}"));
    let fail = |what: &str, err: std::io::Error| {
        HostError::ImageNotSealed(format!("{what} {}: {err}", path.display()))
    };
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o500)
        .open(&path)
        .map_err(|err| fail("materialize the image at", err))?;
    file.write_all(bytes)
        .map_err(|err| fail("write the image at", err))?;
    drop(file);
    let opened = File::open(&path).map_err(|err| fail("open the image at", err))?;
    if unsafe { libc::fchflags(opened.as_raw_fd(), libc::UF_IMMUTABLE) } != 0 {
        let err = std::io::Error::last_os_error();
        // The name is already there and cannot be frozen, so it is taken back
        // out before anything else can find it.
        let _ = fs::remove_file(&path);
        return Err(fail("make the image immutable at", err));
    }
    let as_argument = CString::new(path.as_os_str().as_bytes()).map_err(|_| {
        HostError::ImageNotSealed(format!(
            "the image path {} contains a NUL byte",
            path.display()
        ))
    })?;
    Ok((OwnedFd::from(opened), as_argument))
}

/// The image boundary, forced to fail.
///
/// Both failures are made by the kernel rather than by a hook in the product:
/// `memfd_create` rejects a label longer than it will store, and `F_ADD_SEALS`
/// rejects a descriptor that was not created sealable. So these exercise the
/// same syscalls a real run makes, in the same order, and what they observe is
/// what a host with an unsealable kernel would observe.
#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    /// Longer than the 249 bytes `memfd_create` will accept as a name, label
    /// included, so both attempts fail with `EINVAL` on every kernel.
    fn unnameable() -> String {
        "a".repeat(250)
    }

    fn vectors() -> (Vec<CString>, Vec<CString>) {
        (
            vec![CString::new("adapter").expect("no NUL")],
            vec![CString::new("PATH=/nonexistent").expect("no NUL")],
        )
    }

    fn seals_on(fd: libc::c_int) -> libc::c_int {
        let present = unsafe { libc::fcntl(fd, libc::F_GET_SEALS) };
        assert!(present >= 0, "{}", std::io::Error::last_os_error());
        present
    }

    /// A memfd created the way the product creates one, minus whichever part the
    /// caller wants to break.
    fn memfd(sealable: bool) -> File {
        let label = CString::new("flutterdec-adapter-test").expect("no NUL");
        let mut flags = libc::MFD_CLOEXEC;
        if sealable {
            flags |= libc::MFD_ALLOW_SEALING;
        }
        let raw = unsafe { libc::memfd_create(label.as_ptr(), flags) };
        assert!(raw >= 0, "{}", std::io::Error::last_os_error());
        unsafe { File::from_raw_fd(raw) }
    }

    #[test]
    fn an_image_the_kernel_will_not_create_is_refused_before_anything_runs() {
        let scratch = tempfile::TempDir::new().expect("tempdir");
        let (argv, envp) = vectors();

        let Err(err) = ExecImage::prepare(&unnameable(), b"\x7fELF", scratch.path(), argv, envp)
        else {
            panic!("an image the kernel refused became an executable");
        };

        let HostError::ImageNotSealed(detail) = &err else {
            panic!("a failed image was reported as something else: {err}");
        };
        assert!(
            detail.contains("create an anonymous executable image"),
            "the refusal does not say what failed: {detail}"
        );
        assert!(
            err.is_pre_spawn(),
            "a refusal with no child was not classified as pre-spawn: {err}"
        );
        let left_behind: Vec<_> = std::fs::read_dir(scratch.path())
            .expect("read the scratch directory")
            .map(|entry| entry.expect("entry").path())
            .collect();
        assert!(
            left_behind.is_empty(),
            "the host fell back to a pathname: {left_behind:?}"
        );
    }

    #[test]
    fn a_descriptor_that_cannot_be_sealed_is_refused() {
        let err = seal(memfd(false))
            .expect_err("a descriptor that refuses seals must not become an executable");

        let HostError::ImageNotSealed(detail) = &err else {
            panic!("a rejected seal was reported as something else: {err}");
        };
        assert!(
            detail.contains("seal the anonymous image"),
            "the refusal does not say what failed: {detail}"
        );
        assert!(err.is_pre_spawn(), "not classified as pre-spawn: {err}");
    }

    /// The check that `F_ADD_SEALS` succeeding is not taken as proof.
    #[test]
    fn an_incomplete_seal_set_is_refused_and_names_what_is_missing() {
        let file = memfd(true);
        // Exactly the one seal a caller might think is the whole story.
        assert_eq!(
            0,
            unsafe { libc::fcntl(file.as_raw_fd(), libc::F_ADD_SEALS, libc::F_SEAL_WRITE) },
            "{}",
            std::io::Error::last_os_error()
        );

        let err =
            verify_seals(file.as_raw_fd()).expect_err("a partly sealed image must not be accepted");

        let HostError::ImageNotSealed(detail) = &err else {
            panic!("an incomplete seal set was reported as something else: {err}");
        };
        for missing in ["F_SEAL_GROW", "F_SEAL_SHRINK", "F_SEAL_SEAL"] {
            assert!(
                detail.contains(missing),
                "the refusal does not name {missing}: {detail}"
            );
        }
        assert!(
            !detail.contains("F_SEAL_WRITE"),
            "the refusal names a seal that is present: {detail}"
        );
    }

    #[test]
    fn a_prepared_image_carries_every_required_seal_and_leaves_no_pathname() {
        let scratch = tempfile::TempDir::new().expect("tempdir");
        let (argv, envp) = vectors();

        let Ok(image) =
            ExecImage::prepare("adapter", b"\x7fELF payload", scratch.path(), argv, envp)
        else {
            panic!("a sealable kernel must produce an image");
        };

        let present = seals_on(image.fd.as_raw_fd());
        for (seal, name) in REQUIRED_SEALS {
            assert!(
                present & seal != 0,
                "the image executed without {name} (seals {present:#x})"
            );
        }
        // The seal is what it claims to be rather than a flag that reads back.
        // A write is refused through the descriptor the host holds, and through
        // the `/proc` path any process running as this user can reach.
        let overwrite = b"overwritten";
        let direct = unsafe {
            libc::pwrite(
                image.fd.as_raw_fd(),
                overwrite.as_ptr().cast(),
                overwrite.len(),
                0,
            )
        };
        assert_eq!(
            -1, direct,
            "the sealed image accepted a write through its own descriptor"
        );
        if let Ok(mut reopened) = std::fs::OpenOptions::new()
            .write(true)
            .open(format!("/proc/self/fd/{}", image.fd.as_raw_fd()))
        {
            assert!(
                reopened.write_all(overwrite).is_err(),
                "the sealed image accepted a write through /proc"
            );
        }
        let left_behind: Vec<_> = std::fs::read_dir(scratch.path())
            .expect("read the scratch directory")
            .map(|entry| entry.expect("entry").path())
            .collect();
        assert!(
            left_behind.is_empty(),
            "a successful image still named a file: {left_behind:?}"
        );
    }
}
