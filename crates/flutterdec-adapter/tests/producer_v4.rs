//! End-to-end tests for the checked-in Python producer.
//!
//! Every case here runs the real `adapters/python/adapter_template.py` as a
//! subprocess through [`run_adapter`], so what is under test is the artifact
//! that ships, not a Rust re-implementation of it. The model that comes back has
//! crossed a process boundary as JSON and been through the same parse and
//! semantic validation the core uses.
//!
//! The question these ask is not "did it produce a model" but "did it produce a
//! model that admits what it does not know". A producer that invents
//! `package:app/main.dart`, a `Global` class, a function called `main`, or a
//! calibrated-looking confidence passes a schema check and lies to every
//! consumer downstream.

mod support;

use flutterdec_adapter::model::{
    CapabilityLevel, ClassId, Domain, InputRegionName, PoolIndexSpace, ProducerTrust, ProgramModel,
    Provenance,
};
use flutterdec_adapter::protocol::{BackendId, RequestedBackend};
use flutterdec_adapter::{run_adapter, AdapterInput, AdapterRegionInput, AdapterRun, Limits};
use flutterdec_loader::identity::SnapshotIdentity;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

const VM_INSTR_VA: u64 = 0x1000;
const ISO_INSTR_VA: u64 = 0x2000;

/// `stp x29, x30, [sp, #-16]!` then `ret`: the frame prologue the internal
/// backend scans for, followed by something to end on.
const PROLOGUE: [u8; 4] = 0xA9BF_7BFDu32.to_le_bytes();
const RET: [u8; 4] = 0xD65F_03C0u32.to_le_bytes();

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize repo root")
}

/// The real producer, published as the artifact a registry record authorizes.
fn install(identity: &SnapshotIdentity) -> support::Authorized {
    install_named(identity, None)
}

/// Install under an arbitrary adapter file name.
///
/// Used to prove that a deliberately misleading filename changes nothing: the
/// resolved backend comes from the protocol result, not from the path, and the
/// record authorizes the path rather than the path authorizing itself.
fn install_named(identity: &SnapshotIdentity, file_name: Option<&str>) -> support::Authorized {
    support::Authorized::install_named(
        &repo_root().join("adapters/python/adapter_template.py"),
        identity,
        file_name,
    )

}

struct Snapshot {
    vm_data: Vec<u8>,
    isolate_data: Vec<u8>,
    vm_instr: Vec<u8>,
    isolate_instr: Vec<u8>,
}

/// Bytes with no printable runs and no ARM64 prologues: nothing to recover.
fn empty_snapshot() -> Snapshot {
    Snapshot {
        vm_data: vec![0u8; 64],
        isolate_data: vec![0u8; 64],
        vm_instr: RET.to_vec(),
        isolate_instr: RET.repeat(4),
    }
}

fn set_executable(path: &Path) {
    let mut perms = fs::metadata(path).expect("metadata").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).expect("chmod");
}

fn regions(snapshot: &Snapshot) -> Vec<AdapterRegionInput<'_>> {
    vec![
        AdapterRegionInput {
            region: InputRegionName::VmData,
            bytes: &snapshot.vm_data,
            virtual_address: None,
        },
        AdapterRegionInput {
            region: InputRegionName::IsolateData,
            bytes: &snapshot.isolate_data,
            virtual_address: None,
        },
        AdapterRegionInput {
            region: InputRegionName::VmInstructions,
            bytes: &snapshot.vm_instr,
            virtual_address: Some(VM_INSTR_VA),
        },
        AdapterRegionInput {
            region: InputRegionName::IsolateInstructions,
            bytes: &snapshot.isolate_instr,
            virtual_address: Some(ISO_INSTR_VA),
        },
    ]
}

fn run(
    installed: &support::Authorized,
    identity: &SnapshotIdentity,
    snapshot: &Snapshot,
    backend: RequestedBackend,
) -> Result<AdapterRun, String> {
    run_adapter(
        &installed.exec,
        &AdapterInput {
            identity,
            authorization: installed.authorization(),
            producer: installed.producer(),
            compatibility: installed.binding(),
            regions: regions(snapshot),
            input_path: None,
            libapp: None,
            requested_backend: backend,
            limits: Limits::default(),
        },
    )
    .map_err(|err| err.to_string())
}

