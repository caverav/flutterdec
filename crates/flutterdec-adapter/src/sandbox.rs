//! Containment controls for one adapter child process.
//!
//! Everything here exists to make one claim checkable: the host never says a
//! control is in force unless the control was established in the child that
//! actually ran. A limit is set by the child itself, between `fork` and `exec`,
//! where the only thing that knows whether `setrlimit` succeeded is the child.
//! So the child reports back: it writes one fixed-size record of per-control
//! result codes into a close-on-exec pipe, `exec` closes the pipe, and the
//! parent reads the record and turns it into a [`ContainmentReport`]. A control
//! whose code is not zero is reported `Unavailable` with the reason, never
//! silently as applied.
//!
//! The pre-exec code runs in a forked child of a possibly multi-threaded
//! process, so it does no allocation, takes no locks, and calls nothing but
//! syscalls.
//!
//! Platform differences are conditional and stated rather than smoothed over.
//! Linux can unshare a network namespace and can observe its own per-user
//! process count, so those controls are real there. Darwin can do neither, and
//! its kernel does not enforce `RLIMIT_AS`, so all three are reported
//! unavailable on Darwin rather than claimed. Nothing here compiles or claims
//! anything on Windows.

use serde::Serialize;
use std::fs;
use std::io::Read;
use std::os::unix::io::{FromRawFd, OwnedFd, RawFd};
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::time::Duration;

/// How the six named resource controls plus session, descriptor, and network
/// isolation are reported. One slot per control, in a fixed order, because the
/// child writes them positionally.
const SLOT_SESSION: usize = 0;
const SLOT_DESCRIPTOR_ISOLATION: usize = 1;
const SLOT_CPU: usize = 2;
const SLOT_FILE_SIZE: usize = 3;
const SLOT_ADDRESS_SPACE: usize = 4;
const SLOT_PROCESS_COUNT: usize = 5;
const SLOT_DESCRIPTORS: usize = 6;
const SLOT_NETWORK: usize = 7;
const SLOT_COUNT: usize = 8;

/// The control was established.
const CODE_APPLIED: i32 = 0;
/// The caller did not ask for this control.
const CODE_NOT_REQUESTED: i32 = -1;
/// This host has no mechanism for the control.
const CODE_UNSUPPORTED: i32 = -2;

/// Descriptor scan ceiling.
///
/// The child marks every descriptor from 4 upwards to this bound close-on-exec.
/// A soft `RLIMIT_NOFILE` can be a million on a systemd host, and a million
/// `fcntl` calls per adapter run buys nothing: descriptors above this are not
/// something a host process holds open by accident.
const MAX_DESCRIPTOR_SCAN: u32 = 65_536;

/// The descriptor the status pipe is moved to before the scan runs, so the scan
/// has one contiguous range to close and no exception to test for.
const STATUS_FD: RawFd = 3;

/// What one adapter invocation is allowed to consume.
///
/// These are deliberately concrete numbers rather than "unlimited unless
/// configured": an adapter is a third-party executable, and the default for a
/// third-party executable cannot be no bound at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Overall wall-clock deadline for the whole invocation.
    pub wall_clock: Duration,
    /// `RLIMIT_CPU` seconds. Distinct from `wall_clock`: a process asleep for an
    /// hour has burned no CPU, and a process spinning for an hour has burned no
    /// more wall clock than one that slept.
    pub cpu_seconds: u64,
    /// `RLIMIT_FSIZE`. Bounds any single file the adapter writes, including one
    /// it writes outside its workspace.
    pub max_file_bytes: u64,
    /// `RLIMIT_AS`. `None` leaves address space unbounded, which is reported as
    /// such.
    pub max_address_space_bytes: Option<u64>,
    /// How many more processes the adapter tree may create. Added to the current
    /// per-user process count, because `RLIMIT_NPROC` counts every process of
    /// the real user id and not just this tree.
    pub extra_processes: Option<u64>,
    /// `RLIMIT_NOFILE`.
    pub max_descriptors: u64,
    pub max_stdout_bytes: u64,
    pub max_stderr_bytes: u64,
    /// Cap on the model document the adapter writes.
    pub max_model_bytes: u64,
    /// Cap on the protocol result document.
    pub max_result_bytes: u64,
    /// Cap on one snapshot region handed to the adapter.
    pub max_region_bytes: u64,
    /// Whether to put the child in an empty network namespace.
    pub isolate_network: bool,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            wall_clock: Duration::from_secs(600),
            cpu_seconds: 600,
            max_file_bytes: 512 * 1024 * 1024,
            max_address_space_bytes: Some(8 * 1024 * 1024 * 1024),
            extra_processes: Some(64),
            max_descriptors: 512,
            max_stdout_bytes: 1024 * 1024,
            max_stderr_bytes: 1024 * 1024,
            max_model_bytes: 256 * 1024 * 1024,
            max_result_bytes: 1024 * 1024,
            max_region_bytes: 512 * 1024 * 1024,
            isolate_network: true,
        }
    }
}

