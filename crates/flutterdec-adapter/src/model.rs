//! ProgramModel v4: the only model contract the host accepts.
//!
//! v2 and v3 were shaped so that "recovered nothing" and "recovered everything"
//! serialized the same way. Every name was a required `String`, every reference
//! was a display string matched by equality, and there was no place to say how
//! well anything was known, so an adapter that could not read a snapshot still
//! had to emit *something* and the core could not tell what it was looking at.
//!
//! v4 fixes that structurally rather than by convention:
//!
//! * Unknown is representable. Names are `Option`, so an unnamed function is a
//!   function with no name rather than a function named `main`.
//! * Every recovered fact carries [`Provenance`], and per-domain
//!   [`Capabilities`] say whether the domain is complete, partial, or
//!   unavailable. A model that claims a complete domain and fills it with
//!   guesses contradicts itself and is rejected.
//! * References are typed ids, so they can be checked, rather than strings that
//!   silently fail to match.
//! * The model records who produced it, what it observed, and which
//!   compatibility record authorized it, so the host can verify that the adapter
//!   answered the question it was asked.
//!
//! Structural well-formedness is what this module owns. Semantic invariants that
//! need the host's own view of the world live in [`crate::validate`].

use crate::primitives::Sha256Digest;
use flutterdec_loader::identity::SnapshotIdentity;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fmt;

/// The only accepted model version. v2 and v3 are rejected, not migrated.
pub const MODEL_VERSION: u32 = 4;

/// How well a single recovered fact is known.
///
/// There is deliberately no `Unavailable` variant: a fact that is unavailable is
/// an absent record or a `None`, not a record carrying an "unknown" provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    /// Read from the snapshot by a parser that understands its layout.
    Exact,
    /// Computed from exact facts by a rule that cannot be wrong if they are not.
    Derived,
    /// A guess from pattern evidence. May be wrong, and callers must treat it so.
    Heuristic,
}

impl Provenance {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Derived => "derived",
            Self::Heuristic => "heuristic",
        }
    }

    /// Whether a fact with this provenance is allowed to carry a confidence
    /// score. Only guesses have confidence; a number attached to an exact fact
    /// is decoration that makes the model look calibrated when it is not.
    pub fn admits_confidence(self) -> bool {
        matches!(self, Self::Heuristic)
    }
}

/// How much of one domain the producer recovered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityLevel {
    /// Everything the snapshot contains for this domain is present and exact.
    Complete,
    /// Some of it is present. Absence proves nothing.
    Partial,
    /// None of it was recovered. The domain must be empty and say why.
    Unavailable,
}

impl CapabilityLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Partial => "partial",
            Self::Unavailable => "unavailable",
        }
    }
}

/// The domains a model reports capability for, named so validation can iterate
/// them rather than repeating the same check per field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Domain {
    Libraries,
    Classes,
    ClassRelationships,
    Functions,
    FunctionNames,
    ObjectPool,
    PoolIndexSpace,
}

impl Domain {
    pub const ALL: [Domain; 7] = [
        Domain::Libraries,
        Domain::Classes,
        Domain::ClassRelationships,
        Domain::Functions,
        Domain::FunctionNames,
        Domain::ObjectPool,
        Domain::PoolIndexSpace,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Libraries => "libraries",
            Self::Classes => "classes",
            Self::ClassRelationships => "class_relationships",
            Self::Functions => "functions",
            Self::FunctionNames => "function_names",
            Self::ObjectPool => "object_pool",
            Self::PoolIndexSpace => "pool_index_space",
        }
    }
}

impl fmt::Display for Domain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    pub libraries: CapabilityLevel,
    pub classes: CapabilityLevel,
    /// Superclass edges specifically: a producer can recover class names from
    /// strings without recovering the hierarchy between them.
    pub class_relationships: CapabilityLevel,
    pub functions: CapabilityLevel,
    /// Names specifically: code ranges are frequently recoverable when the names
    /// attached to them are not.
    pub function_names: CapabilityLevel,
    pub object_pool: CapabilityLevel,
    /// Whether pool indexes mean hardware displacements. Separate from
    /// `object_pool` because listing entries and knowing where they live are
    /// different achievements.
    pub pool_index_space: CapabilityLevel,
}

