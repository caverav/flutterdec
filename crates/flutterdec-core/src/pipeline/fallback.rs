// Honest machine-code recovery for snapshots no adapter is authorized to parse.
//
// Nothing here deserializes a snapshot. The only evidence available is ARM64
// instruction bytes and the region table the loader already established, so the
// only facts this can produce are where code plausibly starts and how far it
// plausibly runs. Every one of those is a guess and is marked `heuristic`.
//
// Everything a snapshot parser would supply -- libraries, classes, the class
// hierarchy, function names, the original entry function, and the ObjectPool
// index space -- stays `unavailable` with a diagnostic naming why. The
// alternative is what v3 did: emit `package:app/main.dart`, `Global`, `main` and
// an ordinal pool, and let every consumer downstream treat them as recovered.

/// Why core recovered the program itself instead of running an adapter.
///
/// Closed and typed for the same reason [`FallbackReason`] is: an operator
/// deciding whether to go find a parser needs to know *which* of these it is,
/// and a free-text sentence cannot be matched on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CoreFallbackReason {
    /// The operator pinned `--adapter-backend internal`.
    InternalRequested,
    /// The snapshot identity may not authorize any adapter at all: it is not a
    /// FullAOT snapshot, or its hash did not come out of a header.
    IdentityRejected,
    /// The identity is exact and no compatibility record matches it.
    NoCompatibilityRecord,
    /// A record exists for the snapshot hash and none of them covers this
    /// snapshot: wrong target architecture, or a feature tuple no record was
    /// written for. A registry that is malformed, ambiguous, or carries a
    /// profile that does not verify is an installation failure, not this.
    CompatibilityUnsupported,
    /// A record authorizes an adapter and no verified artifact is installed.
    AdapterNotInstalled,
}

impl CoreFallbackReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InternalRequested => "internal_requested",
            Self::IdentityRejected => "identity_rejected",
            Self::NoCompatibilityRecord => "no_compatibility_record",
            Self::CompatibilityUnsupported => "compatibility_unsupported",
            Self::AdapterNotInstalled => "adapter_not_installed",
        }
    }

    /// One sentence an operator can act on, stable per variant.
    pub fn detail(self) -> &'static str {
        match self {
            Self::InternalRequested => {
                "the internal backend was requested, so no adapter was selected or executed"
            }
            Self::IdentityRejected => {
                "the snapshot identity may not authorize an adapter, so none was selected or executed"
            }
            Self::NoCompatibilityRecord => {
                "no compatibility record matches this snapshot identity, so no adapter was selected or executed"
            }
            Self::CompatibilityUnsupported => {
                "the compatibility record for this snapshot is not usable on this host, so no adapter was executed"
            }
            Self::AdapterNotInstalled => {
                "no verified adapter artifact is installed for the selected record, so none was executed"
            }
        }
    }
}

impl fmt::Display for CoreFallbackReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Largest span a single recovered candidate may claim.
///
/// Two neighbouring starts bound each other; the last one has nothing after it
/// but the end of the region, and a candidate that claims a megabyte of
/// unexamined bytes is not a function candidate, it is the rest of the file.
const MAX_CANDIDATE_SIZE: u64 = 0x8000;

/// A call target has to be reached this often before it counts as a start on its
/// own. One `bl` into the middle of a function is a tail call or a computed
/// offset as easily as it is an entry point.
const MIN_CALL_TARGET_HITS: usize = 2;

/// `bl <label>`: opcode `100101`, signed 26-bit word displacement.
fn decode_bl_target(pc: u64, word: u32) -> Option<u64> {
    if (word >> 26) != 0b100101 {
        return None;
    }
    let imm26 = word & 0x03FF_FFFF;
    // Sign-extend to 32 bits, then scale by 4. Wrapping is correct here: the
    // displacement is a signed offset and a target outside the region is
    // filtered by the caller.
    let signed = ((imm26 << 6) as i32 >> 6) as i64 * 4;
    pc.checked_add_signed(signed)
}

/// `stp x29, x30, [sp, ...]`: the AArch64 frame-record store every non-leaf
/// Dart function opens with.
///
/// Matched on the register triple rather than on a byte pattern so pre-index,
/// post-index and signed-offset encodings all count.
fn is_frame_prologue(word: u32) -> bool {
    let rt = word & 0x1F;
    let rn = (word >> 5) & 0x1F;
    let rt2 = (word >> 10) & 0x1F;
    let is_store_pair_64 = (word >> 30) & 0x3 == 0b10;
    is_store_pair_64 && rt == 29 && rt2 == 30 && rn == 31
}