/// Whether one control is in force, and the bound if it is.
///
/// There is no third variant on purpose. "Probably applied" is the state this
/// type exists to make unrepresentable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ControlState {
    Applied {
        /// The effective bound, when the control has one. A session or a network
        /// namespace is on or off and has no number.
        limit: Option<u64>,
    },
    Unavailable {
        reason: String,
    },
}

impl ControlState {
    pub fn is_applied(&self) -> bool {
        matches!(self, Self::Applied { .. })
    }

    fn from_code(code: i32, value: u64, control: &str) -> Self {
        match code {
            CODE_APPLIED => Self::Applied {
                limit: (value != u64::MAX).then_some(value),
            },
            CODE_NOT_REQUESTED => Self::Unavailable {
                reason: format!("{control} was not requested for this invocation"),
            },
            CODE_UNSUPPORTED => Self::Unavailable {
                reason: format!("{control} has no mechanism on {}", std::env::consts::OS),
            },
            errno => Self::Unavailable {
                reason: format!(
                    "{control} could not be established: {}",
                    std::io::Error::from_raw_os_error(errno)
                ),
            },
        }
    }

    fn unavailable(reason: impl Into<String>) -> Self {
        Self::Unavailable {
            reason: reason.into(),
        }
    }
}

/// The containment state of one completed or terminated invocation.
///
/// Serialized into the decompile report so the accuracy of every claim is
/// inspectable from outside the process that made it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContainmentReport {
    /// The host-side wall-clock deadline. Enforced by the host, so it is applied
    /// whenever an invocation happened at all.
    pub wall_clock_deadline: ControlState,
    /// A new session and process group, which is what makes a tree kill
    /// possible.
    pub process_group: ControlState,
    /// Inherited descriptors above the three standard ones closed before `exec`.
    pub descriptor_isolation: ControlState,
    pub cpu_seconds: ControlState,
    pub file_size: ControlState,
    pub address_space: ControlState,
    pub process_count: ControlState,
    pub descriptors: ControlState,
    pub network: ControlState,
    /// Host-side output caps. Applied by the reader, so always in force.
    pub stdout_bytes: ControlState,
    pub stderr_bytes: ControlState,
    pub model_bytes: ControlState,
    /// Whether the host had to terminate a tree that was still running, rather
    /// than reaping a child that finished on its own.
    ///
    /// The group is signalled on every exit path, including the clean one,
    /// because a backend that forked and abandoned a grandchild leaves one
    /// behind on the clean path too. This field is about the other case: the
    /// deadline or an output cap ran out and the host ended the run.
    pub process_tree_terminated: bool,
}

