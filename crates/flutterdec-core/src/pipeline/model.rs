use flutterdec_loader::registry::{
    CompatibilityRecord, CompatibilityRegistry, RegistryError, RegistrySelection, ResolvedArtifact,
};
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
    compatibility_record: CompatibilityRecord,
    profile: ResolvedDartProfile,
}

fn registry_error(error: RegistryError) -> anyhow::Error {
    anyhow!("compatibility registry selection failed: {}", error)
}

/// Select a record only after the identity's FullAOT/header gate passes.
fn select_registry(layout: &Layout, bundle: &SnapshotBundle) -> Result<RegistrySelection> {
    let registry = CompatibilityRegistry::load(&layout.registry_path()).map_err(registry_error)?;
    registry
        .select(&bundle.identity)
        .map_err(registry_error)
}

/// Attach the verified runtime profile selected by the registry to a bundle.
///
/// `Ok(None)` is reserved for callers that choose not to attempt selection
/// (for example an `info` report for a non-FullAOT input); a selected record
/// with a bad profile is an error, never an unverified fallback.
fn attach_registry_profile(
    layout: &Layout,
    bundle: &mut SnapshotBundle,
) -> Result<Option<RegistrySelection>> {
    let selection = select_registry(layout, bundle)?;
    let profile = selection
        .load_profile(layout.data_dir())
        .map_err(registry_error)?;
    bundle.dart_profile = Some(profile);
    Ok(Some(selection))
}

/// The compatibility binding comes entirely from the selected registry record.
fn compatibility_binding(
    selection: &RegistrySelection,
    profile: &ResolvedDartProfile,
) -> Result<CompatibilityBinding> {
    let record_sha256 = Sha256Digest::parse(
        &selection
            .record_sha256()
            .map_err(registry_error)?,
    )
    .map_err(|err| anyhow!("registry record digest is invalid: {}", err))?;
    let profile_sha256 = Sha256Digest::parse(&profile.profile_sha256)
        .map_err(|err| anyhow!("registry profile digest is invalid: {}", err))?;
    Ok(CompatibilityBinding {
        record_sha256,
        parser_family_id: selection.record().parser_family.id.clone(),
        profile_id: selection.record().profile.id.clone(),
        profile_sha256,
    })
}

/// Who the host is about to run, digest included and checked against the
/// selected host artifact variant.
fn producer_for(
    exec_path: &Path,
    selection: &RegistrySelection,
    artifact: &ResolvedArtifact,
) -> Result<Producer> {
    let bytes = fs::read(exec_path)
        .with_context(|| format!("read adapter artifact: {}", exec_path.display()))?;
    let actual = Sha256Digest::of(&bytes);
    let expected = Sha256Digest::parse(&artifact.variant.sha256)
        .map_err(|err| anyhow!("registry artifact digest is invalid: {}", err))?;
    if actual != expected || bytes.len() as u64 != artifact.variant.size {
        bail!(
            "adapter artifact changed after registry verification: expected {} bytes with {}, got {} bytes with {}",
            artifact.variant.size,
            expected,
            bytes.len(),
            actual
        );
    }
    Ok(Producer {
        id: selection.record().parser_family.id.clone(),
        version: selection
            .record()
            .parser_family
            .version
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        artifact_sha256: actual,
        trust: ProducerTrust::Registered,
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

/// The pre-lookup identity gate, in the one place every adapter path goes
/// through.
fn require_exact_selection(bundle: &SnapshotBundle) -> Result<ExactSelectionKey> {
    bundle
        .identity
        .exact_selection_key()
        .map_err(flutterdec_adapter::identity_rejected)
}

fn load_model(
    layout: &Layout,
    bundle: &SnapshotBundle,
    backend: AdapterBackend,
) -> Result<LoadedModel> {
    // Before the registry is read, before a path is resolved, before anything
    // is spawned.
    require_exact_selection(bundle)?;
    let selection = select_registry(layout, bundle)?;
    // Profiles come out of the read-only package data; executables come out of
    // the writable store. Resolving both against one root is what made the
    // adapter store part of the source checkout.
    let profile = selection
        .load_profile(layout.data_dir())
        .map_err(registry_error)?;
    let artifact = selection
        .resolve_current_artifact(layout.store_dir())
        .map_err(registry_error)?;
    let producer = producer_for(&artifact.path, &selection, &artifact)?;
    let compatibility = compatibility_binding(&selection, &profile)?;

    let run = run_adapter(
        &artifact.path,
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
        adapter_exec: artifact.path,
        producer,
        compatibility,
        compatibility_record: selection.record().clone(),
        profile,
    })
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod model_gate_tests;