impl Capabilities {
    pub fn level(&self, domain: Domain) -> CapabilityLevel {
        match domain {
            Domain::Libraries => self.libraries,
            Domain::Classes => self.classes,
            Domain::ClassRelationships => self.class_relationships,
            Domain::Functions => self.functions,
            Domain::FunctionNames => self.function_names,
            Domain::ObjectPool => self.object_pool,
            Domain::PoolIndexSpace => self.pool_index_space,
        }
    }

    /// Nothing was recovered. The honest capability set for a snapshot no parser
    /// understands.
    pub fn all_unavailable() -> Self {
        Self {
            libraries: CapabilityLevel::Unavailable,
            classes: CapabilityLevel::Unavailable,
            class_relationships: CapabilityLevel::Unavailable,
            functions: CapabilityLevel::Unavailable,
            function_names: CapabilityLevel::Unavailable,
            object_pool: CapabilityLevel::Unavailable,
            pool_index_space: CapabilityLevel::Unavailable,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Error,
}

/// Closed set of reasons a model can give for what it did not do.
///
/// Closed because a free-text reason cannot be checked, and an unavailable
/// domain with an unparseable explanation is indistinguishable from one with no
/// explanation at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCode {
    /// The domain was not attempted, because the parser has no support for it.
    DomainUnsupported,
    /// The domain was attempted and nothing was recovered.
    DomainNotRecovered,
    /// The domain was attempted and only part of it was recovered.
    DomainPartiallyRecovered,
    /// Records in the domain are pattern guesses, not parser output.
    DomainHeuristicOnly,
    /// A region of the input could not be decoded.
    RegionNotDecoded,
    /// A record was dropped because it did not survive the producer's own checks.
    RecordDiscarded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Diagnostic {
    pub code: DiagnosticCode,
    pub severity: DiagnosticSeverity,
    /// The domain or region the diagnostic is about, when it is about one.
    pub subject: Option<String>,
    pub message: String,
}

impl Diagnostic {
    /// The diagnostic an unavailable domain is required to carry.
    pub fn unavailable(domain: Domain, message: impl Into<String>) -> Self {
        Self {
            code: DiagnosticCode::DomainNotRecovered,
            severity: DiagnosticSeverity::Warning,
            subject: Some(domain.to_string()),
            message: message.into(),
        }
    }
}

/// How far the host trusts the thing that produced the model.
///
/// Trust is host-assigned. An adapter that writes `registered` into its own
/// output has not become registered; validation compares this against what the
/// host selected and rejects the claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProducerTrust {
    /// Reached through a compatibility record in the registry.
    Registered,
    /// Locally installed and digest-verified, with no registry record.
    Local,
    /// Neither. Output is evidence, not authority.
    Untrusted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Producer {
    pub id: String,
    pub version: String,
    /// Digest of the artifact that ran, not of the sources it was built from.
    pub artifact_sha256: Sha256Digest,
    pub trust: ProducerTrust,
}

/// The named regions an Android ARM64 AOT snapshot is carved into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputRegionName {
    VmData,
    IsolateData,
    VmInstructions,
    IsolateInstructions,
}

impl InputRegionName {
    pub const ALL: [InputRegionName; 4] = [
        InputRegionName::VmData,
        InputRegionName::IsolateData,
        InputRegionName::VmInstructions,
        InputRegionName::IsolateInstructions,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::VmData => "vm_data",
            Self::IsolateData => "isolate_data",
            Self::VmInstructions => "vm_instructions",
            Self::IsolateInstructions => "isolate_instructions",
        }
    }

    /// Whether this region holds code. Only executable regions can contain a
    /// code range or be the target of a call.
    pub fn is_executable(self) -> bool {
        matches!(self, Self::VmInstructions | Self::IsolateInstructions)
    }
}