/// Strings that would be fabrications if they appeared anywhere in the model.
const FABRICATIONS: &[&str] = &[
    "package:app/main.dart",
    "\"Global\"",
    "sub_",
    "fn_0x",
    "EntryPointCandidate",
    "BootMainCandidate",
    "dynamic_snapshot_string_model_v1",
];

fn assert_no_fabrications(model: &ProgramModel) {
    let json = String::from_utf8(model.to_canonical_json()).expect("model is utf-8");
    for needle in FABRICATIONS {
        assert!(
            !json.contains(needle),
            "model contains the fabricated token {needle}: {json}"
        );
    }
    // A confidence anywhere is a calibrated-looking number none of these
    // backends has anything to calibrate against.
    assert!(
        !json.contains("\"confidence\":0.") && !json.contains("\"confidence\":1"),
        "model carries a confidence score: {json}"
    );
    for function in &model.functions {
        if let Some(name) = &function.name {
            assert!(
                !name.text.starts_with("sub_"),
                "function {} carries an address-derived name",
                function.id
            );
        }
    }
    for class in &model.classes {
        assert_ne!(class.name, "Global", "a `Global` class was invented");
    }
}

/// Every unavailable domain has to say why, or "nothing was there" and "we did
/// not look" are the same answer.
fn assert_unavailable_domains_are_explained(model: &ProgramModel) {
    for domain in Domain::ALL {
        if model.capabilities.level(domain) != CapabilityLevel::Unavailable {
            continue;
        }
        assert!(
            model
                .diagnostics
                .iter()
                .any(|d| d.subject.as_deref() == Some(domain.as_str())),
            "domain {domain} is unavailable with no diagnostic: {:?}",
            model.diagnostics
        );
    }
}

#[test]
fn a_snapshot_with_nothing_in_it_yields_unavailable_domains_and_no_invented_records() {
    let identity = support::identity();
    let installed = install(&identity);
    let run = run(
        &installed,
        &identity,
        &empty_snapshot(),
        RequestedBackend::Fixed(BackendId::Internal),
    )
    .expect("internal backend runs on an empty snapshot");

    assert_eq!(run.resolved_backend, BackendId::Internal);
    assert_eq!(run.fallback_reason, None);

    let model = &run.model;
    assert_no_fabrications(model);
    assert_unavailable_domains_are_explained(model);

    // No library URI in the data image means no libraries, not one library named
    // after an app that may not exist.
    assert!(model.libraries.is_empty());
    assert_eq!(model.capabilities.libraries, CapabilityLevel::Unavailable);

    // This backend does not deserialize the snapshot, so it has no class table.
    assert!(model.classes.is_empty());
    assert_eq!(model.capabilities.classes, CapabilityLevel::Unavailable);
    assert_eq!(
        model.capabilities.class_relationships,
        CapabilityLevel::Unavailable
    );

    // Names are never recoverable from instruction bytes alone.
    assert_eq!(
        model.capabilities.function_names,
        CapabilityLevel::Unavailable
    );
    assert!(model.functions.iter().all(|f| f.name.is_none()));

    // Carved strings are not an ObjectPool.
    assert_eq!(model.object_pool.index_space, PoolIndexSpace::Ordinal);
    assert!(model.object_pool.geometry.is_none());
    assert_eq!(
        model.capabilities.pool_index_space,
        CapabilityLevel::Unavailable
    );
}

#[test]
fn heuristic_code_ranges_are_labelled_heuristic_and_stay_unnamed() {
    let identity = support::identity();
    let installed = install(&identity);
    let mut snapshot = empty_snapshot();
    // Three prologues in the isolate instruction image, so the scanner has
    // something to find and the domain comes back partial rather than empty.
    snapshot.isolate_instr = [
        PROLOGUE.as_slice(),
        RET.as_slice(),
        PROLOGUE.as_slice(),
        RET.as_slice(),
        PROLOGUE.as_slice(),
        RET.as_slice(),
    ]
    .concat();

    let run = run(
        &installed,
        &identity,
        &snapshot,
        RequestedBackend::Fixed(BackendId::Internal),
    )
    .expect("internal backend runs");
    let model = &run.model;
    assert_no_fabrications(model);
    assert_unavailable_domains_are_explained(model);

    assert!(
        !model.functions.is_empty(),
        "prologue scanning should recover code ranges"
    );
    assert_eq!(model.capabilities.functions, CapabilityLevel::Partial);
    for function in &model.functions {
        assert_eq!(
            function.provenance,
            Provenance::Heuristic,
            "a prologue guess is not an exact fact"
        );
        assert!(function.name.is_none());
        assert!(function.owner.is_none());
        assert_eq!(function.code_section_va, ISO_INSTR_VA);
        assert!(function.code.size > 0);
    }
    assert!(
        model
            .diagnostics
            .iter()
            .any(|d| d.subject.as_deref() == Some("functions")),
        "a heuristic-only domain has to say so: {:?}",
        model.diagnostics
    );
}