impl ContainmentReport {
    /// Every control this report names, for callers that iterate rather than
    /// hard-code the list.
    pub fn controls(&self) -> Vec<(&'static str, &ControlState)> {
        vec![
            ("wall_clock_deadline", &self.wall_clock_deadline),
            ("process_group", &self.process_group),
            ("descriptor_isolation", &self.descriptor_isolation),
            ("cpu_seconds", &self.cpu_seconds),
            ("file_size", &self.file_size),
            ("address_space", &self.address_space),
            ("process_count", &self.process_count),
            ("descriptors", &self.descriptors),
            ("network", &self.network),
            ("stdout_bytes", &self.stdout_bytes),
            ("stderr_bytes", &self.stderr_bytes),
            ("model_bytes", &self.model_bytes),
        ]
    }
}

/// The integer plan the child carries across `fork`.
///
/// `Copy` and nothing but integers, because the pre-exec closure has to be
/// `'static` and must not allocate.
#[derive(Debug, Clone, Copy)]
struct ChildPlan {
    status_fd: RawFd,
    descriptor_scan_ceiling: u32,
    cpu_seconds: u64,
    max_file_bytes: u64,
    /// `u64::MAX` means "not requested"; `RLIM_INFINITY` is a legitimate value
    /// so it cannot double as the sentinel.
    max_address_space_bytes: u64,
    /// Budget when the child shares the host's user namespace: the host's
    /// current task count plus the allowance.
    process_count: u64,
    /// Budget when the child gets its own user namespace: the allowance plus the
    /// one task that is the child itself.
    process_count_isolated: u64,
    max_descriptors: u64,
    isolate_network: bool,
}

const NOT_REQUESTED: u64 = u64::MAX;

/// The record the child writes and the parent reads.
///
/// Fixed size and well under `PIPE_BUF`, so one `write` is atomic and a short
/// read means the child died before it finished rather than that the record
/// interleaved with something else.
const STATUS_BYTES: usize = SLOT_COUNT * 4 + SLOT_COUNT * 8;

/// One `write` of this size is atomic, so a short read in the parent means the
/// child died mid-record rather than that two writers interleaved.
const _: () = assert!(STATUS_BYTES < 512);

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(-3)
}

/// Lower one resource limit, clamping to the inherited hard limit.
///
/// Clamping rather than failing matters: a host whose hard limit is already
/// below what the caller asked for still gets a limit, and the value reported is
/// the one that is actually in force.
///
/// # Safety
/// Called between `fork` and `exec`. Only makes syscalls.
#[allow(clippy::unnecessary_cast)]
unsafe fn lower_limit(resource: RlimitResource, requested: u64, applied: &mut u64) -> i32 {
    let mut current = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    if libc::getrlimit(resource, &mut current) != 0 {
        return errno();
    }
    let want = requested as libc::rlim_t;
    let effective = if current.rlim_max != libc::RLIM_INFINITY && want > current.rlim_max {
        current.rlim_max
    } else {
        want
    };
    let next = libc::rlimit {
        rlim_cur: effective,
        rlim_max: current.rlim_max,
    };
    if libc::setrlimit(resource, &next) != 0 {
        return errno();
    }
    *applied = effective as u64;
    CODE_APPLIED
}

/// The integer type `getrlimit` names a resource with, which glibc and Darwin
/// spell differently.
#[cfg(target_env = "gnu")]
type RlimitResource = libc::__rlimit_resource_t;
#[cfg(not(target_env = "gnu"))]
type RlimitResource = libc::c_int;

/// Drop the child into an empty network namespace.
///
/// Two attempts, because the two ways to get one need different authority:
/// `CLONE_NEWNET` alone needs `CAP_SYS_ADMIN`, and pairing it with
/// `CLONE_NEWUSER` gets that capability inside a fresh user namespace on hosts
/// that allow unprivileged user namespaces. Where neither works the errno from
/// the first attempt is reported and nothing is claimed.
///
/// The second return value says whether a *user* namespace was entered, which
/// the caller needs: the kernel counts `RLIMIT_NPROC` per user namespace and
/// uid, so entering one resets that count and a budget computed against the
/// host's count would no longer bound anything.
///
/// # Safety
/// Called between `fork` and `exec`. Only makes syscalls.
#[cfg(target_os = "linux")]
unsafe fn isolate_network() -> (i32, bool) {
    if libc::unshare(libc::CLONE_NEWNET) == 0 {
        return (CODE_APPLIED, false);
    }
    let first = errno();
    if libc::unshare(libc::CLONE_NEWUSER | libc::CLONE_NEWNET) == 0 {
        return (CODE_APPLIED, true);
    }
    (first, false)
}

