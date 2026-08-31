use flutterdec_loader::registry::{
    CompatibilityRecord, CompatibilityRegistry, RegistrySelection, ResolvedArtifact,
};

/// One program model plus everything the operator needs to know about where it
/// came from.
///
/// `compatibility`, `compatibility_record`, `profile`, `adapter_exec` and
/// `containment` are all `Option` together with `core_fallback`: either a
/// registry record authorized an adapter and all five describe that run, or
/// core recovered the program itself and `core_fallback` says why. Nothing here
/// is inferred from a filename or from a substring of adapter output.
#[derive(Debug, Clone)]
struct LoadedProgram {
    model: ProgramModel,
    /// The backend the protocol result named, or `Internal` when core recovered
    /// the program itself.
    resolved_backend: BackendId,
    /// Why the producer used a backend other than the one `auto` prefers. Only
    /// an adapter run can report this.
    fallback_reason: Option<FallbackReason>,
    /// Why no adapter ran at all. `Some` means zero adapter processes existed.
    core_fallback: Option<CoreFallbackReason>,
    /// The condition behind `core_fallback`, verbatim, when there was one to
    /// quote.
    core_fallback_detail: Option<String>,
    /// Which containment controls were established for the adapter child.
    containment: Option<ContainmentReport>,
    adapter_exec: Option<PathBuf>,
    producer: Producer,
    compatibility: Option<CompatibilityBinding>,
    compatibility_record: Option<CompatibilityRecord>,
    profile: Option<ResolvedDartProfile>,
}

impl LoadedProgram {
    fn registry_record_present(&self) -> bool {
        self.compatibility_record.is_some()
    }
}

/// What every operator surface reports about a loaded program.
///
/// Built once, from host facts and the protocol result, so `info`, the
/// decompile report and both sides of a `diff` cannot describe the same run
/// differently.
fn provider_report(
    loaded: &LoadedProgram,
    bundle: &SnapshotBundle,
    requested: AdapterBackend,
) -> ProviderReport {
    let resolved = backend_from_id(loaded.resolved_backend);
    let backend_mismatch = match requested {
        AdapterBackend::Auto => false,
        _ => resolved != requested,
    };
    let identity_rejection = bundle.identity.exact_selection_key().err();
    let record = loaded.compatibility_record.as_ref();
    ProviderReport {
        requested_backend: requested.as_str().to_string(),
        resolved_backend: resolved.as_str().to_string(),
        backend_mismatch,
        backend_fallback_reason: loaded
            .fallback_reason
            .map(|reason| reason.as_str().to_string()),
        core_fallback_reason: loaded
            .core_fallback
            .map(|reason| reason.as_str().to_string()),
        core_fallback_detail: loaded.core_fallback_detail.clone(),
        core_fallback_effect: loaded.core_fallback.map(|_| CORE_FALLBACK_EFFECT.to_string()),
        adapter_executed: loaded.core_fallback.is_none(),
        adapter_exec_path: loaded
            .adapter_exec
            .as_ref()
            .map(|path| path.display().to_string()),
        producer_id: loaded.producer.id.clone(),
        producer_version: loaded.producer.version.clone(),
        producer_artifact_sha256: loaded.producer.artifact_sha256.to_string(),
        producer_trust: producer_trust_label(loaded.producer.trust).to_string(),
        registry_record_present: loaded.registry_record_present(),
        compatibility_record_sha256: loaded
            .compatibility
            .as_ref()
            .map(|binding| binding.record_sha256.to_string()),
        parser_family_id: loaded
            .compatibility
            .as_ref()
            .map(|binding| binding.parser_family_id.clone()),
        profile_id: loaded
            .compatibility
            .as_ref()
            .map(|binding| binding.profile_id.clone()),
        profile_sha256: loaded
            .compatibility
            .as_ref()
            .map(|binding| binding.profile_sha256.to_string()),
        artifact_id: record.map(|record| record.artifact.id.clone()),
        // The digest of the bytes that ran, re-read and compared against the
        // record immediately before the spawn.
        artifact_sha256: loaded
            .adapter_exec
            .as_ref()
            .map(|_| loaded.producer.artifact_sha256.to_string()),
        host_os: std::env::consts::OS.to_string(),
        host_arch: std::env::consts::ARCH.to_string(),
        target_arch: bundle.identity.target_arch.to_string(),
        snapshot_identity_is_exact: bundle.identity.is_exact(),
        identity_rejection: identity_rejection.as_ref().map(ToString::to_string),
        capabilities: capability_map(&loaded.model.capabilities),
        containment: loaded.containment.clone(),
        warnings: collect_compatibility_warnings(
            loaded.registry_record_present(),
            bundle.identity.is_exact(),
            backend_mismatch,
            loaded.core_fallback,
        ),
    }
}