#[test]
fn carved_strings_become_ordinal_pool_entries_never_hardware_ones() {
    let identity = support::identity();
    let installed = install(&identity);
    let mut snapshot = empty_snapshot();
    snapshot.isolate_data = b"package:sample/widgets/home.dart\0onPressed\0Scaffold\0".to_vec();

    let run = run(
        &installed,
        &identity,
        &snapshot,
        RequestedBackend::Fixed(BackendId::Internal),
    )
    .expect("internal backend runs");
    let model = &run.model;
    assert_no_fabrications(model);
    assert_unavailable_domains_are_explained(model);

    // The carved URI becomes a library, but a heuristic one: it is a string that
    // looks like a URI, not an entry read out of a library table.
    assert_eq!(
        model
            .libraries
            .iter()
            .map(|l| l.uri.as_str())
            .collect::<Vec<_>>(),
        vec!["package:sample/widgets/home.dart"]
    );
    assert!(model
        .libraries
        .iter()
        .all(|l| l.provenance == Provenance::Heuristic));
    assert_eq!(model.capabilities.libraries, CapabilityLevel::Partial);

    assert!(!model.object_pool.entries.is_empty());
    assert_eq!(model.object_pool.index_space, PoolIndexSpace::Ordinal);
    assert_eq!(
        model.capabilities.pool_index_space,
        CapabilityLevel::Unavailable
    );
    for entry in &model.object_pool.entries {
        assert_eq!(entry.provenance, Provenance::Heuristic);
        assert_eq!(entry.confidence, None);
        assert_eq!(entry.target_va, None);
    }
    // Ascending, unique indexes: the host rejects anything else.
    let indexes = model
        .object_pool
        .entries
        .iter()
        .map(|e| e.index)
        .collect::<Vec<_>>();
    let mut sorted = indexes.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(indexes, sorted);
}

#[test]
fn a_pinned_backend_that_cannot_run_fails_instead_of_falling_back() {
    let identity = support::identity();
    let installed = install(&identity);
    // r2flutter is not on PATH in this environment, and the input path the
    // backend needs is absent, so it cannot run. Pinned means it must fail.
    let err = run(
        &installed,
        &identity,
        &empty_snapshot(),
        RequestedBackend::Fixed(BackendId::R2Flutter),
    )
    .expect_err("a pinned r2flutter run must not fall back to internal");
    assert!(
        err.contains("r2flutter"),
        "the failure should name the backend that could not run: {err}"
    );
}

#[test]
fn auto_falls_back_to_internal_and_says_why() {

    let identity = support::identity();
    let installed = install(&identity);
    let run = run(
        &installed,
        &identity,
        &empty_snapshot(),
        RequestedBackend::Auto,
    )
    .expect("auto reaches the internal backend");

    assert_eq!(run.resolved_backend, BackendId::Internal);
    assert!(
        run.fallback_reason.is_some(),
        "auto that did not get its first choice has to say why"
    );
}

/// The resolved backend is a typed field on the protocol result. A filename that
/// says otherwise, however loudly, changes nothing.
#[test]
fn a_misleading_adapter_filename_cannot_change_the_resolved_backend() {
    let identity = support::identity();
    for name in [
        "r2flutter_serwalker_adapter",
        "blutter_bridge_model_v1",
        "internal_but_actually_r2flutter",
        "snapshot_serwalker",
    ] {
        let installed = install_named(&identity, Some(name));
        let run = run(
            &installed,
            &identity,
            &empty_snapshot(),
            RequestedBackend::Fixed(BackendId::Internal),
        )
        .expect("internal backend runs");
        assert_eq!(
            run.resolved_backend,
            BackendId::Internal,
            "adapter file named {name} changed the resolved backend"
        );
        assert_eq!(
            run.model.producer.trust,
            ProducerTrust::Registered,
            "trust is host-assigned; a filename cannot change it"
        );
    }
}