impl fmt::Display for InputRegionName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One region as the host read it, digest included.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputRegion {
    pub region: InputRegionName,
    pub size: u64,
    pub sha256: Sha256Digest,
    /// Load address. Present exactly for executable regions; a data region has
    /// no address space that a code range could live in.
    pub virtual_address: Option<u64>,
    pub executable: bool,
}

impl InputRegion {
    /// One past the last byte, or `None` if the region overflows the address
    /// space and is therefore not a region.
    pub fn end_va(&self) -> Option<u64> {
        self.virtual_address?.checked_add(self.size)
    }

    /// Whether `[start, start + size)` lies wholly inside this region.
    pub fn contains_range(&self, start: u64, size: u64) -> bool {
        let (Some(base), Some(region_end)) = (self.virtual_address, self.end_va()) else {
            return false;
        };
        let Some(end) = start.checked_add(size) else {
            return false;
        };
        start >= base && end <= region_end
    }

    pub fn contains_address(&self, address: u64) -> bool {
        self.contains_range(address, 1)
    }
}

/// What the producer says it was given, so the host can check it against what it
/// actually handed over.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedInput {
    pub identity: SnapshotIdentity,
    pub regions: Vec<InputRegion>,
}

impl ObservedInput {
    pub fn region(&self, name: InputRegionName) -> Option<&InputRegion> {
        self.regions.iter().find(|r| r.region == name)
    }

    pub fn executable_regions(&self) -> impl Iterator<Item = &InputRegion> {
        self.regions.iter().filter(|r| r.executable)
    }
}

/// The registry decision that authorized this run, echoed back for checking.
///
/// None of these are things an adapter may choose. They are recorded in the
/// model so that a model can be tied to the decision that produced it, and so a
/// model produced under a different decision is detectable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityBinding {
    /// Digest of the compatibility record the host selected.
    pub record_sha256: Sha256Digest,
    /// Which parser family the record pointed at.
    pub parser_family_id: String,
    pub profile_id: String,
    pub profile_sha256: Sha256Digest,
}

macro_rules! id_newtype {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub u32);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

id_newtype!(
    LibraryId,
    "Model-local library id. Distinct type from the other ids so a class cannot reference a function."
);
id_newtype!(ClassId, "Model-local class id.");
id_newtype!(FunctionId, "Model-local function id.");

/// A recovered name, with how it was recovered.
///
/// Wrapping the string is what makes `Option<Name>` mean "no name was
/// recovered" instead of forcing a placeholder into a required field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Name {
    pub text: String,
    pub provenance: Provenance,
    /// Only meaningful, and only permitted, for heuristic names.
    pub confidence: Option<f64>,
}

impl Name {
    pub fn exact(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            provenance: Provenance::Exact,
            confidence: None,
        }
    }
}

impl Function {
    /// The recovered name, if there is one. `None` is the honest answer for a
    /// code range nobody could put a name to.
    pub fn name_text(&self) -> Option<&str> {
        Some(self.name.as_ref()?.text.as_str())
    }
}

/// A half-open `[start_va, start_va + size)` span of code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodeRange {
    pub start_va: u64,
    pub size: u64,
}