#[cfg(not(target_os = "linux"))]
unsafe fn isolate_network() -> (i32, bool) {
    (CODE_UNSUPPORTED, false)
}

/// Apply the plan and report each outcome. Runs in the forked child.
///
/// # Safety
/// Called between `fork` and `exec` from `pre_exec`. Allocates nothing, takes no
/// locks, and calls only async-signal-safe syscalls.
unsafe fn apply_plan(plan: &ChildPlan) {
    let mut codes = [CODE_NOT_REQUESTED; SLOT_COUNT];
    let mut values = [NOT_REQUESTED; SLOT_COUNT];

    // A new session, so the child is a process-group leader and the whole tree
    // can be signalled with one negative pid. Done first: everything after this
    // point is a limit on a process the host can already terminate.
    codes[SLOT_SESSION] = if libc::setsid() < 0 {
        errno()
    } else {
        CODE_APPLIED
    };

    // Move the status pipe somewhere known so the scan below is one range with
    // no exception in it. `dup2` clears close-on-exec, so it is set again: the
    // parent detects `exec` by this descriptor closing.
    if plan.status_fd != STATUS_FD && libc::dup2(plan.status_fd, STATUS_FD) < 0 {
        // Nothing can be reported without the pipe. Exec still proceeds; the
        // parent treats a missing record as a failed invocation.
        return;
    }
    libc::fcntl(STATUS_FD, libc::F_SETFD, libc::FD_CLOEXEC);

    // Close-on-exec rather than close. The two are equivalent for the adapter,
    // which only exists after `exec`, but `close` would also destroy the pipe
    // the standard library uses to report an `exec` failure back to the host,
    // turning "artifact vanished" into an unexplained abort.
    let mut scan_failures = 0i32;
    for fd in (STATUS_FD + 1)..=(plan.descriptor_scan_ceiling as RawFd) {
        if libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) != 0 && errno() != libc::EBADF {
            scan_failures = errno();
        }
    }
    codes[SLOT_DESCRIPTOR_ISOLATION] = if scan_failures == 0 {
        values[SLOT_DESCRIPTOR_ISOLATION] = u64::from(plan.descriptor_scan_ceiling);
        CODE_APPLIED
    } else {
        scan_failures
    };

    codes[SLOT_CPU] = lower_limit(libc::RLIMIT_CPU, plan.cpu_seconds, &mut values[SLOT_CPU]);
    codes[SLOT_FILE_SIZE] = lower_limit(
        libc::RLIMIT_FSIZE,
        plan.max_file_bytes,
        &mut values[SLOT_FILE_SIZE],
    );

    // Darwin accepts `RLIMIT_AS` and does not enforce it, so setting it there
    // would produce a true syscall and a false claim.
    #[cfg(target_os = "linux")]
    if plan.max_address_space_bytes != NOT_REQUESTED {
        codes[SLOT_ADDRESS_SPACE] = lower_limit(
            libc::RLIMIT_AS,
            plan.max_address_space_bytes,
            &mut values[SLOT_ADDRESS_SPACE],
        );
    }
    #[cfg(not(target_os = "linux"))]
    {
        // Read on this platform too, so the field cannot rot into one that is
        // only ever written.
        let _ = plan.max_address_space_bytes;
        codes[SLOT_ADDRESS_SPACE] = CODE_UNSUPPORTED;
    }

    codes[SLOT_DESCRIPTORS] = lower_limit(
        libc::RLIMIT_NOFILE,
        plan.max_descriptors,
        &mut values[SLOT_DESCRIPTORS],
    );

    // Network isolation before the process budget, because which namespace this
    // task ends up in decides what the budget has to be.
    let mut own_user_namespace = false;
    if plan.isolate_network {
        let (code, entered) = isolate_network();
        codes[SLOT_NETWORK] = code;
        own_user_namespace = entered;
    }

    // In a fresh user namespace the per-uid task count starts at this task
    // alone, so the budget is the tree's own allowance. Sharing the host's
    // namespace means sharing its count, so the budget is that count plus the
    // allowance.
    let process_count = if own_user_namespace {
        plan.process_count_isolated
    } else {
        plan.process_count
    };
    if process_count != NOT_REQUESTED {
        codes[SLOT_PROCESS_COUNT] = lower_limit(
            libc::RLIMIT_NPROC,
            process_count,
            &mut values[SLOT_PROCESS_COUNT],
        );
    }

    let mut record = [0u8; STATUS_BYTES];
    for (slot, code) in codes.iter().enumerate() {
        record[slot * 4..slot * 4 + 4].copy_from_slice(&code.to_ne_bytes());
    }
    for (slot, value) in values.iter().enumerate() {
        let base = SLOT_COUNT * 4 + slot * 8;
        record[base..base + 8].copy_from_slice(&value.to_ne_bytes());
    }
    libc::write(
        STATUS_FD,
        record.as_ptr() as *const libc::c_void,
        STATUS_BYTES,
    );
}