/// A producer that answers with a v2 or v3 document is rejected as the wrong
/// contract, not reinterpreted. This is the check that makes "no shim" real
/// across a process boundary rather than only in a unit test.
#[test]
fn a_producer_that_emits_a_legacy_model_is_rejected() {
    let dir = TempDir::new().expect("tempdir");
    let root = dir.path();
    fs::create_dir_all(root.join("artifacts")).expect("mkdir");
    let exec = root.join("artifacts/legacy_adapter");
    fs::write(
        &exec,
        r#"#!/usr/bin/env python3
import argparse, json, pathlib

p = argparse.ArgumentParser()
p.add_argument("--request", required=True)
p.add_argument("--result", required=True)
p.add_argument("--input-path")
p.add_argument("--libapp-path")
args = p.parse_args()

request = json.loads(pathlib.Path(args.request).read_text())
pathlib.Path(request["output"]).write_text(json.dumps({
    "schema_version": 3,
    "adapter_kind": "dynamic_snapshot_string_model_v1",
    "dart_version": "unknown",
    "snapshot_hash": "deadbeef",
    "arch": "arm64",
    "libraries": [{"id": 0, "uri": "package:app/main.dart", "name_display": "package:app/main.dart"}],
    "classes": [{"id": 0, "name": "Global", "super": "Object", "lib": "package:app/main.dart"}],
    "functions": [{"id": 0, "name": "main", "owner_class": "Global", "entry_va": 8192,
                   "size": 16, "code_section_va": 8192, "name_kind": "placeholder"}],
    "object_pool": []
}))
pathlib.Path(args.result).write_text(json.dumps({
    "protocol_major": 1,
    "model_major": 4,
    "status": "ok",
    "model": request["output"],
    "error": None,
    "resolved_backend": "internal",
    "fallback_reason": None,
    "diagnostics": []
}))
"#,
    )
    .expect("write legacy adapter");
    set_executable(&exec);

    let identity = support::identity();
    let installed = support::Authorized::install_named(&exec, &identity, Some("legacy_adapter"));
    drop(dir);
    let err = run(
        &installed,
        &identity,
        &empty_snapshot(),
        RequestedBackend::Fixed(BackendId::Internal),
    )
    .expect_err("a v3 document is not a v4 model");
    assert!(
        err.contains("legacy schema_version 3"),
        "the rejection should name the contract mismatch: {err}"
    );
    assert!(
        err.contains("no compatibility shim"),
        "the rejection should say there is no migration path: {err}"
    );
}

