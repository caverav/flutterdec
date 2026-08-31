/// The one parser family PR1 ships. A registry that could name others is PR2.
const PARSER_FAMILY_ID: &str = "flutterdec-local-python";

/// The profile id used when no Dart profile resolves for the snapshot hash.
///
/// Not a placeholder standing in for a real profile: it names the state, and the
/// digest below is the digest of that state, so two runs without a profile agree
/// and a run with one never collides with them.
const UNRESOLVED_PROFILE_ID: &str = "unresolved";

#[derive(Debug, Clone)]
struct LoadedModel {
    model: ProgramModel,
    /// The backend the protocol result named. Never inferred from a filename or
    /// from a substring of adapter output.
    resolved_backend: BackendId,
    fallback_reason: Option<FallbackReason>,
    adapter_exec: PathBuf,
    producer: Producer,
    compatibility: CompatibilityBinding,
    manifest_entry_version: Option<String>,
    manifest_entry_adapter: Option<String>,
}

/// A stable digest for a layout profile, or for the absence of one.
///
/// `DartProfile` is deserialized from vendored data and is not `Serialize`, so
/// the digest is taken over a canonical rendering of the fields that decide
/// layout. Adding a field to the profile without adding it here would make two
/// different profiles digest the same, which is why the rendering lists them
/// explicitly rather than reflecting.
fn profile_binding(profile: Option<&ResolvedDartProfile>) -> (String, Sha256Digest) {
    let Some(resolved) = profile else {
        return (
            UNRESOLVED_PROFILE_ID.to_string(),
            Sha256Digest::of(b"flutterdec:profile:unresolved"),
        );
    };
    let p = &resolved.profile;
    let mut cids = p.cids.iter().collect::<Vec<_>>();
    cids.sort();
    let canonical = format!(
        "dart_version={};profile_version={};tag_style={};compressed_word_size={};header_fields={};max_alignment={};heap_object_tag={};cids={:?}",
        resolved.dart_version,
        resolved.profile_version,
        p.tag_style.as_str(),
        p.compressed_word_size,
        p.header_fields,
        p.max_alignment,
        p.heap_object_tag,
        cids,
    );
    (
        resolved.profile_version.clone(),
        Sha256Digest::of(canonical.as_bytes()),
    )
}

/// The compatibility decision, materialized locally.
///
/// PR1 has no registry, so there is no record to look up; what there is, is a
/// decision the host made, and this digests it so the model can be tied back to
/// it. The digest covers the exact selection key when the identity cleared the
/// FullAOT gate, and the rejection reason when it did not, so a model produced
/// under a different decision has a different binding.
fn compatibility_binding(bundle: &SnapshotBundle) -> CompatibilityBinding {
    let (profile_id, profile_sha256) = profile_binding(bundle.dart_profile.as_ref());
    let decision = match bundle.identity.exact_selection_key() {
        Ok(key) => format!(
            "exact;hash={};arch={};features={}",
            key.hash,
            key.target_arch.as_str(),
            key.features.join(",")
        ),
        Err(rejection) => format!("inexact;{}", rejection),
    };
    let record = format!(
        "family={};profile={};decision={}",
        PARSER_FAMILY_ID, profile_id, decision
    );
    CompatibilityBinding {
        record_sha256: Sha256Digest::of(record.as_bytes()),
        parser_family_id: PARSER_FAMILY_ID.to_string(),
        profile_id,
        profile_sha256,
    }
}

/// Who the host is about to run, digest included.
///
/// The digest is of the artifact on disk, taken immediately before the spawn, so
/// the model's producer record names the bytes that actually executed rather
/// than whatever the manifest says is installed.
fn producer_for(exec_path: &Path, version: Option<&str>, bundle: &SnapshotBundle) -> Result<Producer> {
    let bytes = fs::read(exec_path)
        .with_context(|| format!("read adapter artifact: {}", exec_path.display()))?;
    Ok(Producer {
        id: PARSER_FAMILY_ID.to_string(),
        version: version.unwrap_or("unknown").to_string(),
        artifact_sha256: Sha256Digest::of(&bytes),
        trust: flutterdec_adapter::local_producer_trust(&bundle.identity),
    })
}

fn requested_backend(backend: AdapterBackend) -> RequestedBackend {
    match backend {
        AdapterBackend::Auto => RequestedBackend::Auto,
        AdapterBackend::Internal => RequestedBackend::Fixed(BackendId::Internal),
        AdapterBackend::Blutter => RequestedBackend::Fixed(BackendId::Blutter),
        AdapterBackend::R2Flutter => RequestedBackend::Fixed(BackendId::R2Flutter),
    }
}

/// The core-facing name for a backend the protocol resolved.
fn backend_from_id(id: BackendId) -> AdapterBackend {
    match id {
        BackendId::Internal => AdapterBackend::Internal,
        BackendId::Blutter => AdapterBackend::Blutter,
        BackendId::R2Flutter => AdapterBackend::R2Flutter,
    }
}

fn load_model(
    repo_root: &Path,
    bundle: &SnapshotBundle,
    backend: AdapterBackend,
) -> Result<LoadedModel> {
    let manifest = flutterdec_adapter::load_manifest(repo_root)?;
    let manifest_entry = manifest
        .entries
        .iter()
        .find(|entry| entry.snapshot_hash == bundle.snapshot_hash);
    let adapter_exec = resolve_adapter_exec(repo_root, &bundle.snapshot_hash)?;
    let producer = producer_for(
        &adapter_exec,
        manifest_entry.map(|entry| entry.version.as_str()),
        bundle,
    )?;
    let compatibility = compatibility_binding(bundle);

    let run = run_adapter(
        &adapter_exec,
        &AdapterInput {
            identity: &bundle.identity,
            producer: producer.clone(),
            compatibility: compatibility.clone(),
            regions: vec![
                AdapterRegionInput {
                    region: InputRegionName::VmData,
                    bytes: &bundle.vm_data,
                    virtual_address: None,
                },
                AdapterRegionInput {
                    region: InputRegionName::IsolateData,
                    bytes: &bundle.isolate_data,
                    virtual_address: None,
                },
                AdapterRegionInput {
                    region: InputRegionName::VmInstructions,
                    bytes: &bundle.vm_instr,
                    virtual_address: Some(bundle.vm_instr_va),
                },
                AdapterRegionInput {
                    region: InputRegionName::IsolateInstructions,
                    bytes: &bundle.isolate_instr,
                    virtual_address: Some(bundle.isolate_instr_va),
                },
            ],
            input_path: Some(&bundle.input_path),
            libapp_path: Some(&bundle.libapp_path),
            requested_backend: requested_backend(backend),
        },
    )?;

    Ok(LoadedModel {
        model: run.model,
        resolved_backend: run.resolved_backend,
        fallback_reason: run.fallback_reason,
        adapter_exec,
        producer,
        compatibility,
        manifest_entry_version: manifest_entry.map(|entry| entry.version.clone()),
        manifest_entry_adapter: manifest_entry.map(|entry| entry.adapter.clone()),
    })
}