impl CodeRange {
    /// `None` when the range runs off the end of the address space, which makes
    /// it not a range rather than a range ending at `u64::MAX`.
    pub fn end_va(&self) -> Option<u64> {
        self.start_va.checked_add(self.size)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Library {
    pub id: LibraryId,
    /// The library's URI, e.g. `dart:core`. Not a display string.
    pub uri: String,
    pub display_name: Option<String>,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Class {
    pub id: ClassId,
    pub name: String,
    /// `None` means the owning library was not recovered. Producers that read
    /// class names out of the snapshot without an attribution table land here,
    /// and forcing them to name a library is how a class ends up filed under an
    /// invented `package:app/main.dart`.
    pub library: Option<LibraryId>,
    /// `None` means no superclass edge was recovered, which is not the same as
    /// having no superclass.
    pub super_class: Option<ClassId>,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Function {
    pub id: FunctionId,
    /// `None` when no name was recovered. A function still has a code range.
    pub name: Option<Name>,
    /// `None` when the owning class was not recovered, or the function has none.
    pub owner: Option<ClassId>,
    pub code: CodeRange,
    /// Start address of the executable region the code lives in. Redundant with
    /// the region table by design: a mismatch means the producer and the host
    /// disagree about the address space.
    pub code_section_va: u64,
    /// Provenance of the code range, independently of the name's.
    pub provenance: Provenance,
}

/// What a pool entry's `index` counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PoolIndexSpace {
    /// Real `ObjectPool` entry indexes: `ldr xN, [x27, #disp]` resolves through
    /// the geometry. Requires geometry to be present.
    Hardware,
    /// Positions in the producer's own list. Carries no address meaning, so a
    /// pool reference in disassembly cannot be resolved through it.
    Ordinal,
}

/// Layout of the `ObjectPool` object `x27`/PP points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PoolGeometry {
    /// Byte offset of entry 0 from the PP base (0x10 on ARM64 AOT).
    pub entries_offset: u64,
    /// Stride between entries in bytes (8 on ARM64 AOT, even with compressed
    /// pointers).
    pub word_size: u64,
}

impl PoolGeometry {
    /// Convert a PP-relative byte displacement into a pool entry index.
    ///
    /// `None` for displacements below the first entry or off a stride boundary;
    /// those are pool-object header accesses, not entry loads.
    pub fn index_for_displacement(&self, displacement: u64) -> Option<u64> {
        if self.word_size == 0 {
            return None;
        }
        let rel = displacement.checked_sub(self.entries_offset)?;
        if rel % self.word_size != 0 {
            return None;
        }
        Some(rel / self.word_size)
    }

    /// The displacement entry `index` sits at, or `None` if it does not fit in
    /// the address space.
    pub fn displacement_for_index(&self, index: u64) -> Option<u64> {
        index
            .checked_mul(self.word_size)?
            .checked_add(self.entries_offset)
    }
}

/// What a pool entry holds. Closed, because an open string here is how "we did
/// not decode this" became a decoded kind in v3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PoolEntryKind {
    /// A Dart `String` object.
    String,
    /// A tagged Smi or other immediate.
    Immediate,
    /// A reference to code, with `target_va` set.
    Code,
    /// A field or offset reference.
    Field,
    /// A class reference.
    Class,
    /// A selector/name used for dynamic dispatch.
    Selector,
    /// A slot the producer read but did not decode. Not a guess.
    Undecoded,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PoolEntry {
    pub index: u64,
    pub kind: PoolEntryKind,
    /// Decoded value, when there is one. `None` for `Undecoded`.
    pub value: Option<String>,
    /// Set only for entries that reference code, and only to an address inside
    /// an executable region.
    pub target_va: Option<u64>,
    pub provenance: Provenance,
    /// Only permitted for heuristic entries.
    pub confidence: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectPool {
    pub index_space: PoolIndexSpace,
    /// Present exactly when `index_space` is `Hardware`.
    pub geometry: Option<PoolGeometry>,
    pub entries: Vec<PoolEntry>,
}

impl ObjectPool {
    /// The empty pool a producer that recovered nothing must emit.
    pub fn unavailable() -> Self {
        Self {
            index_space: PoolIndexSpace::Ordinal,
            geometry: None,
            entries: Vec::new(),
        }
    }
}

/// The one place a model may carry fields this contract does not define.
///
/// Everything else is `deny_unknown_fields`. Confining extension to a named
/// object is what makes "unknown key" a rejectable condition instead of a
/// silently ignored one, while still leaving room for a producer to attach data
/// the host is free to ignore.
pub type Extensions = BTreeMap<String, Value>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramModel {
    /// Must be [`MODEL_VERSION`]. Named `model_version` rather than
    /// `schema_version` so a v2/v3 document cannot deserialize by accident.
    pub model_version: u32,
    pub producer: Producer,
    pub input: ObservedInput,
    pub compatibility: CompatibilityBinding,
    pub capabilities: Capabilities,
    pub libraries: Vec<Library>,
    pub classes: Vec<Class>,
    pub functions: Vec<Function>,
    pub object_pool: ObjectPool,
    pub diagnostics: Vec<Diagnostic>,
    #[serde(default)]
    pub extensions: Extensions,
}

/// Why a document is not a v4 model.
#[derive(Debug)]
pub enum ModelParseError {
    /// Not JSON, or not a JSON object.
    NotAnObject(serde_json::Error),
    /// A `schema_version` field: this is a v2 or v3 document.
    LegacyModel(u64),
    /// A `model_version` this build does not accept.
    UnsupportedVersion(u64),
    /// No version field at all.
    MissingVersion,
    /// Right version, wrong shape.
    Malformed(serde_json::Error),
}

impl fmt::Display for ModelParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAnObject(err) => write!(f, "adapter output is not a JSON object: {}", err),
            Self::LegacyModel(version) => write!(
                f,
                "adapter output is a legacy schema_version {} model; ProgramModel v{} is the only accepted contract and there is no compatibility shim",
                version, MODEL_VERSION
            ),
            Self::UnsupportedVersion(version) => write!(
                f,
                "unsupported model_version {}; expected {}",
                version, MODEL_VERSION
            ),
            Self::MissingVersion => write!(
                f,
                "adapter output has no model_version field; expected {}",
                MODEL_VERSION
            ),
            Self::Malformed(err) => write!(f, "adapter output is not a valid v4 model: {}", err),
        }
    }
}

