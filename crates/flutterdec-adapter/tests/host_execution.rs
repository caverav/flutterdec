//! Hostile adapters, run for real.
//!
//! Every adapter in this file is a real executable that does something a host
//! must survive: sleep past its deadline, fork and abandon a child, flood a
//! pipe, crash, lie about what it produced, or try to consume more of the
//! machine than it was given. Nothing here is mocked, because the failure modes
//! under test are properties of processes and pipes rather than of Rust types.
//!
//! Each adapter can also leave evidence outside its own workspace. The one path
//! it knows about that the host did not create is `--input-path`, so probes are
//! written beside it; that is how a test can inspect the cwd, environment and
//! descriptors of a child that is long gone, and how "the workspace was removed"
//! becomes checkable rather than assumed.

mod support;

use flutterdec_adapter::model::{
    InputRegion, InputRegionName, Producer, ProgramModel, {CompatibilityBinding, ObservedInput},
};
use flutterdec_adapter::primitives::Sha256Digest;
use flutterdec_adapter::protocol::{AdapterErrorCode, AdapterStatus, RequestedBackend};
use flutterdec_adapter::{
    run_adapter, AdapterInput, AdapterRegionInput, AdapterRun, ContainmentReport, HostError,
    LibappSource, Limits, OutputStream,
};
use flutterdec_loader::identity::SnapshotIdentity;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tempfile::TempDir;

const RET: [u8; 4] = 0xD65F_03C0u32.to_le_bytes();

/// The one environment variable a child must never see.
const SECRET: &str = "FLUTTERDEC_TEST_HOST_SECRET";
const SECRET_VALUE: &str = "a-host-secret-no-adapter-may-read";

/// Shared preamble for every hostile adapter.
///
/// It gives each one the request, a way to answer correctly, and a way to leave
/// a probe beside `--input-path`. What each adapter does with them is the test.
const PRELUDE: &str = r#"#!/usr/bin/env python3
import argparse, json, os, pathlib, sys

P = argparse.ArgumentParser()
P.add_argument("--request", required=True)
P.add_argument("--result", required=True)
P.add_argument("--input-path")
P.add_argument("--libapp-path")
ARGS = P.parse_args()
REQUEST = json.loads(pathlib.Path(ARGS.request).read_text())
OUTPUT = REQUEST["output"]


def sidecar(name):
    return pathlib.Path(ARGS.input_path + "." + name)


def write_model(text=None):
    pathlib.Path(OUTPUT).write_text(
        text if text is not None else sidecar("model").read_text()
    )


def write_result(**over):
    doc = {
        "protocol_major": 1,
        "model_major": 4,
        "status": "ok",
        "model": OUTPUT,
        "error": None,
        "resolved_backend": "internal",
        "fallback_reason": None,
        "diagnostics": [],
    }
    doc.update(over)
    pathlib.Path(ARGS.result).write_text(json.dumps(doc))


def succeed(**over):
    write_model()
    write_result(**over)
"#;

/// One hostile adapter, published as the artifact a record authorizes.
struct Rig {
    dir: TempDir,
    installed: support::Authorized,
    identity: SnapshotIdentity,
    input_path: PathBuf,
    regions: Vec<Vec<u8>>,
}

impl Rig {
    /// `body` is appended to [`PRELUDE`] and is what the adapter actually does.
    fn new(body: &str) -> Self {
        Self::named(body, None)
    }

    /// The same adapter, published under a file name the caller chooses.
    fn named(body: &str, file_name: Option<&str>) -> Self {
        let dir = TempDir::new().expect("tempdir");
        let source = dir.path().join("hostile_adapter");
        fs::write(&source, format!("{PRELUDE}\n{body}")).expect("write hostile adapter");
        fs::set_permissions(&source, fs::Permissions::from_mode(0o755)).expect("chmod");
        Self::publish(dir, &source, file_name)
    }

    /// An adapter that is already an executable file: a compiled one, rather
    /// than a script the kernel hands to an interpreter.
    fn built(dir: TempDir, executable: &Path) -> Self {
        Self::publish(dir, executable, None)
    }

    /// Authorize `source` as this snapshot's adapter and prepare the answer a
    /// well-behaved one would give.
    fn publish(dir: TempDir, source: &Path, file_name: Option<&str>) -> Self {
        let identity = support::identity();
        let installed = support::Authorized::install_named(source, &identity, file_name);
        let input_path = dir.path().join("app.apk");
        fs::write(&input_path, b"not really a zip").expect("write input");

        let rig = Self {
            dir,
            installed,
            identity,
            input_path,
            regions: vec![vec![0u8; 64], vec![0u8; 64], RET.to_vec(), RET.repeat(4)],
        };
        // The valid answer, prepared by the host side so a hostile adapter that
        // is only hostile in one respect can still be correct in every other.
        fs::write(rig.sidecar("model"), rig.valid_model().to_canonical_json())
            .expect("write the prepared model");
        rig
    }

    fn sidecar(&self, name: &str) -> PathBuf {
        self.dir.path().join(format!("app.apk.{name}"))
    }

    fn host_regions(&self) -> Vec<InputRegion> {
        self.region_inputs()
            .into_iter()
            .map(|region| InputRegion {
                region: region.region,
                size: region.bytes.len() as u64,
                sha256: Sha256Digest::of(region.bytes),
                virtual_address: region.virtual_address,
                executable: region.region.is_executable(),
            })
            .collect()
    }

    /// A model that carries exactly the host facts this invocation was built
    /// from, and admits it recovered nothing.
    fn valid_model(&self) -> ProgramModel {
        let mut model = support::unavailable_model();
        model.producer = self.producer();
        model.compatibility = Some(self.binding());
        model.input = ObservedInput {
            identity: self.identity.clone(),
            regions: self.host_regions(),
        };
        model
    }