/// How many tasks the real user id already owns.
///
/// `RLIMIT_NPROC` is a per-user ceiling, not a per-tree one, so a fixed number
/// would either be unreachable on an idle host or already breached on a busy
/// one. Counting first turns it into a real budget for this tree.
///
/// Tasks, not processes. The kernel charges the limit per task, so a host
/// process with sixteen threads costs sixteen. Counting `/proc/<pid>` entries
/// alone undercounts a threaded host by an order of magnitude and produces a
/// budget the very next `fork` breaches.
#[cfg(target_os = "linux")]
fn current_process_count() -> Option<u64> {
    use std::os::unix::fs::MetadataExt;

    let uid = unsafe { libc::getuid() };
    let mut count = 0u64;
    for entry in fs::read_dir("/proc").ok()?.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.is_empty() || !name.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.uid() != uid {
            continue;
        }
        // A process that exits between the two reads still counted for one task
        // while it existed, so the fallback is the safe direction.
        count += match fs::read_dir(entry.path().join("task")) {
            Ok(tasks) => tasks.count() as u64,
            Err(_) => 1,
        };
    }
    Some(count)
}

#[cfg(not(target_os = "linux"))]
fn current_process_count() -> Option<u64> {
    None
}

fn descriptor_scan_ceiling() -> u32 {
    let mut current = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    let soft: u64 = if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut current) } == 0 {
        current.rlim_cur
    } else {
        u64::from(MAX_DESCRIPTOR_SCAN)
    };
    u32::try_from(soft.min(u64::from(MAX_DESCRIPTOR_SCAN))).unwrap_or(MAX_DESCRIPTOR_SCAN)
}

/// A pipe whose two ends both close on `exec`.
///
/// `pipe2` does it in one syscall where it exists; Darwin has no `pipe2`, so the
/// flag is set afterwards. The window between the two calls is not a leak risk
/// here: the descriptors are used by this process and by the child it is about
/// to fork, and nothing else runs in between.
fn close_on_exec_pipe() -> std::io::Result<[libc::c_int; 2]> {
    let mut fds = [0 as libc::c_int; 2];
    #[cfg(target_os = "linux")]
    let created = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) };
    #[cfg(not(target_os = "linux"))]
    let created = unsafe { libc::pipe(fds.as_mut_ptr()) };
    if created != 0 {
        return Err(std::io::Error::last_os_error());
    }
    #[cfg(not(target_os = "linux"))]
    for fd in fds {
        if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } != 0 {
            let err = std::io::Error::last_os_error();
            unsafe {
                libc::close(fds[0]);
                libc::close(fds[1]);
            }
            return Err(err);
        }
    }
    Ok(fds)
}