impl std::error::Error for ModelParseError {}

impl ProgramModel {
    /// Parse fresh JSON as a v4 model.
    ///
    /// The version is read before the document is deserialized so that a v2/v3
    /// document is rejected as the wrong contract rather than as a pile of
    /// missing-field errors, which is the difference between an operator
    /// knowing to regenerate it and an operator guessing.
    pub fn from_json(bytes: &[u8]) -> Result<Self, ModelParseError> {
        let raw: Value = serde_json::from_slice(bytes).map_err(ModelParseError::NotAnObject)?;
        if let Some(legacy) = raw.get("schema_version").and_then(Value::as_u64) {
            return Err(ModelParseError::LegacyModel(legacy));
        }
        match raw.get("model_version").and_then(Value::as_u64) {
            Some(version) if version == u64::from(MODEL_VERSION) => {}
            Some(version) => return Err(ModelParseError::UnsupportedVersion(version)),
            None => return Err(ModelParseError::MissingVersion),
        }
        serde_json::from_value(raw).map_err(ModelParseError::Malformed)
    }

    /// Canonical bytes. Struct field order and `BTreeMap` extensions make this
    /// a function of the value alone, so equal models serialize equal.
    pub fn to_canonical_json(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("ProgramModel is always serializable")
    }

    pub fn library(&self, id: LibraryId) -> Option<&Library> {
        self.libraries.iter().find(|l| l.id == id)
    }

    pub fn class(&self, id: ClassId) -> Option<&Class> {
        self.classes.iter().find(|c| c.id == id)
    }

    pub fn function(&self, id: FunctionId) -> Option<&Function> {
        self.functions.iter().find(|f| f.id == id)
    }

    /// The URI of the library a class belongs to, when both are known.
    pub fn class_library_uri(&self, id: ClassId) -> Option<&str> {
        let class = self.class(id)?;
        Some(self.library(class.library?)?.uri.as_str())
    }

    /// The owning class's name, or `None` when no owner was recovered.
    ///
    /// `None` rather than a stand-in: every consumer that used to read v3's
    /// required `owner_class` string got `"Global"` for both "top level" and
    /// "we did not find out", and could not tell the two apart.
    pub fn owner_name(&self, function: &Function) -> Option<&str> {
        Some(self.class(function.owner?)?.name.as_str())
    }

    /// The URI of the library the function's owning class belongs to.
    pub fn owner_library_uri(&self, function: &Function) -> Option<&str> {
        self.class_library_uri(function.owner?)
    }
}

fn level_enum() -> Value {
    json!({ "type": "string", "enum": ["complete", "partial", "unavailable"] })
}

fn provenance_enum() -> Value {
    json!({ "type": "string", "enum": ["exact", "derived", "heuristic"] })
}