/// The Blutter bridge, against a stub that emits blutter's real output shape.
///
/// v3's version of this test asserted that the bridge *synthesized*
/// `EntryPointCandidate` pool entries for `main`-like names. That is exactly the
/// fabrication this contract removes: the bridge now reports what blutter's dump
/// says and nothing more, and boot-flow classification is the host's job.
#[test]
fn the_blutter_bridge_reports_what_the_dump_says_and_invents_nothing() {
    let dir = TempDir::new().expect("tempdir");
    let root = dir.path().to_path_buf();
    fs::create_dir_all(root.join("adapters/python")).expect("mkdir python");
    fs::create_dir_all(root.join("adapters/installed")).expect("mkdir installed");
    fs::copy(
        repo_root().join("adapters/python/adapter_template.py"),
        root.join("adapters/python/adapter_template.py"),
    )
    .expect("copy producer");

    let fake = root.join("fake_blutter");
    fs::write(
        &fake,
        r#"#!/usr/bin/env python3
from pathlib import Path
import sys

out_dir = Path(sys.argv[2])
asm = out_dir / "asm"
asm.mkdir(parents=True, exist_ok=True)
(asm / "main.dart").write_text(
    "// lib: 0, url: package:sample/main.dart\n"
    "class :: {\n"
    "  dynamic main() {\n"
    "// ** addr: 0x2000, size: 0x10\n"
    "  }\n"
    "}\n",
    encoding="utf-8",
)
(asm / "router.dart").write_text(
    "// lib: 1, url: package:sample/router.dart\n"
    "class RouterHost extends Object {\n"
    "  dynamic onNewIntent() {\n"
    "// ** addr: 0x2010, size: 0x10\n"
    "  }\n"
    "}\n",
    encoding="utf-8",
)
(out_dir / "pp.txt").write_text("[pp+0x18] \"a pool string\"\n", encoding="utf-8")
"#,
    )
    .expect("write fake blutter");
    set_executable(&fake);

    // The runner is pointed at through the adapter's own environment rather than
    // this process's, so concurrent tests cannot see it.
    let exec = root.join("adapters/installed/blutter_adapter");
    fs::write(
        &exec,
        format!(
            "#!/usr/bin/env python3\nfrom pathlib import Path\nimport os\nimport sys\nroot = Path(__file__).resolve().parents[1]\nos.environ['FLUTTERDEC_BLUTTER_CMD'] = {:?}\nsys.path.insert(0, str(root / 'python'))\nimport adapter_template\nif __name__ == '__main__':\n    raise SystemExit(adapter_template.entrypoint())\n",
            fake.display().to_string()
        ),
    )
    .expect("write adapter exec");
    set_executable(&exec);

    let identity = support::identity();
    let installed = support::Authorized::install_named(&exec, &identity, Some("blutter_adapter"));
    // The wrapper resolves the producer library relative to its own parent's
    // parent, so the library has to sit beside the published artifact's
    // directory rather than beside the copy it was made from.
    fs::create_dir_all(installed.store_root.join("python")).expect("mkdir published python");
    fs::copy(
        repo_root().join("adapters/python/adapter_template.py"),
        installed.store_root.join("python/adapter_template.py"),
    )
    .expect("publish producer library");
    let mut snapshot = empty_snapshot();
    snapshot.isolate_instr = RET.repeat(16);
    let input = root.join("app.apk");
    fs::write(&input, b"dummy").expect("write dummy input");

    let run = run_adapter(
        &installed.exec,
        &AdapterInput {
            identity: &identity,
            authorization: installed.authorization(),
            producer: installed.producer(),
            compatibility: installed.binding(),
            regions: regions(&snapshot),
            input_path: Some(&input),
            libapp: None,
            requested_backend: RequestedBackend::Fixed(BackendId::Blutter),
            limits: Limits::default(),
        },
    )
    .map_err(|err| err.to_string())
    .expect("blutter bridge runs");

    assert_eq!(run.resolved_backend, BackendId::Blutter);
    let model = &run.model;
    assert_no_fabrications(model);
    assert_unavailable_domains_are_explained(model);

    // `class :: {` is blutter's header for a library's top-level members. It is
    // not a class, so `main` has no owner rather than an owner called `Global`.
    let main = model
        .functions
        .iter()
        .find(|f| f.code.start_va == 0x2000)
        .expect("main's code range");
    assert_eq!(main.name_text(), Some("main"));
    assert_eq!(main.owner, None);
    assert_eq!(
        main.name.as_ref().expect("name").provenance,
        Provenance::Heuristic,
        "a name scraped out of rendered source is a guess about the text"
    );

    let on_new_intent = model
        .functions
        .iter()
        .find(|f| f.code.start_va == 0x2010)
        .expect("onNewIntent's code range");
    assert_eq!(model.owner_name(on_new_intent), Some("RouterHost"));
    assert_eq!(
        model.owner_library_uri(on_new_intent),
        Some("package:sample/router.dart")
    );

    // No boot-flow candidate was written into the pool, and the pool that is
    // there is ordinal: blutter's `pp+` displacements are not confirmed to be
    // PP-relative for this snapshot, so no index space is claimed.
    assert_eq!(model.object_pool.index_space, PoolIndexSpace::Ordinal);
    assert_eq!(
        model.capabilities.pool_index_space,
        CapabilityLevel::Unavailable
    );
    assert!(model
        .object_pool
        .entries
        .iter()
        .all(|e| e.target_va.is_none()));
}