    fn producer(&self) -> Producer {
        self.installed.producer()
    }

    fn binding(&self) -> CompatibilityBinding {
        self.installed.binding()
    }

    fn region_inputs(&self) -> Vec<AdapterRegionInput<'_>> {
        vec![
            AdapterRegionInput {
                region: InputRegionName::VmData,
                bytes: &self.regions[0],
                virtual_address: None,
            },
            AdapterRegionInput {
                region: InputRegionName::IsolateData,
                bytes: &self.regions[1],
                virtual_address: None,
            },
            AdapterRegionInput {
                region: InputRegionName::VmInstructions,
                bytes: &self.regions[2],
                virtual_address: Some(0x1000),
            },
            AdapterRegionInput {
                region: InputRegionName::IsolateInstructions,
                bytes: &self.regions[3],
                virtual_address: Some(0x2000),
            },
        ]
    }

    fn input(&self, limits: Limits) -> AdapterInput<'_> {
        AdapterInput {
            identity: &self.identity,
            authorization: self.installed.authorization(),
            producer: self.producer(),
            compatibility: self.binding(),
            regions: self.region_inputs(),
            input_path: Some(&self.input_path),
            libapp: Some(LibappSource::File(&self.input_path)),
            requested_backend: RequestedBackend::Auto,
            limits,
        }
    }

    fn run(&self, limits: Limits) -> Result<AdapterRun, HostError> {
        run_adapter(&self.installed.exec, &self.input(limits))
    }

    fn fail(&self, limits: Limits) -> HostError {
        let err = self
            .run(limits)
            .expect_err("this adapter cannot produce a usable model");
        assert!(
            !err.is_pre_spawn(),
            "a failure of a running child was classified as a pre-spawn refusal: {err}"
        );
        err
    }
}

/// Short deadlines everywhere, so a test that is supposed to hit a bound does it
/// in test time rather than in adapter time.
fn brisk() -> Limits {
    Limits {
        wall_clock: Duration::from_millis(1500),
        ..Limits::default()
    }
}

// -- VAL-HOST-002: bounded, process-tree-safe execution -----------------------

#[test]
fn an_adapter_that_never_finishes_is_terminated_at_its_deadline() {
    let rig = Rig::new("import time\ntime.sleep(600)\n");
    let started = Instant::now();
    let err = rig.fail(Limits {
        wall_clock: Duration::from_millis(400),
        ..Limits::default()
    });
    let elapsed = started.elapsed();

    assert!(
        matches!(err, HostError::Timeout { .. }),
        "wrong failure: {err}"
    );
    assert!(
        elapsed < Duration::from_secs(20),
        "the host waited {elapsed:?} on a 400ms deadline"
    );
    assert!(
        elapsed >= Duration::from_millis(400),
        "the host gave up in {elapsed:?}, before the deadline it promised"
    );
}

/// A backend that shells out and abandons the child is the ordinary case, not an
/// exotic one. The grandchild must not outlive the run, and it must not hold the
/// host's pipes open either.
#[test]
fn a_grandchild_the_adapter_abandoned_does_not_outlive_the_run() {
    let rig = Rig::new(
        r#"import os, time
if os.fork() == 0:
    # Detached from the adapter's own lifetime on purpose.
    time.sleep(4)
    sidecar("grandchild").write_text("still running")
    os._exit(0)
time.sleep(0.1)
raise SystemExit(3)
"#,
    );
    let started = Instant::now();
    let err = rig.fail(brisk());
    let elapsed = started.elapsed();

    assert!(
        matches!(err, HostError::NoResult { .. }),
        "wrong failure: {err}"
    );
    assert!(
        elapsed < Duration::from_secs(3),
        "the host waited {elapsed:?} for a grandchild that was holding its pipes open"
    );
    std::thread::sleep(Duration::from_secs(6));
    assert!(
        !rig.sidecar("grandchild").exists(),
        "a grandchild outlived the adapter run that created it"
    );
}

#[test]
fn an_adapter_that_floods_a_stream_is_capped_and_terminated() {
    for (stream, target) in [
        (OutputStream::Stdout, "stdout"),
        (OutputStream::Stderr, "stderr"),
    ] {
        let rig = Rig::new(&format!(
            "import sys\nblock = 'x' * 65536\nwhile True:\n    sys.{target}.write(block)\n    sys.{target}.flush()\n"
        ));
        let started = Instant::now();
        let err = rig.fail(Limits {
            max_stdout_bytes: 256 * 1024,
            max_stderr_bytes: 256 * 1024,
            ..brisk()
        });
        assert!(
            matches!(
                err,
                HostError::OutputLimitExceeded { stream: seen, limit } if seen == stream && limit == 256 * 1024
            ),
            "{target}: wrong failure: {err}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "{target}: the flood was not stopped promptly"
        );
    }
}

#[test]
fn an_oversized_model_is_refused_by_size_rather_than_read() {
    let rig = Rig::new(
        r#"write_model("[" + "0," * 400000 + "0]")
write_result()
"#,
    );
    let err = rig.fail(Limits {
        max_model_bytes: 4096,
        ..brisk()
    });
    assert!(
        matches!(
            err,
            HostError::DocumentTooLarge { ref document, limit: 4096, .. } if document == "model"
        ),
        "wrong failure: {err}"
    );
}