fn digest_schema() -> Value {
    json!({ "type": "string", "pattern": "^[0-9a-f]{64}$" })
}

fn u64_schema() -> Value {
    json!({ "type": "integer", "minimum": 0, "maximum": 18446744073709551615u64 })
}

fn nullable_u64_schema() -> Value {
    json!({
        "type": ["integer", "null"],
        "minimum": 0,
        "maximum": 18446744073709551615u64
    })
}

fn confidence_schema() -> Value {
    json!({ "type": ["number", "null"], "minimum": 0.0, "maximum": 1.0 })
}

fn object(properties: Value, required: Vec<&str>) -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": required,
        "properties": properties,
    })
}

fn identity_schema() -> Value {
    object(
        json!({
            "hash": { "type": ["string", "null"], "pattern": "^[0-9a-f]{32}$" },
            "hash_source": { "type": "string", "enum": ["header", "scan", "unavailable"] },
            "kind": {
                "type": ["string", "null"],
                "enum": ["full", "full_core", "full_jit", "full_aot", "unrecognized", null]
            },
            "target_arch": {
                "oneOf": [
                    { "type": "string", "enum": ["arm64"] },
                    object(json!({ "unsupported": { "type": "string" } }), vec!["unsupported"])
                ]
            },
            "features": object(
                json!({
                    "raw": { "type": ["string", "null"] },
                    "normalized": { "type": "array", "items": { "type": "string" } }
                }),
                vec!["raw", "normalized"],
            ),
            "pointer_compression": {
                "type": "string",
                "enum": ["compressed", "uncompressed", "unavailable", "conflicting"]
            }
        }),
        vec![
            "hash",
            "hash_source",
            "kind",
            "target_arch",
            "features",
            "pointer_compression",
        ],
    )
}