/// Code ranges from frame prologues and call targets.
///
/// Returned records carry no name and no owner: there is no name evidence in a
/// prologue, and `sub_1234` was never one. Each is `heuristic`, in ascending
/// address order, with dense ids.
#[allow(clippy::chunks_exact_to_as_chunks)]
fn recover_code_candidates(instr: &[u8], base_va: u64) -> Vec<Function> {
    let Some(end_va) = base_va.checked_add(instr.len() as u64) else {
        return Vec::new();
    };
    let in_region = |va: u64| va >= base_va && va < end_va && (va - base_va).is_multiple_of(4);

    let mut prologues: BTreeSet<u64> = BTreeSet::new();
    let mut call_targets: BTreeMap<u64, usize> = BTreeMap::new();
    for (index, chunk) in instr.chunks_exact(4).enumerate() {
        let word = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        let pc = base_va + (index as u64 * 4);
        if let Some(target) = decode_bl_target(pc, word) {
            if in_region(target) {
                *call_targets.entry(target).or_insert(0) += 1;
            }
        }
        if is_frame_prologue(word) {
            prologues.insert(pc);
        }
    }

    let mut starts = prologues.clone();
    // The region's own base is a start: whatever is at offset zero begins there
    // whether or not it opens with a frame record.
    if !instr.is_empty() {
        starts.insert(base_va);
    }
    for (target, hits) in &call_targets {
        if prologues.contains(target) || *hits >= MIN_CALL_TARGET_HITS {
            starts.insert(*target);
        }
    }

    let ordered = starts.into_iter().collect::<Vec<_>>();
    let mut out = Vec::with_capacity(ordered.len());
    for (index, start) in ordered.iter().enumerate() {
        let next = ordered.get(index + 1).copied().unwrap_or(end_va);
        let size = (next - start).min(MAX_CANDIDATE_SIZE);
        if size == 0 {
            continue;
        }
        out.push(Function {
            id: FunctionId(out.len() as u32),
            name: None,
            owner: None,
            code: CodeRange {
                start_va: *start,
                size,
            },
            code_section_va: base_va,
            provenance: Provenance::Heuristic,
        });
    }
    out
}

/// The digest of the binary that is about to do the recovering.
///
/// This is a real producer with a real artifact, so its digest is the digest of
/// that artifact rather than a stand-in. Computed once: the executable does not
/// change under a running process, and hashing it per call would cost a full
/// read of the binary on every command.
fn core_artifact_digest() -> Result<Sha256Digest> {
    static DIGEST: OnceLock<std::result::Result<Sha256Digest, String>> = OnceLock::new();
    DIGEST
        .get_or_init(|| {
            let exe = std::env::current_exe().map_err(|err| err.to_string())?;
            let bytes = fs::read(&exe).map_err(|err| format!("{}: {}", exe.display(), err))?;
            Ok(Sha256Digest::of(&bytes))
        })
        .clone()
        .map_err(|err| anyhow!("hash the running flutterdec executable: {}", err))
}

/// Who produced a core-recovered model.
///
/// `Local` rather than `Registered`: no registry record authorized this, which
/// is the whole reason the recovery ran.
fn core_producer() -> Result<Producer> {
    Ok(Producer {
        id: "flutterdec-core-internal".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        artifact_sha256: core_artifact_digest()?,
        trust: ProducerTrust::Local,
    })
}

fn region_of(name: InputRegionName, bytes: &[u8], virtual_address: Option<u64>) -> InputRegion {
    InputRegion {
        region: name,
        size: bytes.len() as u64,
        sha256: Sha256Digest::of(bytes),
        virtual_address,
        executable: name.is_executable(),
    }
}

/// The four regions the loader carved, as the model's observed input.
fn observed_regions(bundle: &SnapshotBundle) -> Vec<InputRegion> {
    let mut regions = vec![
        region_of(InputRegionName::VmData, &bundle.vm_data, None),
        region_of(InputRegionName::IsolateData, &bundle.isolate_data, None),
        region_of(
            InputRegionName::VmInstructions,
            &bundle.vm_instr,
            Some(bundle.vm_instr_va),
        ),
        region_of(
            InputRegionName::IsolateInstructions,
            &bundle.isolate_instr,
            Some(bundle.isolate_instr_va),
        ),
    ];
    regions.sort_by_key(|region| region.region);
    regions
}