#[test]
fn an_oversized_result_document_is_refused() {
    let rig = Rig::new(
        r#"write_model()
write_result(diagnostics=[
    {"code": "domain_not_recovered", "severity": "warning",
     "subject": "functions", "message": "x" * 200000}
])
"#,
    );
    let err = rig.fail(Limits {
        max_result_bytes: 4096,
        ..brisk()
    });
    assert!(
        matches!(
            err,
            HostError::DocumentTooLarge { ref document, limit: 4096, .. } if document == "result"
        ),
        "wrong failure: {err}"
    );
}

#[test]
fn an_adapter_that_crashes_is_reported_as_a_signal() {
    let rig = Rig::new("import os, signal\nos.kill(os.getpid(), signal.SIGSEGV)\n");
    let err = rig.fail(brisk());
    assert!(
        matches!(err, HostError::Crashed { signal, .. } if signal == libc::SIGSEGV),
        "wrong failure: {err}"
    );
}

#[test]
fn a_nonzero_exit_with_no_result_is_reported_with_bounded_output() {
    let rig = Rig::new(
        r#"import sys
sys.stderr.write("A" * 500000)
sys.stderr.write("\nthe last line is the one that matters\n")
raise SystemExit(9)
"#,
    );
    let err = rig.fail(brisk());
    let HostError::NoResult { ref stderr, .. } = err else {
        panic!("wrong failure: {err}");
    };
    assert!(
        stderr.contains("the last line is the one that matters"),
        "the excerpt dropped the useful end of the stream"
    );
    assert!(
        stderr.len() < 4096,
        "a diagnostic carried {} bytes of child output",
        stderr.len()
    );
    assert!(
        format!("{err}").len() < 8192,
        "the rendered error is not bounded"
    );
}

#[test]
fn a_malformed_result_document_is_rejected_as_such() {
    let rig = Rig::new(
        r#"import pathlib
write_model()
pathlib.Path(ARGS.result).write_text("{ this is not json")
"#,
    );
    let err = rig.fail(brisk());
    assert!(
        matches!(
            err,
            HostError::MalformedDocument { ref document, .. } if document == "result"
        ),
        "wrong failure: {err}"
    );
}

#[test]
fn a_malformed_model_document_is_rejected_as_such() {
    let rig = Rig::new("write_model(\"{\\\"model_version\\\": 4}\")\nwrite_result()\n");
    let err = rig.fail(brisk());
    assert!(
        matches!(
            err,
            HostError::MalformedDocument { ref document, .. } if document == "model"
        ),
        "wrong failure: {err}"
    );
}

/// A model that claims a different snapshot than the one the host read. The
/// adapter does not get to describe its own input.
#[test]
fn a_model_that_claims_another_identity_is_rejected() {
    let rig = Rig::new(
        r#"import json
model = json.loads(sidecar("model").read_text())
model["input"]["identity"]["hash"] = "ffffffffffffffffffffffffffffffff"
write_model(json.dumps(model))
write_result()
"#,
    );
    let err = rig.fail(brisk());
    assert!(
        matches!(err, HostError::ModelRejected(_)),
        "wrong failure: {err}"
    );
}

#[test]
fn a_model_written_somewhere_other_than_the_output_handle_is_refused() {
    let rig = Rig::new(
        r#"import pathlib
pathlib.Path("elsewhere.json").write_text(sidecar("model").read_text())
write_result(model="elsewhere.json")
"#,
    );
    let err = rig.fail(brisk());
    assert!(
        matches!(err, HostError::ModelPathMismatch { .. }),
        "wrong failure: {err}"
    );
}

#[test]
fn an_adapter_that_reports_failure_is_reported_verbatim_and_bounded() {
    let rig = Rig::new(
        r#"write_result(status="failed", model=None, resolved_backend=None,
             error={"code": "parse_failed", "message": "B" * 300000})
"#,
    );
    let err = rig.fail(Limits {
        max_result_bytes: 1024 * 1024,
        ..brisk()
    });
    let HostError::AdapterFailed {
        status,
        code,
        ref message,
    } = err
    else {
        panic!("wrong failure: {err}");
    };
    assert_eq!(status, AdapterStatus::Failed);
    assert_eq!(code, AdapterErrorCode::ParseFailed);
    assert!(
        message.len() < 4096,
        "the adapter's own message was quoted unbounded: {} bytes",
        message.len()
    );
}

// -- VAL-HOST-003: workspace and inherited authority --------------------------

/// The probe every isolation case runs.
///
/// It records what the child could see, then answers correctly, so the same
/// adapter serves both the success path and the "what was visible" question.
const PROBE: &str = r#"import json, os, pathlib, sys

cwd = os.getcwd()
visible = []
try:
    for name in sorted(os.listdir("/proc/self/fd")):
        try:
            visible.append(os.readlink("/proc/self/fd/" + name))
        except OSError:
            pass
except OSError:
    visible = None

inputs_writable = {}
for handle in REQUEST["inputs"]:
    try:
        with open(handle["path"], "ab") as fp:
            fp.write(b"tampered")
        inputs_writable[handle["region"]] = True
    except OSError:
        inputs_writable[handle["region"]] = False

sidecar("probe").write_text(json.dumps({
    "cwd": cwd,
    "cwd_mode": oct(os.stat(cwd).st_mode & 0o7777),
    "env": dict(os.environ),
    "fds": visible,
    "inputs_writable": inputs_writable,
    # Resolved, because `os.getcwd()` above is resolved too and Darwin's
    # temporary directory lives behind a `/var` -> `/private/var` symlink. Two
    # spellings of one directory would fail a containment check that is true.
    "home": os.path.realpath(os.environ["HOME"]) if "HOME" in os.environ else None,
    "tmpdir": os.path.realpath(os.environ["TMPDIR"]) if "TMPDIR" in os.environ else None,
    "stdin_is_tty": sys.stdin.isatty(),
    "stdin_read": (lambda: sys.stdin.read(16))(),
}))
"#;