/// One status pipe, and the plan the child will apply through it.
pub(crate) struct Containment {
    read_end: OwnedFd,
    /// Held only until the child is forked. The parent must drop it or the read
    /// below never reaches end of file.
    write_end: Option<OwnedFd>,
    plan: ChildPlan,
    limits: Limits,
    /// Reasons the parent already knows, before the child says anything.
    process_count_note: Option<String>,
}

impl Containment {
    /// Build the pipe and the plan. Fails only if a pipe cannot be created.
    pub(crate) fn prepare(limits: &Limits) -> std::io::Result<Self> {
        // Both ends close on exec: the write end so the parent sees end of file
        // the moment the child execs, the read end so the child never holds it.
        let fds = close_on_exec_pipe()?;
        let read_end = unsafe { OwnedFd::from_raw_fd(fds[0]) };
        let write_end = unsafe { OwnedFd::from_raw_fd(fds[1]) };

        let (process_count, process_count_isolated, process_count_note) = match limits
            .extra_processes
        {
            None => (NOT_REQUESTED, NOT_REQUESTED, None),
            Some(extra) => match current_process_count() {
                Some(current) => (
                    current.saturating_add(extra),
                    extra.saturating_add(1),
                    None,
                ),
                None => (
                    NOT_REQUESTED,
                    NOT_REQUESTED,
                    Some(format!(
                        "RLIMIT_NPROC counts every task of the real user id and {} cannot observe that count, so no process budget was set",
                        std::env::consts::OS
                    )),
                ),
            },
        };

        Ok(Self {
            plan: ChildPlan {
                status_fd: std::os::unix::io::AsRawFd::as_raw_fd(&write_end),
                descriptor_scan_ceiling: descriptor_scan_ceiling(),
                cpu_seconds: limits.cpu_seconds,
                max_file_bytes: limits.max_file_bytes,
                max_address_space_bytes: limits.max_address_space_bytes.unwrap_or(NOT_REQUESTED),
                process_count,
                process_count_isolated,
                max_descriptors: limits.max_descriptors,
                isolate_network: limits.isolate_network,
            },
            read_end,
            write_end: Some(write_end),
            limits: *limits,
            process_count_note,
        })
    }

    /// Register the pre-exec hook that applies the plan.
    ///
    /// Registering any pre-exec closure also takes `Command` off its
    /// `posix_spawn` fast path, which is required: `posix_spawn` cannot run
    /// arbitrary code in the child.
    pub(crate) fn install(&self, command: &mut Command) {
        let plan = self.plan;
        unsafe {
            command.pre_exec(move || {
                apply_plan(&plan);
                Ok(())
            });
        }
    }