/// What a core-recovered model does not contain, in one stable sentence.
///
/// Stated once and reused so that every surface says the same thing, and so an
/// operator reading only the JSON knows what is missing without inferring it
/// from a capability map.
const CORE_FALLBACK_EFFECT: &str = "core recovered heuristic ARM64 code candidates only: \
     no libraries, no classes, no function names, no original entry function, and no \
     authoritative ObjectPool index space";

fn registry_error(error: RegistryError) -> anyhow::Error {
    anyhow!("compatibility registry selection failed: {}", error)
}

/// Keep the typed host refusal downcastable, and the identity rejection inside
/// it downcastable too, so a caller can still tell which check stopped the run.
fn adapter_error(error: HostError) -> anyhow::Error {
    anyhow::Error::new(error).context("adapter invocation refused")
}

/// Select a record only after the identity's FullAOT/header gate passes.
fn select_registry(layout: &Layout, bundle: &SnapshotBundle) -> Result<RegistrySelection> {
    let registry = CompatibilityRegistry::load(&layout.registry_path()).map_err(registry_error)?;
    registry.select(&bundle.identity).map_err(registry_error)
}

/// The compatibility binding comes entirely from the selected registry record.
fn compatibility_binding(
    selection: &RegistrySelection,
    profile: &ResolvedDartProfile,
) -> Result<CompatibilityBinding> {
    let record_sha256 = Sha256Digest::parse(&selection.record_sha256().map_err(registry_error)?)
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

/// Whether the operator pinned a backend that only an external tool can serve.
///
/// Such a request may fail; it may not be quietly answered by something else.
/// `--adapter-backend r2flutter` means "exact names or nothing", and handing
/// back heuristic prologue scanning under that flag is the substitution the
/// protocol refuses inside an adapter run, done one layer up.
fn pins_external_backend(backend: AdapterBackend) -> bool {
    matches!(
        backend,
        AdapterBackend::Blutter | AdapterBackend::R2Flutter
    )
}

/// Whether a registry refusal is a fact about *this snapshot* rather than about
/// the host's own installed data.
///
/// "No record for this hash", "no record for this target", "no record for this
/// feature tuple" and "two records claim it" all mean the same thing to an
/// operator: nothing here parses this snapshot, so core will do what it can. A
/// malformed registry, a record that fails its own invariants, a profile that
/// does not verify, or an artifact whose bytes are not the authorized bytes are
/// integrity failures of the installation and must stay loud.
fn fallback_reason_for_registry(error: &RegistryError) -> Option<(CoreFallbackReason, String)> {
    let reason = match error {
        RegistryError::NoRecord(_) => CoreFallbackReason::NoCompatibilityRecord,
        RegistryError::Identity(_) => CoreFallbackReason::IdentityRejected,
        RegistryError::TargetMismatch { .. }
        | RegistryError::FeatureMismatch { .. }
        | RegistryError::Ambiguous(_) => CoreFallbackReason::CompatibilityUnsupported,
        RegistryError::ArtifactAbsent(_) => CoreFallbackReason::AdapterNotInstalled,
        RegistryError::Malformed(_)
        | RegistryError::UnsupportedVersion(_)
        | RegistryError::InvalidRecord(_)
        | RegistryError::Profile(_)
        | RegistryError::Artifact(_) => return None,
    };
    Some((reason, error.to_string()))
}

/// Recover the program with core's own ARM64 scanning, having executed nothing.
///
/// `record` is the record the registry did select, when one was selected and the
/// run stopped after it. "A record exists and its artifact is not installed" and
/// "no record exists" are different things to report, and an operator acts on
/// them differently.
fn load_core_fallback(
    bundle: &SnapshotBundle,
    reason: CoreFallbackReason,
    detail: Option<String>,
    record: Option<CompatibilityRecord>,
) -> Result<LoadedProgram> {
    let model = core_recovered_model(bundle, reason)?;
    Ok(LoadedProgram {
        producer: model.producer.clone(),
        model,
        // The internal backend is what ran, and it ran here rather than inside a
        // producer. `fallback_reason` stays `None`: it describes a producer
        // choosing between backends, and no producer was consulted.
        resolved_backend: BackendId::Internal,
        fallback_reason: None,
        core_fallback: Some(reason),
        core_fallback_detail: detail,
        containment: None,
        adapter_exec: None,
        // No binding: nothing authorized this run, even where a record exists.
        compatibility: None,
        compatibility_record: record,
        profile: None,
    })
}

/// A refusal that names the deterministic reason a pinned external backend
/// could not be honored.
fn pinned_backend_refused(
    backend: AdapterBackend,
    reason: CoreFallbackReason,
    detail: &str,
) -> anyhow::Error {
    anyhow!(
        "--adapter-backend {} cannot be served for this snapshot ({}): {}. \
         core recovery is available with --adapter-backend internal, and reports \
         heuristic code candidates with no names, libraries, classes or ObjectPool",
        backend.as_str(),
        reason.as_str(),
        detail
    )
}

/// Turn a registry refusal into core recovery, a pinned-backend refusal, or the
/// original error, whichever the refusal actually means.
fn recover_or_refuse(
    bundle: &SnapshotBundle,
    backend: AdapterBackend,
    error: RegistryError,
    record: Option<CompatibilityRecord>,
) -> Result<LoadedProgram> {
    let Some((reason, detail)) = fallback_reason_for_registry(&error) else {
        return Err(registry_error(error));
    };
    if pins_external_backend(backend) {
        return Err(pinned_backend_refused(backend, reason, &detail));
    }
    load_core_fallback(bundle, reason, Some(detail), record)
}

/// Decide what produces the model, then produce it.
///
/// The order is the contract. The identity gate runs before the registry is
/// read; an explicitly internal run never reads the registry at all; and every
/// path that cannot reach an authorized adapter returns a typed reason instead
/// of a wrapper, a stand-in producer or a guess.
fn load_program(
    layout: &Layout,
    bundle: &mut SnapshotBundle,
    backend: AdapterBackend,
) -> Result<LoadedProgram> {
    // Before the registry is read, before a path is resolved, before anything
    // is spawned.
    if let Err(rejection) = bundle.identity.exact_selection_key() {
        let detail = rejection.to_string();
        if pins_external_backend(backend) {
            return Err(pinned_backend_refused(
                backend,
                CoreFallbackReason::IdentityRejected,
                &detail,
            ));
        }
        return load_core_fallback(
            bundle,
            CoreFallbackReason::IdentityRejected,
            Some(detail),
            None,
        );
    }
    // An explicitly internal run selects nothing and executes nothing. Reading
    // the registry here would make "internal" mean "internal, once an adapter
    // has been authorized", which is not what the operator asked for.
    if backend == AdapterBackend::Internal {
        return load_core_fallback(bundle, CoreFallbackReason::InternalRequested, None, None);
    }

    // Both of these can refuse for a reason that is about the snapshot rather
    // than about the installation, and only those reasons reach core recovery.
    let registry =
        CompatibilityRegistry::load(&layout.registry_path()).map_err(registry_error)?;
    let selection = match registry.select(&bundle.identity) {
        Ok(selection) => selection,
        Err(error) => return recover_or_refuse(bundle, backend, error, None),
    };
    let artifact = match selection.resolve_current_artifact(layout.store_dir()) {
        Ok(artifact) => artifact,
        Err(error) => {
            return recover_or_refuse(bundle, backend, error, Some(selection.record().clone()))
        }
    };

    // The profile is loaded only once a real artifact is going to run: a
    // verified profile is part of authorizing that run, and core recovery uses
    // none, so a fallback must not depend on one loading.
    let profile = selection
        .load_profile(layout.data_dir())
        .map_err(registry_error)?;
    bundle.dart_profile = Some(profile.clone());
    let profile_path = layout.data_dir().join(&selection.record().profile.path);
    let producer = producer_for(&artifact.path, &selection, &artifact)?;
    let compatibility = compatibility_binding(&selection, &profile)?;

    // An APK member is not a path. A backend that opens `--libapp-path` needs a
    // real file, so the member is materialized into the private invocation
    // directory and the adapter is handed that instead of a zip entry name.
    let member = match &bundle.libapp_entry {
        Some(entry) => Some((
            entry.clone(),
            flutterdec_loader::read_apk_entry(&bundle.input_path, entry).with_context(|| {
                format!("materialize {} from {}", entry, bundle.input_path.display())
            })?,
        )),
        None => None,
    };
    let libapp = match &member {
        Some((name, bytes)) => LibappSource::Member { name, bytes },
        None => LibappSource::File(&bundle.libapp_path),
    };

    let run = run_adapter(
        &artifact.path,
        &AdapterInput {
            identity: &bundle.identity,
            authorization: HostAuthorization {
                record: selection.record(),
                variant: &artifact.variant,
                store_root: layout.store_dir(),
                profile_path: &profile_path,
            },
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
            libapp: Some(libapp),
            requested_backend: requested_backend(backend),
            limits: Limits::default(),
        },
    )
    // An adapter that was authorized, started, and then failed is a failure.
    // There is no fallback arm here on purpose: recovering quietly from a
    // timeout or a corrupt model would hide exactly the condition the operator
    // needs to see.
    .map_err(adapter_error)?;

    Ok(LoadedProgram {
        model: run.model,
        resolved_backend: run.resolved_backend,
        fallback_reason: run.fallback_reason,
        core_fallback: None,
        core_fallback_detail: None,
        containment: Some(run.containment),
        adapter_exec: Some(artifact.path),
        producer,
        compatibility: Some(compatibility),
        compatibility_record: Some(selection.record().clone()),
        profile: Some(profile),
    })
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod model_gate_tests;
