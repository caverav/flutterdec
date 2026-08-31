//! Semantic validation of a [`ProgramModel`] against what the host actually did.
//!
//! Parsing proves a model is well-shaped. It does not prove the model is about
//! the snapshot the host loaded, that its references resolve, that its addresses
//! exist, or that its capability claims match its contents. Those are the
//! failures that produce confident wrong output rather than an error, so they
//! are checked here, once, before anything reaches core analysis.
//!
//! Two rules shape the checks:
//!
//! * **The host decides, the adapter reports.** Identity, producer, and
//!   compatibility are compared against [`HostSelectedContext`]. An adapter
//!   cannot promote itself, change which snapshot it was given, or claim a
//!   different compatibility record.
//! * **Every size and address is checked arithmetic.** A range that overflows
//!   `u64` is not a range that ends at `u64::MAX`; it is a rejection.

use crate::model::{
    CapabilityLevel, ClassId, CompatibilityBinding, Domain, InputRegion, InputRegionName,
    PoolEntryKind, PoolIndexSpace, Producer, ProgramModel, Provenance, MODEL_VERSION,
};
use flutterdec_loader::identity::SnapshotIdentity;
use std::collections::BTreeSet;
use std::fmt;

/// What the host selected and observed, which the model is checked against.
///
/// Built by the host from the loaded snapshot and the registry decision. None of
/// it comes from adapter output.
#[derive(Debug, Clone, PartialEq)]
pub struct HostSelectedContext {
    pub identity: SnapshotIdentity,
    pub producer: Producer,
    /// The registry decision the host acted on, or `None` when the host
    /// recovered the program itself and no record authorized anything.
    ///
    /// An adapter run always has one, so a model that answers an adapter request
    /// with `null` here fails the same equality check that catches a model
    /// claiming someone else's record.
    pub compatibility: Option<CompatibilityBinding>,
    pub regions: Vec<InputRegion>,
}

/// Strings that are an admission that nothing was recovered.
///
/// A model must leave an unknown name absent. Writing one of these into a
/// required field is how v3 turned "no name" into a name, and every consumer
/// downstream then treated it as one.
const PLACEHOLDER_NAMES: &[&str] = &[
    "",
    "-",
    "?",
    "??",
    "???",
    "n/a",
    "na",
    "none",
    "null",
    "nil",
    "todo",
    "tbd",
    "unknown",
    "<unknown>",
    "unnamed",
    "anonymous",
    "placeholder",
    "undefined",
];

fn is_placeholder(text: &str) -> bool {
    let normalized = text.trim().to_ascii_lowercase();
    PLACEHOLDER_NAMES.contains(&normalized.as_str())
}

/// The first invariant a model breaks.
///
/// One error rather than a list: every variant here means the model cannot be
/// used, so collecting more of them would only delay the same refusal.
#[derive(Debug, Clone, PartialEq)]
pub enum ValidationError {
    UnsupportedModelVersion(u32),

    /// A host-selected fact the model reported differently.
    HostFactMismatch {
        field: &'static str,
    },
    /// The model's region table does not match the regions the host provided.
    RegionMismatch {
        region: InputRegionName,
        field: &'static str,
    },
    MissingRegion(InputRegionName),
    UnexpectedRegion(InputRegionName),
    DuplicateRegion(InputRegionName),
    /// A region's executability contradicts what that region is.
    RegionExecutabilityMismatch(InputRegionName),
    /// An executable region without a load address, or a data region with one.
    RegionAddressMismatch(InputRegionName),
    /// A region that runs off the end of the address space.
    RegionOverflows(InputRegionName),

    EmptyField {
        field: &'static str,
    },
    /// A name that is an admission of ignorance rather than a name.
    PlaceholderName {
        field: &'static str,
        value: String,
    },

    /// A confidence score on a fact that is not a guess.
    ConfidenceWithoutHeuristicProvenance {
        field: &'static str,
        provenance: Provenance,
    },
    /// A confidence outside `[0, 1]`, or not a number.
    ConfidenceOutOfRange {
        field: &'static str,
        value: f64,
    },

    DuplicateLibraryId(u32),
    DuplicateClassId(u32),
    DuplicateFunctionId(u32),
    DuplicatePoolIndex(u64),