/// Install the real producer with a fake `r2flutter` wired in, plus a dummy
/// target file for it to be pointed at.
///
/// `classes_json` is what the fake answers for `-jc`. Everything else answers
/// the least the backend needs to build a model: one instruction-table entry
/// inside the declared isolate region, no strings, and a `-jp` failure so no
/// pool geometry is claimed.
fn install_with_fake_r2flutter(hash: &str, classes_json: &str) -> (Installed, PathBuf) {
    let installed = install(hash);
    let adapters = installed
        .exec
        .parent()
        .and_then(Path::parent)
        .expect("the installed adapter lives under <root>/adapters/installed")
        .to_path_buf();

    let fake = adapters.join("fake_r2flutter");
    fs::write(
        &fake,
        format!(
            r#"#!/usr/bin/env python3
import json
import sys

if "-jc" in sys.argv:
    print(json.dumps({classes_json}))
elif "-ji" in sys.argv:
    print(json.dumps({{"entries": [{{"index": 0, "address": {iso}, "name": "method.Widget.build"}}]}}))
elif "-jxz" in sys.argv or "-jzz" in sys.argv:
    print(json.dumps([]))
else:
    raise SystemExit(1)
"#,
            classes_json = classes_json,
            iso = ISO_INSTR_VA,
        ),
    )
    .expect("write fake r2flutter");
    set_executable(&fake);

    // `FLUTTERDEC_R2FLUTTER_CMD` outranks `_BIN` and `PATH` in the producer's
    // resolver, so a real r2flutter on the developer's machine cannot win.
    fs::write(
        &installed.exec,
        r#"#!/usr/bin/env python3
from pathlib import Path
import os
import sys
root = Path(__file__).resolve().parents[1]
os.environ["FLUTTERDEC_R2FLUTTER_CMD"] = str(root / "fake_r2flutter")
sys.path.insert(0, str(root / "python"))
import adapter_template
if __name__ == "__main__":
    raise SystemExit(adapter_template.entrypoint())
"#,
    )
    .expect("write adapter exec");
    set_executable(&installed.exec);

    let target = adapters.join("libapp.so");
    fs::write(&target, b"dummy").expect("write dummy target");
    (installed, target)
}

fn run_r2flutter(
    installed: &Installed,
    identity: &SnapshotIdentity,
    snapshot: &Snapshot,
    target: &Path,
) -> Result<AdapterRun, String> {
    run_adapter(
        &installed.exec,
        &AdapterInput {
            identity,
            producer: producer(&installed.exec),
            compatibility: compatibility(),
            regions: regions(snapshot),
            input_path: None,
            libapp_path: Some(target),
            requested_backend: RequestedBackend::Fixed(BackendId::R2Flutter),
        },
    )
    .map_err(|err| format!("{err:#}"))
}

#[test]
fn a_structured_superclass_links_when_r2flutter_resolved_it_and_stays_null_when_it_did_not() {
    // r2flutter emits `super` as an object. It fills in `name` itself when the
    // reference resolves, so `Child` here is a superclass it could not name and
    // `Widget` is one it could.
    let (installed, target) = install_with_fake_r2flutter(
        "deadbeefdeadbeefdeadbeefdeadbeef",
        r#"[
        {"name": "Child", "super": {"type_ref": 35836}},
        {"name": "Widget", "super": {"ref": 12, "name": "StatefulWidget"}},
        {"name": "StatefulWidget"}
    ]"#,
    );
    let identity = support::identity();
    let run = run_r2flutter(&installed, &identity, &empty_snapshot(), &target)
        .expect("the r2flutter backend runs against the fake");

    assert_eq!(run.resolved_backend, BackendId::R2Flutter);
    let model = &run.model;
    assert_no_fabrications(model);
    assert_unavailable_domains_are_explained(model);

    let by_name = |name: &str| {
        model
            .classes
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("class {name} is missing from the model"))
    };

    // An unresolved reference is no edge, not an edge to `Object`. Inventing
    // one would assert a hierarchy the snapshot never stated.
    assert_eq!(by_name("Child").super_class, None);
    assert_eq!(
        by_name("Widget").super_class,
        Some(ClassId(by_name("StatefulWidget").id.0))
    );
    assert_eq!(
        model.capabilities.class_relationships,
        CapabilityLevel::Partial
    );
}

#[test]
fn class_relationships_stay_unavailable_when_no_superclass_resolves() {
    // Both shapes r2flutter emits for a superclass it could not name. Neither
    // is an edge, so the domain has nothing partial about it.
    let (installed, target) = install_with_fake_r2flutter(
        "deadbeefdeadbeefdeadbeefdeadbeef",
        r#"[
        {"name": "Child", "super": {"type_ref": 35836}},
        {"name": "Widget", "super": {"ref": 987}}
    ]"#,
    );
    let identity = support::identity();
    let run = run_r2flutter(&installed, &identity, &empty_snapshot(), &target)
        .expect("the r2flutter backend runs against the fake");

    let model = &run.model;
    assert_no_fabrications(model);
    assert_unavailable_domains_are_explained(model);

    assert_eq!(model.classes.len(), 2);
    assert!(model.classes.iter().all(|c| c.super_class.is_none()));
    // The class table itself was read, so classes are partial while the
    // relationships between them are not there at all.
    assert_eq!(model.capabilities.classes, CapabilityLevel::Partial);
    assert_eq!(
        model.capabilities.class_relationships,
        CapabilityLevel::Unavailable
    );
}
