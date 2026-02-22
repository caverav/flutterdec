#[derive(Debug, Clone)]
pub struct SymbolMapOptions {
    pub out_dir: PathBuf,
    pub include_branches: bool,
    pub nearest_max_distance: u64,
    pub require_exec_match: bool,
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