    /// Read the child's record after the fork.
    ///
    /// Must be called once the child exists. Blocks until `exec` closes the
    /// write end, which is bounded: `exec` either happens or the child exits,
    /// and both close the descriptor.
    pub(crate) fn collect(mut self, terminated: bool) -> ContainmentReport {
        // The parent's own copy of the write end would keep the pipe open
        // forever.
        drop(self.write_end.take());

        let mut bytes = Vec::with_capacity(STATUS_BYTES);
        let mut file = fs::File::from(self.read_end);
        let record = match file.read_to_end(&mut bytes) {
            Ok(_) if bytes.len() == STATUS_BYTES => Some(bytes),
            _ => None,
        };

        let (codes, values) = match &record {
            Some(bytes) => {
                let mut codes = [CODE_UNSUPPORTED; SLOT_COUNT];
                let mut values = [NOT_REQUESTED; SLOT_COUNT];
                for slot in 0..SLOT_COUNT {
                    codes[slot] = i32::from_ne_bytes(
                        bytes[slot * 4..slot * 4 + 4]
                            .try_into()
                            .expect("four bytes"),
                    );
                    let base = SLOT_COUNT * 4 + slot * 8;
                    values[slot] =
                        u64::from_ne_bytes(bytes[base..base + 8].try_into().expect("eight bytes"));
                }
                (Some(codes), Some(values))
            }
            None => (None, None),
        };

        let state = |slot: usize, control: &str| match (codes, values) {
            (Some(codes), Some(values)) => {
                ControlState::from_code(codes[slot], values[slot], control)
            }
            _ => ControlState::unavailable(format!(
                "{control} cannot be reported: the child wrote no containment record"
            )),
        };

        let process_count = match &self.process_count_note {
            Some(note) => ControlState::unavailable(note.clone()),
            None => state(SLOT_PROCESS_COUNT, "a per-user process budget"),
        };

        ContainmentReport {
            wall_clock_deadline: ControlState::Applied {
                limit: Some(self.limits.wall_clock.as_millis().min(u128::from(u64::MAX)) as u64),
            },
            process_group: state(SLOT_SESSION, "a private session and process group"),
            descriptor_isolation: state(SLOT_DESCRIPTOR_ISOLATION, "descriptor isolation"),
            cpu_seconds: state(SLOT_CPU, "a CPU time limit"),
            file_size: state(SLOT_FILE_SIZE, "a file size limit"),
            address_space: state(SLOT_ADDRESS_SPACE, "an address space limit"),
            process_count,
            descriptors: state(SLOT_DESCRIPTORS, "a descriptor limit"),
            network: state(SLOT_NETWORK, "network isolation"),
            stdout_bytes: ControlState::Applied {
                limit: Some(self.limits.max_stdout_bytes),
            },
            stderr_bytes: ControlState::Applied {
                limit: Some(self.limits.max_stderr_bytes),
            },
            model_bytes: ControlState::Applied {
                limit: Some(self.limits.max_model_bytes),
            },
            process_tree_terminated: terminated,
        }
    }
}

/// Signal a whole process group and swallow the "already gone" case.
///
/// The negative pid is the point: the child made itself a group leader, so this
/// reaches grandchildren the child forked and then abandoned.
pub(crate) fn kill_tree(pid: u32) -> bool {
    let group = match i32::try_from(pid) {
        Ok(pid) => -pid,
        Err(_) => return false,
    };
    unsafe { libc::kill(group, libc::SIGKILL) == 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_defaulted_limit_set_is_bounded_everywhere() {
        let limits = Limits::default();
        assert!(limits.wall_clock > Duration::ZERO);
        for bound in [
            limits.cpu_seconds,
            limits.max_file_bytes,
            limits.max_descriptors,
            limits.max_stdout_bytes,
            limits.max_stderr_bytes,
            limits.max_model_bytes,
            limits.max_result_bytes,
            limits.max_region_bytes,
        ] {
            assert!(bound > 0, "a default bound of zero is not a bound");
        }
    }

    /// A missing record must degrade to "unavailable" for every child-applied
    /// control rather than to a claim.
    #[test]
    fn a_child_that_reports_nothing_yields_no_applied_claim() {
        let limits = Limits::default();
        let containment = Containment::prepare(&limits).expect("pipe");
        // Nothing was forked, so dropping the write end gives an empty read.
        let report = containment.collect(false);
        for (name, state) in report.controls() {
            match name {
                // Host-side controls need no child cooperation.
                "wall_clock_deadline" | "stdout_bytes" | "stderr_bytes" | "model_bytes" => {
                    assert!(
                        state.is_applied(),
                        "{name} is host-side and must be applied"
                    )
                }
                _ => assert!(
                    !state.is_applied(),
                    "{name} claims to be applied with no record from the child: {state:?}"
                ),
            }
        }
    }

    /// The record has to be small enough for one `write` to be atomic, or a
    /// short read would be ambiguous between "child died" and "record split".
    #[test]
    fn the_status_record_fits_one_atomic_pipe_write() {
        assert_eq!(STATUS_BYTES, 96);
    }
}