#[derive(serde::Deserialize)]
struct Probe {
    cwd: String,
    cwd_mode: String,
    env: std::collections::BTreeMap<String, String>,
    fds: Option<Vec<String>>,
    inputs_writable: std::collections::BTreeMap<String, bool>,
    home: Option<String>,
    tmpdir: Option<String>,
    stdin_read: String,
}

fn probe_of(rig: &Rig) -> Probe {
    serde_json::from_slice(&fs::read(rig.sidecar("probe")).expect("the adapter wrote a probe"))
        .expect("the probe is JSON")
}

/// A file the host has open across the run, whose name is unmistakable if it
/// ever shows up in a child's descriptor table.
fn open_sentinel(dir: &Path) -> fs::File {
    let path = dir.join("host-only-descriptor-sentinel");
    fs::File::create(path).expect("open the descriptor sentinel")
}

#[test]
fn an_invocation_sees_a_private_directory_and_nothing_of_the_host() {
    std::env::set_var(SECRET, SECRET_VALUE);
    let rig = Rig::new(&format!("{PROBE}\nsucceed()\n"));
    let _sentinel = open_sentinel(rig.dir.path());

    let run = rig.run(brisk()).expect("the probe adapter succeeds");
    assert!(run.model.functions.is_empty());
    let probe = probe_of(&rig);

    assert_eq!(
        probe.cwd_mode, "0o700",
        "the invocation directory is readable by someone other than its owner"
    );
    assert!(
        !Path::new(&probe.cwd).exists(),
        "the invocation directory {} survived a successful run",
        probe.cwd
    );

    // The environment is an allowlist, not a filter of things that looked
    // dangerous. Anything not named is simply not there.
    let allowed: std::collections::BTreeSet<&str> = [
        "PATH",
        "LANG",
        "LC_ALL",
        "PYTHON",
        "XDG_CACHE_HOME",
        "FLUTTERDEC_BLUTTER_CMD",
        "FLUTTERDEC_BLUTTER_PY",
        "FLUTTERDEC_R2FLUTTER_CMD",
        "FLUTTERDEC_R2FLUTTER_BIN",
        "FLUTTERDEC_R2FLUTTER_TIMEOUT",
        "HOME",
        "TMPDIR",
        "PWD",
    ]
    .into_iter()
    .collect();
    // Darwin's CoreFoundation writes this into its *own* environment while the
    // library initializes, so it appears in any child that links it no matter
    // what the host passed. It is the platform naming the user's text encoding
    // in the child, not a host variable that leaked through: the host builds the
    // child's environment vector explicitly and this name is not in it.
    #[cfg(not(target_os = "linux"))]
    let allowed = {
        let mut allowed = allowed;
        allowed.insert("__CF_USER_TEXT_ENCODING");
        allowed
    };
    for name in probe.env.keys() {
        assert!(
            allowed.contains(name.as_str()),
            "the adapter inherited {name}, which is not on the allowlist"
        );
    }
    assert!(
        !probe.env.contains_key(SECRET),
        "the adapter inherited the host secret"
    );
    let rendered = serde_json::to_string(&probe.env).expect("env is serializable");
    assert!(
        !rendered.contains(SECRET_VALUE),
        "the secret's value reached the adapter under another name"
    );

    // `HOME` and `TMPDIR` point inside the workspace, so an adapter that writes
    // to either writes somewhere that gets cleaned up.
    for (label, value) in [
        ("HOME", probe.home.clone()),
        ("TMPDIR", probe.tmpdir.clone()),
    ] {
        let value = value.unwrap_or_else(|| panic!("{label} is set"));
        assert!(
            Path::new(&value).starts_with(&probe.cwd),
            "{label} is {value}, which is outside the invocation directory"
        );
    }

    for (region, writable) in &probe.inputs_writable {
        assert!(
            !writable,
            "the adapter was able to rewrite its own input handle {region}"
        );
    }

    assert_eq!(probe.stdin_read, "", "stdin is not empty");

    if let Some(fds) = &probe.fds {
        for target in fds {
            assert!(
                !target.contains("host-only-descriptor-sentinel"),
                "the adapter inherited an unrelated host descriptor: {target}"
            );
        }
        assert!(
            fds.len() <= 8,
            "the adapter started with {} descriptors open: {fds:?}",
            fds.len()
        );
    }
}

#[test]
fn the_invocation_directory_is_removed_after_a_failure_and_after_a_timeout() {
    let failing = Rig::new(&format!("{PROBE}\nraise SystemExit(4)\n"));
    let err = failing.fail(brisk());
    assert!(
        matches!(err, HostError::NoResult { .. }),
        "unexpected: {err}"
    );
    let probe = probe_of(&failing);
    assert!(
        !Path::new(&probe.cwd).exists(),
        "the invocation directory {} survived a failed run",
        probe.cwd
    );

    let hanging = Rig::new(&format!("{PROBE}\nimport time\ntime.sleep(600)\n"));
    let err = hanging.fail(Limits {
        wall_clock: Duration::from_millis(600),
        ..Limits::default()
    });
    assert!(
        matches!(err, HostError::Timeout { .. }),
        "unexpected: {err}"
    );
    let probe = probe_of(&hanging);
    assert!(
        !Path::new(&probe.cwd).exists(),
        "the invocation directory {} survived a timed-out run",
        probe.cwd
    );
}