/// One diagnostic per domain that stayed unavailable, each naming the fallback.
fn fallback_diagnostics(reason: CoreFallbackReason, candidates: usize) -> Vec<Diagnostic> {
    let mut diagnostics = vec![
        Diagnostic::unavailable(
            Domain::Libraries,
            format!(
                "core recovery reads instruction bytes and never deserializes the snapshot, so no library table is reachable ({})",
                reason.detail()
            ),
        ),
        Diagnostic::unavailable(
            Domain::Classes,
            format!(
                "core recovery reads instruction bytes and never deserializes the snapshot, so no class table is reachable ({})",
                reason.detail()
            ),
        ),
        Diagnostic::unavailable(
            Domain::ClassRelationships,
            "no class table was recovered, so no superclass or interface edge exists to report"
                .to_string(),
        ),
        Diagnostic::unavailable(
            Domain::FunctionNames,
            "no function name is recoverable from instruction bytes alone, and no original entry function was identified"
                .to_string(),
        ),
        Diagnostic::unavailable(
            Domain::ObjectPool,
            "the ObjectPool lives in the serialized heap, which core recovery does not read"
                .to_string(),
        ),
        Diagnostic::unavailable(
            Domain::PoolIndexSpace,
            "no ObjectPool was recovered, so pool displacements in the disassembly resolve to nothing"
                .to_string(),
        ),
    ];
    diagnostics.push(if candidates == 0 {
        Diagnostic::unavailable(
            Domain::Functions,
            "no frame prologue or repeated call target was found in the isolate instructions"
                .to_string(),
        )
    } else {
        Diagnostic {
            code: DiagnosticCode::DomainHeuristicOnly,
            severity: DiagnosticSeverity::Warning,
            subject: Some(Domain::Functions.as_str().to_string()),
            message: format!(
                "{candidates} code ranges come from frame-prologue and call-target scanning, not from a snapshot parser, so each boundary is a guess"
            ),
        }
    });
    diagnostics.sort_by(|a, b| a.subject.cmp(&b.subject));
    diagnostics
}

/// Build the model core recovers for itself when no adapter is authorized.
///
/// Candidates come from the isolate instructions only. The VM instructions are
/// reported as an observed executable region but are not scanned: core
/// disassembles the isolate region, so a candidate outside it would be a
/// function record nothing can ever decompile.
fn core_recovered_model(
    bundle: &SnapshotBundle,
    reason: CoreFallbackReason,
) -> Result<ProgramModel> {
    // The scan decodes AArch64 words. Running it over anything else does not
    // fail, it invents boundaries: an x64 instruction stream contains plenty of
    // 32-bit words that read as a frame-record store. A snapshot built for
    // another target recovers nothing and says so.
    let is_arm64 = matches!(
        bundle.identity.target_arch,
        flutterdec_loader::identity::TargetArch::Arm64
    );
    let functions = if is_arm64 {
        recover_code_candidates(&bundle.isolate_instr, bundle.isolate_instr_va)
    } else {
        Vec::new()
    };
    let capabilities = Capabilities {
        libraries: CapabilityLevel::Unavailable,
        classes: CapabilityLevel::Unavailable,
        class_relationships: CapabilityLevel::Unavailable,
        functions: if functions.is_empty() {
            CapabilityLevel::Unavailable
        } else {
            CapabilityLevel::Partial
        },
        function_names: CapabilityLevel::Unavailable,
        object_pool: CapabilityLevel::Unavailable,
        pool_index_space: CapabilityLevel::Unavailable,
    };
    let mut diagnostics = fallback_diagnostics(reason, functions.len());
    if !is_arm64 {
        diagnostics.push(Diagnostic::unavailable(
            Domain::Functions,
            format!(
                "core recovery decodes AArch64 instructions and this snapshot targets {}",
                bundle.identity.target_arch
            ),
        ));
        diagnostics.sort_by(|a, b| a.subject.cmp(&b.subject));
    }
    let model = ProgramModel {
        model_version: flutterdec_adapter::model::MODEL_VERSION,
        producer: core_producer()?,
        input: ObservedInput {
            identity: bundle.identity.clone(),
            regions: observed_regions(bundle),
        },
        // No record selected this run. See `ProgramModel::compatibility`.
        compatibility: None,
        capabilities,
        libraries: Vec::new(),
        classes: Vec::new(),
        functions,
        object_pool: ObjectPool::unavailable(),
        diagnostics,
        extensions: Default::default(),
    };

    // The same check every adapter model goes through, against the same host
    // facts. A recovery pass that invented a name, a library or a pool entry
    // fails here rather than reaching the decompiler.
    validate::validate(
        &model,
        &validate::HostSelectedContext {
            identity: bundle.identity.clone(),
            producer: model.producer.clone(),
            compatibility: None,
            regions: observed_regions(bundle),
        },
    )
    .map_err(|err| anyhow!("core recovered an invalid model: {}", err))?;
    Ok(model)
}

#[cfg(test)]
#[path = "fallback_tests.rs"]
mod fallback_tests;
