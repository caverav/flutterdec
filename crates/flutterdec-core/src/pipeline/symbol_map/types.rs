#[derive(Debug, Clone)]
pub struct SymbolMapOptions {
    pub out_dir: PathBuf,
    pub include_branches: bool,
    pub nearest_max_distance: u64,
    pub require_exec_match: bool,
    pub local_cache_root: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolTargetSummary {
    pub target_va: u64,
    pub call_count: usize,
    pub match_kind: String,
    pub symbol_name: Option<String>,
    pub symbol_va: Option<u64>,
    pub symbol_offset: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SymbolMapReport {
    pub stripped_path: String,
    pub unstripped_path: String,
    pub arch: String,
    pub unstripped_symbol_count: usize,
    pub scanned_exec_section_count: usize,
    pub total_direct_calls: usize,
    pub unique_call_targets: usize,
    pub exact_symbol_hits: usize,
    pub nearest_symbol_hits: usize,
    pub unresolved_calls: usize,
    pub exec_layout_match: bool,
    pub exec_bytes_match: bool,
    pub notes: Vec<String>,
    pub report_path: String,
    pub targets_path: String,
    pub callsites_path: String,
    pub local_cache_manifest_path: Option<String>,
    pub local_cache_build_id: Option<String>,
    pub local_cache_flutter_version: Option<String>,
    pub local_cache_registered_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct LocalSymbolCacheManifest {
    #[serde(default)]
    entries: Vec<LocalSymbolCacheEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct LocalSymbolCacheEntry {
    arch: String,
    build_id: Option<String>,
    flutter_version: Option<String>,
    dart_version: Option<String>,
    build_id_target_summary_path: Option<String>,
    version_target_summary_path: Option<String>,
    report_path: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct LocalSymbolCacheResolution {
    match_kind: Option<String>,
    paths: Vec<PathBuf>,
    manifest_path: Option<PathBuf>,
    error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct LocalSymbolCacheRegistration {
    manifest_path: Option<PathBuf>,
    build_id: Option<String>,
    flutter_version: Option<String>,
    registered_paths: Vec<String>,
}

#[derive(Debug, Clone)]
struct ExecSection {
    name: String,
    addr: u64,
    offset: usize,
    size: usize,
}

#[derive(Debug, Clone)]
struct CallSite {
    section: String,
    call_va: u64,
    mnemonic: String,
    target_va: Option<u64>,
    match_kind: MatchKind,
    symbol_name: Option<String>,
    symbol_va: Option<u64>,
    symbol_offset: Option<i64>,
}

#[derive(Debug, Clone, Copy)]
enum MatchKind {
    Exact,
    Nearest,
    Unresolved,
}

impl MatchKind {
    fn as_str(self) -> &'static str {
        match self {
            MatchKind::Exact => "exact",
            MatchKind::Nearest => "nearest",
            MatchKind::Unresolved => "unresolved",
        }
    }
}

#[derive(Debug, Clone)]
struct ResolvedTarget {
    kind: MatchKind,
    symbol_name: Option<String>,
    symbol_va: Option<u64>,
    symbol_offset: Option<i64>,
}