    /// Records not in canonical ascending order.
    NoncanonicalOrder {
        collection: &'static str,
    },

    MissingLibraryReference {
        class: u32,
        library: u32,
    },
    MissingSuperClassReference {
        class: u32,
        super_class: u32,
    },
    MissingOwnerReference {
        function: u32,
        owner: u32,
    },
    /// A superclass chain that returns to a class it already visited.
    SuperClassCycle {
        class: u32,
    },

    /// A capability level the model's own contents contradict.
    CapabilityContradiction {
        domain: Domain,
        detail: &'static str,
    },
    /// An unavailable domain with no diagnostic saying so.
    UnavailableDomainWithoutDiagnostic(Domain),

    /// A code range of zero length, which is not a range.
    EmptyCodeRange {
        function: u32,
    },
    /// A range or address whose arithmetic overflows `u64`.
    AddressOverflow {
        context: &'static str,
        id: u64,
    },
    /// A code range that is not inside any declared executable region.
    CodeRangeOutsideExecutableRegions {
        function: u32,
        start_va: u64,
        size: u64,
    },
    /// A `code_section_va` that is not the base of an executable region, or not
    /// the base of the region the range actually lies in.
    CodeSectionMismatch {
        function: u32,
        code_section_va: u64,
    },
    /// A pool target address outside every executable region.
    PoolTargetOutsideExecutableRegions {
        index: u64,
        target_va: u64,
    },
    /// A pool entry whose kind and payload disagree.
    PoolEntryShape {
        index: u64,
        detail: &'static str,
    },
    /// Geometry that cannot describe an `ObjectPool`.
    PoolGeometry {
        detail: &'static str,
    },
    /// A hardware index that does not map back into the address space.
    PoolIndexOutOfBounds {
        index: u64,
    },
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedModelVersion(v) => write!(
                f,
                "model_version {} is not the accepted contract version {}",
                v, MODEL_VERSION
            ),
            Self::HostFactMismatch { field } => write!(
                f,
                "model reports a different {} than the host selected; adapter output cannot change host-selected facts",
                field
            ),
            Self::RegionMismatch { region, field } => write!(
                f,
                "region {} reports a different {} than the host observed",
                region, field
            ),
            Self::MissingRegion(r) => write!(f, "model omits input region {}", r),
            Self::UnexpectedRegion(r) => write!(f, "model declares region {} the host did not provide", r),
            Self::DuplicateRegion(r) => write!(f, "input region {} appears more than once", r),
            Self::RegionExecutabilityMismatch(r) => write!(
                f,
                "region {} declares an executability that contradicts what it is",
                r
            ),
            Self::RegionAddressMismatch(r) => write!(
                f,
                "region {} must have a virtual address exactly when it is executable",
                r
            ),
            Self::RegionOverflows(r) => {
                write!(f, "region {} runs past the end of the address space", r)
            }
            Self::EmptyField { field } => write!(f, "{} must not be empty", field),
            Self::PlaceholderName { field, value } => write!(
                f,
                "{} is the placeholder {:?}; an unrecovered name must be absent, not filled in",
                field, value
            ),
            Self::ConfidenceWithoutHeuristicProvenance { field, provenance } => write!(
                f,
                "{} carries a confidence score but its provenance is {}; only heuristic facts may be scored",
                field,
                provenance.as_str()
            ),
            Self::ConfidenceOutOfRange { field, value } => {
                write!(f, "{} confidence {} is outside [0, 1]", field, value)
            }
            Self::DuplicateLibraryId(id) => write!(f, "duplicate library id {}", id),
            Self::DuplicateClassId(id) => write!(f, "duplicate class id {}", id),
            Self::DuplicateFunctionId(id) => write!(f, "duplicate function id {}", id),
            Self::DuplicatePoolIndex(i) => write!(f, "duplicate object pool index {}", i),
            Self::NoncanonicalOrder { collection } => write!(
                f,
                "{} are not in canonical ascending order",
                collection
            ),
            Self::MissingLibraryReference { class, library } => write!(
                f,
                "class {} references library {}, which the model does not define",
                class, library
            ),
            Self::MissingSuperClassReference { class, super_class } => write!(
                f,
                "class {} references superclass {}, which the model does not define",
                class, super_class
            ),
            Self::MissingOwnerReference { function, owner } => write!(
                f,
                "function {} references owner class {}, which the model does not define",
                function, owner
            ),
            Self::SuperClassCycle { class } => {
                write!(f, "class {} is in a superclass cycle", class)
            }
            Self::CapabilityContradiction { domain, detail } => write!(
                f,
                "capability for {} contradicts the model's contents: {}",
                domain, detail
            ),
            Self::UnavailableDomainWithoutDiagnostic(domain) => write!(
                f,
                "{} is unavailable but no diagnostic explains why",
                domain
            ),
            Self::EmptyCodeRange { function } => {
                write!(f, "function {} has a zero-length code range", function)
            }
            Self::AddressOverflow { context, id } => {
                write!(f, "{} {} overflows the address space", context, id)
            }
            Self::CodeRangeOutsideExecutableRegions {
                function,
                start_va,
                size,
            } => write!(
                f,
                "function {} code range [{:#x}, +{:#x}) is not inside any executable input region",
                function, start_va, size
            ),
            Self::CodeSectionMismatch {
                function,
                code_section_va,
            } => write!(
                f,
                "function {} declares code_section_va {:#x}, which is not the base of the executable region containing its code",
                function, code_section_va
            ),
            Self::PoolTargetOutsideExecutableRegions { index, target_va } => write!(
                f,
                "object pool entry {} targets {:#x}, which is outside every executable input region",
                index, target_va
            ),
            Self::PoolEntryShape { index, detail } => {
                write!(f, "object pool entry {}: {}", index, detail)
            }
            Self::PoolGeometry { detail } => write!(f, "object pool geometry: {}", detail),
            Self::PoolIndexOutOfBounds { index } => write!(
                f,
                "object pool index {} does not map to a displacement in the address space",
                index
            ),
        }
    }
}