/// An adapter that makes its own workspace unremovable must not be able to leak
/// it onto the host.
#[test]
fn a_workspace_the_adapter_sealed_is_still_removed() {
    let rig = Rig::new(
        r#"import os, pathlib
d = pathlib.Path("sealed")
d.mkdir()
(d / "inside").write_text("x")
os.chmod(d, 0o500)
sidecar("probe").write_text('{"cwd": %s, "cwd_mode": "0o700", "env": {}, "fds": null, "inputs_writable": {}, "home": null, "tmpdir": null, "stdin_read": ""}' % __import__("json").dumps(os.getcwd()))
succeed()
"#,
    );
    rig.run(brisk()).expect("the adapter succeeds");
    let probe = probe_of(&rig);
    assert!(
        !Path::new(&probe.cwd).exists(),
        "an adapter kept its workspace alive by making a directory unwritable: {}",
        probe.cwd
    );
}

// -- VAL-HOST-006: what the child actually is ---------------------------------

/// A `#!` adapter is the awkward case and the one this project actually ships:
/// the kernel cannot hand an interpreter a descriptor, so it hands it a name for
/// one, and a host that got this wrong would either fail to start scripts at all
/// or quietly fall back to a file anyone could re-point.
///
/// The adapter reports what it is: the name it was started under, the digest of
/// the bytes behind that name, every executable file it can find in its own
/// workspace, and what happened when it tried to change its own image through
/// the only name it has. The last of those is the portable half of the property.
/// Where the image is an anonymous descriptor the name is `/dev/fd/N` and the
/// seals refuse the write; where the platform cannot execute a descriptor the
/// name is a real path and the kernel refuses the write, the rename and the
/// unlink because the host froze it. Either way the answer to "can the bytes
/// behind this name change while they run" is no, and it is answered by the
/// kernel rather than by a mode bit.
#[test]
fn a_shebang_adapter_runs_from_an_image_that_cannot_be_repointed() {
    let rig = Rig::new(
        r#"import hashlib

executables = []
for base, _dirs, files in os.walk(os.getcwd()):
    for name in files:
        found = os.path.join(base, name)
        try:
            if os.stat(found).st_mode & 0o111:
                executables.append(found)
        except OSError:
            pass

image = sys.argv[0]

def attempt(action):
    try:
        action()
        return "succeeded"
    except OSError as exc:
        return exc.strerror or type(exc).__name__

def overwrite():
    with open(image, "r+b") as handle:
        handle.write(b"\x00")
        handle.flush()

mutations = {
    "overwrite": attempt(overwrite),
    "rename": attempt(lambda: os.rename(image, image + ".moved")),
    "unlink": attempt(lambda: os.unlink(image)),
}

sidecar("image").write_text(json.dumps({
    "argv0": sys.argv[0],
    "sha256": hashlib.sha256(pathlib.Path(sys.argv[0]).read_bytes()).hexdigest(),
    "workspace_executables": executables,
    "cwd": os.getcwd(),
    "mutations": mutations,
}))
succeed()
"#,
    );

    let run = rig.run(brisk()).expect("the shebang adapter succeeds");
    assert!(run.model.functions.is_empty());

    #[derive(serde::Deserialize)]
    struct Image {
        argv0: String,
        sha256: String,
        workspace_executables: Vec<String>,
        #[allow(dead_code)]
        cwd: String,
        mutations: std::collections::BTreeMap<String, String>,
    }
    let image: Image =
        serde_json::from_slice(&fs::read(rig.sidecar("image")).expect("the adapter wrote a probe"))
            .expect("the probe is JSON");

    let authorized = fs::read(&rig.installed.exec).expect("read the authorized artifact");
    assert_eq!(
        image.sha256,
        support::hex_digest(&authorized),
        "the interpreter read something other than the verified artifact"
    );

    // The portable half: whatever the name is, it is not a way to change what
    // is running.
    for step in ["overwrite", "rename", "unlink"] {
        let outcome = image
            .mutations
            .get(step)
            .unwrap_or_else(|| panic!("the adapter did not report {step}: {:?}", image.mutations));
        assert_ne!(
            outcome, "succeeded",
            "the running adapter could {step} its own image through {}",
            image.argv0
        );
    }

    // The Linux half, which is stronger because the platform allows it: the
    // image is not on any filesystem at all.
    #[cfg(target_os = "linux")]
    {
        assert!(
            image.workspace_executables.is_empty(),
            "the invocation workspace holds an executable pathname: {:?}",
            image.workspace_executables
        );
        assert!(
            !image.argv0.starts_with(&image.cwd),
            "the adapter was started from a path inside its own workspace: {}",
            image.argv0
        );
        assert!(
            image.argv0.starts_with("/dev/fd/") || image.argv0.starts_with("/proc/self/fd/"),
            "the adapter was started from a filesystem pathname: {}",
            image.argv0
        );
    }

    // Elsewhere a descriptor cannot be executed, so the image is a path — and
    // then it has to be the *only* executable path the invocation exposes. The
    // two spellings can differ by the platform's own symlinks (`/var` against
    // `/private/var`), so the final component is what is compared.
    #[cfg(not(target_os = "linux"))]
    {
        let image_name = image
            .argv0
            .rsplit('/')
            .next()
            .expect("argv[0] has a final component");
        assert_eq!(
            image.workspace_executables.len(),
            1,
            "the invocation workspace exposes an executable other than the frozen image: {:?}",
            image.workspace_executables
        );
        assert!(
            image.workspace_executables[0].ends_with(image_name),
            "the executable in the workspace is not the image the adapter is running: {:?} against {}",
            image.workspace_executables,
            image.argv0
        );
    }
}

// -- VAL-HOST-008: containment must not cost the ability to execute -----------