/// The closed JSON Schema for [`ProgramModel`].
///
/// Hand-built rather than derived, because the alternative is a schema-generator
/// dependency for one document. `schema_matches_rust_types` in this module's
/// tests walks a maximal model against this schema in both directions, so a
/// field added to a struct without a matching property here fails the build
/// rather than shipping a schema that quietly describes an older model.
pub fn schema() -> Value {
    let library = object(
        json!({
            "id": { "type": "integer", "minimum": 0, "maximum": 4294967295u32 },
            "uri": { "type": "string", "minLength": 1 },
            "display_name": { "type": ["string", "null"] },
            "provenance": provenance_enum(),
        }),
        vec!["id", "uri", "display_name", "provenance"],
    );
    let class = object(
        json!({
            "id": { "type": "integer", "minimum": 0, "maximum": 4294967295u32 },
            "name": { "type": "string", "minLength": 1 },
            "library": { "type": ["integer", "null"], "minimum": 0, "maximum": 4294967295u32 },
            "super_class": { "type": ["integer", "null"], "minimum": 0, "maximum": 4294967295u32 },
            "provenance": provenance_enum(),
        }),
        vec!["id", "name", "library", "super_class", "provenance"],
    );
    let name = object(
        json!({
            "text": { "type": "string", "minLength": 1 },
            "provenance": provenance_enum(),
            "confidence": confidence_schema(),
        }),
        vec!["text", "provenance", "confidence"],
    );
    let code_range = object(
        json!({ "start_va": u64_schema(), "size": u64_schema() }),
        vec!["start_va", "size"],
    );
    let function = object(
        json!({
            "id": { "type": "integer", "minimum": 0, "maximum": 4294967295u32 },
            "name": { "oneOf": [name, { "type": "null" }] },
            "owner": { "type": ["integer", "null"], "minimum": 0, "maximum": 4294967295u32 },
            "code": code_range,
            "code_section_va": u64_schema(),
            "provenance": provenance_enum(),
        }),
        vec![
            "id",
            "name",
            "owner",
            "code",
            "code_section_va",
            "provenance",
        ],
    );
    let geometry = object(
        json!({ "entries_offset": u64_schema(), "word_size": u64_schema() }),
        vec!["entries_offset", "word_size"],
    );
    let pool_entry = object(
        json!({
            "index": u64_schema(),
            "kind": {
                "type": "string",
                "enum": ["string", "immediate", "code", "field", "class", "selector", "undecoded"]
            },
            "value": { "type": ["string", "null"] },
            "target_va": nullable_u64_schema(),
            "provenance": provenance_enum(),
            "confidence": confidence_schema(),
        }),
        vec![
            "index",
            "kind",
            "value",
            "target_va",
            "provenance",
            "confidence",
        ],
    );
    let object_pool = object(
        json!({
            "index_space": { "type": "string", "enum": ["hardware", "ordinal"] },
            "geometry": { "oneOf": [geometry, { "type": "null" }] },
            "entries": { "type": "array", "items": pool_entry },
        }),
        vec!["index_space", "geometry", "entries"],
    );
    let input_region = object(
        json!({
            "region": {
                "type": "string",
                "enum": ["vm_data", "isolate_data", "vm_instructions", "isolate_instructions"]
            },
            "size": u64_schema(),
            "sha256": digest_schema(),
            "virtual_address": nullable_u64_schema(),
            "executable": { "type": "boolean" },
        }),
        vec!["region", "size", "sha256", "virtual_address", "executable"],
    );
    let diagnostic = object(
        json!({
            "code": {
                "type": "string",
                "enum": [
                    "domain_unsupported",
                    "domain_not_recovered",
                    "domain_partially_recovered",
                    "domain_heuristic_only",
                    "region_not_decoded",
                    "record_discarded"
                ]
            },
            "severity": { "type": "string", "enum": ["info", "warning", "error"] },
            "subject": { "type": ["string", "null"] },
            "message": { "type": "string" },
        }),
        vec!["code", "severity", "subject", "message"],
    );

    let mut root = object(
        json!({
            "model_version": { "type": "integer", "const": MODEL_VERSION },
            "producer": object(
                json!({
                    "id": { "type": "string", "minLength": 1 },
                    "version": { "type": "string", "minLength": 1 },
                    "artifact_sha256": digest_schema(),
                    "trust": { "type": "string", "enum": ["registered", "local", "untrusted"] },
                }),
                vec!["id", "version", "artifact_sha256", "trust"],
            ),
            "input": object(
                json!({
                    "identity": identity_schema(),
                    "regions": { "type": "array", "items": input_region },
                }),
                vec!["identity", "regions"],
            ),
            "compatibility": object(
                json!({
                    "record_sha256": digest_schema(),
                    "parser_family_id": { "type": "string", "minLength": 1 },
                    "profile_id": { "type": "string", "minLength": 1 },
                    "profile_sha256": digest_schema(),
                }),
                vec!["record_sha256", "parser_family_id", "profile_id", "profile_sha256"],
            ),
            "capabilities": object(
                json!({
                    "libraries": level_enum(),
                    "classes": level_enum(),
                    "class_relationships": level_enum(),
                    "functions": level_enum(),
                    "function_names": level_enum(),
                    "object_pool": level_enum(),
                    "pool_index_space": level_enum(),
                }),
                Domain::ALL.iter().map(|d| d.as_str()).collect(),
            ),
            "libraries": { "type": "array", "items": library },
            "classes": { "type": "array", "items": class },
            "functions": { "type": "array", "items": function },
            "object_pool": object_pool,
            "diagnostics": { "type": "array", "items": diagnostic },
            "extensions": {
                "description": "The only object in this schema that accepts undeclared keys. Hosts may ignore its contents; nothing in it carries authority.",
                "type": "object",
                "additionalProperties": true
            },
        }),
        vec![
            "model_version",
            "producer",
            "input",
            "compatibility",
            "capabilities",
            "libraries",
            "classes",
            "functions",
            "object_pool",
            "diagnostics",
        ],
    );
    let map = root.as_object_mut().expect("schema root is an object");
    map.insert(
        "$schema".to_string(),
        json!("https://json-schema.org/draft/2020-12/schema"),
    );
    map.insert("title".to_string(), json!("flutterdec ProgramModel v4"));
    root
}