impl std::error::Error for ValidationError {}

type Check = Result<(), ValidationError>;

/// Reject a model that cannot be trusted to describe the host's snapshot.
///
/// Checks run cheapest-and-most-fundamental first, so the reported error is the
/// most explanatory one rather than a downstream symptom of it.
pub fn validate(model: &ProgramModel, host: &HostSelectedContext) -> Check {
    if model.model_version != MODEL_VERSION {
        return Err(ValidationError::UnsupportedModelVersion(
            model.model_version,
        ));
    }
    check_regions(model)?;
    check_host_facts(model, host)?;
    check_strings(model)?;
    check_confidence(model)?;
    check_identity_and_order(model)?;
    check_references(model)?;
    check_capabilities(model)?;
    check_geometry(model)?;
    check_addresses(model)?;
    Ok(())
}

fn check_regions(model: &ProgramModel) -> Check {
    let mut seen = BTreeSet::new();
    for region in &model.input.regions {
        if !seen.insert(region.region) {
            return Err(ValidationError::DuplicateRegion(region.region));
        }
        if region.executable != region.region.is_executable() {
            return Err(ValidationError::RegionExecutabilityMismatch(region.region));
        }
        if region.executable != region.virtual_address.is_some() {
            return Err(ValidationError::RegionAddressMismatch(region.region));
        }
        if region.executable && region.end_va().is_none() {
            return Err(ValidationError::RegionOverflows(region.region));
        }
    }
    // Canonical order for the region table too, so two models describing the
    // same input serialize identically.
    if model
        .input
        .regions
        .windows(2)
        .any(|w| w[0].region >= w[1].region)
    {
        return Err(ValidationError::NoncanonicalOrder {
            collection: "input regions",
        });
    }
    Ok(())
}