/// The two halves of this host can be made to fight, and one kernel in ordinary
/// use makes them fight.
///
/// Asking for an empty route table without privileges means asking for a user
/// namespace, and some kernels answer that by placing the caller under a
/// mandatory access control profile rather than by refusing. Such a profile
/// decides what may be executed by *pathname*, and the image deliberately has
/// none — so a child that took the namespace could no longer start the adapter
/// at all, and every run on that host failed before a process existed.
///
/// The rule is that the run wins. An unavailable network control is a reported,
/// bounded loss; a host that cannot execute a verified image is not a host.
#[test]
fn network_isolation_is_never_bought_with_the_ability_to_execute() {
    let rig = Rig::new(
        "succeed()
",
    );
    let run = rig
        .run(brisk())
        .expect("the adapter runs whatever the host could or could not isolate");
    assert!(run.model.functions.is_empty());

    #[cfg(target_os = "linux")]
    if fs::read_to_string("/proc/sys/kernel/apparmor_restrict_unprivileged_userns")
        .map(|value| value.trim() != "0")
        .unwrap_or(false)
    {
        assert!(
            matches!(
                run.containment.network,
                flutterdec_adapter::ControlState::Unavailable { .. }
            ),
            "the host took a user namespace on a kernel that confines one, which is paid for with the ability to execute a pathless image: {:?}",
            run.containment.network
        );
    }
}

// -- VAL-HOST-004: platform containment claims --------------------------------

fn successful_report(limits: Limits) -> (ContainmentReport, Rig) {
    let rig = Rig::new("succeed()\n");
    let run = rig.run(limits).expect("the adapter succeeds");
    (run.containment, rig)
}

#[test]
fn every_named_control_is_reported_as_applied_or_unavailable() {
    let (report, _rig) = successful_report(brisk());
    for (name, state) in report.controls() {
        match state {
            flutterdec_adapter::ControlState::Applied { .. } => {}
            flutterdec_adapter::ControlState::Unavailable { reason } => assert!(
                !reason.trim().is_empty(),
                "{name} is unavailable without saying why"
            ),
        }
    }
    assert!(
        !report.process_tree_terminated,
        "a run that finished on its own was reported as one the host had to end"
    );
    // These need nothing the host cannot do for itself.
    assert!(report.wall_clock_deadline.is_applied());
    assert!(report.stdout_bytes.is_applied());
    assert!(report.stderr_bytes.is_applied());
    assert!(report.model_bytes.is_applied());
    // These are POSIX and must hold on every platform this crate builds for.
    assert!(
        report.process_group.is_applied(),
        "{:?}",
        report.process_group
    );
    assert!(
        report.descriptor_isolation.is_applied(),
        "{:?}",
        report.descriptor_isolation
    );
    assert!(report.cpu_seconds.is_applied(), "{:?}", report.cpu_seconds);
    assert!(report.file_size.is_applied(), "{:?}", report.file_size);
    assert!(report.descriptors.is_applied(), "{:?}", report.descriptors);
}

/// The Darwin kernel does not enforce `RLIMIT_AS`, offers no network namespace,
/// and gives no way to observe the per-user task count. Setting the first and
/// pretending about the other two is exactly the false claim the contract bans.
#[test]
fn a_platform_that_cannot_establish_a_control_never_claims_it() {
    let (report, _rig) = successful_report(brisk());
    if cfg!(target_os = "linux") {
        assert!(
            report.address_space.is_applied(),
            "linux can bound address space: {:?}",
            report.address_space
        );
        assert!(
            report.process_count.is_applied(),
            "linux can observe its task count: {:?}",
            report.process_count
        );
    } else {
        assert!(
            !report.address_space.is_applied(),
            "{:?}",
            report.address_space
        );
        assert!(
            !report.process_count.is_applied(),
            "{:?}",
            report.process_count
        );
        assert!(!report.network.is_applied(), "{:?}", report.network);
    }
}

#[test]
fn the_cpu_limit_stops_an_adapter_that_only_spins() {
    let rig = Rig::new("while True:\n    pass\n");
    let started = Instant::now();
    let err = rig.fail(Limits {
        cpu_seconds: 1,
        // Long enough that the deadline cannot be what stopped it.
        wall_clock: Duration::from_secs(60),
        ..Limits::default()
    });
    assert!(
        matches!(err, HostError::Crashed { signal, .. } if signal == libc::SIGXCPU),
        "wrong failure: {err}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "the CPU limit did not stop the spin"
    );
}

#[test]
fn the_file_size_limit_stops_an_adapter_that_writes_too_much() {
    // The interpreter ignores `SIGXFSZ`, so the limit shows up as a failed
    // write rather than as a dead process. Either way the bound is the claim,
    // and the bound is what the probe measures.
    let body = r#"import json, os
written = 0
try:
    with open("hog", "wb") as fp:
        for _ in range(64):
            fp.write(b"z" * 65536)
            fp.flush()
            written = os.path.getsize("hog")
except OSError:
    pass
sidecar("filesize").write_text(json.dumps(os.path.getsize("hog")))
succeed()
"#;

    let control = Rig::new(body);
    control
        .run(Limits {
            max_file_bytes: 16 * 1024 * 1024,
            ..brisk()
        })
        .expect("the control adapter answers");
    let control_size: u64 =
        serde_json::from_slice(&fs::read(control.sidecar("filesize")).expect("probe"))
            .expect("a size");
    assert_eq!(
        control_size,
        64 * 65536,
        "the control could not write the whole file, so the limited case proves nothing"
    );

    let rig = Rig::new(body);
    let run = rig
        .run(Limits {
            max_file_bytes: 64 * 1024,
            ..brisk()
        })
        .expect("the adapter still answers");
    assert!(run.containment.file_size.is_applied());
    let size: u64 =
        serde_json::from_slice(&fs::read(rig.sidecar("filesize")).expect("probe")).expect("a size");
    assert!(
        size <= 64 * 1024,
        "the adapter wrote a {size} byte file under a 64 KiB limit"
    );
}

#[test]
fn the_descriptor_limit_stops_an_adapter_that_opens_too_many() {
    let body = r#"import json
held = []
opened = 0
try:
    while opened < 200:
        held.append(open(ARGS.request, "rb"))
        opened += 1
except OSError:
    pass
for fp in held:
    fp.close()
sidecar("descriptors").write_text(json.dumps(opened))
succeed()
"#;

    let control = Rig::new(body);
    control
        .run(Limits {
            // Comfortably above the target and below the smallest per-process
            // ceiling this crate builds for; Darwin's inherited soft limit is
            // 256, so the control has to raise it to open anything.
            max_descriptors: 1024,
            ..brisk()
        })
        .expect("the control adapter answers");
    let control_opened: u32 =
        serde_json::from_slice(&fs::read(control.sidecar("descriptors")).expect("probe"))
            .expect("a count");
    assert_eq!(
        control_opened, 200,
        "the control could not open 200 descriptors, so the limited case proves nothing"
    );

    let rig = Rig::new(body);
    let run = rig
        .run(Limits {
            max_descriptors: 64,
            ..brisk()
        })
        .expect("the adapter still answers");
    assert!(run.containment.descriptors.is_applied());
    let opened: u32 = serde_json::from_slice(&fs::read(rig.sidecar("descriptors")).expect("probe"))
        .expect("a count");
    assert!(
        opened < 64,
        "the adapter opened {opened} descriptors under a limit of 64"
    );
}

/// `RLIMIT_AS` is Linux-only here, so the probe is too.
#[test]
#[cfg(target_os = "linux")]
fn the_address_space_limit_stops_an_adapter_that_allocates_too_much() {
    let rig = Rig::new(
        r#"import json
try:
    hog = bytearray(3 * 1024 * 1024 * 1024)
    outcome = "allocated"
except MemoryError:
    outcome = "refused"
sidecar("allocation").write_text(json.dumps(outcome))
succeed()
"#,
    );
    let run = rig
        .run(Limits {
            max_address_space_bytes: Some(1024 * 1024 * 1024),
            ..brisk()
        })
        .expect("the adapter still answers");
    assert!(run.containment.address_space.is_applied());
    let outcome: String =
        serde_json::from_slice(&fs::read(rig.sidecar("allocation")).expect("probe"))
            .expect("an outcome");
    assert_eq!(
        outcome, "refused",
        "a 3 GiB allocation succeeded under a 1 GiB address space limit"
    );
}

/// The process budget, shown as a difference rather than as an absolute.
///
/// An absolute assertion would be a guess about how busy the host is. Running
/// the same forking adapter twice, once with a budget and once without, is not.
#[test]
#[cfg(target_os = "linux")]
fn the_process_budget_stops_an_adapter_that_forks() {
    const ATTEMPTS: u32 = 64;
    let body = format!(
        r#"import json, os, time
started = 0
children = []
for _ in range({ATTEMPTS}):
    try:
        pid = os.fork()
    except OSError:
        break
    if pid == 0:
        time.sleep(0.5)
        os._exit(0)
    children.append(pid)
    started += 1
sidecar("forks").write_text(json.dumps(started))
for pid in children:
    try:
        os.waitpid(pid, 0)
    except OSError:
        pass
succeed()
"#
    );

    let unbudgeted = Rig::new(&body);
    unbudgeted
        .run(Limits {
            extra_processes: None,
            wall_clock: Duration::from_secs(30),
            ..Limits::default()
        })
        .expect("the control adapter answers");
    let control: u32 =
        serde_json::from_slice(&fs::read(unbudgeted.sidecar("forks")).expect("probe"))
            .expect("a count");
    assert_eq!(
        control, ATTEMPTS,
        "the control could not fork {ATTEMPTS} times, so the budgeted case proves nothing"
    );

    let budgeted = Rig::new(&body);
    let run = budgeted
        .run(Limits {
            extra_processes: Some(0),
            wall_clock: Duration::from_secs(30),
            ..Limits::default()
        })
        .expect("the budgeted adapter answers");
    assert!(
        run.containment.process_count.is_applied(),
        "{:?}",
        run.containment.process_count
    );
    let budgeted_forks: u32 =
        serde_json::from_slice(&fs::read(budgeted.sidecar("forks")).expect("probe"))
            .expect("a count");
    assert!(
        budgeted_forks < ATTEMPTS,
        "a zero process budget still allowed all {ATTEMPTS} forks"
    );
}

/// Network isolation is conditional on what the host permits, so the assertion
/// is conditional on what the host reported. What is not conditional is that the
/// two agree.
#[test]
fn network_isolation_is_enforced_exactly_when_it_is_claimed() {
    let rig = Rig::new(
        r#"import json, socket
try:
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.settimeout(0.5)
    s.connect(("192.0.2.1", 53))
    s.send(b"probe")
    outcome = "reachable"
except OSError as exc:
    outcome = "unreachable"
sidecar("network").write_text(json.dumps(outcome))
succeed()
"#,
    );
    let run = rig.run(brisk()).expect("the adapter answers");
    let outcome: String = serde_json::from_slice(&fs::read(rig.sidecar("network")).expect("probe"))
        .expect("an outcome");

    if run.containment.network.is_applied() {
        assert_eq!(
            outcome, "unreachable",
            "the host claimed network isolation and the adapter reached the network"
        );
    } else {
        // Nothing to assert about the network itself; the claim is the point,
        // and the host made none.
        assert!(matches!(
            run.containment.network,
            flutterdec_adapter::ControlState::Unavailable { .. }
        ));
    }
}