fn check_host_facts(model: &ProgramModel, host: &HostSelectedContext) -> Check {
    let observed = &model.input.identity;
    let expected = &host.identity;
    let identity_fields: [(&'static str, bool); 6] = [
        ("snapshot hash", observed.hash == expected.hash),
        ("hash source", observed.hash_source == expected.hash_source),
        ("snapshot kind", observed.kind == expected.kind),
        (
            "target architecture",
            observed.target_arch == expected.target_arch,
        ),
        (
            "normalized features",
            observed.features.normalized == expected.features.normalized,
        ),
        (
            "pointer compression",
            observed.pointer_compression == expected.pointer_compression,
        ),
    ];
    for (field, matches) in identity_fields {
        if !matches {
            return Err(ValidationError::HostFactMismatch { field });
        }
    }

    let producer_fields: [(&'static str, bool); 4] = [
        ("producer id", model.producer.id == host.producer.id),
        (
            "producer version",
            model.producer.version == host.producer.version,
        ),
        (
            "producer artifact digest",
            model.producer.artifact_sha256 == host.producer.artifact_sha256,
        ),
        (
            "producer trust",
            model.producer.trust == host.producer.trust,
        ),
    ];
    for (field, matches) in producer_fields {
        if !matches {
            return Err(ValidationError::HostFactMismatch { field });
        }
    }

    // Presence is checked first and separately: a model that drops the binding
    // entirely is a different failure from one that carries someone else's, and
    // the two arms below cannot be expressed as one field comparison.
    let compatibility_fields: [(&'static str, bool); 4] =
        match (&model.compatibility, &host.compatibility) {
            (None, None) => [("", true); 4],
            (model_binding, host_binding) => {
                let (Some(model_binding), Some(host_binding)) = (model_binding, host_binding)
                else {
                    return Err(ValidationError::HostFactMismatch {
                        field: "compatibility binding presence",
                    });
                };
                [
                    (
                        "compatibility record digest",
                        model_binding.record_sha256 == host_binding.record_sha256,
                    ),
                    (
                        "parser family",
                        model_binding.parser_family_id == host_binding.parser_family_id,
                    ),
                    (
                        "profile id",
                        model_binding.profile_id == host_binding.profile_id,
                    ),
                    (
                        "profile digest",
                        model_binding.profile_sha256 == host_binding.profile_sha256,
                    ),
                ]
            }
        };
    for (field, matches) in compatibility_fields {
        if !matches {
            return Err(ValidationError::HostFactMismatch { field });
        }
    }

    for expected_region in &host.regions {
        let Some(observed_region) = model.input.region(expected_region.region) else {
            return Err(ValidationError::MissingRegion(expected_region.region));
        };
        let region = expected_region.region;
        if observed_region.size != expected_region.size {
            return Err(ValidationError::RegionMismatch {
                region,
                field: "size",
            });
        }
        if observed_region.sha256 != expected_region.sha256 {
            return Err(ValidationError::RegionMismatch {
                region,
                field: "sha-256 digest",
            });
        }
        if observed_region.virtual_address != expected_region.virtual_address {
            return Err(ValidationError::RegionMismatch {
                region,
                field: "virtual address",
            });
        }
    }
    for observed_region in &model.input.regions {
        if !host
            .regions
            .iter()
            .any(|r| r.region == observed_region.region)
        {
            return Err(ValidationError::UnexpectedRegion(observed_region.region));
        }
    }
    Ok(())
}

fn non_empty(value: &str, field: &'static str) -> Check {
    if value.trim().is_empty() {
        return Err(ValidationError::EmptyField { field });
    }
    Ok(())
}

fn not_placeholder(value: &str, field: &'static str) -> Check {
    non_empty(value, field)?;
    if is_placeholder(value) {
        return Err(ValidationError::PlaceholderName {
            field,
            value: value.to_string(),
        });
    }
    Ok(())
}

fn check_strings(model: &ProgramModel) -> Check {
    non_empty(&model.producer.id, "producer id")?;
    non_empty(&model.producer.version, "producer version")?;
    if let Some(compatibility) = &model.compatibility {
        non_empty(&compatibility.parser_family_id, "parser family id")?;
        non_empty(&compatibility.profile_id, "profile id")?;
    }
    for library in &model.libraries {
        not_placeholder(&library.uri, "library uri")?;
        if let Some(display) = &library.display_name {
            not_placeholder(display, "library display name")?;
        }
    }
    for class in &model.classes {
        not_placeholder(&class.name, "class name")?;
    }
    for function in &model.functions {
        if let Some(name) = &function.name {
            not_placeholder(&name.text, "function name")?;
        }
    }
    for entry in &model.object_pool.entries {
        if let Some(value) = &entry.value {
            not_placeholder(value, "object pool entry value")?;
        }
    }
    Ok(())
}

fn check_one_confidence(
    confidence: Option<f64>,
    provenance: Provenance,
    field: &'static str,
) -> Check {
    let Some(value) = confidence else {
        return Ok(());
    };
    if !provenance.admits_confidence() {
        return Err(ValidationError::ConfidenceWithoutHeuristicProvenance { field, provenance });
    }
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(ValidationError::ConfidenceOutOfRange { field, value });
    }
    Ok(())
}

fn check_confidence(model: &ProgramModel) -> Check {
    for function in &model.functions {
        if let Some(name) = &function.name {
            check_one_confidence(name.confidence, name.provenance, "function name")?;
        }
    }
    for entry in &model.object_pool.entries {
        check_one_confidence(entry.confidence, entry.provenance, "object pool entry")?;
    }
    Ok(())
}

fn check_identity_and_order(model: &ProgramModel) -> Check {
    let mut library_ids = BTreeSet::new();
    for library in &model.libraries {
        if !library_ids.insert(library.id) {
            return Err(ValidationError::DuplicateLibraryId(library.id.0));
        }
    }
    let mut class_ids = BTreeSet::new();
    for class in &model.classes {
        if !class_ids.insert(class.id) {
            return Err(ValidationError::DuplicateClassId(class.id.0));
        }
    }
    let mut function_ids = BTreeSet::new();
    for function in &model.functions {
        if !function_ids.insert(function.id) {
            return Err(ValidationError::DuplicateFunctionId(function.id.0));
        }
    }
    let mut indexes = BTreeSet::new();
    for entry in &model.object_pool.entries {
        if !indexes.insert(entry.index) {
            return Err(ValidationError::DuplicatePoolIndex(entry.index));
        }
    }

    // Duplicates are reported above, so anything still out of order here is an
    // ordering problem and gets its own error.
    if model.libraries.windows(2).any(|w| w[0].id > w[1].id) {
        return Err(ValidationError::NoncanonicalOrder {
            collection: "libraries",
        });
    }
    if model.classes.windows(2).any(|w| w[0].id > w[1].id) {
        return Err(ValidationError::NoncanonicalOrder {
            collection: "classes",
        });
    }
    if model.functions.windows(2).any(|w| w[0].id > w[1].id) {
        return Err(ValidationError::NoncanonicalOrder {
            collection: "functions",
        });
    }
    if model
        .object_pool
        .entries
        .windows(2)
        .any(|w| w[0].index > w[1].index)
    {
        return Err(ValidationError::NoncanonicalOrder {
            collection: "object pool entries",
        });
    }
    Ok(())
}

fn check_references(model: &ProgramModel) -> Check {
    for class in &model.classes {
        if let Some(library) = class.library {
            if model.library(library).is_none() {
                return Err(ValidationError::MissingLibraryReference {
                    class: class.id.0,
                    library: library.0,
                });
            }
        }
        if let Some(super_class) = class.super_class {
            if model.class(super_class).is_none() {
                return Err(ValidationError::MissingSuperClassReference {
                    class: class.id.0,
                    super_class: super_class.0,
                });
            }
        }
    }
    for function in &model.functions {
        if let Some(owner) = function.owner {
            if model.class(owner).is_none() {
                return Err(ValidationError::MissingOwnerReference {
                    function: function.id.0,
                    owner: owner.0,
                });
            }
        }
    }
    // A class that reaches itself through superclass edges is not a hierarchy.
    // Every reference already resolves, so the walk terminates.
    for class in &model.classes {
        let mut visited: BTreeSet<ClassId> = BTreeSet::new();
        visited.insert(class.id);
        let mut cursor = class.super_class;
        while let Some(next) = cursor {
            if !visited.insert(next) {
                return Err(ValidationError::SuperClassCycle { class: class.id.0 });
            }
            cursor = model.class(next).and_then(|c| c.super_class);
        }
    }
    Ok(())
}

fn contradiction(domain: Domain, detail: &'static str) -> ValidationError {
    ValidationError::CapabilityContradiction { domain, detail }
}

fn check_capabilities(model: &ProgramModel) -> Check {
    use CapabilityLevel::{Complete, Unavailable};
    let caps = &model.capabilities;
    let pool = &model.object_pool;

    if caps.libraries == Unavailable && !model.libraries.is_empty() {
        return Err(contradiction(Domain::Libraries, "libraries are present"));
    }
    if caps.libraries == Complete
        && model
            .libraries
            .iter()
            .any(|l| l.provenance == Provenance::Heuristic)
    {
        return Err(contradiction(
            Domain::Libraries,
            "a complete domain contains heuristic records",
        ));
    }

    if caps.classes == Unavailable && !model.classes.is_empty() {
        return Err(contradiction(Domain::Classes, "classes are present"));
    }
    if caps.classes == Complete
        && model
            .classes
            .iter()
            .any(|c| c.provenance == Provenance::Heuristic)
    {
        return Err(contradiction(
            Domain::Classes,
            "a complete domain contains heuristic records",
        ));
    }
    // Edges cannot exist without the nodes they connect.
    if caps.classes == Unavailable && caps.class_relationships != Unavailable {
        return Err(contradiction(
            Domain::ClassRelationships,
            "relationships are claimed while classes are unavailable",
        ));
    }
    if caps.class_relationships == Unavailable
        && model.classes.iter().any(|c| c.super_class.is_some())
    {
        return Err(contradiction(
            Domain::ClassRelationships,
            "superclass edges are present",
        ));
    }

    if caps.functions == Unavailable && !model.functions.is_empty() {
        return Err(contradiction(Domain::Functions, "functions are present"));
    }
    if caps.functions == Complete
        && model
            .functions
            .iter()
            .any(|f| f.provenance == Provenance::Heuristic)
    {
        return Err(contradiction(
            Domain::Functions,
            "a complete domain contains heuristic records",
        ));
    }
    if caps.functions == Unavailable && caps.function_names != Unavailable {
        return Err(contradiction(
            Domain::FunctionNames,
            "names are claimed while functions are unavailable",
        ));
    }
    if caps.function_names == Unavailable && model.functions.iter().any(|f| f.name.is_some()) {
        return Err(contradiction(Domain::FunctionNames, "names are present"));
    }
    if caps.function_names == Complete {
        if model.functions.iter().any(|f| f.name.is_none()) {
            return Err(contradiction(
                Domain::FunctionNames,
                "a complete domain leaves functions unnamed",
            ));
        }
        if model
            .functions
            .iter()
            .filter_map(|f| f.name.as_ref())
            .any(|n| n.provenance == Provenance::Heuristic)
        {
            return Err(contradiction(
                Domain::FunctionNames,
                "a complete domain contains heuristic names",
            ));
        }
    }

    if caps.object_pool == Unavailable && !pool.entries.is_empty() {
        return Err(contradiction(
            Domain::ObjectPool,
            "pool entries are present",
        ));
    }
    if caps.object_pool == Complete {
        if pool
            .entries
            .iter()
            .any(|e| e.provenance == Provenance::Heuristic)
        {
            return Err(contradiction(
                Domain::ObjectPool,
                "a complete domain contains heuristic entries",
            ));
        }
        if pool
            .entries
            .iter()
            .any(|e| e.kind == PoolEntryKind::Undecoded)
        {
            return Err(contradiction(
                Domain::ObjectPool,
                "a complete domain contains undecoded entries",
            ));
        }
    }
    if caps.object_pool == Unavailable && caps.pool_index_space != Unavailable {
        return Err(contradiction(
            Domain::PoolIndexSpace,
            "an index space is claimed while the pool is unavailable",
        ));
    }
    // The index space claim and the geometry have to agree: a hardware index is
    // meaningless without the layout that resolves it, and geometry alongside
    // ordinal indexes invites the core to resolve positions as displacements.
    match pool.index_space {
        PoolIndexSpace::Hardware => {
            if pool.geometry.is_none() {
                return Err(contradiction(
                    Domain::PoolIndexSpace,
                    "hardware indexes are claimed without pool geometry",
                ));
            }
            if caps.pool_index_space == Unavailable {
                return Err(contradiction(
                    Domain::PoolIndexSpace,
                    "hardware indexes are claimed while the index space is unavailable",
                ));
            }
        }
        PoolIndexSpace::Ordinal => {
            if pool.geometry.is_some() {
                return Err(contradiction(
                    Domain::PoolIndexSpace,
                    "geometry is present but indexes are only ordinal",
                ));
            }
            if caps.pool_index_space != Unavailable {
                return Err(contradiction(
                    Domain::PoolIndexSpace,
                    "ordinal indexes carry no address meaning, so the index space is unavailable",
                ));
            }
        }
    }

    // An unavailable domain has to say why. Silence is indistinguishable from a
    // producer that forgot to look.
    for domain in Domain::ALL {
        if caps.level(domain) != Unavailable {
            continue;
        }
        let explained = model
            .diagnostics
            .iter()
            .any(|d| d.subject.as_deref() == Some(domain.as_str()));
        if !explained {
            return Err(ValidationError::UnavailableDomainWithoutDiagnostic(domain));
        }
    }
    Ok(())
}

fn check_geometry(model: &ProgramModel) -> Check {
    let Some(geometry) = model.object_pool.geometry else {
        return Ok(());
    };
    // A stride has to be a power of two and no wider than a machine word; a
    // stride of zero would make every displacement resolve to entry zero.
    if geometry.word_size == 0 || geometry.word_size > 8 || !geometry.word_size.is_power_of_two() {
        return Err(ValidationError::PoolGeometry {
            detail: "word size must be a power of two no larger than 8",
        });
    }
    if geometry.entries_offset % geometry.word_size != 0 {
        return Err(ValidationError::PoolGeometry {
            detail: "entries offset must be a multiple of the word size",
        });
    }
    for entry in &model.object_pool.entries {
        if geometry.displacement_for_index(entry.index).is_none() {
            return Err(ValidationError::PoolIndexOutOfBounds { index: entry.index });
        }
    }
    Ok(())
}

fn check_addresses(model: &ProgramModel) -> Check {
    let executable: Vec<&InputRegion> = model.input.executable_regions().collect();

    for function in &model.functions {
        if function.code.size == 0 {
            return Err(ValidationError::EmptyCodeRange {
                function: function.id.0,
            });
        }
        if function.code.end_va().is_none() {
            return Err(ValidationError::AddressOverflow {
                context: "code range of function",
                id: u64::from(function.id.0),
            });
        }
        let contained =
            |r: &&InputRegion| r.contains_range(function.code.start_va, function.code.size);
        if !executable.iter().any(contained) {
            return Err(ValidationError::CodeRangeOutsideExecutableRegions {
                function: function.id.0,
                start_va: function.code.start_va,
                size: function.code.size,
            });
        }
        // The declared section base has to name the region the code is actually
        // in. Keying off the declared base rather than off whichever region
        // happens to match first keeps this answer independent of region order.
        let declared_holds_the_code = executable
            .iter()
            .filter(|r| r.virtual_address == Some(function.code_section_va))
            .any(contained);
        if !declared_holds_the_code {
            return Err(ValidationError::CodeSectionMismatch {
                function: function.id.0,
                code_section_va: function.code_section_va,
            });
        }
    }

    for entry in &model.object_pool.entries {
        match (entry.kind, entry.target_va, &entry.value) {
            (PoolEntryKind::Code, None, _) => {
                return Err(ValidationError::PoolEntryShape {
                    index: entry.index,
                    detail: "a code entry must carry the address it references",
                })
            }
            (PoolEntryKind::Undecoded, _, Some(_)) => {
                return Err(ValidationError::PoolEntryShape {
                    index: entry.index,
                    detail: "an undecoded entry must not carry a decoded value",
                })
            }
            (kind, Some(_), _)
                if !matches!(kind, PoolEntryKind::Code | PoolEntryKind::Selector) =>
            {
                return Err(ValidationError::PoolEntryShape {
                    index: entry.index,
                    detail: "only code and selector entries may reference an address",
                })
            }
            (kind, _, None) if kind != PoolEntryKind::Undecoded => {
                return Err(ValidationError::PoolEntryShape {
                    index: entry.index,
                    detail: "a decoded entry must carry a value",
                })
            }
            _ => {}
        }
        if let Some(target) = entry.target_va {
            if !executable.iter().any(|r| r.contains_address(target)) {
                return Err(ValidationError::PoolTargetOutsideExecutableRegions {
                    index: entry.index,
                    target_va: target,
                });
            }
        }
    }
    Ok(())
}