// -- VAL-HOST-007: the image is sealed or nothing runs ------------------------

/// A native adapter, so the sealed descriptor is proved on the path where the
/// kernel executes the image itself rather than handing its name to an
/// interpreter.
///
/// It reports where the kernel thinks it came from. On Linux that is the
/// anonymous file the host created, which has no directory entry anywhere, and
/// the run still has to succeed end to end: a host that could only start scripts
/// would be a host nobody could ship a compiled adapter to.
const NATIVE_ADAPTER: &str = r#"
#include <stdio.h>
#include <string.h>
#include <unistd.h>

/* Reads the prepared model beside --input-path and answers the request. The
   output handle is the one the host always issues, so no JSON parser is needed
   to be a correct adapter. */
int main(int argc, char **argv) {
    const char *result = 0, *input = 0;
    for (int i = 1; i + 1 < argc; i++) {
        if (!strcmp(argv[i], "--result")) result = argv[++i];
        else if (!strcmp(argv[i], "--input-path")) input = argv[++i];
    }
    if (!result || !input) return 2;

    char link[4096];
    ssize_t linked = readlink("/proc/self/exe", link, sizeof link - 1);
    if (linked < 0) linked = 0;
    link[linked] = 0;

    char path[4096];
    snprintf(path, sizeof path, "%s.image", input);
    FILE *probe = fopen(path, "w");
    if (!probe) return 3;
    fprintf(probe, "{\"argv0\":\"%s\",\"exe\":\"%s\"}", argv[0], link);
    fclose(probe);

    snprintf(path, sizeof path, "%s.model", input);
    FILE *prepared = fopen(path, "r");
    if (!prepared) return 4;
    FILE *model = fopen("out/model.json", "w");
    if (!model) return 5;
    char buffer[8192];
    size_t got;
    while ((got = fread(buffer, 1, sizeof buffer, prepared)) > 0)
        fwrite(buffer, 1, got, model);
    fclose(prepared);
    fclose(model);

    FILE *answer = fopen(result, "w");
    if (!answer) return 6;
    fputs("{\"protocol_major\":1,\"model_major\":4,\"status\":\"ok\","
          "\"model\":\"out/model.json\",\"error\":null,"
          "\"resolved_backend\":\"internal\",\"fallback_reason\":null,"
          "\"diagnostics\":[]}", answer);
    fclose(answer);
    return 0;
}
"#;

#[test]
fn a_native_adapter_runs_from_the_sealed_descriptor() {
    let dir = TempDir::new().expect("tempdir");
    let source = dir.path().join("native_adapter.c");
    let built = dir.path().join("native_adapter");
    fs::write(&source, NATIVE_ADAPTER).expect("write the native adapter");
    // `cc` is what linked this test binary, so a host that can build the suite
    // can build a native adapter for it.
    let compiler = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
    let compiled = std::process::Command::new(&compiler)
        .arg("-O0")
        .arg("-o")
        .arg(&built)
        .arg(&source)
        .output()
        .unwrap_or_else(|err| panic!("run {compiler}: {err}"));
    assert!(
        compiled.status.success(),
        "{compiler} refused the native adapter: {}",
        String::from_utf8_lossy(&compiled.stderr)
    );

    let rig = Rig::built(dir, &built);
    let run = rig.run(brisk()).expect("the native adapter succeeds");
    assert!(run.model.functions.is_empty());

    #[derive(serde::Deserialize)]
    struct Image {
        argv0: String,
        exe: String,
    }
    let image: Image =
        serde_json::from_slice(&fs::read(rig.sidecar("image")).expect("the adapter wrote a probe"))
            .expect("the probe is JSON");
    assert_eq!(
        image.argv0, "flutterdec-local-python",
        "the adapter was not started under the name the registry authorized"
    );
    if cfg!(target_os = "linux") {
        assert!(
            image.exe.starts_with("/memfd:flutterdec-adapter:"),
            "a native adapter ran from something other than the anonymous image: {}",
            image.exe
        );
    }
}

/// The kernel refusing to provide a sealed anonymous image is the end of the
/// run, not the start of a fallback.
///
/// `memfd_create` will not accept a name longer than it can store, and the label
/// is derived from the authorized artifact's file name, so publishing under a
/// long enough name makes the real syscall fail on every kernel without a hook
/// anywhere in the product. What follows has to be a typed pre-spawn refusal:
/// the adapter records that it ran, and that record must not exist.
#[test]
#[cfg(target_os = "linux")]
fn an_image_the_host_cannot_seal_refuses_the_run_instead_of_naming_a_file() {
    let unnameable = "a".repeat(250);
    let rig = Rig::named(
        "sidecar(\"ran\").write_text(\"the adapter started\")\nsucceed()\n",
        Some(&unnameable),
    );

    let err = rig
        .run(brisk())
        .expect_err("an image that cannot be sealed must not be executed");

    let HostError::ImageNotSealed(ref detail) = err else {
        panic!("wrong failure: {err}");
    };
    assert!(
        detail.contains("create an anonymous executable image"),
        "the refusal does not say what failed: {detail}"
    );
    assert!(
        err.is_pre_spawn(),
        "a refusal that started no child was not classified as pre-spawn: {err}"
    );
    assert!(
        !rig.sidecar("ran").exists(),
        "a child ran after the host refused to seal its image"
    );
    // Nothing was written next to the artifact either: the store holds what it
    // held before the run.
    let published: Vec<_> = fs::read_dir(rig.installed.store_root.join("artifacts"))
        .expect("read the store")
        .map(|entry| entry.expect("entry").file_name())
        .collect();
    assert_eq!(
        published,
        vec![std::ffi::OsString::from(&unnameable)],
        "the refused run left something in the store"
    );
}
